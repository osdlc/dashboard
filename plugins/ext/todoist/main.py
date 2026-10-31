#!/usr/bin/env python3
"""Todoist: tasks matching a Todoist filter, by default what's due today or overdue. Untested
live: built from Todoist's API v1 docs (GET /api/v1/tasks/filter) and checked against
sample.json.

    modules:
      todo:
        package: todoist
        config: { filter: "today | overdue | p1" }   # token in $TODOIST_TOKEN

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
from view import SAMPLE_TODAY, view  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
SAMPLE = "--sample" in sys.argv[1:]
# How long one request may take, in seconds.
TIMEOUT = 20

config: Dict[str, Any] = {
    "filter": "today | overdue",
    "limit": 10,
    "token_env": "TODOIST_TOKEN",
    "token_command": None,
}


def token() -> str:
    return apitoken.secret(config, "token_env", "token_command")


def tasks() -> List[Dict[str, Any]]:
    if SAMPLE:
        with open(os.path.join(HERE, "sample.json"), encoding="utf-8") as f:
            return json.load(f)["results"]
    # 200 is the API's page size; one page is plenty for a tile.
    query = urllib.parse.urlencode({"query": config["filter"], "limit": "200"})
    url = f"https://api.todoist.com/api/v1/tasks/filter?{query}"
    req = urllib.request.Request(url, headers={"Authorization": f"Bearer {token()}"})
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as res:
            return json.load(res)["results"]
    except urllib.error.HTTPError as e:
        if e.code in (401, 403):
            apitoken.forget()
            raise upstream_error(f"Todoist rejected the token (HTTP {e.code})") from None
        if e.code == 400:
            raise upstream_error(f"Todoist did not accept the filter {json.dumps(config['filter'])}") from None
        raise upstream_error(f"Todoist: HTTP {e.code}") from None
    except KeyError:
        raise upstream_error("Todoist: unexpected response (no `results`)") from None
    except (urllib.error.URLError, OSError, ValueError) as e:
        raise upstream_error(f"Todoist unreachable: {getattr(e, 'reason', e)}") from None


def init(c: Dict[str, Any]) -> None:
    global config
    config = with_defaults(c, config)
    apitoken.check(config, "token_env", "token_command")
    if not isinstance(config["filter"], str) or not config["filter"].strip():
        raise config_error("filter is empty")
    limit = config["limit"]
    if not isinstance(limit, int) or isinstance(limit, bool) or limit < 1:
        raise config_error("limit must be at least 1")


def render() -> View:
    # Today's date where the dashboard runs.
    today = SAMPLE_TODAY if SAMPLE else datetime.date.today().isoformat()
    return view(tasks(), today, config["limit"])


serve(
    manifest={
        "id": "todoist",
        "name": "Todoist",
        "refresh": {"mode": "poll", "interval_ms": 120000, "min_interval_ms": 30000},
        "layout": {"size": "medium", "priority": 20},
    },
    init=init,
    render=render,
)
