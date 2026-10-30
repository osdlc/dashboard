#!/usr/bin/env python3
"""__NAME__: a dashboard plugin. Made by `dashboard plugin new`.

Try it:   dashboard plugin-check -- python3 main.py
SDK:      dashboard_plugin.py, next to this file. Protocol and widgets: docs/plugins.md.
"""

import time

from dashboard_plugin import View, serve, with_defaults

# Settings from the tile's `config:` block, with their defaults.
config = {"greeting": "hello"}


def init(c):
    """Check the config here. Raise config_error("...") if it is wrong."""
    config.update(with_defaults(c, config))


def render() -> View:
    """Return what the tile shows. Raise upstream_error("...") if a service you need is down."""
    return {
        "status": "ok",
        "summary": f"{config['greeting']} · {time.strftime('%H:%M')}",
        "root": {
            "type": "column",
            "children": [
                {
                    "type": "row",
                    "children": [
                        {"type": "stat", "label": "Greeting", "value": config["greeting"]},
                        {"type": "stat", "label": "Time", "value": time.strftime("%H:%M:%S")},
                    ],
                },
                {
                    "type": "list",
                    "items": [
                        {"title": "Edit main.py to show your own data", "tags": ["todo"]},
                        {
                            "title": "Read about widgets",
                            "subtitle": "docs/plugins.md",
                            "action": {"type": "open_url", "url": "https://github.com/osdlc/dashboard/blob/main/docs/plugins.md"},
                        },
                    ],
                },
            ],
        },
    }


serve(
    manifest={
        "id": "__ID__",
        "name": "__NAME__",
        "refresh": {"mode": "poll", "interval_ms": 30000, "min_interval_ms": 5000},
        "layout": {"size": "medium"},
        "config_schema": {
            "type": "object",
            "additionalProperties": False,
            "properties": {"greeting": {"type": "string"}},
        },
    },
    init=init,
    render=render,
)
