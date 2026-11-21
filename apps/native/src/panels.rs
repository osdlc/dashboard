//! Shared controls, and the help and settings panels (`HelpPanel.vue`,
//! `SettingsPanel.vue`).

use gpui::{
    Context, Div, ElementId, FontWeight, KeyDownEvent, MouseButton, Pixels, SharedString, Size,
    Stateful, div, prelude::*, px,
};
use sitroom_contract::{ClientMessage, Theme, UiSettings};

use crate::app::{Dashboard, Overlay};
use crate::input::TextField;
use crate::theme::{Metrics, Palette};

/// Every key the app handles. The help panel is built from this table.
const KEYS: &[(&str, &str)] = &[
    ("?", "show or hide this help"),
    ("1-9", "go to screen"),
    (
        "← ↑ ↓ → / h j k l",
        "move between tiles; inside a tile, move between items",
    ),
    (
        "Enter",
        "go into the focused tile; on an item, open its link in the browser",
    ),
    ("[ / ]", "previous / next screen"),
    ("s", "show or hide settings"),
    (
        "d",
        "compact or comfortable spacing between and inside tiles",
    ),
    ("e", "edit screens: add, remove, resize and reorder tiles"),
    (
        "double-click",
        "show a tile fullscreen (Esc or double-click to close)",
    ),
    ("r", "start or stop screen rotation (when configured)"),
    ("PgUp PgDn Space Home End", "scroll the page"),
    (
        "Esc",
        "close help or settings; otherwise step back out of an item, fullscreen, then the tile",
    ),
];

fn format_ms(ms: u64) -> String {
    if ms.is_multiple_of(60_000) {
        format!("{}m", ms / 60_000)
    } else if ms.is_multiple_of(1000) {
        format!("{}s", ms / 1000)
    } else {
        format!("{ms}ms")
    }
}

fn refresh_text(ms: Option<u64>) -> String {
    ms.map_or("push".to_string(), format_ms)
}

/// The "new dashboard" dialog: a name, and whether to copy the one in use.
pub struct NewDashboard {
    pub name: TextField,
    pub copy: bool,
    pub error: Option<String>,
    /// The outcome counter when the request went out, while waiting for the answer.
    pub waiting: Option<u64>,
}

/// Colors, spacing and the window size, for building controls.
pub struct Ui<'a> {
    pub p: &'a Palette,
    pub m: Metrics,
    pub window: Size<Pixels>,
}

impl Ui<'_> {
    pub fn text(&self, rem: f32, text: impl Into<SharedString>) -> Div {
        div()
            .text_size(px(self.m.font(rem)))
            .line_height(px(self.m.line(rem)))
            .child(text.into())
    }

    pub fn muted(&self, rem: f32, text: impl Into<SharedString>) -> Div {
        self.text(rem, text).text_color(self.p.get("muted"))
    }

    pub fn heading(&self, text: impl Into<SharedString>, rem: f32) -> Div {
        self.text(rem, text).font_weight(FontWeight::BOLD)
    }

    /// A small section heading: `.help h3`.
    pub fn section(&self, text: &str) -> Div {
        self.muted(0.85, text.to_uppercase())
            .mt(px(16.0))
            .mb(px(6.0))
    }

    /// A bordered button; `active` marks the current choice.
    pub fn button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        active: bool,
    ) -> Stateful<Div> {
        let hover = self.p.get("surface-2");
        div()
            .id(id)
            .flex_none()
            .px(px(10.0))
            .py(px(4.0))
            .rounded(px(6.0))
            .border_1()
            .border_color(if active {
                self.p.get("accent")
            } else {
                self.p.get("border")
            })
            .text_color(if active {
                self.p.get("text")
            } else {
                self.p.get("muted")
            })
            .text_size(px(self.m.font(0.85)))
            .line_height(px(self.m.line(0.85)))
            .whitespace_nowrap()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(label.into())
    }

    /// The filled button that does the dialog's main thing.
    pub fn primary(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        enabled: bool,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .flex_none()
            .px(px(10.0))
            .py(px(4.0))
            .rounded(px(6.0))
            .bg(self.p.get("accent"))
            .text_color(self.p.get("on-accent"))
            .text_size(px(self.m.font(0.85)))
            .line_height(px(self.m.line(0.85)))
            .whitespace_nowrap()
            .when(enabled, |b| b.cursor_pointer())
            .when(!enabled, |b| b.opacity(0.45))
            .child(label.into())
    }
}

impl Dashboard {
    pub fn open_new_dashboard(&mut self, copy: bool) {
        self.overlay = Overlay::None;
        self.new_dashboard = Some(NewDashboard {
            name: TextField::default(),
            copy,
            error: None,
            waiting: None,
        });
    }

    fn submit_new_dashboard(&mut self) {
        let before = self.settings.config.created.as_ref().map_or(0, |c| c.seq);
        let Some(new) = &mut self.new_dashboard else {
            return;
        };
        if new.waiting.is_some() || new.name.text.trim().is_empty() {
            return;
        }
        new.waiting = Some(before);
        new.error = None;
        let message = ClientMessage::CreateDashboard {
            name: new.name.text.clone(),
            copy: new.copy,
        };
        self.send(message);
    }

    /// Reads how a request from this window went. Closes the dialog once the
    /// dashboard exists; returns true if it was a blank one.
    pub fn dashboard_created(&mut self, settings: &UiSettings) -> bool {
        let Some(new) = &mut self.new_dashboard else {
            return false;
        };
        let (Some(before), Some(created)) = (new.waiting, &settings.config.created) else {
            return false;
        };
        if created.seq <= before {
            return false;
        }
        new.waiting = None;
        if let Some(error) = &created.error {
            new.error = Some(error.lines().next().unwrap_or("").to_string());
            return false;
        }
        let blank = !new.copy;
        self.new_dashboard = None;
        blank
    }

    pub fn new_dashboard_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        match keystroke.key.as_str() {
            "escape" => self.new_dashboard = None,
            "enter" => self.submit_new_dashboard(),
            "v" if keystroke.modifiers.platform => {
                let pasted = cx.read_from_clipboard().and_then(|c| c.text());
                if let (Some(text), Some(new)) = (pasted, &mut self.new_dashboard) {
                    new.name.insert(&text.replace('\n', " "));
                }
            }
            _ => {
                if let Some(new) = &mut self.new_dashboard {
                    new.name.key(event, false);
                }
            }
        }
    }

    pub fn new_dashboard_panel(
        &self,
        ui: &Ui,
        new: &NewDashboard,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let width = (f32::from(ui.window.width) - 32.0).min(460.0);
        let line = ui.m.line(0.95);
        let (before, after) = new.name.text.split_at(new.name.caret);
        let field = div()
            .flex()
            .h(px(line + 12.0))
            .px(px(8.0))
            .py(px(6.0))
            .rounded(px(6.0))
            .border_1()
            .border_color(ui.p.get("accent"))
            .bg(ui.p.get("bg"))
            .text_size(px(ui.m.font(0.95)))
            .line_height(px(line))
            .whitespace_nowrap()
            .overflow_hidden()
            .child(before.to_string())
            .child(
                div()
                    .flex_none()
                    .w(px(1.0))
                    .h(px(line))
                    .bg(ui.p.get("text")),
            )
            .child(after.to_string());
        let choice = |id: &'static str, label: &'static str, copy: bool, cx: &mut Context<Self>| {
            ui.button(id, label, new.copy == copy)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(new) = &mut this.new_dashboard {
                        new.copy = copy;
                        cx.notify();
                    }
                }))
        };
        let current = self
            .settings
            .config
            .dashboard
            .clone()
            .unwrap_or("config.yaml".to_string());
        let hint = if new.copy {
            format!("A copy of {current}, as its own file you can change.")
        } else {
            "One empty screen. Modules in config.yaml keep running; add what you want to show."
                .to_string()
        };
        let ready = new.waiting.is_none() && !new.name.text.trim().is_empty();
        div()
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .p(px(16.0))
            .bg(gpui::black().opacity(0.4))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.new_dashboard = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .w(px(width))
                    .px(px(20.0))
                    .py(px(16.0))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(ui.p.get("border"))
                    .bg(ui.p.get("surface"))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(ui.heading(
                        if new.copy {
                            "Save sitroom as"
                        } else {
                            "New dashboard"
                        },
                        1.1,
                    ))
                    .child(ui.muted(0.85, "Name"))
                    .child(field)
                    .child(
                        div()
                            .flex()
                            .gap(px(6.0))
                            .child(choice("new-blank", "Blank", false, cx))
                            .child(choice("new-copy", "Copy of current", true, cx)),
                    )
                    .child(ui.muted(0.85, hint))
                    .children(
                        new.error
                            .clone()
                            .map(|error| ui.text(0.85, error).text_color(ui.p.get("error"))),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(8.0))
                            .child(
                                ui.button("new-cancel", "Cancel", false)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.new_dashboard = None;
                                        cx.notify();
                                    })),
                            )
                            .child(ui.primary("new-create", "Create", ready).when(ready, |b| {
                                b.on_click(cx.listener(|this, _, _, cx| {
                                    this.submit_new_dashboard();
                                    cx.notify();
                                }))
                            })),
                    ),
            )
    }

    /// A dimmed backdrop and a centered panel. A click outside closes it.
    fn panel(
        &self,
        ui: &Ui,
        title: &str,
        height: f32,
        body: Vec<gpui::AnyElement>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let width = (f32::from(ui.window.width) - 32.0).min(640.0);
        div()
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .p(px(16.0))
            .bg(gpui::black().opacity(0.4))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.overlay = Overlay::None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("panel")
                    .w(px(width))
                    .max_h(px(height * 0.85))
                    .overflow_y_scroll()
                    .track_scroll(&self.panel_scroll)
                    .px(px(20.0))
                    .py(px(16.0))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(ui.p.get("border"))
                    .bg(ui.p.get("surface"))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(ui.heading(title.to_string(), 1.1))
                            .child(
                                ui.button("close-panel", "Close", false)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.overlay = Overlay::None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .children(body),
            )
    }

    pub fn help_panel(
        &self,
        ui: &Ui,
        height: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let mono = |text: &str| {
            ui.text(0.8, text.to_string())
                .font_family("Menlo")
                .flex_none()
                .px(px(5.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(ui.p.get("border"))
        };
        let keys = KEYS.iter().map(|(key, what)| {
            div()
                .flex()
                .items_start()
                .gap(px(16.0))
                .mb(px(4.0))
                .child(div().flex().w(px(215.0)).flex_none().child(mono(key)))
                .child(ui.text(0.95, what.to_string()).flex_1().min_w_0())
        });
        let mut body: Vec<gpui::AnyElement> = vec![
            ui.section("Keys").into_any_element(),
            div().children(keys).into_any_element(),
            ui.section("Screens").into_any_element(),
        ];
        for (i, screen) in self.settings.screens.iter().enumerate() {
            let line = div()
                .flex()
                .gap(px(6.0))
                .when(i == self.screen, |d| d.font_weight(FontWeight::SEMIBOLD))
                .child(ui.text(0.95, format!("{}. {}", i + 1, screen.name)))
                .child(ui.muted(0.8, format!("({} tiles)", screen.tiles.len())));
            body.push(line.into_any_element());
        }
        body.push(ui.section("Modules").into_any_element());
        for module in &self.settings.modules {
            let line =
                div()
                    .mb(px(2.0))
                    .child(
                        div()
                            .flex()
                            .gap(px(6.0))
                            .child(ui.text(0.95, module.name.clone()))
                            .child(ui.muted(
                                0.8,
                                format!(
                                    "({}) every {}",
                                    module.tile_id,
                                    refresh_text(module.refresh_ms)
                                ),
                            )),
                    )
                    .children((!module.commands.is_empty()).then(|| {
                        ui.muted(0.8, format!("commands: {}", module.commands.join(", ")))
                    }));
            body.push(line.into_any_element());
        }
        self.panel(ui, "Help", height, body, cx)
    }

    pub fn settings_panel(
        &self,
        ui: &Ui,
        height: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let config = &self.settings.config;
        let mut body: Vec<gpui::AnyElement> = vec![
            ui.section("Config file").into_any_element(),
            ui.text(0.8, config.path.clone())
                .font_family("Menlo")
                .into_any_element(),
        ];
        if !config.exists {
            body.push(
                ui.muted(0.85, "Not created yet; showing the built-in demo.")
                    .into_any_element(),
            );
        }
        if let Some(error) = &config.error {
            body.push(
                div()
                    .mt(px(6.0))
                    .text_color(ui.p.get("error"))
                    .child(ui.text(0.85, "Not applied (previous config still in effect)"))
                    .child(ui.text(0.8, error.clone()).font_family("Menlo"))
                    .into_any_element(),
            );
        }
        body.push(
            ui.muted(
                0.85,
                "Edit the file directly for screens and modules; changes apply while running.",
            )
            .mt(px(6.0))
            .into_any_element(),
        );
        if !config.layers.is_empty() {
            body.push(ui.section("Config layers").into_any_element());
            body.push(
                ui.muted(
                    0.85,
                    "Merged in this order; later files win. Settings are saved to sitroom file, else config.yaml.",
                )
                .into_any_element(),
            );
            for layer in &config.layers {
                body.push(
                    ui.text(0.8, layer.clone())
                        .font_family("Menlo")
                        .mt(px(4.0))
                        .into_any_element(),
                );
            }
        }
        body.push(ui.section("Theme").into_any_element());
        let etag = config.etag.clone();
        let themes = [
            (Theme::Auto, "auto"),
            (Theme::Light, "light"),
            (Theme::Dark, "dark"),
        ]
        .into_iter()
        .map(|(theme, label)| {
            let etag = etag.clone();
            ui.button(label, label, self.settings.theme == theme)
                .on_click(cx.listener(move |this, _, _, _| {
                    this.send(ClientMessage::SetTheme {
                        theme,
                        etag: etag.clone(),
                    });
                }))
        });
        body.push(
            div()
                .flex()
                .gap(px(6.0))
                .children(themes)
                .into_any_element(),
        );
        body.push(ui.section("Colors").into_any_element());
        let colors = self
            .settings
            .color_themes
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let etag = etag.clone();
                let picked = name.clone();
                ui.button(
                    ("color", i),
                    name.clone(),
                    &self.settings.color_theme.name == name,
                )
                .on_click(cx.listener(move |this, _, _, _| {
                    this.send(ClientMessage::SetColorTheme {
                        name: picked.clone(),
                        etag: etag.clone(),
                    });
                }))
            });
        body.push(
            div()
                .flex()
                .flex_wrap()
                .gap(px(6.0))
                .children(colors)
                .into_any_element(),
        );
        body.push(
            ui.muted(
                0.85,
                "Saved to the config file; your comments and formatting are kept.",
            )
            .mt(px(6.0))
            .into_any_element(),
        );
        body.push(ui.section("Rotation").into_any_element());
        body.push(match &self.settings.rotate {
            Some(rotate) => ui
                .text(
                    0.95,
                    format!(
                        "Every {} through {}; resumes {} after input.",
                        format_ms(rotate.interval_ms),
                        rotate.screens.join(", "),
                        format_ms(rotate.resume_after_ms),
                    ),
                )
                .into_any_element(),
            None => ui
                .muted(0.85, "Not configured. Add rotate: to the config.")
                .into_any_element(),
        });
        body.push(ui.section("Modules").into_any_element());
        let border = ui.p.get("border");
        let row = |a: gpui::AnyElement, b: Div, c: Div| {
            div()
                .flex()
                .items_center()
                .py(px(4.0))
                .border_t_1()
                .border_color(border)
                .child(div().flex_1().min_w_0().child(a))
                .child(b.w(px(80.0)).flex_none())
                .child(c.w(px(120.0)).flex_none())
        };
        body.push(
            row(
                ui.muted(0.85, "Module").into_any_element(),
                ui.muted(0.85, "Refresh"),
                ui.muted(0.85, "Source"),
            )
            .into_any_element(),
        );
        for module in &self.settings.modules {
            let name = div()
                .flex()
                .gap(px(6.0))
                .child(ui.text(0.85, module.name.clone()))
                .child(ui.muted(0.8, format!("({})", module.tile_id)));
            body.push(
                row(
                    name.into_any_element(),
                    ui.text(0.85, refresh_text(module.refresh_ms)),
                    ui.muted(
                        0.85,
                        if module.refresh_overridden {
                            "config"
                        } else {
                            "module default"
                        },
                    ),
                )
                .into_any_element(),
            );
        }
        self.panel(ui, "Settings", height, body, cx)
    }
}
