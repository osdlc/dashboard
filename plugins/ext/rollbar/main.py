#!/usr/bin/env python3
"""Rollbar: a project's active items (errors and messages), most recent first, with their
level, occurrences, environment and age. Click one to open it in Rollbar.

Untested live: built from Rollbar's API docs (GET /api/1/items) with a hand-written sample
reply. It needs a project access token with read scope.

    modules:
      rollbar:
        package: rollbar
        config: { account: acme, project: web, token_env: ROLLBAR_TOKEN }

`--sample` shows sample.json instead of calling the API:

    dashboard plugin-check -- python3 plugins/ext/rollbar/main.py --sample
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
from token_api import check_limit, read_sample, request_json  # noqa: E402
from view import LEVELS, SAMPLE_NOW, view  # noqa: E402

API = "https://api.rollbar.com/api/1/items"
SAMPLE = "--sample" in sys.argv[1:]

config: Dict[str, Any] = {
    "account": None,
    "project": None,
    "environment": None,
    "level": "warning",
    "limit": 10,
    "token_env": "ROLLBAR_TOKEN",
    "token_command": None,
}


def init(c: Any) -> None:
    if SAMPLE:
        c = {"account": "acme", "project": "web", **(c or {})}
    config.update(with_defaults(c, config))
    for key in ("account", "project"):
        if not isinstance(config[key], str) or not config[key]:
            raise config_error(f"{key}: the Rollbar {key} slug from its URL, rollbar.com/<account>/<project>")
    if config["environment"] is not None and not isinstance(config["environment"], str):
        raise config_error("environment: expected a name, e.g. production")
    if config["level"] not in LEVELS:
        raise config_error(f"level: expected one of {', '.join(LEVELS)}")
    check_limit(config, 50)
    apitoken.check(config, "token_env", "token_command")


def render() -> View:
    if SAMPLE:
        reply = read_sample(os.path.join(HERE, "sample.json"))
        now = SAMPLE_NOW
    else:
        token = apitoken.secret(config, "token_env", "token_command")
        query = {"status": "active"}
        if config["environment"]:
            query["environment"] = config["environment"]
        reply = request_json(f"{API}?{urllib.parse.urlencode(query)}", {"X-Rollbar-Access-Token": token})
        now = int(time.time())
    return view(reply, config["account"], config["project"], config["level"], config["limit"], now)


serve(
    manifest={
        "id": "rollbar",
        "name": "Rollbar",
        "refresh": {"mode": "poll", "interval_ms": 120000, "min_interval_ms": 30000},
        "layout": {"size": "medium", "priority": 30, "min_size": "small"},
    },
    init=init,
    render=render,
)
