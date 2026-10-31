//! Packs tiles into rows on a 12-column grid by size class.
//!
//! A tile's rows come from its size class alone, never from its content, so
//! a long view can't push other tiles around: the view is fitted to the tile
//! instead (see `widgets`). When the rows fit in the terminal they are
//! stretched to fill it; when they don't, the page is taller than the
//! terminal and scrolls.

use dashboard_contract::Size;
use ratatui::layout::{Constraint, Layout, Rect};

const COLUMNS: u16 = 12;
/// Below this width every tile takes a full row.
const NARROW: u16 = 70;
/// Below this width tiles get wider column spans (see `columns`).
const MEDIUM: u16 = 120;
/// Terminal rows per height unit: the least a size class gets.
const ROW_UNIT: u16 = 5;

/// (width in columns, height in row units)
fn units(size: Size) -> (u16, u16) {
    match size {
        Size::Small => (3, 1),
        Size::Medium => (4, 2),
        Size::Large => (6, 3),
        Size::Wide => (8, 2),
        Size::Tall => (4, 4),
    }
}

/// Column span at this terminal width. Mid-width terminals keep two medium
/// tiles side by side but give large and wide tiles the whole row.
fn columns(size: Size, width: u16) -> u16 {
    if width < NARROW {
        return COLUMNS;
    }
    if width < MEDIUM {
        return match size {
            Size::Small => 4,
            Size::Medium | Size::Tall => 6,
            Size::Large | Size::Wide => COLUMNS,
        };
    }
    units(size).0
}

/// One `Rect` per tile, in order, with y from 0 on a page `width` wide, and
/// the page height (at least `view_height`).
pub fn place(width: u16, view_height: u16, sizes: &[Size]) -> (Vec<Rect>, u16) {
    place_with_tier(width, width, view_height, sizes)
}

/// Like `place`, but picks column spans for `tier_width`: the terminal width,
/// so taking a column for the scrollbar doesn't change how tiles pack.
pub fn place_with_tier(
    width: u16,
    tier_width: u16,
    view_height: u16,
    sizes: &[Size],
) -> (Vec<Rect>, u16) {
    if sizes.is_empty() {
        return (vec![], view_height);
    }
    let mut rows: Vec<Vec<(usize, u16)>> = vec![];
    let mut used = COLUMNS;
    for (i, size) in sizes.iter().enumerate() {
        let w = columns(*size, tier_width);
        if used + w > COLUMNS {
            rows.push(vec![]);
            used = 0;
        }
        rows.last_mut().unwrap().push((i, w));
        used += w;
    }

    let full = Rect::new(0, 0, width, 1);
    let cols: Vec<Vec<Rect>> = rows
        .iter()
        .map(|row| {
            let total: u32 = row.iter().map(|(_, w)| *w as u32).sum();
            Layout::horizontal(row.iter().map(|(_, w)| Constraint::Ratio(*w as u32, total)))
                .split(full)
                .to_vec()
        })
        .collect();

    // A row is as tall as its tallest size class.
    let units_h: Vec<u16> = rows
        .iter()
        .map(|r| r.iter().map(|(i, _)| units(sizes[*i]).1).max().unwrap_or(1))
        .collect();
    let mins: Vec<u16> = units_h.iter().map(|u| u * ROW_UNIT).collect();

    // Spare room goes to rows in proportion to their size class.
    let need: u16 = mins.iter().sum();
    let spare = view_height.saturating_sub(need);
    let unit_total: u16 = units_h.iter().sum();
    let mut heights: Vec<u16> = mins
        .iter()
        .zip(&units_h)
        .map(|(m, u)| m + spare * u / unit_total.max(1))
        .collect();
    // Rounding leftovers go to the last row so the page fills exactly.
    let page: u16 = heights.iter().sum();
    if page < view_height
        && let Some(last) = heights.last_mut()
    {
        *last += view_height - page;
    }

    let mut out = vec![Rect::default(); sizes.len()];
    let mut y = 0;
    for ((row, rects), h) in rows.iter().zip(&cols).zip(&heights) {
        for ((i, _), r) in row.iter().zip(rects) {
            out[*i] = Rect {
                y,
                height: *h,
                ..*r
            };
        }
        y += h;
    }
    (out, y.max(view_height))
}

/// The area one tile of `size` gets on a page `width` wide when it shares a
/// full row: its column span and its size class's rows. For the site's
/// previews.
#[cfg(test)]
pub fn single(size: Size, width: u16) -> Rect {
    let span = columns(size, width);
    let r = Layout::horizontal([
        Constraint::Ratio(span as u32, COLUMNS as u32),
        Constraint::Ratio((COLUMNS - span) as u32, COLUMNS as u32),
    ])
    .split(Rect::new(0, 0, width, 1))[0];
    Rect {
        height: units(size).1 * ROW_UNIT,
        ..r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_rows_and_fills_the_view() {
        // large(6) + medium(4) fit one row; wide(8) starts the next.
        let (r, page) = place(160, 40, &[Size::Large, Size::Medium, Size::Wide]);
        assert_eq!(r[0].y, r[1].y);
        assert!(r[2].y > r[0].y);
        // Leftover width is shared, so the first row spans the full width.
        assert_eq!(r[1].x + r[1].width, 160);
        assert_eq!(page, 40);
        assert_eq!(r[2].y + r[2].height, 40);
    }

    #[test]
    fn narrow_is_one_per_row() {
        let (r, _) = place(60, 40, &[Size::Small, Size::Small]);
        assert_eq!(r[0].width, 60);
        assert!(r[1].y > r[0].y);
    }

    #[test]
    fn rows_come_from_size_classes_and_the_page_grows() {
        // Six tiles, one per row on a narrow terminal with 30 rows: each gets
        // its size class's rows and the page scrolls.
        let sizes = [
            Size::Large,
            Size::Large,
            Size::Wide,
            Size::Medium,
            Size::Wide,
            Size::Small,
        ];
        let (r, page) = place(60, 30, &sizes);
        let heights: Vec<u16> = r.iter().map(|t| t.height).collect();
        assert_eq!(heights, [15, 15, 10, 10, 10, 5]);
        assert_eq!(page, 65);
    }
}
