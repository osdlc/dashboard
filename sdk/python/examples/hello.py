#!/usr/bin/env python3
"""The smallest plugin with the SDK: a greeting, a clock and a render counter.

    dashboard plugin-check -- python3 sdk/python/examples/hello.py

    modules:
      hello:
        plugin: [python3, sdk/python/examples/hello.py]
        config: { name: you }
"""

import os
import sys
import time

# The SDK is one file in the folder above; a plugin of your own keeps a copy next to it.
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from dashboard_plugin import serve, with_defaults  # noqa: E402

config = {"name": "world"}
renders = 0


def init(c):
    config.update(with_defaults(c, config))


def render():
    global renders
    renders += 1
    return {
        "status": "ok",
        "summary": f"hello, {config['name']}",
        "root": {
            "type": "row",
            "children": [
                {"type": "stat", "label": f"Hello, {config['name']}", "value": time.strftime("%H:%M:%S")},
                {"type": "stat", "label": "Renders", "value": str(renders)},
            ],
        },
    }


serve(
    manifest={
        "id": "hello",
        "name": "Hello",
        "refresh": {"mode": "poll", "interval_ms": 5000, "min_interval_ms": 1000},
        "layout": {"size": "small", "priority": 0},
    },
    init=init,
    render=render,
)
