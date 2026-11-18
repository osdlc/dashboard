//! The screen editor and its catalog: `ScreenEditor.vue`, `CatalogPalette.vue`
//! and `AddModuleDialog.vue`. It works on a copy; nothing changes until Save.
//! Choices are rows of buttons where the web view has select boxes.

use std::collections::{HashMap, HashSet};

use dashboard_contract::{
    CatalogEntry, CatalogResponse, ClientMessage, EntryKind, ScreenLayout, Size, TilePlacement,
    TileUpdate, UiSettings,
};
use gpui::{
    Context, Div, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Point, Render,
    SharedString, Stateful, Window, div, prelude::*, px,
};

use crate::app::{Dashboard, PendingAdd};
use crate::catalog::{Field, FieldKind, build_config, fields, free_id, id_error, installed};
use crate::grid;
use crate::input::TextField;
use crate::panels::Ui;
use crate::widgets::Draw;

const SIZES: [(Size, &str, &str); 5] = [
    (Size::Small, "small", "Small · 3×1"),
    (Size::Medium, "medium", "Medium · 4×2"),
    (Size::Large, "large", "Large · 6×3"),
    (Size::Wide, "wide", "Wide · 8×2"),
    (Size::Tall, "tall", "Tall · 4×4"),
];

fn size_name(size: Size) -> &'static str {
    SIZES
        .iter()
        .find(|(s, _, _)| *s == size)
        .map_or("medium", |(_, name, _)| name)
}

/// The text field that has the keyboard.
#[derive(Clone, Debug, PartialEq)]
pub enum Active {
    ScreenName,
    Search,
    ModuleId,
    Config(String),
}

pub struct Picked {
    pub entry: CatalogEntry,
    pub form: Vec<Field>,
    pub tile_id: TextField,
    pub size: Size,
    pub demo: bool,
    pub values: HashMap<String, TextField>,
    pub errors: HashMap<String, String>,
}

struct Pending {
    module_id: String,
    before: u64,
    screen: usize,
    size: Size,
}

struct Menu {
    index: usize,
    at: Point<Pixels>,
}

pub struct Editor {
    draft: Vec<ScreenLayout>,
    selected: usize,
    name: TextField,
    active: Option<Active>,
    query: TextField,
    tag: String,
    /// There can be a hundred tags; they stay folded until asked for.
    tags_open: bool,
    catalog: Option<(Vec<CatalogEntry>, Vec<String>)>,
    picked: Option<Picked>,
    pending: Option<Pending>,
    add_error: String,
    menu: Option<Menu>,
}

/// A tile being dragged to a new place on the screen.
struct DragTile {
    index: usize,
    id: String,
}

struct DragPreview(String);

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(6.0))
            .bg(gpui::black().opacity(0.6))
            .text_color(gpui::white())
            .child(self.0.clone())
    }
}

impl Editor {
    pub fn new(settings: &UiSettings, start: usize) -> Self {
        let draft = settings.screens.clone();
        let selected = start.min(draft.len().saturating_sub(1));
        let name = TextField::new(draft.get(selected).map_or("", |s| s.name.as_str()));
        Self {
            draft,
            selected,
            name,
            active: None,
            query: TextField::default(),
            tag: String::new(),
            tags_open: false,
            catalog: None,
            picked: None,
            pending: None,
            add_error: String::new(),
            menu: None,
        }
    }

    fn select(&mut self, index: usize) {
        self.selected = index;
        self.name = TextField::new(self.draft.get(index).map_or("", |s| s.name.as_str()));
        self.active = None;
    }

    fn sync_name(&mut self) {
        if let Some(screen) = self.draft.get_mut(self.selected) {
            screen.name = self.name.text.clone();
        }
    }

    fn error(&self) -> Option<&'static str> {
        let names: Vec<&str> = self.draft.iter().map(|s| s.name.trim()).collect();
        if names.iter().any(|n| n.is_empty()) {
            return Some("Every screen needs a name.");
        }
        let unique: HashSet<&&str> = names.iter().collect();
        (unique.len() != names.len()).then_some("Screen names must be different.")
    }

    fn add_screen(&mut self) {
        let mut n = self.draft.len() + 1;
        while self.draft.iter().any(|s| s.name == format!("screen {n}")) {
            n += 1;
        }
        self.draft.push(ScreenLayout {
            name: format!("screen {n}"),
            tiles: vec![],
        });
        self.select(self.draft.len() - 1);
    }

    fn remove_screen(&mut self) {
        if self.selected < self.draft.len() {
            self.draft.remove(self.selected);
        }
        self.select(self.selected.saturating_sub(1));
    }

    fn move_tile(&mut self, from: usize, to: usize) {
        let Some(screen) = self.draft.get_mut(self.selected) else {
            return;
        };
        if from >= screen.tiles.len() || to >= screen.tiles.len() || from == to {
            return;
        }
        let tile = screen.tiles.remove(from);
        screen.tiles.insert(to, tile);
    }

    fn taken(&self, settings: &UiSettings) -> HashSet<String> {
        let mut ids: HashSet<String> = settings.modules.iter().map(|m| m.tile_id.clone()).collect();
        for screen in settings.screens.iter().chain(&self.draft) {
            ids.extend(screen.tiles.iter().map(|t| t.tile_id.clone()));
        }
        ids
    }

    /// The saved screen a tile goes on: the one being edited, if it is saved
    /// under that name.
    fn saved_screen(&self, settings: &UiSettings) -> Option<String> {
        let name = self.draft.get(self.selected)?.name.trim().to_string();
        settings
            .screens
            .iter()
            .any(|s| s.name == name)
            .then_some(name)
    }

    fn pick(&mut self, entry: CatalogEntry, taken: &HashSet<String>) {
        let form = fields(entry.config_schema.as_ref());
        self.picked = Some(Picked {
            tile_id: TextField::new(free_id(&entry.id, taken)),
            size: entry.default_size,
            demo: false,
            values: form
                .iter()
                .map(|f| (f.key.clone(), TextField::default()))
                .collect(),
            errors: HashMap::new(),
            form,
            entry,
        });
        self.active = Some(Active::ModuleId);
        self.add_error.clear();
    }

    fn active_field(&mut self) -> Option<(&mut TextField, bool)> {
        match self.active.clone()? {
            Active::ScreenName => Some((&mut self.name, false)),
            Active::Search => Some((&mut self.query, false)),
            Active::ModuleId => self.picked.as_mut().map(|p| (&mut p.tile_id, false)),
            Active::Config(key) => {
                let picked = self.picked.as_mut()?;
                let multiline = picked
                    .form
                    .iter()
                    .any(|f| f.key == key && matches!(f.kind, FieldKind::Lines | FieldKind::Json));
                picked.values.get_mut(&key).map(|v| (v, multiline))
            }
        }
    }

    /// The host writes the config, reloads, and says how the add went. Once
    /// written, the tile goes in the draft even if the module fails to start,
    /// so the next Save keeps it in the file.
    pub fn settings_changed(&mut self, settings: &UiSettings) {
        let Some(pending) = &self.pending else {
            return;
        };
        let Some(added) = &settings.config.added else {
            return;
        };
        if added.module_id != pending.module_id || added.seq <= pending.before {
            return;
        }
        let Some(pending) = self.pending.take() else {
            return;
        };
        if let Some(error) = &added.error {
            self.add_error = error.lines().next().unwrap_or("").to_string();
            return;
        }
        if let Some(target) = self.draft.get_mut(pending.screen)
            && !target.tiles.iter().any(|t| t.tile_id == pending.module_id)
        {
            let placed = settings
                .screens
                .iter()
                .flat_map(|s| &s.tiles)
                .find(|t| t.tile_id == pending.module_id);
            target.tiles.push(TilePlacement {
                tile_id: pending.module_id,
                size: placed.map_or(pending.size, |t| t.size),
            });
        }
    }
}

impl Dashboard {
    pub fn load_catalog(&mut self, cx: &mut Context<Self>) {
        let source = self.shared.catalog.clone();
        let reading = self.shared.runtime.spawn_blocking(move || source());
        cx.spawn(async move |this, cx| {
            // Reading packages touches the disk.
            let Ok((entries, problems)) = reading.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(editor) = &mut this.editor {
                    editor.catalog = Some((entries, problems));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn editor_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let Some(editor) = &mut self.editor else {
            return;
        };
        if keystroke.key == "escape" {
            if editor.menu.is_some() {
                editor.menu = None;
            } else if editor.picked.is_some() {
                editor.picked = None;
                editor.active = None;
            } else {
                self.editor = None;
            }
            return;
        }
        if keystroke.modifiers.platform && keystroke.key == "v" {
            let pasted = cx.read_from_clipboard().and_then(|c| c.text());
            if let (Some(text), Some((field, multiline))) = (pasted, editor.active_field()) {
                let text = if multiline {
                    text
                } else {
                    text.replace('\n', " ")
                };
                field.insert(&text);
            }
        } else if let Some((field, multiline)) = editor.active_field() {
            field.key(event, multiline);
        }
        if editor.active == Some(Active::ScreenName) {
            editor.sync_name();
        }
    }

    fn submit_add(&mut self) {
        let taken = match &self.editor {
            Some(editor) => editor.taken(&self.settings),
            None => return,
        };
        let etag_matches = self.settings.config.etag == self.edit_base;
        let before = self.settings.config.added.as_ref().map_or(0, |a| a.seq);
        let screen_name = self
            .editor
            .as_ref()
            .and_then(|e| e.saved_screen(&self.settings));
        let Some(editor) = &mut self.editor else {
            return;
        };
        let selected = editor.selected;
        let busy = editor.pending.is_some();
        let Some(picked) = &mut editor.picked else {
            return;
        };
        let values: HashMap<String, String> = picked
            .values
            .iter()
            .map(|(k, v)| (k.clone(), v.text.clone()))
            .collect();
        let (config, errors) = build_config(&picked.form, &values);
        picked.errors = errors;
        let id = picked.tile_id.text.trim().to_string();
        if id_error(&id, &taken).is_some()
            || !picked.errors.is_empty()
            || !installed(&picked.entry)
            || busy
        {
            return;
        }
        let message = ClientMessage::AddModule {
            module_id: id.clone(),
            entry: picked.entry.id.clone(),
            kind: picked.entry.kind,
            config: serde_json::Value::Object(config),
            demo: picked.demo,
            screen: screen_name,
            size: picked.size,
        };
        editor.pending = Some(Pending {
            module_id: id.clone(),
            before,
            screen: selected,
            size: picked.size,
        });
        editor.add_error.clear();
        editor.picked = None;
        editor.active = None;
        self.pending_add = Some(PendingAdd {
            module_id: id,
            before,
            on_base: etag_matches,
        });
        self.send(message);
    }

    fn save_screens(&mut self) {
        let Some(editor) = &self.editor else {
            return;
        };
        if editor.error().is_some() {
            return;
        }
        let screens = editor
            .draft
            .iter()
            .map(|s| ScreenLayout {
                name: s.name.trim().to_string(),
                tiles: s.tiles.clone(),
            })
            .collect();
        self.send(ClientMessage::SaveScreens {
            screens,
            etag: self.edit_base.clone(),
        });
        self.editor = None;
    }

    /// A text field; the caret shows while it has the keyboard.
    fn field_view(
        &self,
        ui: &Ui,
        which: Active,
        field: &TextField,
        placeholder: &str,
        multiline: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let editing = self.editor.as_ref().and_then(|e| e.active.as_ref()) == Some(&which);
        let color = if field.text.is_empty() {
            ui.p.get("muted")
        } else {
            ui.p.get("text")
        };
        let caret = |height: f32| {
            div()
                .flex_none()
                .w(px(1.0))
                .h(px(height))
                .bg(ui.p.get("text"))
        };
        let line_h = ui.m.line(0.85);
        let mut lines = vec![];
        if field.text.is_empty() {
            lines.push(
                div()
                    .flex()
                    .h(px(line_h))
                    .children(editing.then(|| caret(line_h)))
                    .child(placeholder.lines().next().unwrap_or("").to_string()),
            );
        } else {
            let mut start = 0;
            for text in field.text.split('\n') {
                let end = start + text.len();
                let line = div().flex().h(px(line_h)).whitespace_nowrap();
                let line = if editing && (start..=end).contains(&field.caret) {
                    let at = field.caret - start;
                    line.child(text[..at].to_string())
                        .child(caret(line_h))
                        .child(text[at..].to_string())
                } else {
                    line.child(text.to_string())
                };
                lines.push(line);
                start = end + 1;
            }
        }
        let rows = if multiline { 3 } else { 1 };
        let height = lines.len().max(rows) as f32 * line_h + 12.0;
        let id = SharedString::from(format!("field-{which:?}"));
        div()
            .id(id)
            .flex()
            .flex_col()
            .flex_none()
            .overflow_hidden()
            .h(px(height))
            .px(px(8.0))
            .py(px(5.0))
            .rounded(px(6.0))
            .border_1()
            .border_color(if editing {
                ui.p.get("accent")
            } else {
                ui.p.get("border")
            })
            .bg(ui.p.get("bg"))
            .text_color(color)
            .text_size(px(ui.m.font(0.85)))
            .line_height(px(line_h))
            .cursor_text()
            .children(lines)
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(editor) = &mut this.editor {
                    editor.active = Some(which.clone());
                    editor.menu = None;
                    cx.notify();
                }
            }))
    }

    pub fn editor_view(&mut self, ui: &Ui, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let Some(editor) = &self.editor else {
            return div().into_any_element();
        };
        let view_w = f32::from(ui.window.width);
        let page_w = view_w - 32.0;
        let side_by_side = view_w > 900.0;
        let main_w = if side_by_side { page_w - 316.0 } else { page_w };
        let dirty = editor.draft != self.settings.screens;
        let error = editor.error();
        let tabs = editor.draft.iter().enumerate().map(|(i, s)| {
            let label = if s.name.is_empty() {
                "(unnamed)".to_string()
            } else {
                s.name.clone()
            };
            ui.button(("draft", i), label, i == editor.selected)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(editor) = &mut this.editor {
                        editor.select(i);
                        cx.notify();
                    }
                }))
        });
        let status = if let Some(error) = error {
            Some(ui.text(0.85, error).text_color(ui.p.get("error")))
        } else if !editor.add_error.is_empty() {
            Some(
                ui.text(0.85, format!("Not added: {}", editor.add_error))
                    .text_color(ui.p.get("error")),
            )
        } else {
            editor
                .pending
                .as_ref()
                .map(|p| ui.muted(0.85, format!("Adding {}…", p.module_id)))
        };
        let bar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(10.0))
            .mb(px(12.0))
            .child(ui.heading("Edit screens", 1.0))
            .child(
                div().flex().flex_wrap().gap(px(4.0)).children(tabs).child(
                    ui.button("new-screen", "+ New screen", false)
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(editor) = &mut this.editor {
                                editor.add_screen();
                                cx.notify();
                            }
                        })),
                ),
            )
            .child(div().flex_1())
            .children(status)
            .child(
                ui.button("cancel", "Cancel", false)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.editor = None;
                        cx.notify();
                    })),
            )
            .child(ui.primary("save", "Save", dirty && error.is_none()).when(
                dirty && error.is_none(),
                |b| {
                    b.on_click(cx.listener(|this, _, _, cx| {
                        this.save_screens();
                        cx.notify();
                    }))
                },
            ));
        let can_delete = editor.draft.len() > 1;
        let screen_bar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(10.0))
            .mb(px(12.0))
            .child(ui.muted(0.85, "Name"))
            .child(
                self.field_view(ui, Active::ScreenName, &editor.name, "", false, cx)
                    .w(px(200.0)),
            )
            .child(
                ui.button("delete-screen", "Delete screen", false)
                    .when(!can_delete, |b| b.opacity(0.45))
                    .when(can_delete, |b| {
                        b.on_click(cx.listener(|this, _, _, cx| {
                            if let Some(editor) = &mut this.editor {
                                editor.remove_screen();
                                cx.notify();
                            }
                        }))
                    }),
            )
            .child(ui.muted(
                0.8,
                "Right-click a tile to resize, move or remove it. Drag tiles to reorder.",
            ));
        let grid = self.editor_grid(ui, main_w, view_w, cx);
        let palette = self.palette_view(ui, cx);
        let body = if side_by_side {
            div()
                .flex()
                .items_start()
                .gap(px(16.0))
                .child(div().flex_none().w(px(main_w)).child(grid))
                .child(palette.flex_none().w(px(300.0)))
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(16.0))
                .child(grid)
                .child(palette)
        };
        let editor = self.editor.as_ref().expect("checked above");
        let dialog = editor
            .picked
            .as_ref()
            .map(|picked| self.dialog_view(ui, picked, cx).into_any_element());
        let menu = editor
            .menu
            .as_ref()
            .map(|menu| self.menu_view(ui, menu, cx).into_any_element());
        div()
            .relative()
            .size_full()
            .child(
                div()
                    .id("editor")
                    .size_full()
                    .overflow_y_scroll()
                    .p(px(16.0))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(editor) = &mut this.editor
                            && editor.menu.take().is_some()
                        {
                            cx.notify();
                        }
                    }))
                    .child(bar)
                    .child(screen_bar)
                    .child(body),
            )
            .children(dialog)
            .children(menu)
            .into_any_element()
    }

    fn editor_grid(
        &self,
        ui: &Ui,
        width: f32,
        window_width: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let editor = self.editor.as_ref().expect("the editor is open");
        let tiles = editor
            .draft
            .get(editor.selected)
            .map_or(vec![], |s| s.tiles.clone());
        let sizes: Vec<Size> = tiles.iter().map(|t| t.size).collect();
        let m = ui.m;
        let (boxes, height) = grid::place(&sizes, width, window_width, m.gap, m.row);
        let cards = tiles.iter().zip(&boxes).enumerate().map(|(i, (tile, b))| {
            let draw = Draw {
                interactive: false,
                ..self.draw(ui.p, m)
            };
            let update = self.tiles.get(&tile.tile_id);
            let small = tile.size == Size::Small;
            let face = draw
                .tile(&tile.tile_id, update, b.width, Some(b.height), small)
                .size_full();
            let badge = ui
                .button(("badge", i), format!("{} ▾", size_name(tile.size)), false)
                .bg(ui.p.get("surface"))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        if let Some(editor) = &mut this.editor {
                            editor.menu = Some(Menu {
                                index: i,
                                at: event.position,
                            });
                            cx.notify();
                        }
                    }),
                );
            let id = tile.tile_id.clone();
            let accent = ui.p.get("accent");
            div()
                .id(("edit-tile", i))
                .absolute()
                .left(px(b.x))
                .top(px(b.y))
                .w(px(b.width))
                .h(px(b.height))
                .cursor_grab()
                .rounded(px(8.0))
                .drag_over::<DragTile>(move |style, _, _, _| style.border_2().border_color(accent))
                .child(face)
                .child(div().absolute().top(px(8.0)).right(px(8.0)).child(badge))
                .on_drag(DragTile { index: i, id }, |drag, _, _, cx| {
                    cx.new(|_| DragPreview(drag.id.clone()))
                })
                .on_drop(cx.listener(move |this, drag: &DragTile, _, cx| {
                    if let Some(editor) = &mut this.editor {
                        editor.move_tile(drag.index, i);
                        cx.notify();
                    }
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        if let Some(editor) = &mut this.editor {
                            editor.menu = Some(Menu {
                                index: i,
                                at: event.position,
                            });
                            cx.notify();
                        }
                    }),
                )
        });
        let empty = tiles.is_empty().then(|| {
            ui.muted(
                1.0,
                "Empty screen. Add components from the list on the right.",
            )
        });
        div()
            .relative()
            .w(px(width))
            .h(px(height.max(ui.m.row)))
            .children(cards.collect::<Vec<_>>())
            .children(empty)
    }

    fn menu_view(&self, ui: &Ui, menu: &Menu, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let editor = self.editor.as_ref().expect("the editor is open");
        let index = menu.index;
        let tiles = editor
            .draft
            .get(editor.selected)
            .map_or(0, |s| s.tiles.len());
        let current = editor
            .draft
            .get(editor.selected)
            .and_then(|s| s.tiles.get(index))
            .map(|t| t.size);
        let x = f32::from(menu.at.x).min(f32::from(ui.window.width) - 200.0);
        let y = f32::from(menu.at.y).min(f32::from(ui.window.height) - 280.0);
        let hover = ui.p.get("surface-2");
        let item = |id: SharedString, label: String, color: gpui::Hsla, enabled: bool| {
            div()
                .id(id)
                .px(px(10.0))
                .py(px(5.0))
                .rounded(px(4.0))
                .text_color(color)
                .text_size(px(ui.m.font(0.9)))
                .when(enabled, move |d| {
                    d.cursor_pointer().hover(move |s| s.bg(hover))
                })
                .when(!enabled, |d| d.opacity(0.45))
                .child(label)
        };
        let sizes = SIZES.iter().map(|(size, name, label)| {
            let size = *size;
            let on = current == Some(size);
            let text = if on {
                format!("● {label}")
            } else {
                label.to_string()
            };
            let color = if on {
                ui.p.get("accent")
            } else {
                ui.p.get("text")
            };
            item(
                SharedString::from(format!("size-{name}")),
                text,
                color,
                true,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(editor) = &mut this.editor {
                    let selected = editor.selected;
                    if let Some(tile) = editor
                        .draft
                        .get_mut(selected)
                        .and_then(|s| s.tiles.get_mut(index))
                    {
                        tile.size = size;
                    }
                    editor.menu = None;
                    cx.notify();
                }
            }))
        });
        let text = ui.p.get("text");
        let separator = || div().my(px(4.0)).h(px(1.0)).bg(ui.p.get("border"));
        let earlier = item(
            "earlier".into(),
            "Move earlier".to_string(),
            text,
            index > 0,
        )
        .when(index > 0, |d| {
            d.on_click(cx.listener(move |this, _, _, cx| {
                if let Some(editor) = &mut this.editor {
                    editor.move_tile(index, index - 1);
                    editor.menu = None;
                    cx.notify();
                }
            }))
        });
        let can_later = index + 1 < tiles;
        let later =
            item("later".into(), "Move later".to_string(), text, can_later).when(can_later, |d| {
                d.on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(editor) = &mut this.editor {
                        editor.move_tile(index, index + 1);
                        editor.menu = None;
                        cx.notify();
                    }
                }))
            });
        let remove = item(
            "remove".into(),
            "Remove from screen".to_string(),
            ui.p.get("error"),
            true,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(editor) = &mut this.editor {
                let selected = editor.selected;
                if let Some(screen) = editor.draft.get_mut(selected)
                    && index < screen.tiles.len()
                {
                    screen.tiles.remove(index);
                }
                editor.menu = None;
                cx.notify();
            }
        }));
        div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(190.0))
            .occlude()
            .flex()
            .flex_col()
            .p(px(6.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(ui.p.get("border"))
            .bg(ui.p.get("surface"))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(ui.muted(0.72, "Size").px(px(10.0)).py(px(2.0)))
            .children(sizes)
            .child(separator())
            .child(earlier)
            .child(later)
            .child(separator())
            .child(remove)
    }

    /// The "add" list: running modules go straight on the screen; a catalog
    /// entry opens the add dialog.
    fn palette_view(&self, ui: &Ui, cx: &mut Context<Self>) -> Stateful<Div> {
        let editor = self.editor.as_ref().expect("the editor is open");
        let query = editor.query.text.clone();
        let words: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let on_screen: HashSet<&str> = editor
            .draft
            .get(editor.selected)
            .map(|s| s.tiles.iter().map(|t| t.tile_id.as_str()).collect())
            .unwrap_or_default();
        let mut running: Vec<_> = self
            .settings
            .modules
            .iter()
            .filter(|m| {
                let hay = format!("{} {}", m.tile_id, m.name).to_lowercase();
                words.iter().all(|w| hay.contains(w))
            })
            .collect();
        running.sort_by(|a, b| a.tile_id.cmp(&b.tile_id));
        let running_ids: HashSet<&str> = self
            .settings
            .modules
            .iter()
            .map(|m| m.tile_id.as_str())
            .collect();
        let response = editor.catalog.as_ref().map(|(all, problems)| {
            let tag = (!editor.tag.is_empty()).then_some(editor.tag.as_str());
            CatalogResponse::new(all, &query, tag, problems.clone())
        });
        let border = ui.p.get("accent");
        let item = |id: SharedString, title: String, sub: String, faded: bool| {
            div()
                .id(id)
                .flex()
                .flex_col()
                .flex_none()
                .w_full()
                .px(px(10.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(ui.p.get("border"))
                .bg(ui.p.get("surface"))
                .cursor_pointer()
                .hover(move |s| s.border_color(border))
                .when(faded, |d| d.opacity(0.75))
                .child(ui.text(0.9, title))
                .child(ui.muted(0.72, sub).line_clamp(2))
        };
        let tag_button = |id: SharedString, label: String, on: bool| {
            ui.button(id, label, on).px(px(8.0)).py(px(1.0))
        };
        let tag = editor.tag.clone();
        let tags_open = editor.tags_open;
        let tags = response.as_ref().map_or(vec![], |r| r.tags.clone());
        let tag_row = div()
            .flex()
            .flex_wrap()
            .flex_none()
            .gap(px(4.0))
            .child(
                tag_button("tag-all".into(), "all".to_string(), tag.is_empty()).on_click(
                    cx.listener(|this, _, _, cx| {
                        if let Some(editor) = &mut this.editor {
                            editor.tag.clear();
                            cx.notify();
                        }
                    }),
                ),
            )
            .child(
                tag_button(
                    "tag-more".into(),
                    if tags_open { "fewer ▴" } else { "tags ▾" }.to_string(),
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(editor) = &mut this.editor {
                        editor.tags_open = !editor.tags_open;
                        cx.notify();
                    }
                })),
            )
            .children(
                tags.into_iter()
                    .filter(|t| tags_open || *t == tag)
                    .map(|t| {
                        let on = t == tag;
                        let picked = t.clone();
                        tag_button(SharedString::from(format!("tag-{t}")), t, on).on_click(
                            cx.listener(move |this, _, _, cx| {
                                if let Some(editor) = &mut this.editor {
                                    editor.tag = if on { String::new() } else { picked.clone() };
                                    cx.notify();
                                }
                            }),
                        )
                    }),
            );
        let show_running = !running.is_empty() && tag.is_empty();
        let running_items: Vec<_> = running
            .iter()
            .filter(|_| show_running)
            .map(|m| {
                let used = on_screen.contains(m.tile_id.as_str());
                let title = if used {
                    format!("{} ✓", m.tile_id)
                } else {
                    m.tile_id.clone()
                };
                let sub = format!("{} · {}", m.name, size_name(m.default_size));
                let id = m.tile_id.clone();
                let size = m.default_size;
                item(SharedString::from(format!("run-{id}")), title, sub, false).on_click(
                    cx.listener(move |this, _, _, cx| {
                        if let Some(editor) = &mut this.editor {
                            let selected = editor.selected;
                            if let Some(screen) = editor.draft.get_mut(selected) {
                                screen.tiles.push(TilePlacement {
                                    tile_id: id.clone(),
                                    size,
                                });
                            }
                            cx.notify();
                        }
                    }),
                )
            })
            .collect();
        let entries = response.as_ref().map_or(vec![], |r| r.entries.clone());
        let nothing = response.is_some() && entries.is_empty();
        let problems = response.as_ref().map_or(vec![], |r| r.errors.clone());
        let catalog_items: Vec<_> = entries
            .into_iter()
            .map(|entry| {
                let kind = if !installed(&entry) {
                    " · marketplace"
                } else if entry.kind == EntryKind::Builtin {
                    ""
                } else {
                    " · plugin"
                };
                let dot = if running_ids.contains(entry.id.as_str()) {
                    " ●"
                } else {
                    ""
                };
                let title = format!("{}{kind}{dot}", entry.name);
                let sub = format!("{} · {}", entry.id, entry.description);
                let id = SharedString::from(format!("cat-{:?}-{}", entry.kind, entry.id));
                let faded = !installed(&entry);
                item(id, title, sub, faded).on_click(cx.listener(move |this, _, _, cx| {
                    let taken = this.editor.as_ref().map(|e| e.taken(&this.settings));
                    if let (Some(editor), Some(taken)) = (&mut this.editor, taken) {
                        editor.pick(entry.clone(), &taken);
                        cx.notify();
                    }
                }))
            })
            .collect();
        let heading = |text: &str| ui.muted(0.75, text.to_uppercase()).mt(px(8.0)).flex_none();
        div()
            .id("palette")
            .flex()
            .flex_col()
            .gap(px(6.0))
            .p(px(12.0))
            .max_h(px(f32::from(ui.window.height) - 32.0))
            .overflow_y_scroll()
            .rounded(px(10.0))
            .border_1()
            .border_color(ui.p.get("border"))
            .bg(ui.p.get("surface"))
            .child(ui.heading("Add component", 0.9))
            .child(self.field_view(
                ui,
                Active::Search,
                &editor.query,
                "Search widgets…",
                false,
                cx,
            ))
            .child(tag_row)
            .children(show_running.then(|| heading("Running")))
            .children(running_items)
            .child(heading(if response.is_none() {
                "Catalog …"
            } else {
                "Catalog"
            }))
            .children(nothing.then(|| ui.muted(0.85, "Nothing matches.")))
            .children(catalog_items)
            .children((!problems.is_empty()).then(|| {
                ui.muted(
                    0.75,
                    format!("{} problem(s) reading packages", problems.len()),
                )
            }))
            .children(problems.into_iter().map(|p| ui.muted(0.75, p)))
    }

    fn dialog_view(
        &self,
        ui: &Ui,
        picked: &Picked,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let editor = self.editor.as_ref().expect("the editor is open");
        let entry = &picked.entry;
        let width = (f32::from(ui.window.width) - 32.0).min(960.0);
        let taken = editor.taken(&self.settings);
        let id_problem = id_error(&picked.tile_id.text, &taken);
        let can_add = installed(entry);
        let busy = editor.pending.is_some();
        let screen = editor.saved_screen(&self.settings);
        let source = if entry.source == "bundled" {
            "bundled plugin"
        } else if entry.kind == EntryKind::Builtin {
            "built-in"
        } else if can_add {
            "plugin package"
        } else {
            "marketplace"
        };
        let close = |ui: &Ui, cx: &mut Context<Self>, id: &'static str, label: &'static str| {
            ui.button(id, label, false)
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(editor) = &mut this.editor {
                        editor.picked = None;
                        editor.active = None;
                        cx.notify();
                    }
                }))
        };
        let head = div()
            .flex()
            .justify_between()
            .items_start()
            .child(
                div()
                    .child(ui.heading(entry.name.clone(), 1.1))
                    .child(ui.muted(
                        0.85,
                        format!("{} · {source} · v{}", entry.id, entry.version),
                    )),
            )
            .child(close(ui, cx, "dialog-x", "✕"));
        let tags = div()
            .flex()
            .flex_wrap()
            .gap(px(4.0))
            .children(entry.tags.iter().map(|t| {
                ui.muted(0.72, t.clone())
                    .px(px(5.0))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(ui.p.get("border"))
            }));

        // The demo view, shaped like a live tile so the usual code draws it.
        let preview_update = TileUpdate {
            tile_id: entry.id.clone(),
            view: entry.demo.clone(),
            stale: false,
            demo: true,
            updated_at: 0,
        };
        let column_w = (width - 40.0 - 20.0) * 0.55;
        let (share, rows) = match picked.size {
            Size::Large => (1.0, 3.0),
            Size::Wide => (1.0, 2.0),
            Size::Medium => (0.75, 2.0),
            Size::Tall => (0.75, 4.0),
            Size::Small => (0.5, 1.0),
        };
        let preview_h = ui.m.row * rows + ui.m.gap * (rows - 1.0);
        let preview_w = column_w * share;
        let draw = Draw {
            interactive: false,
            ..self.draw(ui.p, ui.m)
        };
        let shown_id = if picked.tile_id.text.is_empty() {
            entry.id.as_str()
        } else {
            picked.tile_id.text.as_str()
        };
        let preview_face = draw
            .tile(
                shown_id,
                Some(&preview_update),
                preview_w,
                Some(preview_h),
                picked.size == Size::Small,
            )
            .w(px(preview_w))
            .h(px(preview_h));
        let req = &entry.requirements;
        let need = |label: &str, values: &[String]| {
            (!values.is_empty()).then(|| {
                div()
                    .flex()
                    .gap(px(12.0))
                    .child(ui.muted(0.8, label.to_string()).w(px(90.0)).flex_none())
                    .child(ui.text(0.8, values.join(", ")).flex_1())
            })
        };
        let preview = div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .w(px(column_w))
            .flex_none()
            .child(ui.section("Preview (made-up data)").mt(px(0.0)))
            .child(preview_face)
            .children(need("Programs", &req.binaries))
            .children(need("Environment", &req.env))
            .children(need("Calls", &req.hosts));

        let chip = |id: SharedString, label: String, on: bool| ui.button(id, label, on);
        let sizes: Vec<_> = SIZES
            .iter()
            .map(|(size, name, _)| {
                let size = *size;
                let label = if size == entry.default_size {
                    format!("{name} (suggested)")
                } else {
                    name.to_string()
                };
                chip(
                    SharedString::from(format!("add-size-{name}")),
                    label,
                    picked.size == size,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(picked) = this.editor.as_mut().and_then(|e| e.picked.as_mut()) {
                        picked.size = size;
                        cx.notify();
                    }
                }))
            })
            .collect();
        let demo = chip(
            "add-demo".into(),
            format!(
                "{} Demo data only (nothing runs; no token needed)",
                if picked.demo { "☑" } else { "☐" }
            ),
            picked.demo,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            if let Some(picked) = this.editor.as_mut().and_then(|e| e.picked.as_mut()) {
                picked.demo = !picked.demo;
                cx.notify();
            }
        }));
        let form_rows: Vec<_> = picked
            .form
            .iter()
            .map(|field| self.form_field(ui, picked, field, cx).into_any_element())
            .collect();
        let form = div()
            .id("add-form")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(10.0))
            .max_h(px(f32::from(ui.window.height) * 0.6))
            .overflow_y_scroll()
            .child(ui.section("Settings").mt(px(0.0)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(ui.heading("Module ID", 0.85))
                    .child(self.field_view(ui, Active::ModuleId, &picked.tile_id, "", false, cx))
                    .child(match &id_problem {
                        Some(problem) => {
                            ui.text(0.75, problem.clone()).text_color(ui.p.get("error"))
                        }
                        None => ui.muted(0.75, "The key under modules:."),
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(ui.heading("Size", 0.85))
                    .child(div().flex().flex_wrap().gap(px(4.0)).children(sizes)),
            )
            .child(demo)
            .children(
                picked
                    .form
                    .is_empty()
                    .then(|| ui.muted(0.85, "No settings.")),
            )
            .children(form_rows);
        let id_text = picked.tile_id.text.trim().to_string();
        let foot_text = if can_add {
            let on = screen
                .as_ref()
                .map_or(String::new(), |s| format!(" and a tile on “{s}”"));
            format!("Writes modules.{id_text}{on} to the config file; comments stay.")
        } else {
            format!(
                "Not installed. Install it first: dashboard plugin install {}",
                entry.id
            )
        };
        let enabled = can_add && id_problem.is_none() && !busy;
        let add_label = screen
            .as_ref()
            .map_or("Add".to_string(), |s| format!("Add to {s}"));
        let foot = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(10.0))
            .pt(px(10.0))
            .border_t_1()
            .border_color(ui.p.get("border"))
            .child(ui.muted(0.85, foot_text))
            .child(div().flex_1())
            .child(close(ui, cx, "dialog-cancel", "Cancel"))
            .child(
                ui.primary("dialog-add", add_label, enabled)
                    .when(enabled, |b| {
                        b.on_click(cx.listener(|this, _, _, cx| {
                            this.submit_add();
                            cx.notify();
                        }))
                    }),
            );
        div()
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .justify_center()
            .items_start()
            .p(px(16.0))
            .bg(gpui::black().opacity(0.4))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some(editor) = &mut this.editor {
                        editor.picked = None;
                        editor.active = None;
                        cx.notify();
                    }
                }),
            )
            .child(
                div()
                    .id("add-dialog")
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .w(px(width))
                    .max_h_full()
                    .overflow_y_scroll()
                    .px(px(20.0))
                    .py(px(16.0))
                    .rounded(px(10.0))
                    .border_1()
                    .border_color(ui.p.get("border"))
                    .bg(ui.p.get("surface"))
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(head)
                    .child(ui.text(0.95, entry.description.clone()))
                    .child(tags)
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap(px(20.0))
                            .child(preview)
                            .child(form),
                    )
                    .child(foot),
            )
    }

    fn form_field(
        &self,
        ui: &Ui,
        picked: &Picked,
        field: &Field,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let key = field.key.clone();
        let title = if field.required {
            format!("{key} *")
        } else {
            key.clone()
        };
        let value = picked.values.get(&key).cloned().unwrap_or_default();
        let control = match field.kind {
            FieldKind::Choice | FieldKind::Boolean => {
                let mut options = vec![(
                    String::new(),
                    if field.placeholder.is_empty() {
                        "default".to_string()
                    } else {
                        format!("default ({})", field.placeholder)
                    },
                )];
                if field.kind == FieldKind::Boolean {
                    options.push(("true".to_string(), "true".to_string()));
                    options.push(("false".to_string(), "false".to_string()));
                } else {
                    options.extend(field.choices.iter().cloned());
                }
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(4.0))
                    .children(options.into_iter().enumerate().map(|(i, (option, label))| {
                        let on = value.text == option;
                        let key = key.clone();
                        ui.button(SharedString::from(format!("opt-{key}-{i}")), label, on)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(picked) =
                                    this.editor.as_mut().and_then(|e| e.picked.as_mut())
                                    && let Some(field) = picked.values.get_mut(&key)
                                {
                                    *field = TextField::new(option.clone());
                                    cx.notify();
                                }
                            }))
                    }))
                    .into_any_element()
            }
            kind => self
                .field_view(
                    ui,
                    Active::Config(key.clone()),
                    &value,
                    &field.placeholder,
                    matches!(kind, FieldKind::Lines | FieldKind::Json),
                    cx,
                )
                .into_any_element(),
        };
        let suffix = match field.kind {
            FieldKind::Lines => " One per line.",
            FieldKind::Json => " As JSON.",
            _ => "",
        };
        let description = format!("{}{suffix}", field.description.replace('`', ""));
        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(3.0))
            .child(ui.text(0.85, title).font_weight(FontWeight::SEMIBOLD))
            .child(control)
            .children(
                picked
                    .errors
                    .get(&key)
                    .map(|e| ui.text(0.75, e.clone()).text_color(ui.p.get("error"))),
            )
            .children((!description.trim().is_empty()).then(|| ui.muted(0.75, description)))
    }
}
