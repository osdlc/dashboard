#!/usr/bin/env python3
"""The smallest useful plugin: a greeting, a clock and a render counter.

Run it through the dashboard's checker:

    dashboard plugin-check -- python3 examples/plugins/hello/hello.py

The protocol: the dashboard starts this program and writes one JSON-RPC 2.0
request per line to its stdin. The program answers each request with one line
on stdout. stdout carries only protocol messages; print anything else to
stderr (the dashboard logs it).
"""

import json
import sys
import time

renders = 0
name = "world"


def initialize(params):
    """Takes this tile's `config:` block, returns the manifest."""
    global name
    name = (params.get("config") or {}).get("name", name)
    return {
        "manifest": {
            "id": "hello",
            "name": "Hello",
            "contract_version": "0.1",
            # Ask to be rendered every 5 seconds; the config's `refresh:` can
            # change that but not below min_interval_ms.
            "refresh": {"mode": "poll", "interval_ms": 5000, "min_interval_ms": 1000},
            "layout": {"size": "small", "priority": 0},
            "commands": [],
        }
    }


def render(_params):
    """Returns the tile's current view."""
    global renders
    renders += 1
    return {
        "status": "ok",
        "summary": f"hello, {name}",
        "root": {
            "type": "row",
            "children": [
                {"type": "stat", "label": f"Hello, {name}", "value": time.strftime("%H:%M:%S")},
                {"type": "stat", "label": "Renders", "value": str(renders)},
            ],
        },
    }


HANDLERS = {"initialize": initialize, "view/render": render, "shutdown": lambda _: None}

for line in sys.stdin:
    msg = json.loads(line)
    if msg.get("method") == "exit":
        break
    if "id" not in msg:
        continue  # a notification we don't handle
    handler = HANDLERS.get(msg["method"])
    if handler:
        reply = {"jsonrpc": "2.0", "id": msg["id"], "result": handler(msg.get("params") or {})}
    else:
        reply = {"jsonrpc": "2.0", "id": msg["id"], "error": {"code": -32601, "message": f"unknown method {msg['method']}"}}
    print(json.dumps(reply), flush=True)
