//! Keyboard focus, as in `web/src/nav.ts` and `crates/tui/src/nav.rs`. Arrow
//! keys move between tiles; Enter goes into a tile's items, where Up and Down
//! move and Enter runs the item's action. Focus is kept by tile ID and item
//! key, so it stays on the same thing when live updates reorder items.

use std::collections::HashMap;

use sitroom_contract::{Action, Cell, Widget};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Focus {
    None,
    Tile(String),
    /// `index` is where the item was when last seen: the fallback when its key is gone.
    Item {
        tile: String,
        key: String,
        index: usize,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct NavItem {
    pub key: String,
    pub action: Option<Action>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub fn dir_for_key(key: &str) -> Option<Dir> {
    match key {
        "up" | "k" => Some(Dir::Up),
        "down" | "j" => Some(Dir::Down),
        "left" | "h" => Some(Dir::Left),
        "right" | "l" => Some(Dir::Right),
        _ => None,
    }
}

impl Focus {
    pub fn tile(&self) -> Option<&str> {
        match self {
            Focus::None => None,
            Focus::Tile(tile) | Focus::Item { tile, .. } => Some(tile),
        }
    }

    /// Where the focused item is now: the same key if it is still there,
    /// otherwise the nearest position.
    pub fn item_index(&self, items: &[NavItem]) -> Option<usize> {
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

    pub fn moved(&self, dir: Dir, tiles: &[(String, Bounds)], items: &[NavItem]) -> Focus {
        match self {
            Focus::None => tiles
                .first()
                .map_or(Focus::None, |(id, _)| Focus::Tile(id.clone())),
            Focus::Tile(tile) => {
                let Some(from) = tiles.iter().position(|(id, _)| id == tile) else {
                    return Focus::None;
                };
                let boxes: Vec<Bounds> = tiles.iter().map(|(_, b)| *b).collect();
                match neighbor(&boxes, from, dir) {
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
    pub fn enter(&self, items: &[NavItem]) -> (Focus, Option<Action>) {
        match self {
            Focus::Tile(tile) if !items.is_empty() => (item(tile, items, 0), None),
            Focus::Item { .. } => {
                let action = self
                    .item_index(items)
                    .and_then(|at| items[at].action.clone());
                (self.clone(), action)
            }
            _ => (self.clone(), None),
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

fn item(tile: &str, items: &[NavItem], index: usize) -> Focus {
    Focus::Item {
        tile: tile.to_string(),
        key: items[index].key.clone(),
        index,
    }
}

/// The selectable items in a widget tree, in the order they are drawn.
/// Repeated keys get "#2", "#3"...
pub fn items(widget: &Widget) -> Vec<NavItem> {
    let mut out = vec![];
    collect(widget, &mut out);
    let mut seen: HashMap<String, usize> = HashMap::new();
    for item in &mut out {
        let n = seen.entry(item.key.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            item.key = format!("{}#{n}", item.key);
        }
    }
    out
}

fn collect(widget: &Widget, out: &mut Vec<NavItem>) {
    // The text people see, or the link when there is none.
    let key = |text: String, action: &Option<Action>| match action {
        Some(Action::OpenUrl { url }) if text.is_empty() => url.clone(),
        _ => text,
    };
    match widget {
        Widget::List { items, .. } => out.extend(items.iter().map(|i| NavItem {
            key: key(i.title.clone(), &i.action),
            action: i.action.clone(),
        })),
        Widget::Table { columns, rows } => {
            let first = columns.first().map_or("", |c| c.key.as_str());
            out.extend(rows.iter().map(|r| {
                let text = match r.cells.get(first) {
                    Some(Cell::Text(t)) => t.clone(),
                    Some(Cell::Number(n)) => n.to_string(),
                    _ => String::new(),
                };
                NavItem {
                    key: key(text, &r.action),
                    action: r.action.clone(),
                }
            }));
        }
        Widget::Uptime { checks } => out.extend(checks.iter().map(|c| NavItem {
            key: c.name.clone(),
            action: c.url.clone().map(|url| Action::OpenUrl { url }),
        })),
        Widget::Bars { bars, .. } => out.extend(bars.iter().map(|b| NavItem {
            key: b.label.clone(),
            action: None,
        })),
        Widget::Row { children } | Widget::Column { children } => {
            children.iter().for_each(|c| collect(c, out));
        }
        _ => {}
    }
}

/// The box nearest to `boxes[from]` in direction `dir`. Prefers boxes that
/// line up with it, then the smallest gap, then the closest centre.
pub fn neighbor(boxes: &[Bounds], from: usize, dir: Dir) -> Option<usize> {
    let a = boxes[from];
    let along_x = matches!(dir, Dir::Left | Dir::Right);
    let sign = if matches!(dir, Dir::Right | Dir::Down) {
        1.0
    } else {
        -1.0
    };
    let span = |b: &Bounds, x: bool| {
        if x {
            (b.x, b.x + b.width)
        } else {
            (b.y, b.y + b.height)
        }
    };
    let (a0, a1) = span(&a, along_x);
    let (c0, c1) = span(&a, !along_x);
    let mut best: Option<(usize, [f32; 4])> = None;
    for (i, b) in boxes.iter().enumerate() {
        if i == from {
            continue;
        }
        let (b0, b1) = span(b, along_x);
        let far_edge = if sign > 0.0 { b1 > a1 } else { b0 < a0 };
        if sign * (b0 + b1 - (a0 + a1)) <= 0.0 || !far_edge {
            continue;
        }
        let gap = if sign > 0.0 { b0 - a1 } else { a0 - b1 }.max(0.0);
        let (d0, d1) = span(b, !along_x);
        let overlap = c1.min(d1) - c0.max(d0);
        let lines_up = if overlap < 1.0 { 1.0 } else { 0.0 };
        let score = [
            lines_up,
            gap,
            (-overlap).max(0.0),
            (d0 + d1 - (c0 + c1)).abs(),
        ];
        if best
            .as_ref()
            .is_none_or(|(_, s)| score.partial_cmp(s) == Some(std::cmp::Ordering::Less))
        {
            best = Some((i, score));
        }
    }
    best.map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sitroom_contract::ListItem;

    fn at(x: f32, y: f32) -> Bounds {
        Bounds {
            x,
            y,
            width: 10.0,
            height: 10.0,
        }
    }

    fn list(titles: &[&str]) -> Widget {
        Widget::List {
            empty_text: None,
            items: titles
                .iter()
                .map(|t| ListItem {
                    title: t.to_string(),
                    subtitle: None,
                    status: None,
                    tone: None,
                    tags: vec![],
                    action: None,
                })
                .collect(),
        }
    }

    #[test]
    fn moves_to_the_nearest_box_that_lines_up() {
        let boxes = [at(0.0, 0.0), at(20.0, 0.0), at(40.0, 0.0), at(0.0, 20.0)];
        assert_eq!(neighbor(&boxes, 0, Dir::Right), Some(1));
        assert_eq!(neighbor(&boxes, 0, Dir::Down), Some(3));
        assert_eq!(neighbor(&boxes, 0, Dir::Up), None);
    }

    #[test]
    fn focus_stays_on_the_item_when_rows_move() {
        let before = items(&list(&["a", "b", "c"]));
        let focus = Focus::Tile("t".into()).enter(&before).0;
        let focus = focus.moved(Dir::Down, &[], &before);
        let after = items(&list(&["b", "a", "c"]));
        assert_eq!(focus.item_index(&after), Some(0));
        // Gone: the nearest position.
        let gone = items(&list(&["a"]));
        assert_eq!(focus.item_index(&gone), Some(0));
    }

    #[test]
    fn repeated_keys_are_numbered() {
        let keys: Vec<String> = items(&list(&["a", "a"]))
            .into_iter()
            .map(|i| i.key)
            .collect();
        assert_eq!(keys, ["a", "a#2"]);
    }

    #[test]
    fn esc_steps_back_one_level() {
        let item = Focus::Item {
            tile: "t".into(),
            key: "a".into(),
            index: 0,
        };
        assert_eq!(item.back(), Focus::Tile("t".into()));
        assert_eq!(item.back().back(), Focus::None);
    }
}
