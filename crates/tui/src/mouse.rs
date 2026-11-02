//! Mouse support. Drawing records what can be clicked where; a click looks
//! up the topmost target under the pointer.

use dashboard_contract::{Action, Theme};
use ratatui::layout::{Position, Rect};

#[derive(Debug, Clone, PartialEq)]
pub enum Click {
    Screen(usize),
    Help,
    Settings,
    Quit,
    Rotate,
    Theme(Theme),
    /// A color theme, by name.
    ColorTheme(String),
    EditConfig,
    /// Outside an open overlay: close it.
    Close,
    /// Inside an overlay but not on a target: do nothing.
    Inside,
    /// A list item, table row or uptime check with an action.
    Run(Action),
    /// Anywhere else on a tile; a double-click expands it.
    Tile(String),
}

#[derive(Debug, Default)]
pub struct Hits {
    targets: Vec<(Rect, Click)>,
    /// Where each keyboard-selectable item was drawn, in `nav::items` order.
    /// Items cut off at the bottom of a tile get an empty rect.
    pub items: Vec<Rect>,
}

impl Hits {
    pub fn add(&mut self, area: Rect, click: Click) {
        if area.width > 0 && area.height > 0 {
            self.targets.push((area, click));
        }
    }

    /// Records where the next selectable item is, clipped to `within`.
    pub fn item(&mut self, area: Rect, within: Rect) {
        let r = area.intersection(within);
        self.items.push(if r.height == 0 {
            Rect { height: 0, ..r }
        } else {
            r
        });
    }

    /// Adds `other`'s targets moved by (dx, dy) and cut to `clip`: for
    /// targets drawn on the scrolled page.
    pub fn extend_shifted(&mut self, other: Hits, dx: i32, dy: i32, clip: Rect) {
        for (r, click) in other.targets {
            let (x, y) = (r.x as i32 + dx, r.y as i32 + dy);
            let (x0, y0) = (x.max(clip.x as i32), y.max(clip.y as i32));
            let x1 = (x + r.width as i32).min(clip.right() as i32);
            let y1 = (y + r.height as i32).min(clip.bottom() as i32);
            if x1 > x0 && y1 > y0 {
                let rect = Rect::new(x0 as u16, y0 as u16, (x1 - x0) as u16, (y1 - y0) as u16);
                self.add(rect, click);
            }
        }
    }

    /// Every target containing (x, y), topmost first.
    pub fn all_at(&self, x: u16, y: u16) -> impl Iterator<Item = &(Rect, Click)> {
        self.targets
            .iter()
            .rev()
            .filter(move |(r, _)| r.contains(Position { x, y }))
    }

    /// The tile under (x, y), unless something other than a tile or its
    /// items (a tab, an overlay) is on top there.
    pub fn tile_at(&self, x: u16, y: u16) -> Option<&str> {
        for (_, click) in self.all_at(x, y) {
            match click {
                Click::Tile(id) => return Some(id),
                Click::Run(_) => continue,
                _ => return None,
            }
        }
        None
    }

    /// The last-added target containing (x, y); later ones are drawn on top.
    pub fn at(&self, x: u16, y: u16) -> Option<&Click> {
        self.targets
            .iter()
            .rev()
            .find(|(r, _)| r.contains(Position { x, y }))
            .map(|(_, c)| c)
    }
}

/// A clickable run of text on one line: draws `parts` left to right from
/// `area.x` and records each part that has a target.
pub fn spans(area: Rect, parts: &[(String, Option<Click>)], hits: &mut Hits) {
    let mut x = area.x;
    for (text, click) in parts {
        let w = text.chars().count() as u16;
        if let Some(click) = click {
            let w = w.min(area.right().saturating_sub(x));
            hits.add(
                Rect {
                    x,
                    y: area.y,
                    width: w,
                    height: 1,
                },
                click.clone(),
            );
        }
        x = x.saturating_add(w);
    }
}
