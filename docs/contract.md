# Module Contract (v0.1)

This is the contract between the host and a module. Built-in Rust modules implement it as a trait. Stdio plugins implement it over JSON-RPC 2.0. Both produce the same `View` JSON, which is also what the host sends to the TUI and web renderers.

**Writing a plugin:** [plugins.md](plugins.md),
the practical guide with every widget and the protocol as implemented.

**Implemented vs planned.** Built today: poll-mode modules and plugins, every widget below,
host-kept series history, `open_url` actions, the [catalog](#catalog) and plugin packages.
Designed here but not built yet: push mode
(the host rejects push plugins), running declared commands with approval, and the
`run_command` action. The Rust types in `crates/contract` are the source of truth;
`dashboard schema view` prints the current JSON Schema.

**Every widget, by example:** [examples/blocks/](../examples/blocks/) has a view for each widget
type and ways to combine them, checked against this contract by `cargo test -p dashboard-contract`.

## Overview

```text
            ┌────────────── host ──────────────┐
 built-in ──┤ Module trait                     │
 module     │                                  ├── TUI renderer (ratatui)
            │ scheduler → latest View per tile ├── web renderer (axum → Vue)
 stdio   ───┤ JSON-RPC client                  │
 plugin     │ command runner + approvals       │
            └──────────────────────────────────┘
```

- The host asks modules for views, keeps the latest `View` per tile, and pushes it to every attached renderer.
- **Modules never run commands.** They declare commands; the host runs them after confirmation. In v1 a plugin cannot cause anything to execute on its own.

## Versioning

`contract_version` is `MAJOR.MINOR`. The host refuses a plugin with a different major version. Minor versions only add optional fields; both sides ignore fields they don't know.

## Types

Rust is the source of truth. JSON Schema (for plugin authors) is generated with `schemars`; TypeScript types (for the Vue app) with `ts-rs` or `specta`. All JSON fields are `snake_case`.

```rust
// ---- Manifest: what a module is ----

pub struct Manifest {
    pub id: String,                    // "cswap", unique per host
    pub name: String,                  // "Claude accounts"
    pub contract_version: String,      // "0.1"
    pub refresh: Refresh,
    pub layout: LayoutHints,
    pub commands: Vec<CommandDecl>,    // may be empty
    pub config_schema: Option<serde_json::Value>, // JSON Schema for this module's config block
}

#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Refresh {
    Poll {
        interval_ms: u64,              // suggested period; config can override
        min_interval_ms: Option<u64>,  // floor for overrides (e.g. API rate limits)
    },
    Push,                              // module sends view/update when it has something new
}

pub struct LayoutHints {
    pub size: Size,
    pub priority: i32,                 // higher = placed earlier when auto-placing
    pub min_size: Option<Size>,
}

#[serde(rename_all = "snake_case")]
pub enum Size { Small, Medium, Large, Wide, Tall }

// ---- Commands ----

pub struct CommandDecl {
    pub id: String,                    // "switch"
    pub label: String,                 // "Switch to account"
    pub argv: Vec<String>,             // ["cswap", "switch", "{account}"]
    pub cwd: Option<String>,
    pub params: Vec<ParamDecl>,        // placeholders allowed in argv
}

pub struct ParamDecl {
    pub name: String,                  // "account"
    pub pattern: String,               // regex the value must fully match, e.g. "^[0-9]+$"
}

// ---- View: what a module shows ----

pub struct View {
    pub status: Status,                // overall tile status
    pub summary: String,               // one line, used when the tile is too small
    pub root: Widget,
}

#[serde(rename_all = "snake_case")]
pub enum Status { Ok, Warn, Error, Unknown }

#[serde(tag = "type", rename_all = "snake_case")]
pub enum Widget {
    Text   { text: String, #[serde(default)] markdown: bool },
    Stat   { label: String, value: String, unit: Option<String>, delta: Option<String>, status: Option<Status> },
    Gauge  { label: String, percent: f64, marker: Option<f64>, thresholds: Option<Thresholds>, caption: Option<String> },
    Series { label: String, unit: Option<String>, y_max: Option<f64>,
             id: Option<String>, history: Option<u32>,   // host-kept history, see below
             style: SeriesStyle,      // line | area | bars
             color: ColorScale,       // accent | heat | cool | sunset (colored by value)
             mirrored: bool,          // first series above the axis, second below (e.g. net down/up)
             format: ValueFormat,     // plain | bytes (KiB/MiB/GiB) | count (12.3k, 4.2M), then `unit`
             series: Vec<SeriesData> },
    MeterGrid { label: Option<String>, meters: Vec<Meter>, color: ColorScale },   // e.g. per-core CPU
    Bars   { label: Option<String>, unit: Option<String>, format: ValueFormat, max: Option<f64>, bars: Vec<Bar> },
    Pie    { label: Option<String>, slices: Vec<Slice>,                 // shares of a whole, see below
             donut: bool,             // default true; false draws a full disc
             format: ValueFormat, unit: Option<String>,
             color: ColorScale },     // accent: a color per slice; heat | cool | sunset: spread along the scale
    List   { items: Vec<ListItem>, empty_text: Option<String> },
    Table  { columns: Vec<Column>, rows: Vec<Row> },
    Log    { label: Option<String>, lines: Vec<LogLine> },               // newest last
    Uptime { checks: Vec<UptimeCheck> },                                 // status strip per check
    Row    { children: Vec<Widget> },
    Column { children: Vec<Widget> },
}

pub struct Thresholds { pub warn: f64, pub error: f64 }   // percent values

// A color theme token, e.g. "ok" or "series-2". Widgets never carry raw colors.
pub enum Tone { Accent, Ok, Warn, Error, Info, Muted, Series1, .., Series6 }
// as JSON: "accent" | "ok" | "warn" | "error" | "info" | "muted" | "series-1" .. "series-6"

pub struct Slice { pub label: String, pub value: f64, pub status: Option<Status>,   // value >= 0
                   pub tone: Option<Tone> }                                         // color when no status

pub struct SeriesData {
    pub name: String,
    pub points: Vec<(i64, f64)>,       // (unix ms, value)
    pub tone: Option<Tone>,            // this series' color; default: the widget's `color` for
                                       // area and bars, `series-1`, `series-2`, ... for lines
}

pub struct Meter { pub label: String, pub percent: f64, pub tone: Option<Tone> }   // tone: fixed color instead of the scale
pub struct Bar   { pub label: String, pub value: f64, pub status: Option<Status>,
                   pub tone: Option<Tone> }                                         // fill when no status; default accent

pub struct ListItem {
    pub title: String,
    pub subtitle: Option<String>,
    pub status: Option<Status>,
    pub tone: Option<Tone>,            // marker color when there is no status
    pub tags: Vec<String>,             // short labels: "draft", "review requested"
    pub action: Option<Action>,
}

pub struct Column { pub key: String, pub label: String,
                    pub priority: Option<i32> }    // which to drop first when narrow, see below

pub struct Row {
    pub cells: BTreeMap<String, Cell>, // keyed by Column.key
    pub status: Option<Status>,
    pub action: Option<Action>,
}

#[serde(untagged)]
pub enum Cell {
    Text(String),
    Number(f64),
    Widget(Widget),                    // Stat, Gauge and Pie are rendered inline; others fall back to text
}

// ---- Actions ----

#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    OpenUrl    { url: String },                            // http/https only
    RunCommand { command: String, params: BTreeMap<String, String> },
}
```

### Built-in module trait

```rust
#[async_trait]
pub trait Module: Send {
    /// Called once with this module's config block. Returns the manifest
    /// (for plugins, the manifest only exists after `initialize`).
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest>;
    /// Called on each poll tick. Push-mode modules are given a sender instead (see `start`).
    async fn render(&mut self) -> Result<View>;
    /// Push-mode only: keep running and send views when they change.
    async fn start(&mut self, _tx: ViewSender) -> Result<()> { Ok(()) }
}

/// Each view as it changes, or an error. An error shows like a failed poll:
/// the last good view, marked stale (or the error, before the first view).
pub type ViewSender = mpsc::Sender<Result<View>>;
```

### Series history

A module that sets `id` on a `Series` widget sends only **new** samples in each update. The host appends them to a buffer per (tile, `id`, series name), keeps the last `history` points (default 300), drops samples older than the last one kept, and hands renderers the full history. Without `id`, `points` is taken as the complete series.

Renderers currently receive the full series on every update. On localhost this is cheap; sending only the new points to renderers is a possible later optimization and doesn't change what modules send.

### Pie

A `pie` shows shares of a whole. Each slice's share is its value over the sum of all values, and
slices go clockwise from twelve o'clock in the order given. The legend shows each slice's label,
its value (with `format` and `unit`) and its percent.

```json
{"type": "pie", "label": "Disk", "format": "bytes",
 "slices": [
   {"label": "photos", "value": 64424509440},
   {"label": "apps",   "value": 32212254720},
   {"label": "free",   "value": 10737418240, "status": "warn"}
 ]}
```

- `donut` defaults to true (a ring); `false` draws a full disc.
- `color: accent` (the default) gives the slices the theme's `series-1` to `series-6` colors in
  order. `heat`, `cool` and `sunset` spread the slices along that scale in order. A slice's
  `status`, else its `tone`, overrides its color.
- Values must be finite and 0 or more (`dashboard plugin-check` reports others). A pie with no
  slices, or only zeros, shows "no data".
- The web view draws an SVG with the legend beside it, or below it in a narrow tile. The TUI draws
  a braille disc with the legend beside it when there is room, and otherwise one bar split into
  the slices, with the legend packed below. In a table cell the web view shows a small disc and the
  TUI the largest slice as text ("photos 60%").

Pies compose like the other widgets. A donut next to two stats:

```json
{"type": "row", "children": [
  {"type": "pie", "label": "Disk", "format": "bytes", "slices": [
    {"label": "used", "value": 300000000000},
    {"label": "free", "value": 200000000000}]},
  {"type": "column", "children": [
    {"type": "stat", "label": "Used", "value": "279", "unit": "GiB"},
    {"type": "stat", "label": "Free", "value": "186", "unit": "GiB"}]}
]}
```

### When a view doesn't fit

A tile's size comes from its size class and the screen, never from its view. On the 12-column
grid a row unit is 120px in the web view (84px compact) and 5 lines in a 120-column terminal, so
a small tile (3x1) is 120px or 5 lines tall and a tall one (4x4) 528px or 20 lines. A long view
never makes its tile taller or pushes other tiles around. The renderer fits the view to the tile
instead, the same way in both views:

- **Top to bottom, a widget at a time.** A `column` (and a `row` stacked because it is narrow)
  gives its children the room they want in order. The first child that doesn't fit gets what
  is left if it can show something there (below), and the children after it are left out. A
  one-line `text` just before the cut is a heading: it is left out too, and (web) its room goes
  to the section it heads. A list or table that would show only "+N more" is left out.
  Gaps between children go first when room is short (terminal).
- **The child at the cut shows a shorter form.** A `list` shows the items that fit whole and
  "+N more" in the last line (in one terminal line: the first title and "+N"). A `table`,
  `bars`, `uptime`, a pie's legend and a meter grid do the same with their rows. A `series`
  shrinks to its value line and a strip of graph; a `log` shows its newest lines.
- **Text is cut at the edge with "…"**, counting wide characters (CJK, emoji) as two cells:
  list titles and subtitles, table cells, bar and legend labels, log lines.
- **Too narrow for a table:** columns are dropped, lowest `priority` first (default 0; of equal
  ones the rightmost), until the rest fit at their least width (terminal: 12 cells for the
  first column, 6 for others, or less if the column is narrower). The first column is always
  kept. The widest columns then give up room and their cells end with "…".
- **Too small for anything:** the tile shows the view's `summary`. The web view always shows the
  summary next to the tile's name, on one line.
- A `row` side by side gives each child the full height and the same share of the width; it
  stacks (terminal) or wraps (web) when a child would get less than about 24 cells or 140px
  (90px for a stat). In a narrow web list (under 300px) item tags are left out.
- The fullscreen (double-click) view isn't fitted: it shows the whole view and scrolls.

**How a module adapts.** Modules aren't told their tile's size: the same view goes to every
renderer and screen (see "Not in v0.1"). So put what matters most first: the cut is always at the
bottom, the right of a row, and the end of a list. A typical tile leads with one or two stats or a
gauge, then a graph or list, then detail. Give table columns a `priority` so a narrow tile keeps
the useful ones (the kubernetes tile keeps pod, status and ready). Keep the `summary` short: it
is what a tile too small for the view shows. `dashboard plugin-check --width W --height H`
shows the view fitted to a tile of that size.

## Stdio plugin protocol

### Transport

- The host spawns the plugin from its config (`plugin.command`, an argv list).
- **stdin/stdout:** JSON-RPC 2.0, one message per line (newline-delimited JSON, as MCP's stdio transport does). Messages must not contain raw newlines.
- **stderr:** free text, captured into the host log under the module's ID.

### Methods

| Direction | Method | Kind | Purpose |
|---|---|---|---|
| host → plugin | `initialize` | request | Send contract version and the module's config; get the `Manifest` back. |
| host → plugin | `view/render` | request | Poll mode: return the current `View`. |
| host → plugin | `shutdown` | request | Stop work; reply, then wait for `exit`. |
| host → plugin | `exit` | notification | Exit the process. |
| plugin → host | `view/update` | notification | Push mode: here is a new `View`. |
| plugin → host | `log` | notification | `{ level, message }` for the host log. |

No method lets a plugin run a command. Commands run only from a user action in a renderer.

### Lifecycle and failure handling

1. Spawn, in the config file's folder → `initialize`. A failure or invalid manifest shows an error tile.
2. Poll mode: `view/render` every `interval_ms` (or the config's `refresh` override, never below `min_interval_ms`), never overlapping. Every request times out after 30 s.
3. A render error or timeout keeps the last good view, marked stale, with an error status and message.
4. If the process exits, or a request fails or times out, the process is dropped and started again at the next refresh, so the refresh period is the backoff.
5. On host exit or config change the process is killed. (`shutdown` → `exit` is planned.)

### Errors

Standard JSON-RPC error codes, plus:

| Code | Meaning |
|---|---|
| -32001 | Upstream unavailable (e.g. `gh` not logged in, `cswap` not found). The host shows `message` in the tile. |
| -32002 | Config invalid. |

## Commands: execution and approval

When the user triggers `RunCommand { command, params }`:

1. Look up `command` in the module's manifest. If it's unknown, reject.
2. Check that every `{name}` placeholder has a param, every param value fully matches its `pattern`, and there are no extra params.
3. Substitute the values into `argv`. Each value becomes exactly one argument; no shell is involved.
4. Check approvals. The approval key is `module_id + command_id + sha256(argv template + cwd)`. Param values are not part of the key, so "approve always" covers the command for any value that passes its pattern. If the module later changes the template, the key changes and the user is asked again.
5. If not approved: show the label and the fully resolved argv, with options **Run**, **Always allow**, **Cancel**.
6. Run it, stream stdout and stderr to the output panel, and show the exit code. Then trigger an immediate `view/render` for that module so the tile reflects the change.

Approvals are stored at `~/.config/<app>/approvals.yaml` and can be listed and revoked from the CLI.

## Renderer feed (host → TUI / web)

Renderers receive the same `View` JSON, wrapped per tile:

```json
{ "type": "tile", "tile": { "tile_id": "cswap", "view": { ... }, "updated_at": 1793000000000, "stale": false, "demo": false } }
```

`demo` is true when the config sets `demo: true` for the module: the view is its catalog demo
view and the module is not running.

On connect, the web view first gets renderer settings, then a tile message per tile:

```json
{ "type": "settings", "settings": {
    "theme": "auto",
    "color_theme": { "name": "default", "light": { "bg": "#f6f7f9", "accent": "#2f6fde", ... }, "dark": { ... } },
    "color_themes": ["default", "contrast", "paper", "ember"],
    "screens": [{ "name": "main", "tiles": [{ "tile_id": "sysperf", "size": "large" }] }],
    "modules": [{ "tile_id": "sysperf", "name": "System", "refresh_ms": 1000, "refresh_overridden": false, "commands": [] }],
    "rotate": null,
    "config": { "path": "/Users/me/.config/dashboard/config.yaml", "exists": true, "error": null } } }
```

The web view gets these over a WebSocket. Settings are sent again whenever the config is reloaded.

The web view sends actions back as `ClientMessage`s (the TUI sends the same values over a channel):

```json
{ "type": "set_theme", "theme": "dark" }
{ "type": "set_color_theme", "name": "paper" }
```

The host writes `theme:` (or `color_theme:`) into the config file with yaml-splice (only that key's bytes change) and reloads.

`color_theme` holds every token of the theme in use, for both variants; `theme` picks the variant.
The web view turns the tokens into CSS variables of the same names (`--accent`, `--series-1`).
Token names and the built-in themes are in `crates/contract/src/theme.rs` and in the README's Themes
section. Running commands will be added here as `run_command`.

## Example: cswap plugin

The plugin is a small adapter around `cswap list --json`. (Emails below are placeholders.)

**host → plugin**

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"contract_version":"0.1","host":{"name":"dashboard","version":"0.1.0"},"config":{}}}
```

**plugin → host**

```json
{"jsonrpc":"2.0","id":1,"result":{"manifest":{
  "id":"cswap",
  "name":"Claude accounts",
  "contract_version":"0.1",
  "refresh":{"mode":"poll","interval_ms":60000,"min_interval_ms":15000},
  "layout":{"size":"medium","priority":50,"min_size":"small"},
  "commands":[{
    "id":"switch",
    "label":"Switch to account",
    "argv":["cswap","switch","{account}"],
    "params":[{"name":"account","pattern":"^[0-9]+$"}]
  }],
  "config_schema":null
}}}
```

**host → plugin**

```json
{"jsonrpc":"2.0","id":2,"method":"view/render"}
```

**plugin → host**

```json
{"jsonrpc":"2.0","id":2,"result":{
  "status":"warn",
  "summary":"2 accounts · active #1 at 81% (5h)",
  "root":{"type":"table",
    "columns":[
      {"key":"account","label":"Account"},
      {"key":"five_hour","label":"5 hours"},
      {"key":"seven_day","label":"7 days"},
      {"key":"resets","label":"5h resets"}
    ],
    "rows":[
      {"status":"warn",
       "cells":{
         "account":"● 1 work@example.com",
         "five_hour":{"type":"gauge","label":"5h","percent":81,"thresholds":{"warn":75,"error":90}},
         "seven_day":{"type":"gauge","label":"7d","percent":40,"marker":52,"thresholds":{"warn":75,"error":90},"caption":"on pace"},
         "resets":"1h 12m"},
       "action":null},
      {"status":"ok",
       "cells":{
         "account":"2 personal@example.com",
         "five_hour":{"type":"gauge","label":"5h","percent":5},
         "seven_day":{"type":"gauge","label":"7d","percent":22,"marker":52},
         "resets":"4h 40m"},
       "action":{"type":"run_command","command":"switch","params":{"account":"2"}}}
    ]}
}}
```

Mapping from `cswap list --json`: `usage.fiveHour.pct` → 5-hour gauge, `usage.sevenDay.pct` → 7-day gauge, `usage.sevenDay.expectedPct` → `marker`, `aheadOfPace` / `willLastToReset` → caption and row status, `fiveHour.countdown` → resets column. The active account has no switch action.

## Catalog

Everything that can be added to a dashboard has a catalog entry: built-in modules (from the
binary), bundled plugins (cswap), installed plugin packages (from their
`dashboard-plugin.yaml`, see [plugins.md](plugins.md#package-it)), and marketplace packages from
the cached index that aren't installed yet (`source: "marketplace"`). `dashboard widgets` lists
and searches them; `dashboard schema catalog-entry` prints the JSON Schema.

```rust
pub struct CatalogEntry {
    pub id: String,              // what the config names: modules.<id>, or package: <id>
    pub name: String,
    pub version: String,
    pub description: String,
    pub tags: Vec<String>,
    pub kind: EntryKind,         // builtin | plugin
    pub source: String,          // "builtin", "bundled", the package's folder, or "marketplace"
    pub default_size: Size,
    pub config_schema: Option<serde_json::Value>,  // JSON Schema for the config: block
    pub requirements: Requirements,
    pub demo: View,              // made-up data; needs no account
}

pub struct Requirements {
    pub binaries: Vec<String>,   // e.g. "gh", "node>=22"
    pub env: Vec<String>,        // e.g. "JIRA_API_TOKEN"
    pub hosts: Vec<String>,      // network hosts it calls
}
```

The demo view is what the editor preview, the site's screenshots and the tests show. Built
from a sample response in the API's documented shape, it goes through the same parse code as
live data. Modules built only from API docs, with no live test yet, carry the tag
`untested-live` and say so in their description.

Search (`catalog::search`) keeps an entry when every word of the query appears in its ID, name,
description or tags, ignoring case, and when it has the `--tag` given, if any.

The web view gets the catalog from `GET /api/catalog?q=<words>&tag=<tag>` (with the session
token), as a `CatalogResponse`: the matching `entries`, every `tags` value in the whole catalog,
and `errors` from reading packages or the index. Adding one sends `add_module` over the
WebSocket:

```json
{"type": "add_module", "module_id": "weather", "entry": "weather", "kind": "builtin",
 "config": {"place": "Oslo"}, "demo": false, "screen": "main", "size": "medium"}
```

The host writes `modules.<module_id>` (with `builtin:` when `module_id` differs from a
built-in's ID, `package:` for a package) and a tile at the end of `screen`, editing the file in
place so comments stay. The reload that follows starts the module. The next settings carry
`config.added`: the `module_id`, if the add was refused an `error` (the file is then left
as it was), and `seq`, which counts the adds the server has handled. Every later settings
message carries the same `added`, so a renderer waits for a `seq` higher than the one it saw
when it sent the add. A module that was added but fails to start shows as the config error, like any
other.

Marketplace index entries get the same `check_view` as installed packages. An entry whose demo
fails it (say, a link that isn't http or https) is left out of the catalog and listed in
`errors`.

A built-in module adds its entry with a `catalog()` function next to the module, named in the
`Builtin` entry of its bundle's `MODULES` list (see docs/modules.md). A test checks every built-in has one, with a description, tags
and a demo view that passes `check_view`.

## Not in v0.1

- WASM plugins. The same types will be mirrored in a WIT file once the contract is stable.
- Plugin-defined events or callbacks (clicks sent to the plugin).
- Data passed between modules.
- Per-size rendering (a different view for each size class). `view/render` has no size: a
  module is fitted to its tile by the renderer, as above.
