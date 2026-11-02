/* Generated from the Rust contract by pnpm gen:types. Do not edit. */

export type Size = "small" | "medium" | "large" | "wide" | "tall";
export type Widget =
  | {
      /**
       * Read markdown in `text`: `**bold**`, and code between backticks.
       */
      markdown?: boolean;
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
      color?: "accent" | "heat" | "cool" | "sunset";
      /**
       * How values are written: plain, bytes or count.
       */
      format?: "plain" | "bytes" | "count";
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
      label: string;
      /**
       * Draw the first series above the axis and the second below it,
       * e.g. network download and upload.
       */
      mirrored?: boolean;
      /**
       * One or more lines, each with its points.
       */
      series: SeriesData[];
      /**
       * A line, a filled area or one bar per sample.
       */
      style?: "line" | "area" | "bars";
      type: "series";
      /**
       * Shown after values, e.g. "%" or "/s".
       */
      unit?: string | null;
      /**
       * The top of the chart. Default: the largest value.
       */
      y_max?: number | null;
    }
  | {
      /**
       * The meters' color: `accent`, or a scale colored by each meter's value.
       */
      color?: "accent" | "heat" | "cool" | "sunset";
      label?: string | null;
      meters: Meter[];
      type: "meter_grid";
    }
  | {
      bars: Bar[];
      /**
       * How values are written: plain, bytes or count.
       */
      format?: "plain" | "bytes" | "count";
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
      color?: "accent" | "heat" | "cool" | "sunset";
      /**
       * A ring with a hole in the middle. Default true; false draws a full disc.
       */
      donut?: boolean;
      /**
       * How values are written in the legend: plain, bytes or count.
       */
      format?: "plain" | "bytes" | "count";
      label?: string | null;
      slices: Slice[];
      type: "pie";
      /**
       * Shown after each value in the legend.
       */
      unit?: string | null;
    }
  | {
      /**
       * Shown when there are no items.
       */
      empty_text?: string | null;
      items: ListItem[];
      type: "list";
    }
  | {
      columns: Column[];
      rows: Row[];
      type: "table";
    }
  | {
      label?: string | null;
      lines: LogLine[];
      type: "log";
    }
  | {
      checks: UptimeCheck[];
      type: "uptime";
    }
  | {
      children: Widget[];
      type: "row";
    }
  | {
      children: Widget[];
      type: "column";
    };
export type Status = "ok" | "warn" | "error" | "unknown";
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
export type Action =
  | {
      type: "open_url";
      url: string;
    }
  | {
      command: string;
      params?: {
        [k: string]: string;
      };
      type: "run_command";
    };
export type Cell =
  | string
  | number
  | (
      | {
          /**
           * Read markdown in `text`: `**bold**`, and code between backticks.
           */
          markdown?: boolean;
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
          color?: "accent" | "heat" | "cool" | "sunset";
          /**
           * How values are written: plain, bytes or count.
           */
          format?: "plain" | "bytes" | "count";
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
          label: string;
          /**
           * Draw the first series above the axis and the second below it,
           * e.g. network download and upload.
           */
          mirrored?: boolean;
          /**
           * One or more lines, each with its points.
           */
          series: SeriesData[];
          /**
           * A line, a filled area or one bar per sample.
           */
          style?: "line" | "area" | "bars";
          type: "series";
          /**
           * Shown after values, e.g. "%" or "/s".
           */
          unit?: string | null;
          /**
           * The top of the chart. Default: the largest value.
           */
          y_max?: number | null;
        }
      | {
          /**
           * The meters' color: `accent`, or a scale colored by each meter's value.
           */
          color?: "accent" | "heat" | "cool" | "sunset";
          label?: string | null;
          meters: Meter[];
          type: "meter_grid";
        }
      | {
          bars: Bar[];
          /**
           * How values are written: plain, bytes or count.
           */
          format?: "plain" | "bytes" | "count";
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
          color?: "accent" | "heat" | "cool" | "sunset";
          /**
           * A ring with a hole in the middle. Default true; false draws a full disc.
           */
          donut?: boolean;
          /**
           * How values are written in the legend: plain, bytes or count.
           */
          format?: "plain" | "bytes" | "count";
          label?: string | null;
          slices: Slice[];
          type: "pie";
          /**
           * Shown after each value in the legend.
           */
          unit?: string | null;
        }
      | {
          /**
           * Shown when there are no items.
           */
          empty_text?: string | null;
          items: ListItem[];
          type: "list";
        }
      | {
          columns: Column[];
          rows: Row[];
          type: "table";
        }
      | {
          label?: string | null;
          lines: LogLine[];
          type: "log";
        }
      | {
          checks: UptimeCheck[];
          type: "uptime";
        }
      | {
          children: Widget[];
          type: "row";
        }
      | {
          children: Widget[];
          type: "column";
        }
    );
export type LogLevel = "error" | "warn" | "info" | "debug";
export type EntryKind = "builtin" | "plugin";
/**
 * Actions from a renderer: the web view sends these over its WebSocket, the
 * TUI over a channel. The host carries them out.
 */
export type ClientMessage =
  | {
      etag?: string | null;
      theme: Theme;
      type: "set_theme";
    }
  | {
      etag?: string | null;
      name: string;
      type: "set_color_theme";
    }
  | {
      etag?: string | null;
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
      size: Size;
      type: "add_module";
    }
  | {
      name?: string | null;
      type: "switch_dashboard";
    };
export type Theme = ("light" | "dark") | "auto";
/**
 * Messages the host sends to the web view over its WebSocket.
 */
export type ServerMessage =
  | {
      settings: UiSettings;
      type: "settings";
    }
  | {
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
  default_size: Size;
  demo: View;
  description: string;
  /**
   * The module ID to put in a config: under `modules:` for a built-in, or
   * `package:` for a plugin package.
   */
  id: string;
  kind: EntryKind;
  name: string;
  requirements?: Requirements;
  /**
   * Where it comes from: `builtin`, `bundled` (a plugin inside the
   * dashboard binary), or the folder of an installed package.
   */
  source: string;
  tags?: string[];
  version: string;
}
/**
 * Made-up but realistic data, shown in previews. Needs no account.
 */
export interface View {
  root: Widget;
  status: Status;
  /**
   * One line, used when the tile is too small.
   */
  summary: string;
}
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
export interface Meter {
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
export interface Bar {
  label: string;
  /**
   * Colors the bar ok, warn or error.
   */
  status?: Status | null;
  /**
   * Fill color when `status` is not set. Default: `accent`.
   */
  tone?: Tone | null;
  value: number;
}
export interface Slice {
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
  title: string;
  /**
   * Color of the item's marker when `status` is not set.
   */
  tone?: Tone | null;
}
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
export interface LogLine {
  /**
   * error, warn, info or debug: colors the line.
   */
  level?: LogLevel | null;
  text: string;
  /**
   * Shown before the text, e.g. "14:02:11".
   */
  time?: string | null;
}
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
  name: string;
  /**
   * Result of the latest check.
   */
  status: "ok" | "warn" | "error" | "unknown";
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
export interface ScreenLayout {
  name: string;
  tiles: TilePlacement[];
}
export interface TilePlacement {
  /**
   * Resolved size: screen config, else module config, else the module's hint.
   */
  size: "small" | "medium" | "large" | "wide" | "tall";
  tile_id: string;
}
/**
 * Renderer-wide settings derived from the config.
 */
export interface UiSettings {
  color_theme?: ColorTheme;
  /**
   * Color themes to choose from: built-ins, then the config's own.
   */
  color_themes?: string[];
  config: ConfigInfo;
  /**
   * Running modules, for the help screen.
   */
  modules: ModuleSummary[];
  /**
   * Automatic screen rotation, if configured.
   */
  rotate?: RotateSettings | null;
  /**
   * At least one screen.
   */
  screens: ScreenLayout[];
  theme: Theme;
}
/**
 * The color theme in use, both variants; `theme` picks one.
 */
export interface ColorTheme {
  dark: Tokens;
  light: Tokens;
  name: string;
}
/**
 * Every color a renderer uses, as `#rrggbb`.
 */
export interface Tokens {
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
  "cool-high": string;
  "cool-low": string;
  "cool-mid": string;
  error: string;
  "heat-high": string;
  /**
   * Stops of the `heat` color scale: low, middle, high.
   */
  "heat-low": string;
  "heat-mid": string;
  info: string;
  /**
   * Labels, captions and other quiet text.
   */
  muted: string;
  ok: string;
  /**
   * Text on an `accent` background.
   */
  "on-accent": string;
  /**
   * Categorical colors for series, bars and slices, in order.
   */
  "series-1": string;
  "series-2": string;
  "series-3": string;
  "series-4": string;
  "series-5": string;
  "series-6": string;
  "sunset-high": string;
  "sunset-low": string;
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
   * "machine: ~/.config/dashboard/config.yaml".
   */
  layers?: string[];
  path: string;
}
/**
 * The result of an `add_module`. With no error the config was written; the
 * module may still fail to start, which shows as the config error.
 */
export interface AddOutcome {
  error?: string | null;
  module_id: string;
  /**
   * Counts the adds the server has handled, so a renderer can tell the
   * outcome of its add from an earlier one, which every later settings
   * message still carries.
   */
  seq?: number;
}
/**
 * A saved dashboard, for the switcher.
 */
export interface DashboardSummary {
  description?: string | null;
  name: string;
}
export interface ModuleSummary {
  /**
   * Labels of declared commands.
   */
  commands?: string[];
  /**
   * The size the module suggests, used when a screen doesn't set one.
   */
  default_size: "small" | "medium" | "large" | "wide" | "tall";
  name: string;
  /**
   * Effective poll period; `None` for push modules.
   */
  refresh_ms?: number | null;
  /**
   * True when `refresh_ms` comes from the config rather than the module.
   */
  refresh_overridden?: boolean;
  tile_id: string;
}
export interface RotateSettings {
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
  tile_id: string;
  /**
   * Unix ms.
   */
  updated_at: number;
  view: View1;
}
export interface View1 {
  root: Widget;
  status: Status;
  /**
   * One line, used when the tile is too small.
   */
  summary: string;
}
