#!/usr/bin/env python3
"""Battery: power source, charge and time remaining, as a gauge. Reads `pmset -g batt` on
macOS and /sys/class/power_supply elsewhere.

The Python version of a native-vs-plugin benchmark widget. It follows the same rules as the
native module (crates/modules-system/src/battery.rs) and the TypeScript plugin
(plugins/ext/battery-ts), and must show the same view for the same input.

    dashboard plugin-check -- python3 plugins/ext/battery-py/main.py
"""

import math
import os
import re
import subprocess
import sys
from typing import List, Optional, TypedDict

from dashboard_plugin import View, Widget, serve, upstream_error, with_defaults

SYSFS = "/sys/class/power_supply"

# A plain decimal number, as Rust's f64 parser takes it (no underscores, no blanks).
DECIMAL = re.compile(r"[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?", re.ASCII)


def decimal(s: Optional[str]) -> Optional[float]:
    """The number in `s`, or None if it isn't a plain, finite decimal."""
    if s is None or not DECIMAL.fullmatch(s):
        return None
    n = float(s)
    return n if math.isfinite(n) else None

config = {"low": 20, "critical": 10, "sample": None}


class Charge(TypedDict):
    percent: float  # whole percent, 0-100
    state: str  # "charging", "discharging", "charged", "not charging" or what the system said
    minutes: Optional[int]  # to empty when discharging, to full when charging


class Reading(TypedDict):
    source: str  # "AC power", "battery" or "UPS power"
    battery: Optional[Charge]


def clamp(n: float) -> float:
    return min(100.0, max(0.0, float(n)))


def pmset() -> str:
    try:
        out = subprocess.run(["pmset", "-g", "batt"], capture_output=True, text=True, timeout=10, check=True)
    except (OSError, subprocess.SubprocessError) as e:
        raise upstream_error(f"running pmset: {e}")
    return out.stdout


def read_path(path: str) -> Reading:
    """A saved sample: a folder is read as /sys/class/power_supply, a file as pmset output."""
    if os.path.isdir(path):
        return read_sysfs(path)
    try:
        with open(path, encoding="utf-8") as f:
            text = f.read()
    except (OSError, UnicodeDecodeError) as e:
        raise upstream_error(f"reading {path}: {e}")
    return parse_pmset(text)


def parse_pmset(text: str) -> Reading:
    """Parses `pmset -g batt`:

        Now drawing from 'Battery Power'
         -InternalBattery-0 (id=4653155)	76%; discharging; 4:12 remaining present: true

    A Mac without a battery prints only the first line.
    """
    lines = text.split("\n")
    first = next((l.strip() for l in lines if l.strip().startswith("Now drawing from '")), None)
    if first is None:
        raise upstream_error(f"unexpected pmset output: {text.strip()!r}")
    frm = first[len("Now drawing from '"):].rstrip("'")
    source = "battery" if "Battery" in frm else "UPS power" if "UPS" in frm else "AC power"
    return {"source": source, "battery": pmset_battery(lines)}


def pmset_battery(lines: List[str]) -> Optional[Charge]:
    line = next((l for l in lines if l.lstrip().startswith("-InternalBattery")), None)
    if line is None or "\t" not in line:
        return None
    parts = [p.strip() for p in line.split("\t", 1)[1].split(";")]
    if not parts[0].endswith("%"):
        return None
    percent = decimal(parts[0][:-1])
    if percent is None:
        return None
    raw = parts[1] if len(parts) > 1 else ""
    state = {"finishing charge": "charging", "AC attached": "not charging"}.get(raw, raw.lower())
    minutes = remaining(parts[2] if len(parts) > 2 else "") if moving(state) else None
    return {"percent": clamp(percent), "state": state, "minutes": minutes}


def remaining(text: str) -> Optional[int]:
    """"4:12 remaining present: true" -> 252."""
    if " remaining" not in text:
        return None
    h, sep, m = text.split(" remaining", 1)[0].strip().partition(":")
    if not (sep and re.fullmatch(r"\d+", h, re.ASCII) and re.fullmatch(r"\d+", m, re.ASCII)):
        return None
    return int(h) * 60 + int(m)


def moving(state: str) -> bool:
    """Only charging and discharging have a time worth showing; pmset says "0:00 remaining" once charged."""
    return state in ("charging", "discharging")


def read_sysfs(folder: str) -> Reading:
    """Reads a /sys/class/power_supply folder: one folder per supply, each with a `type` file.
    The first battery (by name) is shown. A battery whose `scope` is `Device` belongs to a
    mouse, keyboard or other peripheral and is skipped. No folder, or no battery in it, reads
    as AC power with no battery."""
    try:
        names = sorted(os.listdir(folder))
    except OSError:
        names = []
    ac = False
    battery: Optional[Charge] = None
    for name in names:
        def file(f: str) -> Optional[str]:
            try:
                with open(os.path.join(folder, name, f), encoding="utf-8") as fh:
                    return fh.read().strip()
            except (OSError, UnicodeDecodeError):
                return None

        def number(f: str) -> Optional[float]:
            return decimal(file(f))

        if file("type") != "Battery":
            if file("online") == "1":
                ac = True
            continue
        if battery is not None or file("present") == "0" or file("scope") == "Device":
            continue
        status = file("status")
        state = "charged" if status == "Full" else "unknown" if status is None else status.lower()
        # Energy in µWh and µW, or charge in µAh and µA: either works for a ratio.
        now = number("energy_now")
        if now is not None:
            full, rate = number("energy_full"), number("power_now")
        else:
            now, full, rate = number("charge_now"), number("charge_full"), number("current_now")
        capacity = number("capacity")
        percent = 0.0
        if capacity is not None:
            percent = capacity
        elif now is not None and full is not None and full > 0:
            percent = float(math.floor(now / full * 100 + 0.5))
        minutes = None
        # Some drivers report the rate as negative while discharging.
        if rate is not None:
            rate = abs(rate)
        if rate is not None and rate > 0:
            if state == "discharging" and now is not None:
                minutes = math.floor(now * 60 / rate)
            elif state == "charging" and now is not None and full is not None:
                minutes = math.floor(max(0.0, full - now) * 60 / rate)
        battery = {"percent": clamp(percent), "state": state, "minutes": minutes}
    return {"source": "battery" if not ac and battery is not None else "AC power", "battery": battery}


def stat(label: str, value: str) -> Widget:
    return {"type": "stat", "label": label, "value": value}


def view(r: Reading, low: float, critical: float) -> View:
    """The tile, built exactly like the native module's."""
    b = r["battery"]
    if b is None:
        return {
            "status": "ok",
            "summary": f"{r['source']} · no battery",
            "root": {"type": "text", "text": f"No battery found. Running on {r['source']}.", "markdown": False},
        }
    pct = int(b["percent"])
    on_battery = b["state"] == "discharging"
    status = "error" if on_battery and b["percent"] <= critical else "warn" if on_battery and b["percent"] <= low else "ok"
    m = b["minutes"]
    time = "-" if m is None else f"{m // 60}:{m % 60:02d}"
    summary = f"{pct}% · {b['state']}"
    if m is not None and b["state"] == "discharging":
        summary += f" · {time} left"
    if m is not None and b["state"] == "charging":
        summary += f" · {time} to full"
    return {
        "status": status,
        "summary": summary,
        "root": {
            "type": "column",
            "children": [
                {"type": "gauge", "label": "Battery", "percent": b["percent"], "caption": f"on {r['source']}"},
                {
                    "type": "row",
                    "children": [
                        stat("Power", r["source"]),
                        stat("State", b["state"]),
                        stat("To full" if b["state"] == "charging" else "Time left", time),
                    ],
                },
            ],
        },
    }


def init(c):
    config.update(with_defaults(c, config))
    if isinstance(config["sample"], str):
        config["sample"] = os.path.expanduser(config["sample"])


def render() -> View:
    if config["sample"] is not None:
        reading = read_path(config["sample"])
    elif sys.platform == "darwin":
        reading = parse_pmset(pmset())
    elif sys.platform.startswith("linux"):
        reading = read_sysfs(SYSFS)
    else:
        raise upstream_error(f"battery info is not available on {sys.platform}")
    return view(reading, config["low"], config["critical"])


serve(
    manifest={
        "id": "battery-py",
        "name": "Battery (Python)",
        "refresh": {"mode": "poll", "interval_ms": 30000, "min_interval_ms": 5000},
        "layout": {"size": "small", "priority": 20, "min_size": "small"},
    },
    init=init,
    render=render,
)
