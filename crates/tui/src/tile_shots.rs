//! Draws each catalog entry's demo view as one tile at every size, with the
//! real tile renderer, and saves the cells with their colors as JSON for the
//! site's widget pages (site/scripts/registry.mjs runs this).
//!
//! `cargo test -p dashboard-tui tile_shots -- --ignored` with, all optional:
//! CATALOG (a `dashboard widgets --json` file, default the site's
//! registry.json), OUT (default site/src/data/tui-tiles), THEMES (default
//! `dark,light`), WIDTH (the terminal width the tiles are laid out for,
//! default 120) and ONLY (comma-separated IDs).
//!
//! Writes `<OUT>/<theme>/<id>.json`:
//! `{"width", "styles": [[fg, bg, modifiers]], "sizes": {size: {"cols", "rows", "lines"}}}`.
//! A line is a list of `[style index, text]` runs. Colors are `""` (the
//! terminal's own), a name (`green`, `darkgray`), `#rrggbb` or `i<n>` (256-color
//! index); modifiers are letters: b bold, d dim, i italic, u underlined,
//! r reversed, x crossed out.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use dashboard_contract::{Size, Theme, TileUpdate, View};
use ratatui::buffer::{Buffer, Cell};
use ratatui::style::{Color, Modifier};
use ratatui::text::Span;
use serde_json::Value;

use super::*;

const SIZES: [Size; 5] = [
    Size::Small,
    Size::Medium,
    Size::Large,
    Size::Wide,
    Size::Tall,
];

fn env_or(key: &str, default: impl Into<String>) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.into())
}

/// The palette for a theme name as the config spells it (`dark`, `light`).
fn palette_named(name: &str) -> Palette {
    let theme: Theme = serde_json::from_value(Value::String(name.into()))
        .unwrap_or_else(|_| panic!("unknown theme {name:?}"));
    assert!(theme != Theme::Auto, "pick a fixed theme, not auto");
    Palette::for_theme(theme, false)
}

/// A size as the config spells it.
fn size_name(size: Size) -> String {
    serde_json::to_value(size)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}

fn color(c: Color) -> String {
    match c {
        Color::Reset => String::new(),
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Indexed(n) => format!("i{n}"),
        named => format!("{named:?}").to_lowercase(),
    }
}

fn modifiers(m: Modifier) -> String {
    [
        (Modifier::BOLD, 'b'),
        (Modifier::DIM, 'd'),
        (Modifier::ITALIC, 'i'),
        (Modifier::UNDERLINED, 'u'),
        (Modifier::REVERSED, 'r'),
        (Modifier::CROSSED_OUT, 'x'),
    ]
    .iter()
    .filter(|(flag, _)| m.contains(*flag))
    .map(|(_, c)| *c)
    .collect()
}

/// A cell that looks the same as an empty one: trimmed from line ends.
fn blank(cell: &Cell) -> bool {
    cell.symbol() == " "
        && cell.bg == Color::Reset
        && !cell
            .modifier
            .intersects(Modifier::REVERSED | Modifier::UNDERLINED | Modifier::CROSSED_OUT)
}

/// Shared across a file's sizes: each distinct style once.
#[derive(Default)]
struct Styles {
    list: Vec<[String; 3]>,
    index: HashMap<[String; 3], usize>,
}

impl Styles {
    fn of(&mut self, cell: &Cell) -> usize {
        let key = [color(cell.fg), color(cell.bg), modifiers(cell.modifier)];
        if let Some(i) = self.index.get(&key) {
            return *i;
        }
        self.list.push(key.clone());
        self.index.insert(key, self.list.len() - 1);
        self.list.len() - 1
    }
}

/// One line of `[style, text]` runs, as JSON.
fn line_json(buf: &Buffer, y: u16, styles: &mut Styles) -> String {
    let area = buf.area;
    let mut end = area.width;
    while end > 0 && blank(&buf[(end - 1, y)]) {
        end -= 1;
    }
    let mut runs: Vec<(usize, String)> = vec![];
    let mut x = 0;
    while x < end {
        let cell = &buf[(x, y)];
        let symbol = cell.symbol();
        match runs.last_mut() {
            // A blank looks the same in any style, so it joins the run before.
            Some((_, text)) if blank(cell) => text.push_str(symbol),
            Some((s, text)) if *s == styles.of(cell) => text.push_str(symbol),
            _ => runs.push((styles.of(cell), symbol.to_string())),
        }
        // A wide character covers the next cell too.
        x += (Span::raw(symbol).width() as u16).max(1);
    }
    serde_json::to_string(&runs).unwrap()
}

#[tokio::test]
#[ignore]
async fn tile_shots() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let catalog = env_or(
        "CATALOG",
        root.join("site/src/data/registry.json").to_string_lossy(),
    );
    let out = PathBuf::from(env_or(
        "OUT",
        root.join("site/src/data/tui-tiles").to_string_lossy(),
    ));
    let themes = env_or("THEMES", "dark,light");
    let width: u16 = env_or("WIDTH", "120").parse().expect("WIDTH");
    let only = std::env::var("ONLY").unwrap_or_default();
    let only: Vec<&str> = only.split(',').filter(|s| !s.is_empty()).collect();

    let text = std::fs::read_to_string(&catalog).unwrap_or_else(|e| panic!("{catalog}: {e}"));
    let catalog: Value = serde_json::from_str(&text).unwrap();
    let entries = catalog["entries"].as_array().expect("entries");

    for theme in themes.split(',').filter(|s| !s.is_empty()) {
        let palette = palette_named(theme);
        let dir = out.join(theme);
        // A full run starts clean, so removed widgets lose their files.
        if only.is_empty() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        std::fs::create_dir_all(&dir).unwrap();
        for entry in entries {
            let id = entry["id"].as_str().expect("id");
            if !only.is_empty() && !only.contains(&id) {
                continue;
            }
            let view: View = serde_json::from_value(entry["demo"].clone())
                .unwrap_or_else(|e| panic!("{id}: demo view: {e}"));
            let tile = TileUpdate {
                tile_id: id.into(),
                view,
                updated_at: 0,
                stale: false,
                demo: false,
            };
            let mut styles = Styles::default();
            let mut sizes = String::new();
            for size in SIZES {
                let area = layout::single(size, width);
                let mut buf = Buffer::empty(area);
                widgets::draw_tile(
                    &mut canvas::Canvas(&mut buf),
                    &palette,
                    area,
                    &tile,
                    &mut Hits::default(),
                );
                let lines: Vec<String> = (0..area.height)
                    .map(|y| line_json(&buf, y, &mut styles))
                    .collect();
                if !sizes.is_empty() {
                    sizes.push_str(",\n");
                }
                write!(
                    sizes,
                    "{:?}: {{\"cols\": {}, \"rows\": {}, \"lines\": [\n{}\n]}}",
                    size_name(size),
                    area.width,
                    area.height,
                    lines.join(",\n")
                )
                .unwrap();
            }
            let file = dir.join(format!("{id}.json"));
            let json = format!(
                "{{\"width\": {width}, \"styles\": {},\n\"sizes\": {{\n{sizes}\n}}}}\n",
                serde_json::to_string(&styles.list).unwrap()
            );
            std::fs::write(&file, json).unwrap();
            println!("{}", file.display());
        }
    }
}
