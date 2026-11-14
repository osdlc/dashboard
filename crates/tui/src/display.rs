//! Big text: a block font built from `█▀▄`, 5 or 3 rows tall, whichever is the
//! biggest that fits. Text with a character the font lacks, or in too small
//! an area, is plain.

use dashboard_contract::Status;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::canvas::Canvas;
use crate::fit;
use crate::theme::Palette;

pub struct DisplaySpec<'a> {
    pub text: &'a str,
    pub label: Option<&'a str>,
    pub caption: Option<&'a str>,
    pub unit: Option<&'a str>,
    pub status: Option<Status>,
}

/// Rows of the big font, tallest first.
const TALL: usize = 5;
const SHORT: usize = 3;

/// One glyph in the 5-row font: every row the same width.
fn tall(c: char) -> Option<[&'static str; TALL]> {
    Some(match c {
        '0' => ["▄▀▀▄", "█  █", "█  █", "█  █", "▀▄▄▀"],
        '1' => [" ▄█ ", "  █ ", "  █ ", "  █ ", " ▄█▄"],
        '2' => ["▄▀▀▄", "   █", " ▄▀ ", "█   ", "████"],
        '3' => ["▄▀▀▄", "   █", " ▀▀▄", "   █", "▀▄▄▀"],
        '4' => ["█  █", "█  █", "▀▀▀█", "   █", "   █"],
        '5' => ["████", "█   ", "▀▀▀▄", "   █", "▀▄▄▀"],
        '6' => ["▄▀▀▄", "█   ", "█▀▀▄", "█  █", "▀▄▄▀"],
        '7' => ["████", "   █", "  ▄▀", " ▄▀ ", " █  "],
        '8' => ["▄▀▀▄", "█  █", "▄▀▀▄", "█  █", "▀▄▄▀"],
        '9' => ["▄▀▀▄", "█  █", "▀▄▄█", "   █", "▀▄▄▀"],
        ':' => [" ", "▄", " ", "▀", " "],
        '.' => [" ", " ", " ", " ", "▄"],
        '-' => ["    ", "    ", "████", "    ", "    "],
        '+' => ["   ", " █ ", "███", " █ ", "   "],
        '%' => ["█  █", "  ▄▀", " ▄▀ ", "▄▀  ", "█  █"],
        ' ' => ["  ", "  ", "  ", "  ", "  "],
        'A' => ["▄▀▀▄", "█  █", "████", "█  █", "█  █"],
        'M' => ["█▄ ▄█", "█ ▀ █", "█   █", "█   █", "█   █"],
        'P' => ["█▀▀▄", "█  █", "█▀▀▄", "█   ", "█   "],
        _ => return None,
    })
}

/// One glyph in the 3-row font.
fn short(c: char) -> Option<[&'static str; SHORT]> {
    Some(match c {
        '0' => ["█▀█", "█ █", "▀▀▀"],
        '1' => ["▄█ ", " █ ", "▀▀▀"],
        '2' => ["▀▀█", "█▀▀", "▀▀▀"],
        '3' => ["▀▀█", "▀▀█", "▀▀▀"],
        '4' => ["█ █", "▀▀█", "  ▀"],
        '5' => ["█▀▀", "▀▀█", "▀▀▀"],
        '6' => ["█▀▀", "█▀█", "▀▀▀"],
        '7' => ["▀▀█", "  █", "  ▀"],
        '8' => ["█▀█", "█▀█", "▀▀▀"],
        '9' => ["█▀█", "▀▀█", "▀▀▀"],
        ':' => ["▄", " ", "▀"],
        '.' => [" ", " ", "▀"],
        '-' => ["   ", "▀▀▀", "   "],
        '+' => [" ▄ ", "▀█▀", " ▀ "],
        '%' => ["█ ▄", " ▄▀", "▀ █"],
        ' ' => ["  ", "  ", "  "],
        'A' => ["▄▀▄", "█▀█", "▀ ▀"],
        'M' => ["█▄█", "█ █", "▀ ▀"],
        'P' => ["█▀█", "█▀▀", "▀  "],
        _ => return None,
    })
}

/// The glyph rows of `text` in one font, or `None` if a character is
/// missing. Letters are looked up in capitals.
fn render<const N: usize>(
    text: &str,
    glyph: fn(char) -> Option<[&'static str; N]>,
) -> Option<Vec<String>> {
    let mut rows = vec![String::new(); N];
    for (i, c) in text.chars().enumerate() {
        let g = glyph(c.to_ascii_uppercase())?;
        for (row, part) in rows.iter_mut().zip(g) {
            if i > 0 {
                row.push(' ');
            }
            row.push_str(part);
        }
    }
    Some(rows)
}

/// The biggest rendering that fits `width` columns and `height` rows:
/// the rows, and the font height. The unit goes after the last row, one
/// column on, so it counts toward the width.
fn biggest(text: &str, unit: Option<&str>, width: u16, height: u16) -> Option<Vec<String>> {
    let extra = unit.map_or(0, |u| 1 + fit::width(u));
    let fits = |rows: &[String]| !text.is_empty() && fit::width(&rows[0]) + extra <= width as usize;
    if height as usize >= TALL
        && let Some(rows) = render(text, tall).filter(|r| fits(r))
    {
        return Some(rows);
    }
    if height as usize >= SHORT
        && let Some(rows) = render(text, short).filter(|r| fits(r))
    {
        return Some(rows);
    }
    None
}

/// Rows a display wants inside a column: the big font and its extras.
pub fn wanted_height(labeled: bool, captioned: bool) -> u16 {
    TALL as u16 + labeled as u16 + captioned as u16
}

pub fn draw_display(f: &mut Canvas, p: &Palette, area: Rect, s: &DisplaySpec) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let value = Style::new()
        .add_modifier(Modifier::BOLD)
        .fg(p.opt_status(s.status));
    let muted = Style::new().fg(p.muted);
    // The label and caption each take a row once the value has the room.
    let find = |h: u16| biggest(s.text, s.unit, area.width, h);
    let mut lines: Vec<Line> = vec![];
    if let Some(rows) = find(area.height) {
        let used = rows.len() as u16;
        let spare = area.height - used;
        let label = s.label.filter(|_| spare >= 1);
        let caption = s.caption.filter(|_| spare > label.is_some() as u16);
        // A little space around the numbers when there is plenty.
        let pad = (spare - label.is_some() as u16 - caption.is_some() as u16) / 2;
        let w = rows.iter().map(|r| fit::width(r)).max().unwrap_or(0)
            + s.unit.map_or(0, |u| 1 + fit::width(u));
        let left = (area.width as usize).saturating_sub(w) / 2;
        let indent = " ".repeat(left);
        let center = |t: &str| {
            let t = fit::truncate(t, area.width as usize);
            let gap = (area.width as usize).saturating_sub(fit::width(&t)) / 2;
            Line::styled(format!("{}{t}", " ".repeat(gap)), muted)
        };
        lines.extend((0..pad).map(|_| Line::raw("")));
        if let Some(l) = label {
            lines.push(center(l));
        }
        for (i, row) in rows.iter().enumerate() {
            let mut spans = vec![Span::raw(indent.clone()), Span::styled(row.clone(), value)];
            if i + 1 == rows.len()
                && let Some(u) = s.unit
            {
                spans.push(Span::styled(format!(" {u}"), muted));
            }
            lines.push(Line::from(spans));
        }
        if let Some(c) = caption {
            lines.push(center(c));
        }
    } else {
        // Plain: the text and its unit, with the label before and the caption after.
        let mut spans = vec![];
        if let Some(l) = s.label {
            spans.push(Span::styled(format!("{l} "), muted));
        }
        spans.push(Span::styled(s.text.to_string(), value));
        if let Some(u) = s.unit {
            spans.push(Span::raw(format!(" {u}")));
        }
        lines.push(Line::from(spans));
        if let Some(c) = s.caption.filter(|_| area.height > 1) {
            lines.push(Line::styled(c.to_string(), muted));
        }
    }
    f.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
mod tests {
    use dashboard_contract::Theme;
    use ratatui::buffer::Buffer;

    use super::*;

    fn draw(spec: &DisplaySpec, w: u16, h: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        let p = Palette::for_theme(Theme::Dark, false);
        draw_display(&mut Canvas(&mut buf), &p, area, spec);
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    fn spec(text: &str) -> DisplaySpec<'_> {
        DisplaySpec {
            text,
            label: Some("Tokyo"),
            caption: Some("Tue 27 Oct"),
            unit: None,
            status: None,
        }
    }

    #[test]
    fn every_glyph_is_a_rectangle() {
        for c in "0123456789:.-+% AMP".chars() {
            let t = tall(c).unwrap();
            assert!(
                t.iter().all(|r| r.chars().count() == t[0].chars().count()),
                "{c} tall"
            );
            let s = short(c).unwrap();
            assert!(
                s.iter().all(|r| r.chars().count() == s[0].chars().count()),
                "{c} short"
            );
        }
        assert!(tall('k').is_none() && short('k').is_none());
    }

    #[test]
    fn five_rows_when_they_fit() {
        let rows = draw(&spec("14:05"), 40, 7);
        assert_eq!(rows[0].trim(), "Tokyo");
        assert_eq!(rows[6].trim(), "Tue 27 Oct");
        assert!(rows[1..6].iter().all(|r| !r.is_empty()), "{rows:#?}");
        assert_eq!(rows[1].trim(), "▄█  █  █   ▄▀▀▄ ████");
        // 21 columns wide, centered in 40; the "1" starts with a space.
        assert_eq!(
            rows[1].chars().take_while(|c| *c == ' ').count(),
            10,
            "{rows:#?}"
        );
    }

    #[test]
    fn three_rows_when_five_do_not_fit() {
        let rows = draw(&spec("14:05"), 40, 4);
        assert_eq!(rows[0].trim(), "Tokyo");
        assert_eq!(rows[1].trim(), "▄█  █ █ ▄ █▀█ █▀▀");
        assert_eq!(rows.len(), 4);
        // Exactly three rows: no label or caption.
        let tight = draw(&spec("14:05"), 40, 3);
        assert!(tight.iter().all(|r| !r.contains("Tokyo")), "{tight:#?}");
        assert_eq!(tight[0].trim(), "▄█  █ █ ▄ █▀█ █▀▀");
    }

    #[test]
    fn narrow_areas_use_the_small_font_then_plain_text() {
        // 5 wide digits need 21 columns; 3-row ones need 16.
        let rows = draw(&spec("14:05"), 17, 6);
        assert!(rows[1].contains('█') && rows.len() == 6);
        let plain = draw(&spec("14:05"), 10, 6);
        assert_eq!(plain[0], "Tokyo 14:0");
    }

    #[test]
    fn unit_follows_the_last_row() {
        let mut s = spec("2:05");
        s.unit = Some("PM");
        let rows = draw(&s, 40, 7);
        assert!(rows[5].ends_with(" PM"), "{rows:#?}");
    }

    #[test]
    fn a_missing_character_makes_the_whole_text_plain() {
        let rows = draw(&spec("12 kB"), 40, 7);
        assert_eq!(rows[0], "Tokyo 12 kB");
        assert_eq!(rows[1], "Tue 27 Oct");
    }
}
