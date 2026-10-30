#!/usr/bin/env python3
"""The largest files under a folder: a row of stats and a table. Shows config with
defaults, a config error, and a table with a gauge in each row.

    dashboard plugin-check --plugin-config '{"dir": "."}' -- python3 sdk/python/examples/big_files.py

    modules:
      big-files:
        plugin: [python3, sdk/python/examples/big_files.py]
        refresh: 1m
        config: { dir: ~/Downloads, top: 8 }
"""

import os
import sys
from typing import List, Tuple

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from dashboard_plugin import Row, View, config_error, log, serve, with_defaults  # noqa: E402

config = {"dir": ".", "top": 5, "max_files": 20000}

SCHEMA = {
    "type": "object",
    "additionalProperties": False,
    "properties": {
        "dir": {"type": "string", "description": "Folder to scan. `~` is your home folder."},
        "top": {"type": "integer", "minimum": 1, "description": "How many files to list."},
        "max_files": {"type": "integer", "minimum": 1, "description": "Stop scanning after this many files."},
    },
}


def init(c):
    config.update(with_defaults(c, config))
    config["dir"] = os.path.expanduser(config["dir"])
    if not os.path.isdir(config["dir"]):
        raise config_error(f"not a folder: {config['dir']}")


def walk() -> List[Tuple[str, int]]:
    """Every file under the folder, skipping dot folders and stopping at `max_files`."""
    files = []
    for root, dirs, names in os.walk(config["dir"]):
        dirs[:] = [d for d in dirs if not d.startswith(".") and d != "node_modules"]
        for name in names:
            if len(files) >= config["max_files"]:
                return files
            path = os.path.join(root, name)
            try:
                files.append((path, os.lstat(path).st_size))
            except OSError:
                pass  # gone or unreadable: skip it
    return files


def size(n: float) -> str:
    units = ["B", "KiB", "MiB", "GiB", "TiB"]
    i = 0
    while n >= 1024 and i < len(units) - 1:
        n /= 1024
        i += 1
    return f"{n:.0f} B" if i == 0 else f"{n:.1f} {units[i]}"


def render() -> View:
    files = walk()
    if len(files) >= config["max_files"]:
        log("info", f"stopped after {config['max_files']} files")
    files.sort(key=lambda f: f[1], reverse=True)
    total = sum(s for _, s in files)
    top = files[: config["top"]]
    rows: List[Row] = []
    for path, s in top:
        share = s / total * 100 if total else 0
        rows.append({
            "cells": {
                "file": os.path.relpath(path, config["dir"]),
                "size": size(s),
                "share": {"type": "gauge", "label": "", "percent": share},
            },
            "status": "warn" if share > 50 else "ok",
        })
    return {
        "status": "ok",
        "summary": f"{len(files)} files · {size(total)}",
        "root": {
            "type": "column",
            "children": [
                {
                    "type": "row",
                    "children": [
                        {"type": "stat", "label": "Files", "value": str(len(files))},
                        {"type": "stat", "label": "Total", "value": size(total)},
                        {"type": "stat", "label": "Largest", "value": size(top[0][1]) if top else "-"},
                    ],
                },
                {
                    "type": "table",
                    "columns": [
                        {"key": "file", "label": "File"},
                        {"key": "size", "label": "Size"},
                        {"key": "share", "label": "Share"},
                    ],
                    "rows": rows,
                },
            ],
        },
    }


serve(
    manifest={
        "id": "big-files",
        "name": "Big files",
        "refresh": {"mode": "poll", "interval_ms": 60000, "min_interval_ms": 5000},
        "layout": {"size": "medium", "priority": 10, "min_size": "small"},
        "config_schema": SCHEMA,
    },
    init=init,
    render=render,
)
