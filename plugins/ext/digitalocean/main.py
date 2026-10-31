#!/usr/bin/env python3
"""DigitalOcean: your droplets with their state, size, region and public IP, and what they
cost a month. Click one to open it in the control panel.

Untested live: built from DigitalOcean's API docs (GET /v2/droplets) with a hand-written
sample reply. A read-only personal access token is enough.

    modules:
      droplets:
        package: digitalocean
        config: { tag: prod, token_env: DIGITALOCEAN_TOKEN }

`--sample` shows sample.json instead of calling the API:

    dashboard plugin-check -- python3 plugins/ext/digitalocean/main.py --sample
"""

import os
import sys
import urllib.parse
from typing import Any, Dict

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import apitoken  # noqa: E402
from dashboard_plugin import View, config_error, serve, with_defaults  # noqa: E402
from token_api import check_limit, read_sample, request_json  # noqa: E402
from view import view  # noqa: E402

API = "https://api.digitalocean.com/v2/droplets"
SAMPLE = "--sample" in sys.argv[1:]

config: Dict[str, Any] = {
    "tag": None,
    "limit": 20,
    "token_env": "DIGITALOCEAN_TOKEN",
    "token_command": None,
}


def init(c: Any) -> None:
    config.update(with_defaults(c, config))
    if config["tag"] is not None and (not isinstance(config["tag"], str) or not config["tag"]):
        raise config_error("tag: expected a tag name")
    check_limit(config, 200)
    apitoken.check(config, "token_env", "token_command")


def render() -> View:
    if SAMPLE:
        return view(read_sample(os.path.join(HERE, "sample.json")), config["limit"])
    token = apitoken.secret(config, "token_env", "token_command")
    # One page of up to 200 covers the count and cost for most accounts.
    query = {"per_page": 200}
    if config["tag"]:
        query["tag_name"] = config["tag"]
    reply = request_json(f"{API}?{urllib.parse.urlencode(query)}", {"Authorization": f"Bearer {token}"})
    return view(reply, config["limit"])


serve(
    manifest={
        "id": "digitalocean",
        "name": "DigitalOcean",
        "refresh": {"mode": "poll", "interval_ms": 300000, "min_interval_ms": 60000},
        "layout": {"size": "medium", "priority": 30, "min_size": "small"},
    },
    init=init,
    render=render,
)
