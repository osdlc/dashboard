//! Places tiles on the 12-column grid: the web view's CSS grid (`.grid` in
//! `web/src/style.css`, `grid-auto-flow: dense`) worked out by hand, so every
//! tile's box is known before drawing. Fitting and arrow keys both use it.

use sitroom_contract::Size;

use crate::nav::Bounds;

const COLUMNS: usize = 12;

/// (columns, rows) a tile spans on a window this wide.
fn span(size: Size, window_width: f32) -> (usize, usize) {
    let (columns, rows) = match size {
        Size::Small => (3, 1),
        Size::Medium => (4, 2),
        Size::Large => (6, 3),
        Size::Wide => (8, 2),
        Size::Tall => (4, 4),
    };
    (if window_width <= 1100.0 { 6 } else { columns }, rows)
}

/// One box per tile in order, and the height of the page.
pub fn place(
    sizes: &[Size],
    width: f32,
    window_width: f32,
    gap: f32,
    row: f32,
) -> (Vec<Bounds>, f32) {
    let columns = if window_width <= 700.0 { 1 } else { COLUMNS };
    let cell = (width - gap * (columns - 1) as f32) / columns as f32;
    let mut used: Vec<[bool; COLUMNS]> = vec![];
    let mut boxes = Vec::with_capacity(sizes.len());
    let mut rows_used = 0;
    for size in sizes {
        let (w, h) = span(*size, window_width);
        let w = w.min(columns);
        let (x, y) = first_free(&mut used, columns, w, h);
        for line in &mut used[y..y + h] {
            line[x..x + w].fill(true);
        }
        rows_used = rows_used.max(y + h);
        boxes.push(Bounds {
            x: x as f32 * (cell + gap),
            y: y as f32 * (row + gap),
            width: cell * w as f32 + gap * (w - 1) as f32,
            height: row * h as f32 + gap * (h - 1) as f32,
        });
    }
    let height = if rows_used == 0 {
        0.0
    } else {
        rows_used as f32 * (row + gap) - gap
    };
    (boxes, height)
}

/// The first spot, top left first, with room for `w` by `h` cells.
fn first_free(
    used: &mut Vec<[bool; COLUMNS]>,
    columns: usize,
    w: usize,
    h: usize,
) -> (usize, usize) {
    for y in 0.. {
        if used.len() < y + h {
            used.resize(y + h, [false; COLUMNS]);
        }
        for x in 0..=columns - w {
            let free = used[y..y + h]
                .iter()
                .all(|line| line[x..x + w].iter().all(|c| !c));
            if free {
                return (x, y);
            }
        }
    }
    unreachable!("the grid grows until a spot is free")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_like_the_css_grid() {
        let sizes = [Size::Large, Size::Medium, Size::Small, Size::Wide];
        let (b, height) = place(&sizes, 1200.0, 1400.0, 0.0, 100.0);
        assert_eq!(
            (b[0].x, b[0].y, b[0].width, b[0].height),
            (0.0, 0.0, 600.0, 300.0)
        );
        assert_eq!((b[1].x, b[1].y), (600.0, 0.0));
        // Dense: the small tile takes the hole below the medium one.
        assert_eq!((b[2].x, b[2].y), (600.0, 200.0));
        // Too wide for the holes beside the large tile: below it.
        assert_eq!((b[3].x, b[3].y), (0.0, 300.0));
        assert_eq!(height, 500.0);
    }

    #[test]
    fn a_narrow_window_has_one_column() {
        let (b, _) = place(&[Size::Small, Size::Wide], 600.0, 650.0, 10.0, 100.0);
        assert_eq!(b[0].width, 600.0);
        assert_eq!(b[1].width, 600.0);
        assert_eq!(b[1].y, 110.0);
    }

    #[test]
    fn mid_width_windows_use_half_rows() {
        let (b, _) = place(&[Size::Small], 900.0, 900.0, 0.0, 100.0);
        assert_eq!(b[0].width, 450.0);
    }
}
