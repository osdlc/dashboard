#!/usr/bin/env python3
"""Jenkins: the jobs on a Jenkins server (or one of its views) with each job's last build.
Untested live: built from Jenkins' remote access API docs (GET /api/json?tree=...) and
checked against sample.json.

    modules:
      jobs:
        package: jenkins
        config: { url: "https://ci.example.com", view: backend, user: dana }   # token in $JENKINS_TOKEN

Without `user:` it calls Jenkins with no login, for servers that allow anonymous reads.
`python3 main.py --sample` shows sample.json instead of calling the server (the tests and the
catalog demo use it).
"""

import base64
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Dict, List

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import apitoken  # noqa: E402
from dashboard_plugin import View, config_error, serve, upstream_error, with_defaults  # noqa: E402
from view import SAMPLE_NOW, TREE, view  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
SAMPLE = "--sample" in sys.argv[1:]
# How long one request may take, in seconds.
TIMEOUT = 20

config: Dict[str, Any] = {
    "url": "",
    "view": None,
    "user": None,
    "token_env": "JENKINS_TOKEN",
    "token_command": None,
}


def token() -> str:
    return apitoken.secret(config, "token_env", "token_command")


def jobs() -> List[Dict[str, Any]]:
    if SAMPLE:
        with open(os.path.join(HERE, "sample.json"), encoding="utf-8") as f:
            return json.load(f)["jobs"]
    path = f"/view/{urllib.parse.quote(config['view'], safe='')}/api/json" if config["view"] else "/api/json"
    url = config["url"].rstrip("/") + path + "?" + urllib.parse.urlencode({"tree": TREE})
    headers = {}
    if config["user"]:
        login = base64.b64encode(f"{config['user']}:{token()}".encode()).decode()
        headers["Authorization"] = f"Basic {login}"
    try:
        with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=TIMEOUT) as res:
            return json.load(res).get("jobs") or []
    except urllib.error.HTTPError as e:
        if e.code in (401, 403):
            apitoken.forget()
            raise upstream_error(f"Jenkins refused the request (HTTP {e.code}): check user and token") from None
        raise upstream_error(f"Jenkins: HTTP {e.code} for {path}") from None
    except (urllib.error.URLError, OSError, ValueError) as e:
        raise upstream_error(f"Jenkins unreachable: {getattr(e, 'reason', e)}") from None


def init(c: Dict[str, Any]) -> None:
    global config
    config = with_defaults(c, config)
    apitoken.check(config, "token_env", "token_command")
    if SAMPLE:
        return
    if not isinstance(config["url"], str) or not re.match(r"https?://", config["url"]):
        raise config_error("set `url:` to your Jenkins address, e.g. https://ci.example.com")


def render() -> View:
    return view(jobs(), SAMPLE_NOW if SAMPLE else time.time())


serve(
    manifest={
        "id": "jenkins",
        "name": "Jenkins",
        "refresh": {"mode": "poll", "interval_ms": 60000, "min_interval_ms": 10000},
        "layout": {"size": "medium", "priority": 30},
    },
    init=init,
    render=render,
)
