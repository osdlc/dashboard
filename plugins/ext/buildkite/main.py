#!/usr/bin/env python3
"""Buildkite: recent builds for an organization or one pipeline, with passed, failed and
running counts. Untested live: built from Buildkite's REST API docs
(GET /v2/organizations/{org}/builds) and checked against sample.json.

    modules:
      builds:
        package: buildkite
        config: { org: acme, pipeline: web, branch: main }   # token in $BUILDKITE_TOKEN

`python3 main.py --sample` shows sample.json instead of calling the API (the tests and the
catalog demo use it).
"""

import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Dict, List

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import apitoken  # noqa: E402
from dashboard_plugin import View, config_error, serve, upstream_error, with_defaults  # noqa: E402
from view import SAMPLE_NOW, view  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
SAMPLE = "--sample" in sys.argv[1:]
# How long one request may take, in seconds.
TIMEOUT = 20

config: Dict[str, Any] = {
    "org": "",
    "pipeline": None,
    "branch": None,
    "limit": 10,
    "token_env": "BUILDKITE_TOKEN",
    "token_command": None,
}


def token() -> str:
    return apitoken.secret(config, "token_env", "token_command")


def builds() -> List[Dict[str, Any]]:
    if SAMPLE:
        with open(os.path.join(HERE, "sample.json"), encoding="utf-8") as f:
            return json.load(f)
    org = urllib.parse.quote(config["org"], safe="")
    path = f"organizations/{org}/builds"
    if config["pipeline"]:
        path = f"organizations/{org}/pipelines/{urllib.parse.quote(config['pipeline'], safe='')}/builds"
    query = {"per_page": str(config["limit"])}
    if config["branch"]:
        query["branch"] = config["branch"]
    url = f"https://api.buildkite.com/v2/{path}?{urllib.parse.urlencode(query)}"
    req = urllib.request.Request(url, headers={"Authorization": f"Bearer {token()}"})
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as res:
            return json.load(res)
    except urllib.error.HTTPError as e:
        if e.code == 401:
            apitoken.forget()
            raise upstream_error("Buildkite rejected the token (HTTP 401)") from None
        raise upstream_error(f"Buildkite: HTTP {e.code} for /v2/{path}") from None
    except (urllib.error.URLError, OSError, ValueError) as e:
        raise upstream_error(f"Buildkite unreachable: {getattr(e, 'reason', e)}") from None


def init(c: Dict[str, Any]) -> None:
    global config
    config = with_defaults(c, config)
    apitoken.check(config, "token_env", "token_command")
    if not SAMPLE and not config["org"]:
        raise config_error("set `org:` to your Buildkite organization slug")
    limit = config["limit"]
    if not isinstance(limit, int) or isinstance(limit, bool) or limit < 1:
        raise config_error("limit must be at least 1")


def render() -> View:
    return view(builds(), SAMPLE_NOW if SAMPLE else time.time())


serve(
    manifest={
        "id": "buildkite",
        "name": "Buildkite",
        "refresh": {"mode": "poll", "interval_ms": 60000, "min_interval_ms": 15000},
        "layout": {"size": "medium", "priority": 30},
    },
    init=init,
    render=render,
)
