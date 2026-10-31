/* Generated from the Rust contract by pnpm gen:types. Do not edit. */

export type Size = "small" | "medium" | "large" | "wide" | "tall";
export type Widget =
  | {
      markdown?: boolean;
      text: string;
      type: "text";
    }
  | {
      delta?: string | null;
      label: string;
      status?: Status | null;
      type: "stat";
      unit?: string | null;
      value: string;
    }
  | {
      caption?: string | null;
      label: string;
      marker?: number | null;
      percent: number;
      thresholds?: Thresholds | null;
      type: "gauge";
    }
  | {
      color?: "accent" | "heat" | "cool" | "sunset";
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
      series: SeriesData[];
      style?: "line" | "area" | "bars";
      type: "series";
      unit?: string | null;
      y_max?: number | null;
    }
  | {
      color?: "accent" | "heat" | "cool" | "sunset";
      label?: string | null;
      meters: Meter[];
      type: "meter_grid";
    }
  | {
      bars: Bar[];
      format?: "plain" | "bytes" | "count";
      label?: string | null;
      /**
       * Value of a full bar. Default: the largest value.
       */
      max?: number | null;
      type: "bars";
      unit?: string | null;
    }
  | {
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
          markdown?: boolean;
          text: string;
          type: "text";
        }
      | {
          delta?: string | null;
          label: string;
          status?: Status | null;
          type: "stat";
          unit?: string | null;
          value: string;
        }
      | {
          caption?: string | null;
          label: string;
          marker?: number | null;
          percent: number;
          thresholds?: Thresholds | null;
          type: "gauge";
        }
      | {
          color?: "accent" | "heat" | "cool" | "sunset";
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
          series: SeriesData[];
          style?: "line" | "area" | "bars";
          type: "series";
          unit?: string | null;
          y_max?: number | null;
        }
      | {
          color?: "accent" | "heat" | "cool" | "sunset";
          label?: string | null;
          meters: Meter[];
          type: "meter_grid";
        }
      | {
          bars: Bar[];
          format?: "plain" | "bytes" | "count";
          label?: string | null;
          /**
           * Value of a full bar. Default: the largest value.
           */
          max?: number | null;
          type: "bars";
          unit?: string | null;
        }
      | {
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
  error: number;
  warn: number;
}
export interface SeriesData {
  name: string;
  /**
   * (unix ms, value)
   */
  points: [unknown, unknown][];
}
export interface Meter {
  label: string;
  percent: number;
}
export interface Bar {
  label: string;
  status?: Status | null;
  value: number;
}
export interface ListItem {
  action?: Action | null;
  status?: Status | null;
  subtitle?: string | null;
  tags?: string[];
  title: string;
}
export interface Column {
  key: string;
  label: string;
}
export interface Row {
  action?: Action | null;
  /**
   * Keyed by `Column::key`.
   */
  cells: {
    [k: string]: Cell;
  };
  status?: Status | null;
}
export interface LogLine {
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
