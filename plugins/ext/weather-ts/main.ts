// Current weather and a short forecast from Open-Meteo (no key needed), for a place name or a
// latitude and longitude.
//
// The TypeScript copy of the built-in `weather` module (crates/modules-personal/src/weather.rs),
// for the native-vs-plugin benchmark. It builds the same view from the same Open-Meteo reply.
//
//   dashboard plugin-check -- node plugins/ext/weather-ts/main.ts
//
// A `plugin:` runs in the config file's folder, so this path is for a config in the repository
// root; examples/bench.yaml uses `../plugins/ext/weather-ts/main.ts`.
//
//   modules:
//     weather-ts:
//       plugin: [node, plugins/ext/weather-ts/main.ts]
//       config: { place: Berlin, days: 5 }

import { configError, serve, upstreamError, withDefaults } from "./dashboard-plugin.ts";
import type { Row, StatWidget, View } from "./dashboard-plugin.ts";

/** How long one request may take, in ms. */
const TIMEOUT = 20_000;

let config = {
  place: null as string | null,
  latitude: null as number | null,
  longitude: null as number | null,
  name: null as string | null,
  units: "metric",
  days: 5,
  forecast_url: "https://api.open-meteo.com/v1/forecast",
  geocoding_url: "https://geocoding-api.open-meteo.com/v1/search",
};

/** Where the forecast is for, and the name shown for it. */
type Location = { latitude: number; longitude: number; label: string };
/** Set from the coordinates at init, or by the first place lookup. */
let location: Location | null = null;

/** The parts of a forecast reply the tile uses. Any daily entry may be null. */
type Forecast = {
  current_units: { temperature_2m: string; wind_speed_10m: string };
  current: { temperature_2m: number; apparent_temperature: number; weather_code: number; wind_speed_10m: number };
  daily: {
    time: string[];
    weather_code: (number | null)[];
    temperature_2m_max: (number | null)[];
    temperature_2m_min: (number | null)[];
    precipitation_probability_max: (number | null)[];
  };
};

const isNumber = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);

/** The same checks, and messages, as the built-in module. */
function check(): void {
  const c = config;
  for (const key of ["place", "name"] as const) {
    if (c[key] !== null && typeof c[key] !== "string") throw configError(`${key} must be a string`);
  }
  for (const key of ["latitude", "longitude"] as const) {
    if (c[key] !== null && !isNumber(c[key])) throw configError(`${key} must be a number`);
  }
  for (const key of ["forecast_url", "geocoding_url"] as const) {
    if (typeof c[key] !== "string") throw configError(`${key} must be a string`);
  }
  if (c.units !== "metric" && c.units !== "imperial") throw configError("units must be metric or imperial");
  if (!Number.isInteger(c.days)) throw configError("days must be a whole number");
  const hasPlace = c.place !== null;
  const hasLat = c.latitude !== null;
  const hasLon = c.longitude !== null;
  if (hasPlace && !hasLat && !hasLon) {
    if (c.place!.trim() === "") throw configError("place must not be empty");
  } else if (!hasPlace && hasLat && hasLon) {
    if (c.latitude! < -90 || c.latitude! > 90) throw configError("latitude must be between -90 and 90");
    if (c.longitude! < -180 || c.longitude! > 180) throw configError("longitude must be between -180 and 180");
  } else if (hasPlace) {
    throw configError("set place, or latitude and longitude, not both");
  } else {
    throw configError("set place, or latitude and longitude");
  }
  if (c.days < 1 || c.days > 16) throw configError("days must be between 1 and 16");
}

/** GETs `url` and returns the JSON reply. Open-Meteo explains a bad request in a `reason` field. */
async function get(url: string, query: Record<string, string>): Promise<any> {
  let res: Response;
  let body: string;
  try {
    res = await fetch(`${url}?${new URLSearchParams(query)}`, { signal: AbortSignal.timeout(TIMEOUT) });
    body = await res.text();
  } catch (e) {
    throw upstreamError(`Open-Meteo: ${(e as Error).message}`);
  }
  let json: any = undefined;
  try {
    json = JSON.parse(body);
  } catch {
    // not JSON
  }
  if (!res.ok) {
    if (typeof json?.reason === "string") throw upstreamError(`Open-Meteo: ${json.reason}`);
    throw upstreamError(`Open-Meteo: HTTP ${res.status}`);
  }
  if (json === undefined) throw upstreamError("unexpected reply from Open-Meteo");
  return json;
}

/** Looks up `place` with the geocoding API. */
async function geocode(): Promise<Location> {
  const name = config.place ?? "";
  const reply = await get(config.geocoding_url, { name, count: "1", language: "en", format: "json" });
  const place = Array.isArray(reply?.results) ? reply.results[0] : undefined;
  if (place === undefined) throw upstreamError(`no place named "${name}"`);
  if (typeof place.name !== "string" || !isNumber(place.latitude) || !isNumber(place.longitude)) {
    throw upstreamError("unexpected reply from Open-Meteo");
  }
  return { latitude: place.latitude, longitude: place.longitude, label: config.name ?? place.name };
}

/** The reply, or an error when a value the tile needs is missing. */
function forecastFrom(reply: any): Forecast {
  const u = reply?.current_units;
  const c = reply?.current;
  const ok =
    typeof u?.temperature_2m === "string" &&
    typeof u?.wind_speed_10m === "string" &&
    isNumber(c?.temperature_2m) &&
    isNumber(c?.apparent_temperature) &&
    Number.isInteger(c?.weather_code) &&
    isNumber(c?.wind_speed_10m);
  if (!ok) throw upstreamError("unexpected reply from Open-Meteo");
  const d = reply.daily ?? {};
  const list = (v: unknown) => (Array.isArray(v) ? v : []);
  return {
    current_units: u,
    current: c,
    daily: {
      time: list(d.time),
      weather_code: list(d.weather_code),
      temperature_2m_max: list(d.temperature_2m_max),
      temperature_2m_min: list(d.temperature_2m_min),
      precipitation_probability_max: list(d.precipitation_probability_max),
    },
  };
}

const CONDITIONS: Record<number, string> = {
  0: "Clear sky",
  1: "Mainly clear",
  2: "Partly cloudy",
  3: "Overcast",
  45: "Fog",
  48: "Rime fog",
  51: "Light drizzle",
  53: "Drizzle",
  55: "Heavy drizzle",
  56: "Light freezing drizzle",
  57: "Freezing drizzle",
  61: "Light rain",
  63: "Rain",
  65: "Heavy rain",
  66: "Light freezing rain",
  67: "Freezing rain",
  71: "Light snow",
  73: "Snow",
  75: "Heavy snow",
  77: "Snow grains",
  80: "Light showers",
  81: "Showers",
  82: "Heavy showers",
  85: "Light snow showers",
  86: "Snow showers",
  95: "Thunderstorm",
  96: "Thunderstorm, hail",
  99: "Thunderstorm, heavy hail",
};

/** What a WMO weather code means. */
const conditions = (code: number) => CONDITIONS[code] ?? `Code ${code}`;
const stormy = (code: number) => code >= 95;

/** Rounded half away from zero, like Rust's f64::round (Math.round differs for negatives). */
function whole(x: number): number {
  const n = Math.floor(Math.abs(x) + 0.5);
  return x < 0 && n !== 0 ? -n : n;
}

/** "52.52": two decimals, rounded the same way as `whole`. */
function coordinate(x: number): string {
  const n = whole(x * 100);
  const a = Math.abs(n);
  return `${n < 0 ? "-" : ""}${Math.floor(a / 100)}.${String(a % 100).padStart(2, "0")}`;
}

/** "Today" for the first day, then "Wed 28". */
function dayLabel(i: number, date: string): string {
  if (i === 0) return "Today";
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(date);
  if (!m) return date;
  const weekday = new Date(Date.UTC(+m[1], +m[2] - 1, +m[3])).getUTCDay();
  return `${["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][weekday]} ${+m[3]}`;
}

function stat(label: string, value: number, unit: string, delta?: string): StatWidget {
  const w: StatWidget = { type: "stat", label, value: String(whole(value)), unit };
  if (delta !== undefined) w.delta = delta;
  return w;
}

const entry = <T>(v: T[], i: number): T | null => (i < v.length && v[i] !== undefined ? v[i] : null);

/** The tile: now, then one row per day. */
function view(label: string, f: Forecast): View {
  const now = f.current;
  const tempUnit = f.current_units.temperature_2m;
  // Open-Meteo writes miles per hour as "mp/h".
  const windUnit = f.current_units.wind_speed_10m === "mp/h" ? "mph" : f.current_units.wind_speed_10m;
  const sky = conditions(now.weather_code);
  const summary = `${label} · ${whole(now.temperature_2m)}${tempUnit} · ${sky}`;

  const d = f.daily;
  const degrees = (v: (number | null)[], i: number) => {
    const x = entry(v, i);
    return x === null ? "-" : `${whole(x)}°`;
  };
  const rows: Row[] = d.time.map((date, i) => {
    const code = entry(d.weather_code, i);
    const rain = entry(d.precipitation_probability_max, i);
    const row: Row = {
      cells: {
        day: dayLabel(i, date),
        sky: code === null ? "-" : conditions(code),
        high: degrees(d.temperature_2m_max, i),
        low: degrees(d.temperature_2m_min, i),
        rain: rain === null ? "-" : `${whole(rain)}%`,
      },
    };
    if (code !== null && stormy(code)) row.status = "warn";
    return row;
  });

  return {
    status: stormy(now.weather_code) ? "warn" : "ok",
    summary,
    root: {
      type: "column",
      children: [
        {
          type: "row",
          children: [
            stat(label, now.temperature_2m, tempUnit, sky),
            stat("Feels like", now.apparent_temperature, tempUnit),
            stat("Wind", now.wind_speed_10m, windUnit),
          ],
        },
        {
          type: "table",
          columns: [
            { key: "day", label: "Day" },
            { key: "sky", label: "Conditions" },
            { key: "high", label: "High" },
            { key: "low", label: "Low" },
            { key: "rain", label: "Rain" },
          ],
          rows,
        },
      ],
    },
  };
}

serve({
  manifest: {
    id: "weather-ts",
    name: "Weather (TypeScript)",
    refresh: { mode: "poll", interval_ms: 900000, min_interval_ms: 60000 },
    layout: { size: "medium", priority: 30, min_size: "small" },
    config_schema: {
      type: "object",
      additionalProperties: false,
      properties: {
        place: { type: ["string", "null"], description: "A place name, looked up with Open-Meteo's geocoding API, e.g. \"Berlin\". Set this, or latitude and longitude." },
        latitude: { type: ["number", "null"] },
        longitude: { type: ["number", "null"] },
        name: { type: ["string", "null"], description: "The name shown on the tile. Default: the place's name, or the coordinates." },
        units: { type: "string", enum: ["metric", "imperial"], description: "metric: °C, km/h. imperial: °F, mph." },
        days: { type: "integer", minimum: 1, maximum: 16, description: "Forecast days, today first (1 to 16)." },
        forecast_url: { type: "string", description: "The forecast API, e.g. a self-hosted Open-Meteo." },
        geocoding_url: { type: "string", description: "The geocoding API, used for `place`." },
      },
    },
  },
  init(c) {
    config = withDefaults(c, config);
    check();
    if (config.latitude !== null && config.longitude !== null) {
      const { latitude, longitude } = config;
      location = { latitude, longitude, label: config.name ?? `${coordinate(latitude)}, ${coordinate(longitude)}` };
    }
  },
  async render(): Promise<View> {
    if (location === null) location = await geocode();
    const query: Record<string, string> = {
      latitude: String(location.latitude),
      longitude: String(location.longitude),
      current: "temperature_2m,apparent_temperature,weather_code,wind_speed_10m",
      daily: "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max",
      timezone: "auto",
      forecast_days: String(config.days),
    };
    if (config.units === "imperial") {
      query.temperature_unit = "fahrenheit";
      query.wind_speed_unit = "mph";
    }
    return view(location.label, forecastFrom(await get(config.forecast_url, query)));
  },
});
