//! Pie and donut charts. With room, a braille disc with the legend beside
//! it; in a small area, one stacked bar with the legend packed below.

use dashboard_contract::{ColorScale, Slice, ValueFormat, format_value};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::canvas::Canvas;
use crate::fit;
use crate::theme::Palette;

pub struct PieSpec<'a> {
    pub label: Option<&'a str>,
    pub slices: &'a [Slice],
    pub donut: bool,
    pub format: ValueFormat,
    pub unit: Option<&'a str>,
    pub color: ColorScale,
}

/// Fewest rows for the disc; below this the stacked bar is used.
const MIN_DISC_ROWS: u16 = 4;
/// Narrowest legend worth putting beside the disc: "● label  100%".
const MIN_LEGEND: u16 = 12;
/// The longest label the legend shows in full.
const MAX_LABEL: usize = 18;
/// Inner radius of the donut hole, as a share of the outer radius.
const HOLE: f64 = 0.55;

/// Rows a pie needs to be usable at this width: the stacked bar and its
/// packed legend.
pub fn min_height(slices: &[Slice], labeled: bool, width: u16) -> u16 {
    // Each entry as "● label 100%", the widest a percent gets.
    let lens = slices
        .iter()
        .map(|s| 2 + fit::width(&s.label).min(MAX_LABEL) + 5);
    1 + packed_rows(lens, width as usize).max(1) + labeled as u16
}

/// Lines needed to pack items of these lengths, two spaces apart.
fn packed_rows(lens: impl Iterator<Item = usize>, width: usize) -> u16 {
    let (mut rows, mut used) = (0, 0);
    for len in lens {
        if used == 0 || used + 2 + len > width {
            rows += 1;
            used = len;
        } else {
            used += 2 + len;
        }
    }
    rows
}

/// Rows a pie wants inside a column: a small disc, or one row per slice.
pub fn wanted_height(slices: usize, labeled: bool) -> u16 {
    (slices as u16).clamp(MIN_DISC_ROWS, 8) + labeled as u16
}

pub fn draw_pie(f: &mut Canvas, p: &Palette, area: Rect, s: &PieSpec) {
    let mut area = area;
    if let Some(label) = s.label {
        f.render_widget(
            Paragraph::new(label).style(Style::new().fg(p.muted)),
            Rect { height: 1, ..area },
        );
        area.y += 1;
        area.height = area.height.saturating_sub(1);
    }
    if area.height == 0 || area.width == 0 {
        return;
    }
    let total: f64 = s.slices.iter().map(|x| value(x.value)).sum();
    if total <= 0.0 {
        f.render_widget(
            Paragraph::new("no data").style(Style::new().fg(p.muted)),
            area,
        );
        return;
    }
    let shares: Vec<f64> = s.slices.iter().map(|x| value(x.value) / total).collect();
    let colors: Vec<Color> = (0..s.slices.len())
        .map(|i| slice_color(p, s.color, i, s.slices))
        .collect();
    let entries: Vec<Entry> = s
        .slices
        .iter()
        .zip(&shares)
        .zip(&colors)
        .map(|((slice, share), color)| Entry {
            label: &slice.label,
            value: format_value(slice.value, s.format, s.unit),
            percent: percent(*share),
            color: *color,
        })
        .collect();

    // The disc is twice as wide as tall in cells, which is round on screen.
    let legend_w = legend_width(&entries);
    let rows = area
        .height
        .min(area.width.saturating_sub(MIN_LEGEND + 2) / 2);
    // Only when the legend beside it can show every slice, up to eight.
    let need = MIN_DISC_ROWS.max(entries.len().min(8) as u16);
    if rows >= need {
        // Give the legend its full width first, if the disc can stay big enough.
        let rows = rows.min((area.width.saturating_sub(legend_w + 2) / 2).max(need));
        let disc = Rect {
            width: rows * 2,
            height: rows,
            ..area
        };
        draw_disc(f, disc, &shares, &colors, s.donut);
        let legend = Rect {
            x: disc.right() + 2,
            width: area.width - disc.width - 2,
            ..area
        };
        let lines = legend_lines(p, &entries, legend.width, rows);
        // Centered beside the disc.
        let top = (rows - lines.len() as u16) / 2;
        f.render_widget(
            Paragraph::new(lines),
            Rect {
                y: legend.y + top,
                height: legend.height - top,
                ..legend
            },
        );
    } else {
        draw_stacked(f, Rect { height: 1, ..area }, &shares, &colors);
        let below = Rect {
            y: area.y + 1,
            height: area.height - 1,
            ..area
        };
        if below.height as usize >= entries.len() {
            let lines = legend_lines(p, &entries, below.width, below.height);
            f.render_widget(Paragraph::new(lines), below);
        } else {
            f.render_widget(Paragraph::new(packed(p, &entries, below.width)), below);
        }
    }
}

/// Negative and non-finite values count as zero.
fn value(v: f64) -> f64 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

fn percent(share: f64) -> String {
    let pc = share * 100.0;
    if pc > 0.0 && pc < 1.0 {
        "<1%".into()
    } else {
        format!("{pc:.0}%")
    }
}

/// A status wins, then the slice's tone; otherwise `accent` gives each slice
/// the next categorical color (`series-1`, `series-2`, ...) and the other
/// scales spread the slices from one end to the other.
pub fn slice_color(p: &Palette, scale: ColorScale, i: usize, slices: &[Slice]) -> Color {
    if let Some(st) = slices[i].status {
        return p.status(st);
    }
    if let Some(tone) = slices[i].tone {
        return p.tone(tone);
    }
    match scale {
        ColorScale::Accent => p.series[i % p.series.len()],
        _ => {
            let t = if slices.len() > 1 {
                i as f64 / (slices.len() - 1) as f64
            } else {
                0.5
            };
            p.scale(scale, t)
        }
    }
}

/// Braille dot bits for (column, dot row), dot rows counted from the top.
const BRAILLE: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

/// The disc in braille, slices clockwise from twelve o'clock. A cell shared
/// by two slices takes the color of the one with more dots in it.
fn draw_disc(f: &mut Canvas, area: Rect, shares: &[f64], colors: &[Color], donut: bool) {
    let (w, h) = (area.width as usize * 2, area.height as usize * 4);
    let r = w.min(h) as f64 / 2.0;
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let inner = if donut { r * HOLE } else { 0.0 };
    // Where each slice ends, as a fraction of the turn.
    let ends: Vec<f64> = shares
        .iter()
        .scan(0.0, |acc, s| {
            *acc += s;
            Some(*acc)
        })
        .collect();
    let buf = f.buffer_mut();
    for row in 0..area.height as usize {
        for col in 0..area.width as usize {
            let mut bits = 0u8;
            let mut counts = vec![0u8; shares.len()];
            for (dx, dots) in BRAILLE.iter().enumerate() {
                for (dy, bit) in dots.iter().enumerate() {
                    let x = (col * 2 + dx) as f64 + 0.5 - cx;
                    let y = (row * 4 + dy) as f64 + 0.5 - cy;
                    let d = (x * x + y * y).sqrt();
                    if d > r || d < inner {
                        continue;
                    }
                    // Clockwise from straight up, 0..1.
                    let turn = (x.atan2(-y) / std::f64::consts::TAU).rem_euclid(1.0);
                    let i = ends
                        .iter()
                        .position(|e| turn < *e)
                        .unwrap_or(shares.len() - 1);
                    bits |= bit;
                    counts[i] += 1;
                }
            }
            if bits == 0 {
                continue;
            }
            let most = (0..counts.len()).max_by_key(|i| counts[*i]).unwrap_or(0);
            let pos = (area.x + col as u16, area.y + row as u16);
            if buf.area.contains(pos.into()) {
                buf[pos]
                    .set_char(char::from_u32(0x2800 + bits as u32).unwrap())
                    .set_fg(colors[most]);
            }
        }
    }
}

/// One full-width bar split into the slices, in order.
fn draw_stacked(f: &mut Canvas, area: Rect, shares: &[f64], colors: &[Color]) {
    let w = area.width as f64;
    let mut spans = vec![];
    let (mut acc, mut from) = (0.0, 0usize);
    for (share, color) in shares.iter().zip(colors) {
        acc += share;
        let to = (acc * w).round() as usize;
        if to > from {
            spans.push(Span::styled("█".repeat(to - from), Style::new().fg(*color)));
        }
        from = from.max(to);
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

struct Entry<'a> {
    label: &'a str,
    value: String,
    percent: String,
    color: Color,
}

/// Width of a legend line with nothing cut.
fn legend_width(entries: &[Entry]) -> u16 {
    let label = entries
        .iter()
        .map(|e| fit::width(e.label))
        .max()
        .unwrap_or(0)
        .min(MAX_LABEL);
    let value = entries
        .iter()
        .map(|e| e.value.chars().count())
        .max()
        .unwrap_or(0);
    (2 + label + 2 + value + 2 + 4) as u16
}

/// "● label   value   42%", one per slice, at most `rows` of them: the last
/// becomes "+3 more" when they don't fit. The value goes first when narrow,
/// then the label is cut.
fn legend_lines<'a>(p: &Palette, entries: &[Entry], width: u16, rows: u16) -> Vec<Line<'a>> {
    let width = width as usize;
    let value_w = entries
        .iter()
        .map(|e| e.value.chars().count())
        .max()
        .unwrap_or(0);
    let label_w = entries
        .iter()
        .map(|e| fit::width(e.label))
        .max()
        .unwrap_or(0)
        .min(MAX_LABEL);
    // "● " + label + "  " + value + "  " + "100%"
    let with_value = 2 + 2 + value_w + 2 + 4;
    let show_value = width >= with_value + label_w.min(6);
    let fixed = if show_value { with_value } else { 2 + 2 + 4 };
    let label_w = label_w.min(width.saturating_sub(fixed));

    let rows = rows as usize;
    let shown = if entries.len() > rows {
        rows.saturating_sub(1)
    } else {
        entries.len()
    };
    let mut lines: Vec<Line> = entries[..shown]
        .iter()
        .map(|e| {
            let label = fit::truncate(e.label, label_w);
            let pad = label_w.saturating_sub(fit::width(&label));
            let mut spans = vec![
                Span::styled("● ", Style::new().fg(e.color)),
                Span::raw(format!("{label}{}  ", " ".repeat(pad))),
            ];
            if show_value {
                spans.push(Span::raw(format!("{:>value_w$}  ", e.value)));
            }
            spans.push(Span::styled(
                format!("{:>4}", e.percent),
                Style::new().fg(p.muted),
            ));
            Line::from(spans)
        })
        .collect();
    if shown < entries.len() && rows > 0 {
        lines.push(Line::styled(
            format!("  +{} more", entries.len() - shown),
            Style::new().fg(p.muted),
        ));
    }
    lines
}

/// Legend entries side by side, "● label 42%", wrapped at the width.
fn packed<'a>(p: &Palette, entries: &[Entry], width: u16) -> Vec<Line<'a>> {
    let width = width as usize;
    let mut lines = vec![];
    let mut line: Vec<Span> = vec![];
    let mut used = 0;
    for e in entries {
        let label = fit::truncate(e.label, MAX_LABEL);
        let text = format!("{label} {}", e.percent);
        let len = 2 + fit::width(&text);
        let gap = if used == 0 { 0 } else { 2 };
        if used > 0 && used + gap + len > width {
            lines.push(Line::from(std::mem::take(&mut line)));
            used = 0;
        } else if gap > 0 {
            line.push(Span::raw("  "));
            used += gap;
        }
        line.push(Span::styled("● ", Style::new().fg(e.color)));
        line.push(Span::styled(text, Style::new().fg(p.muted)));
        used += len;
    }
    if !line.is_empty() {
        lines.push(Line::from(line));
    }
    lines
}

#[cfg(test)]
mod tests {
    use dashboard_contract::{Status, Theme};
    use ratatui::buffer::Buffer;

    use super::*;

    fn slices(values: &[(&str, f64)]) -> Vec<Slice> {
        values
            .iter()
            .map(|(l, v)| Slice {
                tone: None,
                label: (*l).into(),
                value: *v,
                status: None,
            })
            .collect()
    }

    fn draw(slices: &[Slice], label: Option<&str>, w: u16, h: u16) -> (Buffer, Vec<String>) {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        let p = Palette::for_theme(Theme::Dark, false);
        let spec = PieSpec {
            label,
            slices,
            donut: true,
            format: ValueFormat::Bytes,
            unit: None,
            color: ColorScale::Accent,
        };
        draw_pie(&mut Canvas(&mut buf), &p, area, &spec);
        let rows = (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect())
            .collect();
        (buf, rows)
    }

    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

    #[test]
    fn disc_with_legend_beside_it() {
        let s = slices(&[
            ("photos", 60.0 * GIB),
            ("apps", 30.0 * GIB),
            ("other", 10.0 * GIB),
        ]);
        let (buf, rows) = draw(&s, Some("Disk"), 50, 9);
        assert_eq!(rows[0].trim_end(), "Disk");
        // 8 rows of disc, 16 cells wide, then the legend centered beside it.
        assert!(
            rows[1..]
                .iter()
                .all(|r| r.chars().take(16).any(|c| c != ' '))
        );
        let legend: Vec<&str> = rows.iter().map(|r| r.trim_end()).collect();
        assert!(
            legend[3].ends_with("● photos  60.0 GiB   60%"),
            "{legend:#?}"
        );
        assert!(
            legend[4].ends_with("● apps    30.0 GiB   30%"),
            "{legend:#?}"
        );
        assert!(
            legend[5].ends_with("● other   10.0 GiB   10%"),
            "{legend:#?}"
        );
        // Braille, and the top right (start of the turn) is the first slice's color.
        let p = Palette::for_theme(Theme::Dark, false);
        let top_right = &buf[(9, 1)];
        assert!(('\u{2800}'..='\u{28FF}').contains(&top_right.symbol().chars().next().unwrap()));
        assert_eq!(top_right.fg, p.series[0]);
        // The donut hole stays empty.
        assert_eq!(buf[(8, 5)].symbol(), " ");
    }

    #[test]
    fn small_area_falls_back_to_a_stacked_bar() {
        let s = slices(&[("a", 3.0), ("b", 1.0)]);
        let (buf, rows) = draw(&s, None, 20, 3);
        assert_eq!(rows[0], "████████████████████");
        let p = Palette::for_theme(Theme::Dark, false);
        assert_eq!(buf[(0, 0)].fg, p.accent);
        assert_eq!(buf[(14, 0)].fg, p.accent);
        assert_ne!(buf[(15, 0)].fg, p.accent);
        assert_eq!(rows[1].trim_end(), "● a  3 B   75%");
        assert_eq!(rows[2].trim_end(), "● b  1 B   25%");
    }

    #[test]
    fn tiny_area_packs_the_legend() {
        let s = slices(&[("alpha", 1.0), ("beta", 1.0), ("gamma", 2.0)]);
        let (_, rows) = draw(&s, None, 24, 2);
        assert_eq!(rows[0].chars().filter(|c| *c == '█').count(), 24);
        assert_eq!(rows[1].trim_end(), "● alpha 25%  ● beta 25%");
        let (_, rows) = draw(&s, None, 6, 1);
        assert_eq!(rows[0], "██████");
    }

    #[test]
    fn many_slices_end_with_more() {
        let names: Vec<String> = (0..10).map(|i| format!("s{i}")).collect();
        let values: Vec<(&str, f64)> = names.iter().map(|n| (n.as_str(), 1.0)).collect();
        let (_, rows) = draw(&slices(&values), None, 40, 8);
        let text = rows.join("\n");
        assert!(text.contains('⣿'), "{text}");
        assert!(text.contains("● s6"), "{text}");
        assert!(!text.contains("● s7"), "{text}");
        assert!(text.contains("+3 more"), "{text}");
    }

    #[test]
    fn min_height_fits_the_packed_legend() {
        let s = slices(&[("alpha", 1.0), ("beta", 1.0), ("gamma", 2.0)]);
        assert_eq!(min_height(&s, false, 60), 2);
        assert_eq!(min_height(&s, true, 24), 5);
        assert_eq!(min_height(&s, false, 5), 4);
        assert_eq!(min_height(&[], false, 20), 2);
    }

    #[test]
    fn nothing_to_show() {
        let (_, rows) = draw(&slices(&[("a", 0.0)]), None, 20, 4);
        assert_eq!(rows[0].trim_end(), "no data");
        let (_, rows) = draw(&[], None, 20, 4);
        assert_eq!(rows[0].trim_end(), "no data");
    }

    #[test]
    fn status_and_scales_pick_colors() {
        let p = Palette::for_theme(Theme::Dark, false);
        let mut s = slices(&[("a", 1.0), ("b", 1.0), ("c", 1.0)]);
        s[1].status = Some(Status::Error);
        assert_eq!(slice_color(&p, ColorScale::Accent, 1, &s), p.error);
        assert_eq!(slice_color(&p, ColorScale::Accent, 0, &s), p.accent);
        assert_ne!(slice_color(&p, ColorScale::Accent, 2, &s), p.accent);
        assert_eq!(
            slice_color(&p, ColorScale::Heat, 2, &s),
            p.scale(ColorScale::Heat, 1.0)
        );
        assert_eq!(percent(0.004), "<1%");
        assert_eq!(percent(0.5), "50%");
    }
}
