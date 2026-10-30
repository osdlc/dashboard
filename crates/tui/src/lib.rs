//! TUI renderer (ratatui). Draws the latest `TileUpdate` per tile, one
//! screen at a time.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use anyhow::Result;
use dashboard_contract::{ClientMessage, ScreenLayout, TileUpdate, UiSettings};
use dashboard_host::Host;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    MouseButton, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::Rect;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use tokio::sync::broadcast::error::TryRecvError;
use tokio::sync::{mpsc, watch};

mod canvas;
mod charts;
mod help;
mod layout;
mod mouse;
mod rotate;
mod settings;
#[cfg(test)]
mod size_tests;
mod theme;
mod widgets;

use canvas::Canvas;
use mouse::{Click, Hits};
use theme::Palette;

struct App {
    settings: UiSettings,
    palette: Palette,
    /// Terminal background detected at startup, for `Theme::Auto`.
    auto_light: bool,
    tiles: BTreeMap<String, TileUpdate>,
    screen: usize,
    overlay: Overlay,
    quit: bool,
    rotation: Option<rotate::Rotation>,
    /// Where actions such as changing the theme go. `None` in tests.
    actions: Option<mpsc::UnboundedSender<ClientMessage>>,
    /// Set by `e` in settings; the run loop opens the editor.
    edit_config: bool,
    /// Click targets from the last draw.
    hits: Hits,
    /// Tile shown fullscreen after a double-click.
    expanded: Option<String>,
    /// For spotting double-clicks.
    last_click: Option<(Click, Instant)>,
    /// Where the mouse pointer is, for hover highlights.
    hover: Option<(u16, u16)>,
    /// Rows the page is scrolled down, when it is taller than the terminal.
    scroll: u16,
    /// How far the page can scroll, from the last draw.
    max_scroll: std::cell::Cell<u16>,
}

/// Rows per mouse-wheel step.
const WHEEL_ROWS: u16 = 3;

/// Two clicks on the same tile within this make a double-click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// What the run loop reacts to.
enum Input {
    Key(KeyEvent),
    Click(u16, u16),
    Wheel(i8),
    Hover(u16, u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Overlay {
    None,
    Help,
    Settings,
}

pub async fn run(
    host: Host,
    mut settings_rx: watch::Receiver<UiSettings>,
    actions: mpsc::UnboundedSender<ClientMessage>,
) -> Result<()> {
    // Query the terminal before entering raw mode / the alternate screen.
    let auto_light = Palette::terminal_is_light();
    let settings = settings_rx.borrow_and_update().clone();
    let palette = Palette::for_theme(settings.theme, auto_light);
    let mut rx = host.subscribe();
    let tiles = host
        .snapshot()
        .await
        .into_iter()
        .map(|t| (t.tile_id.clone(), t))
        .collect();
    let rotation = settings
        .rotate
        .as_ref()
        .and_then(|r| rotate::Rotation::new(r, &settings.screens, Instant::now()));
    let mut app = App {
        settings,
        palette,
        auto_light,
        tiles,
        screen: 0,
        overlay: Overlay::None,
        quit: false,
        rotation,
        actions: Some(actions),
        edit_config: false,
        hits: Hits::default(),
        expanded: None,
        last_click: None,
        hover: None,
        scroll: 0,
        max_scroll: Default::default(),
    };

    let mut terminal = init_terminal()?;
    let result = loop {
        loop {
            match rx.try_recv() {
                Ok(update) => {
                    app.tiles.insert(update.tile_id.clone(), update);
                }
                Err(TryRecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }

        if settings_rx.has_changed().unwrap_or(false) {
            let settings = settings_rx.borrow_and_update().clone();
            app.apply_settings(settings, Instant::now());
        }

        app.tick(Instant::now());
        let mut hits = Hits::default();
        if let Err(err) = terminal.draw(|f| hits = draw(f, &app, Instant::now())) {
            break Err(err.into());
        }
        app.hits = hits;

        let input = tokio::task::block_in_place(|| -> Result<Option<Input>> {
            if !event::poll(Duration::from_millis(100))? {
                return Ok(None);
            }
            Ok(match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => Some(Input::Key(key)),
                Event::Mouse(m) => match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => Some(Input::Click(m.column, m.row)),
                    MouseEventKind::ScrollDown => Some(Input::Wheel(1)),
                    MouseEventKind::ScrollUp => Some(Input::Wheel(-1)),
                    MouseEventKind::Moved => Some(Input::Hover(m.column, m.row)),
                    _ => None,
                },
                _ => None,
            })
        });
        let now = Instant::now();
        match input {
            Ok(Some(Input::Key(key))) => app.on_key(key, now),
            Ok(Some(Input::Click(x, y))) => {
                if let Some(click) = app.hits.at(x, y).cloned() {
                    app.on_click(click, now);
                }
            }
            Ok(Some(Input::Wheel(step))) => app.on_wheel(step, now),
            Ok(Some(Input::Hover(x, y))) => app.hover = Some((x, y)),
            Ok(None) => {}
            Err(err) => break Err(err),
        }
        if app.quit {
            break Ok(());
        }
        if std::mem::take(&mut app.edit_config) {
            // Hand the terminal to the editor, then take it back. Saving
            // reloads the config through the file poll.
            restore_terminal();
            let edited = tokio::task::block_in_place(|| open_editor(&app.settings.config.path));
            terminal = match init_terminal() {
                Ok(t) => t,
                Err(err) => break Err(err),
            };
            if let Err(err) = edited {
                break Err(err);
            }
        }
    };
    restore_terminal();
    result
}

fn init_terminal() -> Result<ratatui::DefaultTerminal> {
    let terminal = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture)?;
    Ok(terminal)
}

fn restore_terminal() {
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
}

impl App {
    /// New settings from a config reload. Stays on the same screen if it still exists.
    fn apply_settings(&mut self, settings: UiSettings, now: Instant) {
        let current = self
            .settings
            .screens
            .get(self.screen)
            .map(|s| s.name.clone());
        self.screen = current
            .and_then(|name| settings.screens.iter().position(|s| s.name == name))
            .unwrap_or(0);
        self.palette = Palette::for_theme(settings.theme, self.auto_light);
        self.rotation = settings
            .rotate
            .as_ref()
            .and_then(|r| rotate::Rotation::new(r, &settings.screens, now));
        self.settings = settings;
    }

    fn tick(&mut self, now: Instant) {
        if let Some(next) = self
            .rotation
            .as_mut()
            .and_then(|r| r.tick(self.screen, now))
        {
            self.screen = next;
        }
    }

    fn on_click(&mut self, click: Click, now: Instant) {
        let double = self
            .last_click
            .as_ref()
            .is_some_and(|(c, at)| *c == click && now.duration_since(*at) < DOUBLE_CLICK);
        // A double-click uses up both clicks, so a third starts over.
        self.last_click = if double {
            None
        } else {
            Some((click.clone(), now))
        };
        if let Some(r) = self.rotation.as_mut() {
            if click == Click::Rotate {
                r.toggle(now);
                return;
            }
            r.input(now);
        }
        let toggle = |current: Overlay, target: Overlay| {
            if current == target {
                Overlay::None
            } else {
                target
            }
        };
        match click {
            Click::Screen(i) => {
                self.go(i);
                self.overlay = Overlay::None;
            }
            Click::Tile(id) if double => {
                self.expanded = match self.expanded {
                    Some(_) => None,
                    None => Some(id),
                };
            }
            Click::Tile(_) => {}
            Click::Help => self.overlay = toggle(self.overlay, Overlay::Help),
            Click::Settings => self.overlay = toggle(self.overlay, Overlay::Settings),
            Click::Quit => self.quit = true,
            Click::Theme(theme) => {
                if let Some(tx) = &self.actions {
                    let _ = tx.send(ClientMessage::SetTheme { theme });
                }
            }
            Click::EditConfig => self.edit_config = true,
            Click::Close => self.overlay = Overlay::None,
            Click::Open(url) => mouse::open_url(&url),
            Click::Rotate | Click::Inside => {}
        }
    }

    /// The wheel scrolls the page. Ignored while an overlay is open.
    fn on_wheel(&mut self, step: i8, now: Instant) {
        if self.overlay != Overlay::None {
            return;
        }
        if let Some(r) = self.rotation.as_mut() {
            r.input(now);
        }
        self.scroll_by(step as i32 * WHEEL_ROWS as i32);
    }

    fn scroll_by(&mut self, rows: i32) {
        let max = self.max_scroll.get() as i32;
        self.scroll = (self.scroll as i32 + rows).clamp(0, max) as u16;
    }

    /// Switch screen, starting at the top.
    fn go(&mut self, screen: usize) {
        self.screen = screen;
        self.expanded = None;
        self.scroll = 0;
    }

    /// Keys here must match `help::KEYS`.
    fn on_key(&mut self, key: KeyEvent, now: Instant) {
        let count = self.settings.screens.len().max(1);
        if let Some(r) = self.rotation.as_mut() {
            if key.code == KeyCode::Char('r') {
                r.toggle(now);
                return;
            }
            r.input(now);
        }
        if self.overlay == Overlay::Settings {
            match key.code {
                KeyCode::Char('t') => {
                    let theme = settings::next_theme(self.settings.theme);
                    if let Some(tx) = &self.actions {
                        let _ = tx.send(ClientMessage::SetTheme { theme });
                    }
                    return;
                }
                KeyCode::Char('e') => {
                    self.edit_config = true;
                    return;
                }
                _ => {}
            }
        }
        let toggle = |current: Overlay, target: Overlay| {
            if current == target {
                Overlay::None
            } else {
                target
            }
        };
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.overlay = toggle(self.overlay, Overlay::Help),
            KeyCode::Char('s') => self.overlay = toggle(self.overlay, Overlay::Settings),
            KeyCode::Esc if self.overlay == Overlay::None => self.expanded = None,
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Tab => self.go((self.screen + 1) % count),
            KeyCode::BackTab => self.go((self.screen + count - 1) % count),
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < count {
                    self.go(i);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => self.scroll_by(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_by(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_by(10),
            KeyCode::PageUp => self.scroll_by(-10),
            KeyCode::Home => self.scroll = 0,
            KeyCode::End => self.scroll = self.max_scroll.get(),
            _ => {}
        }
    }
}

/// Draws everything and returns where the click targets ended up.
fn draw(f: &mut Frame, app: &App, now: Instant) -> Hits {
    let p = &app.palette;
    let mut hits = Hits::default();
    let [tabs, main, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(f.area());

    // Screen tabs: " 1 main │ 2 charts │ ..."
    let mut parts = vec![];
    let mut spans = vec![];
    for (i, s) in app.settings.screens.iter().enumerate() {
        if i > 0 {
            parts.push(("│".to_string(), None));
            spans.push(Span::styled("│", Style::new().fg(p.muted)));
        }
        let text = format!(" {} {} ", i + 1, s.name);
        let style = if i == app.screen {
            Style::new().fg(p.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(p.muted)
        };
        spans.push(Span::styled(text.clone(), style));
        parts.push((text, Some(Click::Screen(i))));
    }
    mouse::spans(tabs, &parts, &mut hits);
    f.render_widget(Paragraph::new(Line::from(spans)), tabs);

    let rotation = app.rotation.as_ref().map(|r| match r.state(now) {
        rotate::State::On => ("  ⟳ rotating (r to stop)", p.accent),
        rotate::State::Paused => ("  ⟳ paused until idle (r to stop)", p.muted),
        rotate::State::Off => ("  ⟳ off (r to start)", p.muted),
    });
    let mut parts = vec![
        (" ? help".to_string(), Some(Click::Help)),
        ("  ".to_string(), None),
        ("s settings".to_string(), Some(Click::Settings)),
        ("  Tab screens  ".to_string(), None),
        ("q quit".to_string(), Some(Click::Quit)),
    ];
    let mut hints: Vec<Span> = parts.iter().map(|(t, _)| Span::raw(t.clone())).collect();
    if let Some((text, color)) = rotation {
        parts.push((text.to_string(), Some(Click::Rotate)));
        hints.push(Span::styled(text, Style::new().fg(color)));
    }
    mouse::spans(footer, &parts, &mut hits);
    f.render_widget(
        Paragraph::new(Line::from(hints)).style(Style::new().fg(p.muted)),
        footer,
    );

    app.max_scroll.set(0);
    let expanded = app.expanded.as_ref().and_then(|id| app.tiles.get(id));
    if let Some(update) = expanded {
        widgets::draw_tile(&mut Canvas(f.buffer_mut()), p, main, update, &mut hits);
    } else if let Some(screen) = app.settings.screens.get(app.screen) {
        if screen.tiles.is_empty() {
            f.render_widget(Paragraph::new("this screen has no running modules"), main);
        } else {
            draw_page(f, app, main, screen, &mut hits);
        }
    }

    if app.overlay != Overlay::None {
        // Anywhere outside the overlay closes it; the overlay adds its own targets on top.
        hits.add(f.area(), Click::Close);
    }
    match app.overlay {
        Overlay::Help => help::draw(f, p, &app.settings, app.screen, &mut hits),
        Overlay::Settings => settings::draw(f, p, &app.settings, &mut hits),
        Overlay::None => {}
    }
    draw_hover(f, app, &hits);
    hits
}

/// Outlines the tile under the mouse and highlights the clickable thing
/// (link, tab, button) under it. Drawn last, on top of everything.
fn draw_hover(f: &mut Frame, app: &App, hits: &Hits) {
    let Some((x, y)) = app.hover else { return };
    let p = &app.palette;
    let buf = f.buffer_mut();
    let mut highlighted = false;
    for (rect, click) in hits.all_at(x, y) {
        match click {
            // An overlay is open and the mouse is on it or outside it.
            Click::Inside | Click::Close => return,
            Click::Tile(_) => {
                // Recolor only border characters: a tile scrolled partly out
                // of view has a clipped rect whose edges aren't its border.
                for (cx, cy) in perimeter(*rect) {
                    let cell = &mut buf[(cx, cy)];
                    if "─│┌┐└┘".contains(cell.symbol()) {
                        cell.set_fg(p.accent);
                    }
                }
                return;
            }
            _ if !highlighted => {
                for cy in rect.top()..rect.bottom() {
                    for cx in rect.left()..rect.right() {
                        buf[(cx, cy)].set_bg(p.hover);
                    }
                }
                highlighted = true;
            }
            _ => {}
        }
    }
}

fn perimeter(r: Rect) -> impl Iterator<Item = (u16, u16)> {
    let top_bottom =
        (r.left()..r.right()).flat_map(move |x| [(x, r.top()), (x, r.bottom().saturating_sub(1))]);
    let sides =
        (r.top()..r.bottom()).flat_map(move |y| [(r.left(), y), (r.right().saturating_sub(1), y)]);
    top_bottom.chain(sides)
}

/// Tile rects on the page, the page width and its height. When the page is
/// taller than the view, the last column is left for the scrollbar.
fn page_layout(app: &App, screen: &ScreenLayout, width: u16, height: u16) -> (Vec<Rect>, u16, u16) {
    let sizes: Vec<_> = screen.tiles.iter().map(|t| t.size).collect();
    let min_height = |i: usize, w: u16| {
        app.tiles
            .get(&screen.tiles[i].tile_id)
            .map(|t| widgets::tile_min_height(t, w))
            .unwrap_or(3)
    };
    let (rects, page) = layout::place(width, height, &sizes, min_height);
    if page <= height {
        return (rects, width, page);
    }
    let page_width = width.saturating_sub(1);
    let (rects, page) = layout::place_with_tier(page_width, width, height, &sizes, min_height);
    (rects, page_width, page)
}

/// Draws the screen's tiles on a page that may be taller than `main`, then
/// shows the part scrolled to, with a scrollbar when there's more.
fn draw_page(f: &mut Frame, app: &App, main: Rect, screen: &ScreenLayout, hits: &mut Hits) {
    let p = &app.palette;
    let (rects, width, page_height) = page_layout(app, screen, main.width, main.height);
    let max_scroll = page_height.saturating_sub(main.height);
    app.max_scroll.set(max_scroll);
    let scroll = app.scroll.min(max_scroll);

    let mut page = Buffer::empty(Rect::new(0, 0, width, page_height));
    let mut page_hits = Hits::default();
    let mut canvas = Canvas(&mut page);
    for (tile, area) in screen.tiles.iter().zip(rects) {
        match app.tiles.get(&tile.tile_id) {
            Some(update) => widgets::draw_tile(&mut canvas, p, area, update, &mut page_hits),
            None => widgets::draw_waiting(&mut canvas, p, area, &tile.tile_id),
        }
    }

    let buf = f.buffer_mut();
    for y in 0..main.height {
        for x in 0..width {
            buf[(main.x + x, main.y + y)] = page[(x, y + scroll)].clone();
        }
    }
    hits.extend_shifted(
        page_hits,
        main.x as i32,
        main.y as i32 - scroll as i32,
        main,
    );

    if max_scroll > 0 {
        let mut state = ScrollbarState::new(max_scroll as usize).position(scroll as usize);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight).style(Style::new().fg(p.muted)),
            main,
            &mut state,
        );
    }
}

/// Run `$VISUAL` or `$EDITOR` (default `vi`) on the config file and wait.
/// The variable may include arguments, e.g. `code -w`.
fn open_editor(path: &str) -> Result<()> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".into());
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or("vi");
    std::process::Command::new(program)
        .args(parts)
        .arg(path)
        .status()
        .map_err(|e| anyhow::anyhow!("running {program}: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashboard_contract::{
        ModuleSummary, ScreenLayout, Size, Status, Theme, TilePlacement, View, Widget,
    };
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn app() -> App {
        let tile = |id: &str, size| TilePlacement {
            tile_id: id.into(),
            size,
        };
        let settings = UiSettings {
            theme: Theme::Dark,
            screens: vec![
                ScreenLayout {
                    name: "work".into(),
                    tiles: vec![tile("sysperf", Size::Large), tile("cswap", Size::Medium)],
                },
                ScreenLayout {
                    name: "other".into(),
                    tiles: vec![],
                },
            ],
            modules: vec![ModuleSummary {
                tile_id: "sysperf".into(),
                name: "System".into(),
                refresh_ms: Some(1000),
                refresh_overridden: false,
                commands: vec![],
                default_size: Size::Medium,
            }],
            rotate: None,
            config: Default::default(),
        };
        let view = View {
            status: Status::Ok,
            summary: "fine".into(),
            root: Widget::Text {
                text: "hello from sysperf".into(),
                markdown: false,
            },
        };
        let tiles = BTreeMap::from([(
            "sysperf".to_string(),
            TileUpdate {
                tile_id: "sysperf".into(),
                view,
                updated_at: 0,
                stale: false,
                demo: false,
            },
        )]);
        App {
            settings,
            palette: Palette::for_theme(Theme::Dark, false),
            auto_light: false,
            tiles,
            screen: 0,
            overlay: Overlay::None,
            quit: false,
            rotation: None,
            actions: None,
            edit_config: false,
            hits: Hits::default(),
            expanded: None,
            last_click: None,
            hover: None,
            scroll: 0,
            max_scroll: Default::default(),
        }
    }

    /// Draw, then click at (x, y) the way the run loop does.
    fn click(app: &mut App, x: u16, y: u16) {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let mut hits = Hits::default();
        terminal
            .draw(|f| hits = draw(f, app, Instant::now()))
            .unwrap();
        if let Some(c) = hits.at(x, y).cloned() {
            app.on_click(c, Instant::now());
        }
    }

    #[test]
    fn clicks_switch_screens_and_open_overlays() {
        let mut app = app();
        // Tabs: " 1 work │ 2 other " - column 12 is inside "2 other".
        click(&mut app, 12, 0);
        assert_eq!(app.screen, 1);
        // Footer: " ? help  s settings ..." on the last row.
        click(&mut app, 10, 29);
        assert_eq!(app.overlay, Overlay::Settings);
        // Inside the overlay but not on a target: stays open.
        click(&mut app, 50, 15);
        assert_eq!(app.overlay, Overlay::Settings);
        // Outside: closes.
        click(&mut app, 0, 1);
        assert_eq!(app.overlay, Overlay::None);
        click(&mut app, 2, 29);
        assert_eq!(app.overlay, Overlay::Help);
        app.max_scroll.set(10);
        app.on_wheel(1, Instant::now());
        assert_eq!(app.scroll, 0, "wheel is ignored while an overlay is open");
        app.overlay = Overlay::None;
        app.on_wheel(1, Instant::now());
        assert_eq!(app.scroll, WHEEL_ROWS);
        app.on_wheel(-1, Instant::now());
        app.on_wheel(-1, Instant::now());
        assert_eq!(app.scroll, 0, "can't scroll above the top");
    }

    #[test]
    fn double_click_expands_a_tile() {
        let mut app = app();
        // The "sysperf" tile starts at the top left, under the tabs.
        click(&mut app, 5, 3);
        assert_eq!(app.expanded, None);
        click(&mut app, 5, 3);
        assert_eq!(app.expanded.as_deref(), Some("sysperf"));
        assert!(render(&app).contains("hello from sysperf"));
        click(&mut app, 5, 3);
        click(&mut app, 5, 3);
        assert_eq!(app.expanded, None);
    }

    #[test]
    fn hover_outlines_the_tile_and_highlights_tabs() {
        let mut app = app();
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        // Over the "sysperf" tile: its border turns accent, its inside doesn't.
        app.hover = Some((5, 3));
        terminal
            .draw(|f| {
                draw(f, &app, Instant::now());
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        assert_eq!(buf[(0, 1)].fg, app.palette.accent, "top-left corner");
        assert_ne!(buf[(5, 3)].bg, app.palette.hover);
        // Over a tab: highlighted background.
        app.hover = Some((3, 0));
        terminal
            .draw(|f| {
                draw(f, &app, Instant::now());
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        assert_eq!(buf[(3, 0)].bg, app.palette.hover);
        assert_ne!(
            buf[(0, 1)].fg,
            app.palette.accent,
            "tile no longer outlined"
        );
    }

    #[test]
    fn clicking_a_theme_sends_it() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut app = app();
        app.actions = Some(tx);
        app.overlay = Overlay::Settings;
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let mut hits = Hits::default();
        terminal
            .draw(|f| hits = draw(f, &app, Instant::now()))
            .unwrap();
        // The theme row is the second line inside the settings box; find "light" on it.
        let buf = terminal.backend().buffer().clone();
        let (x, y) = (0..30)
            .flat_map(|y| (0..96).map(move |x| (x, y)))
            .find(|&(x, y)| (0..5).map(|i| buf[(x + i, y)].symbol()).collect::<String>() == "light")
            .expect("theme choices are drawn");
        let c = hits.at(x, y).cloned().expect("theme choice is clickable");
        app.on_click(c, Instant::now());
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientMessage::SetTheme {
                theme: Theme::Light
            })
        ));
    }

    fn render(app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|f| {
                draw(f, app, Instant::now());
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn settings_overlay_changes_theme_and_requests_editor() {
        let mut app = app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.actions = Some(tx);

        // `t` outside settings does nothing.
        app.on_key(KeyEvent::from(KeyCode::Char('t')), Instant::now());
        assert!(rx.try_recv().is_err());

        app.on_key(KeyEvent::from(KeyCode::Char('s')), Instant::now());
        let out = render(&app);
        assert!(out.contains("Settings"), "{out}");
        assert!(out.contains("Config file"));
        assert!(out.contains("[dark]"));

        app.on_key(KeyEvent::from(KeyCode::Char('t')), Instant::now());
        assert_eq!(
            rx.try_recv().unwrap(),
            ClientMessage::SetTheme { theme: Theme::Auto }
        );

        app.on_key(KeyEvent::from(KeyCode::Char('e')), Instant::now());
        assert!(app.edit_config);

        app.on_key(KeyEvent::from(KeyCode::Esc), Instant::now());
        assert_eq!(app.overlay, Overlay::None);
    }

    #[test]
    fn draws_screen_tabs_tiles_and_help() {
        let mut app = app();
        let out = render(&app);
        assert!(out.contains("1 work"), "{out}");
        assert!(out.contains("2 other"));
        assert!(out.contains("hello from sysperf"));
        assert!(out.contains("waiting…"), "tile without a view yet");

        app.on_key(KeyEvent::from(KeyCode::Char('?')), Instant::now());
        let out = render(&app);
        assert!(out.contains("Help"));
        assert!(out.contains("System (sysperf)  every 1s"), "{out}");

        app.on_key(KeyEvent::from(KeyCode::Esc), Instant::now());
        app.on_key(KeyEvent::from(KeyCode::Tab), Instant::now());
        assert_eq!(app.screen, 1);
        assert!(render(&app).contains("no running modules"));
        app.on_key(KeyEvent::from(KeyCode::Char('1')), Instant::now());
        assert_eq!(app.screen, 0);
        app.on_key(KeyEvent::from(KeyCode::Char('q')), Instant::now());
        assert!(app.quit);
    }
}

/// One tile drawn as plain text (no colors), for `dashboard plugin-check`.
pub fn tile_text(tile: &dashboard_contract::TileUpdate, width: u16, height: u16) -> String {
    let area = ratatui::layout::Rect::new(0, 0, width, height);
    let mut buf = ratatui::buffer::Buffer::empty(area);
    let palette = theme::Palette::for_theme(dashboard_contract::Theme::Dark, false);
    widgets::draw_tile(
        &mut canvas::Canvas(&mut buf),
        &palette,
        area,
        tile,
        &mut mouse::Hits::default(),
    );
    (0..height)
        .map(|y| {
            let line: String = (0..width).map(|x| buf[(x, y)].symbol()).collect();
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Rows a tile needs at this width to be usable, borders included.
pub fn tile_min_height(tile: &dashboard_contract::TileUpdate, width: u16) -> u16 {
    widgets::tile_min_height(tile, width)
}

#[cfg(test)]
mod preview {
    //! `cargo test -p dashboard-tui preview -- --ignored --nocapture` prints a
    //! sample system tile, for eyeballing chart rendering without a terminal.
    use super::tests_support::*;

    #[test]
    #[ignore]
    fn preview_system_tile() {
        println!("{}", render_sample(110, 34));
    }
}

#[cfg(test)]
mod tests_support {
    use super::*;
    use dashboard_contract::{
        Bar, ColorScale, Meter, ModuleSummary, ScreenLayout, SeriesData, SeriesStyle, Size, Status,
        Theme, TilePlacement, ValueFormat, View, Widget,
    };
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn series(
        id: &str,
        style: SeriesStyle,
        color: ColorScale,
        mirrored: bool,
        data: Vec<Vec<f64>>,
    ) -> Widget {
        Widget::Series {
            label: id.into(),
            unit: if mirrored {
                Some("/s".into())
            } else {
                Some("%".into())
            },
            y_max: if mirrored { None } else { Some(100.0) },
            id: None,
            history: None,
            style,
            color,
            mirrored,
            format: if mirrored {
                ValueFormat::Bytes
            } else {
                ValueFormat::Plain
            },
            series: data
                .into_iter()
                .enumerate()
                .map(|(i, v)| SeriesData {
                    name: format!("s{i}"),
                    points: v
                        .into_iter()
                        .enumerate()
                        .map(|(t, y)| (t as i64, y))
                        .collect(),
                })
                .collect(),
        }
    }

    /// A dark-themed app with no screens or tiles.
    pub fn empty_app() -> App {
        App {
            settings: UiSettings {
                theme: Theme::Dark,
                screens: vec![],
                modules: vec![],
                rotate: None,
                config: Default::default(),
            },
            palette: Palette::for_theme(Theme::Dark, false),
            auto_light: false,
            tiles: BTreeMap::new(),
            screen: 0,
            overlay: Overlay::None,
            quit: false,
            rotation: None,
            actions: None,
            edit_config: false,
            hits: Hits::default(),
            expanded: None,
            last_click: None,
            hover: None,
            scroll: 0,
            max_scroll: Default::default(),
        }
    }

    pub fn render_sample(w: u16, h: u16) -> String {
        let wave = |n: usize, k: f64, amp: f64, base: f64| -> Vec<f64> {
            (0..n)
                .map(|i| base + amp * ((i as f64) * k).sin().abs())
                .collect()
        };
        let root = Widget::Column {
            children: vec![
                series(
                    "CPU",
                    SeriesStyle::Area,
                    ColorScale::Heat,
                    false,
                    vec![wave(120, 0.11, 70.0, 10.0)],
                ),
                Widget::Row {
                    children: vec![
                        series(
                            "Bars",
                            SeriesStyle::Bars,
                            ColorScale::Cool,
                            false,
                            vec![wave(60, 0.3, 80.0, 5.0)],
                        ),
                        series(
                            "Sunset",
                            SeriesStyle::Area,
                            ColorScale::Sunset,
                            false,
                            vec![wave(120, 0.05, 90.0, 5.0)],
                        ),
                    ],
                },
                Widget::MeterGrid {
                    label: Some("Cores".into()),
                    meters: (0..10)
                        .map(|i| Meter {
                            label: format!("{}", i + 1),
                            percent: (i * 11) as f64,
                        })
                        .collect(),
                    color: ColorScale::Heat,
                },
                Widget::Row {
                    children: vec![
                        Widget::Gauge {
                            label: "Memory".into(),
                            percent: 62.0,
                            marker: None,
                            thresholds: None,
                            caption: Some("19.8 GiB / 32.0 GiB".into()),
                        },
                        Widget::Gauge {
                            label: "Swap".into(),
                            percent: 12.0,
                            marker: None,
                            thresholds: None,
                            caption: None,
                        },
                    ],
                },
                series(
                    "Network",
                    SeriesStyle::Area,
                    ColorScale::Accent,
                    true,
                    vec![
                        wave(120, 0.2, 900_000.0, 0.0),
                        wave(120, 0.07, 200_000.0, 0.0),
                    ],
                ),
                Widget::Bars {
                    label: Some("Top processes (CPU %)".into()),
                    unit: Some("%".into()),
                    format: ValueFormat::Plain,
                    max: Some(100.0),
                    bars: vec![
                        Bar {
                            label: "cargo".into(),
                            value: 88.0,
                            status: None,
                        },
                        Bar {
                            label: "Google Chrome Helper".into(),
                            value: 31.5,
                            status: None,
                        },
                        Bar {
                            label: "node".into(),
                            value: 4.0,
                            status: None,
                        },
                    ],
                },
            ],
        };
        let settings = UiSettings {
            theme: Theme::Dark,
            screens: vec![ScreenLayout {
                name: "system".into(),
                tiles: vec![TilePlacement {
                    tile_id: "sysperf".into(),
                    size: Size::Large,
                }],
            }],
            modules: vec![ModuleSummary {
                tile_id: "sysperf".into(),
                name: "System".into(),
                refresh_ms: Some(1000),
                refresh_overridden: false,
                commands: vec![],
                default_size: Size::Medium,
            }],
            rotate: None,
            config: Default::default(),
        };
        let view = View {
            status: Status::Ok,
            summary: "sample".into(),
            root,
        };
        let tiles = BTreeMap::from([(
            "sysperf".to_string(),
            TileUpdate {
                tile_id: "sysperf".into(),
                view,
                updated_at: 0,
                stale: false,
                demo: false,
            },
        )]);
        let app = App {
            settings,
            palette: Palette::for_theme(Theme::Dark, false),
            auto_light: false,
            tiles,
            screen: 0,
            overlay: Overlay::None,
            quit: false,
            rotation: None,
            actions: None,
            edit_config: false,
            hits: Hits::default(),
            expanded: None,
            last_click: None,
            hover: None,
            scroll: 0,
            max_scroll: Default::default(),
        };
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                draw(f, &app, Instant::now());
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn sample_renders_every_section() {
        let out = render_sample(110, 34);
        for needle in [
            "CPU",
            "Cores",
            "Memory",
            "Network",
            "↓",
            "Top processes",
            "cargo",
        ] {
            assert!(out.contains(needle), "missing {needle}:\n{out}");
        }
    }
}
