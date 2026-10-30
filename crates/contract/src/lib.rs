//! Types shared by the host, built-in modules, stdio plugins and renderers.
//! See `docs/contract.md`. These types are the source of truth for the
//! generated JSON Schema and TypeScript types.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod catalog;
pub mod check;
pub mod rpc;

pub use catalog::{CatalogEntry, EntryKind, PACKAGE_FILE, Package, Requirements};

pub const CONTRACT_VERSION: &str = "0.1";

// ---- Manifest ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub contract_version: String,
    pub refresh: Refresh,
    pub layout: LayoutHints,
    #[serde(default)]
    pub commands: Vec<CommandDecl>,
    #[serde(default)]
    pub config_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Refresh {
    Poll {
        /// Suggested period. Config can override it.
        interval_ms: u64,
        /// Floor for config overrides, e.g. to stay under an API rate limit.
        #[serde(default)]
        min_interval_ms: Option<u64>,
    },
    Push,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutHints {
    pub size: Size,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub min_size: Option<Size>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    Small,
    Medium,
    Large,
    Wide,
    Tall,
}

// ---- Commands ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CommandDecl {
    pub id: String,
    pub label: String,
    /// Program and arguments. May contain `{name}` placeholders declared in `params`.
    pub argv: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub params: Vec<ParamDecl>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ParamDecl {
    pub name: String,
    /// Regex the value must fully match.
    pub pattern: String,
}

// ---- View ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct View {
    pub status: Status,
    /// One line, used when the tile is too small.
    pub summary: String,
    pub root: Widget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Warn,
    Error,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Widget {
    Text {
        text: String,
        #[serde(default)]
        markdown: bool,
    },
    Stat {
        label: String,
        value: String,
        #[serde(default)]
        unit: Option<String>,
        #[serde(default)]
        delta: Option<String>,
        #[serde(default)]
        status: Option<Status>,
    },
    Gauge {
        label: String,
        percent: f64,
        #[serde(default)]
        marker: Option<f64>,
        #[serde(default)]
        thresholds: Option<Thresholds>,
        #[serde(default)]
        caption: Option<String>,
    },
    Series {
        label: String,
        #[serde(default)]
        unit: Option<String>,
        #[serde(default)]
        y_max: Option<f64>,
        /// When set, the host keeps history for this widget: each update's
        /// `points` are new samples, appended to what the host already holds
        /// for (tile, `id`, series name). Renderers always get the full history.
        #[serde(default)]
        id: Option<String>,
        /// Points the host keeps per series when `id` is set. Default 300.
        #[serde(default)]
        history: Option<u32>,
        #[serde(default)]
        style: SeriesStyle,
        #[serde(default)]
        color: ColorScale,
        /// Draw the first series above the axis and the second below it,
        /// e.g. network download and upload.
        #[serde(default)]
        mirrored: bool,
        #[serde(default)]
        format: ValueFormat,
        series: Vec<SeriesData>,
    },
    /// Many small meters in a compact grid, e.g. one per CPU core.
    MeterGrid {
        #[serde(default)]
        label: Option<String>,
        meters: Vec<Meter>,
        #[serde(default)]
        color: ColorScale,
    },
    /// Labeled horizontal bars, in the order given.
    Bars {
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        unit: Option<String>,
        #[serde(default)]
        format: ValueFormat,
        /// Value of a full bar. Default: the largest value.
        #[serde(default)]
        max: Option<f64>,
        bars: Vec<Bar>,
    },
    List {
        items: Vec<ListItem>,
        #[serde(default)]
        empty_text: Option<String>,
    },
    Table {
        columns: Vec<Column>,
        rows: Vec<Row>,
    },
    /// Log output, oldest first. Renderers show the newest lines that fit.
    Log {
        #[serde(default)]
        label: Option<String>,
        lines: Vec<LogLine>,
    },
    /// Availability checks, one row each with a strip of recent results.
    Uptime {
        checks: Vec<UptimeCheck>,
    },
    /// A picture from a file on the machine the dashboard runs on. The terminal
    /// draws it with the best graphics it supports (Kitty, iTerm2 or Sixel),
    /// or with colored half blocks. It is scaled to fit and keeps its shape.
    Image {
        /// Image file: PNG, JPEG, GIF (first frame), WebP or BMP.
        path: String,
        /// A line under the picture, e.g. the file name.
        #[serde(default)]
        caption: Option<String>,
    },
    Row {
        children: Vec<Widget>,
    },
    Column {
        children: Vec<Widget>,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SeriesStyle {
    #[default]
    Line,
    /// Filled under the line.
    Area,
    /// One column per sample.
    Bars,
}

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ValueFormat {
    #[default]
    Plain,
    /// Show as KiB / MiB / GiB; `unit` is appended (e.g. "/s").
    Bytes,
    /// Show large counts short: 950, 12.3k, 4.2M, 1.1B.
    Count,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Meter {
    pub label: String,
    pub percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Bar {
    pub label: String,
    pub value: f64,
    #[serde(default)]
    pub status: Option<Status>,
}

/// Formats a value for display. Shared so every renderer shows the same text.
pub fn format_value(value: f64, format: ValueFormat, unit: Option<&str>) -> String {
    let unit = unit.unwrap_or("");
    match format {
        ValueFormat::Plain => format!("{value:.0}{unit}"),
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Thresholds {
    pub warn: f64,
    pub error: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SeriesData {
    pub name: String,
    /// (unix ms, value)
    pub points: Vec<(i64, f64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UptimeCheck {
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LogLine {
    pub text: String,
    #[serde(default)]
    pub level: Option<rpc::LogLevel>,
    /// Shown before the text, e.g. "14:02:11".
    #[serde(default)]
    pub time: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ListItem {
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub action: Option<Action>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Column {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Row {
    /// Keyed by `Column::key`.
    pub cells: BTreeMap<String, Cell>,
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default)]
    pub action: Option<Action>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Cell {
    Text(String),
    Number(f64),
    /// Only `Stat` and `Gauge` are rendered inline; others fall back to text.
    Widget(Widget),
}

// ---- Actions ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    OpenUrl {
        url: String,
    },
    RunCommand {
        command: String,
        #[serde(default)]
        params: BTreeMap<String, String>,
    },
}

// ---- Renderer feed ----

/// What the host sends to renderers (TUI, web) for each tile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TileUpdate {
    pub tile_id: String,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Follow the OS (web) or the terminal's background color (TUI).
    #[default]
    Auto,
    Light,
    Dark,
}

/// Renderer-wide settings derived from the config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiSettings {
    pub theme: Theme,
    /// At least one screen.
    pub screens: Vec<ScreenLayout>,
    /// Running modules, for the help screen.
    pub modules: Vec<ModuleSummary>,
    /// Automatic screen rotation, if configured.
    #[serde(default)]
    pub rotate: Option<RotateSettings>,
    pub config: ConfigInfo,
}

/// Where the config came from, for the settings screen.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigInfo {
    pub path: String,
    /// False when no file exists and defaults are in use.
    pub exists: bool,
    /// Set when the file changed but couldn't be applied; the previous
    /// config stays in effect.
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RotateSettings {
    pub interval_ms: u64,
    /// Screen names to rotate through, in order. Never empty.
    pub screens: Vec<String>,
    /// Idle time after user input before rotation resumes.
    pub resume_after_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScreenLayout {
    pub name: String,
    pub tiles: Vec<TilePlacement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TilePlacement {
    pub tile_id: String,
    /// Resolved size: screen config, else module config, else the module's hint.
    pub size: Size,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModuleSummary {
    pub tile_id: String,
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
    /// Write `theme:` into the config file.
    SetTheme { theme: Theme },
    /// Replace `screens:` in the config file (from the screen editor).
    SaveScreens { screens: Vec<ScreenLayout> },
}

/// Messages the host sends to the web view over its WebSocket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Sent first on connect, and again when settings change.
    Settings {
        settings: UiSettings,
    },
    Tile {
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
