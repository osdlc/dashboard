//! Renders every screen of `examples/demo.yaml`, with views from the real
//! demo modules, at terminal sizes from tiny to large.
//!
//! `cargo test -p dashboard-tui size_preview -- --ignored --nocapture` prints
//! a screen at a given size for eyeballing (see `size_preview`).

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use dashboard_contract::{ScreenLayout, TilePlacement, TileUpdate, UiSettings, View, Widget};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use super::*;

const SIZES: &[(u16, u16)] = &[
    (40, 15),
    (60, 20),
    (80, 24),
    (100, 30),
    (120, 40),
    (200, 60),
];

/// Runs each demo tile's module enough times to fill its graphs, the way the
/// host's history would.
async fn demo_tiles() -> (Vec<ScreenLayout>, BTreeMap<String, TileUpdate>) {
    let config = dashboard_config::parse(dashboard_config::DEFAULT_CONFIG).unwrap();
    let mut tiles = BTreeMap::new();
    for (id, m) in &config.modules {
        if m.builtin.as_deref() != Some("demo") {
            continue;
        }
        let mut module = dashboard_modules::builtin("demo").unwrap();
        module.init(m.config.clone()).await.unwrap();
        let mut history: HashMap<(String, String), Vec<(i64, f64)>> = HashMap::new();
        let mut view = None;
        for step in 0..60 {
            let mut v = module.render().await.unwrap();
            collect(&mut v.root, &mut history, step);
            view = Some(v);
        }
        let mut view: View = view.unwrap();
        fill(&mut view.root, &history);
        tiles.insert(
            id.clone(),
            TileUpdate {
                tile_id: id.clone(),
                view,
                updated_at: 0,
                stale: false,
                demo: false,
            },
        );
    }
    // Every placed tile that's a demo tile; others (cpu, github…) aren't run here.
    let screens = config
        .screens
        .iter()
        .map(|s| ScreenLayout {
            name: s.name.clone(),
            tiles: s
                .tiles
                .iter()
                .filter(|t| tiles.contains_key(&t.module))
                .map(|t| TilePlacement {
                    tile_id: t.module.clone(),
                    size: t.size.unwrap_or(dashboard_contract::Size::Medium),
                })
                .collect(),
        })
        .filter(|s| !s.tiles.is_empty())
        .collect();
    (screens, tiles)
}

fn collect(w: &mut Widget, h: &mut HashMap<(String, String), Vec<(i64, f64)>>, step: i64) {
    match w {
        Widget::Series {
            id: Some(id),
            series,
            ..
        } => {
            for s in series {
                let pts = s.points.iter().map(|(_, v)| (step, *v));
                h.entry((id.clone(), s.name.clone()))
                    .or_default()
                    .extend(pts);
            }
        }
        Widget::Row { children } | Widget::Column { children } => {
            children.iter_mut().for_each(|c| collect(c, h, step))
        }
        _ => {}
    }
}

fn fill(w: &mut Widget, h: &HashMap<(String, String), Vec<(i64, f64)>>) {
    match w {
        Widget::Series {
            id: Some(id),
            series,
            ..
        } => {
            for s in series {
                if let Some(p) = h.get(&(id.clone(), s.name.clone())) {
                    s.points = p.clone();
                }
            }
        }
        Widget::Row { children } | Widget::Column { children } => {
            children.iter_mut().for_each(|c| fill(c, h))
        }
        _ => {}
    }
}

fn demo_app(screens: Vec<ScreenLayout>, tiles: BTreeMap<String, TileUpdate>) -> App {
    let mut app = tests_support::empty_app();
    app.settings = UiSettings {
        screens,
        ..app.settings
    };
    app.tiles = tiles;
    app
}

fn draw_text(app: &App, w: u16, h: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|f| {
            draw(f, app, Instant::now());
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn every_screen_at_every_size() {
    let (screens, tiles) = demo_tiles().await;
    assert!(screens.len() >= 5, "demo screens found: {}", screens.len());
    let mut app = demo_app(screens.clone(), tiles);
    for &(w, h) in SIZES {
        for (i, screen) in screens.iter().enumerate() {
            app.go(i);
            draw_text(&app, w, h);

            // Same layout the draw used: tabs and footer take a row each.
            let main_h = h - 2;
            let (rects, page_w, page) = page_layout(&app, screen, w, main_h);
            let min = |k: usize, width| {
                widgets::tile_min_height(&app.tiles[&screen.tiles[k].tile_id], width)
            };
            for (k, r) in rects.iter().enumerate() {
                let id = &screen.tiles[k].tile_id;
                assert!(
                    r.height >= min(k, r.width),
                    "{w}x{h} {}: {id} got {} rows",
                    screen.name,
                    r.height
                );
                assert!(
                    r.right() <= page_w,
                    "{w}x{h} {}: {id} past the right edge",
                    screen.name
                );
                for (j, o) in rects.iter().enumerate().skip(k + 1) {
                    assert!(
                        r.intersection(*o).is_empty(),
                        "{w}x{h} {}: tiles {k} and {j} overlap",
                        screen.name
                    );
                }
            }
            assert_eq!(
                app.max_scroll.get(),
                page - main_h,
                "{w}x{h} {}",
                screen.name
            );

            // Scrolled to the end, the last row of the page is a bottom border.
            app.scroll = app.max_scroll.get();
            let text = draw_text(&app, w, h);
            let last_page_row = text.lines().nth((h - 2) as usize).unwrap();
            assert!(
                last_page_row.contains('└'),
                "{w}x{h} {}: bottom not reachable:\n{text}",
                screen.name
            );
        }
    }
}

/// Prints one screen at one size, top and scrolled to the end.
/// `SIZE=60x20 SCREEN=showcase cargo test -p dashboard-tui size_preview -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn size_preview() {
    let size = std::env::var("SIZE").unwrap_or_else(|_| "60x20".into());
    let (w, h) = size
        .split_once('x')
        .map(|(w, h)| (w.parse().unwrap(), h.parse().unwrap()))
        .unwrap();
    let name = std::env::var("SCREEN").unwrap_or_else(|_| "showcase".into());
    let (screens, tiles) = demo_tiles().await;
    let i = screens.iter().position(|s| s.name == name).expect("screen");
    let mut app = demo_app(screens, tiles);
    app.go(i);
    println!("{}\n", draw_text(&app, w, h));
    app.scroll = app.max_scroll.get() / 2;
    println!(
        "--- scrolled {} of {} ---\n{}",
        app.scroll,
        app.max_scroll.get(),
        draw_text(&app, w, h)
    );
}

/// Headings and short lists in a tall tile stay together at the top, instead
/// of each getting an equal share of the height (the GitHub tile bug).
#[test]
fn column_of_sections_packs_to_the_top() {
    use dashboard_contract::{ListItem, Status};
    let item = |t: &str| ListItem {
        title: t.into(),
        subtitle: Some("owner/repo#1 · updated 2m ago".into()),
        status: None,
        tags: vec![],
        action: None,
    };
    let root = Widget::Column {
        children: vec![
            Widget::Text {
                text: "**Review requested**".into(),
                markdown: true,
            },
            Widget::List {
                items: vec![],
                empty_text: Some("none".into()),
            },
            Widget::Text {
                text: "**Mine**".into(),
                markdown: true,
            },
            Widget::List {
                items: vec![item("first PR"), item("second PR")],
                empty_text: None,
            },
        ],
    };
    let tile = TileUpdate {
        tile_id: "github-prs".into(),
        view: View {
            status: Status::Ok,
            summary: String::new(),
            root,
        },
        updated_at: 0,
        stale: false,
        demo: false,
    };
    let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 60, 30));
    widgets::draw_tile(
        &mut canvas::Canvas(&mut buf),
        &tests_support::empty_app().palette,
        ratatui::layout::Rect::new(0, 0, 60, 30),
        &tile,
        &mut Hits::default(),
    );
    let rows: Vec<String> = (0..30)
        .map(|y| (0..60).map(|x| buf[(x, y)].symbol()).collect())
        .collect();
    let row_of = |s: &str| {
        rows.iter()
            .position(|r| r.contains(s))
            .unwrap_or_else(|| panic!("{s} not drawn:\n{}", rows.join("\n")))
    };
    // Border, heading, gap, "none", gap, heading, gap, items.
    assert_eq!(row_of("Review requested"), 1);
    assert_eq!(row_of("none"), 3);
    assert_eq!(row_of("Mine"), 5);
    assert_eq!(row_of("first PR"), 7);
    assert_eq!(row_of("second PR"), 9);
}
