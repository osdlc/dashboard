//! The analog clock face: a braille dial with hour ticks and hands. Too small
//! for a dial, it is the time as text.

use dashboard_contract::Status;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas as Plot, Circle, Line as Seg};
use ratatui::widgets::{Paragraph, Widget};

use crate::canvas::Canvas;
use crate::fit;
use crate::theme::Palette;

pub struct FaceSpec<'a> {
    pub hour: u8,
    pub minute: u8,
    pub second: Option<u8>,
    pub label: Option<&'a str>,
    pub caption: Option<&'a str>,
    pub status: Option<Status>,
}

/// Narrowest and shortest area a dial is drawn in; smaller shows text.
const MIN_COLS: u16 = 10;
const MIN_ROWS: u16 = 5;
/// Rows the dial wants in a column, without its label and caption.
const WANTED_ROWS: u16 = 9;
/// Rows the dial needs before a label or caption is worth a row.
const ROWS_FOR_TEXT: u16 = 7;

/// "14:05".
pub fn time_text(hour: u8, minute: u8) -> String {
    format!("{hour:02}:{minute:02}")
}

/// Rows a face wants inside a column.
pub fn wanted_height(labeled: bool, captioned: bool) -> u16 {
    WANTED_ROWS + labeled as u16 + captioned as u16
}

/// Point at `turn` (a fraction of a full turn clockwise from twelve) and
/// `len` from the center, in a plane where up is +y.
fn at(turn: f64, len: f64) -> (f64, f64) {
    let a = turn * std::f64::consts::TAU;
    (a.sin() * len, a.cos() * len)
}

/// Hand positions as a fraction of a turn: hour, minute and second.
pub fn turns(hour: u8, minute: u8, second: Option<u8>) -> (f64, f64, Option<f64>) {
    let (h, m) = (f64::from(hour % 12), f64::from(minute));
    let s = second.map(f64::from);
    (
        (h + m / 60.0) / 12.0,
        (m + s.unwrap_or(0.0) / 60.0) / 60.0,
        s.map(|s| s / 60.0),
    )
}

pub fn draw_face(f: &mut Canvas, p: &Palette, area: Rect, s: &FaceSpec) {
    if area.width < MIN_COLS || area.height < MIN_ROWS {
        draw_text(f, p, area, s);
        return;
    }
    let mut area = area;
    let mut label = None;
    let mut caption = None;
    // Text lines only while the dial keeps ROWS_FOR_TEXT or more rows.
    if s.label.is_some() && area.height > ROWS_FOR_TEXT {
        label = s.label;
        area.y += 1;
        area.height -= 1;
    }
    if s.caption.is_some() && area.height > ROWS_FOR_TEXT {
        caption = s.caption;
        area.height -= 1;
    }
    let full = area;
    // The dial is twice as wide as tall in cells, which is round on screen.
    let rows = area.height.min(area.width / 2);
    let dial = Rect {
        x: area.x + (area.width - rows * 2) / 2,
        y: area.y + (area.height - rows) / 2,
        width: rows * 2,
        height: rows,
    };
    if let Some(label) = label {
        centered(
            f,
            Line::styled(label.to_string(), Style::new().fg(p.muted)),
            full,
            full.y - 1,
        );
    }
    if let Some(caption) = caption {
        centered(
            f,
            Line::styled(caption.to_string(), Style::new().fg(p.muted)),
            full,
            full.bottom(),
        );
    }

    let rim = s.status.map_or(p.muted, |st| p.status(st));
    let (hour, minute, second) = turns(s.hour, s.minute, s.second);
    let plot = Plot::default()
        .marker(Marker::Braille)
        .x_bounds([-1.0, 1.0])
        .y_bounds([-1.0, 1.0])
        .paint(move |ctx| {
            ctx.draw(&Circle {
                x: 0.0,
                y: 0.0,
                radius: 0.97,
                color: rim,
            });
            for i in 0..12 {
                let quarter = i % 3 == 0;
                let (x1, y1) = at(f64::from(i) / 12.0, if quarter { 0.74 } else { 0.84 });
                let (x2, y2) = at(f64::from(i) / 12.0, 0.92);
                ctx.draw(&Seg {
                    x1,
                    y1,
                    x2,
                    y2,
                    color: if quarter { Color::Reset } else { p.muted },
                });
            }
            ctx.layer();
            for (turn, len, color) in [(hour, 0.5, Color::Reset), (minute, 0.78, Color::Reset)] {
                let (x2, y2) = at(turn, len);
                ctx.draw(&Seg {
                    x1: 0.0,
                    y1: 0.0,
                    x2,
                    y2,
                    color,
                });
            }
            if let Some(turn) = second {
                ctx.layer();
                let (x2, y2) = at(turn, 0.86);
                ctx.draw(&Seg {
                    x1: 0.0,
                    y1: 0.0,
                    x2,
                    y2,
                    color: p.accent,
                });
            }
        });
    f.render_widget(plot, dial);
}

/// One line centered in `within`'s width, at row `y`.
fn centered(f: &mut Canvas, line: Line, within: Rect, y: u16) {
    let w = (line.width() as u16).min(within.width);
    f.render_widget(
        Paragraph::new(line),
        Rect {
            x: within.x + (within.width - w) / 2,
            y,
            width: w,
            height: 1,
        },
    );
}

/// "Tokyo 14:05", the label cut to fit, then the caption if there is a row.
fn draw_text(f: &mut Canvas, p: &Palette, area: Rect, s: &FaceSpec) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let time = time_text(s.hour, s.minute);
    let room = (area.width as usize).saturating_sub(time.len() + 1);
    let mut spans = vec![];
    if let Some(label) = s.label.filter(|_| room > 0) {
        spans.push(Span::styled(
            format!("{} ", fit::truncate(label, room)),
            Style::new().fg(p.muted),
        ));
    }
    spans.push(Span::styled(
        time,
        Style::new().fg(s.status.map_or(Color::Reset, |st| p.status(st))),
    ));
    let mut lines = vec![Line::from(spans)];
    if let Some(caption) = s.caption.filter(|_| area.height > 1) {
        lines.push(Line::styled(
            fit::truncate(caption, area.width as usize),
            Style::new().fg(p.muted),
        ));
    }
    Paragraph::new(lines).render(area, f.buffer_mut());
}

#[cfg(test)]
mod tests {
    use dashboard_contract::Theme;
    use ratatui::buffer::Buffer;

    use super::*;

    fn draw(spec: &FaceSpec, w: u16, h: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        let p = Palette::for_theme(Theme::Dark, false);
        draw_face(&mut Canvas(&mut buf), &p, area, spec);
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    fn spec(hour: u8, minute: u8, second: Option<u8>) -> FaceSpec<'static> {
        FaceSpec {
            hour,
            minute,
            second,
            label: Some("Tokyo"),
            caption: Some("Tue 27 Oct"),
            status: None,
        }
    }

    fn ink(rows: &[String]) -> usize {
        rows.iter()
            .flat_map(|r| r.chars())
            .filter(|c| ('\u{2801}'..='\u{28ff}').contains(c))
            .count()
    }

    #[test]
    fn hands_turn_with_the_time() {
        let (h, m, s) = turns(3, 0, None);
        assert!((h - 0.25).abs() < 1e-9 && m == 0.0 && s.is_none());
        // The hour hand moves with the minutes: half past 2 is 2.5/12.
        assert!((turns(14, 30, None).0 - 2.5 / 12.0).abs() < 1e-9);
        assert!((turns(0, 15, Some(30)).2.unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn dial_with_label_and_caption() {
        let rows = draw(&spec(14, 5, Some(9)), 30, 11);
        assert_eq!(rows[0].trim(), "Tokyo");
        assert_eq!(rows[10].trim(), "Tue 27 Oct");
        // A 9-row dial, 18 cells wide, in the middle.
        assert!(ink(&rows[1..10]) > 40, "{rows:#?}");
        assert!(
            rows[1..10]
                .iter()
                .all(|r| r.chars().take(5).all(|c| c == ' ')),
            "{rows:#?}"
        );
    }

    #[test]
    fn hands_point_where_the_time_says() {
        // Three o'clock: the right half has more ink than the left, and the
        // hand reaches the right of the center row.
        let rows = draw(&spec(3, 0, None), 20, 9);
        let mid: Vec<char> = rows[4].chars().collect();
        let left = mid[..10].iter().filter(|c| **c != ' ').count();
        let right = mid[10..].iter().filter(|c| **c != ' ').count();
        assert!(right > left, "{rows:#?}");
        // Without a second hand the picture differs from one with it.
        assert_ne!(draw(&spec(3, 0, Some(30)), 20, 9), rows);
    }

    #[test]
    fn small_areas_show_text() {
        let rows = draw(&spec(14, 5, None), 14, 4);
        assert_eq!(rows[0].trim_end(), "Tokyo 14:05");
        assert_eq!(rows[1].trim_end(), "Tue 27 Oct");
        let narrow = draw(&spec(14, 5, None), 9, 9);
        assert_eq!(ink(&narrow), 0);
        assert_eq!(narrow[0].trim_end(), "To… 14:05");
    }

    #[test]
    fn short_dial_has_no_label_rows() {
        let rows = draw(&spec(14, 5, None), 30, 7);
        assert!(rows.iter().all(|r| !r.contains("Tokyo")), "{rows:#?}");
        assert!(ink(&rows) > 40);
    }
}
