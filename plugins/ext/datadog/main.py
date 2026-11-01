#!/usr/bin/env python3
"""Datadog monitors: how many are alerting, warning or without data, and a list of the ones
that need a look. Built from the public API docs (GET /api/v1/monitor); untested live.

    modules:
      datadog:
        package: datadog
        config: { site: datadoghq.eu, tags: ["team:web"] }

Needs an API key and an application key: $DD_API_KEY and $DD_APP_KEY, or commands that
print them (token_command, app_key_command).
"""

import re
import urllib.parse
from typing import Any, Dict, List, Optional

from dashboard_plugin import ListItem, Status, View, config_error, serve, upstream_error, with_defaults
from ops import ago, base_url, count, env_name, get_json, join, parse_time, plural, read_token, strings

DEFAULTS: Dict[str, Any] = {
    "site": "datadoghq.com",
    "base_url": None,
    "tags": None,
    "name": None,
    "show_ok": False,
    "limit": 10,
    "token_env": "DD_API_KEY",
    "token_command": None,
    "app_key_env": "DD_APP_KEY",
    "app_key_command": None,
}
config = dict(DEFAULTS)
api = ""
app = ""

# Most urgent first.
ORDER = ["Alert", "Warn", "No Data", "Unknown", "Skipped", "Ignored", "OK"]


def state(m: Dict[str, Any]) -> str:
    s = m.get("overall_state")
    return s if isinstance(s, str) else "Unknown"


def item_status(s: str) -> Status:
    if s == "Alert":
        return "error"
    if s == "Warn":
        return "warn"
    if s == "OK":
        return "ok"
    return "unknown"


def rank(m: Dict[str, Any]) -> tuple:
    s = state(m)
    return (ORDER.index(s) if s in ORDER else len(ORDER), -(parse_time(m.get("overall_state_modified")) or 0))


def view(monitors: List[Dict[str, Any]], now: Optional[float] = None) -> View:
    """The view for a list of monitors, as GET /api/v1/monitor returns them."""

    def n(s: str) -> int:
        return sum(1 for m in monitors if state(m) == s)

    alert, warn, no_data, ok = n("Alert"), n("Warn"), n("No Data"), n("OK")
    shown = sorted((m for m in monitors if config["show_ok"] or state(m) != "OK"), key=rank)
    items: List[ListItem] = []
    for m in shown[: config["limit"]]:
        changed = m.get("overall_state_modified")
        items.append(
            {
                "title": str(m.get("name") or f"monitor {m.get('id')}"),
                "subtitle": join([state(m), m.get("type"), changed and ago(changed, now)]),
                "status": item_status(state(m)),
                "tags": [t for t in (m.get("tags") or []) if isinstance(t, str)][:2],
                "action": {"type": "open_url", "url": f"{app}/monitors/{m.get('id')}"},
            }
        )
    status: Status = "error" if alert else "warn" if warn + no_data else "ok"
    summary = join([alert and f"{alert} alert", warn and f"{warn} warn", no_data and f"{no_data} no data", f"{ok} ok"])
    return {
        "status": status,
        "summary": summary,
        "root": {
            "type": "column",
            "children": [
                {
                    "type": "row",
                    "children": [
                        {"type": "stat", "label": "Alert", "value": str(alert), "status": "error" if alert else "ok"},
                        {"type": "stat", "label": "Warn", "value": str(warn), "status": "warn" if warn else "ok"},
                        {"type": "stat", "label": "No data", "value": str(no_data), "status": "warn" if no_data else "ok"},
                        {"type": "stat", "label": "OK", "value": str(ok)},
                    ],
                },
                {"type": "list", "items": items, "empty_text": f"All {plural(len(monitors), 'monitor')} OK"},
            ],
        },
    }


def init(c: Any) -> None:
    global api, app
    config.update(with_defaults(c, DEFAULTS))
    site = config["site"]
    if not isinstance(site, str) or not re.match(r"^[a-z0-9.-]+$", site):
        raise config_error("site must be a Datadog site such as datadoghq.com or datadoghq.eu")
    # us3.datadoghq.com is its own app host; datadoghq.com and datadoghq.eu use app.<site>.
    app = f"https://{site}" if len(site.split(".")) > 2 else f"https://app.{site}"
    api = f"https://api.{site}" if config["base_url"] is None else base_url(config["base_url"], "base_url")
    strings(config["tags"], "tags")
    strings(config["token_command"], "token_command")
    strings(config["app_key_command"], "app_key_command")
    count(config["limit"], "limit")
    env_name(config["token_env"], "token_env")
    env_name(config["app_key_env"], "app_key_env")
    if config["name"] is not None and not isinstance(config["name"], str):
        raise config_error("name must be text")
    if not isinstance(config["show_ok"], bool):
        raise config_error("show_ok must be true or false")


def render() -> View:
    api_key = read_token(config["token_env"], config["token_command"], "Datadog API key")
    app_key = read_token(config["app_key_env"], config["app_key_command"], "Datadog application key")
    query = {}
    if config["tags"]:
        query["monitor_tags"] = ",".join(config["tags"])
    if config["name"]:
        query["name"] = config["name"]
    url = f"{api}/api/v1/monitor" + (f"?{urllib.parse.urlencode(query)}" if query else "")
    monitors = get_json("Datadog", url, {"DD-API-KEY": api_key, "DD-APPLICATION-KEY": app_key})
    if not isinstance(monitors, list):
        raise upstream_error("Datadog: expected a list of monitors")
    return view([m for m in monitors if isinstance(m, dict)])


if __name__ == "__main__":
    serve(
        manifest={
            "id": "datadog",
            "name": "Datadog monitors",
            "refresh": {"mode": "poll", "interval_ms": 60000, "min_interval_ms": 30000},
            "layout": {"size": "medium", "priority": 0, "min_size": "small"},
        },
        init=init,
        render=render,
    )
