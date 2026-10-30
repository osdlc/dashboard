//! Mouse support. Drawing records what can be clicked where; a click looks
//! up the topmost target under the pointer.

use dashboard_contract::Theme;
use ratatui::layout::{Position, Rect};

#[derive(Debug, Clone, PartialEq)]
pub enum Click {
    Screen(usize),
    Help,
    Settings,
    Quit,
    Rotate,
    Theme(Theme),
    EditConfig,
    /// Outside an open overlay: close it.
    Close,
    /// Inside an overlay but not on a target: do nothing.
    Inside,
    /// A list item or table row with an `open_url` action.
    Open(String),
    /// Anywhere else on a tile; a double-click expands it.
    Tile(String),
}

#[derive(Debug, Default)]
pub struct Hits(Vec<(Rect, Click)>);

impl Hits {
    pub fn add(&mut self, area: Rect, click: Click) {
        if area.width > 0 && area.height > 0 {
            self.0.push((area, click));
        }
    }

    /// Adds `other`'s targets moved by (dx, dy) and cut to `clip`: for
    /// targets drawn on the scrolled page.
    pub fn extend_shifted(&mut self, other: Hits, dx: i32, dy: i32, clip: Rect) {
        for (r, click) in other.0 {
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
        self.0
            .iter()
            .rev()
            .filter(move |(r, _)| r.contains(Position { x, y }))
    }

    /// The last-added target containing (x, y); later ones are drawn on top.
    pub fn at(&self, x: u16, y: u16) -> Option<&Click> {
        self.0
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

/// Open a web link in the default browser. Only http(s), so a module can't
/// make this run something else.
pub fn open_url(url: &str) {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return;
    }
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
