//! Drawing tiles and the widgets inside them.

use dashboard_contract::rpc::LogLevel;
use dashboard_contract::{Action, Cell as ViewCell, Status, TileUpdate, UptimeCheck, Widget};
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, Wrap};

use crate::canvas::Canvas;
use crate::charts;
use crate::mouse::{Click, Hits};
use crate::theme::Palette;

pub fn draw_tile(f: &mut Canvas, p: &Palette, area: Rect, tile: &TileUpdate, hits: &mut Hits) {
    let mut title = vec![Span::raw(format!(" {} ", tile.tile_id))];
    if tile.stale {
        title.push(Span::styled("stale ", Style::new().fg(p.warn)));
    }
    if tile.demo {
        title.push(Span::styled("demo ", Style::new().fg(p.accent)));
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(p.status(tile.view.status)))
        .title(Line::from(title));
    let inner = block.inner(area);
    f.render_widget(block, area);
    // Added before the widgets' own targets, so links inside still win.
    hits.add(area, Click::Tile(tile.tile_id.clone()));

    if inner.height < 3 {
        f.render_widget(Paragraph::new(tile.view.summary.as_str()), inner);
    } else {
        draw_widget(f, p, inner, &tile.view.root, hits);
    }
}

fn draw_widget(f: &mut Canvas, p: &Palette, area: Rect, widget: &Widget, hits: &mut Hits) {
    match widget {
        Widget::Text { text, markdown } => {
            let lines: Vec<Line> = text
                .split('\n')
                .map(|l| {
                    if *markdown {
                        Line::from(inline_markdown(p, l))
                    } else {
                        Line::raw(l)
                    }
                })
                .collect();
            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
        }
        Widget::Stat {
            label,
            value,
            unit,
            delta,
            status,
        } => {
            let mut spans = vec![
                Span::styled(format!("{label} "), Style::new().fg(p.muted)),
                Span::styled(
                    value.clone(),
                    Style::new()
                        .add_modifier(Modifier::BOLD)
                        .fg(p.opt_status(*status)),
                ),
            ];
            if let Some(unit) = unit {
                spans.push(Span::raw(format!(" {unit}")));
            }
            if let Some(delta) = delta {
                spans.push(Span::styled(format!(" {delta}"), Style::new().fg(p.muted)));
            }
            f.render_widget(Paragraph::new(Line::from(spans)), area);
        }
        Widget::Gauge {
            label,
            percent,
            marker,
            thresholds,
            caption,
        } => {
            let color = match thresholds {
                Some(t) if *percent >= t.error => p.error,
                Some(t) if *percent >= t.warn => p.warn,
                _ => p.ok,
            };
            let spec = charts::GaugeSpec {
                label,
                percent: *percent,
                marker: *marker,
                caption: caption.as_deref(),
                color,
            };
            charts::draw_gauge(
                f,
                p,
                Rect {
                    height: area.height.min(1),
                    ..area
                },
                &spec,
            );
        }
        Widget::Series {
            label,
            unit,
            y_max,
            style,
            color,
            mirrored,
            format,
            series,
            ..
        } => {
            let spec = charts::SeriesSpec {
                label,
                unit: unit.as_deref(),
                y_max: *y_max,
                style: *style,
                color: *color,
                mirrored: *mirrored,
                format: *format,
                series,
            };
            charts::draw_series(f, p, area, &spec);
        }
        Widget::MeterGrid {
            label,
            meters,
            color,
        } => {
            charts::draw_meter_grid(f, p, area, label.as_deref(), meters, *color);
        }
        Widget::Bars {
            label,
            unit,
            format,
            max,
            bars,
        } => {
            let top = area.y + label.is_some() as u16;
            for i in 0..bars.len() as u16 {
                hits.item(
                    Rect {
                        y: top.saturating_add(i),
                        height: 1,
                        ..area
                    },
                    area,
                );
            }
            let spec = charts::BarsSpec {
                label: label.as_deref(),
                unit: unit.as_deref(),
                format: *format,
                max: *max,
                bars,
            };
            charts::draw_bars(f, p, area, &spec);
        }
        Widget::List { items, empty_text } => {
            if items.is_empty() {
                f.render_widget(
                    Paragraph::new(empty_text.as_deref().unwrap_or("nothing here")),
                    area,
                );
                return;
            }
            // Each item is a title line plus an optional subtitle line.
            let mut y = area.y;
            for item in items {
                let height = 1 + item.subtitle.is_some() as u16;
                let row = Rect { y, height, ..area };
                hits.item(row, area);
                if let Some(action) = &item.action {
                    hits.add(row.intersection(area), Click::Run(action.clone()));
                }
                y = y.saturating_add(height);
            }
            let items: Vec<ListItem> = items
                .iter()
                .map(|item| {
                    let mut spans = vec![
                        Span::styled("● ", Style::new().fg(p.opt_status(item.status))),
                        Span::raw(item.title.clone()),
                    ];
                    for tag in &item.tags {
                        spans.push(Span::styled(format!(" [{tag}]"), Style::new().fg(p.muted)));
                    }
                    let mut lines = vec![Line::from(spans)];
                    if let Some(sub) = &item.subtitle {
                        lines.push(Line::styled(format!("  {sub}"), Style::new().fg(p.muted)));
                    }
                    ListItem::new(lines)
                })
                .collect();
            f.render_widget(List::new(items), area);
        }
        Widget::Table { columns, rows } => {
            // One header line, then one line per row.
            for (i, row) in rows.iter().enumerate() {
                let line = Rect {
                    y: (area.y + 1).saturating_add(i as u16),
                    height: 1,
                    ..area
                };
                hits.item(line, area);
                if let Some(action) = &row.action {
                    hits.add(line.intersection(area), Click::Run(action.clone()));
                }
            }
            let header = Row::new(columns.iter().map(|c| Cell::from(c.label.clone())))
                .style(Style::new().add_modifier(Modifier::BOLD));
            let rows = rows.iter().map(|row| {
                Row::new(
                    columns
                        .iter()
                        .map(|c| Cell::from(cell_text(row.cells.get(&c.key)))),
                )
                .style(Style::new().fg(p.opt_status(row.status)))
            });
            let widths = vec![Constraint::Fill(1); columns.len()];
            f.render_widget(Table::new(rows, widths).header(header), area);
        }
        Widget::Log { label, lines } => {
            let mut area = area;
            if let Some(label) = label {
                f.render_widget(
                    Paragraph::new(label.as_str()).style(Style::new().fg(p.muted)),
                    Rect { height: 1, ..area },
                );
                area.y += 1;
                area.height = area.height.saturating_sub(1);
            }
            // Newest at the bottom; one row per line, cut at the edge.
            let shown = &lines[lines.len().saturating_sub(area.height as usize)..];
            let rows: Vec<Line> = shown
                .iter()
                .map(|l| {
                    let color = match l.level {
                        Some(LogLevel::Error) => p.error,
                        Some(LogLevel::Warn) => p.warn,
                        Some(LogLevel::Debug) => p.muted,
                        _ => ratatui::style::Color::Reset,
                    };
                    let mut spans = vec![];
                    if let Some(time) = &l.time {
                        spans.push(Span::styled(format!("{time} "), Style::new().fg(p.muted)));
                    }
                    spans.push(Span::styled(l.text.as_str(), Style::new().fg(color)));
                    Line::from(spans)
                })
                .collect();
            f.render_widget(Paragraph::new(rows), area);
        }
        Widget::Uptime { checks } => {
            let name_w = checks
                .iter()
                .map(|c| c.name.chars().count())
                .max()
                .unwrap_or(0)
                .min(20) as u16;
            for (i, check) in checks.iter().enumerate() {
                let row = Rect {
                    y: area.y.saturating_add(i as u16),
                    height: 1,
                    ..area
                };
                hits.item(row, area);
                if row.y >= area.bottom() {
                    continue;
                }
                if let Some(url) = &check.url {
                    let action = Action::OpenUrl { url: url.clone() };
                    hits.add(row, Click::Run(action));
                }
                f.render_widget(
                    Paragraph::new(uptime_line(p, check, name_w, area.width)),
                    row,
                );
            }
        }
        Widget::Row { children } if stack(children.len(), area.width) => {
            let constraints = children
                .iter()
                .map(|c| Constraint::Length(min_height(c, area.width)));
            let areas = Layout::vertical(constraints).split(area);
            for (child, a) in children.iter().zip(areas.iter()) {
                draw_widget(f, p, *a, child, hits);
            }
        }
        Widget::Row { children } => {
            let areas = Layout::horizontal(vec![Constraint::Fill(1); children.len()])
                .spacing(2)
                .split(area);
            for (child, a) in children.iter().zip(areas.iter()) {
                draw_widget(f, p, *a, child, hits);
            }
        }
        Widget::Column { children } => {
            let constraints = children.iter().map(|c| height_hint(c, area.width));
            // Spare room stays at the bottom rather than stretching the last child.
            let areas = Layout::vertical(constraints)
                .spacing(1)
                .flex(Flex::Start)
                .split(area);
            for (child, a) in children.iter().zip(areas.iter()) {
                draw_widget(f, p, *a, child, hits);
            }
        }
    }
}

/// `● name  ▌▌▌▌▌▌  99.9%  120ms`: the strip takes the room left over and is
/// dropped first when the tile is narrow. A failing check shows its error
/// instead of the time.
fn uptime_line<'a>(p: &Palette, c: &'a UptimeCheck, name_w: u16, width: u16) -> Line<'a> {
    let muted = Style::new().fg(p.muted);
    let percent = c.uptime.map(|u| {
        if u >= 100.0 {
            "100%".to_string()
        } else {
            format!("{u:.1}%")
        }
    });
    let last = if c.status == Status::Ok || c.detail.is_none() {
        c.latency_ms.map(|ms| {
            if ms < 1000.0 {
                format!("{ms:.0}ms")
            } else {
                format!("{:.1}s", ms / 1000.0)
            }
        })
    } else {
        c.detail.as_ref().map(|d| d.chars().take(24).collect())
    };
    let mut tail = String::new();
    if let Some(pc) = &percent {
        tail.push_str(&format!(" {pc:>6}"));
    }
    if let Some(last) = &last {
        tail.push_str(&format!("  {last}"));
    }
    let name: String = c.name.chars().take(name_w as usize).collect();
    let fixed = 2 + name_w + 2 + tail.chars().count() as u16;
    let strip_w = width.saturating_sub(fixed) as usize;

    let mut spans = vec![
        Span::styled("● ", Style::new().fg(p.status(c.status))),
        Span::raw(format!("{name:<w$}  ", w = name_w as usize)),
    ];
    if strip_w >= 4 {
        let shown = &c.history[c.history.len().saturating_sub(strip_w)..];
        // Pad on the left so the newest result always sits at the same spot.
        spans.push(Span::styled(
            "·".repeat(strip_w - shown.len()),
            Style::new().fg(p.muted),
        ));
        for s in shown {
            let color = if *s == Status::Unknown {
                p.muted
            } else {
                p.status(*s)
            };
            spans.push(Span::styled("▌", Style::new().fg(color)));
        }
    }
    if let Some(pc) = percent {
        spans.push(Span::raw(format!(" {pc:>6}")));
    }
    if let Some(last) = last {
        let style = if c.status == Status::Ok {
            muted
        } else {
            Style::new().fg(p.status(c.status))
        };
        spans.push(Span::styled(format!("  {last}"), style));
    }
    Line::from(spans)
}

/// Children of a row narrower than this are stacked instead.
const MIN_ROW_CHILD: u16 = 24;

/// Whether a row of `n` children is too cramped side by side at this width.
fn stack(n: usize, width: u16) -> bool {
    n > 1 && width / (n as u16) < MIN_ROW_CHILD
}

/// Rows a tile needs to be usable at this width, borders included.
pub fn tile_min_height(tile: &TileUpdate, width: u16) -> u16 {
    min_height(&tile.view.root, width.saturating_sub(2)) + 2
}

/// Rows a widget needs to be usable: fixed parts at full size, and enough of
/// the flexible parts (graphs, lists, tables, logs) to be worth looking at.
pub fn min_height(widget: &Widget, width: u16) -> u16 {
    let width = width.max(1);
    match widget {
        Widget::Text { text, .. } => text_rows(text, width).clamp(1, 6),
        Widget::Stat { .. } | Widget::Gauge { .. } => 1,
        // Value line plus a few rows of plot (4 dots each in braille).
        Widget::Series { mirrored, .. } => {
            if *mirrored {
                7
            } else {
                5
            }
        }
        Widget::MeterGrid { label, meters, .. } => {
            charts::meter_grid_height(width, meters.len(), label.is_some())
        }
        Widget::Bars { label, bars, .. } => bars.len().min(8) as u16 + label.is_some() as u16,
        Widget::List { items, .. } => {
            let shown: u16 = items
                .iter()
                .take(4)
                .map(|i| 1 + i.subtitle.is_some() as u16)
                .sum();
            shown.max(1)
        }
        Widget::Table { rows, .. } => 1 + rows.len().clamp(1, 5) as u16,
        Widget::Log { label, .. } => 5 + label.is_some() as u16,
        Widget::Uptime { checks } => checks.len().max(1) as u16,
        Widget::Row { children } if stack(children.len(), width) => {
            children.iter().map(|c| min_height(c, width)).sum()
        }
        Widget::Row { children } => {
            let n = children.len().max(1) as u16;
            let w = width.saturating_sub(2 * (n - 1)) / n;
            children.iter().map(|c| min_height(c, w)).max().unwrap_or(1)
        }
        Widget::Column { children } => {
            let gaps = children.len().saturating_sub(1) as u16;
            children.iter().map(|c| min_height(c, width)).sum::<u16>() + gaps
        }
    }
}

/// How much height a widget wants inside a column: text, lists and tables
/// take the lines they have, graphs and logs share what is left.
fn height_hint(widget: &Widget, width: u16) -> Constraint {
    match widget {
        Widget::Gauge { .. } | Widget::Stat { .. } => Constraint::Length(1),
        Widget::Text { text, .. } => Constraint::Length(text_rows(text, width).max(1)),
        Widget::List { items, .. } => Constraint::Length(
            items
                .iter()
                .map(|i| 1 + i.subtitle.is_some() as u16)
                .sum::<u16>()
                .max(1),
        ),
        Widget::Table { rows, .. } => Constraint::Length(1 + rows.len() as u16),
        Widget::Uptime { checks } => Constraint::Length(checks.len().max(1) as u16),
        Widget::MeterGrid { label, meters, .. } => Constraint::Length(charts::meter_grid_height(
            width,
            meters.len(),
            label.is_some(),
        )),
        Widget::Bars { label, bars, .. } => {
            Constraint::Length(bars.len() as u16 + label.is_some() as u16)
        }
        Widget::Row { children } if stack(children.len(), width) => {
            Constraint::Length(min_height(widget, width))
        }
        Widget::Row { children } => {
            let lengths: Option<Vec<u16>> = children
                .iter()
                .map(
                    |c| match height_hint(c, width / children.len().max(1) as u16) {
                        Constraint::Length(n) => Some(n),
                        _ => None,
                    },
                )
                .collect();
            match lengths {
                Some(l) => Constraint::Length(l.into_iter().max().unwrap_or(1)),
                None => Constraint::Fill(1),
            }
        }
        Widget::Series { .. } => Constraint::Min(4),
        _ => Constraint::Fill(1),
    }
}

/// Plain-text form of a table cell. Inline gauges become "81%".
fn cell_text(cell: Option<&ViewCell>) -> String {
    match cell {
        None => String::new(),
        Some(ViewCell::Text(s)) => s.clone(),
        Some(ViewCell::Number(n)) => format!("{n}"),
        Some(ViewCell::Widget(Widget::Gauge {
            percent, caption, ..
        })) => match caption {
            Some(c) => format!("{percent:.0}% {c}"),
            None => format!("{percent:.0}%"),
        },
        Some(ViewCell::Widget(Widget::Stat { value, unit, .. })) => {
            format!(
                "{value}{}",
                unit.as_deref().map(|u| format!(" {u}")).unwrap_or_default()
            )
        }
        Some(ViewCell::Widget(_)) => "…".into(),
    }
}

/// A tile whose module hasn't produced a view yet.
pub fn draw_waiting(f: &mut Canvas, p: &Palette, area: Rect, tile_id: &str) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(p.muted))
        .title(format!(" {tile_id} "));
    f.render_widget(
        Paragraph::new("waiting…")
            .style(Style::new().fg(p.muted))
            .block(block),
        area,
    );
}

/// Rows a text needs at this width: each of its lines, wrapped.
fn text_rows(text: &str, width: u16) -> u16 {
    let width = width.max(1);
    text.split('\n')
        .map(|l| (l.chars().count() as u16).div_ceil(width).max(1))
        .fold(0, u16::saturating_add)
}

/// The small markdown subset text widgets support: `**bold**` and `` `code` ``.
fn inline_markdown<'a>(p: &Palette, text: &'a str) -> Vec<Span<'a>> {
    let mut spans = vec![];
    let mut rest = text;
    while !rest.is_empty() {
        let next = [
            ("**", Style::new().add_modifier(Modifier::BOLD)),
            ("`", Style::new().fg(p.accent)),
        ]
        .into_iter()
        .filter_map(|(mark, style)| {
            let start = rest.find(mark)?;
            let len = rest[start + mark.len()..].find(mark)?;
            Some((start, mark.len(), len, style))
        })
        .min_by_key(|(start, ..)| *start);
        let Some((start, mark, len, style)) = next else {
            spans.push(Span::raw(rest));
            break;
        };
        if start > 0 {
            spans.push(Span::raw(&rest[..start]));
        }
        spans.push(Span::styled(&rest[start + mark..start + mark + len], style));
        rest = &rest[start + mark + len + mark..];
    }
    spans
}

#[cfg(test)]
mod tests {
    use dashboard_contract::Theme;
    use ratatui::buffer::Buffer;

    use super::*;

    fn check(name: &str, status: Status, history: Vec<Status>, detail: &str) -> UptimeCheck {
        UptimeCheck {
            name: name.into(),
            status,
            uptime: Some(97.5),
            latency_ms: Some(212.0),
            detail: Some(detail.into()),
            history,
            url: None,
        }
    }

    fn text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol())
            .collect::<String>()
    }

    #[test]
    fn text_keeps_its_line_breaks() {
        let widget = Widget::Text {
            text: "**one**\ntwo".into(),
            markdown: true,
        };
        assert_eq!(min_height(&widget, 10), 2);
        assert_eq!(text_rows("a\n\n123456", 4), 4);
        let area = Rect::new(0, 0, 10, 2);
        let mut buf = Buffer::empty(area);
        let p = Palette::for_theme(Theme::Dark, false);
        draw_widget(
            &mut Canvas(&mut buf),
            &p,
            area,
            &widget,
            &mut Hits::default(),
        );
        assert_eq!(text(&buf, 0), "one       ");
        assert_eq!(text(&buf, 1), "two       ");
    }

    #[test]
    fn uptime_rows_show_strip_percent_and_time_or_error() {
        let widget = Widget::Uptime {
            checks: vec![
                check("api", Status::Ok, vec![Status::Ok; 3], "HTTP 200"),
                check(
                    "db",
                    Status::Error,
                    vec![Status::Ok, Status::Error],
                    "refused",
                ),
            ],
        };
        let area = Rect::new(0, 0, 40, 2);
        let mut buf = Buffer::empty(area);
        let p = Palette::for_theme(Theme::Dark, false);
        draw_widget(
            &mut Canvas(&mut buf),
            &p,
            area,
            &widget,
            &mut Hits::default(),
        );
        assert_eq!(text(&buf, 0), "● api  ················▌▌▌  97.5%  212ms");
        assert_eq!(text(&buf, 1), "● db   ···············▌▌  97.5%  refused");
    }
}
