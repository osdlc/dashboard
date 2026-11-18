//! The dashboard view: its state, keys and top-level layout. It behaves like
//! the web view's `App.vue` and `AmbientApp.vue`, but reads the host directly.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashboard_contract::{CatalogEntry, ClientMessage, Theme, TileUpdate, UiSettings};
use dashboard_runtime::Host;
use gpui::{
    Context, FocusHandle, KeyDownEvent, MouseMoveEvent, ScrollHandle, SharedString, Task, Window,
    WindowAppearance, div, point, prelude::*, px,
};
use tokio::sync::{broadcast, mpsc, watch};

use crate::editor::Editor;
use crate::grid;
use crate::motion::Motion;
use crate::nav::{self, Bounds, Focus};
use crate::panels::{NewDashboard, Ui};
use crate::theme::{Metrics, Palette};
use crate::widgets::{Draw, LogScroll, run_action};

/// How long each screen shows in ambient mode when the config sets no rotation.
const AMBIENT_INTERVAL: Duration = Duration::from_secs(30);

/// What every window shares: the running host and how to talk to it.
#[derive(Clone)]
pub struct Shared {
    pub host: Host,
    pub settings: watch::Receiver<UiSettings>,
    pub actions: mpsc::UnboundedSender<ClientMessage>,
    pub catalog: Arc<dyn Fn() -> (Vec<CatalogEntry>, Vec<String>) + Send + Sync>,
    pub runtime: tokio::runtime::Handle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Window,
    /// No controls, large type, screens rotating.
    Ambient,
    /// Ambient on the desktop; the window ignores the pointer.
    Wallpaper,
    /// Ambient until Esc. A click shows a tile fullscreen.
    Screensaver,
}

impl Mode {
    fn ambient(self) -> bool {
        self != Mode::Window
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Overlay {
    None,
    Help,
    Settings,
}

#[derive(Clone, Copy, PartialEq)]
pub enum RotationState {
    On,
    Paused,
    Off,
    None,
}

/// Screen rotation, as in `web/src/useRotation.ts`: input pauses it until idle
/// for `resume_after_ms`; `r` turns it off and on. In ambient mode every screen
/// rotates even when the config sets no rotation, and input doesn't pause it.
pub struct Rotation {
    ambient: bool,
    enabled: bool,
    last_input: Option<Instant>,
    last_switch: Instant,
    last_move: Instant,
}

impl Rotation {
    fn new(ambient: bool) -> Self {
        Self {
            ambient,
            enabled: true,
            last_input: None,
            last_switch: Instant::now(),
            last_move: Instant::now(),
        }
    }

    fn order(&self, settings: &UiSettings) -> Vec<usize> {
        match &settings.rotate {
            None if self.ambient => (0..settings.screens.len()).collect(),
            None => vec![],
            Some(r) => r
                .screens
                .iter()
                .filter_map(|name| settings.screens.iter().position(|s| &s.name == name))
                .collect(),
        }
    }

    fn interval(&self, settings: &UiSettings) -> Option<Duration> {
        settings
            .rotate
            .as_ref()
            .map(|r| Duration::from_millis(r.interval_ms))
            .or(self.ambient.then_some(AMBIENT_INTERVAL))
    }

    pub fn state(&self, settings: &UiSettings) -> RotationState {
        if self.interval(settings).is_none() || self.order(settings).len() < 2 {
            return RotationState::None;
        }
        if self.ambient {
            return RotationState::On;
        }
        if !self.enabled {
            return RotationState::Off;
        }
        if let (Some(r), Some(input)) = (&settings.rotate, self.last_input)
            && input.elapsed() < Duration::from_millis(r.resume_after_ms)
        {
            return RotationState::Paused;
        }
        RotationState::On
    }

    pub fn input(&mut self) {
        let now = Instant::now();
        self.last_input = Some(now);
        self.last_switch = now;
    }

    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
        self.last_input = None;
        self.last_switch = Instant::now();
    }

    /// The screen to go to now, if it is time.
    fn tick(&mut self, settings: &UiSettings, current: usize) -> Option<usize> {
        let interval = self.interval(settings)?;
        if self.state(settings) != RotationState::On || self.last_switch.elapsed() < interval {
            return None;
        }
        self.last_switch = Instant::now();
        let order = self.order(settings);
        let at = order.iter().position(|i| *i == current);
        at.map_or(order.first(), |at| order.get((at + 1) % order.len()))
            .copied()
    }
}

/// An add sent from the editor: the outcome counter when it was sent, and
/// whether the editor's base was the file as it was then.
pub struct PendingAdd {
    pub module_id: String,
    pub before: u64,
    pub on_base: bool,
}

pub struct Dashboard {
    pub shared: Shared,
    pub mode: Mode,
    pub settings: UiSettings,
    pub tiles: HashMap<String, TileUpdate>,
    pub screen: usize,
    pub overlay: Overlay,
    pub focus: Focus,
    pub expanded: Option<String>,
    pub compact: bool,
    pub rotation: Rotation,
    pub editor: Option<Editor>,
    /// The config version the editor started from: a save over newer changes is refused.
    pub edit_base: Option<String>,
    pub pending_add: Option<PendingAdd>,
    /// The "new dashboard" dialog, while open.
    pub new_dashboard: Option<NewDashboard>,
    pub focus_handle: FocusHandle,
    pub page_scroll: ScrollHandle,
    pub panel_scroll: ScrollHandle,
    pub motion: Motion,
    pub logs: RefCell<HashMap<String, LogScroll>>,
    /// Where the tiles were last drawn, for the arrow keys.
    pub boxes: Vec<(String, Bounds)>,
    last_frame: Instant,
    /// A redraw waiting for its turn; there is never more than one.
    next_frame: Option<Task<()>>,
    _tasks: Vec<Task<()>>,
}

impl Dashboard {
    pub fn new(shared: Shared, mode: Mode, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        cx.observe_window_appearance(window, |_, _, cx| cx.notify())
            .detach();
        let mut settings_rx = shared.settings.clone();
        let settings = settings_rx.borrow_and_update().clone();
        let mut updates = shared.host.subscribe();
        let host = shared.host.clone();
        let tiles = shared
            .runtime
            .block_on(async move { host.snapshot().await })
            .into_iter()
            .map(|t| (t.tile_id.clone(), t))
            .collect();
        let tile_feed = cx.spawn(async move |this, cx| {
            loop {
                match updates.recv().await {
                    Ok(tile) => {
                        let updated = this.update(cx, |this, cx| {
                            let shown = this.is_shown(&tile.tile_id);
                            this.tiles.insert(tile.tile_id.clone(), tile);
                            if shown {
                                this.redraw(cx);
                            }
                        });
                        if updated.is_err() {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        let settings_feed = cx.spawn(async move |this, cx| {
            while settings_rx.changed().await.is_ok() {
                let settings = settings_rx.borrow_and_update().clone();
                let updated = this.update(cx, |this, cx| {
                    if this.apply_settings(settings) {
                        this.open_editor(cx);
                    }
                    cx.notify();
                });
                if updated.is_err() {
                    return;
                }
            }
        });
        let clock = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let updated = this.update(cx, |this, cx| {
                    // A tile shown fullscreen holds the screen.
                    if this.expanded.is_none()
                        && let Some(next) = this.rotation.tick(&this.settings, this.screen)
                    {
                        this.go(next);
                    }
                    if !this.settings.rotate.is_none() {
                        cx.notify();
                    }
                });
                if updated.is_err() {
                    return;
                }
            }
        });
        Self {
            shared,
            mode,
            settings,
            tiles,
            screen: 0,
            overlay: Overlay::None,
            focus: Focus::None,
            expanded: None,
            compact: false,
            rotation: Rotation::new(mode.ambient()),
            editor: None,
            edit_base: None,
            pending_add: None,
            new_dashboard: None,
            focus_handle,
            page_scroll: ScrollHandle::new(),
            panel_scroll: ScrollHandle::new(),
            motion: Motion::default(),
            logs: RefCell::default(),
            boxes: vec![],
            last_frame: Instant::now(),
            next_frame: None,
            _tasks: vec![tile_feed, settings_feed, clock],
        }
    }

    /// Returns true when a new blank dashboard is ready to be filled in.
    fn apply_settings(&mut self, settings: UiSettings) -> bool {
        // Keep the same screen by name, as the web view's URL hash does.
        let current = self
            .settings
            .screens
            .get(self.screen)
            .map(|s| s.name.clone());
        let at = current.and_then(|name| settings.screens.iter().position(|s| s.name == name));
        self.screen = at
            .unwrap_or(0)
            .min(settings.screens.len().saturating_sub(1));
        // The add wrote the file and the editor puts its tile in the draft, so a
        // save builds on the new version; not when the file had already changed.
        if let (Some(pending), Some(added)) = (&self.pending_add, &settings.config.added)
            && added.module_id == pending.module_id
            && added.seq > pending.before
        {
            if added.error.is_none() && pending.on_base {
                self.edit_base = settings.config.etag.clone();
            }
            self.pending_add = None;
        }
        if let Some(editor) = &mut self.editor {
            editor.settings_changed(&settings);
        }
        let blank = self.dashboard_created(&settings);
        self.settings = settings;
        blank
    }

    /// Whether a change to this tile changes what's on screen. Most updates
    /// are for other screens; those wait until the screen is shown.
    fn is_shown(&self, tile: &str) -> bool {
        self.editor.is_some()
            || self.expanded.as_deref() == Some(tile)
            || self
                .settings
                .screens
                .get(self.screen)
                .is_some_and(|s| s.tiles.iter().any(|t| t.tile_id == tile))
    }

    fn frame_interval(&self) -> Duration {
        Duration::from_secs(1) / self.settings.native_fps.max(1)
    }

    /// Draw again, no sooner than `native.fps` allows. Data and animation come
    /// through here; input redraws at once.
    fn redraw(&mut self, cx: &mut Context<Self>) {
        if self.next_frame.is_some() {
            return;
        }
        let wait = self
            .frame_interval()
            .saturating_sub(self.last_frame.elapsed());
        self.next_frame = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |this, cx| {
                this.next_frame = None;
                cx.notify();
            });
        }));
    }

    pub fn send(&self, message: ClientMessage) {
        let _ = self.shared.actions.send(message);
    }

    /// Go to a screen, wrapping around.
    pub fn go(&mut self, index: usize) {
        let count = self.settings.screens.len();
        if count == 0 {
            return;
        }
        let next = index % count;
        if next == self.screen {
            return;
        }
        if let Some(screen) = self.settings.screens.get(next) {
            self.motion.forget_fade(&screen.name);
        }
        self.screen = next;
        self.focus = Focus::None;
        self.motion.clear();
        self.page_scroll.set_offset(point(px(0.0), px(0.0)));
    }

    pub fn open_editor(&mut self, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
        self.rotation.input();
        self.edit_base = self.settings.config.etag.clone();
        self.editor = Some(Editor::new(&self.settings, self.screen));
        self.load_catalog(cx);
    }

    fn focused_items(&self) -> Vec<nav::NavItem> {
        self.focus
            .tile()
            .and_then(|id| self.tiles.get(id))
            .map_or(vec![], |t| nav::items(&t.view.root))
    }

    /// Scroll the page so the focused tile shows.
    fn reveal_focus(&self, window: &Window) {
        let Some(id) = self.focus.tile() else {
            return;
        };
        let Some((_, tile)) = self.boxes.iter().find(|(t, _)| t == id) else {
            return;
        };
        // The header above the grid.
        const TOP: f32 = 64.0;
        let height = f32::from(window.viewport_size().height);
        let offset = -f32::from(self.page_scroll.offset().y);
        let (top, bottom) = (tile.y + TOP, tile.y + TOP + tile.height);
        if top < offset + TOP {
            self.page_scroll
                .set_offset(point(px(0.0), px(-(top - TOP).max(0.0))));
        } else if bottom > offset + height {
            self.page_scroll
                .set_offset(point(px(0.0), px(-(bottom - height + 16.0))));
        }
    }

    /// Arrow keys, Enter and Esc for tile focus. Returns true when the key was used.
    fn nav_key(&mut self, key: &str) -> bool {
        if let Some(dir) = nav::dir_for_key(key) {
            if self.boxes.is_empty() {
                return false;
            }
            let items = self.focused_items();
            self.focus = self.focus.moved(dir, &self.boxes, &items);
            return true;
        }
        if key == "enter" {
            if self.focus == Focus::None {
                return false;
            }
            let (focus, action) = self.focus.enter(&self.focused_items());
            self.focus = focus;
            if let Some(action) = action {
                run_action(&action);
            }
            return true;
        }
        if key == "escape" && matches!(self.focus, Focus::Item { .. }) {
            self.focus = self.focus.back();
            return true;
        }
        false
    }

    fn scroll_key(&self, key: &str, window: &Window) -> bool {
        let height = f32::from(window.viewport_size().height);
        let offset = f32::from(self.page_scroll.offset().y);
        let least = -f32::from(self.page_scroll.max_offset().y);
        let to = match key {
            "pageup" => offset + height * 0.9,
            "pagedown" | "space" => offset - height * 0.9,
            "home" => 0.0,
            "end" => least,
            _ => return false,
        };
        self.page_scroll
            .set_offset(point(px(0.0), px(to.clamp(least, 0.0))));
        true
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = &event.keystroke.modifiers;
        if modifiers.platform && event.keystroke.key == "q" {
            cx.quit();
            return;
        }
        if self.mode == Mode::Screensaver {
            // Esc steps back: out of a tile, then out of the screensaver.
            if event.keystroke.key == "escape" {
                if self.expanded.take().is_none() {
                    cx.quit();
                }
                cx.notify();
            }
            return;
        }
        if self.new_dashboard.is_some() {
            self.new_dashboard_key(event, cx);
            cx.notify();
            return;
        }
        if self.editor.is_some() {
            self.editor_key(event, cx);
            cx.notify();
            return;
        }
        if modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }
        // `?` arrives as shift-/: the character typed is what counts.
        let key = match event.keystroke.key_char.as_deref() {
            Some(typed) if !typed.trim().is_empty() => typed.to_string(),
            _ => event.keystroke.key.clone(),
        };
        let key = key.as_str();
        cx.notify();
        let ambient = self.mode.ambient();
        if !ambient && self.overlay == Overlay::None && self.nav_key(key) {
            self.rotation.input();
            self.reveal_focus(window);
            return;
        }
        if self.expanded.is_some() {
            if key == "escape" {
                self.expanded = None;
            }
            return;
        }
        if key == "r" && self.rotation.state(&self.settings) != RotationState::None {
            self.rotation.toggle();
            return;
        }
        self.rotation.input();
        if !ambient
            && self.overlay == Overlay::None
            && self.scroll_key(event.keystroke.key.as_str(), window)
        {
            return;
        }
        match key {
            "?" if !ambient => self.toggle_overlay(Overlay::Help),
            "s" if !ambient => self.toggle_overlay(Overlay::Settings),
            "e" if !ambient => self.open_editor(cx),
            "d" if !ambient => self.compact = !self.compact,
            "escape" if self.overlay == Overlay::None => self.focus = self.focus.back(),
            "escape" => self.overlay = Overlay::None,
            "]" => self.go(self.screen + 1),
            "[" => self.go(self.screen + self.settings.screens.len().max(1) - 1),
            _ => {
                if let Ok(n) = key.parse::<usize>()
                    && (1..=9).contains(&n)
                    && n <= self.settings.screens.len()
                {
                    self.go(n - 1);
                }
            }
        }
    }

    pub fn toggle_overlay(&mut self, overlay: Overlay) {
        self.overlay = if self.overlay == overlay {
            Overlay::None
        } else {
            overlay
        };
        self.panel_scroll.set_offset(point(px(0.0), px(0.0)));
    }

    fn on_mouse_move(&mut self, _: &MouseMoveEvent, _: &mut Window, _: &mut Context<Self>) {
        // Moves fire constantly; count at most one per second.
        if !self.mode.ambient() && self.rotation.last_move.elapsed() > Duration::from_secs(1) {
            self.rotation.last_move = Instant::now();
            self.rotation.input();
        }
    }

    fn metrics(&self, window: &Window) -> Metrics {
        if self.mode.ambient() {
            Metrics::ambient(f32::from(window.viewport_size().width))
        } else if self.compact {
            Metrics::compact()
        } else {
            Metrics::comfortable()
        }
    }

    pub fn palette(&self, window: &Window) -> Palette {
        let dark = match self.settings.theme {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::Auto => matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        };
        Palette::new(self.settings.color_theme.variant(dark).clone())
    }

    pub fn draw<'a>(&'a self, palette: &'a Palette, metrics: Metrics) -> Draw<'a> {
        Draw {
            p: palette,
            m: metrics,
            motion: &self.motion,
            logs: &self.logs,
            selected: None,
            full: None,
            interactive: !self.mode.ambient(),
        }
    }

    fn selected_item(&self, tile: &TileUpdate) -> Option<usize> {
        if self.mode.ambient() || self.focus.tile() != Some(tile.tile_id.as_str()) {
            return None;
        }
        self.focus.item_index(&nav::items(&tile.view.root))
    }

    fn header(&self, ui: &Ui, cx: &mut Context<Self>) -> impl IntoElement {
        let config = &self.settings.config;
        let rotation = self.rotation.state(&self.settings);
        let rotation_text = match rotation {
            RotationState::On => "⟳ rotating",
            RotationState::Paused => "⟳ paused",
            RotationState::Off => "⟳ off",
            RotationState::None => "",
        };
        let dashboards = config.dashboard.is_some() || !config.dashboards.is_empty();
        let names = std::iter::once(None)
            .chain(config.dashboards.iter().map(|d| Some(d.name.clone())))
            .filter(|_| dashboards)
            .enumerate()
            .map(|(i, name)| {
                let active = name == config.dashboard;
                let label = name.clone().unwrap_or("config.yaml".to_string());
                ui.button(("dash", i), label, active)
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.send(ClientMessage::SwitchDashboard { name: name.clone() });
                    }))
            });
        let tabs = self.settings.screens.iter().enumerate().map(|(i, s)| {
            ui.button(
                ("screen", i),
                format!("{} {}", i + 1, s.name),
                i == self.screen,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.go(i);
                cx.notify();
            }))
        });
        div()
            .flex()
            .flex_wrap()
            .flex_none()
            .items_center()
            .gap(px(if self.compact { 8.0 } else { 12.0 }))
            .mb(px(if self.compact { 6.0 } else { 16.0 }))
            .child(ui.heading("Dashboard", 1.25))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(4.0))
                    .children(names)
                    .child(
                        ui.button("new-dashboard", "+ New", false)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_new_dashboard(false);
                                cx.notify();
                            })),
                    )
                    .child(
                        ui.button("save-as", "Save as…", false)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_new_dashboard(true);
                                cx.notify();
                            })),
                    ),
            )
            .child(div().flex().flex_wrap().gap(px(4.0)).children(tabs))
            .children((rotation != RotationState::None).then(|| {
                let color = if rotation == RotationState::On {
                    ui.p.get("accent")
                } else {
                    ui.p.get("muted")
                };
                div()
                    .id("rotation")
                    .cursor_pointer()
                    .text_size(px(ui.m.font(0.8)))
                    .text_color(color)
                    .child(rotation_text)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.rotation.toggle();
                        cx.notify();
                    }))
            }))
            .child(
                ui.button("edit", "✎ Edit", false)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_editor(cx);
                        cx.notify();
                    })),
            )
            .child(
                ui.button(
                    "density",
                    if self.compact {
                        "▦ Compact"
                    } else {
                        "▢ Roomy"
                    },
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.compact = !this.compact;
                    cx.notify();
                })),
            )
            .child(div().flex_1())
            .child(
                ui.button("settings", "⚙", false)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_overlay(Overlay::Settings);
                        cx.notify();
                    })),
            )
            .child(
                ui.button("help", "?", false)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_overlay(Overlay::Help);
                        cx.notify();
                    })),
            )
    }

    /// Lay out the current screen and remember where the tiles go, for the
    /// arrow keys.
    fn place_tiles(&mut self, width: f32, window_width: f32, m: Metrics) {
        let tiles = self
            .settings
            .screens
            .get(self.screen)
            .map_or(&[][..], |s| &s.tiles);
        let sizes: Vec<_> = tiles.iter().map(|t| t.size).collect();
        let (boxes, _) = grid::place(&sizes, width, window_width, m.gap, m.row);
        self.boxes = tiles.iter().map(|t| t.tile_id.clone()).zip(boxes).collect();
    }

    fn grid(
        &self,
        draw: &Draw<'_>,
        width: f32,
        window_width: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let screen = self.settings.screens.get(self.screen);
        let name = screen.map_or(String::new(), |s| s.name.clone());
        let tiles = screen.map_or(vec![], |s| s.tiles.clone());
        let sizes: Vec<_> = tiles.iter().map(|t| t.size).collect();
        let m = draw.m;
        let (boxes, height) = grid::place(&sizes, width, window_width, m.gap, m.row);
        let radius = if m.compact { 4.0 } else { 8.0 };
        let accent = draw.p.get("accent");
        let transparent = accent.opacity(0.0);
        let interactive = draw.interactive;
        let focused = self.focus.tile().map(str::to_string);
        let cards: Vec<_> = tiles
            .iter()
            .zip(&boxes)
            .enumerate()
            .map(|(i, (placement, b))| {
                let id = placement.tile_id.clone();
                let update = self.tiles.get(&id);
                let tile = Draw {
                    selected: update.and_then(|u| self.selected_item(u)),
                    ..self.draw(draw.p, m)
                };
                let small = placement.size == dashboard_contract::Size::Small;
                let face = tile
                    .tile(&id, update, b.width, Some(b.height), small)
                    .size_full();
                let group = SharedString::from(format!("tile-{i}"));
                let ring = interactive.then(|| {
                    let on = focused.as_deref() == Some(id.as_str());
                    let (inset, thick) = if on {
                        if m.compact { (3.0, 2.0) } else { (5.0, 3.0) }
                    } else {
                        (2.0, 2.0)
                    };
                    div()
                        .absolute()
                        .top(px(-inset))
                        .left(px(-inset))
                        .right(px(-inset))
                        .bottom(px(-inset))
                        .rounded(px(radius + inset))
                        .border(px(thick))
                        .border_color(if on { accent } else { transparent })
                        .group_hover(group.clone(), |s| s.border_color(accent))
                });
                let target = id.clone();
                let saver_target = id.clone();
                let saver = self.mode == Mode::Screensaver;
                div()
                    .id(("tile", i))
                    .group(group)
                    .absolute()
                    .left(px(b.x))
                    .top(px(b.y))
                    .w(px(b.width))
                    .h(px(b.height))
                    .child(face)
                    .children(ring)
                    .when(interactive, |card| {
                        card.on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                            if event.click_count() == 2 {
                                this.expanded = Some(target.clone());
                            }
                            if this.focus.tile() != Some(target.as_str()) {
                                this.focus = Focus::Tile(target.clone());
                            }
                            cx.notify();
                        }))
                    })
                    .when(saver, |card| {
                        card.on_click(cx.listener(move |this, _, _, cx| {
                            this.expanded = Some(saver_target.clone());
                            cx.notify();
                        }))
                    })
            })
            .collect();
        let empty = tiles.is_empty().then(|| {
            div()
                .text_color(draw.p.get("muted"))
                .child("This screen has no running modules.")
        });
        let fade = self.motion.fade_in(&name);
        div()
            .relative()
            .w(px(width))
            .h(px(height))
            .flex_none()
            .opacity(fade)
            .children(cards)
            .children(empty)
    }

    fn fullscreen(&self, id: &str, ui: &Ui, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let padding = if self.compact { 8.0 } else { 24.0 };
        let view_w = f32::from(ui.window.width);
        let view_h = f32::from(ui.window.height);
        let width = (view_w - padding * 2.0).min(1400.0);
        let update = self.tiles.get(id);
        let draw = Draw {
            selected: update.and_then(|u| self.selected_item(u)),
            full: Some(view_h - padding * 2.0),
            ..self.draw(ui.p, ui.m)
        };
        let face = draw.tile(id, update, width, None, false).min_h_full();
        div()
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .justify_center()
            .p(px(padding))
            .bg(gpui::black().opacity(0.4))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.expanded = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("full")
                    .relative()
                    .w(px(width))
                    .h_full()
                    .overflow_y_scroll()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(face)
                    .child(div().absolute().top(px(10.0)).right(px(10.0)).child(
                        ui.button("close-full", "✕", false).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.expanded = None;
                                cx.notify();
                            },
                        )),
                    )),
            )
    }

    fn ambient_footer(&self, ui: &Ui) -> impl IntoElement + use<> {
        let count = self.settings.screens.len();
        let name = self
            .settings
            .screens
            .get(self.screen)
            .map_or("", |s| s.name.as_str());
        let width = f32::from(ui.window.width);
        div()
            .absolute()
            .right(px(width * 0.02))
            .bottom(px(width * 0.01))
            .px(px(ui.m.rem * 0.5))
            .rounded(px(4.0))
            .bg(ui.p.get("bg"))
            .text_color(ui.p.get("muted"))
            .text_size(px(ui.m.font(0.8)))
            .children((count > 1).then(|| format!("{name} · {}/{count}", self.screen + 1)))
    }
}

impl Render for Dashboard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = window.viewport_size();
        let (view_w, view_h) = (f32::from(view.width), f32::from(view.height));
        let m = self.metrics(window);
        window.set_rem_size(px(m.rem));
        let palette = self.palette(window);
        self.motion.begin(self.settings.native_animations);
        self.last_frame = Instant::now();
        let ui = Ui {
            p: &palette,
            m,
            window: view,
        };

        let root = div()
            .id("root")
            .relative()
            .size_full()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .bg(palette.get("bg"))
            .text_color(palette.get("text"))
            .font_family(UI_FONT)
            .text_size(px(m.rem));

        let new_dashboard = self
            .new_dashboard
            .as_ref()
            .map(|new| self.new_dashboard_panel(&ui, new, cx).into_any_element());
        if self.editor.is_some() {
            let editor = self.editor_view(&ui, cx);
            if self.motion.moving() {
                self.redraw(cx);
            }
            return root.child(editor).into_any_element();
        }

        let width = (view_w - m.page * 2.0).min(m.max_width);
        self.place_tiles(width, view_w, m);
        let grid = self.grid(&self.draw(&palette, m), width, view_w, cx);
        let header = (self.mode == Mode::Window).then(|| self.header(&ui, cx).into_any_element());
        let error = self
            .settings
            .config
            .error
            .clone()
            .filter(|_| self.overlay != Overlay::Settings && self.mode == Mode::Window);
        let banner = error.map(|error| {
            let first = error.lines().next().unwrap_or("").to_string();
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .mt(px(12.0))
                .text_size(px(m.font(0.85)))
                .text_color(palette.get("error"))
                .child(format!("Config not applied: {first}"))
                .child(ui.button("details", "Details", false).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.overlay = Overlay::Settings;
                        cx.notify();
                    },
                )))
        });
        let page = div()
            .id("page")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.page_scroll)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(width))
                    .mx_auto()
                    .py(px(m.page))
                    .children(header)
                    .child(grid)
                    .children(banner),
            );
        let full = self
            .expanded
            .clone()
            .map(|id| self.fullscreen(&id, &ui, cx).into_any_element());
        let panel = match self.overlay {
            Overlay::None => None,
            Overlay::Help => Some(self.help_panel(&ui, view_h, cx).into_any_element()),
            Overlay::Settings => Some(self.settings_panel(&ui, view_h, cx).into_any_element()),
        };
        let footer = self
            .mode
            .ambient()
            .then(|| self.ambient_footer(&ui).into_any_element());
        if self.motion.moving() {
            self.redraw(cx);
        }
        root.child(page)
            .children(full)
            .children(panel)
            .children(footer)
            .children(new_dashboard)
            .into_any_element()
    }
}

pub const UI_FONT: &str = ".SystemUIFont";
