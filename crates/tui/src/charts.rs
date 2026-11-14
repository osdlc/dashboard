//! Graphs, meter grids and bar lists, drawn straight into the buffer.

use dashboard_contract::{
    Bar, ColorScale, Meter, SeriesData, SeriesStyle, ValueFormat, format_value, series_scale,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Paragraph};

use crate::canvas::Canvas;
use crate::fit;
use crate::theme::Palette;

/// Width of one meter in a meter grid, including padding.
const METER_WIDTH: u16 = 18;
const BAR_LABEL_WIDTH: usize = 16;

pub struct SeriesSpec<'a> {
    pub label: &'a str,
    pub unit: Option<&'a str>,
    pub y_max: Option<f64>,
    pub y_min: Option<f64>,
    pub fit: bool,
    pub style: SeriesStyle,
    pub color: ColorScale,
    pub mirrored: bool,
    pub format: ValueFormat,
    pub series: &'a [SeriesData],
}

pub fn draw_series(f: &mut Canvas, p: &Palette, area: Rect, s: &SeriesSpec) {
    let [head, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);

    let mut spans = vec![Span::raw(format!("{} ", s.label))];
    let mirrored = s.mirrored && s.series.len() >= 2;
    if mirrored {
        for (i, data) in s.series.iter().take(2).enumerate() {
            if let Some(last) = data.points.last() {
                let text = format_value(last.1, s.format, s.unit);
                let arrow = ["↓", "↑"][i];
                spans.push(Span::styled(
                    format!(" {arrow}{text}"),
                    Style::new().fg(p.accent),
                ));
            }
        }
    } else if s.series.len() > 1 {
        // Several lines: each latest value in its line's color, with its name.
        for (i, data) in s.series.iter().take(6).enumerate() {
            if let Some(last) = data.points.last() {
                let text = format_value(last.1, s.format, s.unit);
                spans.push(Span::styled(
                    format!(" {} {text}", data.name),
                    Style::new().fg(series_color(p, i, data)),
                ));
            }
        }
    } else if let Some(last) = s.series.first().and_then(|d| d.points.last()) {
        let text = format_value(last.1, s.format, s.unit);
        spans.push(Span::styled(format!(" {text}"), Style::new().fg(p.accent)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), head);
    if body.height == 0 || body.width == 0 {
        return;
    }

    let width = body.width as usize * 2; // two braille samples per cell
    let visible = |d: &'_ SeriesData| -> Vec<f64> {
        d.points
            .iter()
            .rev()
            .take(width)
            .rev()
            .map(|p| p.1)
            .collect()
    };
    // Mirrored charts draw magnitudes: negatives count as 0.
    let up = |v: Vec<f64>| -> Vec<f64> { v.into_iter().map(|x| x.max(0.0)).collect() };

    if mirrored {
        let shown: Vec<Vec<f64>> = s.series.iter().take(2).map(|d| up(visible(d))).collect();
        let max = s
            .y_max
            .unwrap_or_else(|| shown.iter().flatten().copied().fold(0.0, f64::max).max(1.0));
        let scale = Scale { lo: 0.0, hi: max };
        let [top, bottom] =
            Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).areas(body);
        fill_columns(
            f,
            p,
            top,
            &shown[0],
            scale,
            Paint::of(p, &s.series[0], s.color),
            Direction::Up,
        );
        fill_columns(
            f,
            p,
            bottom,
            &shown[1],
            scale,
            Paint::of(p, &s.series[1], s.color),
            Direction::Down,
        );
        return;
    }
    let all: Vec<f64> = s.series.iter().flat_map(&visible).collect();
    let scale = Scale::of(&all, s);
    match s.style {
        SeriesStyle::Area => {
            if let Some(d) = s.series.first() {
                let paint = Paint::of(p, d, s.color);
                fill_columns(f, p, body, &visible(d), scale, paint, Direction::Up);
            }
        }
        SeriesStyle::Bars => {
            if let Some(d) = s.series.first() {
                let values = visible(d);
                let tail = &values[values.len().saturating_sub(body.width as usize)..];
                draw_bars_chart(f, p, body, tail, scale, Paint::of(p, d, s.color));
            }
        }
        SeriesStyle::Line => draw_lines(f, p, body, s.series, scale),
    }
}

/// A line's color: its `tone`, else the next categorical color.
fn series_color(p: &Palette, i: usize, d: &SeriesData) -> Color {
    d.tone.map(|t| p.tone(t)).unwrap_or(p.series[i % 6])
}

/// The vertical range of a chart, bottom to top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    pub lo: f64,
    pub hi: f64,
}

impl Scale {
    fn of(values: &[f64], s: &SeriesSpec) -> Self {
        let (lo, hi) = series_scale(values, s.y_min, s.y_max, s.fit);
        Scale { lo, hi }
    }

    fn span(self) -> f64 {
        self.hi - self.lo
    }

    /// Where a value sits from the bottom, 0 to 1.
    fn ratio(self, v: f64) -> f64 {
        ((v - self.lo) / self.span()).clamp(0.0, 1.0)
    }

    /// Where bars and fills start: zero, or the nearer edge when zero is off the scale.
    fn base(self) -> f64 {
        0.0_f64.clamp(self.lo, self.hi)
    }

    fn spans_zero(self) -> bool {
        self.lo < 0.0 && self.hi > 0.0
    }
}

/// What colors a graph: a color scale by height, or one color from a series' `tone`.
#[derive(Clone, Copy)]
enum Paint {
    Scale(ColorScale),
    Flat(Color),
}

impl Paint {
    fn of(p: &Palette, d: &SeriesData, scale: ColorScale) -> Self {
        match d.tone {
            Some(tone) => Paint::Flat(p.tone(tone)),
            None => Paint::Scale(scale),
        }
    }

    fn at(self, p: &Palette, t: f64) -> Color {
        match self {
            Paint::Scale(scale) => p.scale(scale, t),
            Paint::Flat(c) => c,
        }
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
/// Columns grow from the zero line (or the nearer edge); negatives hang below it in
/// whole cells.
fn draw_bars_chart(
    f: &mut Canvas,
    p: &Palette,
    area: Rect,
    values: &[f64],
    scale: Scale,
    color: Paint,
) {
    let rows = area.height as usize;
    let base = scale.base();
    // Rows under the baseline, so the baseline sits on a cell edge.
    let below = ((base - scale.lo) / scale.span() * rows as f64).round() as usize;
    let above = rows - below;
    let buf = f.buffer_mut();
    let offset = area.width as usize - values.len();
    for (i, v) in values.iter().enumerate() {
        let x = area.x + (offset + i) as u16;
        // Whole column takes the color of its tip, so bars read as one value each.
        let fg = color.at(p, scale.ratio(*v));
        if *v >= base {
            let ratio = if scale.hi > base {
                ((v - base) / (scale.hi - base)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let units = (ratio * (above * 8) as f64).round() as usize;
            for row in 0..above {
                let filled = units.saturating_sub(row * 8).min(8);
                if filled == 0 {
                    break;
                }
                let y = area.y + (above - 1 - row) as u16;
                buf[(x, y)].set_char(EIGHTHS[filled - 1]).set_fg(fg);
            }
        } else if below > 0 {
            let ratio = ((base - v) / (base - scale.lo)).clamp(0.0, 1.0);
            let cells = ((ratio * below as f64).round() as usize).max(1);
            for row in 0..cells.min(below) {
                buf[(x, area.y + (above + row) as u16)]
                    .set_char('█')
                    .set_fg(fg);
            }
        }
    }
}

/// Braille dot bits for (column, dot row), dot rows counted from the top.
const BRAILLE: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

/// Area fill in braille: two samples per cell, four dots tall, newest on the
/// right. `Up` fills from the bottom, `Down` from the top. The fill meets the
/// zero line (or the nearer edge); where the scale spans zero a faint line marks it.
fn fill_columns(
    f: &mut Canvas,
    p: &Palette,
    area: Rect,
    values: &[f64],
    scale: Scale,
    color: Paint,
    dir: Direction,
) {
    let rows = area.height as usize;
    let cols = area.width as usize;
    let dots = rows * 4;
    let dot_at = |v: f64| (scale.ratio(v) * dots as f64).round() as usize;
    let base = dot_at(scale.base());
    let buf = f.buffer_mut();
    // Right-align: the newest sample sits in the right half of the last cell.
    let offset = (cols * 2).saturating_sub(values.len());
    let mut cells = vec![0u8; cols * rows];
    for (i, v) in values.iter().enumerate() {
        let slot = offset + i;
        let (cx, half) = (slot / 2, slot % 2);
        let mut end = dot_at(*v);
        let mut start = base;
        if end == start && *v > 0.0 {
            end += 1; // show that something happened
        } else if end == start && *v < 0.0 && start > 0 {
            end -= 1;
        }
        if end < start {
            std::mem::swap(&mut end, &mut start);
        }
        for d in start..end.min(dots) {
            // Dot row counted from the top of the whole area.
            let dy = match dir {
                Direction::Up => dots - 1 - d,
                Direction::Down => d,
            };
            cells[(dy / 4) * cols + cx] |= BRAILLE[half][dy % 4];
        }
    }
    let zero_row = scale.spans_zero().then(|| {
        let at = base.min(dots - 1);
        let dy = match dir {
            Direction::Up => dots - 1 - at,
            Direction::Down => at,
        };
        dy / 4
    });
    for row in 0..rows {
        for cx in 0..cols {
            let bits = cells[row * cols + cx];
            let (x, y) = (area.x + cx as u16, area.y + row as u16);
            if bits == 0 {
                if zero_row == Some(row) {
                    buf[(x, y)].set_char('─').set_fg(p.muted);
                }
                continue;
            }
            let level = match dir {
                Direction::Up => rows - 1 - row,
                Direction::Down => row,
            };
            // Colored by height, so the top of a tall heat column is red.
            let fg = color.at(p, (level as f64 + 0.5) / rows as f64);
            let ch = char::from_u32(0x2800 + bits as u32).unwrap();
            buf[(x, y)].set_char(ch).set_fg(fg);
        }
    }
}

fn draw_lines(f: &mut Canvas, p: &Palette, area: Rect, series: &[SeriesData], scale: Scale) {
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
    let t1 = t1.max(t0 + 1.0);
    let zero = [(t0, 0.0), (t1, 0.0)];
    let mut datasets = vec![];
    if scale.spans_zero() {
        datasets.push(
            Dataset::default()
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::new().fg(p.muted))
                .data(&zero),
        );
    }
    datasets.extend(data.iter().zip(series).enumerate().map(|(i, (d, s))| {
        Dataset::default()
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::new().fg(series_color(p, i, s)))
            .data(d)
    }));
    let chart = Chart::new(datasets)
        .x_axis(Axis::default().bounds([t0, t1]))
        .y_axis(Axis::default().bounds([scale.lo, scale.hi]));
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
        let fg = m.tone.map(|t| p.tone(t)).unwrap_or(p.scale(color, ratio));
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
    let (_, shown) = bars_fit(s.label.is_some(), s.bars.len(), area.height);
    for b in &s.bars[..shown] {
        let ratio = (b.value / max).clamp(0.0, 1.0);
        let fg = b
            .status
            .map(|st| p.status(st))
            .or(b.tone.map(|t| p.tone(t)))
            .unwrap_or(p.accent);
        let label = fit::truncate(&b.label, BAR_LABEL_WIDTH - 1);
        let pad = BAR_LABEL_WIDTH - fit::width(&label);
        lines.push(Line::from(vec![
            Span::raw(format!("{label}{}", " ".repeat(pad))),
            Span::styled(bar(ratio, bar_width), Style::new().fg(fg)),
            Span::raw(format!(
                " {:>value_width$}",
                format_value(b.value, s.format, s.unit)
            )),
        ]));
    }
    if shown < s.bars.len() && lines.len() < area.height as usize {
        lines.push(Line::styled(
            format!("  +{} more", s.bars.len() - shown),
            Style::new().fg(p.muted),
        ));
    }
    f.render_widget(Paragraph::new(lines), area);
}

/// Whether a bar list shows its label, and how many bars, in `rows` rows.
/// The label goes first when there is a single row; when the bars don't all
/// fit, the last row says how many are left out ("+3 more").
pub fn bars_fit(labeled: bool, count: usize, rows: u16) -> (bool, usize) {
    let labeled = labeled && rows >= 2;
    let rows = (rows - labeled as u16) as usize;
    let shown = if count <= rows {
        count
    } else if rows >= 2 {
        rows - 1
    } else {
        rows
    };
    (labeled, shown)
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
    let fixed = fit::width(&label) + value.len();
    let width = area.width as usize;
    let (caption, bar_width) = match width.checked_sub(fixed + fit::width(&caption) + 8) {
        Some(_) => (caption.clone(), width - fixed - fit::width(&caption)),
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
    use dashboard_contract::Theme;
    use ratatui::buffer::Buffer;

    fn data(name: &str, values: &[f64]) -> SeriesData {
        SeriesData {
            tone: None,
            name: name.into(),
            points: values
                .iter()
                .enumerate()
                .map(|(i, v)| (i as i64, *v))
                .collect(),
        }
    }

    fn spec<'a>(series: &'a [SeriesData], style: SeriesStyle) -> SeriesSpec<'a> {
        SeriesSpec {
            label: "L",
            unit: Some("%"),
            y_max: None,
            y_min: None,
            fit: false,
            style,
            color: ColorScale::Accent,
            mirrored: false,
            format: ValueFormat::Plain,
            series,
        }
    }

    fn render(s: &SeriesSpec, w: u16, h: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        let p = Palette::for_theme(Theme::Dark, false);
        draw_series(&mut Canvas(&mut buf), &p, area, s);
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn scale_without_negatives_is_unchanged() {
        let d = [data("a", &[3.0, 40.0, 12.0])];
        let sp = spec(&d, SeriesStyle::Area);
        assert_eq!(
            Scale::of(&[3.0, 40.0, 12.0], &sp),
            Scale { lo: 0.0, hi: 40.0 }
        );
        assert_eq!(Scale::of(&[0.2, 0.5], &sp), Scale { lo: 0.0, hi: 1.0 });
        let capped = SeriesSpec {
            y_max: Some(100.0),
            ..spec(&d, SeriesStyle::Area)
        };
        assert_eq!(Scale::of(&[3.0], &capped), Scale { lo: 0.0, hi: 100.0 });
    }

    #[test]
    fn scale_reaches_negative_values_and_fits() {
        let d = [data("a", &[])];
        let sp = spec(&d, SeriesStyle::Line);
        assert_eq!(Scale::of(&[-2.0, 1.0], &sp), Scale { lo: -2.0, hi: 1.0 });
        let pinned = SeriesSpec {
            y_min: Some(-5.0),
            y_max: Some(5.0),
            ..spec(&d, SeriesStyle::Line)
        };
        assert_eq!(
            Scale::of(&[-2.0, 1.0], &pinned),
            Scale { lo: -5.0, hi: 5.0 }
        );
        let fit = SeriesSpec {
            fit: true,
            ..spec(&d, SeriesStyle::Line)
        };
        let s = Scale::of(&[354.0, 357.0], &fit);
        assert!(
            (s.lo - 353.85).abs() < 1e-9 && (s.hi - 357.15).abs() < 1e-9,
            "{s:?}"
        );
    }

    #[test]
    fn an_area_chart_with_negatives_fills_to_a_zero_line() {
        let d = [data("a", &[-2.0, -1.0, 1.0, 2.0])];
        let rows = render(&spec(&d, SeriesStyle::Area), 2, 9);
        let body = &rows[1..];
        // The zero line is drawn in the row where the fill meets it, in cells with no fill.
        assert!(body.iter().any(|r| r.contains('─')), "{rows:#?}");
        let line = body.iter().position(|r| r.contains('─')).unwrap();
        // Positive samples (right) fill above the line, negative ones (left) below it.
        assert!(body[..line].iter().any(|r| r.chars().nth(1) != Some(' ')));
        assert!(body[line + 1..].iter().any(|r| !r.starts_with(' ')));
        assert!(body[..line].iter().all(|r| r.starts_with(' ')));
    }

    #[test]
    fn bars_hang_below_the_zero_line() {
        let d = [data("a", &[-2.0, -2.0, 2.0, 2.0])];
        let rows = render(&spec(&d, SeriesStyle::Bars), 4, 5);
        let body = &rows[1..];
        let col = |c: usize| -> String { body.iter().map(|r| r.chars().nth(c).unwrap()).collect() };
        assert_eq!(col(0), "  ██", "{rows:#?}");
        assert_eq!(col(3), "██  ");
    }

    #[test]
    fn lines_draw_a_zero_line_only_when_spanning_zero() {
        let d = [data("a", &[-1.0, 1.0, -1.0, 1.0])];
        let spanning = render(&spec(&d, SeriesStyle::Line), 6, 5);
        let flat = [data("a", &[1.0, 2.0, 1.0, 2.0])];
        let positive = render(&spec(&flat, SeriesStyle::Line), 6, 5);
        let dots = |rows: &[String]| {
            rows[1..]
                .iter()
                .flat_map(|r| r.chars())
                .filter(|c| *c != ' ')
                .count()
        };
        assert!(dots(&spanning) > 0 && dots(&positive) > 0);
        // Zero sits mid-height: the middle body row carries the faint line.
        assert!(
            spanning[1..].iter().any(|r| r.chars().all(|c| c != ' ')),
            "{spanning:#?}"
        );
    }

    #[test]
    fn positive_area_looks_as_before() {
        // Full-scale samples fill every dot of their half cell, and no zero line appears.
        let d = [data("a", &[100.0, 100.0])];
        let sp = SeriesSpec {
            y_max: Some(100.0),
            ..spec(&d, SeriesStyle::Area)
        };
        let rows = render(&sp, 1, 3);
        assert_eq!(&rows[1..], ["⣿", "⣿"]);
    }

    #[test]
    fn legend_shows_signed_values_with_one_decimal() {
        let d = [data("SPY", &[0.3, -1.24]), data("QQQ", &[0.1, 0.04])];
        let rows = render(&spec(&d, SeriesStyle::Line), 40, 3);
        assert_eq!(rows[0].trim_end(), "L  SPY -1.2% QQQ 0.0%");
    }

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
