#!/usr/bin/env python3
"""Healthchecks.io: your cron and heartbeat checks, down and late ones first, with counts of
up, late and down checks. Clicking a check opens its page on Healthchecks.

Untested live: built from the Healthchecks.io Management API v3 docs (GET /api/v3/checks/)
and checked against sample.json. Works with a self-hosted Healthchecks too (`base_url`).

    modules:
      cron:
        plugin: [python3, plugins/ext/healthchecks/main.py]
        config: { tag: prod }        # API key in $HEALTHCHECKS_API_KEY

A read-only API key works, but the API then leaves out each check's id, so clicking a check
opens the site's front page instead of the check. Use a full key for working links.

`python3 main.py --sample` shows sample.json instead of calling the API (the tests and the
catalog demo use it).

    dashboard plugin-check -- python3 plugins/ext/healthchecks/main.py --sample
"""

import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime
from typing import Any, Dict, List, Optional

from dashboard_plugin import ListItem, View, config_error, serve, upstream_error, with_defaults

SAMPLE = "--sample" in sys.argv[1:]
# "Now" for the sample: 2026-10-27T12:00:00Z.
SAMPLE_NOW = 1793102400

config: Dict[str, Any] = {
    "base_url": "https://healthchecks.io",
    "tag": None,
    "limit": 20,
    "token_env": "HEALTHCHECKS_API_KEY",
    "token_command": None,
}

# Per API status: the word shown, the tile status, and the sort order (problems first).
STATES = {
    "down": ("down", "error", 0),
    "grace": ("late", "warn", 1),
    "up": ("up", "ok", 2),
    "new": ("new", "unknown", 3),
    "paused": ("paused", "unknown", 4),
}


def token() -> str:
    """The API key: from token_command if set, else from the token_env variable."""
    cmd = config["token_command"]
    if cmd:
        try:
            out = subprocess.run(cmd, capture_output=True, text=True, timeout=20, check=True).stdout.strip()
        except (OSError, subprocess.SubprocessError):
            raise upstream_error(f"token_command {cmd[0]!r} failed")
        if not out:
            raise upstream_error(f"token_command {cmd[0]!r} printed nothing")
        return out
    value = os.environ.get(config["token_env"], "").strip()
    if not value:
        raise upstream_error(
            f"no API key: set ${config['token_env']} before starting the dashboard, or set token_env / token_command"
        )
    return value


def fetch_checks() -> List[Any]:
    url = config["base_url"] + "/api/v3/checks/"
    if config["tag"]:
        url += "?" + urllib.parse.urlencode({"tag": config["tag"]})
    req = urllib.request.Request(url, headers={"X-Api-Key": token(), "User-Agent": "dashboard-plugin healthchecks"})
    try:
        with urllib.request.urlopen(req, timeout=20) as resp:
            data = json.load(resp)
    except urllib.error.HTTPError as e:
        if e.code in (401, 403):
            raise upstream_error(f"Healthchecks rejected the API key (HTTP {e.code})")
        raise upstream_error(f"Healthchecks: HTTP {e.code}")
    except (OSError, ValueError) as e:
        raise upstream_error(f"Healthchecks unreachable: {e}")
    return data.get("checks", []) if isinstance(data, dict) else []


def read_sample() -> List[Any]:
    with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "sample.json"), encoding="utf-8") as f:
        return json.load(f)["checks"]


def parse_time(ts: Any) -> Optional[int]:
    """Unix seconds from an ISO 8601 time like "2026-10-27T11:59:00+00:00", or None."""
    if not isinstance(ts, str) or not ts:
        return None
    try:
        return int(datetime.fromisoformat(ts.replace("Z", "+00:00")).timestamp())
    except ValueError:
        return None


def ago(secs: int) -> str:
    """ "5m ago" style age from seconds."""
    s = max(0, secs)
    if s < 60:
        return "just now"
    if s < 3600:
        return f"{s // 60}m ago"
    if s < 86400:
        return f"{s // 3600}h ago"
    return f"{s // 86400}d ago"


def state(check: Dict[str, Any]) -> tuple:
    word, status, rank = STATES.get(check.get("status"), ("unknown", "unknown", 3))
    if check.get("started") and check.get("status") == "up":
        word = "running"
    return word, status, rank


def item(check: Dict[str, Any], base: str, now: int) -> ListItem:
    word, status, _ = state(check)
    last = parse_time(check.get("last_ping"))
    sub = [word, "never pinged" if last is None else f"last ping {ago(now - last)}"]
    # Read-only keys get `unique_key` instead of `uuid`, and no page URL is built from it.
    uuid = check.get("uuid")
    link = f"{base}/checks/{uuid}/details/" if isinstance(uuid, str) and uuid else f"{base}/"
    tags = check.get("tags")
    return {
        "title": check.get("name") or check.get("slug") or "unnamed check",
        "subtitle": " · ".join(sub),
        "status": status,
        "tags": tags.split() if isinstance(tags, str) else [],
        "action": {"type": "open_url", "url": link},
    }


def view(checks: List[Any], base: str, limit: int, now: int) -> View:
    """The tile: up, late and down counts, then the checks, problems first. `now` is unix seconds."""
    checks = sorted((c for c in checks if isinstance(c, dict)), key=lambda c: state(c)[2])
    down = sum(1 for c in checks if c.get("status") == "down")
    late = sum(1 for c in checks if c.get("status") == "grace")
    up = sum(1 for c in checks if c.get("status") == "up")
    status = "error" if down else "warn" if late else "ok"
    total = f"{len(checks)} check" + ("" if len(checks) == 1 else "s")
    if down or late:
        parts = ([f"{down} down"] if down else []) + ([f"{late} late"] if late else [])
        summary = " · ".join(parts + [total])
    else:
        summary = f"{total} · none down"
    return {
        "status": status,
        "summary": summary,
        "root": {
            "type": "column",
            "children": [
                {
                    "type": "row",
                    "children": [
                        {"type": "stat", "label": "Up", "value": str(up), "status": "ok"},
                        {"type": "stat", "label": "Late", "value": str(late), "status": "warn" if late else "ok"},
                        {"type": "stat", "label": "Down", "value": str(down), "status": "error" if down else "ok"},
                    ],
                },
                {
                    "type": "list",
                    "items": [item(c, base, now) for c in checks[:limit]],
                    "empty_text": "no checks",
                },
            ],
        },
    }


def init(c):
    config.update(with_defaults(c, config))
    base = config["base_url"]
    if not isinstance(base, str) or not base.startswith(("http://", "https://")):
        raise config_error("base_url: must start with http:// or https://")
    config["base_url"] = base.rstrip("/")
    if config["tag"] is not None and (not isinstance(config["tag"], str) or not config["tag"].strip()):
        raise config_error("tag: expected a tag name")
    limit = config["limit"]
    if not isinstance(limit, int) or isinstance(limit, bool) or limit < 1:
        raise config_error("limit: expected a whole number, 1 or more")
    if not isinstance(config["token_env"], str) or not config["token_env"]:
        raise config_error("token_env: expected a variable name")
    cmd = config["token_command"]
    if cmd is not None and (not isinstance(cmd, list) or not cmd or not all(isinstance(a, str) for a in cmd)):
        raise config_error("token_command: expected a list of strings, e.g. [pass, healthchecks]")


def render() -> View:
    if SAMPLE:
        return view(read_sample(), config["base_url"], config["limit"], SAMPLE_NOW)
    return view(fetch_checks(), config["base_url"], config["limit"], int(time.time()))


if __name__ == "__main__":
    serve(
        manifest={
            "id": "healthchecks",
            "name": "Healthchecks",
            "refresh": {"mode": "poll", "interval_ms": 120000, "min_interval_ms": 30000},
            "layout": {"size": "medium", "priority": 30, "min_size": "small"},
        },
        init=init,
        render=render,
    )
