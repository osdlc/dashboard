#!/usr/bin/env python3
"""Airbrake: a project's unresolved error groups, most recent first, with notice counts,
environment, where it happened and age. Click one to open it in Airbrake.

Untested live: built from Airbrake's API v4 docs (GET /api/v4/projects/<id>/groups) with a
hand-written sample reply. It needs a user API key (Profile > API key).

    modules:
      airbrake:
        package: airbrake
        config: { project_id: 95123, token_env: AIRBRAKE_USER_KEY }

`--sample` shows sample.json instead of calling the API:

    dashboard plugin-check -- python3 plugins/ext/airbrake/main.py --sample
"""

import os
import sys
import time
import urllib.parse
from typing import Any, Dict

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import apitoken  # noqa: E402
from dashboard_plugin import View, config_error, serve, with_defaults  # noqa: E402
from token_api import check_limit, is_int, read_sample, request_json  # noqa: E402
from view import SAMPLE_NOW, view  # noqa: E402

API = "https://api.airbrake.io/api/v4/projects"
SAMPLE = "--sample" in sys.argv[1:]

config: Dict[str, Any] = {
    "project_id": None,
    "limit": 10,
    "token_env": "AIRBRAKE_USER_KEY",
    "token_command": None,
}


def init(c: Any) -> None:
    if SAMPLE:
        c = {"project_id": 95123, **(c or {})}
    config.update(with_defaults(c, config))
    if not is_int(config["project_id"]) or config["project_id"] < 1:
        raise config_error("project_id: the Airbrake project's number, from its URL")
    check_limit(config, 50)
    apitoken.check(config, "token_env", "token_command")


def render() -> View:
    if SAMPLE:
        reply = read_sample(os.path.join(HERE, "sample.json"))
        now = SAMPLE_NOW
    else:
        key = apitoken.secret(config, "token_env", "token_command")
        query = urllib.parse.urlencode({"limit": config["limit"], "order": "last_notice"})
        url = f"{API}/{config['project_id']}/groups?{query}"
        reply = request_json(url, {"Authorization": f"Bearer {key}"})
        now = int(time.time())
    return view(reply, config["project_id"], config["limit"], now)


serve(
    manifest={
        "id": "airbrake",
        "name": "Airbrake",
        "refresh": {"mode": "poll", "interval_ms": 120000, "min_interval_ms": 30000},
        "layout": {"size": "medium", "priority": 30, "min_size": "small"},
    },
    init=init,
    render=render,
)
