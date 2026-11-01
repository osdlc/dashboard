#!/usr/bin/env python3
"""UptimeRobot: your monitors with their status, how long they have been up or down, the
30-day uptime and the latest response time. Click one to open it in UptimeRobot.

Untested live: built from UptimeRobot's API v2 docs (POST /v2/getMonitors) with a
hand-written sample reply. Use the read-only API key (Integrations & API > API).

    modules:
      uptimerobot:
        package: uptimerobot
        config: { token_env: UPTIMEROBOT_API_KEY }

`--sample` shows sample.json instead of calling the API:

    dashboard plugin-check -- python3 plugins/ext/uptimerobot/main.py --sample
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

API = "https://api.uptimerobot.com/v2/getMonitors"
SAMPLE = "--sample" in sys.argv[1:]

config: Dict[str, Any] = {
    "monitors": None,
    "days": 30,
    "limit": 20,
    "token_env": "UPTIMEROBOT_API_KEY",
    "token_command": None,
}


def init(c: Any) -> None:
    config.update(with_defaults(c, config))
    ids = config["monitors"]
    if ids is not None and (not isinstance(ids, list) or not all(is_int(i) for i in ids)):
        raise config_error("monitors: expected a list of monitor ids (numbers)")
    if not is_int(config["days"]) or not 1 <= config["days"] <= 90:
        raise config_error("days: expected a whole number from 1 to 90")
    check_limit(config, 50)
    apitoken.check(config, "token_env", "token_command")


def render() -> View:
    if SAMPLE:
        return view(read_sample(os.path.join(HERE, "sample.json")), config["limit"], SAMPLE_NOW)
    key = apitoken.secret(config, "token_env", "token_command")
    form = {
        "api_key": key,
        "format": "json",
        "custom_uptime_ratios": str(config["days"]),
        "response_times": "1",
        "response_times_limit": "1",
        "logs": "1",
        "logs_limit": "5",
    }
    if config["monitors"]:
        form["monitors"] = "-".join(str(i) for i in config["monitors"])
    data = urllib.parse.urlencode(form).encode()
    reply = request_json(API, {"Content-Type": "application/x-www-form-urlencoded"}, data)
    return view(reply, config["limit"], int(time.time()))


serve(
    manifest={
        "id": "uptimerobot",
        "name": "UptimeRobot",
        "refresh": {"mode": "poll", "interval_ms": 60000, "min_interval_ms": 30000},
        "layout": {"size": "medium", "priority": 30, "min_size": "small"},
    },
    init=init,
    render=render,
)
