//! Drawing a tile. The same widgets and rules as the web view
//! (`web/src/components`, `web/src/style.css`). Every widget works out its own
//! height from fixed line heights as it is built, so it can be fitted to its
//! tile right here (`web/src/fit.ts` does it by measuring after drawing): lists
//! show the items that fit and "+N more", and a row or column leaves out the
//! child that doesn't fit and all after it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::f32::consts::TAU;

use gpui::{
    AnyElement, Background, ContentMask, Div, FontWeight, Hsla, PathBuilder, ScrollHandle,
    SharedString, StyledText, TextRun, Window, canvas, div, fill, font, linear_color_stop,
    linear_gradient, point, prelude::*, px, size,
};
use sitroom_contract::{
    Action, Bar, Cell, ColorScale, Column, ListItem, LogLine, Meter, Row, SeriesData, SeriesStyle,
    Slice, Status, TileUpdate, Tone, UptimeCheck, ValueFormat, Widget, format_value, rpc::LogLevel,
};

use crate::motion::{Motion, parse_number};
use crate::theme::{Metrics, Palette};

const UI_FONT: &str = ".SystemUIFont";
const MONO_FONT: &str = "Menlo";
/// Results an uptime strip keeps; older ones wouldn't fit anyway.
const UPTIME_SHOWN: usize = 90;

/// A scrolling log's state between frames: it follows new lines unless the
/// reader has scrolled up.
#[derive(Default)]
pub struct LogScroll {
    handle: ScrollHandle,
    following: bool,
    seen: bool,
    last_max: f32,
}

pub struct Draw<'a> {
    pub p: &'a Palette,
    pub m: Metrics,
    pub motion: &'a Motion,
    pub logs: &'a RefCell<HashMap<String, LogScroll>>,
    /// The selected item in this tile, in `nav::items` order.
    pub selected: Option<usize>,
    /// Fullscreen: the view scrolls instead of being fitted, and this is the
    /// height of the window.
    pub full: Option<f32>,
    /// False in ambient mode and previews: nothing reacts to the pointer.
    pub interactive: bool,
}

/// A built widget and its height.
struct Out {
    el: AnyElement,
    h: f32,
    /// It was fitted so tightly that none of its items show.
    none_shown: bool,
    one_line: bool,
}

impl Out {
    fn new(el: impl IntoElement, h: f32) -> Self {
        Self {
            el: el.into_any_element(),
            h,
            none_shown: false,
            one_line: false,
        }
    }
}

/// Open a link from a module. Only web links: a module can't make this open a
/// `file:` URL or an app's custom scheme. `run_command` isn't run yet, as in
/// the web view and the TUI.
pub fn run_action(action: &Action) {
    if let Action::OpenUrl { url } = action
        && is_web_url(url)
        && let Err(err) = sitroom_core::host::browser::open(url)
    {
        eprintln!("couldn't open the browser: {err}");
    }
}

fn is_web_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// How many selectable items a widget draws (see `nav::items`).
fn nav_count(widget: &Widget) -> usize {
    match widget {
        Widget::List { items, .. } => items.len(),
        Widget::Table { rows, .. } => rows.len(),
        Widget::Uptime { checks } => checks.len(),
        Widget::Bars { bars, .. } => bars.len(),
        Widget::Row { children } | Widget::Column { children } => {
            children.iter().map(nav_count).sum()
        }
        _ => 0,
    }
}

/// How many items, from the first, fit in `room` with a "+N more" line when
/// some are left out.
fn fit_items(heights: &[f32], room: f32, more: f32) -> usize {
    let total: f32 = heights.iter().sum();
    if total <= room + 0.5 {
        return heights.len();
    }
    let mut used = more;
    let mut count = 0;
    for h in heights {
        used += h;
        if used > room + 0.5 {
            break;
        }
        count += 1;
    }
    count
}

impl Draw<'_> {
    fn text(&self, rem: f32, color: Hsla) -> Div {
        div()
            .text_size(px(self.m.font(rem)))
            .line_height(px(self.m.line(rem)))
            .text_color(color)
    }

    /// One line of text; what doesn't fit ends with "…".
    fn line(&self, rem: f32, color: Hsla, text: impl Into<SharedString>) -> Div {
        self.text(rem, color)
            .flex_none()
            .min_w_0()
            .h(px(self.m.line(rem)))
            .truncate()
            .child(text.into())
    }

    fn muted(&self) -> Hsla {
        self.p.get("muted")
    }

    fn more_height(&self) -> f32 {
        self.m.line(0.75) + 2.0
    }

    fn more(&self, count: usize) -> Div {
        self.line(0.75, self.muted(), format!("+{count} more"))
            .mt(px(2.0))
    }

    fn label_height(&self) -> f32 {
        self.m.line(0.8) + 4.0
    }

    fn label(&self, text: &str) -> Div {
        self.line(0.8, self.muted(), text.to_string()).mb(px(4.0))
    }

    /// A selected row's band, with the accent stripe on its left edge.
    fn select(&self, row: Div, selected: bool) -> Div {
        if !selected {
            return row;
        }
        row.bg(self.p.get("accent-soft")).child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(px(3.0))
                .bg(self.p.get("accent")),
        )
    }

    fn number(&self, key: &str, value: f64) -> f64 {
        self.motion.number(key, value)
    }

    /// A horizontal track, filled to `percent`.
    fn track(&self, key: &str, percent: f64, height: f32, color: Hsla) -> Div {
        let shown = self.number(key, percent.clamp(0.0, 100.0));
        div()
            .relative()
            .flex_none()
            .h(px(height))
            .rounded(px(height / 2.0))
            .overflow_hidden()
            .bg(self.p.get("track"))
            .child(
                div()
                    .h_full()
                    .w(gpui::relative((shown / 100.0) as f32))
                    .bg(color),
            )
    }

    fn widget(
        &self,
        widget: &Widget,
        width: f32,
        room: f32,
        key: &str,
        nav: Option<usize>,
    ) -> Option<Out> {
        match widget {
            Widget::Text { text, markdown } => Some(self.paragraph(text, *markdown, width)),
            Widget::Stat {
                label,
                value,
                unit,
                delta,
                status,
            } => Some(self.stat(label, value, unit, delta, *status, key)),
            Widget::Gauge {
                label,
                percent,
                marker,
                thresholds,
                caption,
            } => {
                let level = match thresholds {
                    Some(t) if *percent >= t.error => "error",
                    Some(t) if *percent >= t.warn => "warn",
                    _ => "ok",
                };
                Some(self.gauge(
                    label,
                    *percent,
                    *marker,
                    self.p.get(level),
                    caption,
                    key,
                    nav.is_some(),
                ))
            }
            Widget::Series {
                label,
                unit,
                y_max,
                style,
                color,
                mirrored,
                format,
                series,
                ..
            } => self.series(
                SeriesView {
                    label,
                    unit: unit.as_deref(),
                    y_max: *y_max,
                    style: *style,
                    color: *color,
                    mirrored: *mirrored,
                    format: *format,
                    series,
                },
                room,
                key,
            ),
            Widget::MeterGrid {
                label,
                meters,
                color,
            } => Some(self.meters(label.as_deref(), meters, *color, width, room, key)),
            Widget::Bars {
                label,
                unit,
                format,
                max,
                bars,
            } => Some(self.bars(
                label.as_deref(),
                unit.as_deref(),
                *format,
                *max,
                bars,
                room,
                key,
                nav,
            )),
            Widget::Pie {
                label,
                slices,
                donut,
                format,
                unit,
                color,
            } => Some(self.pie(
                PieView {
                    label: label.as_deref(),
                    slices,
                    donut: *donut,
                    format: *format,
                    unit: unit.as_deref(),
                    color: *color,
                },
                width,
                room,
                key,
            )),
            Widget::List { items, empty_text } => {
                Some(self.list(items, empty_text.as_deref(), width, room, key, nav))
            }
            Widget::Table { columns, rows } => {
                Some(self.table(columns, rows, width, room, key, nav))
            }
            Widget::Log { label, lines } => self.log(label.as_deref(), lines, room, key),
            Widget::Uptime { checks } => Some(self.uptime(checks, width, room, key, nav)),
            Widget::ClockFace {
                hour,
                minute,
                second,
                label,
                caption,
                status,
            } => Some(self.clock_face(
                (*hour, *minute, *second),
                label.as_deref(),
                caption.as_deref(),
                *status,
                width,
                room,
            )),
            Widget::Display {
                text,
                label,
                caption,
                unit,
                status,
            } => Some(self.display(
                text,
                label.as_deref(),
                caption.as_deref(),
                unit.as_deref(),
                *status,
                width,
                room,
            )),
            Widget::Image { path, caption } => {
                Some(self.image(path, caption.as_deref(), width, room))
            }
            Widget::Row { children } => Some(self.row(children, width, room, key, nav)),
            Widget::Column { children } => Some(self.column(children, width, room, key, nav)),
        }
    }

    // ---- text and numbers ----

    fn paragraph(&self, text: &str, markdown: bool, width: f32) -> Out {
        let lines: usize = text
            .split('\n')
            .map(|l| ((self.m.text_width(l, 1.0) / width.max(1.0)).ceil() as usize).max(1))
            .sum();
        let body = self.text(1.0, self.p.get("text"));
        let el = if markdown {
            body.child(self.markdown(text))
        } else {
            body.child(text.to_string())
        };
        let mut out = Out::new(el, lines as f32 * self.m.line(1.0));
        out.one_line = lines == 1;
        out
    }

    /// `**bold**` and `` `code` ``, as in `inlineMarkdown` in `web/src/format.ts`.
    fn markdown(&self, text: &str) -> StyledText {
        let color = self.p.get("text");
        let mut plain = String::new();
        let mut runs = vec![];
        for (part, kind) in inline_markdown(text) {
            plain.push_str(part);
            let (face, background) = match kind {
                Inline::Plain => (font(UI_FONT), None),
                Inline::Strong => (font(UI_FONT).bold(), None),
                Inline::Code => (font(MONO_FONT), Some(self.p.get("track"))),
            };
            runs.push(TextRun {
                len: part.len(),
                font: face,
                color,
                background_color: background,
                underline: None,
                strikethrough: None,
            });
        }
        StyledText::new(plain).with_runs(runs)
    }

    fn stat(
        &self,
        label: &str,
        value: &str,
        unit: &Option<String>,
        delta: &Option<String>,
        status: Option<Status>,
        key: &str,
    ) -> Out {
        let shown = match parse_number(value) {
            Some((n, decimals)) => {
                let v = self.number(key, n);
                if v == n {
                    value.to_string()
                } else {
                    format!("{v:.decimals$}")
                }
            }
            None => value.to_string(),
        };
        let color = match status {
            Some(Status::Warn) => self.p.get("warn"),
            Some(Status::Error) => self.p.get("error"),
            _ => self.p.get("text"),
        };
        let mut h = self.m.line(0.8) + self.m.line(1.5);
        let mut el = div()
            .flex()
            .flex_col()
            .min_w_0()
            .child(self.line(0.8, self.muted(), label.to_string()))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .h(px(self.m.line(1.5)))
                    .overflow_hidden()
                    .child(
                        self.text(1.5, color)
                            .font_weight(FontWeight::SEMIBOLD)
                            .whitespace_nowrap()
                            .child(shown),
                    )
                    .children(unit.clone().map(|u| {
                        self.text(0.85, color)
                            .ml(px(4.0))
                            .whitespace_nowrap()
                            .child(u)
                    })),
            );
        if let Some(delta) = delta {
            el = el.child(self.line(0.8, self.muted(), delta.clone()));
            h += self.m.line(0.8);
        }
        Out::new(el, h)
    }

    fn status_color(&self, status: Option<Status>, otherwise: &str) -> Hsla {
        match status {
            Some(Status::Warn) => self.p.get("warn"),
            Some(Status::Error) => self.p.get("error"),
            _ => self.p.get(otherwise),
        }
    }

    /// Height left for a body between an optional label and caption. A body
    /// in a column or table cell (`room` unbounded) gets `otherwise`.
    fn body_room(&self, label: bool, caption: bool, room: f32, otherwise: f32) -> f32 {
        let around = label as u8 as f32 * self.label_height()
            + caption as u8 as f32 * (self.m.line(0.8) + 4.0);
        if room.is_finite() {
            (room - around).max(self.m.line(1.0))
        } else {
            otherwise
        }
    }

    fn framed(&self, label: Option<&str>, caption: Option<&str>, body: Div, body_h: f32) -> Out {
        let mut h = body_h;
        let mut el = div().flex().flex_col().min_w_0();
        if let Some(l) = label {
            el = el.child(self.label(l));
            h += self.label_height();
        }
        el = el.child(body);
        if let Some(c) = caption {
            el = el.child(self.line(0.8, self.muted(), c.to_string()).mt(px(4.0)));
            h += self.m.line(0.8) + 4.0;
        }
        Out::new(el, h)
    }

    /// One value as large as the space allows.
    #[allow(clippy::too_many_arguments)]
    fn display(
        &self,
        text: &str,
        label: Option<&str>,
        caption: Option<&str>,
        unit: Option<&str>,
        status: Option<Status>,
        width: f32,
        room: f32,
    ) -> Out {
        let fallback = self.m.line(3.0);
        let space = self.body_room(label.is_some(), caption.is_some(), room, fallback);
        // The unit is drawn at 0.4 of the text's size.
        let wide = self.m.text_width(text, 1.0)
            + unit.map_or(0.0, |u| self.m.text_width(u, 0.4) + self.m.font(0.15));
        let by_width = width * 0.95 / wide.max(1.0);
        let by_height = space / self.m.line(1.0);
        let rem = by_width.min(by_height).clamp(1.0, 12.0);
        let color = self.status_color(status, "text");
        let body_h = self.m.line(rem);
        let body = div()
            .flex()
            .items_baseline()
            .justify_center()
            .h(px(body_h))
            .overflow_hidden()
            .child(
                self.text(rem, color)
                    .font_weight(FontWeight::SEMIBOLD)
                    .whitespace_nowrap()
                    .child(text.to_string()),
            )
            .children(unit.map(|u| {
                self.text(rem * 0.4, color)
                    .ml(px(self.m.font(rem * 0.15)))
                    .whitespace_nowrap()
                    .child(u.to_string())
            }));
        self.framed(label, caption, body, body_h)
    }

    /// An analog dial: hour, minute and an optional second hand.
    fn clock_face(
        &self,
        (hour, minute, second): (u8, u8, Option<u8>),
        label: Option<&str>,
        caption: Option<&str>,
        status: Option<Status>,
        width: f32,
        room: f32,
    ) -> Out {
        let space = self.body_room(label.is_some(), caption.is_some(), room, width.min(160.0));
        let disc = width.min(space).max(24.0);
        let rim = self.status_color(status, "muted");
        let ink = self.p.get("text");
        let accent = self.p.get("accent");
        let (h, m, s) = (hour as f32 % 12.0, minute as f32, second.map(f32::from));
        let turns = [
            ((h + m / 60.0) / 12.0, 0.5, disc * 0.035, ink),
            (
                (m + s.unwrap_or(0.0) / 60.0) / 60.0,
                0.75,
                disc * 0.025,
                ink,
            ),
        ];
        let second_hand = s.map(|s| (s / 60.0, 0.8, disc * 0.01, accent));
        let face = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let r = disc / 2.0;
                let c = bounds.origin + point(px(r), px(r));
                let at = |turn: f32, radius: f32| {
                    let a = turn * TAU;
                    c + point(px(radius * a.sin()), px(-radius * a.cos()))
                };
                let edge = r - disc * 0.02;
                let mut outline = PathBuilder::stroke(px(disc * 0.02 + 0.5));
                let ring: Vec<_> = (0..=120).map(|i| at(i as f32 / 120.0, edge)).collect();
                outline.add_polygon(&ring, true);
                if let Ok(path) = outline.build() {
                    window.paint_path(path, rim);
                }
                for i in 0..12 {
                    let long = i % 3 == 0;
                    let mut tick = PathBuilder::stroke(px(disc * if long { 0.025 } else { 0.012 }));
                    tick.move_to(at(i as f32 / 12.0, edge * if long { 0.82 } else { 0.88 }));
                    tick.line_to(at(i as f32 / 12.0, edge * 0.95));
                    if let Ok(path) = tick.build() {
                        window.paint_path(path, rim);
                    }
                }
                for (turn, length, thickness, color) in turns.into_iter().chain(second_hand) {
                    let mut hand = PathBuilder::stroke(px(thickness.max(1.0)));
                    hand.move_to(c);
                    hand.line_to(at(turn, edge * length));
                    if let Ok(path) = hand.build() {
                        window.paint_path(path, color);
                    }
                }
            },
        )
        .flex_none()
        .size(px(disc));
        let body = div().flex().justify_center().child(face);
        self.framed(label, caption, body, disc)
    }

    /// A picture file, scaled to fit and centered.
    fn image(&self, path: &str, caption: Option<&str>, width: f32, room: f32) -> Out {
        let body_h = self.body_room(false, caption.is_some(), room, width * 0.6);
        let body = div().flex().justify_center().h(px(body_h)).child(
            gpui::img(std::path::PathBuf::from(path))
                .object_fit(gpui::ObjectFit::Contain)
                .w(px(width))
                .h(px(body_h)),
        );
        self.framed(None, caption, body, body_h)
    }

    #[allow(clippy::too_many_arguments)]
    fn gauge(
        &self,
        label: &str,
        percent: f64,
        marker: Option<f64>,
        color: Hsla,
        caption: &Option<String>,
        key: &str,
        show_label: bool,
    ) -> Out {
        let track = self.track(key, percent, 8.0, color);
        let track = track.children(marker.map(|m| {
            let left = self.number(&format!("{key}/m"), m.clamp(0.0, 100.0));
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .w(px(2.0))
                .left(gpui::relative((left / 100.0) as f32))
                .bg(self.p.get("text").opacity(0.6))
        }));
        let head = div()
            .flex()
            .justify_between()
            .gap(px(8.0))
            .mb(px(4.0))
            .child(if show_label {
                self.line(0.8, self.p.get("text"), label.to_string())
            } else {
                div()
            })
            .child(self.line(0.8, self.p.get("text"), format!("{percent:.0}%")));
        let mut h = self.m.line(0.8) + 4.0 + 8.0;
        let mut el = div().flex().flex_col().min_w_0().child(head).child(track);
        if let Some(caption) = caption {
            el = el.child(self.line(0.75, self.muted(), caption.clone()).mt(px(3.0)));
            h += self.m.line(0.75) + 3.0;
        }
        Out::new(el, h)
    }

    // ---- charts ----

    fn series(&self, view: SeriesView, room: f32, key: &str) -> Option<Out> {
        let caption = self.m.line(0.8) + 4.0;
        let least = if self.m.compact { 40.0 } else { 48.0 };
        if room < least {
            return None;
        }
        let h = if room.is_finite() {
            room.min(caption + 160.0)
        } else if let Some(full) = self.full {
            (full * 0.7).max(caption + 240.0)
        } else {
            caption + 90.0
        };
        let mirrored = view.mirrored && view.series.len() >= 2;
        let arrows: [&str; 2] = if mirrored { ["↓", "↑"] } else { ["", ""] };
        let latest = view
            .series
            .iter()
            .take(2)
            .enumerate()
            .filter_map(|(i, s)| {
                let (_, last) = s.points.last()?;
                let text = format_value(*last, view.format, view.unit);
                Some(format!("{}{text}", arrows[i]))
            })
            .collect::<Vec<_>>()
            .join(" ");
        let head = div()
            .flex()
            .flex_none()
            .justify_between()
            .gap(px(8.0))
            .mb(px(4.0))
            .child(self.line(0.8, self.p.get("text"), view.label.to_string()))
            .child(
                self.line(0.8, self.p.get("text"), latest)
                    .font_weight(FontWeight::BOLD),
            );
        let chart = self.chart(&view, mirrored, key).h(px(h - caption)).w_full();
        Some(Out::new(
            div().flex().flex_col().h(px(h)).child(head).child(chart),
            h,
        ))
    }

    fn chart(&self, view: &SeriesView, mirrored: bool, key: &str) -> gpui::Canvas<()> {
        let data: Vec<Vec<(i64, f64)>> = view
            .series
            .iter()
            .map(|s| s.points.iter().map(|(t, v)| (*t, v.max(0.0))).collect())
            .collect();
        let flat = || data.iter().flatten();
        let enough = flat().count() >= 2;
        let t0 = flat().map(|p| p.0).min().unwrap_or(0);
        let t1 = flat().map(|p| p.0).max().unwrap_or(0);
        let y_max = view
            .y_max
            .unwrap_or_else(|| flat().map(|p| p.1).fold(1.0, f64::max));
        let bars = view.style == SeriesStyle::Bars && !mirrored;
        let first = data.first().cloned().unwrap_or_default();
        let shift = self.motion.slide(key, t0, t1, |before| {
            if bars && !first.is_empty() {
                first.iter().filter(|p| p.0 > before).count() as f64 / first.len() as f64
            } else {
                (t1 - before) as f64 / (t1 - t0).max(1) as f64
            }
        }) as f32;
        let area = view.style == SeriesStyle::Area || mirrored;
        let scale = view.color;
        let palette = self.p.clone();
        let ends = self.p.scale_ends(scale);
        let accent = self.p.get("accent");
        let axis = self.p.get("border");
        let tones: Vec<Option<Tone>> = view.series.iter().map(|s| s.tone).collect();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                if !enough {
                    return;
                }
                let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                let origin = bounds.origin;
                let at = |x: f32, y: f32| origin + point(px(x), px(y));
                if mirrored {
                    let mid = bounds.origin.y + px(h / 2.0);
                    let line = gpui::Bounds::new(point(origin.x, mid), size(px(w), px(1.0)));
                    window.paint_quad(fill(line, axis));
                }
                let mask = ContentMask { bounds };
                window.with_content_mask(Some(mask), |window| {
                    let dx = shift * w;
                    if bars {
                        let w_each = w / first.len().max(1) as f32;
                        for (i, (_, v)) in first.iter().enumerate() {
                            let t = (v.min(y_max) / y_max) as f32;
                            let color = match tones.first().copied().flatten() {
                                Some(tone) => palette.tone(tone),
                                None => palette.scale(scale, t as f64),
                            };
                            let bar = gpui::Bounds::new(
                                at(i as f32 * w_each + w_each * 0.1 + dx, h - t * h),
                                size(px(w_each * 0.8), px(t * h)),
                            );
                            window.paint_quad(fill(bar, color).corner_radii(px(1.0)));
                        }
                        return;
                    }
                    let x = |t: i64| (t - t0) as f32 / (t1 - t0).max(1) as f32 * w + dx;
                    let draw = |window: &mut Window,
                                i: usize,
                                pts: &[(i64, f64)],
                                base: f32,
                                height: f32,
                                up: bool| {
                        if pts.is_empty() {
                            return;
                        }
                        let sign = if up { -1.0 } else { 1.0 };
                        let y = |v: f64| base + sign * (v.min(y_max) / y_max) as f32 * height;
                        let tone = tones.get(i).copied().flatten();
                        let fill_color: Background = match (tone, ends) {
                            (Some(tone), _) => palette.tone(tone).into(),
                            (None, Some((low, high))) => linear_gradient(
                                0.0,
                                linear_color_stop(low, 0.0),
                                linear_color_stop(high, 1.0),
                            ),
                            (None, None) => accent.into(),
                        };
                        if area {
                            let mut shape = PathBuilder::fill();
                            shape.move_to(at(x(pts[0].0), base));
                            for (t, v) in pts {
                                shape.line_to(at(x(*t), y(*v)));
                            }
                            shape.line_to(at(x(pts[pts.len() - 1].0), base));
                            shape.close();
                            if let Ok(path) = shape.build() {
                                let share = if mirrored && i > 0 { 0.3 } else { 0.55 };
                                window.paint_path(path, fill_color.opacity(share));
                            }
                        }
                        let mut stroke = PathBuilder::stroke(px(1.5));
                        stroke.move_to(at(x(pts[0].0), y(pts[0].1)));
                        for (t, v) in &pts[1..] {
                            stroke.line_to(at(x(*t), y(*v)));
                        }
                        if let Ok(path) = stroke.build() {
                            if area {
                                window.paint_path(path, fill_color);
                            } else {
                                let color = tone.unwrap_or(Tone::series(i));
                                window.paint_path(path, palette.tone(color));
                            }
                        }
                    };
                    if mirrored {
                        draw(window, 0, &data[0], h / 2.0, h / 2.0, true);
                        draw(window, 1, &data[1], h / 2.0, h / 2.0, false);
                    } else {
                        for (i, pts) in data.iter().enumerate() {
                            draw(window, i, pts, h, h, true);
                        }
                    }
                });
            },
        )
    }

    fn meters(
        &self,
        label: Option<&str>,
        meters: &[Meter],
        scale: ColorScale,
        width: f32,
        room: f32,
        key: &str,
    ) -> Out {
        let gap_x = if self.m.compact { 10.0 } else { 12.0 };
        let gap_y = if self.m.compact { 2.0 } else { 4.0 };
        let columns = (((width + gap_x) / (130.0 + gap_x)).floor() as usize).max(1);
        let cell = (width - gap_x * (columns - 1) as f32) / columns as f32;
        let line = self.m.line(0.75);
        let head = label.map_or(0.0, |_| self.label_height());
        let fit_rows = ((room - head + gap_y) / (line + gap_y)).floor().max(0.0) as usize;
        let all_rows = meters.len().div_ceil(columns);
        let mut shown = meters.len();
        let mut h_more = 0.0;
        if all_rows > fit_rows {
            let more_room = room - head - self.more_height() + gap_y;
            let rows = (more_room / (line + gap_y)).floor().max(0.0) as usize;
            shown = (rows * columns).min(meters.len());
            h_more = self.more_height();
        }
        let rows = shown.div_ceil(columns);
        let body_h = if rows == 0 {
            0.0
        } else {
            rows as f32 * (line + gap_y) - gap_y
        };
        let em = self.m.font(0.75);
        let grid = div()
            .flex()
            .flex_wrap()
            .gap_x(px(gap_x))
            .gap_y(px(gap_y))
            .children(meters.iter().take(shown).map(|m| {
                let color = match m.tone {
                    Some(tone) => self.p.tone(tone),
                    None => self.p.scale(scale, m.percent / 100.0),
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .w(px(cell))
                    .h(px(line))
                    .child(
                        self.line(0.75, self.muted(), m.label.clone())
                            .w(px(em * 2.2))
                            .text_right(),
                    )
                    .child(
                        self.track(&format!("{key}/{}", m.label), m.percent, 6.0, color)
                            .flex_1(),
                    )
                    .child(
                        self.line(0.75, self.p.get("text"), format!("{:.0}", m.percent))
                            .w(px(em * 2.2))
                            .text_right(),
                    )
            }));
        let el = div()
            .flex()
            .flex_col()
            .children(label.map(|l| self.label(l)))
            .child(grid)
            .children((shown < meters.len()).then(|| self.more(meters.len() - shown)));
        let mut out = Out::new(el, head + body_h + h_more);
        out.none_shown = shown == 0 && !meters.is_empty();
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn bars(
        &self,
        label: Option<&str>,
        unit: Option<&str>,
        format: ValueFormat,
        max: Option<f64>,
        bars: &[Bar],
        room: f32,
        key: &str,
        nav: Option<usize>,
    ) -> Out {
        let head = label.map_or(0.0, |_| self.label_height());
        let pad = if self.m.compact { 1.0 } else { 2.0 };
        let row_h = self.m.line(0.8) + pad * 2.0;
        let shown = fit_items(&vec![row_h; bars.len()], room - head, self.more_height());
        let top = max
            .unwrap_or_else(|| bars.iter().map(|b| b.value).fold(f64::EPSILON, f64::max))
            .max(f64::EPSILON);
        let em = self.m.font(0.8);
        let rows = bars.iter().take(shown).enumerate().map(|(i, b)| {
            let color = match (b.status, b.tone) {
                (Some(Status::Warn), _) => self.p.get("warn"),
                (Some(Status::Error), _) => self.p.get("error"),
                (_, Some(tone)) => self.p.tone(tone),
                _ => self.p.get("accent"),
            };
            let selected = nav.is_some_and(|n| self.selected == Some(n + i));
            let row = div()
                .relative()
                .flex()
                .items_center()
                .gap(px(8.0))
                .h(px(row_h))
                .rounded(px(3.0))
                .child(
                    self.line(0.8, self.p.get("text"), b.label.clone())
                        .max_w(px(em * 11.0))
                        .w(px(em * 11.0)),
                )
                .child(
                    self.track(
                        &format!("{key}/{}", b.label),
                        (b.value / top * 100.0).min(100.0),
                        6.0,
                        color,
                    )
                    .flex_1(),
                )
                .child(
                    self.line(0.8, self.p.get("text"), format_value(b.value, format, unit))
                        .w(px(em * 6.0))
                        .text_right(),
                );
            self.select(row, selected)
        });
        let more = (shown < bars.len()).then(|| self.more(bars.len() - shown));
        let h = head
            + shown as f32 * row_h
            + if shown < bars.len() {
                self.more_height()
            } else {
                0.0
            };
        let el = div()
            .flex()
            .flex_col()
            .children(label.map(|l| self.label(l)))
            .children(rows)
            .children(more);
        let mut out = Out::new(el, h);
        out.none_shown = shown == 0 && !bars.is_empty();
        out
    }

    /// Slices clockwise from twelve o'clock, as in `web/src/pie.ts`. Beside
    /// the legend; in a narrow spot the legend goes below and drops values, and
    /// in the narrowest (or a table cell) only the disc shows.
    fn pie(&self, view: PieView, width: f32, room: f32, _key: &str) -> Out {
        let clean = |v: f64| if v.is_finite() { v.max(0.0) } else { 0.0 };
        let total: f64 = view.slices.iter().map(|s| clean(s.value)).sum();
        let head = view.label.map_or(0.0, |_| self.label_height());
        let in_cell = !room.is_finite() && self.full.is_none() && width < 110.0;
        if total <= 0.0 {
            let el = div()
                .flex()
                .flex_col()
                .children(view.label.map(|l| self.label(l)))
                .child(self.line(0.8, self.muted(), "No data"));
            return Out::new(el, head + self.m.line(0.8));
        }
        let colors: Vec<Hsla> = view
            .slices
            .iter()
            .enumerate()
            .map(|(i, s)| match (s.status, s.tone) {
                (Some(status), _) => self.p.status(status),
                (_, Some(tone)) => self.p.tone(tone),
                _ if view.color == ColorScale::Accent => self.p.tone(Tone::series(i)),
                _ => {
                    let t = if view.slices.len() > 1 {
                        i as f64 / (view.slices.len() - 1) as f64
                    } else {
                        0.5
                    };
                    self.p.scale(view.color, t)
                }
            })
            .collect();
        let shares: Vec<f32> = view
            .slices
            .iter()
            .map(|s| (clean(s.value) / total) as f32)
            .collect();
        let narrow = width <= 260.0;
        let tiny = width <= 110.0;
        let disc_size = if in_cell {
            self.m.font(1.6)
        } else if narrow {
            (width * 0.5).clamp(56.0, 120.0)
        } else {
            (width * 0.38).clamp(56.0, 160.0)
        };
        let disc_size = if room.is_finite() {
            disc_size.min((room - head).max(32.0))
        } else {
            disc_size
        };
        let disc = self.pie_disc(shares.clone(), colors.clone(), view.donut, disc_size);
        if in_cell {
            return Out::new(disc, disc_size);
        }
        let line = self.m.line(0.8) + 2.0;
        let legend_room = if narrow {
            room - head - disc_size - 12.0
        } else {
            room - head
        };
        let shown = fit_items(
            &vec![line; view.slices.len()],
            legend_room,
            self.more_height(),
        );
        let em = self.m.font(0.8);
        let more = (shown < view.slices.len()).then(|| self.more(view.slices.len() - shown));
        let legend_h = shown as f32 * line
            + if more.is_some() {
                self.more_height()
            } else {
                0.0
            };
        let rows = view.slices.iter().take(shown).enumerate().map(|(i, s)| {
            let percent = shares[i] * 100.0;
            let text = if percent > 0.0 && percent < 1.0 {
                "<1%".to_string()
            } else {
                format!("{percent:.0}%")
            };
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .h(px(line))
                .child(
                    div()
                        .flex_none()
                        .size(px(10.0))
                        .rounded(px(2.0))
                        .bg(colors[i]),
                )
                .child(self.line(0.8, self.p.get("text"), s.label.clone()).flex_1())
                .children((!narrow).then(|| {
                    self.line(
                        0.8,
                        self.muted(),
                        format_value(s.value, view.format, view.unit),
                    )
                }))
                .child(
                    self.line(0.8, self.p.get("text"), text)
                        .w(px(em * 3.2))
                        .text_right(),
                )
        });
        let legend = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .children(rows)
            .children(more);
        let body = if narrow {
            div()
                .flex()
                .flex_col()
                .items_stretch()
                .gap(px(12.0))
                .child(div().flex().justify_center().child(disc))
        } else {
            div().flex().items_center().gap(px(12.0)).child(disc)
        };
        let body = if tiny { body } else { body.child(legend) };
        let body_h = match (tiny, narrow) {
            (true, _) => disc_size,
            (false, true) => disc_size + 12.0 + legend_h,
            (false, false) => disc_size.max(legend_h),
        };
        let el = div()
            .flex()
            .flex_col()
            .min_w_0()
            .children(view.label.map(|l| self.label(l)))
            .child(body);
        Out::new(el, head + body_h)
    }

    fn pie_disc(
        &self,
        shares: Vec<f32>,
        colors: Vec<Hsla>,
        donut: bool,
        disc: f32,
    ) -> impl IntoElement {
        let edge = self.p.get("surface");
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let r = disc / 2.0;
                let inner = if donut { r * 0.55 } else { 0.0 };
                let c = bounds.origin + point(px(r), px(r));
                let at = |turn: f32, radius: f32| {
                    let a = turn * TAU;
                    c + point(px(radius * a.sin()), px(-radius * a.cos()))
                };
                let mut from = 0.0;
                for (share, color) in shares.iter().zip(&colors) {
                    let to = from + share;
                    if *share > 0.0 {
                        // Two pieces for a whole turn: one outline can't end where it starts.
                        let pieces = if *share > 0.999 { 2 } else { 1 };
                        for piece in 0..pieces {
                            let a = from + share * piece as f32 / pieces as f32;
                            let b = from + share * (piece + 1) as f32 / pieces as f32;
                            let steps = ((b - a) * 120.0).ceil().max(1.0) as usize;
                            let mut outline: Vec<gpui::Point<gpui::Pixels>> =
                                Vec::with_capacity(steps * 2 + 2);
                            for s in 0..=steps {
                                outline.push(at(a + (b - a) * s as f32 / steps as f32, r));
                            }
                            if inner > 0.0 {
                                for s in (0..=steps).rev() {
                                    outline.push(at(a + (b - a) * s as f32 / steps as f32, inner));
                                }
                            } else {
                                outline.push(c);
                            }
                            let mut shape = PathBuilder::fill();
                            shape.add_polygon(&outline, true);
                            if let Ok(path) = shape.build() {
                                window.paint_path(path, *color);
                            }
                            if pieces == 1 {
                                let mut line = PathBuilder::stroke(px(disc * 0.006 + 0.5));
                                line.add_polygon(&outline, true);
                                if let Ok(path) = line.build() {
                                    window.paint_path(path, edge);
                                }
                            }
                        }
                    }
                    from = to;
                }
            },
        )
        .flex_none()
        .size(px(disc))
    }

    // ---- lists ----

    fn list(
        &self,
        items: &[ListItem],
        empty: Option<&str>,
        width: f32,
        room: f32,
        key: &str,
        nav: Option<usize>,
    ) -> Out {
        if items.is_empty() {
            let pad = if self.m.compact { 3.0 } else { 6.0 };
            let el = self
                .line(
                    1.0,
                    self.muted(),
                    empty.unwrap_or("Nothing here").to_string(),
                )
                .my(px(pad));
            return Out::new(el, self.m.line(1.0) + pad * 2.0);
        }
        let pad = if self.m.compact { 3.0 } else { 6.0 };
        let heights: Vec<f32> = items
            .iter()
            .map(|i| {
                pad * 2.0
                    + 1.0
                    + self.m.line(1.0)
                    + i.subtitle.as_ref().map_or(0.0, |_| self.m.line(0.8))
            })
            .collect();
        let shown = fit_items(&heights, room, self.more_height());
        let show_tags = width > 300.0;
        let rows = items.iter().take(shown).enumerate().map(|(i, item)| {
            let selected = nav.is_some_and(|n| self.selected == Some(n + i));
            let stripe = match (item.status, item.tone) {
                (Some(status), _) => Some(self.p.status(status)),
                (_, Some(tone)) => Some(self.p.tone(tone)),
                _ => None,
            };
            let title_color = match item.status {
                Some(Status::Warn) => self.p.get("warn"),
                Some(Status::Error) => self.p.get("error"),
                _ => match &item.action {
                    Some(Action::OpenUrl { url }) if is_web_url(url) => self.p.get("accent"),
                    _ => self.p.get("text"),
                },
            };
            let head = div()
                .flex()
                .items_baseline()
                .min_w_0()
                .h(px(self.m.line(1.0)))
                .child(
                    self.line(1.0, title_color, item.title.clone())
                        .flex_shrink(1.0),
                )
                .children(item.tags.iter().filter(|_| show_tags).map(|tag| {
                    self.line(0.7, self.muted(), tag.clone())
                        .ml(px(6.0))
                        .px(px(4.0))
                        .rounded(px(4.0))
                        .border_1()
                        .border_color(self.p.get("border"))
                }));
            let row = div()
                .relative()
                .flex()
                .flex_col()
                .min_w_0()
                .h(px(heights[i]))
                .py(px(pad))
                .border_b_1()
                .border_color(self.p.get("border"))
                .when_some(stripe, |row, color| {
                    row.pl(px(8.0)).child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(px(3.0))
                            .bg(color),
                    )
                })
                .child(head)
                .children(
                    item.subtitle
                        .clone()
                        .map(|s| self.line(0.8, self.muted(), s)),
                );
            self.clickable(
                self.select(row, selected),
                format!("{key}/{i}"),
                item.action.clone(),
            )
        });
        let more = shown < items.len();
        let h: f32 =
            heights[..shown].iter().sum::<f32>() + if more { self.more_height() } else { 0.0 };
        let el = div()
            .flex()
            .flex_col()
            .min_w_0()
            .children(rows)
            .children(more.then(|| self.more(items.len() - shown)));
        let mut out = Out::new(el, h);
        out.none_shown = shown == 0;
        out
    }

    /// A row that highlights under the pointer and opens its link when clicked.
    fn clickable(&self, row: Div, id: String, action: Option<Action>) -> AnyElement {
        if !self.interactive {
            return row.into_any_element();
        }
        let hover = self.p.get("surface-2");
        let links = matches!(&action, Some(Action::OpenUrl { url }) if is_web_url(url));
        row.id(SharedString::from(id))
            .hover(move |s| s.bg(hover))
            .when(links, |row| row.cursor_pointer())
            .when_some(action.filter(|_| links), |row, action| {
                row.on_click(move |_, _, _| run_action(&action))
            })
            .into_any_element()
    }

    fn table(
        &self,
        columns: &[Column],
        rows: &[Row],
        width: f32,
        room: f32,
        key: &str,
        nav: Option<usize>,
    ) -> Out {
        let pad_x = if self.m.compact { 8.0 } else { 12.0 };
        let pad_y = if self.m.compact { 3.0 } else { 6.0 };
        let cap = self.m.font(0.85) * 16.0;
        let natural: Vec<f32> = columns
            .iter()
            .map(|c| {
                let head = self.m.text_width(&c.label, 0.85);
                let body = rows
                    .iter()
                    .filter_map(|r| r.cells.get(&c.key))
                    .map(|cell| self.cell_width(cell))
                    .fold(head, f32::max);
                (body + pad_x).min(cap + pad_x)
            })
            .collect();
        let em = self.m.font(0.85);
        let least: Vec<f32> = natural
            .iter()
            .enumerate()
            .map(|(i, w)| w.min(if i == 0 { 7.0 } else { 3.5 } * em + pad_x))
            .collect();
        let priorities: Vec<i32> = columns.iter().map(|c| c.priority.unwrap_or(0)).collect();
        let kept = table_columns(&natural, &least, &priorities, width);
        // A table fills its width: spare room goes to the columns by their size.
        let used: f32 = kept.iter().map(|(_, w)| w).sum();
        let stretch = if used > 0.0 && used < width {
            width / used
        } else {
            1.0
        };
        let widths: Vec<(usize, f32)> = kept.iter().map(|(i, w)| (*i, w * stretch)).collect();

        let head_h = self.m.line(0.85) + pad_y * 2.0;
        // A widget in a cell is built once, to know the row's height.
        let mut built: Vec<(Vec<Option<Out>>, f32)> = Vec::with_capacity(rows.len());
        for (r, row) in rows.iter().enumerate() {
            let mut h = self.m.line(0.85);
            let mut cells = Vec::with_capacity(widths.len());
            for (c, w) in &widths {
                let column = &columns[*c];
                let out = match row.cells.get(&column.key) {
                    Some(Cell::Widget(widget)) => self.widget(
                        widget,
                        w - pad_x,
                        f32::INFINITY,
                        &format!("{key}/{r}/{}", column.key),
                        None,
                    ),
                    _ => None,
                };
                if let Some(out) = &out {
                    h = h.max(out.h);
                }
                cells.push(out);
            }
            built.push((cells, h + pad_y * 2.0 + 1.0));
        }
        let heights: Vec<f32> = built.iter().map(|(_, h)| *h).collect();
        let shown = fit_items(&heights, room - head_h, self.more_height());
        let header = div()
            .flex()
            .h(px(head_h))
            .items_center()
            .children(widths.iter().map(|(c, w)| {
                self.line(0.85, self.muted(), columns[*c].label.clone())
                    .w(px(*w))
                    .px(px(pad_x / 2.0))
                    .font_weight(FontWeight::MEDIUM)
            }));
        let total = rows.len();
        let body =
            rows.iter()
                .zip(built)
                .take(shown)
                .enumerate()
                .map(|(r, (row, (cells, h)))| {
                    let first_color = match row.status {
                        Some(Status::Warn) => self.p.get("warn"),
                        Some(Status::Error) => self.p.get("error"),
                        _ => self.p.get("text"),
                    };
                    let selected = nav.is_some_and(|n| self.selected == Some(n + r));
                    let line = div()
                        .relative()
                        .flex()
                        .items_center()
                        .h(px(h))
                        .border_t_1()
                        .border_color(self.p.get("border"))
                        .children(widths.iter().zip(cells).enumerate().map(
                            |(i, ((c, w), out))| {
                                let color = if i == 0 {
                                    first_color
                                } else {
                                    self.p.get("text")
                                };
                                let text = match row.cells.get(&columns[*c].key) {
                                    Some(Cell::Text(t)) => t.clone(),
                                    Some(Cell::Number(n)) => n.to_string(),
                                    _ => String::new(),
                                };
                                let cell =
                                    div().flex_none().w(px(*w)).px(px(pad_x / 2.0)).min_w_0();
                                match out {
                                    Some(out) => cell.child(out.el),
                                    None => cell.child(self.line(0.85, color, text)),
                                }
                            },
                        ));
                    self.clickable(
                        self.select(line, selected),
                        format!("{key}/row/{r}"),
                        row.action.clone(),
                    )
                });
        let more = shown < total;
        let h = head_h
            + heights[..shown].iter().sum::<f32>()
            + if more { self.more_height() } else { 0.0 };
        let el = div()
            .flex()
            .flex_col()
            .child(header)
            .children(body)
            .children(more.then(|| self.more(total - shown)));
        let mut out = Out::new(el, h);
        out.none_shown = shown == 0 && total > 0;
        out
    }

    fn cell_width(&self, cell: &Cell) -> f32 {
        match cell {
            Cell::Text(t) => self.m.text_width(t, 0.85),
            Cell::Number(n) => self.m.text_width(&n.to_string(), 0.85),
            Cell::Widget(Widget::Gauge { .. }) => 90.0,
            Cell::Widget(Widget::Stat { value, .. }) => self.m.text_width(value, 1.5),
            Cell::Widget(Widget::Pie { .. }) => self.m.font(1.6),
            Cell::Widget(_) => 90.0,
        }
    }

    fn log(&self, label: Option<&str>, lines: &[LogLine], room: f32, key: &str) -> Option<Out> {
        let head = label.map_or(0.0, |_| self.label_height());
        let least = head + self.m.font(0.78) * 3.0;
        if room < least {
            return None;
        }
        let h = if room.is_finite() {
            if self.full.is_some() {
                room
            } else {
                room.min(head + 420.0)
            }
        } else if let Some(full) = self.full {
            (full * 0.7).max(160.0)
        } else {
            160.0
        };
        let mut logs = self.logs.borrow_mut();
        let state = logs.entry(key.to_string()).or_default();
        let handle = state.handle.clone();
        // Follow new lines unless the reader has scrolled up from the bottom.
        let scrolled = -f32::from(handle.offset().y);
        let near = !state.seen || state.last_max - scrolled < 24.0;
        state.seen = true;
        state.following = near;
        state.last_max = f32::from(handle.max_offset().y);
        if near {
            handle.scroll_to_bottom();
        }
        let following = state.following;
        drop(logs);
        let text_line = self.m.font(0.78) * 1.45;
        let rows = lines.iter().enumerate().map(|(i, line)| {
            let color = match line.level {
                Some(LogLevel::Error) => self.p.get("error"),
                Some(LogLevel::Warn) => self.p.get("warn"),
                Some(LogLevel::Debug) => self.muted(),
                _ => self.p.get("text"),
            };
            let level = line.level.map(|l| format!("{l:?}").to_ascii_uppercase());
            let level_color = if matches!(line.level, Some(LogLevel::Error | LogLevel::Warn)) {
                color
            } else {
                self.muted()
            };
            div()
                .id(i)
                .flex()
                .items_start()
                .font_family(MONO_FONT)
                .text_size(px(self.m.font(0.78)))
                .line_height(px(text_line))
                .text_color(color)
                .children(line.time.clone().map(|t| {
                    div()
                        .flex_none()
                        .mr(px(8.0))
                        .text_color(self.muted())
                        .child(t)
                }))
                .children(level.map(|l| {
                    div()
                        .flex_none()
                        .w(px(self.m.font(0.7) * 3.6))
                        .mr(px(6.0))
                        .text_size(px(self.m.font(0.7)))
                        .text_color(level_color)
                        .child(l)
                }))
                .child(div().flex_1().min_w_0().child(line.text.clone()))
        });
        let box_ = div()
            .id(SharedString::from(format!("{key}/log")))
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&handle)
            .bg(self.p.get("bg"))
            .border_1()
            .border_color(self.p.get("border"))
            .rounded(px(6.0))
            .px(px(8.0))
            .py(px(6.0))
            .children(rows);
        let button = (!following && self.interactive).then(|| {
            let handle = handle.clone();
            div()
                .id(SharedString::from(format!("{key}/follow")))
                .absolute()
                .right(px(14.0))
                .bottom(px(10.0))
                .px(px(8.0))
                .py(px(2.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(self.p.get("border"))
                .bg(self.p.get("surface"))
                .text_size(px(self.m.font(0.75)))
                .text_color(self.p.get("text"))
                .cursor_pointer()
                .child("↓ Follow")
                .on_click(move |_, window, _| {
                    handle.scroll_to_bottom();
                    window.refresh();
                })
        });
        let el = div()
            .relative()
            .flex()
            .flex_col()
            .h(px(h))
            .children(label.map(|l| self.label(l)))
            .child(box_)
            .children(button);
        Some(Out::new(el, h))
    }

    fn uptime(
        &self,
        checks: &[UptimeCheck],
        width: f32,
        room: f32,
        key: &str,
        nav: Option<usize>,
    ) -> Out {
        let gap = if self.m.compact { 3.0 } else { 6.0 };
        let row_h = self.m.line(0.8).max(16.0);
        let heights = vec![row_h + gap; checks.len()];
        let shown = fit_items(&heights, room + gap, self.more_height());
        let em = self.m.font(0.8);
        let name_width = checks
            .iter()
            .map(|c| self.m.text_width(&c.name, 0.8))
            .fold(em * 4.0, f32::max)
            .min(width * 0.4);
        let rows = checks.iter().take(shown).enumerate().map(|(i, c)| {
            let last = if c.status != Status::Ok && c.detail.is_some() {
                c.detail.clone().unwrap_or_default()
            } else {
                match c.latency_ms {
                    Some(ms) if ms < 1000.0 => format!("{ms:.0}ms"),
                    Some(ms) => format!("{:.1}s", ms / 1000.0),
                    None => String::new(),
                }
            };
            let percent = match c.uptime {
                Some(u) if u >= 100.0 => "100%".to_string(),
                Some(u) => format!("{u:.1}%"),
                None => String::new(),
            };
            let last_color = match c.status {
                Status::Warn => self.p.get("warn"),
                Status::Error => self.p.get("error"),
                _ => self.muted(),
            };
            let name_color = if c.url.as_deref().is_some_and(is_web_url) {
                self.p.get("accent")
            } else {
                self.p.get("text")
            };
            let selected = nav.is_some_and(|n| self.selected == Some(n + i));
            let row = div()
                .relative()
                .flex()
                .items_center()
                .gap(px(8.0))
                .h(px(row_h))
                .mb(px(gap))
                .rounded(px(3.0))
                .child(
                    div()
                        .flex_none()
                        .size(px(8.0))
                        .rounded_full()
                        .bg(self.p.status(c.status)),
                )
                .child(self.line(0.8, name_color, c.name.clone()).w(px(name_width)))
                .child(self.strip(&c.history).flex_1().h(px(16.0)))
                .child(
                    self.line(0.8, self.p.get("text"), percent)
                        .w(px(em * 4.2))
                        .text_right(),
                )
                .child(
                    self.line(0.8, last_color, last)
                        .w(px((em * 6.0).min(width * 0.2))),
                );
            let action = c.url.clone().map(|url| Action::OpenUrl { url });
            self.clickable(self.select(row, selected), format!("{key}/{i}"), action)
        });
        let more = shown < checks.len();
        let h = shown as f32 * (row_h + gap) + if more { self.more_height() } else { 0.0 };
        let el = div()
            .flex()
            .flex_col()
            .children(rows)
            .children(more.then(|| self.more(checks.len() - shown)));
        let mut out = Out::new(el, h);
        out.none_shown = shown == 0 && !checks.is_empty();
        out
    }

    /// Recent results, newest on the right, in as many cells as fit.
    fn strip(&self, history: &[Status]) -> gpui::Canvas<()> {
        let start = history.len().saturating_sub(UPTIME_SHOWN);
        let colors: Vec<Hsla> = history[start..]
            .iter()
            .map(|s| match s {
                Status::Unknown => self.p.get("track"),
                other => self.p.status(*other),
            })
            .collect();
        let track = self.p.get("track");
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                const CELL: f32 = 4.0;
                const STEP: f32 = 6.0;
                let width = f32::from(bounds.size.width);
                let fit = ((width + 2.0) / STEP).floor() as usize;
                let right = bounds.origin.x + bounds.size.width;
                for i in 0..fit.min(UPTIME_SHOWN) {
                    let color = colors
                        .len()
                        .checked_sub(i + 1)
                        .map_or(track, |at| colors[at]);
                    let x = right - px(CELL + i as f32 * STEP);
                    let cell = gpui::Bounds::new(
                        point(x, bounds.origin.y),
                        size(px(CELL), bounds.size.height),
                    );
                    window.paint_quad(fill(cell, color).corner_radii(px(1.0)));
                }
            },
        )
    }

    // ---- containers ----

    /// Widgets side by side, wrapping when there isn't room: `flex: 1 1 140px`,
    /// or 90px for a stat.
    fn row(
        &self,
        children: &[Widget],
        width: f32,
        room: f32,
        key: &str,
        nav: Option<usize>,
    ) -> Out {
        let gap_x = if self.m.compact { 10.0 } else { 16.0 };
        let gap_y = self.m.column_gap;
        let basis = |w: &Widget| {
            if matches!(w, Widget::Stat { .. }) {
                90.0
            } else {
                140.0
            }
        };
        let mut lines: Vec<Vec<usize>> = vec![];
        let mut used = 0.0;
        for (i, child) in children.iter().enumerate() {
            let need = basis(child);
            match lines.last_mut() {
                Some(line) if used + gap_x + need <= width => {
                    line.push(i);
                    used += gap_x + need;
                }
                _ => {
                    lines.push(vec![i]);
                    used = need;
                }
            }
        }
        let mut offsets = Vec::with_capacity(children.len());
        let mut offset = nav;
        for child in children {
            offsets.push(offset);
            offset = offset.map(|n| n + nav_count(child));
        }
        let mut y = 0.0;
        let mut built = vec![];
        'lines: for line in lines {
            let bases: f32 = line.iter().map(|i| basis(&children[*i])).sum();
            let free = width - bases - gap_x * (line.len() - 1) as f32;
            let extra = free / line.len() as f32;
            let top = if built.is_empty() { 0.0 } else { gap_y };
            let line_room = room - y - top;
            let mut h: f32 = 0.0;
            let mut els = vec![];
            for i in &line {
                let w = (basis(&children[*i]) + extra).min(width);
                let child = &children[*i];
                let Some(out) = self
                    .widget(child, w, line_room, &format!("{key}/{i}"), offsets[*i])
                    .filter(|o| !o.none_shown && (o.h <= line_room + 0.5 || !room.is_finite()))
                else {
                    if els.is_empty() {
                        break 'lines;
                    }
                    break;
                };
                h = h.max(out.h);
                els.push(div().flex_none().w(px(w)).min_w_0().child(out.el));
            }
            y += top + h;
            built.push(
                div()
                    .flex()
                    .gap(px(gap_x))
                    .when(top > 0.0, |d| d.mt(px(top)))
                    .children(els),
            );
        }
        Out::new(div().flex().flex_col().min_w_0().children(built), y)
    }

    /// Widgets stacked. Charts and logs share the height that's left. A child
    /// that doesn't fit is left out with all after it, and a one-line text
    /// just before it (a heading) goes first, to make room.
    fn column(
        &self,
        children: &[Widget],
        width: f32,
        room: f32,
        key: &str,
        nav: Option<usize>,
    ) -> Out {
        let gap = self.m.column_gap;
        let mut offsets = Vec::with_capacity(children.len());
        let mut offset = nav;
        for child in children {
            offsets.push(offset);
            offset = offset.map(|n| n + nav_count(child));
        }
        let child_key = |i: usize| format!("{key}/{i}");
        let flexible = |w: &Widget| matches!(w, Widget::Series { .. } | Widget::Log { .. });
        let mut natural: Vec<Option<Out>> = children
            .iter()
            .enumerate()
            .map(|(i, c)| self.widget(c, width, f32::INFINITY, &child_key(i), offsets[i]))
            .collect();
        let total: f32 = natural.iter().flatten().map(|o| o.h).sum::<f32>()
            + gap * children.len().saturating_sub(1) as f32;
        let mut shown: Vec<Out> = vec![];
        let mut y = 0.0;
        if total <= room + 0.5 {
            let spare = if room.is_finite() { room - total } else { 0.0 };
            let flex_count = children.iter().filter(|c| flexible(c)).count();
            for (i, child) in children.iter().enumerate() {
                let out = natural[i].take();
                let out = if flexible(child) && spare > 0.0 {
                    let grown = out.as_ref().map_or(0.0, |o| o.h) + spare / flex_count as f32;
                    self.widget(child, width, grown, &child_key(i), offsets[i])
                } else {
                    out
                };
                if let Some(out) = out {
                    y += out.h + if shown.is_empty() { 0.0 } else { gap };
                    shown.push(out);
                }
            }
        } else {
            for (i, child) in children.iter().enumerate() {
                let top = if shown.is_empty() { 0.0 } else { gap };
                let left = room - y - top;
                let fits = |o: &Out, left: f32| !o.none_shown && o.h <= left + 0.5;
                let attempt = (left > 0.0)
                    .then(|| self.widget(child, width, left, &child_key(i), offsets[i]))
                    .flatten()
                    .filter(|o| fits(o, left));
                if let Some(out) = attempt {
                    y += top + out.h;
                    shown.push(out);
                    continue;
                }
                if let Some(heading) = shown.pop_if(|o| o.one_line) {
                    y -= heading.h + if shown.is_empty() { 0.0 } else { gap };
                    let top = if shown.is_empty() { 0.0 } else { gap };
                    let left = room - y - top;
                    if let Some(out) = self
                        .widget(child, width, left, &child_key(i), offsets[i])
                        .filter(|o| fits(o, left))
                    {
                        y += top + out.h;
                        shown.push(out);
                    }
                }
                break;
            }
        }
        let el = div()
            .flex()
            .flex_col()
            .min_w_0()
            .gap(px(gap))
            .children(shown.into_iter().map(|o| o.el));
        Out::new(el, y)
    }

    // ---- the tile ----

    /// The tile's face in its box: header and fitted view. `height` is `None`
    /// for fullscreen, where the view takes the height it needs.
    pub fn tile(
        &self,
        id: &str,
        update: Option<&TileUpdate>,
        width: f32,
        height: Option<f32>,
        small: bool,
    ) -> Div {
        let (px_, py) = match (self.m.compact, small) {
            (true, _) => (8.0, 6.0),
            (false, true) => (12.0, 8.0),
            (false, false) => (14.0, 12.0),
        };
        let header_gap = match (self.m.compact, small) {
            (true, _) | (_, true) => 4.0,
            _ => 10.0,
        };
        let radius = if self.m.compact { 4.0 } else { 8.0 };
        let stripe_w = if self.m.compact { 3.0 } else { 4.0 };
        let status = update.map_or(Status::Unknown, |u| u.view.status);
        let header_h = self.m.line(0.95);
        let header = div()
            .flex()
            .flex_none()
            .items_baseline()
            .gap(px(8.0))
            .min_w_0()
            .h(px(header_h))
            .mb(px(header_gap))
            .child(
                self.text(0.95, self.p.get("text"))
                    .flex_none()
                    .whitespace_nowrap()
                    .font_weight(FontWeight::BOLD)
                    .child(id.to_string()),
            )
            .children(update.filter(|u| u.stale).map(|_| {
                self.text(0.75, self.p.get("warn"))
                    .flex_none()
                    .child("stale")
            }))
            .children(update.filter(|u| u.demo).map(|_| {
                self.text(0.75, self.p.get("accent"))
                    .flex_none()
                    .child("demo")
            }))
            .child(
                self.line(
                    0.8,
                    self.muted(),
                    update.map_or("waiting…".to_string(), |u| u.view.summary.clone()),
                )
                .flex_shrink(1.0),
            );
        let inner_width = width - stripe_w - 1.0 - px_ * 2.0;
        let room = height.map_or(f32::INFINITY, |h| {
            h - 2.0 - py * 2.0 - header_h - header_gap
        });
        let view = update.and_then(|u| self.widget(&u.view.root, inner_width, room, id, Some(0)));
        div()
            .relative()
            .flex()
            .flex_col()
            .min_w_0()
            .overflow_hidden()
            .rounded(px(radius))
            .border_1()
            .border_color(self.p.get("border"))
            .bg(self.p.get("surface"))
            .pl(px(px_ + stripe_w - 1.0))
            .pr(px(px_))
            .py(px(py))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(px(stripe_w))
                    .bg(self.p.status(status)),
            )
            .child(header)
            .children(view.map(|v| v.el))
    }
}

struct SeriesView<'a> {
    label: &'a str,
    unit: Option<&'a str>,
    y_max: Option<f64>,
    style: SeriesStyle,
    color: ColorScale,
    mirrored: bool,
    format: ValueFormat,
    series: &'a [SeriesData],
}

struct PieView<'a> {
    label: Option<&'a str>,
    slices: &'a [Slice],
    donut: bool,
    format: ValueFormat,
    unit: Option<&'a str>,
    color: ColorScale,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Inline {
    Plain,
    Strong,
    Code,
}

fn inline_markdown(text: &str) -> Vec<(&str, Inline)> {
    let mut out = vec![];
    let mut rest = text;
    while !rest.is_empty() {
        let bold = rest.find("**").and_then(|at| {
            let inner = &rest[at + 2..];
            let end = inner.find('*')?;
            (end > 0 && inner[end..].starts_with("**")).then_some((
                at,
                at + 2 + end,
                2,
                Inline::Strong,
            ))
        });
        let code = rest.find('`').and_then(|at| {
            let end = rest[at + 1..].find('`')?;
            (end > 0).then_some((at, at + 1 + end, 1, Inline::Code))
        });
        let next = match (bold, code) {
            (Some(b), Some(c)) => Some(if b.0 < c.0 { b } else { c }),
            (b, c) => b.or(c),
        };
        let Some((start, end, marks, kind)) = next else {
            out.push((rest, Inline::Plain));
            break;
        };
        if start > 0 {
            out.push((&rest[..start], Inline::Plain));
        }
        out.push((&rest[start + marks..end], kind));
        rest = &rest[end + marks..];
    }
    out
}

/// The columns a table keeps in `room` pixels, with their widths, like
/// `tableColumns` in `web/src/fit.ts`: while the least widths don't fit, drop a
/// column, lowest priority first (of equal ones the rightmost, never the
/// first); then the widest columns give up width until the row fits.
pub fn table_columns(
    natural: &[f32],
    least: &[f32],
    priorities: &[i32],
    room: f32,
) -> Vec<(usize, f32)> {
    let mut order: Vec<usize> = (1..natural.len()).collect();
    order.sort_by_key(|i| (priorities[*i], std::cmp::Reverse(*i)));
    let mut order = order.into_iter();
    let mut keep: Vec<usize> = (0..natural.len()).collect();
    while keep.len() > 1 && keep.iter().map(|i| least[*i]).sum::<f32>() > room {
        let Some(drop) = order.next() else {
            break;
        };
        keep.retain(|i| *i != drop);
    }
    let mut widths: Vec<f32> = keep.iter().map(|i| natural[*i]).collect();
    let mut over = widths.iter().sum::<f32>() - room;
    while over > 0.5 {
        let widest = (0..widths.len())
            .filter(|j| widths[*j] > least[keep[*j]] + 0.5)
            .max_by(|a, b| widths[*a].total_cmp(&widths[*b]));
        let Some(k) = widest else {
            break;
        };
        let give = over.min(1.0).min(widths[k] - least[keep[k]]);
        widths[k] -= give;
        over -= give;
    }
    if over > 0.5 && !widths.is_empty() {
        widths[0] = (widths[0] - over).max(0.0);
    }
    keep.into_iter().zip(widths).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_whole_items_and_room_for_more() {
        assert_eq!(fit_items(&[10.0, 10.0], 20.0, 5.0), 2);
        assert_eq!(fit_items(&[10.0, 10.0, 10.0], 25.0, 5.0), 2);
        assert_eq!(fit_items(&[10.0, 10.0, 10.0], 20.0, 5.0), 1);
        assert_eq!(fit_items(&[10.0], 4.0, 5.0), 0);
    }

    #[test]
    fn drops_low_priority_columns_first_and_keeps_the_first() {
        let natural = [100.0, 100.0, 100.0];
        let least = [50.0, 50.0, 50.0];
        let kept = table_columns(&natural, &least, &[0, 1, 0], 110.0);
        let columns: Vec<usize> = kept.iter().map(|(i, _)| *i).collect();
        assert_eq!(columns, [0, 1]);
        assert!(kept.iter().map(|(_, w)| w).sum::<f32>() <= 110.5);
        let kept = table_columns(&natural, &least, &[0, 0, 0], 60.0);
        assert_eq!(kept[0].0, 0);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn reads_inline_markdown() {
        assert_eq!(
            inline_markdown("a **b** `c`"),
            [
                ("a ", Inline::Plain),
                ("b", Inline::Strong),
                (" ", Inline::Plain),
                ("c", Inline::Code)
            ]
        );
        assert_eq!(inline_markdown("2 * 3"), [("2 * 3", Inline::Plain)]);
    }
}
