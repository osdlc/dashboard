#!/usr/bin/env python3
"""Asana: your open tasks in a workspace, or every open task in one project. Untested live:
built from Asana's API docs (GET /api/1.0/tasks) and checked against sample.json.

    modules:
      asana-tasks:
        package: asana
        config: { workspace: "1208000000000000" }   # token in $ASANA_TOKEN

`python3 main.py --sample` shows sample.json instead of calling the API (the tests and the
catalog demo use it).
"""

import datetime
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Dict, List

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import apitoken  # noqa: E402
from dashboard_plugin import View, config_error, serve, upstream_error, with_defaults  # noqa: E402
from view import FIELDS, PAGE, SAMPLE_TODAY, view  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
SAMPLE = "--sample" in sys.argv[1:]
# How long one request may take, in seconds.
TIMEOUT = 20

config: Dict[str, Any] = {
    "workspace": None,
    "project": None,
    "limit": 10,
    "token_env": "ASANA_TOKEN",
    "token_command": None,
}


def token() -> str:
    return apitoken.secret(config, "token_env", "token_command")


def tasks() -> List[Dict[str, Any]]:
    if SAMPLE:
        with open(os.path.join(HERE, "sample.json"), encoding="utf-8") as f:
            return json.load(f)["data"]
    if config["project"]:
        query = {"project": config["project"]}
    else:
        query = {"assignee": "me", "workspace": config["workspace"]}
    # completed_since=now: only tasks not completed yet.
    query.update({"completed_since": "now", "opt_fields": FIELDS, "limit": str(PAGE)})
    url = "https://app.asana.com/api/1.0/tasks?" + urllib.parse.urlencode(query)
    req = urllib.request.Request(url, headers={"Authorization": f"Bearer {token()}"})
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as res:
            return json.load(res)["data"]
    except urllib.error.HTTPError as e:
        if e.code == 401:
            apitoken.forget()
            raise upstream_error("Asana rejected the token (HTTP 401)") from None
        raise upstream_error(f"Asana: HTTP {e.code}") from None
    except KeyError:
        raise upstream_error("Asana: unexpected response (no `data`)") from None
    except (urllib.error.URLError, OSError, ValueError) as e:
        raise upstream_error(f"Asana unreachable: {getattr(e, 'reason', e)}") from None


def init(c: Dict[str, Any]) -> None:
    global config
    config = with_defaults(c, config)
    apitoken.check(config, "token_env", "token_command")
    limit = config["limit"]
    if not isinstance(limit, int) or isinstance(limit, bool) or limit < 1:
        raise config_error("limit must be at least 1")
    if not SAMPLE and not config["workspace"] and not config["project"]:
        raise config_error("set `workspace:` (its gid, for your tasks) or `project:` (its gid)")


def render() -> View:
    # Today's date where the dashboard runs.
    today = SAMPLE_TODAY if SAMPLE else datetime.date.today().isoformat()
    return view(tasks(), today, config["limit"])


serve(
    manifest={
        "id": "asana",
        "name": "Asana",
        "refresh": {"mode": "poll", "interval_ms": 120000, "min_interval_ms": 30000},
        "layout": {"size": "medium", "priority": 20},
    },
    init=init,
    render=render,
)
