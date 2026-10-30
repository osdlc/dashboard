//! Graphs, meter grids and bar lists, drawn straight into the buffer.

use dashboard_contract::{
    Bar, ColorScale, Meter, SeriesData, SeriesStyle, ValueFormat, format_value,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Paragraph};

use crate::canvas::Canvas;
use crate::theme::Palette;

/// Width of one meter in a meter grid, including padding.
const METER_WIDTH: u16 = 18;
const BAR_LABEL_WIDTH: usize = 16;

pub struct SeriesSpec<'a> {
    pub label: &'a str,
    pub unit: Option<&'a str>,
    pub y_max: Option<f64>,
    pub style: SeriesStyle,
    pub color: ColorScale,
    pub mirrored: bool,
    pub format: ValueFormat,
    pub series: &'a [SeriesData],
}

pub fn draw_series(f: &mut Canvas, p: &Palette, area: Rect, s: &SeriesSpec) {
    let [head, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);

    let mut spans = vec![Span::raw(format!("{} ", s.label))];
    let arrows = if s.mirrored { ["↓", "↑"] } else { ["", ""] };
    for (i, data) in s.series.iter().take(2).enumerate() {
        if let Some(last) = data.points.last() {
            let text = format_value(last.1, s.format, s.unit);
            spans.push(Span::styled(
                format!(" {}{text}", arrows[i]),
                Style::new().fg(p.accent),
            ));
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), head);
    if body.height == 0 || body.width == 0 {
        return;
    }

    // Shared scale: y_max, else the largest value shown.
    let width = body.width as usize * 2; // two braille samples per cell
    let visible = |d: &'_ SeriesData| -> Vec<f64> {
        d.points
            .iter()
            .rev()
            .take(width)
            .rev()
            .map(|p| p.1.max(0.0))
            .collect()
    };
    let max = s.y_max.unwrap_or_else(|| {
        s.series
            .iter()
            .flat_map(&visible)
            .fold(0.0, f64::max)
            .max(1.0)
    });

    if s.mirrored && s.series.len() >= 2 {
        let [top, bottom] =
            Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).areas(body);
        fill_columns(
            f,
            p,
            top,
            &visible(&s.series[0]),
            max,
            s.color,
            Direction::Up,
        );
        fill_columns(
            f,
            p,
            bottom,
            &visible(&s.series[1]),
            max,
            s.color,
            Direction::Down,
        );
        return;
    }
    match s.style {
        SeriesStyle::Area => {
            if let Some(d) = s.series.first() {
                fill_columns(f, p, body, &visible(d), max, s.color, Direction::Up);
            }
        }
        SeriesStyle::Bars => {
            if let Some(d) = s.series.first() {
                let values = visible(d);
                let tail = &values[values.len().saturating_sub(body.width as usize)..];
                draw_bars_chart(f, p, body, tail, max, s.color);
            }
        }
        SeriesStyle::Line => draw_lines(f, p, body, s.series, max),
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Up,
    Down,
}

/// Partial blocks from 1/8 to 8/8, bottom-up.
const EIGHTHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Column chart: one sample per cell, eighth-block resolution, newest on the right.
fn draw_bars_chart(
    f: &mut Canvas,
    p: &Palette,
    area: Rect,
    values: &[f64],
    max: f64,
    color: ColorScale,
) {
    let rows = area.height as usize;
    let buf = f.buffer_mut();
    let offset = area.width as usize - values.len();
    for (i, v) in values.iter().enumerate() {
        let ratio = (v / max).clamp(0.0, 1.0);
        let units = (ratio * (rows * 8) as f64).round() as usize;
        // Whole column takes the color of its top, so bars read as one value each.
        let fg = p.scale(color, ratio);
        for row in 0..rows {
            let filled = units.saturating_sub(row * 8).min(8);
            if filled == 0 {
                break;
            }
            let (x, y) = (
                area.x + (offset + i) as u16,
                area.y + area.height - 1 - row as u16,
            );
            buf[(x, y)].set_char(EIGHTHS[filled - 1]).set_fg(fg);
        }
    }
}

/// Braille dot bits for (column, dot row), dot rows counted from the top.
const BRAILLE: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

/// Area fill in braille: two samples per cell, four dots tall, newest on the
/// right. `Up` fills from the bottom, `Down` from the top.
fn fill_columns(
    f: &mut Canvas,
    p: &Palette,
    area: Rect,
    values: &[f64],
    max: f64,
    color: ColorScale,
    dir: Direction,
) {
    let rows = area.height as usize;
    let cols = area.width as usize;
    let dots = rows * 4;
    let buf = f.buffer_mut();
    // Right-align: the newest sample sits in the right half of the last cell.
    let offset = (cols * 2).saturating_sub(values.len());
    let mut cells = vec![0u8; cols * rows];
    for (i, v) in values.iter().enumerate() {
        let slot = offset + i;
        let (cx, half) = (slot / 2, slot % 2);
        let mut height = ((v / max).clamp(0.0, 1.0) * dots as f64).round() as usize;
        if height == 0 && *v > 0.0 {
            height = 1; // show that something happened
        }
        for d in 0..height {
            // Dot row counted from the top of the whole area.
            let dy = match dir {
                Direction::Up => dots - 1 - d,
                Direction::Down => d,
            };
            cells[(dy / 4) * cols + cx] |= BRAILLE[half][dy % 4];
        }
    }
    for row in 0..rows {
        for cx in 0..cols {
            let bits = cells[row * cols + cx];
            if bits == 0 {
                continue;
            }
            let level = match dir {
                Direction::Up => rows - 1 - row,
                Direction::Down => row,
            };
            // Colored by height, so the top of a tall heat column is red.
            let fg = p.scale(color, (level as f64 + 0.5) / rows as f64);
            let ch = char::from_u32(0x2800 + bits as u32).unwrap();
            buf[(area.x + cx as u16, area.y + row as u16)]
                .set_char(ch)
                .set_fg(fg);
        }
    }
}

fn draw_lines(f: &mut Canvas, p: &Palette, area: Rect, series: &[SeriesData], max: f64) {
    let colors = [p.accent, p.warn, p.ok, p.error];
    let width = area.width as usize * 2; // braille has two dots per cell
    let data: Vec<Vec<(f64, f64)>> = series
        .iter()
        .map(|d| {
            d.points
                .iter()
                .rev()
                .take(width)
                .rev()
                .map(|(t, v)| (*t as f64, *v))
                .collect()
        })
        .collect();
    let t0 = data
        .iter()
        .filter_map(|d| d.first())
        .map(|p| p.0)
        .fold(f64::INFINITY, f64::min);
    let t1 = data
        .iter()
        .filter_map(|d| d.last())
        .map(|p| p.0)
        .fold(f64::NEG_INFINITY, f64::max);
    if !t0.is_finite() || !t1.is_finite() {
        return;
    }
    let datasets = data
        .iter()
        .zip(colors.iter().cycle())
        .map(|(d, c)| {
            Dataset::default()
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::new().fg(*c))
                .data(d)
        })
        .collect();
    let chart = Chart::new(datasets)
        .x_axis(Axis::default().bounds([t0, t1.max(t0 + 1.0)]))
        .y_axis(Axis::default().bounds([0.0, max]));
    f.render_widget(chart, area);
}

/// Rows a meter grid needs at this width.
pub fn meter_grid_height(width: u16, count: usize, labeled: bool) -> u16 {
    let cols = (width / METER_WIDTH).max(1) as usize;
    count.div_ceil(cols) as u16 + labeled as u16
}

pub fn draw_meter_grid(
    f: &mut Canvas,
    p: &Palette,
    area: Rect,
    label: Option<&str>,
    meters: &[Meter],
    color: ColorScale,
) {
    let mut area = area;
    if let Some(label) = label {
        f.render_widget(
            Paragraph::new(label).style(Style::new().fg(p.muted)),
            Rect { height: 1, ..area },
        );
        area.y += 1;
        area.height = area.height.saturating_sub(1);
    }
    let cols = (area.width / METER_WIDTH).max(1);
    let cell = area.width / cols;
    for (i, m) in meters.iter().enumerate() {
        let (col, row) = (i as u16 % cols, i as u16 / cols);
        if row >= area.height {
            break;
        }
        let ratio = (m.percent / 100.0).clamp(0.0, 1.0);
        let fg = p.scale(color, ratio);
        // "12 ████░░░░  45"
        let bar_width = cell.saturating_sub(9) as usize;
        let line = Line::from(vec![
            Span::styled(format!("{:>3} ", m.label), Style::new().fg(p.muted)),
            Span::styled(bar(ratio, bar_width), Style::new().fg(fg)),
            Span::styled(format!("{:>4.0}", m.percent), Style::new().fg(fg)),
        ]);
        let rect = Rect {
            x: area.x + col * cell,
            y: area.y + row,
            width: cell,
            height: 1,
        };
        f.render_widget(Paragraph::new(line), rect);
    }
}

pub struct BarsSpec<'a> {
    pub label: Option<&'a str>,
    pub unit: Option<&'a str>,
    pub format: ValueFormat,
    pub max: Option<f64>,
    pub bars: &'a [Bar],
}

pub fn draw_bars(f: &mut Canvas, p: &Palette, area: Rect, s: &BarsSpec) {
    let mut lines = vec![];
    if let Some(label) = s.label {
        lines.push(Line::styled(label.to_string(), Style::new().fg(p.muted)));
    }
    let max = s
        .max
        .unwrap_or_else(|| s.bars.iter().map(|b| b.value).fold(0.0, f64::max))
        .max(f64::EPSILON);
    let value_width = 10;
    let bar_width = (area.width as usize).saturating_sub(BAR_LABEL_WIDTH + value_width + 2);
    for b in s.bars {
        let ratio = (b.value / max).clamp(0.0, 1.0);
        let fg = b.status.map(|st| p.status(st)).unwrap_or(p.accent);
        let label: String = b.label.chars().take(BAR_LABEL_WIDTH - 1).collect();
        lines.push(Line::from(vec![
            Span::raw(format!("{label:<BAR_LABEL_WIDTH$}")),
            Span::styled(bar(ratio, bar_width), Style::new().fg(fg)),
            Span::raw(format!(
                " {:>value_width$}",
                format_value(b.value, s.format, s.unit)
            )),
        ]));
    }
    f.render_widget(Paragraph::new(lines), area);
}

pub struct GaugeSpec<'a> {
    pub label: &'a str,
    pub percent: f64,
    /// Percent position of a reference mark, e.g. expected usage by now.
    pub marker: Option<f64>,
    pub caption: Option<&'a str>,
    pub color: Color,
}

/// "Memory ██████······ 62% · 19.8 GiB / 32.0 GiB" on one line. The caption
/// is dropped first when space is short.
pub fn draw_gauge(f: &mut Canvas, p: &Palette, area: Rect, g: &GaugeSpec) {
    let value = format!(" {:>3.0}%", g.percent);
    let caption = g.caption.map(|c| format!(" · {c}")).unwrap_or_default();
    let label = format!("{} ", g.label);
    let fixed = label.chars().count() + value.len();
    let width = area.width as usize;
    let (caption, bar_width) = match width.checked_sub(fixed + caption.chars().count() + 8) {
        Some(_) => (caption.clone(), width - fixed - caption.chars().count()),
        None => (String::new(), width.saturating_sub(fixed)),
    };

    let ratio = (g.percent / 100.0).clamp(0.0, 1.0);
    let mut cells: Vec<Span> = bar(ratio, bar_width)
        .chars()
        .map(|c| Span::styled(c.to_string(), Style::new().fg(g.color)))
        .collect();
    if let Some(m) = g.marker
        && bar_width > 0
    {
        let i = (((m / 100.0).clamp(0.0, 1.0)) * (bar_width - 1) as f64).round() as usize;
        cells[i] = Span::styled("┃", Style::new().fg(p.muted));
    }
    let mut spans = vec![Span::styled(label, Style::new().fg(p.muted))];
    spans.extend(cells);
    spans.push(Span::styled(value, Style::new().fg(g.color)));
    spans.push(Span::styled(caption, Style::new().fg(p.muted)));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// A horizontal bar with eighth-block precision, padded to `width` with a dim track.
fn bar(ratio: f64, width: usize) -> String {
    const PARTIAL: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let eighths = (ratio * (width * 8) as f64).round() as usize;
    let full = eighths / 8;
    let mut s = "█".repeat(full.min(width));
    if full < width {
        s.push_str(PARTIAL[eighths % 8]);
        let used = full + usize::from(!eighths.is_multiple_of(8));
        s.push_str(&"·".repeat(width.saturating_sub(used)));
    }
    s
}

pub fn heat_color(stops: &[(u8, u8, u8); 3], t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    let (a, b, local) = if t < 0.5 {
        (stops[0], stops[1], t * 2.0)
    } else {
        (stops[1], stops[2], (t - 0.5) * 2.0)
    };
    let mix = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * local).round() as u8;
    Color::Rgb(mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_widths() {
        assert_eq!(bar(0.0, 4), "····");
        assert_eq!(bar(1.0, 4), "████");
        assert_eq!(bar(0.5, 4), "██··");
        assert_eq!(bar(0.125 / 2.0, 4).chars().count(), 4);
    }

    #[test]
    fn heat_endpoints() {
        let stops = [(0, 0, 0), (100, 100, 100), (200, 200, 200)];
        assert_eq!(heat_color(&stops, 0.0), Color::Rgb(0, 0, 0));
        assert_eq!(heat_color(&stops, 0.5), Color::Rgb(100, 100, 100));
        assert_eq!(heat_color(&stops, 1.0), Color::Rgb(200, 200, 200));
    }
}
