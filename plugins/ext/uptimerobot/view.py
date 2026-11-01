"""UptimeRobot monitors to a view. No I/O here, so the tests can feed it sample.json."""

from typing import Any, Dict, List, Optional

from dashboard_plugin import UptimeCheck, View, upstream_error
from token_api import is_int, plural

# The time the sample ages are measured from (2026-10-27T02:00:00Z), so `--sample` shows
# the same view whenever it runs.
SAMPLE_NOW = 1793066400

# Monitor status: 0 paused, 1 not checked yet, 2 up, 8 seems down, 9 down.
STATUS = {0: "unknown", 1: "unknown", 2: "ok", 8: "warn", 9: "error"}
# Log type: 1 down, 2 up, 98 started, 99 paused.
LOG_DOWN, LOG_UP = 1, 2


def span(secs: int) -> str:
    """ "23m", "4h", "3d"."""
    s = max(0, secs)
    if s < 3600:
        return f"{max(1, s // 60)}m"
    if s < 86400:
        return f"{s // 3600}h"
    return f"{s // 86400}d"


def number(v: Any) -> Optional[float]:
    """The API sends ratios and times as strings ("99.984"), sometimes as numbers."""
    if isinstance(v, (int, float)) and not isinstance(v, bool):
        return float(v)
    if isinstance(v, str):
        try:
            # Several ratios come as "99.98-100.000"; the first is the one asked for.
            return float(v.split("-")[0])
        except ValueError:
            return None
    return None


def newest_log(m: Dict[str, Any], kind: int) -> Optional[Dict[str, Any]]:
    logs = m.get("logs") if isinstance(m.get("logs"), list) else []
    found = [entry for entry in logs if isinstance(entry, dict) and entry.get("type") == kind and is_int(entry.get("datetime"))]
    return max(found, key=lambda entry: entry["datetime"]) if found else None


def check(m: Dict[str, Any], now: int) -> UptimeCheck:
    code = m.get("status")
    status = STATUS.get(code, "unknown") if is_int(code) else "unknown"
    if code == 0:
        detail = "paused"
    elif code == 1:
        detail = "not checked yet"
    elif status == "ok":
        up = newest_log(m, LOG_UP)
        detail = f"up for {span(now - up['datetime'])}" if up else "up"
    else:
        down = newest_log(m, LOG_DOWN)
        word = "seems down" if code == 8 else "down"
        detail = f"{word} for {span(now - down['datetime'])}" if down else word
        reason = down.get("reason") if down else None
        if isinstance(reason, dict) and isinstance(reason.get("detail"), str) and reason["detail"]:
            detail += f": {reason['detail']}"
    out: UptimeCheck = {
        "name": str(m.get("friendly_name") or m.get("url") or m.get("id")),
        "status": status,  # type: ignore[typeddict-item]
        "detail": detail,
        "history": [],
    }
    ratio = number(m.get("custom_uptime_ratio"))
    if ratio is not None and code != 0:
        out["uptime"] = ratio
    times = m.get("response_times")
    latest = times[0] if isinstance(times, list) and times and isinstance(times[0], dict) else None
    ms = number(latest.get("value")) if latest else None
    if ms is not None and status == "ok":
        out["latency_ms"] = ms
    if is_int(m.get("id")):
        out["url"] = f"https://dashboard.uptimerobot.com/monitors/{m['id']}"
    return out


def view(reply: Any, limit: int, now: int) -> View:
    """The tile: one row per monitor, the ones that are down first."""
    if isinstance(reply, dict) and reply.get("stat") == "fail":
        err = reply.get("error") if isinstance(reply.get("error"), dict) else {}
        raise upstream_error(f"UptimeRobot: {err.get('message') or err.get('type') or 'request failed'}")
    monitors: Optional[List[Any]] = reply.get("monitors") if isinstance(reply, dict) else None
    if not isinstance(monitors, list):
        raise upstream_error("unexpected UptimeRobot reply: no monitors")
    checks = [check(m, now) for m in monitors if isinstance(m, dict)]
    order = {"error": 0, "warn": 1, "ok": 2, "unknown": 3}
    checks.sort(key=lambda c: order[c["status"]])
    down = sum(1 for c in checks if c["status"] == "error")
    shaky = sum(1 for c in checks if c["status"] == "warn")
    paused = sum(1 for c in checks if c["status"] == "unknown")
    if down + shaky == 0:
        up = len(checks) - paused
        summary = [f"all {up} up" if up > 1 else f"{up} up" if checks else "no monitors"]
    else:
        summary = [plural(len(checks), "monitor")]
        if down:
            summary.append(f"{down} down")
        if shaky:
            summary.append(f"{shaky} seem down")
    if paused:
        summary.append(f"{paused} paused")
    return {
        "status": "error" if down else "warn" if shaky else "ok",
        "summary": " · ".join(summary),
        "root": {"type": "uptime", "checks": checks[:limit]},
    }
