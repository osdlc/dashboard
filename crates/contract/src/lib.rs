//! Types shared by the host, built-in modules, stdio plugins and renderers.
//! See `docs/contract.md`. These types are the source of truth for the
//! generated JSON Schema and TypeScript types.

// The docs here become the descriptions in the JSON Schema and the plugin
// SDKs, so every public item needs one.
#![warn(missing_docs)]

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod catalog;
pub mod check;
pub mod marketplace;
pub mod rpc;
pub mod theme;

pub use catalog::{
    CatalogEntry, CatalogResponse, EntryKind, MARKETPLACE, PACKAGE_FILE, Package, Requirements,
};
pub use marketplace::{Index, IndexEntry};
pub use theme::{ColorTheme, Tokens, Tone};

/// The contract version, `MAJOR.MINOR`. The host refuses a plugin with a different major version.
pub const CONTRACT_VERSION: &str = "0.1";

// ---- Manifest ----

/// What a module declares about itself, in reply to `initialize`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Manifest {
    /// Unique module ID, e.g. `cswap`.
    pub id: String,
    /// Display name, shown in the tile header.
    pub name: String,
    /// The contract version the module was built for, e.g. "0.1".
    pub contract_version: String,
    /// How the module's view is refreshed: polled by the host or pushed by the module.
    pub refresh: Refresh,
    /// Size and placement hints for the module's tile.
    pub layout: LayoutHints,
    /// Commands the host may run for this module after the user confirms.
    #[serde(default)]
    pub commands: Vec<CommandDecl>,
    /// JSON Schema for the module's `config:` block. `None`: no schema.
    #[serde(default)]
    pub config_schema: Option<serde_json::Value>,
}

/// How a tile gets new views.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Refresh {
    /// The host asks for a view (`view/render`) every `interval_ms`.
    Poll {
        /// Suggested period. Config can override it.
        interval_ms: u64,
        /// Floor for config overrides, e.g. to stay under an API rate limit.
        #[serde(default)]
        min_interval_ms: Option<u64>,
    },
    /// The module sends `view/update` notifications when it has a new view.
    Push,
}

/// Size and placement hints for a module's tile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutHints {
    /// The size the module suggests; a screen or the config can override it.
    pub size: Size,
    /// Higher is placed earlier when auto-placing tiles. Default 0.
    #[serde(default)]
    pub priority: i32,
    /// The smallest size the view still works at. `None`: any size.
    #[serde(default)]
    pub min_size: Option<Size>,
}

/// A tile's size class on the 12-column grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    /// 3 columns by 1 row.
    Small,
    /// 4 columns by 2 rows.
    Medium,
    /// 6 columns by 3 rows.
    Large,
    /// 8 columns by 2 rows.
    Wide,
    /// 4 columns by 4 rows.
    Tall,
}

// ---- Commands ----

/// A command a module declares. The host runs it only after the user confirms.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CommandDecl {
    /// Names the command in `run_command` actions, e.g. "switch".
    pub id: String,
    /// Shown to the user, e.g. "Switch to account".
    pub label: String,
    /// Program and arguments. May contain `{name}` placeholders declared in `params`.
    pub argv: Vec<String>,
    /// Working directory to run in. `None`: the host's own.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Placeholders `argv` may use, each checked against its pattern.
    #[serde(default)]
    pub params: Vec<ParamDecl>,
}

/// A `{name}` placeholder in a command's `argv`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ParamDecl {
    /// The placeholder's name, without braces.
    pub name: String,
    /// Regex the value must fully match.
    pub pattern: String,
}

// ---- View ----

/// What a tile shows: an overall status, a one-line summary and a widget tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct View {
    /// The tile's overall health, shown as its status color.
    pub status: Status,
    /// One line, used when the tile is too small.
    pub summary: String,
    /// The widget the tile shows; use `row` and `column` to combine several.
    pub root: Widget,
}

/// Health of a tile, row or item: colors it ok, warn or error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// All good.
    Ok,
    /// Needs attention.
    Warn,
    /// Broken or failing.
    Error,
    /// Not known yet, e.g. before the first check.
    Unknown,
}

/// A piece of a view. `type` names the kind: `text`, `stat`, `gauge`, `table` and so on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Widget {
    /// A paragraph of text.
    Text {
        /// The text to show.
        text: String,
        /// Read markdown in `text`: `**bold**`, and code between backticks.
        #[serde(default)]
        markdown: bool,
    },
    /// One number with a label, e.g. "p99 107 ms".
    Stat {
        /// Shown above or before the value.
        label: String,
        /// Already formatted, e.g. "3451" or "1.2k".
        value: String,
        /// Shown after the value, smaller, e.g. "ms" or "/s".
        #[serde(default)]
        unit: Option<String>,
        /// A change to show beside the value, e.g. "+2.8%".
        #[serde(default)]
        delta: Option<String>,
        /// Colors the value: ok, warn or error.
        #[serde(default)]
        status: Option<Status>,
    },
    /// A horizontal meter from 0 to 100%.
    Gauge {
        /// Shown with the meter.
        label: String,
        /// How full, 0-100.
        percent: f64,
        /// A tick on the track at this percent, e.g. where usage should be by now.
        #[serde(default)]
        marker: Option<f64>,
        /// Percents at which the fill turns warn and error colored.
        #[serde(default)]
        thresholds: Option<Thresholds>,
        /// A line under the meter, e.g. "1721 of 1858 GiB used".
        #[serde(default)]
        caption: Option<String>,
    },
    /// Values over time, as a line, a filled area or bars.
    Series {
        /// The chart's title.
        label: String,
        /// Shown after values, e.g. "%" or "/s".
        #[serde(default)]
        unit: Option<String>,
        /// The top of the chart. Default: the largest value (at least 1 when no
        /// value is negative and `y_min` is unset).
        #[serde(default)]
        y_max: Option<f64>,
        /// The bottom of the chart. Default: 0, or the smallest value when that
        /// is negative. Set it to chart values that never reach 0.
        #[serde(default)]
        y_min: Option<f64>,
        /// Scale to the data's own range with a little padding (for prices that
        /// sit far from 0). Ignored when `y_min` or `y_max` is set, and when
        /// `mirrored`.
        #[serde(default)]
        fit: bool,
        /// When set, the host keeps history for this widget: each update's
        /// `points` are new samples, appended to what the host already holds
        /// for (tile, `id`, series name). Renderers always get the full history.
        #[serde(default)]
        id: Option<String>,
        /// Points the host keeps per series when `id` is set. Default 300.
        #[serde(default)]
        history: Option<u32>,
        /// A line, a filled area or one bar per sample.
        #[serde(default)]
        style: SeriesStyle,
        /// The color of a single series.
        #[serde(default)]
        color: ColorScale,
        /// Draw the first series above the axis and the second below it,
        /// e.g. network download and upload. Negative values count as 0 here
        /// (`sitroom plugin-check` flags them).
        #[serde(default)]
        mirrored: bool,
        /// How values are written: plain, bytes or count.
        #[serde(default)]
        format: ValueFormat,
        /// One or more lines, each with its points.
        series: Vec<SeriesData>,
    },
    /// Many small meters in a compact grid, e.g. one per CPU core.
    MeterGrid {
        /// A title above the grid.
        #[serde(default)]
        label: Option<String>,
        /// The meters, in order.
        meters: Vec<Meter>,
        /// The meters' color: `accent`, or a scale colored by each meter's value.
        #[serde(default)]
        color: ColorScale,
    },
    /// Labeled horizontal bars, in the order given.
    Bars {
        /// A title above the bars.
        #[serde(default)]
        label: Option<String>,
        /// Shown after each value.
        #[serde(default)]
        unit: Option<String>,
        /// How values are written: plain, bytes or count.
        #[serde(default)]
        format: ValueFormat,
        /// Value of a full bar. Default: the largest value.
        #[serde(default)]
        max: Option<f64>,
        /// The bars, top to bottom.
        bars: Vec<Bar>,
    },
    /// Shares of a whole, as a pie or donut with a legend. Slices keep the
    /// order given; each one's share is its value over the sum of all values.
    Pie {
        /// A title above the chart.
        #[serde(default)]
        label: Option<String>,
        /// The slices, in order. Values are zero or more.
        slices: Vec<Slice>,
        /// A ring with a hole in the middle. Default true; false draws a full disc.
        #[serde(default = "yes")]
        donut: bool,
        /// How values are written in the legend: plain, bytes or count.
        #[serde(default)]
        format: ValueFormat,
        /// Shown after each value in the legend.
        #[serde(default)]
        unit: Option<String>,
        /// `accent` (the default) gives each slice its own color, the theme's
        /// accent first. The other scales spread the slices along the scale
        /// in order. A slice's `status` overrides its color.
        #[serde(default)]
        color: ColorScale,
    },
    /// An analog clock face with hour and minute hands, and a second hand
    /// when `second` is given. The renderer draws it; it never reads a clock.
    ClockFace {
        /// 0-23.
        hour: u8,
        /// 0-59.
        minute: u8,
        /// 0-59. No second hand when absent.
        #[serde(default)]
        second: Option<u8>,
        /// Shown above the face, e.g. "Tokyo".
        #[serde(default)]
        label: Option<String>,
        /// Shown below the face, e.g. a date.
        #[serde(default)]
        caption: Option<String>,
        /// Colors the face's outline.
        #[serde(default)]
        status: Option<Status>,
    },
    /// One value as large as the space allows: a big number or a clock.
    Display {
        /// Already formatted, e.g. "14:05". The TUI draws it in a block font
        /// when it has a glyph for every character, else as plain text; see
        /// docs/contract.md, "Clock face and display".
        text: String,
        /// Shown above the text.
        #[serde(default)]
        label: Option<String>,
        /// Shown below the text.
        #[serde(default)]
        caption: Option<String>,
        /// Shown after the text, smaller, e.g. "PM".
        #[serde(default)]
        unit: Option<String>,
        /// Colors the text.
        #[serde(default)]
        status: Option<Status>,
    },
    /// Items with a title, a subtitle, a status marker and tags.
    List {
        /// The items, top to bottom.
        items: Vec<ListItem>,
        /// Shown when there are no items.
        #[serde(default)]
        empty_text: Option<String>,
    },
    /// Rows and columns. A cell holds text, a number, or a stat, gauge, pie, display or clock face.
    Table {
        /// The columns, left to right.
        columns: Vec<Column>,
        /// The rows, top to bottom.
        rows: Vec<Row>,
    },
    /// Log output, oldest first. Renderers show the newest lines that fit.
    Log {
        /// A title above the lines.
        #[serde(default)]
        label: Option<String>,
        /// The lines, oldest first.
        lines: Vec<LogLine>,
    },
    /// Availability checks, one row each with a strip of recent results.
    Uptime {
        /// The checks, one row each.
        checks: Vec<UptimeCheck>,
    },
    /// A picture from a file on the machine sitroom runs on. The terminal
    /// draws it with the best graphics it supports (Kitty, iTerm2 or Sixel),
    /// or with colored half blocks. It is scaled to fit and keeps its shape.
    Image {
        /// Image file: PNG, JPEG, GIF (first frame), WebP or BMP.
        path: String,
        /// A line under the picture, e.g. the file name.
        #[serde(default)]
        caption: Option<String>,
    },
    /// Widgets side by side, sharing the width.
    Row {
        /// The widgets, left to right.
        children: Vec<Widget>,
    },
    /// Widgets stacked top to bottom.
    Column {
        /// The widgets, top to bottom.
        children: Vec<Widget>,
    },
}

/// How a `series` chart draws its values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SeriesStyle {
    /// A plain line. The default.
    #[default]
    Line,
    /// Filled under the line.
    Area,
    /// One column per sample.
    Bars,
}

/// A color, or a scale that colors by value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ColorScale {
    /// The theme's accent color.
    #[default]
    Accent,
    /// Green to yellow to red by value, relative to the maximum.
    Heat,
    /// Teal to blue to violet by value.
    Cool,
    /// Violet to pink to orange by value.
    Sunset,
}

/// How numbers are written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ValueFormat {
    /// As given, e.g. "42%". The default.
    #[default]
    Plain,
    /// Show as KiB / MiB / GiB; `unit` is appended (e.g. "/s").
    Bytes,
    /// Show large counts short: 950, 12.3k, 4.2M, 1.1B.
    Count,
}

/// One small meter in a `meter_grid`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Meter {
    /// Shown beside the meter, e.g. a core number.
    pub label: String,
    /// 0-100.
    pub percent: f64,
    /// A fixed color for this meter instead of the grid's color scale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<Tone>,
}

/// One bar in a `bars` widget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Bar {
    /// Shown beside the bar.
    pub label: String,
    /// Zero or more; the bar's length is this over the widget's `max`.
    pub value: f64,
    /// Colors the bar ok, warn or error.
    #[serde(default)]
    pub status: Option<Status>,
    /// Fill color when `status` is not set. Default: `accent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<Tone>,
}

/// One slice of a `pie`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Slice {
    /// Shown in the legend.
    pub label: String,
    /// Zero or more. Shown with the widget's `format` and `unit`.
    pub value: f64,
    /// Colors the slice ok, warn or error.
    #[serde(default)]
    pub status: Option<Status>,
    /// Color when `status` is not set. Default: the next `series-N` token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<Tone>,
}

fn yes() -> bool {
    true
}

/// The vertical range of a `series` chart as (bottom, top), shared by every renderer.
///
/// Bottom is `y_min`, else the smaller of 0 and the lowest value. Top is `y_max`, else
/// the highest value, which is at least 1 when the bottom is 0 or more and `y_min` is
/// unset (so a chart of small positive values draws as it always has). With `fit` and
/// no bounds, the range is the data's own with 5% padding. The top is always above the
/// bottom.
pub fn series_scale(
    values: &[f64],
    y_min: Option<f64>,
    y_max: Option<f64>,
    fit: bool,
) -> (f64, f64) {
    const EPS: f64 = 1e-9;
    let (lo, hi) = values
        .iter()
        .filter(|v| v.is_finite())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| {
            (l.min(*v), h.max(*v))
        });
    let has_data = lo.is_finite();
    if fit && y_min.is_none() && y_max.is_none() && has_data {
        let pad = if hi > lo {
            (hi - lo) * 0.05
        } else {
            lo.abs().max(1.0) * 0.01
        };
        return (lo - pad, hi + pad);
    }
    let data_lo = if has_data { lo } else { 0.0 };
    let data_hi = if has_data { hi } else { 0.0 };
    let bottom = y_min.unwrap_or(data_lo.min(0.0));
    let top = y_max.unwrap_or(if y_min.is_none() && bottom >= 0.0 {
        data_hi.max(1.0)
    } else {
        data_hi
    });
    (bottom, top.max(bottom + EPS))
}

/// Formats a value for display. Shared so every renderer shows the same text.
pub fn format_value(value: f64, format: ValueFormat, unit: Option<&str>) -> String {
    let unit = unit.unwrap_or("");
    match format {
        ValueFormat::Plain => {
            // Small percentages keep one decimal ("-1.2%"), unless they are whole.
            let decimals = usize::from(unit == "%" && value.abs() < 10.0 && value.fract() != 0.0);
            let text = format!("{value:.decimals$}");
            // Never "-0" or "-0.0": a value that rounds to zero has no sign.
            let text = if text
                .trim_start_matches('-')
                .chars()
                .all(|c| c == '0' || c == '.')
            {
                text.trim_start_matches('-').to_string()
            } else {
                text
            };
            format!("{text}{unit}")
        }
        ValueFormat::Bytes => {
            const STEPS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
            let mut v = value.abs();
            let mut i = 0;
            while v >= 1024.0 && i < STEPS.len() - 1 {
                v /= 1024.0;
                i += 1;
            }
            let sign = if value < 0.0 { "-" } else { "" };
            if i == 0 {
                format!("{sign}{v:.0} {}{unit}", STEPS[i])
            } else {
                format!("{sign}{v:.1} {}{unit}", STEPS[i])
            }
        }
        ValueFormat::Count => {
            const STEPS: [&str; 4] = ["", "k", "M", "B"];
            let mut v = value.abs();
            let mut i = 0;
            while v >= 1000.0 && i < STEPS.len() - 1 {
                v /= 1000.0;
                i += 1;
            }
            let sign = if value < 0.0 { "-" } else { "" };
            if i == 0 || v >= 100.0 {
                format!("{sign}{v:.0}{}{unit}", STEPS[i])
            } else {
                format!("{sign}{v:.1}{}{unit}", STEPS[i])
            }
        }
    }
}

/// Percents at which a gauge's fill changes color.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Thresholds {
    /// The percent at which the fill turns the warn color.
    pub warn: f64,
    /// The percent at which the fill turns the error color.
    pub error: f64,
}

/// One line of a `series` chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SeriesData {
    /// Shown in the legend; also keys the history when the widget has an `id`.
    pub name: String,
    /// (unix ms, value)
    pub points: Vec<(i64, f64)>,
    /// This series' color, instead of the widget's `color` (one series) or
    /// the next `series-N` token (several).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<Tone>,
}

/// One availability check in an `uptime` widget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UptimeCheck {
    /// What is checked, e.g. a host or URL.
    pub name: String,
    /// Result of the latest check.
    pub status: Status,
    /// Share of recent checks that passed, 0-100.
    #[serde(default)]
    pub uptime: Option<f64>,
    /// Response time of the latest check.
    #[serde(default)]
    pub latency_ms: Option<f64>,
    /// e.g. "HTTP 200" or the error.
    #[serde(default)]
    pub detail: Option<String>,
    /// Recent results, oldest first. Renderers show the newest that fit.
    #[serde(default)]
    pub history: Vec<Status>,
    /// Opened when the check is clicked.
    #[serde(default)]
    pub url: Option<String>,
}

/// One line in a `log` widget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LogLine {
    /// The log message.
    pub text: String,
    /// error, warn, info or debug: colors the line.
    #[serde(default)]
    pub level: Option<rpc::LogLevel>,
    /// Shown before the text, e.g. "14:02:11".
    #[serde(default)]
    pub time: Option<String>,
}

/// One item in a `list` widget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ListItem {
    /// The main line.
    pub title: String,
    /// A second, quieter line.
    #[serde(default)]
    pub subtitle: Option<String>,
    /// Colors the item's marker ok, warn or error.
    #[serde(default)]
    pub status: Option<Status>,
    /// Color of the item's marker when `status` is not set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<Tone>,
    /// Short labels shown as chips.
    #[serde(default)]
    pub tags: Vec<String>,
    /// What clicking the item does, e.g. open a link.
    #[serde(default)]
    pub action: Option<Action>,
}

/// A table column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Column {
    /// Names the cell in each row's `cells`.
    pub key: String,
    /// The column heading.
    pub label: String,
    /// Which columns to keep when a table is too narrow for all of them:
    /// renderers drop the lowest priority first (the rightmost of equal
    /// ones) and always keep the first column. Default 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
}

/// A table row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Row {
    /// Keyed by the column's `key`.
    pub cells: BTreeMap<String, Cell>,
    /// Colors the row's marker ok, warn or error.
    #[serde(default)]
    pub status: Option<Status>,
    /// What clicking the row does, e.g. open a link.
    #[serde(default)]
    pub action: Option<Action>,
}

/// A table cell: text, a number or a small widget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Cell {
    /// Plain text.
    Text(String),
    /// A number, shown as is.
    Number(f64),
    /// `Stat`, `Gauge`, `Pie`, `Display` and `ClockFace` are rendered inline (the
    /// TUI shows a pie as its largest slice, a clock face as "14:05"); others
    /// fall back to text.
    Widget(Widget),
}

// ---- Actions ----

/// What clicking a row or item does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Open a link in the browser.
    OpenUrl {
        /// The link: http or https only.
        url: String,
    },
    /// Run a command the module declared, after the user confirms.
    RunCommand {
        /// The command's `id` from the manifest.
        command: String,
        /// Values for the command's `{name}` placeholders. Each must match its pattern.
        #[serde(default)]
        params: BTreeMap<String, String>,
    },
}

// ---- Renderer feed ----

/// What the host sends to renderers (TUI, web) for each tile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TileUpdate {
    /// The tile this update is for.
    pub tile_id: String,
    /// What the tile shows.
    pub view: View,
    /// Unix ms.
    pub updated_at: i64,
    /// True when the last refresh failed and `view` is the last good one.
    pub stale: bool,
    /// True when the config asks for the module's demo view (`demo: true`):
    /// made-up data, not the module's real output.
    #[serde(default)]
    pub demo: bool,
}

/// Light or dark, for the whole sitroom.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Follow the OS (web) or the terminal's background color (TUI).
    #[default]
    Auto,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

/// Renderer-wide settings derived from the config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiSettings {
    /// Light, dark or auto: picks the variant of `color_theme`.
    pub theme: Theme,
    /// The color theme in use, both variants; `theme` picks one.
    #[serde(default)]
    pub color_theme: ColorTheme,
    /// Color themes to choose from: built-ins, then the config's own.
    #[serde(default)]
    pub color_themes: Vec<String>,
    /// At least one screen.
    pub screens: Vec<ScreenLayout>,
    /// Running modules, for the help screen.
    pub modules: Vec<ModuleSummary>,
    /// Automatic screen rotation, if configured.
    #[serde(default)]
    pub rotate: Option<RotateSettings>,
    /// The most times a second the terminal view draws the screen again when
    /// tiles change. The web view does not use it.
    #[serde(default = "default_tui_fps")]
    pub tui_fps: u32,
    /// The most frames a second the native app draws. The web view does not
    /// use it.
    #[serde(default = "default_native_fps")]
    pub native_fps: u32,
    /// False: the native app shows new values without motion.
    #[serde(default = "yes")]
    pub native_animations: bool,
    /// Where the config came from, for the settings screen.
    pub config: ConfigInfo,
}

/// The terminal view's redraw limit when the config sets none.
pub const DEFAULT_TUI_FPS: u32 = 4;

fn default_tui_fps() -> u32 {
    DEFAULT_TUI_FPS
}

/// The native app's frame limit when the config sets none.
pub const DEFAULT_NATIVE_FPS: u32 = 30;

fn default_native_fps() -> u32 {
    DEFAULT_NATIVE_FPS
}

/// Where the config came from, for the settings screen.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigInfo {
    /// The config file's path.
    pub path: String,
    /// False when no file exists and defaults are in use.
    pub exists: bool,
    /// Set when the file changed but couldn't be applied; the previous
    /// config stays in effect.
    #[serde(default)]
    pub error: Option<String>,
    /// How the last `add_module` went, so the editor that sent it can tell.
    #[serde(default)]
    pub added: Option<AddOutcome>,
    /// How the last `create_dashboard` went.
    #[serde(default)]
    pub created: Option<CreateOutcome>,
    /// The saved dashboard in use, by name. `None`: `config.yaml` alone.
    #[serde(default)]
    pub dashboard: Option<String>,
    /// Saved dashboards to switch to.
    #[serde(default)]
    pub dashboards: Vec<DashboardSummary>,
    /// The files merged into the config, lowest precedence first, e.g.
    /// "machine: ~/.config/sitroom/config.yaml".
    #[serde(default)]
    pub layers: Vec<String>,
    /// The version of the file at `path`, which changes are written to (a sha256 etag),
    /// `None` when there is no file. Send it back with a change: a change
    /// based on an older version is refused.
    #[serde(default)]
    pub etag: Option<String>,
}

/// The result of a `create_dashboard`. With no error the file was written and
/// the switch to it tried; a failed switch shows as the config error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CreateOutcome {
    /// The name the request asked for.
    pub name: String,
    /// Why the create failed. `None`: the file was written.
    #[serde(default)]
    pub error: Option<String>,
    /// Counts the requests the server has handled, as in `AddOutcome`.
    #[serde(default)]
    pub seq: u64,
}

/// The result of an `add_module`. With no error the config was written; the
/// module may still fail to start, which shows as the config error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AddOutcome {
    /// The ID the module was added under.
    pub module_id: String,
    /// Why the add failed. `None`: the config was written.
    #[serde(default)]
    pub error: Option<String>,
    /// Counts the adds the server has handled, so a renderer can tell the
    /// outcome of its add from an earlier one, which every later settings
    /// message still carries.
    #[serde(default)]
    pub seq: u64,
}

/// A saved dashboard, for the switcher.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DashboardSummary {
    /// Sitroom's name.
    pub name: String,
    /// A line about what it shows.
    #[serde(default)]
    pub description: Option<String>,
}

/// Automatic screen rotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RotateSettings {
    /// Time on each screen, in ms.
    pub interval_ms: u64,
    /// Screen names to rotate through, in order. Never empty.
    pub screens: Vec<String>,
    /// Idle time after user input before rotation resumes.
    pub resume_after_ms: u64,
}

/// A named screen and the tiles on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScreenLayout {
    /// The screen's name, shown in the switcher.
    pub name: String,
    /// Tiles in placement order.
    pub tiles: Vec<TilePlacement>,
}

/// Where a tile goes on a screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TilePlacement {
    /// The tile's module ID.
    pub tile_id: String,
    /// Resolved size: screen config, else module config, else the module's hint.
    pub size: Size,
}

/// A running module, for the help screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModuleSummary {
    /// The module ID in the config.
    pub tile_id: String,
    /// The module's display name.
    pub name: String,
    /// Effective poll period; `None` for push modules.
    #[serde(default)]
    pub refresh_ms: Option<u64>,
    /// True when `refresh_ms` comes from the config rather than the module.
    #[serde(default)]
    pub refresh_overridden: bool,
    /// Labels of declared commands.
    #[serde(default)]
    pub commands: Vec<String>,
    /// The size the module suggests, used when a screen doesn't set one.
    pub default_size: Size,
}

/// Actions from a renderer: the web view sends these over its WebSocket, the
/// TUI over a channel. The host carries them out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Write `theme:` into the config file. `etag` is `config.etag` from the
    /// settings the change was made on.
    SetTheme {
        /// Light, dark or auto.
        theme: Theme,
        /// `config.etag` from the settings the change was made on. `None` when there was no config file; refused if one exists now.
        #[serde(default)]
        etag: Option<String>,
    },
    /// Write `color_theme:` into the config file. `etag` as for `SetTheme`.
    SetColorTheme {
        /// The color theme's name.
        name: String,
        /// As for `SetTheme`.
        #[serde(default)]
        etag: Option<String>,
    },
    /// Replace `screens:` in the config file (from the screen editor). `etag`
    /// as for `SetTheme`.
    SaveScreens {
        /// The new screens, in order.
        screens: Vec<ScreenLayout>,
        /// As for `SetTheme`.
        #[serde(default)]
        etag: Option<String>,
    },
    /// Add a catalog entry to the config (from the screen editor's catalog):
    /// a `modules:` entry and, with `screen`, a tile at the end of that screen.
    AddModule {
        /// The new module's ID in the config (its key under `modules:`),
        /// usually the catalog ID.
        module_id: String,
        /// The catalog entry's ID and kind.
        entry: String,
        /// Whether `entry` is a built-in or a plugin.
        kind: EntryKind,
        /// The module's `config:` block. Null or `{}` writes none.
        #[serde(default)]
        config: serde_json::Value,
        /// Show the demo view instead of running it.
        #[serde(default)]
        demo: bool,
        /// The screen to put a tile on. Ignored when the config has no `screens:`.
        #[serde(default)]
        screen: Option<String>,
        /// The new tile's size.
        size: Size,
    },
    /// Show another saved dashboard; `None` for `config.yaml` alone.
    SwitchDashboard {
        /// The saved dashboard's name. `None`: `config.yaml` alone.
        #[serde(default)]
        name: Option<String>,
    },
    /// Save a new dashboard and switch to it: a copy of the one in use, or
    /// one empty screen. The name is made into a slug.
    CreateDashboard {
        /// The new dashboard's name, before it is made into a slug.
        name: String,
        /// Copy sitroom in use. `false`: one empty screen.
        #[serde(default)]
        copy: bool,
    },
}

/// Messages the host sends to the web view over its WebSocket.
// Settings (with the color theme) are much larger than a tile update, but
// sent rarely; boxing them would only add noise where they are built.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Sent first on connect, and again when settings change.
    Settings {
        /// Everything renderers need besides tiles.
        settings: UiSettings,
    },
    /// A tile's new view.
    Tile {
        /// The update.
        tile: TileUpdate,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes() {
        assert_eq!(
            format_value(512.0, ValueFormat::Bytes, Some("/s")),
            "512 B/s"
        );
        assert_eq!(format_value(1536.0, ValueFormat::Bytes, None), "1.5 KiB");
        assert_eq!(
            format_value(3.0 * 1024.0 * 1024.0 * 1024.0, ValueFormat::Bytes, None),
            "3.0 GiB"
        );
        assert_eq!(format_value(42.4, ValueFormat::Plain, Some("%")), "42%");
        assert_eq!(format_value(-1.24, ValueFormat::Plain, Some("%")), "-1.2%");
        assert_eq!(format_value(-0.04, ValueFormat::Plain, Some("%")), "0.0%");
        assert_eq!(format_value(5.0, ValueFormat::Plain, Some("%")), "5%");
        assert_eq!(
            format_value(-1.24, ValueFormat::Plain, Some(" ms")),
            "-1 ms"
        );
    }

    #[test]
    fn series_scale_rules() {
        assert_eq!(series_scale(&[3.0, 40.0], None, None, false), (0.0, 40.0));
        assert_eq!(series_scale(&[0.2], None, None, false), (0.0, 1.0));
        assert_eq!(series_scale(&[-2.0, 1.0], None, None, false), (-2.0, 1.0));
        assert_eq!(series_scale(&[3.0], None, Some(100.0), false), (0.0, 100.0));
        let (lo, hi) = series_scale(&[354.0, 357.0], None, None, true);
        assert!((lo - 353.85).abs() < 1e-9 && (hi - 357.15).abs() < 1e-9);
        assert_eq!(
            series_scale(&[354.0], Some(300.0), None, true),
            (300.0, 354.0)
        );
    }

    #[test]
    fn pie_example_from_docs_defaults_to_a_donut() {
        let json = r#"{"type": "row", "children": [
          {"type": "pie", "label": "Disk", "format": "bytes", "slices": [
            {"label": "used", "value": 300000000000},
            {"label": "free", "value": 200000000000, "status": "warn"}]},
          {"type": "stat", "label": "Used", "value": "279", "unit": "GiB"}
        ]}"#;
        let w: Widget = serde_json::from_str(json).unwrap();
        let Widget::Row { children } = &w else {
            panic!("expected row")
        };
        let Widget::Pie {
            slices,
            donut,
            format,
            color,
            ..
        } = &children[0]
        else {
            panic!("expected pie")
        };
        assert!(*donut);
        assert_eq!(*format, ValueFormat::Bytes);
        assert_eq!(*color, ColorScale::Accent);
        assert_eq!(slices[1].status, Some(Status::Warn));
        let again: Widget = serde_json::from_value(serde_json::to_value(&w).unwrap()).unwrap();
        assert_eq!(w, again);
    }

    #[test]
    fn cswap_example_from_docs_round_trips() {
        let json = r#"{
          "status":"warn",
          "summary":"2 accounts",
          "root":{"type":"table",
            "columns":[{"key":"account","label":"Account"},{"key":"five_hour","label":"5 hours"}],
            "rows":[
              {"status":"warn","cells":{"account":"1 work@example.com",
                "five_hour":{"type":"gauge","label":"5h","percent":81,"thresholds":{"warn":75,"error":90}}},
               "action":null},
              {"status":"ok","cells":{"account":"2 personal@example.com","five_hour":12.5},
               "action":{"type":"run_command","command":"switch","params":{"account":"2"}}}
            ]}
        }"#;
        let view: View = serde_json::from_str(json).unwrap();
        let Widget::Table { rows, .. } = &view.root else {
            panic!("expected table")
        };
        assert!(matches!(
            rows[0].cells["five_hour"],
            Cell::Widget(Widget::Gauge { .. })
        ));
        assert!(matches!(rows[1].cells["five_hour"], Cell::Number(_)));
        assert!(matches!(rows[1].action, Some(Action::RunCommand { .. })));

        let again: View = serde_json::from_value(serde_json::to_value(&view).unwrap()).unwrap();
        assert_eq!(view, again);
    }
}
