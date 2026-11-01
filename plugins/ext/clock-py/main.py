#!/usr/bin/env python3
"""Clock: the local time in one or more time zones, as a row of stats. No network: the zone
rules come from the system's time zone database (zoneinfo).

The Python version of a native-vs-plugin benchmark widget. It follows the same rules as the
native module (crates/modules-fun/src/clock.rs) and the TypeScript plugin
(plugins/ext/clock-ts), and must show the same view for the same input.

    dashboard plugin-check -- python3 plugins/ext/clock-py/main.py
"""

import json
import re
import time
from datetime import datetime, timezone, tzinfo
from typing import List, Optional, Tuple, TypedDict
from zoneinfo import ZoneInfo

from dashboard_plugin import View, Widget, config_error, serve, with_defaults

MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]

# Looks like an IANA zone name: a letter, then letters, digits and `_+-/`.
ZONE_NAME = re.compile(r"[A-Za-z][A-Za-z0-9_+\-/]*", re.ASCII)
# `YYYY-MM-DDTHH:MM:SS` followed by `Z` or `+HH:MM` / `-HH:MM`, nothing looser.
NOW = re.compile(
    r"(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:(Z)|([+-])(\d{2}):(\d{2}))", re.ASCII
)


class Zone(TypedDict):
    label: str
    tz: Optional[tzinfo]  # None for this machine's zone


zones: List[Zone] = []
settings = {"hour12": False, "seconds": False}
fixed_now: Optional[int] = None  # seconds since 1970 to show instead of the current time


def lookup(entry) -> Zone:
    label = None
    if isinstance(entry, str):
        name = entry
    elif isinstance(entry, dict):
        unknown = sorted(set(entry) - {"zone", "label"})
        if unknown:
            raise config_error(f"zones: unknown settings: {', '.join(unknown)}")
        name, label = entry.get("zone"), entry.get("label")
        if label is not None and not isinstance(label, str):
            raise config_error("zones: label must be a string")
    else:
        name = None
    if not isinstance(name, str):
        raise config_error("zones: each entry is a zone name or { zone, label }")
    tz = None
    if name != "local":
        if not ZONE_NAME.fullmatch(name):
            raise config_error(f"unknown time zone {json.dumps(name)}")
        try:
            tz = ZoneInfo(name)
        except Exception:
            raise config_error(f"unknown time zone {json.dumps(name)}") from None
    return {"label": default_label(name) if label is None else label, "tz": tz}


def default_label(name: str) -> str:
    """"America/New_York" -> "New York"; "local" -> "Local"."""
    if name == "local":
        return "Local"
    return name.rsplit("/", 1)[-1].replace("_", " ")


def days_in_month(year: int, month: int) -> int:
    if month == 2:
        return 29 if year % 4 == 0 and (year % 100 != 0 or year % 400 == 0) else 28
    return 30 if month in (4, 6, 9, 11) else 31


def days_from_civil(year: int, month: int, day: int) -> int:
    """Days from 1970-01-01 to the given date (Howard Hinnant's algorithm)."""
    y = year - 1 if month <= 2 else year
    era = y // 400
    yoe = y - era * 400
    mp = (month + 9) % 12
    doy = (153 * mp + 2) // 5 + day - 1
    doe = yoe * 365 + yoe // 4 - yoe // 100 + doy
    return era * 146097 + doe - 719468


def parse_now(s: str) -> Optional[int]:
    """Seconds since 1970 for `s`, or None. Same checks and arithmetic as the native module."""
    m = NOW.fullmatch(s)
    if not m:
        return None
    year, month, day, hour, minute, second = (int(g) for g in m.groups()[:6])
    offset = 0
    if not m.group(7):
        h, mins = int(m.group(9)), int(m.group(10))
        if h > 23 or mins > 59:
            return None
        offset = (h * 60 + mins) * (-1 if m.group(8) == "-" else 1)
    if (
        not 1970 <= year <= 2999
        or not 1 <= month <= 12
        or not 1 <= day <= days_in_month(year, month)
        or hour > 23
        or minute > 59
        or second > 59
    ):
        return None
    return days_from_civil(year, month, day) * 86400 + hour * 3600 + minute * 60 + second - offset * 60


def wall(zone: Zone, now: int) -> Tuple[str, Optional[str], str]:
    """The wall time in one zone: ("14:05", None) or ("2:05", "PM"), and the date as "Tue 27 Oct"."""
    utc = datetime.fromtimestamp(now, timezone.utc)
    t = utc.astimezone(zone["tz"]) if zone["tz"] is not None else utc.astimezone()
    secs = f":{t.second:02}" if settings["seconds"] else ""
    if settings["hour12"]:
        h12 = 12 if t.hour % 12 == 0 else t.hour % 12
        time_, unit = f"{h12}:{t.minute:02}{secs}", "AM" if t.hour < 12 else "PM"
    else:
        time_, unit = f"{t.hour:02}:{t.minute:02}{secs}", None
    # 1970-01-01 was a Thursday.
    weekday = WEEKDAYS[(days_from_civil(t.year, t.month, t.day) + 4) % 7]
    return time_, unit, f"{weekday} {t.day} {MONTHS[t.month - 1]}"


def view(now: int) -> View:
    summary: List[str] = []
    children: List[Widget] = []
    for zone in zones:
        time_, unit, date = wall(zone, now)
        summary.append(f"{zone['label']} {time_}" if unit is None else f"{zone['label']} {time_} {unit}")
        children.append(
            {"type": "stat", "label": zone["label"], "value": time_, "unit": unit, "delta": date, "status": None}
        )
    return {"status": "ok", "summary": " · ".join(summary), "root": {"type": "row", "children": children}}


def init(c):
    global zones, fixed_now
    config = with_defaults(c, {"zones": ["local"], "hour12": False, "seconds": False, "now": None})
    if not isinstance(config["zones"], list):
        raise config_error("zones must be a list")
    if not config["zones"]:
        raise config_error("zones is empty: list at least one time zone")
    for key in ("hour12", "seconds"):
        if not isinstance(config[key], bool):
            raise config_error(f"{key} must be true or false")
        settings[key] = config[key]
    zones = [lookup(z) for z in config["zones"]]
    fixed_now = None
    if config["now"] is not None:
        fixed_now = parse_now(config["now"]) if isinstance(config["now"], str) else None
        if fixed_now is None:
            raise config_error(f"now: expected a time like 2026-10-27T14:05:00Z, got {json.dumps(config['now'])}")


def render() -> View:
    return view(fixed_now if fixed_now is not None else int(time.time()))


serve(
    manifest=lambda _config: {
        "id": "clock-py",
        "name": "Clock (Python)",
        "refresh": {"mode": "poll", "interval_ms": 1000 if settings["seconds"] else 5000, "min_interval_ms": 1000},
        "layout": {"size": "small", "priority": 10, "min_size": "small"},
    },
    init=init,
    render=render,
)
