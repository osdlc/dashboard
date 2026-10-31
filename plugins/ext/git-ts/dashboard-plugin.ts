// A dashboard plugin SDK for TypeScript. One file, no packages. Runs as is with Node 24
// (`node plugin.ts`, type stripping) and Bun (`bun plugin.ts`). Copy it next to your plugin.
//
//   import { serve } from "./dashboard-plugin.ts";
//
//   serve({
//     manifest: { id: "hello", name: "Hello", refresh: { mode: "poll", interval_ms: 5000 }, layout: { size: "small" } },
//     render: () => ({ status: "ok", summary: "hi", root: { type: "text", text: "hi" } }),
//   });
//
// The protocol is in docs/plugins.md. Only erasable TypeScript syntax is used here (no enums,
// no namespaces), so Node can strip the types without a build step.

import { createInterface } from "node:readline";

export const CONTRACT_VERSION = "0.1";

/** Something outside the plugin is unavailable: a CLI is missing, not logged in, an API is down. */
export const UPSTREAM_UNAVAILABLE = -32001;
/** The tile's `config:` block is invalid. */
export const CONFIG_INVALID = -32002;
export const METHOD_NOT_FOUND = -32601;
export const INTERNAL_ERROR = -32603;

/** An error the tile shows. Throw it from `init` or `render`. */
export class PluginError extends Error {
  code: number;
  constructor(code: number, message: string) {
    super(message);
    this.code = code;
  }
}

/** The config is wrong: the tile shows `message`. */
export function configError(message: string): PluginError {
  return new PluginError(CONFIG_INVALID, message);
}

/** A CLI, service or API the plugin needs is unavailable: the tile shows `message`. */
export function upstreamError(message: string): PluginError {
  return new PluginError(UPSTREAM_UNAVAILABLE, message);
}

/** The manifest a plugin gives `serve`: `contract_version` and `commands` are filled in. */
export type PluginManifest = Omit<Manifest, "contract_version" | "commands"> &
  Partial<Pick<Manifest, "contract_version" | "commands">>;

export interface Plugin<C = Record<string, unknown>> {
  /** The manifest, or a function of the config that returns it. */
  manifest: PluginManifest | ((config: C) => PluginManifest);
  /**
   * Called once with the tile's `config:` block (`{}` when there is none), before the
   * manifest is sent. Throw `configError(...)` to reject it.
   */
  init?(config: C): void | Promise<void>;
  /** Returns the tile's current view. Called on every refresh, never twice at once. */
  render(): View | Promise<View>;
  /** Called on `shutdown`, before the process exits. */
  shutdown?(): void | Promise<void>;
}

/** stdout carries only protocol messages, so `console.log` and friends go to stderr. */
const writeOut = process.stdout.write.bind(process.stdout);
console.log = console.error;
console.info = console.error;
console.debug = console.error;

function send(message: object): void {
  writeOut(JSON.stringify({ jsonrpc: "2.0", ...message }) + "\n");
}

/** Sends a line to the dashboard's log (not the tile). */
export function log(level: LogLevel, message: string): void {
  send({ method: "log", params: { level, message } });
}

/**
 * Merges the user's config over `defaults`, and rejects keys that `defaults` doesn't have
 * (usually a typo) with a config error.
 */
export function withDefaults<C extends Record<string, unknown>>(config: unknown, defaults: C): C {
  if (config === null || config === undefined) return { ...defaults };
  if (typeof config !== "object" || Array.isArray(config)) {
    throw configError("the config must be a mapping");
  }
  const unknown = Object.keys(config).filter((k) => !(k in defaults));
  if (unknown.length > 0) throw configError(`unknown settings: ${unknown.sort().join(", ")}`);
  return { ...defaults, ...(config as Partial<C>) };
}

/**
 * A chart whose history the host keeps. Each render sends only the newest value per series
 * (`null` sends no point, e.g. when a check failed); the host appends it and keeps the last
 * `history` points (default 300). The plugin can stay stateless.
 *
 *   liveSeries({ id: "latency", label: "Latency", unit: "ms" }, { api: 42, web: null })
 */
export function liveSeries(
  widget: Omit<SeriesWidget, "type" | "series" | "id"> & { id: string },
  values: Record<string, number | null | undefined>,
  now: number = Date.now(),
): SeriesWidget {
  const series = Object.entries(values).map(([name, value]) => ({
    name,
    points: value === null || value === undefined ? [] : [[now, value] as [number, number]],
  }));
  return { type: "series", ...widget, series };
}

/** Answers the dashboard's requests on stdin until it says `exit` or closes stdin. */
export function serve<C = Record<string, unknown>>(plugin: Plugin<C>): void {
  const handlers: Record<string, (params: any) => unknown> = {
    initialize: async (params: InitializeParams) => {
      const config = (params?.config ?? {}) as C;
      await plugin.init?.(config);
      const m = typeof plugin.manifest === "function" ? plugin.manifest(config) : plugin.manifest;
      return { manifest: { contract_version: CONTRACT_VERSION, commands: [], ...m } };
    },
    "view/render": () => plugin.render(),
    shutdown: async () => {
      await plugin.shutdown?.();
      return null;
    },
  };

  const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
  (async () => {
    for await (const line of lines) {
      if (!line.trim()) continue;
      let msg: { id?: number | string; method?: string; params?: unknown };
      try {
        msg = JSON.parse(line);
      } catch {
        console.error(`dashboard-plugin: not JSON: ${line.slice(0, 200)}`);
        continue;
      }
      if (msg.method === "exit") process.exit(0);
      if (msg.id === undefined || msg.method === undefined) continue; // a notification
      const handler = handlers[msg.method];
      try {
        if (!handler) throw new PluginError(METHOD_NOT_FOUND, `unknown method ${msg.method}`);
        const result = await handler(msg.params ?? {});
        send({ id: msg.id, result: result ?? null });
      } catch (err) {
        if (err instanceof PluginError) {
          send({ id: msg.id, error: { code: err.code, message: err.message } });
        } else {
          // A bug: the tile shows the error, the dashboard's log gets the stack.
          console.error(err);
          const message = err instanceof Error ? `${err.name}: ${err.message}` : String(err);
          send({ id: msg.id, error: { code: INTERNAL_ERROR, message } });
        }
      }
    }
    process.exit(0);
  })();
}

// The contract's types. `node sdk/gen-types.mjs` writes everything between the markers.
// ---- BEGIN GENERATED (node sdk/gen-types.mjs) ----

export interface CommandDecl {
  /** Program and arguments. May contain `{name}` placeholders declared in `params`. */
  argv: string[];
  cwd?: string | null;
  id: string;
  label: string;
  params?: ParamDecl[];
}

export interface LayoutHints {
  min_size?: Size | null;
  priority?: number;
  size: Size;
}

export interface ParamDecl {
  name: string;
  /** Regex the value must fully match. */
  pattern: string;
}

export type Refresh =
  | PollRefresh
  | PushRefresh;

export interface PollRefresh {
  /** Suggested period. Config can override it. */
  interval_ms: number;
  /** Floor for config overrides, e.g. to stay under an API rate limit. */
  min_interval_ms?: number | null;
  mode: "poll";
}

export interface PushRefresh {
  mode: "push";
}

export type Size = "small" | "medium" | "large" | "wide" | "tall";

export interface Manifest {
  commands?: CommandDecl[];
  config_schema?: unknown;
  contract_version: string;
  id: string;
  layout: LayoutHints;
  name: string;
  refresh: Refresh;
}

export type Action =
  | OpenUrlAction
  | RunCommandAction;

export interface OpenUrlAction {
  type: "open_url";
  url: string;
}

export interface RunCommandAction {
  command: string;
  params?: Record<string, string>;
  type: "run_command";
}

export interface Bar {
  label: string;
  /** Colors the bar ok, warn or error. */
  status?: Status | null;
  /** Fill color when `status` is not set. Default: `accent`. */
  tone?: Tone | null;
  value: number;
}

export type Cell = string | number | Widget;

export type ColorScale = "accent" | "heat" | "cool" | "sunset";

export interface Column {
  /** Names the cell in each row's `cells`. */
  key: string;
  /** The column heading. */
  label: string;
  /**
   * Which columns to keep when a table is too narrow for all of them:
   * renderers drop the lowest priority first (the rightmost of equal
   * ones) and always keep the first column. Default 0.
   */
  priority?: number | null;
}

export interface ListItem {
  /** What clicking the item does, e.g. open a link. */
  action?: Action | null;
  /** Colors the item's marker ok, warn or error. */
  status?: Status | null;
  /** A second, quieter line. */
  subtitle?: string | null;
  /** Short labels shown as chips. */
  tags?: string[];
  title: string;
  /** Color of the item's marker when `status` is not set. */
  tone?: Tone | null;
}

export type LogLevel = "error" | "warn" | "info" | "debug";

export interface LogLine {
  /** error, warn, info or debug: colors the line. */
  level?: LogLevel | null;
  text: string;
  /** Shown before the text, e.g. "14:02:11". */
  time?: string | null;
}

export interface Meter {
  label: string;
  /** 0-100. */
  percent: number;
  /** A fixed color for this meter instead of the grid's color scale. */
  tone?: Tone | null;
}

export interface Row {
  /** What clicking the row does, e.g. open a link. */
  action?: Action | null;
  /** Keyed by the column's `key`. */
  cells: Record<string, Cell>;
  /** Colors the row's marker ok, warn or error. */
  status?: Status | null;
}

export interface SeriesData {
  /** Shown in the legend; also keys the history when the widget has an `id`. */
  name: string;
  /** (unix ms, value) */
  points: [number, number][];
  /**
   * This series' color, instead of the widget's `color` (one series) or
   * the next `series-N` token (several).
   */
  tone?: Tone | null;
}

export type SeriesStyle = "line" | "area" | "bars";

export interface Slice {
  label: string;
  /** Colors the slice ok, warn or error. */
  status?: Status | null;
  /** Color when `status` is not set. Default: the next `series-N` token. */
  tone?: Tone | null;
  /** Zero or more. Shown with the widget's `format` and `unit`. */
  value: number;
}

export type Status = "ok" | "warn" | "error" | "unknown";

export interface Thresholds {
  /** The percent at which the fill turns the error color. */
  error: number;
  /** The percent at which the fill turns the warn color. */
  warn: number;
}

/** A token a widget can color something with, e.g. one line of a chart. */
export type Tone = "accent" | "ok" | "warn" | "error" | "info" | "muted" | "series-1" | "series-2" | "series-3" | "series-4" | "series-5" | "series-6";

export interface UptimeCheck {
  /** e.g. "HTTP 200" or the error. */
  detail?: string | null;
  /** Recent results, oldest first. Renderers show the newest that fit. */
  history?: Status[];
  /** Response time of the latest check. */
  latency_ms?: number | null;
  name: string;
  /** Result of the latest check. */
  status: Status;
  /** Share of recent checks that passed, 0-100. */
  uptime?: number | null;
  /** Opened when the check is clicked. */
  url?: string | null;
}

export type ValueFormat = "plain" | "bytes" | "count";

export type Widget =
  | TextWidget
  | StatWidget
  | GaugeWidget
  | SeriesWidget
  | MeterGridWidget
  | BarsWidget
  | PieWidget
  | ListWidget
  | TableWidget
  | LogWidget
  | UptimeWidget
  | ImageWidget
  | RowWidget
  | ColumnWidget;

/** A paragraph of text. */
export interface TextWidget {
  /** Read markdown in `text`: `**bold**`, and code between backticks. */
  markdown?: boolean;
  text: string;
  type: "text";
}

/** One number with a label, e.g. "p99 107 ms". */
export interface StatWidget {
  /** A change to show beside the value, e.g. "+2.8%". */
  delta?: string | null;
  /** Shown above or before the value. */
  label: string;
  /** Colors the value: ok, warn or error. */
  status?: Status | null;
  type: "stat";
  /** Shown after the value, smaller, e.g. "ms" or "/s". */
  unit?: string | null;
  /** Already formatted, e.g. "3451" or "1.2k". */
  value: string;
}

/** A horizontal meter from 0 to 100%. */
export interface GaugeWidget {
  /** A line under the meter, e.g. "1721 of 1858 GiB used". */
  caption?: string | null;
  label: string;
  /** A tick on the track at this percent, e.g. where usage should be by now. */
  marker?: number | null;
  /** How full, 0-100. */
  percent: number;
  /** Percents at which the fill turns warn and error colored. */
  thresholds?: Thresholds | null;
  type: "gauge";
}

/** Values over time, as a line, a filled area or bars. */
export interface SeriesWidget {
  /** The color of a single series. */
  color?: ColorScale;
  /** How values are written: plain, bytes or count. */
  format?: ValueFormat;
  /** Points the host keeps per series when `id` is set. Default 300. */
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
  /** One or more lines, each with its points. */
  series: SeriesData[];
  /** A line, a filled area or one bar per sample. */
  style?: SeriesStyle;
  type: "series";
  /** Shown after values, e.g. "%" or "/s". */
  unit?: string | null;
  /** The top of the chart. Default: the largest value. */
  y_max?: number | null;
}

/** Many small meters in a compact grid, e.g. one per CPU core. */
export interface MeterGridWidget {
  /** The meters' color: `accent`, or a scale colored by each meter's value. */
  color?: ColorScale;
  label?: string | null;
  meters: Meter[];
  type: "meter_grid";
}

/** Labeled horizontal bars, in the order given. */
export interface BarsWidget {
  bars: Bar[];
  /** How values are written: plain, bytes or count. */
  format?: ValueFormat;
  label?: string | null;
  /** Value of a full bar. Default: the largest value. */
  max?: number | null;
  type: "bars";
  /** Shown after each value. */
  unit?: string | null;
}

/**
 * Shares of a whole, as a pie or donut with a legend. Slices keep the
 * order given; each one's share is its value over the sum of all values.
 */
export interface PieWidget {
  /**
   * `accent` (the default) gives each slice its own color, the theme's
   * accent first. The other scales spread the slices along the scale
   * in order. A slice's `status` overrides its color.
   */
  color?: ColorScale;
  /** A ring with a hole in the middle. Default true; false draws a full disc. */
  donut?: boolean;
  /** How values are written in the legend: plain, bytes or count. */
  format?: ValueFormat;
  label?: string | null;
  slices: Slice[];
  type: "pie";
  /** Shown after each value in the legend. */
  unit?: string | null;
}

/** Items with a title, a subtitle, a status marker and tags. */
export interface ListWidget {
  /** Shown when there are no items. */
  empty_text?: string | null;
  items: ListItem[];
  type: "list";
}

/** Rows and columns. A cell holds text, a number, or a stat, gauge or pie. */
export interface TableWidget {
  columns: Column[];
  rows: Row[];
  type: "table";
}

/** Log output, oldest first. Renderers show the newest lines that fit. */
export interface LogWidget {
  label?: string | null;
  lines: LogLine[];
  type: "log";
}

/** Availability checks, one row each with a strip of recent results. */
export interface UptimeWidget {
  checks: UptimeCheck[];
  type: "uptime";
}

/**
 * A picture from a file on the machine the dashboard runs on. The terminal
 * draws it with the best graphics it supports (Kitty, iTerm2 or Sixel),
 * or with colored half blocks. It is scaled to fit and keeps its shape.
 */
export interface ImageWidget {
  /** A line under the picture, e.g. the file name. */
  caption?: string | null;
  /** Image file: PNG, JPEG, GIF (first frame), WebP or BMP. */
  path: string;
  type: "image";
}

/** Widgets side by side, sharing the width. */
export interface RowWidget {
  children: Widget[];
  type: "row";
}

/** Widgets stacked top to bottom. */
export interface ColumnWidget {
  children: Widget[];
  type: "column";
}

export interface View {
  root: Widget;
  status: Status;
  /** One line, used when the tile is too small. */
  summary: string;
}

export interface HostInfo {
  name: string;
  version: string;
}

export interface InitializeParams {
  config?: unknown;
  contract_version: string;
  host: HostInfo;
}

// ---- END GENERATED ----
