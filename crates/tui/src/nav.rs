//! Keyboard focus. Two levels: the arrow keys move between tiles; Enter goes
//! into a tile's items (list items, table rows, uptime checks, bars), where
//! Up/Down move and Enter runs the item's action. Esc steps back out.
//!
//! Focus is kept by tile id and item key, not position, so it stays on the
//! same thing when live updates add or reorder items. The web view does the
//! same in web/src/nav.ts.

use std::collections::BTreeMap;

use dashboard_contract::{Action, Cell, Widget};
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Focus {
    #[default]
    None,
    Tile(String),
    /// `index` is where the item was when last seen: the fallback when its
    /// key is gone after an update.
    Item {
        tile: String,
        key: String,
        index: usize,
    },
}

/// Something inside a tile that the keyboard can select.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// Stays the same for the same item across updates.
    pub key: String,
    pub action: Option<Action>,
}

impl Focus {
    pub fn tile(&self) -> Option<&str> {
        match self {
            Focus::None => None,
            Focus::Tile(t) | Focus::Item { tile: t, .. } => Some(t),
        }
    }

    /// Where the focused item is now: the same key if it is still there,
    /// otherwise the nearest position. `None` at tile level or with no items.
    pub fn item_index(&self, items: &[Item]) -> Option<usize> {
        let Focus::Item { key, index, .. } = self else {
            return None;
        };
        if items.is_empty() {
            return None;
        }
        items
            .iter()
            .position(|i| &i.key == key)
            .or(Some((*index).min(items.len() - 1)))
    }

    /// Arrow keys. At tile level: to the nearest tile that way (or the first
    /// tile when nothing is focused). At item level: Up/Down move between
    /// items; Left/Right do nothing.
    pub fn moved(&self, dir: Dir, tiles: &[(String, Rect)], items: &[Item]) -> Focus {
        match self {
            Focus::None => tiles
                .first()
                .map(|(id, _)| Focus::Tile(id.clone()))
                .unwrap_or_default(),
            Focus::Tile(id) => {
                let Some(from) = tiles.iter().position(|(t, _)| t == id) else {
                    return Focus::None;
                };
                let rects: Vec<Rect> = tiles.iter().map(|(_, r)| *r).collect();
                match neighbor(&rects, from, dir) {
                    Some(to) => Focus::Tile(tiles[to].0.clone()),
                    None => self.clone(),
                }
            }
            Focus::Item { tile, .. } => {
                let Some(at) = self.item_index(items) else {
                    return Focus::Tile(tile.clone());
                };
                let to = match dir {
                    Dir::Up => at.saturating_sub(1),
                    Dir::Down => (at + 1).min(items.len() - 1),
                    Dir::Left | Dir::Right => at,
                };
                item(tile, items, to)
            }
        }
    }

    /// Enter. Goes into a tile that has items; on an item, returns its action.
    pub fn enter(&self, items: &[Item]) -> (Focus, Option<Action>) {
        match self {
            Focus::Tile(tile) if !items.is_empty() => (item(tile, items, 0), None),
            Focus::Item { .. } => {
                let action = self.item_index(items).and_then(|i| items[i].action.clone());
                (self.clone(), action)
            }
            _ => (self.clone(), None),
        }
    }

    /// Points an item focus at the item it resolves to among `items` now
    /// (see `item_index`), or back at its tile when there are none.
    pub fn settled(&self, items: &[Item]) -> Focus {
        match (self, self.item_index(items)) {
            (Focus::Item { tile, .. }, Some(at)) => item(tile, items, at),
            (Focus::Item { tile, .. }, None) => Focus::Tile(tile.clone()),
            _ => self.clone(),
        }
    }

    /// Esc: from an item back to its tile, from a tile to nothing.
    pub fn back(&self) -> Focus {
        match self {
            Focus::Item { tile, .. } => Focus::Tile(tile.clone()),
            _ => Focus::None,
        }
    }
}

fn item(tile: &str, items: &[Item], index: usize) -> Focus {
    Focus::Item {
        tile: tile.to_string(),
        key: items[index].key.clone(),
        index,
    }
}

/// The selectable items in a widget tree, in the order they are drawn.
/// Repeated keys get "#2", "#3"…, like `stableKeys` in the web view.
pub fn items(widget: &Widget) -> Vec<Item> {
    let mut out = vec![];
    collect(widget, &mut out);
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for item in &mut out {
        let n = seen.entry(item.key.clone()).or_default();
        *n += 1;
        if *n > 1 {
            item.key = format!("{}#{n}", item.key);
        }
    }
    out
}

fn collect(widget: &Widget, out: &mut Vec<Item>) {
    // The text people see, or the link when there is none. Links make poor
    // keys on their own: many items can share one.
    let key = |text: String, action: &Option<Action>| match action {
        Some(Action::OpenUrl { url }) if text.is_empty() => url.clone(),
        _ => text,
    };
    match widget {
        Widget::List { items, .. } => out.extend(items.iter().map(|i| Item {
            key: key(i.title.clone(), &i.action),
            action: i.action.clone(),
        })),
        Widget::Table { columns, rows } => {
            let first = columns.first().map(|c| c.key.as_str()).unwrap_or("");
            out.extend(rows.iter().map(|r| {
                let text = match r.cells.get(first) {
                    Some(Cell::Text(s)) => s.clone(),
                    Some(Cell::Number(n)) => n.to_string(),
                    _ => String::new(),
                };
                Item {
                    key: key(text, &r.action),
                    action: r.action.clone(),
                }
            }))
        }
        Widget::Uptime { checks } => out.extend(checks.iter().map(|c| Item {
            key: c.name.clone(),
            action: c.url.clone().map(|url| Action::OpenUrl { url }),
        })),
        Widget::Bars { bars, .. } => out.extend(bars.iter().map(|b| Item {
            key: b.label.clone(),
            action: None,
        })),
        Widget::Row { children } | Widget::Column { children } => {
            for c in children {
                collect(c, out);
            }
        }
        _ => {}
    }
}

/// The rect nearest to `rects[from]` in direction `dir`, if any. Prefers
/// rects that line up with it (overlap across the direction of travel), then
/// the smallest gap, then the closest centre.
pub fn neighbor(rects: &[Rect], from: usize, dir: Dir) -> Option<usize> {
    let a = rects[from];
    // (near edge, far edge, centre) along and across the direction of travel.
    let span = |r: Rect, along_x: bool| {
        if along_x {
            (r.x as i32, r.right() as i32)
        } else {
            (r.y as i32, r.bottom() as i32)
        }
    };
    let along_x = matches!(dir, Dir::Left | Dir::Right);
    let sign = if matches!(dir, Dir::Right | Dir::Down) {
        1
    } else {
        -1
    };
    let (a0, a1) = span(a, along_x);
    let (c0, c1) = span(a, !along_x);
    rects
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != from)
        .filter_map(|(i, &b)| {
            let (b0, b1) = span(b, along_x);
            // Must lie past `a` that way: its centre beyond a's centre and
            // its far edge beyond a's far edge.
            let ahead =
                sign * ((b0 + b1) - (a0 + a1)) > 0 && if sign > 0 { b1 > a1 } else { b0 < a0 };
            if !ahead {
                return None;
            }
            let gap = if sign > 0 { b0 - a1 } else { a0 - b1 }.max(0);
            let (d0, d1) = span(b, !along_x);
            // Negative when they don't line up: how far apart they are.
            let overlap = c1.min(d1) - c0.max(d0);
            let centre = ((d0 + d1) - (c0 + c1)).abs();
            Some(((overlap <= 0, gap, -overlap.min(0), centre), i))
        })
        .min()
        .map(|(_, i)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashboard_contract::{ListItem, UptimeCheck};

    fn list(titles: &[&str]) -> Widget {
        Widget::List {
            items: titles
                .iter()
                .map(|t| ListItem {
                    tone: None,
                    title: t.to_string(),
                    subtitle: None,
                    status: None,
                    tags: vec![],
                    action: Some(Action::OpenUrl {
                        url: format!("https://example.com/{t}"),
                    }),
                })
                .collect(),
            empty_text: None,
        }
    }

    /// A 2x2 grid with a wide tile under it:
    /// a b
    /// c d
    /// e e
    fn grid() -> Vec<(String, Rect)> {
        [
            ("a", Rect::new(0, 0, 50, 10)),
            ("b", Rect::new(50, 0, 50, 10)),
            ("c", Rect::new(0, 10, 50, 10)),
            ("d", Rect::new(50, 10, 50, 10)),
            ("e", Rect::new(0, 20, 100, 10)),
        ]
        .into_iter()
        .map(|(id, r)| (id.to_string(), r))
        .collect()
    }

    fn tile(id: &str) -> Focus {
        Focus::Tile(id.into())
    }

    #[test]
    fn arrows_move_to_the_nearest_tile_that_way() {
        let g = grid();
        assert_eq!(Focus::None.moved(Dir::Down, &g, &[]), tile("a"));
        assert_eq!(tile("a").moved(Dir::Right, &g, &[]), tile("b"));
        assert_eq!(tile("a").moved(Dir::Down, &g, &[]), tile("c"));
        assert_eq!(tile("b").moved(Dir::Down, &g, &[]), tile("d"));
        assert_eq!(tile("d").moved(Dir::Left, &g, &[]), tile("c"));
        assert_eq!(tile("d").moved(Dir::Down, &g, &[]), tile("e"));
        assert_eq!(
            tile("e").moved(Dir::Up, &g, &[]),
            tile("c"),
            "ties go to the left"
        );
        // Nothing that way: stay.
        assert_eq!(tile("a").moved(Dir::Up, &g, &[]), tile("a"));
        assert_eq!(tile("b").moved(Dir::Right, &g, &[]), tile("b"));
        // A tile that left the screen drops focus.
        assert_eq!(tile("gone").moved(Dir::Up, &g, &[]), Focus::None);
    }

    #[test]
    fn prefers_tiles_in_line_over_closer_ones_off_to_the_side() {
        // a is tall on the left; b and c stack on the right. From c, Left is a.
        let r = [
            Rect::new(0, 0, 40, 20),
            Rect::new(40, 0, 60, 10),
            Rect::new(40, 10, 60, 10),
            Rect::new(0, 20, 100, 5),
        ];
        assert_eq!(neighbor(&r, 2, Dir::Left), Some(0));
        assert_eq!(neighbor(&r, 0, Dir::Right), Some(1));
        // Edges that only touch don't count as lining up.
        assert_eq!(neighbor(&r, 0, Dir::Down), Some(3));
        // From the bottom, Up goes to the tile nearest its centre.
        assert_eq!(neighbor(&r, 3, Dir::Up), Some(2));
    }

    #[test]
    fn enter_goes_in_moves_and_runs_the_action_and_esc_comes_back() {
        let items = items(&list(&["one", "two", "three"]));
        assert_eq!(
            tile("t").enter(&[]),
            (tile("t"), None),
            "nothing to go into"
        );
        let (f, action) = tile("t").enter(&items);
        assert_eq!(action, None);
        assert_eq!(f.item_index(&items), Some(0));
        let f = f.moved(Dir::Up, &[], &items);
        assert_eq!(f.item_index(&items), Some(0), "stops at the top");
        let f = f
            .moved(Dir::Down, &[], &items)
            .moved(Dir::Down, &[], &items);
        let f = f.moved(Dir::Down, &[], &items);
        assert_eq!(f.item_index(&items), Some(2), "stops at the bottom");
        assert_eq!(
            f.enter(&items).1,
            Some(Action::OpenUrl {
                url: "https://example.com/three".into()
            })
        );
        assert_eq!(f.back(), tile("t"));
        assert_eq!(tile("t").back(), Focus::None);
    }

    #[test]
    fn focus_follows_the_item_across_updates() {
        let before = items(&list(&["a", "b", "c", "d"]));
        let f = tile("t").enter(&before).0.moved(Dir::Down, &[], &before);
        let f = f.moved(Dir::Down, &[], &before);
        assert_eq!(f.item_index(&before), Some(2), "on c");
        // New items arrive at the top: still on c.
        let after = items(&list(&["new", "a", "b", "c", "d"]));
        assert_eq!(f.item_index(&after), Some(3));
        // c goes away: the nearest item takes its place.
        let after = items(&list(&["a", "b", "d"]));
        assert_eq!(f.item_index(&after), Some(2));
        let after = items(&list(&["a"]));
        assert_eq!(f.item_index(&after), Some(0));
        assert_eq!(f.item_index(&[]), None);
        // Moving from a missing item goes from where it was.
        let after = items(&list(&["a", "b", "d", "e"]));
        assert_eq!(f.moved(Dir::Down, &[], &after).item_index(&after), Some(3));
    }

    #[test]
    fn items_come_from_nested_lists_tables_uptime_and_bars() {
        let check = |name: &str, url: Option<&str>| UptimeCheck {
            name: name.into(),
            status: dashboard_contract::Status::Ok,
            uptime: None,
            latency_ms: None,
            detail: None,
            history: vec![],
            url: url.map(Into::into),
        };
        let w = Widget::Column {
            children: vec![
                list(&["x", "x"]),
                Widget::Row {
                    children: vec![Widget::Uptime {
                        checks: vec![check("api", Some("https://api")), check("db", None)],
                    }],
                },
            ],
        };
        let keys: Vec<String> = items(&w).into_iter().map(|i| i.key).collect();
        assert_eq!(keys, ["x", "x#2", "api", "db"]);
    }
}
