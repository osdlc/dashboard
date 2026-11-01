#!/usr/bin/env python3
"""Trello: the open cards on one board, soonest due first, with a bar per list. Untested live:
built from Trello's REST API docs (GET /1/boards/{id}/lists?cards=open) and checked against
sample.json.

    modules:
      cards:
        package: trello
        config: { board: 6710a1b2c3d4e5f600000000, lists: [Doing, Review] }
        # key in $TRELLO_KEY, token in $TRELLO_TOKEN

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
from view import CARD_FIELDS, SAMPLE_NOW, view  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
SAMPLE = "--sample" in sys.argv[1:]
# How long one request may take, in seconds.
TIMEOUT = 20

config: Dict[str, Any] = {
    "board": "",
    "lists": [],
    "limit": 10,
    "key_env": "TRELLO_KEY",
    "key_command": None,
    "token_env": "TRELLO_TOKEN",
    "token_command": None,
}


def token() -> str:
    return apitoken.secret(config, "token_env", "token_command")


def lists() -> List[Dict[str, Any]]:
    if SAMPLE:
        with open(os.path.join(HERE, "sample.json"), encoding="utf-8") as f:
            return json.load(f)
    key = apitoken.secret(config, "key_env", "key_command", "API key")
    query = urllib.parse.urlencode({"cards": "open", "card_fields": CARD_FIELDS, "filter": "open", "fields": "name"})
    url = f"https://api.trello.com/1/boards/{urllib.parse.quote(config['board'], safe='')}/lists?{query}"
    # Key and token go in a header, not the URL, so they stay out of any logs.
    auth = f'OAuth oauth_consumer_key="{key}", oauth_token="{token()}"'
    try:
        with urllib.request.urlopen(urllib.request.Request(url, headers={"Authorization": auth}), timeout=TIMEOUT) as res:
            return json.load(res)
    except urllib.error.HTTPError as e:
        if e.code == 401:
            apitoken.forget()
            raise upstream_error("Trello rejected the key or token (HTTP 401)") from None
        if e.code == 404:
            raise upstream_error(f"Trello has no board {json.dumps(config['board'])}") from None
        raise upstream_error(f"Trello: HTTP {e.code}") from None
    except (urllib.error.URLError, OSError, ValueError) as e:
        raise upstream_error(f"Trello unreachable: {getattr(e, 'reason', e)}") from None


def init(c: Dict[str, Any]) -> None:
    global config
    config = with_defaults(c, config)
    apitoken.check(config, "key_env", "key_command")
    apitoken.check(config, "token_env", "token_command")
    if not SAMPLE and not config["board"]:
        raise config_error("set `board:` to the board's ID (or the short ID in its URL)")
    if not isinstance(config["lists"], list) or not all(isinstance(n, str) for n in config["lists"]):
        raise config_error("lists must be a list of list names")
    limit = config["limit"]
    if not isinstance(limit, int) or isinstance(limit, bool) or limit < 1:
        raise config_error("limit must be at least 1")


def render() -> View:
    return view(lists(), config["lists"], SAMPLE_NOW if SAMPLE else time.time(), config["limit"])


serve(
    manifest={
        "id": "trello",
        "name": "Trello",
        "refresh": {"mode": "poll", "interval_ms": 120000, "min_interval_ms": 30000},
        "layout": {"size": "medium", "priority": 20},
    },
    init=init,
    render=render,
)
