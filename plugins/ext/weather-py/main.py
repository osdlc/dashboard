#!/usr/bin/env python3
"""Current weather and a short forecast from Open-Meteo (no key needed), for a place name or
a latitude and longitude.

The Python copy of the built-in `weather` module (crates/modules-personal/src/weather.rs), for
the native-vs-plugin benchmark. It builds the same view from the same Open-Meteo reply.

    dashboard plugin-check -- python3 plugins/ext/weather-py/main.py

A `plugin:` runs in the config file's folder, so this path is for a config in the repository
root; examples/bench.yaml uses `../plugins/ext/weather-py/main.py`.

    modules:
      weather-py:
        plugin: [python3, plugins/ext/weather-py/main.py]
        config: { place: Berlin, days: 5 }
"""

import datetime
import json
import math
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Dict, List, Optional

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from dashboard_plugin import Row, StatWidget, View, config_error, serve, upstream_error, with_defaults  # noqa: E402

# How long one request may take, in seconds.
TIMEOUT = 20

config: Dict[str, Any] = {
    "place": None,
    "latitude": None,
    "longitude": None,
    "name": None,
    "units": "metric",
    "days": 5,
    "forecast_url": "https://api.open-meteo.com/v1/forecast",
    "geocoding_url": "https://geocoding-api.open-meteo.com/v1/search",
}

# Where the forecast is for, and the name shown for it: latitude, longitude, label. Set from
# the coordinates at init, or by the first place lookup.
location: Optional[Dict[str, Any]] = None

SCHEMA = {
    "type": "object",
    "additionalProperties": False,
    "properties": {
        "place": {
            "type": ["string", "null"],
            "description": "A place name, looked up with Open-Meteo's geocoding API, e.g. \"Berlin\". "
            "Set this, or latitude and longitude.",
        },
        "latitude": {"type": ["number", "null"]},
        "longitude": {"type": ["number", "null"]},
        "name": {
            "type": ["string", "null"],
            "description": "The name shown on the tile. Default: the place's name, or the coordinates.",
        },
        "units": {"type": "string", "enum": ["metric", "imperial"], "description": "metric: °C, km/h. imperial: °F, mph."},
        "days": {"type": "integer", "minimum": 1, "maximum": 16, "description": "Forecast days, today first (1 to 16)."},
        "forecast_url": {"type": "string", "description": "The forecast API, e.g. a self-hosted Open-Meteo."},
        "geocoding_url": {"type": "string", "description": "The geocoding API, used for `place`."},
    },
}

CONDITIONS = {
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
}


def is_number(v: Any) -> bool:
    return isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v)


def is_whole(v: Any) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


def check() -> None:
    """The same checks, and messages, as the built-in module."""
    c = config
    for key in ("place", "name"):
        if c[key] is not None and not isinstance(c[key], str):
            raise config_error(f"{key} must be a string")
    for key in ("latitude", "longitude"):
        if c[key] is not None and not is_number(c[key]):
            raise config_error(f"{key} must be a number")
    for key in ("forecast_url", "geocoding_url"):
        if not isinstance(c[key], str):
            raise config_error(f"{key} must be a string")
    if c["units"] not in ("metric", "imperial"):
        raise config_error("units must be metric or imperial")
    if not is_whole(c["days"]):
        raise config_error("days must be a whole number")
    has_place = c["place"] is not None
    has_lat = c["latitude"] is not None
    has_lon = c["longitude"] is not None
    if has_place and not has_lat and not has_lon:
        if not c["place"].strip():
            raise config_error("place must not be empty")
    elif not has_place and has_lat and has_lon:
        if not -90 <= c["latitude"] <= 90:
            raise config_error("latitude must be between -90 and 90")
        if not -180 <= c["longitude"] <= 180:
            raise config_error("longitude must be between -180 and 180")
    elif has_place:
        raise config_error("set place, or latitude and longitude, not both")
    else:
        raise config_error("set place, or latitude and longitude")
    if not 1 <= c["days"] <= 16:
        raise config_error("days must be between 1 and 16")


def get(url: str, query: Dict[str, str]) -> Any:
    """GETs `url` and returns the JSON reply. Open-Meteo explains a bad request in a `reason`
    field."""
    req = urllib.request.Request(f"{url}?{urllib.parse.urlencode(query)}", headers={"User-Agent": "dashboard"})
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as res:
            body = res.read()
    except urllib.error.HTTPError as e:
        try:
            reason = json.loads(e.read()).get("reason")
        except (ValueError, AttributeError):
            reason = None
        if isinstance(reason, str):
            raise upstream_error(f"Open-Meteo: {reason}")
        raise upstream_error(f"Open-Meteo: HTTP {e.code}")
    except (OSError, ValueError) as e:
        raise upstream_error(f"Open-Meteo: {e}")
    try:
        return json.loads(body)
    except ValueError:
        raise upstream_error("unexpected reply from Open-Meteo")


def geocode() -> Dict[str, Any]:
    """Looks up `place` with the geocoding API."""
    name = config["place"] or ""
    reply = get(config["geocoding_url"], {"name": name, "count": "1", "language": "en", "format": "json"})
    results = reply.get("results") if isinstance(reply, dict) else None
    if not isinstance(results, list) or not results:
        raise upstream_error(f'no place named "{name}"')
    place = results[0]
    if not (isinstance(place, dict) and isinstance(place.get("name"), str)
            and is_number(place.get("latitude")) and is_number(place.get("longitude"))):
        raise upstream_error("unexpected reply from Open-Meteo")
    label = config["name"] if config["name"] is not None else place["name"]
    return {"latitude": place["latitude"], "longitude": place["longitude"], "label": label}


def forecast_from(reply: Any) -> Dict[str, Any]:
    """The reply, or an error when a value the tile needs is missing."""
    u = reply.get("current_units") if isinstance(reply, dict) else None
    c = reply.get("current") if isinstance(reply, dict) else None
    ok = (
        isinstance(u, dict) and isinstance(c, dict)
        and isinstance(u.get("temperature_2m"), str)
        and isinstance(u.get("wind_speed_10m"), str)
        and is_number(c.get("temperature_2m"))
        and is_number(c.get("apparent_temperature"))
        and is_whole(c.get("weather_code"))
        and is_number(c.get("wind_speed_10m"))
    )
    if not ok:
        raise upstream_error("unexpected reply from Open-Meteo")
    d = reply.get("daily") or {}

    def lst(key: str) -> list:
        v = d.get(key) if isinstance(d, dict) else None
        return v if isinstance(v, list) else []

    return {
        "current_units": u,
        "current": c,
        "daily": {k: lst(k) for k in (
            "time", "weather_code", "temperature_2m_max", "temperature_2m_min", "precipitation_probability_max",
        )},
    }


def conditions(code: int) -> str:
    """What a WMO weather code means."""
    return CONDITIONS.get(code, f"Code {code}")


def stormy(code: int) -> bool:
    return code >= 95


def whole(x: float) -> int:
    """Rounded half away from zero, like Rust's f64::round (Python's round goes to even)."""
    n = int(math.floor(abs(x) + 0.5))
    return -n if x < 0 else n


def coordinate(x: float) -> str:
    """"52.52": two decimals, rounded the same way as `whole`."""
    n = whole(x * 100)
    a = abs(n)
    return f"{'-' if n < 0 else ''}{a // 100}.{a % 100:02d}"


def day_label(i: int, date: str) -> str:
    """"Today" for the first day, then "Wed 28"."""
    if i == 0:
        return "Today"
    m = re.match(r"^(\d{4})-(\d{2})-(\d{2})", date)
    if not m:
        return date
    try:
        d = datetime.date(int(m.group(1)), int(m.group(2)), int(m.group(3)))
    except ValueError:
        return date
    return f"{['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'][d.weekday()]} {d.day}"


def stat(label: str, value: float, unit: str, delta: Optional[str] = None) -> StatWidget:
    w: StatWidget = {"type": "stat", "label": label, "value": str(whole(value)), "unit": unit}
    if delta is not None:
        w["delta"] = delta
    return w


def entry(v: list, i: int) -> Any:
    return v[i] if i < len(v) else None


def view(label: str, f: Dict[str, Any]) -> View:
    """The tile: now, then one row per day."""
    now = f["current"]
    temp_unit = f["current_units"]["temperature_2m"]
    # Open-Meteo writes miles per hour as "mp/h".
    wind_unit = f["current_units"]["wind_speed_10m"]
    if wind_unit == "mp/h":
        wind_unit = "mph"
    sky = conditions(now["weather_code"])
    summary = f"{label} · {whole(now['temperature_2m'])}{temp_unit} · {sky}"

    d = f["daily"]

    def degrees(v: list, i: int) -> str:
        x = entry(v, i)
        return "-" if x is None else f"{whole(x)}°"

    rows: List[Row] = []
    for i, date in enumerate(d["time"]):
        code = entry(d["weather_code"], i)
        rain = entry(d["precipitation_probability_max"], i)
        row: Row = {"cells": {
            "day": day_label(i, date),
            "sky": "-" if code is None else conditions(code),
            "high": degrees(d["temperature_2m_max"], i),
            "low": degrees(d["temperature_2m_min"], i),
            "rain": "-" if rain is None else f"{whole(rain)}%",
        }}
        if code is not None and stormy(code):
            row["status"] = "warn"
        rows.append(row)

    return {
        "status": "warn" if stormy(now["weather_code"]) else "ok",
        "summary": summary,
        "root": {
            "type": "column",
            "children": [
                {
                    "type": "row",
                    "children": [
                        stat(label, now["temperature_2m"], temp_unit, sky),
                        stat("Feels like", now["apparent_temperature"], temp_unit),
                        stat("Wind", now["wind_speed_10m"], wind_unit),
                    ],
                },
                {
                    "type": "table",
                    "columns": [
                        {"key": "day", "label": "Day"},
                        {"key": "sky", "label": "Conditions"},
                        {"key": "high", "label": "High"},
                        {"key": "low", "label": "Low"},
                        {"key": "rain", "label": "Rain"},
                    ],
                    "rows": rows,
                },
            ],
        },
    }


def init(c):
    global location
    config.update(with_defaults(c, config))
    check()
    lat, lon = config["latitude"], config["longitude"]
    if lat is not None and lon is not None:
        label = config["name"] if config["name"] is not None else f"{coordinate(lat)}, {coordinate(lon)}"
        location = {"latitude": lat, "longitude": lon, "label": label}


def render() -> View:
    global location
    if location is None:
        location = geocode()
    query = {
        "latitude": str(location["latitude"]),
        "longitude": str(location["longitude"]),
        "current": "temperature_2m,apparent_temperature,weather_code,wind_speed_10m",
        "daily": "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max",
        "timezone": "auto",
        "forecast_days": str(config["days"]),
    }
    if config["units"] == "imperial":
        query["temperature_unit"] = "fahrenheit"
        query["wind_speed_unit"] = "mph"
    return view(location["label"], forecast_from(get(config["forecast_url"], query)))


serve(
    manifest={
        "id": "weather-py",
        "name": "Weather (Python)",
        "refresh": {"mode": "poll", "interval_ms": 900000, "min_interval_ms": 60000},
        "layout": {"size": "medium", "priority": 30, "min_size": "small"},
        "config_schema": SCHEMA,
    },
    init=init,
    render=render,
)
