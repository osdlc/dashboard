#!/usr/bin/env python3
"""System load average as a live chart. Shows `live_series`: each render sends only the
newest values and the dashboard keeps the history, so the plugin stays stateless.

    dashboard plugin-check --renders 5 -- python3 sdk/python/examples/load.py
"""

import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from dashboard_plugin import View, live_series, serve, upstream_error  # noqa: E402


def init(_config):
    if not hasattr(os, "getloadavg"):
        raise upstream_error("no load average on this system")


def render() -> View:
    one, five, fifteen = os.getloadavg()
    cores = os.cpu_count() or 1
    busy = one / cores
    return {
        "status": "error" if busy > 1.5 else "warn" if busy > 1 else "ok",
        "summary": f"load {one:.2f} on {cores} cores",
        "root": {
            "type": "column",
            "children": [
                {
                    "type": "row",
                    "children": [
                        {"type": "stat", "label": "1 min", "value": f"{one:.2f}"},
                        {"type": "stat", "label": "5 min", "value": f"{five:.2f}"},
                        {"type": "stat", "label": "15 min", "value": f"{fifteen:.2f}"},
                    ],
                },
                live_series("load", "Load (1 min)", {"load": one}, history=120, style="area"),
            ],
        },
    }


serve(
    manifest={
        "id": "load",
        "name": "Load",
        "refresh": {"mode": "poll", "interval_ms": 5000, "min_interval_ms": 1000},
        "layout": {"size": "medium", "priority": 20},
    },
    init=init,
    render=render,
)
