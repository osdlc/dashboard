/* Generated from the Rust contract by pnpm gen:types. Do not edit. */

/**
 * A tile's size class on the 12-column grid.
 */
export type Size = "small" | "medium" | "large" | "wide" | "tall";
/**
 * A piece of a view. `type` names the kind: `text`, `stat`, `gauge`, `table` and so on.
 */
export type Widget =
  | {
      /**
       * Read markdown in `text`: `**bold**`, and code between backticks.
       */
      markdown?: boolean;
      /**
       * The text to show.
       */
      text: string;
      type: "text";
    }
  | {
      /**
       * A change to show beside the value, e.g. "+2.8%".
       */
      delta?: string | null;
      /**
       * Shown above or before the value.
       */
      label: string;
      /**
       * Colors the value: ok, warn or error.
       */
      status?: Status | null;
      type: "stat";
      /**
       * Shown after the value, smaller, e.g. "ms" or "/s".
       */
      unit?: string | null;
      /**
       * Already formatted, e.g. "3451" or "1.2k".
       */
      value: string;
    }
  | {
      /**
       * A line under the meter, e.g. "1721 of 1858 GiB used".
       */
      caption?: string | null;
      /**
       * Shown with the meter.
       */
      label: string;
      /**
       * A tick on the track at this percent, e.g. where usage should be by now.
       */
      marker?: number | null;
      /**
       * How full, 0-100.
       */
      percent: number;
      /**
       * Percents at which the fill turns warn and error colored.
       */
      thresholds?: Thresholds | null;
      type: "gauge";
    }
  | {
      /**
       * The color of a single series.
       */
      color?: ColorScale & string;
      /**
       * Scale to the data's own range with a little padding (for prices that
       * sit far from 0). Ignored when `y_min` or `y_max` is set, and when
       * `mirrored`.
       */
      fit?: boolean;
      /**
       * How values are written: plain, bytes or count.
       */
      format?: ValueFormat & string;
      /**
       * Points the host keeps per series when `id` is set. Default 300.
       */
      history?: number | null;
      /**
       * When set, the host keeps history for this widget: each update's
       * `points` are new samples, appended to what the host already holds
       * for (tile, `id`, series name). Renderers always get the full history.
       */
      id?: string | null;
      /**
       * The chart's title.
       */
      label: string;
      /**
       * Draw the first series above the axis and the second below it,
       * e.g. network download and upload. Negative values count as 0 here
       * (`sitroom check` flags them).
       */
      mirrored?: boolean;
      /**
       * One or more lines, each with its points.
       */
      series: SeriesData[];
      /**
       * A line, a filled area or one bar per sample.
       */
      style?: SeriesStyle & string;
      type: "series";
      /**
       * Shown after values, e.g. "%" or "/s".
       */
      unit?: string | null;
      /**
       * The top of the chart. Default: the largest value (at least 1 when no
       * value is negative and `y_min` is unset).
       */
      y_max?: number | null;
      /**
       * The bottom of the chart. Default: 0, or the smallest value when that
       * is negative. Set it to chart values that never reach 0.
       */
      y_min?: number | null;
    }
  | {
      /**
       * The meters' color: `accent`, or a scale colored by each meter's value.
       */
      color?: ColorScale & string;
      /**
       * A title above the grid.
       */
      label?: string | null;
      /**
       * The meters, in order.
       */
      meters: Meter[];
      type: "meter_grid";
    }
  | {
      /**
       * The bars, top to bottom.
       */
      bars: Bar[];
      /**
       * How values are written: plain, bytes or count.
       */
      format?: ValueFormat & string;
      /**
       * A title above the bars.
       */
      label?: string | null;
      /**
       * Value of a full bar. Default: the largest value.
       */
      max?: number | null;
      type: "bars";
      /**
       * Shown after each value.
       */
      unit?: string | null;
    }
  | {
      /**
       * `accent` (the default) gives each slice its own color, the theme's
       * accent first. The other scales spread the slices along the scale
       * in order. A slice's `status` overrides its color.
       */
      color?: ColorScale & string;
      /**
       * A ring with a hole in the middle. Default true; false draws a full disc.
       */
      donut?: boolean;
      /**
       * How values are written in the legend: plain, bytes or count.
       */
      format?: ValueFormat & string;
      /**
       * A title above the chart.
       */
      label?: string | null;
      /**
       * The slices, in order. Values are zero or more.
       */
      slices: Slice[];
      type: "pie";
      /**
       * Shown after each value in the legend.
       */
      unit?: string | null;
    }
  | {
      /**
       * Shown below the face, e.g. a date.
       */
      caption?: string | null;
      /**
       * 0-23.
       */
      hour: number;
      /**
       * Shown above the face, e.g. "Tokyo".
       */
      label?: string | null;
      /**
       * 0-59.
       */
      minute: number;
      /**
       * 0-59. No second hand when absent.
       */
      second?: number | null;
      /**
       * Colors the face's outline.
       */
      status?: Status | null;
      type: "clock_face";
    }
  | {
      /**
       * Shown below the text.
       */
      caption?: string | null;
      /**
       * Shown above the text.
       */
      label?: string | null;
      /**
       * Colors the text.
       */
      status?: Status | null;
      /**
       * Already formatted, e.g. "14:05". The TUI draws it in a block font
       * when it has a glyph for every character, else as plain text; see
       * docs/contract.md, "Clock face and display".
       */
      text: string;
      type: "display";
      /**
       * Shown after the text, smaller, e.g. "PM".
       */
      unit?: string | null;
    }
  | {
      /**
       * Shown when there are no items.
       */
      empty_text?: string | null;
      /**
       * The items, top to bottom.
       */
      items: ListItem[];
      type: "list";
    }
  | {
      /**
       * The columns, left to right.
       */
      columns: Column[];
      /**
       * The rows, top to bottom.
       */
      rows: Row[];
      type: "table";
    }
  | {
      /**
       * A title above the lines.
       */
      label?: string | null;
      /**
       * The lines, oldest first.
       */
      lines: LogLine[];
      type: "log";
    }
  | {
      /**
       * The checks, one row each.
       */
      checks: UptimeCheck[];
      type: "uptime";
    }
  | {
      /**
       * A line under the picture, e.g. the file name.
       */
      caption?: string | null;
      /**
       * Image file: PNG, JPEG, GIF (first frame), WebP or BMP.
       */
      path: string;
      type: "image";
    }
  | {
      /**
       * The widgets, left to right.
       */
      children: Widget[];
      type: "row";
    }
  | {
      /**
       * The widgets, top to bottom.
       */
      children: Widget[];
      type: "column";
    };
/**
 * Health of a tile, row or item: colors it ok, warn or error.
 */
export type Status = "ok" | "warn" | "error" | "unknown";
/**
 * A color, or a scale that colors by value.
 */
export type ColorScale = "accent" | "heat" | "cool" | "sunset";
/**
 * How numbers are written.
 */
export type ValueFormat = "plain" | "bytes" | "count";
/**
 * A token a widget can color something with, e.g. one line of a chart.
 */
export type Tone =
  | "accent"
  | "ok"
  | "warn"
  | "error"
  | "info"
  | "muted"
  | "series-1"
  | "series-2"
  | "series-3"
  | "series-4"
  | "series-5"
  | "series-6";
/**
 * How a `series` chart draws its values.
 */
export type SeriesStyle = "line" | "area" | "bars";
/**
 * What clicking a row or item does.
 */
export type Action =
  | {
      type: "open_url";
      /**
       * The link: http or https only.
       */
      url: string;
    }
  | {
      /**
       * The command's `id` from the manifest.
       */
      command: string;
      /**
       * Values for the command's `{name}` placeholders. Each must match its pattern.
       */
      params?: {
        [k: string]: string;
      };
      type: "run_command";
    };
/**
 * A table cell: text, a number or a small widget.
 */
export type Cell = string | number | Widget;
/**
 * How serious a log line is.
 */
export type LogLevel = "error" | "warn" | "info" | "debug";
/**
 * Whether an entry is compiled in or a separate program.
 */
export type EntryKind = "builtin" | "plugin";
/**
 * Actions from a renderer: the web view sends these over its WebSocket, the
 * TUI over a channel. The host carries them out.
 */
export type ClientMessage =
  | {
      /**
       * `config.etag` from the settings the change was made on. `None` when there was no config file; refused if one exists now.
       */
      etag?: string | null;
      /**
       * Light, dark or auto.
       */
      theme: Theme;
      type: "set_theme";
    }
  | {
      /**
       * As for `SetTheme`.
       */
      etag?: string | null;
      /**
       * The color theme's name.
       */
      name: string;
      type: "set_color_theme";
    }
  | {
      /**
       * As for `SetTheme`.
       */
      etag?: string | null;
      /**
       * The new screens, in order.
       */
      screens: ScreenLayout[];
      type: "save_screens";
    }
  | {
      /**
       * The module's `config:` block. Null or `{}` writes none.
       */
      config?: {
        [k: string]: unknown;
      };
      /**
       * Show the demo view instead of running it.
       */
      demo?: boolean;
      /**
       * The catalog entry's ID and kind.
       */
      entry: string;
      /**
       * Whether `entry` is a built-in or a plugin.
       */
      kind: EntryKind;
      /**
       * The new module's ID in the config (its key under `modules:`),
       * usually the catalog ID.
       */
      module_id: string;
      /**
       * The screen to put a tile on. Ignored when the config has no `screens:`.
       */
      screen?: string | null;
      /**
       * The new tile's size.
       */
      size: Size;
      type: "add_module";
    }
  | {
      /**
       * The saved dashboard's name. `None`: `config.yaml` alone.
       */
      name?: string | null;
      type: "switch_dashboard";
    }
  | {
      /**
       * Copy sitroom in use. `false`: one empty screen.
       */
      copy?: boolean;
      /**
       * The new dashboard's name, before it is made into a slug.
       */
      name: string;
      type: "create_dashboard";
    };
/**
 * Light or dark, for the whole sitroom.
 */
export type Theme = "auto" | "light" | "dark";
/**
 * Messages the host sends to the web view over its WebSocket.
 */
export type ServerMessage =
  | {
      /**
       * Everything renderers need besides tiles.
       */
      settings: UiSettings;
      type: "settings";
    }
  | {
      /**
       * The update.
       */
      tile: TileUpdate;
      type: "tile";
    };

/**
 * Root type so both message enums end up in one schema file.
 */
export interface WebProtocol {
  catalog: CatalogResponse;
  client: ClientMessage;
  server: ServerMessage;
}
/**
 * What `GET /api/catalog` returns to the web editor.
 */
export interface CatalogResponse {
  /**
   * Entries matching the request's `q` and `tag`.
   */
  entries: CatalogEntry[];
  /**
   * Problems reading packages or the marketplace index, to show.
   */
  errors?: string[];
  /**
   * Every tag in the whole catalog, sorted, for the filter buttons.
   */
  tags: string[];
}
/**
 * One thing the catalog lists.
 */
export interface CatalogEntry {
  /**
   * JSON Schema for the module's `config:` block.
   */
  config_schema?: {
    [k: string]: unknown;
  };
  /**
   * The size it suggests for its tile.
   */
  default_size: Size;
  /**
   * Made-up but realistic data, shown in previews. Needs no account.
   */
  demo: View;
  /**
   * One line about what it shows.
   */
  description: string;
  /**
   * The module ID to put in a config: under `modules:` for a built-in, or
   * `package:` for a plugin package.
   */
  id: string;
  /**
   * Built in or plugin.
   */
  kind: EntryKind;
  /**
   * Display name.
   */
  name: string;
  /**
   * What it needs to work: programs, environment variables, hosts.
   */
  requirements?: Requirements;
  /**
   * Where it comes from: `builtin`, `bundled` (a plugin inside the
   * sitroom binary), or the folder of an installed package.
   */
  source: string;
  /**
   * Words to search and filter by, e.g. `github`.
   */
  tags?: string[];
  /**
   * The entry's version.
   */
  version: string;
}
/**
 * What a tile shows: an overall status, a one-line summary and a widget tree.
 */
export interface View {
  /**
   * The widget the tile shows; use `row` and `column` to combine several.
   */
  root: Widget;
  /**
   * The tile's overall health, shown as its status color.
   */
  status: Status;
  /**
   * One line, used when the tile is too small.
   */
  summary: string;
}
/**
 * Percents at which a gauge's fill changes color.
 */
export interface Thresholds {
  /**
   * The percent at which the fill turns the error color.
   */
  error: number;
  /**
   * The percent at which the fill turns the warn color.
   */
  warn: number;
}
/**
 * One line of a `series` chart.
 */
export interface SeriesData {
  /**
   * Shown in the legend; also keys the history when the widget has an `id`.
   */
  name: string;
  /**
   * (unix ms, value)
   */
  points: [unknown, unknown][];
  /**
   * This series' color, instead of the widget's `color` (one series) or
   * the next `series-N` token (several).
   */
  tone?: Tone | null;
}
/**
 * One small meter in a `meter_grid`.
 */
export interface Meter {
  /**
   * Shown beside the meter, e.g. a core number.
   */
  label: string;
  /**
   * 0-100.
   */
  percent: number;
  /**
   * A fixed color for this meter instead of the grid's color scale.
   */
  tone?: Tone | null;
}
/**
 * One bar in a `bars` widget.
 */
export interface Bar {
  /**
   * Shown beside the bar.
   */
  label: string;
  /**
   * Colors the bar ok, warn or error.
   */
  status?: Status | null;
  /**
   * Fill color when `status` is not set. Default: `accent`.
   */
  tone?: Tone | null;
  /**
   * Zero or more; the bar's length is this over the widget's `max`.
   */
  value: number;
}
/**
 * One slice of a `pie`.
 */
export interface Slice {
  /**
   * Shown in the legend.
   */
  label: string;
  /**
   * Colors the slice ok, warn or error.
   */
  status?: Status | null;
  /**
   * Color when `status` is not set. Default: the next `series-N` token.
   */
  tone?: Tone | null;
  /**
   * Zero or more. Shown with the widget's `format` and `unit`.
   */
  value: number;
}
/**
 * One item in a `list` widget.
 */
export interface ListItem {
  /**
   * What clicking the item does, e.g. open a link.
   */
  action?: Action | null;
  /**
   * Colors the item's marker ok, warn or error.
   */
  status?: Status | null;
  /**
   * A second, quieter line.
   */
  subtitle?: string | null;
  /**
   * Short labels shown as chips.
   */
  tags?: string[];
  /**
   * The main line.
   */
  title: string;
  /**
   * Color of the item's marker when `status` is not set.
   */
  tone?: Tone | null;
}
/**
 * A table column.
 */
export interface Column {
  /**
   * Names the cell in each row's `cells`.
   */
  key: string;
  /**
   * The column heading.
   */
  label: string;
  /**
   * Which columns to keep when a table is too narrow for all of them:
   * renderers drop the lowest priority first (the rightmost of equal
   * ones) and always keep the first column. Default 0.
   */
  priority?: number | null;
}
/**
 * A table row.
 */
export interface Row {
  /**
   * What clicking the row does, e.g. open a link.
   */
  action?: Action | null;
  /**
   * Keyed by the column's `key`.
   */
  cells: {
    [k: string]: Cell;
  };
  /**
   * Colors the row's marker ok, warn or error.
   */
  status?: Status | null;
}
/**
 * One line in a `log` widget.
 */
export interface LogLine {
  /**
   * error, warn, info or debug: colors the line.
   */
  level?: LogLevel | null;
  /**
   * The log message.
   */
  text: string;
  /**
   * Shown before the text, e.g. "14:02:11".
   */
  time?: string | null;
}
/**
 * One availability check in an `uptime` widget.
 */
export interface UptimeCheck {
  /**
   * e.g. "HTTP 200" or the error.
   */
  detail?: string | null;
  /**
   * Recent results, oldest first. Renderers show the newest that fit.
   */
  history?: Status[];
  /**
   * Response time of the latest check.
   */
  latency_ms?: number | null;
  /**
   * What is checked, e.g. a host or URL.
   */
  name: string;
  /**
   * Result of the latest check.
   */
  status: Status;
  /**
   * Share of recent checks that passed, 0-100.
   */
  uptime?: number | null;
  /**
   * Opened when the check is clicked.
   */
  url?: string | null;
}
/**
 * What a module needs to work. Shown before adding or installing it.
 */
export interface Requirements {
  /**
   * Programs on PATH, with an optional minimum version, e.g. `gh`, `node>=22`.
   */
  binaries?: string[];
  /**
   * Environment variables it reads, e.g. API tokens.
   */
  env?: string[];
  /**
   * Network hosts it calls.
   */
  hosts?: string[];
}
/**
 * A named screen and the tiles on it.
 */
export interface ScreenLayout {
  /**
   * The screen's name, shown in the switcher.
   */
  name: string;
  /**
   * Tiles in placement order.
   */
  tiles: TilePlacement[];
}
/**
 * Where a tile goes on a screen.
 */
export interface TilePlacement {
  /**
   * Resolved size: screen config, else module config, else the module's hint.
   */
  size: Size;
  /**
   * The tile's module ID.
   */
  tile_id: string;
}
/**
 * Renderer-wide settings derived from the config.
 */
export interface UiSettings {
  /**
   * The color theme in use, both variants; `theme` picks one.
   */
  color_theme?: ColorTheme;
  /**
   * Color themes to choose from: built-ins, then the config's own.
   */
  color_themes?: string[];
  /**
   * Where the config came from, for the settings screen.
   */
  config: ConfigInfo;
  /**
   * Running modules, for the help screen.
   */
  modules: ModuleSummary[];
  /**
   * False: the native app shows new values without motion.
   */
  native_animations?: boolean;
  /**
   * The most frames a second the native app draws. The web view does not
   * use it.
   */
  native_fps?: number;
  /**
   * Automatic screen rotation, if configured.
   */
  rotate?: RotateSettings | null;
  /**
   * At least one screen.
   */
  screens: ScreenLayout[];
  /**
   * Light, dark or auto: picks the variant of `color_theme`.
   */
  theme: Theme;
  /**
   * The most times a second the terminal view draws the screen again when
   * tiles change. The web view does not use it.
   */
  tui_fps?: number;
}
/**
 * A color theme with both variants, as sent to renderers.
 */
export interface ColorTheme {
  /**
   * Colors for a dark background.
   */
  dark: Tokens;
  /**
   * Colors for a light background.
   */
  light: Tokens;
  /**
   * The theme's name, e.g. "default".
   */
  name: string;
}
/**
 * Every color a renderer uses, as `#rrggbb`.
 */
export interface Tokens {
  /**
   * Highlights: the default fill for meters, bars and charts.
   */
  accent: string;
  /**
   * Background of the item picked with the keyboard.
   */
  "accent-soft": string;
  /**
   * Page background.
   */
  bg: string;
  /**
   * Tile and table borders.
   */
  border: string;
  /**
   * High stop of the `cool` scale.
   */
  "cool-high": string;
  /**
   * Low stop of the `cool` scale.
   */
  "cool-low": string;
  /**
   * Middle stop of the `cool` scale.
   */
  "cool-mid": string;
  /**
   * Failing status.
   */
  error: string;
  /**
   * High stop of the `heat` scale.
   */
  "heat-high": string;
  /**
   * Stops of the `heat` color scale: low, middle, high.
   */
  "heat-low": string;
  /**
   * Middle stop of the `heat` scale.
   */
  "heat-mid": string;
  /**
   * Neutral information.
   */
  info: string;
  /**
   * Labels, captions and other quiet text.
   */
  muted: string;
  /**
   * Healthy status.
   */
  ok: string;
  /**
   * Text on an `accent` background.
   */
  "on-accent": string;
  /**
   * Categorical colors for series, bars and slices, in order.
   */
  "series-1": string;
  /**
   * Categorical color 2.
   */
  "series-2": string;
  /**
   * Categorical color 3.
   */
  "series-3": string;
  /**
   * Categorical color 4.
   */
  "series-4": string;
  /**
   * Categorical color 5.
   */
  "series-5": string;
  /**
   * Categorical color 6.
   */
  "series-6": string;
  /**
   * High stop of the `sunset` scale.
   */
  "sunset-high": string;
  /**
   * Low stop of the `sunset` scale.
   */
  "sunset-low": string;
  /**
   * Middle stop of the `sunset` scale.
   */
  "sunset-mid": string;
  /**
   * Tiles and panels.
   */
  surface: string;
  /**
   * Hovered rows and buttons; inline code.
   */
  "surface-2": string;
  /**
   * Body text.
   */
  text: string;
  /**
   * The empty part of gauges, meters and bars.
   */
  track: string;
  /**
   * Needs-attention status.
   */
  warn: string;
}
/**
 * Where the config came from, for the settings screen.
 */
export interface ConfigInfo {
  /**
   * How the last `add_module` went, so the editor that sent it can tell.
   */
  added?: AddOutcome | null;
  /**
   * How the last `create_dashboard` went.
   */
  created?: CreateOutcome | null;
  /**
   * The saved dashboard in use, by name. `None`: `config.yaml` alone.
   */
  dashboard?: string | null;
  /**
   * Saved dashboards to switch to.
   */
  dashboards?: DashboardSummary[];
  /**
   * Set when the file changed but couldn't be applied; the previous
   * config stays in effect.
   */
  error?: string | null;
  /**
   * The version of the file at `path`, which changes are written to (a sha256 etag),
   * `None` when there is no file. Send it back with a change: a change
   * based on an older version is refused.
   */
  etag?: string | null;
  /**
   * False when no file exists and defaults are in use.
   */
  exists: boolean;
  /**
   * The files merged into the config, lowest precedence first, e.g.
   * "machine: ~/.config/sitroom/config.yaml".
   */
  layers?: string[];
  /**
   * The config file's path.
   */
  path: string;
}
/**
 * The result of an `add_module`. With no error the config was written; the
 * module may still fail to start, which shows as the config error.
 */
export interface AddOutcome {
  /**
   * Why the add failed. `None`: the config was written.
   */
  error?: string | null;
  /**
   * The ID the module was added under.
   */
  module_id: string;
  /**
   * Counts the adds the server has handled, so a renderer can tell the
   * outcome of its add from an earlier one, which every later settings
   * message still carries.
   */
  seq?: number;
}
/**
 * The result of a `create_dashboard`. With no error the file was written and
 * the switch to it tried; a failed switch shows as the config error.
 */
export interface CreateOutcome {
  /**
   * Why the create failed. `None`: the file was written.
   */
  error?: string | null;
  /**
   * The name the request asked for.
   */
  name: string;
  /**
   * Counts the requests the server has handled, as in `AddOutcome`.
   */
  seq?: number;
}
/**
 * A saved dashboard, for the switcher.
 */
export interface DashboardSummary {
  /**
   * A line about what it shows.
   */
  description?: string | null;
  /**
   * Sitroom's name.
   */
  name: string;
}
/**
 * A running module, for the help screen.
 */
export interface ModuleSummary {
  /**
   * Labels of declared commands.
   */
  commands?: string[];
  /**
   * The size the module suggests, used when a screen doesn't set one.
   */
  default_size: Size;
  /**
   * The module's display name.
   */
  name: string;
  /**
   * Effective poll period; `None` for push modules.
   */
  refresh_ms?: number | null;
  /**
   * True when `refresh_ms` comes from the config rather than the module.
   */
  refresh_overridden?: boolean;
  /**
   * The module ID in the config.
   */
  tile_id: string;
}
/**
 * Automatic screen rotation.
 */
export interface RotateSettings {
  /**
   * Time on each screen, in ms.
   */
  interval_ms: number;
  /**
   * Idle time after user input before rotation resumes.
   */
  resume_after_ms: number;
  /**
   * Screen names to rotate through, in order. Never empty.
   */
  screens: string[];
}
/**
 * What the host sends to renderers (TUI, web) for each tile.
 */
export interface TileUpdate {
  /**
   * True when the config asks for the module's demo view (`demo: true`):
   * made-up data, not the module's real output.
   */
  demo?: boolean;
  /**
   * True when the last refresh failed and `view` is the last good one.
   */
  stale: boolean;
  /**
   * The tile this update is for.
   */
  tile_id: string;
  /**
   * Unix ms.
   */
  updated_at: number;
  /**
   * What the tile shows.
   */
  view: View;
}
