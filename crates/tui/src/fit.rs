//! Cutting text to a width. Text that doesn't fit ends with "…". Widths are
//! terminal cells: a wide character (CJK, most emoji) counts as two, and a
//! character is never split.

use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const ELLIPSIS: &str = "…";

/// Cells `s` takes in a terminal.
pub fn width(s: &str) -> usize {
    s.width()
}

/// `s` cut to at most `max` cells, ending with "…" when it was cut.
pub fn truncate(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    cut(s, max)
}

/// The start of `s` and "…", in at most `max` cells (none if `max` is 0).
fn cut(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for g in s.graphemes(true) {
        let w = g.width();
        if used + w > max - 1 {
            break;
        }
        out.push_str(g);
        used += w;
    }
    out.push_str(ELLIPSIS);
    out
}

/// The spans cut to at most `max` cells in all, ending with "…" (in the
/// style of the span that was cut) when anything was left out.
pub fn line(spans: Vec<Span<'_>>, max: usize) -> Line<'_> {
    let total: usize = spans.iter().map(|s| s.content.width()).sum();
    if total <= max {
        return Line::from(spans);
    }
    let mut out = vec![];
    let mut used = 0;
    for span in spans {
        let w = span.content.width();
        // Room for the span and the ellipsis after it.
        if used + w < max {
            used += w;
            out.push(span);
            continue;
        }
        // `used < max` here, so there is room for at least the ellipsis.
        out.push(Span::styled(cut(&span.content, max - used), span.style));
        break;
    }
    Line::from(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_left_alone() {
        assert_eq!(truncate("hello", 5), "hello");
        assert_eq!(truncate("", 0), "");
    }

    #[test]
    fn long_text_ends_with_an_ellipsis() {
        assert_eq!(truncate("hello world", 8), "hello w…");
        assert_eq!(truncate("hello", 1), "…");
        assert_eq!(truncate("hello", 0), "");
    }

    #[test]
    fn wide_characters_count_twice_and_are_never_split() {
        // Each of these takes two cells.
        assert_eq!(truncate("日本語のテキスト", 7), "日本語…");
        assert_eq!(width(&truncate("日本語のテキスト", 7)), 7);
        assert_eq!(truncate("日本語", 4), "日…");
        // An emoji made of several code points stays whole.
        assert_eq!(truncate("👩‍💻👩‍💻👩‍💻 dev", 5), "👩‍💻👩‍💻…");
    }

    #[test]
    fn a_line_is_cut_in_the_span_that_crosses_the_edge() {
        let spans = vec![
            Span::raw("● "),
            Span::raw("a long title"),
            Span::raw(" [tag]"),
        ];
        let cut = line(spans, 10);
        let text: String = cut.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "● a long …");
        assert_eq!(cut.width(), 10);
        // Exactly full: nothing is cut.
        let full = line(vec![Span::raw("12345")], 5);
        assert_eq!(full.spans[0].content, "12345");
        // A span that ends right at the edge with more after it is cut too.
        let cut = line(vec![Span::raw("12345"), Span::raw("678")], 5);
        let text: String = cut.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "1234…");
    }
}
