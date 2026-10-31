#!/usr/bin/env python3
"""Zendesk: tickets from a search (by default the unsolved ones assigned to you), with
status, priority, tags and when they were last updated. Click one to open it in Zendesk.

Untested live: built from Zendesk's API docs (GET /api/v2/search.json) with a hand-written
sample reply. It signs in with your email and an API token (Admin Center > Apps and
integrations > Zendesk API).

    modules:
      zendesk:
        package: zendesk
        config: { subdomain: acme, email: me@acme.com, token_env: ZENDESK_API_TOKEN }

`--sample` shows sample.json instead of calling the API:

    dashboard plugin-check -- python3 plugins/ext/zendesk/main.py --sample
"""

import base64
import os
import re
import sys
import time
import urllib.parse
from typing import Any, Dict

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import apitoken  # noqa: E402
from dashboard_plugin import View, config_error, serve, with_defaults  # noqa: E402
from token_api import check_limit, read_sample, request_json  # noqa: E402
from view import SAMPLE_NOW, view  # noqa: E402

SAMPLE = "--sample" in sys.argv[1:]

config: Dict[str, Any] = {
    "subdomain": None,
    "email": None,
    "query": "type:ticket status<solved assignee:me",
    "limit": 10,
    "token_env": "ZENDESK_API_TOKEN",
    "token_command": None,
}


def init(c: Any) -> None:
    if SAMPLE:
        c = {"subdomain": "acme", "email": "me@acme.com", **(c or {})}
    config.update(with_defaults(c, config))
    sub = config["subdomain"]
    if not isinstance(sub, str) or not re.fullmatch(r"[a-z0-9][a-z0-9-]*", sub):
        raise config_error("subdomain: the part before .zendesk.com, e.g. acme")
    if not isinstance(config["email"], str) or "@" not in config["email"]:
        raise config_error("email: the address you sign in to Zendesk with")
    if not isinstance(config["query"], str) or not config["query"].strip():
        raise config_error("query: expected a Zendesk search, e.g. type:ticket status<solved")
    check_limit(config, 100)
    apitoken.check(config, "token_env", "token_command")


def render() -> View:
    if SAMPLE:
        return view(read_sample(os.path.join(HERE, "sample.json")), config["subdomain"], config["limit"], SAMPLE_NOW)
    token = apitoken.secret(config, "token_env", "token_command")
    auth = base64.b64encode(f"{config['email']}/token:{token}".encode()).decode()
    query = urllib.parse.urlencode(
        {"query": config["query"], "sort_by": "updated_at", "sort_order": "desc", "per_page": config["limit"]}
    )
    url = f"https://{config['subdomain']}.zendesk.com/api/v2/search.json?{query}"
    reply = request_json(url, {"Authorization": f"Basic {auth}"})
    return view(reply, config["subdomain"], config["limit"], int(time.time()))


serve(
    manifest={
        "id": "zendesk",
        "name": "Zendesk",
        "refresh": {"mode": "poll", "interval_ms": 120000, "min_interval_ms": 30000},
        "layout": {"size": "medium", "priority": 30, "min_size": "small"},
    },
    init=init,
    render=render,
)
