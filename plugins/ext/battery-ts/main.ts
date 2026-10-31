// Battery: power source, charge and time remaining, as a gauge. Reads `pmset -g batt` on
// macOS and /sys/class/power_supply elsewhere.
//
// The TypeScript version of a native-vs-plugin benchmark widget. It follows the same rules as
// the native module (crates/modules-system/src/battery.rs) and the Python plugin
// (plugins/ext/battery-py), and must show the same view for the same input.
//
//   dashboard plugin-check -- node plugins/ext/battery-ts/main.ts

import { execFile } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";

import { serve, upstreamError, withDefaults } from "./dashboard-plugin.ts";
import type { View, Widget } from "./dashboard-plugin.ts";

const SYSFS = "/sys/class/power_supply";

/** A plain decimal number, as Rust's f64 parser takes it (no hex, no blanks). */
const DECIMAL = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/;

let config: { low: number; critical: number; sample: string | null } = {
  low: 20,
  critical: 10,
  sample: null,
};

/** What the power source reports. */
type Reading = {
  /** "AC power", "battery" or "UPS power". */
  source: string;
  battery: Charge | null;
};

type Charge = {
  /** Whole percent, 0-100. */
  percent: number;
  /** "charging", "discharging", "charged", "not charging" or what the system said. */
  state: string;
  /** To empty when discharging, to full when charging. */
  minutes: number | null;
};

const clamp = (n: number) => Math.min(100, Math.max(0, n));

async function pmset(): Promise<string> {
  try {
    const { stdout } = await promisify(execFile)("pmset", ["-g", "batt"], { timeout: 10_000 });
    return stdout;
  } catch (e) {
    throw upstreamError(`running pmset: ${(e as Error).message}`);
  }
}

/** A saved sample: a folder is read as /sys/class/power_supply, a file as pmset output. */
function readPath(path: string): Reading {
  if (existsSync(path) && statSync(path).isDirectory()) return readSysfs(path);
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch (e) {
    throw upstreamError(`reading ${path}: ${(e as Error).message}`);
  }
  return parsePmset(text);
}

/**
 * Parses `pmset -g batt`:
 *
 *   Now drawing from 'Battery Power'
 *    -InternalBattery-0 (id=4653155)	76%; discharging; 4:12 remaining present: true
 *
 * A Mac without a battery prints only the first line.
 */
function parsePmset(text: string): Reading {
  const lines = text.split("\n");
  const first = lines.map((l) => l.trim()).find((l) => l.startsWith("Now drawing from '"));
  if (first === undefined) throw upstreamError(`unexpected pmset output: ${JSON.stringify(text.trim())}`);
  const from = first.slice("Now drawing from '".length).replace(/'+$/, "");
  const source = from.includes("Battery") ? "battery" : from.includes("UPS") ? "UPS power" : "AC power";
  return { source, battery: pmsetBattery(lines) };
}

function pmsetBattery(lines: string[]): Charge | null {
  const line = lines.find((l) => l.trimStart().startsWith("-InternalBattery"));
  const tab = line?.indexOf("\t") ?? -1;
  if (line === undefined || tab < 0) return null;
  const parts = line.slice(tab + 1).split(";").map((p) => p.trim());
  if (!parts[0].endsWith("%")) return null;
  const text = parts[0].slice(0, -1);
  const percent = DECIMAL.test(text) ? Number(text) : NaN;
  if (!Number.isFinite(percent)) return null;
  const raw = parts[1] ?? "";
  const state = raw === "finishing charge" ? "charging" : raw === "AC attached" ? "not charging" : raw.toLowerCase();
  return { percent: clamp(percent), state, minutes: moving(state) ? remaining(parts[2] ?? "") : null };
}

/** "4:12 remaining present: true" -> 252. */
function remaining(text: string): number | null {
  const at = text.indexOf(" remaining");
  if (at < 0) return null;
  const m = /^(\d+):(\d+)$/.exec(text.slice(0, at).trim());
  return m ? Number(m[1]) * 60 + Number(m[2]) : null;
}

/** Only charging and discharging have a time worth showing; pmset says "0:00 remaining" once charged. */
function moving(state: string): boolean {
  return state === "charging" || state === "discharging";
}

/**
 * Reads a /sys/class/power_supply folder: one folder per supply, each with a `type` file. The
 * first battery (by name) is shown. A battery whose `scope` is `Device` belongs to a mouse,
 * keyboard or other peripheral and is skipped. No folder, or no battery in it, reads as AC power
 * with no battery.
 */
function readSysfs(dir: string): Reading {
  let names: string[] = [];
  try {
    names = readdirSync(dir).sort();
  } catch {
    // No folder: no battery.
  }
  let ac = false;
  let battery: Charge | null = null;
  for (const name of names) {
    const file = (f: string): string | null => {
      try {
        return readFileSync(join(dir, name, f), "utf8").trim();
      } catch {
        return null;
      }
    };
    const number = (f: string): number | null => {
      const s = file(f);
      const n = s === null || !DECIMAL.test(s) ? NaN : Number(s);
      return Number.isFinite(n) ? n : null;
    };
    if (file("type") !== "Battery") {
      if (file("online") === "1") ac = true;
      continue;
    }
    if (battery !== null || file("present") === "0" || file("scope") === "Device") continue;
    const status = file("status");
    const state = status === "Full" ? "charged" : status === null ? "unknown" : status.toLowerCase();
    // Energy in µWh and µW, or charge in µAh and µA: either works for a ratio.
    let now = number("energy_now");
    let full: number | null;
    let rate: number | null;
    if (now !== null) {
      full = number("energy_full");
      rate = number("power_now");
    } else {
      now = number("charge_now");
      full = number("charge_full");
      rate = number("current_now");
    }
    const capacity = number("capacity");
    let percent = 0;
    if (capacity !== null) percent = capacity;
    else if (now !== null && full !== null && full > 0) percent = Math.floor((now / full) * 100 + 0.5);
    // Some drivers report the rate as negative while discharging.
    if (rate !== null) rate = Math.abs(rate);
    let minutes: number | null = null;
    if (rate !== null && rate > 0) {
      if (state === "discharging" && now !== null) minutes = Math.floor((now * 60) / rate);
      else if (state === "charging" && now !== null && full !== null)
        minutes = Math.floor((Math.max(0, full - now) * 60) / rate);
    }
    battery = { percent: clamp(percent), state, minutes };
  }
  return { source: !ac && battery !== null ? "battery" : "AC power", battery };
}

/** The tile, built exactly like the native module's. */
function view(r: Reading, low: number, critical: number): View {
  const b = r.battery;
  if (b === null) {
    return {
      status: "ok",
      summary: `${r.source} · no battery`,
      root: { type: "text", text: `No battery found. Running on ${r.source}.`, markdown: false },
    };
  }
  const pct = Math.trunc(b.percent);
  const onBattery = b.state === "discharging";
  const status = onBattery && b.percent <= critical ? "error" : onBattery && b.percent <= low ? "warn" : "ok";
  const time =
    b.minutes === null ? "-" : `${Math.floor(b.minutes / 60)}:${String(b.minutes % 60).padStart(2, "0")}`;
  let summary = `${pct}% · ${b.state}`;
  if (b.minutes !== null && b.state === "discharging") summary += ` · ${time} left`;
  if (b.minutes !== null && b.state === "charging") summary += ` · ${time} to full`;
  const stat = (label: string, value: string): Widget => ({ type: "stat", label, value });
  return {
    status,
    summary,
    root: {
      type: "column",
      children: [
        { type: "gauge", label: "Battery", percent: b.percent, caption: `on ${r.source}` },
        {
          type: "row",
          children: [
            stat("Power", r.source),
            stat("State", b.state),
            stat(b.state === "charging" ? "To full" : "Time left", time),
          ],
        },
      ],
    },
  };
}

serve({
  manifest: {
    id: "battery-ts",
    name: "Battery (TS)",
    refresh: { mode: "poll", interval_ms: 30_000, min_interval_ms: 5_000 },
    layout: { size: "small", priority: 20, min_size: "small" },
  },
  init(c) {
    config = withDefaults(c, config);
    if (typeof config.sample === "string" && config.sample.startsWith("~/")) {
      config.sample = join(homedir(), config.sample.slice(2));
    }
  },
  async render() {
    let reading: Reading;
    if (config.sample !== null) reading = readPath(config.sample);
    else if (process.platform === "darwin") reading = parsePmset(await pmset());
    else if (process.platform === "linux") reading = readSysfs(SYSFS);
    else throw upstreamError(`battery info is not available on ${process.platform}`);
    return view(reading, config.low, config.critical);
  },
});
