#!/usr/bin/env python3
"""Network latency: the time to open a TCP connection to each target, as a
live chart with the latest values, and the tile turning yellow or red when a
target is slow or unreachable. Standard library only; no ping privileges.

    latency:
      plugin: [python3, examples/plugins/latency/latency.py]
      refresh: 5s
      config:
        slow_ms: 150
        targets:
          - { name: github, host: github.com }            # port 443 by default
          - { name: dns, host: 1.1.1.1, port: 53 }
          - { name: router, host: 192.168.1.1, port: 80 }

Shows the host-kept series history: the chart widget sets `id`, so each
render sends only the newest point per target and the dashboard keeps the
last `history` points. The plugin itself stays stateless.

    dashboard plugin-check --renders 5 --plugin-config '{"targets":[{"name":"github","host":"github.com"}]}' -- python3 examples/plugins/latency/latency.py
"""

import json
import socket
import sys
import time
from concurrent.futures import ThreadPoolExecutor

CONFIG_INVALID = -32002
TIMEOUT_S = 3

config = {"targets": [], "slow_ms": 150, "history": 120}

SCHEMA = {
    "type": "object",
    "additionalProperties": False,
    "required": ["targets"],
    "properties": {
        "slow_ms": {"type": "number", "description": "Slower than this is a warning."},
        "history": {"type": "integer", "minimum": 2, "description": "Points kept per target."},
        "targets": {
            "type": "array",
            "minItems": 1,
            "items": {
                "type": "object",
                "additionalProperties": False,
                "required": ["name", "host"],
                "properties": {
                    "name": {"type": "string"},
                    "host": {"type": "string"},
                    "port": {"type": "integer", "minimum": 1, "maximum": 65535},
                },
            },
        },
    },
}


class RpcError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def initialize(params):
    user = params.get("config") or {}
    unknown = set(user) - set(config)
    if unknown:
        raise RpcError(CONFIG_INVALID, f"unknown settings: {', '.join(sorted(unknown))}")
    config.update(user)
    targets = config["targets"]
    if not targets or not all(isinstance(t, dict) and t.get("name") and t.get("host") for t in targets):
        raise RpcError(CONFIG_INVALID, "set `targets: [{ name, host, port? }]`")
    return {
        "manifest": {
            "id": "latency",
            "name": "Latency",
            "contract_version": "0.1",
            "refresh": {"mode": "poll", "interval_ms": 5000, "min_interval_ms": 1000},
            "layout": {"size": "medium", "priority": 25, "min_size": "small"},
            "commands": [],
            "config_schema": SCHEMA,
        }
    }


def connect_ms(target):
    """Milliseconds to open a TCP connection, or None if it failed."""
    start = time.perf_counter()
    try:
        with socket.create_connection((target["host"], target.get("port", 443)), timeout=TIMEOUT_S):
            return (time.perf_counter() - start) * 1000
    except OSError:
        return None


def render(_params):
    targets = config["targets"]
    with ThreadPoolExecutor(max_workers=len(targets)) as pool:
        results = list(pool.map(connect_ms, targets))
    now_ms = int(time.time() * 1000)  # chart times are unix milliseconds

    down = [t["name"] for t, ms in zip(targets, results) if ms is None]
    slow = [t["name"] for t, ms in zip(targets, results) if ms is not None and ms > config["slow_ms"]]
    status = "error" if down else "warn" if slow else "ok"

    def stat(t, ms):
        if ms is None:
            return {"type": "stat", "label": t["name"], "value": "down", "status": "error"}
        return {"type": "stat", "label": t["name"], "value": f"{ms:.0f}", "unit": "ms",
                "status": "warn" if ms > config["slow_ms"] else "ok"}

    parts = [f"{t['name']} {'down' if ms is None else f'{ms:.0f}ms'}" for t, ms in zip(targets, results)]
    return {
        "status": status,
        "summary": " · ".join(parts),
        "root": {
            "type": "column",
            "children": [
                {"type": "row", "children": [stat(t, ms) for t, ms in zip(targets, results)]},
                {
                    "type": "series",
                    "label": "Connect time",
                    "unit": "ms",
                    # With `id`, send only new points; the host keeps the rest.
                    "id": "connect",
                    "history": config["history"],
                    "style": "line",
                    "series": [
                        # A target that is down sends no point this time.
                        {"name": t["name"], "points": [] if ms is None else [[now_ms, round(ms, 1)]]}
                        for t, ms in zip(targets, results)
                    ],
                },
            ],
        },
    }


HANDLERS = {"initialize": initialize, "view/render": render, "shutdown": lambda _: None}


def send(msg):
    print(json.dumps(msg), flush=True)


for line in sys.stdin:
    msg = json.loads(line)
    if msg.get("method") == "exit":
        break
    if "id" not in msg:
        continue
    try:
        handler = HANDLERS.get(msg["method"])
        if not handler:
            raise RpcError(-32601, f"unknown method {msg['method']}")
        send({"jsonrpc": "2.0", "id": msg["id"], "result": handler(msg.get("params") or {})})
    except RpcError as e:
        send({"jsonrpc": "2.0", "id": msg["id"], "error": {"code": e.code, "message": str(e)}})
    except Exception as e:
        send({"jsonrpc": "2.0", "id": msg["id"], "error": {"code": -32603, "message": f"{type(e).__name__}: {e}"}})
