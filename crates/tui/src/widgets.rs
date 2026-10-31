//! Drawing tiles and the widgets inside them.

use dashboard_contract::rpc::LogLevel;
use dashboard_contract::{Action, Cell as ViewCell, Status, TileUpdate, UptimeCheck, Widget};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};

use crate::canvas::Canvas;
use crate::charts;
use crate::fit;
use crate::mouse::{Click, Hits};
use crate::pie;
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

    // Too small for even the shortest form of the view: its one-line summary.
    if inner.height < least(&tile.view.root, inner.width) {
        f.render_widget(
            Paragraph::new(tile.view.summary.as_str()).wrap(Wrap { trim: true }),
            inner,
        );
        skip_items(&tile.view.root, hits);
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
            let (labeled, shown) = charts::bars_fit(label.is_some(), bars.len(), area.height);
            let top = area.y + labeled as u16;
            for i in 0..bars.len() {
                let height = (i < shown) as u16;
                let y = top.saturating_add(i as u16);
                hits.item(Rect { y, height, ..area }, area);
            }
            let spec = charts::BarsSpec {
                label: label.as_deref().filter(|_| labeled),
                unit: unit.as_deref(),
                format: *format,
                max: *max,
                bars,
            };
            charts::draw_bars(f, p, area, &spec);
        }
        Widget::Pie {
            label,
            slices,
            donut,
            format,
            unit,
            color,
        } => {
            let spec = pie::PieSpec {
                label: label.as_deref(),
                slices,
                donut: *donut,
                format: *format,
                unit: unit.as_deref(),
                color: *color,
            };
            pie::draw_pie(f, p, area, &spec);
        }
        Widget::List { items, empty_text } => draw_list(f, p, area, items, empty_text, hits),
        Widget::Table { columns, rows } => {
            // One header line, then one line per row, as many as fit.
            let (shown, more) = fit_count(rows.len(), area.height.saturating_sub(1));
            for (i, row) in rows.iter().enumerate() {
                let line = Rect {
                    y: (area.y + 1).saturating_add(i as u16),
                    height: (i < shown) as u16,
                    ..area
                };
                hits.item(line, area);
                if let Some(action) = &row.action
                    && i < shown
                {
                    hits.add(line.intersection(area), Click::Run(action.clone()));
                }
            }
            let shown_columns = table_columns(columns, rows, area.width);
            let header = Row::new(
                shown_columns
                    .iter()
                    .map(|(i, w)| Cell::from(fit::truncate(&columns[*i].label, *w as usize))),
            )
            .style(Style::new().add_modifier(Modifier::BOLD));
            let rows = rows.iter().take(shown).map(|row| {
                Row::new(shown_columns.iter().map(|(i, w)| {
                    let text = cell_text(row.cells.get(&columns[*i].key));
                    Cell::from(fit::truncate(&text, *w as usize))
                }))
                .style(Style::new().fg(p.opt_status(row.status)))
            });
            let widths: Vec<Constraint> = shown_columns
                .iter()
                .map(|(_, w)| Constraint::Length(*w))
                .collect();
            let body = Rect {
                height: area.height.min(1 + shown as u16),
                ..area
            };
            let table = Table::new(rows, widths)
                .header(header)
                .column_spacing(COLUMN_GAP);
            f.render_widget(table, body);
            draw_more(f, p, area, 1 + shown as u16, more);
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
                    fit::line(spans, area.width as usize)
                })
                .collect();
            f.render_widget(Paragraph::new(rows), area);
        }
        Widget::Uptime { checks } => {
            let (shown, more) = fit_count(checks.len(), area.height);
            let name_w = checks
                .iter()
                .map(|c| fit::width(&c.name))
                .max()
                .unwrap_or(0)
                .min(20) as u16;
            for (i, check) in checks.iter().enumerate() {
                let row = Rect {
                    y: area.y.saturating_add(i as u16),
                    height: 1,
                    ..area
                };
                if i >= shown {
                    hits.item(Rect { height: 0, ..row }, area);
                    continue;
                }
                hits.item(row, area);
                if let Some(url) = &check.url {
                    let action = Action::OpenUrl { url: url.clone() };
                    hits.add(row, Click::Run(action));
                }
                f.render_widget(
                    Paragraph::new(uptime_line(p, check, name_w, area.width)),
                    row,
                );
            }
            draw_more(f, p, area, shown as u16, more);
        }
        Widget::Image { path, caption } => {
            crate::images::draw(f, p, area, path, caption.as_deref());
        }
        Widget::Row { children } if stack(children.len(), area.width) => {
            draw_stack(f, p, area, children, 0, hits);
        }
        Widget::Row { children } => {
            let areas = Layout::horizontal(vec![Constraint::Fill(1); children.len()])
                .spacing(2)
                .split(area);
            for (child, a) in children.iter().zip(areas.iter()) {
                draw_widget(f, p, *a, child, hits);
            }
        }
        Widget::Column { children } => draw_stack(f, p, area, children, 1, hits),
    }
}

/// Children one under another, `gap` rows apart, with the rows
/// `stack_heights` gives them. Children that get none are not drawn.
fn draw_stack(
    f: &mut Canvas,
    p: &Palette,
    area: Rect,
    children: &[Widget],
    gap: u16,
    hits: &mut Hits,
) {
    let (heights, gap) = stack_heights(children, area.width, area.height, gap);
    let mut y = area.y;
    for (child, height) in children.iter().zip(heights) {
        if height == 0 {
            skip_items(child, hits);
            continue;
        }
        draw_widget(f, p, Rect { y, height, ..area }, child, hits);
        y = y.saturating_add(height + gap);
    }
}

/// Rows for children stacked in `height` rows, `gap` rows apart. Each gets
/// the rows it wants, in order, until one doesn't fit: that one gets what is
/// left if it can show something in it (its least rows), and the ones after
/// it get none. So a view too long for its tile is cut at the bottom, a
/// whole widget at a time, and the widget at the cut shows a shorter form.
/// A one-line text just before the cut (a heading) is left out with what it
/// heads. Gaps are kept only when every child fits with them. Rows left over go to
/// children that grow (graphs, logs, pies), else stay empty at the bottom.
/// Returns the heights and the gap used.
fn stack_heights(children: &[Widget], width: u16, height: u16, gap: u16) -> (Vec<u16>, u16) {
    let wants: Vec<Want> = children.iter().map(|c| want(c, width)).collect();
    let total = wants.iter().fold(0u16, |t, w| t.saturating_add(w.rows));
    let gaps = gap.saturating_mul(children.len().saturating_sub(1) as u16);
    let gap = if total.saturating_add(gaps) <= height {
        gap
    } else {
        0
    };
    let mut heights = vec![0; children.len()];
    let mut left = height;
    for (i, child) in children.iter().enumerate() {
        let room = left.saturating_sub(if i > 0 { gap } else { 0 });
        if wants[i].rows <= room {
            heights[i] = wants[i].rows;
            left = room - wants[i].rows;
            continue;
        }
        if least(child, width) <= room {
            heights[i] = room;
            left = 0;
        }
        break;
    }
    // A one-line text right before the cut is a heading without its section:
    // drop it too.
    if let Some(last) = heights.iter().rposition(|h| *h > 0)
        && last + 1 < children.len()
        && heights[last] == 1
        && matches!(children[last], Widget::Text { .. })
    {
        heights[last] = 0;
        left += 1;
    }
    let growers: Vec<usize> = (0..children.len())
        .filter(|&i| heights[i] > 0 && wants[i].grows)
        .collect();
    if left > 0 && !growers.is_empty() {
        let n = growers.len() as u16;
        for (k, i) in growers.into_iter().enumerate() {
            heights[i] += left / n + u16::from((k as u16) < left % n);
        }
    }
    (heights, gap)
}

/// Records the keyboard items of a widget that isn't drawn, so the items
/// after it keep their places in `nav::items` order.
fn skip_items(widget: &Widget, hits: &mut Hits) {
    for _ in crate::nav::items(widget) {
        hits.item(Rect::default(), Rect::default());
    }
}

/// Of `count` one-line entries in `rows` rows, how many to show and how
/// many are left out. When some are left out the last row says so ("+3
/// more"), unless there is only one row.
fn fit_count(count: usize, rows: u16) -> (usize, usize) {
    let rows = rows as usize;
    if count <= rows {
        (count, 0)
    } else if rows >= 2 {
        (rows - 1, count - rows + 1)
    } else {
        (rows, count - rows)
    }
}

/// "+3 more" in muted text, `y` rows into `area`, when `more` is not 0 and
/// the row is inside the area.
fn draw_more(f: &mut Canvas, p: &Palette, area: Rect, y: u16, more: usize) {
    if more == 0 || y >= area.height {
        return;
    }
    f.render_widget(
        Paragraph::new(format!("  +{more} more")).style(Style::new().fg(p.muted)),
        Rect {
            y: area.y + y,
            height: 1,
            ..area
        },
    );
}

/// A list: each item a title line (marker, title, tags) and an optional
/// subtitle line. When they don't all fit, the items that do are shown
/// whole, the last of them without its subtitle if only its title fits, and
/// the last row says how many are left out. In a single row: the first title
/// and the count ("● title… +4").
fn draw_list(
    f: &mut Canvas,
    p: &Palette,
    area: Rect,
    items: &[dashboard_contract::ListItem],
    empty_text: &Option<String>,
    hits: &mut Hits,
) {
    if items.is_empty() {
        f.render_widget(
            Paragraph::new(empty_text.as_deref().unwrap_or("nothing here")),
            area,
        );
        return;
    }
    let width = area.width as usize;
    let rows_of = |i: &dashboard_contract::ListItem| 1 + i.subtitle.is_some() as u16;
    let total = items.iter().fold(0u16, |t, i| t.saturating_add(rows_of(i)));
    let more_row = total > area.height && area.height >= 2;
    let budget = area.height - more_row as u16;
    // Rows each item gets: 2 with its subtitle, 1 without, 0 not shown.
    let mut rows = vec![0u16; items.len()];
    let mut used = 0;
    for (i, item) in items.iter().enumerate() {
        let want = rows_of(item);
        if used + want <= budget {
            rows[i] = want;
            used += want;
        } else if used < budget {
            rows[i] = 1;
            used += 1;
        } else {
            break;
        }
    }
    let hidden = rows.iter().filter(|r| **r == 0).count();
    let muted = Style::new().fg(p.muted);

    let mut lines = vec![];
    let mut y = area.y;
    for (item, height) in items.iter().zip(&rows) {
        let row = Rect {
            y,
            height: *height,
            ..area
        };
        hits.item(row, area);
        if *height == 0 {
            continue;
        }
        if let Some(action) = &item.action {
            hits.add(row.intersection(area), Click::Run(action.clone()));
        }
        y += height;
        let mut spans = vec![
            Span::styled("● ", Style::new().fg(item_color(p, item))),
            Span::raw(item.title.as_str()),
        ];
        for tag in &item.tags {
            spans.push(Span::styled(format!(" [{tag}]"), muted));
        }
        if hidden > 0 && !more_row {
            let count = format!(" +{hidden}");
            let mut line = fit::line(spans, width.saturating_sub(fit::width(&count)));
            line.spans.push(Span::styled(count, muted));
            lines.push(line);
        } else {
            lines.push(fit::line(spans, width));
        }
        if let (Some(sub), 2) = (&item.subtitle, height) {
            lines.push(fit::line(
                vec![Span::styled(format!("  {sub}"), muted)],
                width,
            ));
        }
    }
    if more_row && hidden > 0 {
        lines.push(Line::styled(format!("  +{hidden} more"), muted));
    }
    f.render_widget(Paragraph::new(lines), area);
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
        c.detail.as_ref().map(|d| fit::truncate(d, 24))
    };
    let mut tail = String::new();
    if let Some(pc) = &percent {
        tail.push_str(&format!(" {pc:>6}"));
    }
    if let Some(last) = &last {
        tail.push_str(&format!("  {last}"));
    }
    let name = fit::truncate(&c.name, name_w as usize);
    let pad = (name_w as usize).saturating_sub(fit::width(&name));
    let fixed = 2 + name_w + 2 + fit::width(&tail) as u16;
    let strip_w = width.saturating_sub(fixed) as usize;

    let mut spans = vec![
        Span::styled("● ", Style::new().fg(p.status(c.status))),
        Span::raw(format!("{name}{}  ", " ".repeat(pad))),
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

/// Rows a tile would take to show all of its view at this width, borders
/// included. Tiles get their rows from their size, not from this; `dashboard
/// plugin-check` uses it to size its preview.
pub fn tile_wanted_height(tile: &TileUpdate, width: u16) -> u16 {
    want(&tile.view.root, width.saturating_sub(2)).rows + 2
}

/// The rows a widget would like at a width, and whether it takes rows left
/// over (graphs, logs and pies stretch; text, lists and tables don't).
#[derive(Debug, Clone, Copy)]
struct Want {
    rows: u16,
    grows: bool,
}

fn want(widget: &Widget, width: u16) -> Want {
    let width = width.max(1);
    let fixed = |rows: u16| Want { rows, grows: false };
    let grows = |rows: u16| Want { rows, grows: true };
    match widget {
        Widget::Text { text, .. } => fixed(text_rows(text, width).max(1)),
        Widget::Stat { .. } | Widget::Gauge { .. } => fixed(1),
        // Value line plus a few rows of plot (4 dots each in braille).
        Widget::Series { mirrored, .. } => grows(if *mirrored { 7 } else { 5 }),
        Widget::MeterGrid { label, meters, .. } => fixed(charts::meter_grid_height(
            width,
            meters.len(),
            label.is_some(),
        )),
        Widget::Bars { label, bars, .. } => {
            fixed(bars.len().max(1) as u16 + label.is_some() as u16)
        }
        Widget::Pie { label, slices, .. } => grows(
            pie::wanted_height(slices.len(), label.is_some()).max(pie::min_height(
                slices,
                label.is_some(),
                width,
            )),
        ),
        Widget::List { items, .. } => fixed(
            items
                .iter()
                .fold(0u16, |t, i| {
                    t.saturating_add(1 + i.subtitle.is_some() as u16)
                })
                .max(1),
        ),
        Widget::Table { rows, .. } => fixed(1 + rows.len().max(1) as u16),
        Widget::Log { label, .. } => grows(5 + label.is_some() as u16),
        Widget::Uptime { checks } => fixed(checks.len().max(1) as u16),
        Widget::Image { caption, .. } => grows(4 + caption.is_some() as u16),
        Widget::Row { children } if stack(children.len(), width) => {
            let wants = children.iter().map(|c| want(c, width));
            wants.fold(fixed(0), |a, w| Want {
                rows: a.rows.saturating_add(w.rows),
                grows: a.grows || w.grows,
            })
        }
        Widget::Row { children } => {
            let w = row_child_width(children.len(), width);
            let wants = children.iter().map(|c| want(c, w));
            wants.fold(fixed(1), |a, w| Want {
                rows: a.rows.max(w.rows),
                grows: a.grows || w.grows,
            })
        }
        Widget::Column { children } => {
            let gaps = children.len().saturating_sub(1) as u16;
            let wants = children.iter().map(|c| want(c, width));
            wants.fold(fixed(gaps), |a, w| Want {
                rows: a.rows.saturating_add(w.rows),
                grows: a.grows || w.grows,
            })
        }
    }
}

/// The fewest rows a widget can show something useful in: a list its first
/// title, a table its header and one row, a graph its value line. Rows and
/// columns need only their first child's, since they drop the rest.
fn least(widget: &Widget, width: u16) -> u16 {
    let width = width.max(1);
    match widget {
        Widget::Table { .. } => 2,
        Widget::MeterGrid { label, .. } | Widget::Pie { label, .. } | Widget::Log { label, .. } => {
            1 + label.is_some() as u16
        }
        Widget::Row { children } if stack(children.len(), width) => {
            children.first().map_or(1, |c| least(c, width))
        }
        Widget::Row { children } => {
            let w = row_child_width(children.len(), width);
            children.iter().map(|c| least(c, w)).max().unwrap_or(1)
        }
        Widget::Column { children } => children.first().map_or(1, |c| least(c, width)),
        _ => 1,
    }
}

/// Width of each child of a row of `n` side by side, 2 columns apart.
fn row_child_width(n: usize, width: u16) -> u16 {
    let n = n.max(1) as u16;
    width.saturating_sub(2 * (n - 1)) / n
}

/// Cells between table columns.
const COLUMN_GAP: u16 = 1;
/// The least width a table column is cut to, unless it is narrower.
const LEAST_COLUMN: usize = 6;
/// The least width the first column is cut to, so it stays readable.
const LEAST_FIRST_COLUMN: usize = 12;

/// Which columns a table shows in `width` cells, and how wide each is. Each
/// wants its widest cell or label. When they don't all fit at their least
/// width, columns are dropped, lowest `priority` first (of equal ones the
/// rightmost); the first column is always kept. Then the widest columns
/// give up cells until the row fits, and cells left over are shared out.
fn table_columns(
    columns: &[dashboard_contract::Column],
    rows: &[dashboard_contract::Row],
    width: u16,
) -> Vec<(usize, u16)> {
    let natural: Vec<usize> = columns
        .iter()
        .map(|c| {
            rows.iter()
                .map(|r| fit::width(&cell_text(r.cells.get(&c.key))))
                .fold(fit::width(&c.label), usize::max)
                .max(1)
        })
        .collect();
    let least = |i: usize| {
        natural[i].min(if i == 0 {
            LEAST_FIRST_COLUMN
        } else {
            LEAST_COLUMN
        })
    };
    let width = width as usize;
    let gap = COLUMN_GAP as usize;
    let need = |keep: &[usize]| {
        keep.iter().map(|&i| least(i)).sum::<usize>() + gap * keep.len().saturating_sub(1)
    };
    let mut keep: Vec<usize> = (0..columns.len()).collect();
    while keep.len() > 1 && need(&keep) > width {
        let drop = keep[1..]
            .iter()
            .copied()
            .min_by_key(|&i| (columns[i].priority.unwrap_or(0), std::cmp::Reverse(i)))
            .expect("more than one column");
        keep.retain(|&i| i != drop);
    }
    let mut widths: Vec<usize> = keep.iter().map(|&i| natural[i]).collect();
    let mut total = widths.iter().sum::<usize>() + gap * keep.len().saturating_sub(1);
    while total > width {
        let widest = (0..keep.len())
            .filter(|&k| widths[k] > least(keep[k]))
            .max_by_key(|&k| (widths[k], std::cmp::Reverse(k)));
        let Some(k) = widest else { break };
        widths[k] -= 1;
        total -= 1;
    }
    if total > width
        && let Some(first) = widths.first_mut()
    {
        // Only the first column is left and the tile is narrower than it.
        *first = first.saturating_sub(total - width);
    }
    if total < width && !keep.is_empty() {
        let (extra, n) = (width - total, keep.len());
        for (k, w) in widths.iter_mut().enumerate() {
            *w += extra / n + usize::from(k < extra % n);
        }
    }
    keep.into_iter()
        .zip(widths)
        .map(|(i, w)| (i, w as u16))
        .collect()
}

/// Plain-text form of a table cell. Inline gauges become "81%", pies their
/// largest slice, "photos 60%".
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
        Some(ViewCell::Widget(Widget::Pie { slices, .. })) => {
            let total: f64 = slices.iter().map(|s| s.value.max(0.0)).sum();
            match slices.iter().max_by(|a, b| a.value.total_cmp(&b.value)) {
                Some(top) if total > 0.0 && total.is_finite() => {
                    format!("{} {:.0}%", top.label, top.value / total * 100.0)
                }
                _ => String::new(),
            }
        }
        Some(ViewCell::Widget(_)) => "…".into(),
    }
}

/// A tile whose module hasn't produced a view yet.
/// A list item's marker: its status, else its tone, else the terminal's color.
fn item_color(p: &Palette, item: &dashboard_contract::ListItem) -> ratatui::style::Color {
    match (item.status, item.tone) {
        (Some(s), _) => p.status(s),
        (None, Some(t)) => p.tone(t),
        (None, None) => ratatui::style::Color::Reset,
    }
}

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
        assert_eq!(want(&widget, 10).rows, 2);
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

    /// The demo widget at large lost its third gauge: a column inside a
    /// column got an equal share of the height instead of what it needs.
    #[test]
    fn nested_column_gets_room_for_every_child() {
        let gauge = |label: &str| Widget::Gauge {
            label: label.into(),
            percent: 50.0,
            marker: None,
            thresholds: None,
            caption: None,
        };
        let widget = Widget::Column {
            children: vec![
                Widget::Column {
                    children: vec![gauge("one"), gauge("two"), gauge("three"), gauge("four")],
                },
                Widget::Series {
                    label: "graph".into(),
                    unit: None,
                    y_max: None,
                    id: None,
                    history: None,
                    style: Default::default(),
                    color: Default::default(),
                    mirrored: false,
                    format: Default::default(),
                    series: vec![],
                },
                Widget::Series {
                    label: "other".into(),
                    unit: None,
                    y_max: None,
                    id: None,
                    history: None,
                    style: Default::default(),
                    color: Default::default(),
                    mirrored: false,
                    format: Default::default(),
                    series: vec![],
                },
            ],
        };
        let area = Rect::new(0, 0, 40, 20);
        let mut buf = Buffer::empty(area);
        let p = Palette::for_theme(Theme::Dark, false);
        draw_widget(
            &mut Canvas(&mut buf),
            &p,
            area,
            &widget,
            &mut Hits::default(),
        );
        let rows: Vec<String> = (0..area.height).map(|y| text(&buf, y)).collect();
        for label in ["one", "two", "three", "four"] {
            assert!(
                rows.iter().any(|r| r.starts_with(label)),
                "{label} not drawn:\n{}",
                rows.join("\n")
            );
        }
    }

    fn list(n: usize) -> Widget {
        Widget::List {
            items: (1..=n)
                .map(|i| dashboard_contract::ListItem {
                    title: format!("item {i}"),
                    subtitle: None,
                    status: None,
                    tone: None,
                    tags: vec![],
                    action: None,
                })
                .collect(),
            empty_text: None,
        }
    }

    fn stat(label: &str) -> Widget {
        Widget::Stat {
            label: label.into(),
            value: "1".into(),
            unit: None,
            delta: None,
            status: None,
        }
    }

    fn draw(widget: &Widget, width: u16, height: u16) -> Vec<String> {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        let p = Palette::for_theme(Theme::Dark, false);
        draw_widget(
            &mut Canvas(&mut buf),
            &p,
            area,
            widget,
            &mut Hits::default(),
        );
        (0..height)
            .map(|y| text(&buf, y).trim_end().to_string())
            .collect()
    }

    #[test]
    fn a_long_list_shows_what_fits_and_how_many_are_left() {
        assert_eq!(
            draw(&list(10), 20, 3),
            ["● item 1", "● item 2", "  +8 more"]
        );
        // One row: the first title and the count.
        assert_eq!(draw(&list(10), 20, 1), ["● item 1 +9"]);
        assert_eq!(draw(&list(2), 20, 3), ["● item 1", "● item 2", ""]);
    }

    #[test]
    fn a_column_too_long_for_its_rows_is_cut_a_widget_at_a_time() {
        let column = Widget::Column {
            children: vec![stat("one"), stat("two"), list(5), stat("three")],
        };
        // Everything fits only without the gaps; the list shortens and the
        // last stat is left out.
        assert_eq!(
            draw(&column, 20, 4),
            ["one 1", "two 1", "● item 1", "  +4 more"]
        );
        // A heading whose section is cut goes with it.
        let heading = |t: &str| Widget::Text {
            text: t.into(),
            markdown: false,
        };
        let sections = Widget::Column {
            children: vec![heading("Mine"), list(1), heading("Others"), list(3)],
        };
        assert_eq!(draw(&sections, 20, 3), ["Mine", "● item 1", ""]);
        // With room for all of it, gaps come back.
        assert_eq!(draw(&column, 20, 11)[..4], ["one 1", "", "two 1", ""]);
    }

    #[test]
    fn a_table_shows_the_rows_that_fit() {
        let table = Widget::Table {
            columns: vec![dashboard_contract::Column {
                key: "k".into(),
                label: "Name".into(),
                priority: None,
            }],
            rows: (1..=6)
                .map(|i| dashboard_contract::Row {
                    cells: [("k".to_string(), ViewCell::Text(format!("row {i}")))].into(),
                    status: None,
                    action: None,
                })
                .collect(),
        };
        assert_eq!(draw(&table, 20, 4), ["Name", "row 1", "row 2", "  +4 more"]);
    }

    #[test]
    fn a_narrow_table_drops_low_priority_columns_and_keeps_the_first() {
        let col = |key: &str, priority| dashboard_contract::Column {
            key: key.into(),
            label: key.into(),
            priority,
        };
        let columns = vec![
            col("pod", Some(3)),
            col("namespace", Some(-1)),
            col("status", Some(2)),
            col("ready", Some(1)),
            col("restarts", None),
            col("age", None),
        ];
        let row = |cells: [&str; 6]| dashboard_contract::Row {
            cells: columns
                .iter()
                .zip(cells)
                .map(|(c, v)| (c.key.clone(), ViewCell::Text(v.into())))
                .collect(),
            status: None,
            action: None,
        };
        let rows = vec![row([
            "worker-7d9f8c6b5-x2k4p",
            "shop",
            "CrashLoopBackOff",
            "0/1",
            "14",
            "90m",
        ])];
        let table = Widget::Table {
            columns: columns.clone(),
            rows: rows.clone(),
        };
        // Namespace goes first, then age and restarts (rightmost first).
        assert_eq!(
            draw(&table, 28, 2),
            ["pod          status    ready", "worker-7d9f… CrashLoo… 0/1"]
        );
        let keys = |w| -> Vec<&str> {
            table_columns(&columns, &rows, w)
                .iter()
                .map(|(i, _)| columns[*i].key.as_str())
                .collect()
        };
        assert_eq!(
            keys(80),
            ["pod", "namespace", "status", "ready", "restarts", "age"]
        );
        assert_eq!(keys(40), ["pod", "status", "ready", "restarts", "age"]);
        // Never less than the first column, cut to the width if need be.
        assert_eq!(keys(5), ["pod"]);
        assert_eq!(table_columns(&columns, &rows, 5)[0].1, 5);
    }

    #[test]
    fn a_tile_too_small_for_its_view_shows_the_summary() {
        let tile = TileUpdate {
            tile_id: "t".into(),
            view: dashboard_contract::View {
                status: Status::Ok,
                summary: "all good".into(),
                root: Widget::Table {
                    columns: vec![],
                    rows: vec![],
                },
            },
            updated_at: 0,
            stale: false,
            demo: false,
        };
        let area = Rect::new(0, 0, 20, 3);
        let mut buf = Buffer::empty(area);
        let p = Palette::for_theme(Theme::Dark, false);
        draw_tile(&mut Canvas(&mut buf), &p, area, &tile, &mut Hits::default());
        assert_eq!(text(&buf, 1), "│all good          │");
    }

    #[test]
    fn long_list_titles_end_with_an_ellipsis() {
        let widget = Widget::List {
            items: vec![dashboard_contract::ListItem {
                title: "Fix the flaky integration test in the billing service".into(),
                subtitle: Some("org/billing!412 · updated 3h ago by someone".into()),
                status: None,
                tone: None,
                tags: vec!["draft".into()],
                action: None,
            }],
            empty_text: None,
        };
        let area = Rect::new(0, 0, 24, 2);
        let mut buf = Buffer::empty(area);
        let p = Palette::for_theme(Theme::Dark, false);
        draw_widget(
            &mut Canvas(&mut buf),
            &p,
            area,
            &widget,
            &mut Hits::default(),
        );
        assert_eq!(text(&buf, 0), "● Fix the flaky integra…");
        assert_eq!(text(&buf, 1), "  org/billing!412 · upd…");
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
