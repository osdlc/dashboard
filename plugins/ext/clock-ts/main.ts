// Clock: the local time in one or more time zones, as a row of stats. No network: the zone
// rules come from Node's built-in time zone data (Intl).
//
// The TypeScript version of a native-vs-plugin benchmark widget. It follows the same rules as
// the native module (crates/modules-fun/src/clock.rs) and the Python plugin
// (plugins/ext/clock-py), and must show the same view for the same input.
//
//   dashboard plugin-check -- node plugins/ext/clock-ts/main.ts

import { configError, serve, withDefaults } from "./dashboard-plugin.ts";
import type { View, Widget } from "./dashboard-plugin.ts";

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/** Looks like an IANA zone name: a letter, then letters, digits and `_+-/`. */
const ZONE_NAME = /^[A-Za-z][A-Za-z0-9_+\-/]*$/;
/** `YYYY-MM-DDTHH:MM:SS` followed by `Z` or `+HH:MM` / `-HH:MM`, nothing looser. */
const NOW = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:(Z)|([+-])(\d{2}):(\d{2}))$/;

type Config = { zones: unknown; hour12: unknown; seconds: unknown; now: unknown };

/** One zone to show, looked up. */
type Zone = { label: string; format: Intl.DateTimeFormat };

let zones: Zone[] = [];
let hour12 = false;
let seconds = false;
/** Seconds since 1970 to show instead of the current time. */
let fixedNow: number | null = null;

function lookup(entry: unknown): Zone {
  let name: unknown;
  let label: unknown = null;
  if (typeof entry === "string") name = entry;
  else if (entry !== null && typeof entry === "object" && !Array.isArray(entry)) {
    const unknown = Object.keys(entry).filter((k) => k !== "zone" && k !== "label");
    if (unknown.length > 0) throw configError(`zones: unknown settings: ${unknown.sort().join(", ")}`);
    ({ zone: name, label = null } = entry as { zone?: unknown; label?: unknown });
    if (label !== null && typeof label !== "string") throw configError("zones: label must be a string");
  }
  if (typeof name !== "string") throw configError("zones: each entry is a zone name or { zone, label }");
  let timeZone: string | undefined;
  if (name !== "local") {
    if (!ZONE_NAME.test(name)) throw configError(`unknown time zone ${JSON.stringify(name)}`);
    timeZone = name;
  }
  let format: Intl.DateTimeFormat;
  try {
    format = new Intl.DateTimeFormat("en-US", {
      timeZone,
      hourCycle: "h23",
      year: "numeric",
      month: "numeric",
      day: "numeric",
      hour: "numeric",
      minute: "numeric",
      second: "numeric",
    });
  } catch {
    throw configError(`unknown time zone ${JSON.stringify(name)}`);
  }
  return { label: (label as string | null) ?? defaultLabel(name), format };
}

/** "America/New_York" -> "New York"; "local" -> "Local". */
function defaultLabel(name: string): string {
  if (name === "local") return "Local";
  return name.slice(name.lastIndexOf("/") + 1).replaceAll("_", " ");
}

function daysInMonth(year: number, month: number): number {
  if (month === 2) return year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0) ? 29 : 28;
  return [4, 6, 9, 11].includes(month) ? 30 : 31;
}

/** Days from 1970-01-01 to the given date (Howard Hinnant's algorithm). */
function daysFromCivil(year: number, month: number, day: number): number {
  const y = month <= 2 ? year - 1 : year;
  const era = Math.floor(y / 400);
  const yoe = y - era * 400;
  const mp = (month + 9) % 12;
  const doy = Math.floor((153 * mp + 2) / 5) + day - 1;
  const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
  return era * 146_097 + doe - 719_468;
}

/** Seconds since 1970 for `s`, or null. Same checks and arithmetic as the native module. */
function parseNow(s: string): number | null {
  const m = NOW.exec(s);
  if (!m) return null;
  const [year, month, day, hour, minute, second] = m.slice(1, 7).map(Number);
  let offset = 0;
  if (!m[7]) {
    const [h, min] = [Number(m[9]), Number(m[10])];
    if (h > 23 || min > 59) return null;
    offset = (h * 60 + min) * (m[8] === "-" ? -1 : 1);
  }
  if (
    year < 1970 ||
    year > 2999 ||
    month < 1 ||
    month > 12 ||
    day < 1 ||
    day > daysInMonth(year, month) ||
    hour > 23 ||
    minute > 59 ||
    second > 59
  ) {
    return null;
  }
  return daysFromCivil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second - offset * 60;
}

/** The wall time in one zone: ["14:05", null] or ["2:05", "PM"], and the date as "Tue 27 Oct". */
function wall(zone: Zone, now: number): [string, string | null, string] {
  const parts: Record<string, number> = {};
  for (const p of zone.format.formatToParts(new Date(now * 1000))) {
    if (p.type !== "literal") parts[p.type] = Number(p.value);
  }
  const { year, month, day, hour: h, minute: m, second: s } = parts;
  const pad = (n: number) => String(n).padStart(2, "0");
  const secs = seconds ? `:${pad(s)}` : "";
  let time: string;
  let unit: string | null = null;
  if (hour12) {
    time = `${h % 12 === 0 ? 12 : h % 12}:${pad(m)}${secs}`;
    unit = h < 12 ? "AM" : "PM";
  } else {
    time = `${pad(h)}:${pad(m)}${secs}`;
  }
  // 1970-01-01 was a Thursday.
  const weekday = WEEKDAYS[(((daysFromCivil(year, month, day) + 4) % 7) + 7) % 7];
  return [time, unit, `${weekday} ${day} ${MONTHS[month - 1]}`];
}

function view(now: number): View {
  const summary: string[] = [];
  const children: Widget[] = [];
  for (const zone of zones) {
    const [time, unit, date] = wall(zone, now);
    summary.push(unit === null ? `${zone.label} ${time}` : `${zone.label} ${time} ${unit}`);
    children.push({ type: "stat", label: zone.label, value: time, unit, delta: date, status: null });
  }
  return { status: "ok", summary: summary.join(" · "), root: { type: "row", children } };
}

serve<Config>({
  manifest: () => ({
    id: "clock-ts",
    name: "Clock (TS)",
    refresh: { mode: "poll", interval_ms: seconds ? 1_000 : 5_000, min_interval_ms: 1_000 },
    layout: { size: "small", priority: 10, min_size: "small" },
  }),
  init(c) {
    const config = withDefaults(c, { zones: ["local"], hour12: false, seconds: false, now: null });
    if (!Array.isArray(config.zones)) throw configError("zones must be a list");
    if (config.zones.length === 0) throw configError("zones is empty: list at least one time zone");
    if (typeof config.hour12 !== "boolean") throw configError("hour12 must be true or false");
    if (typeof config.seconds !== "boolean") throw configError("seconds must be true or false");
    zones = config.zones.map(lookup);
    hour12 = config.hour12;
    seconds = config.seconds;
    fixedNow = null;
    if (config.now !== null) {
      fixedNow = typeof config.now === "string" ? parseNow(config.now) : null;
      if (fixedNow === null) {
        throw configError(`now: expected a time like 2026-10-27T14:05:00Z, got ${JSON.stringify(config.now)}`);
      }
    }
  },
  render() {
    return view(fixedNow ?? Math.floor(Date.now() / 1000));
  },
});
