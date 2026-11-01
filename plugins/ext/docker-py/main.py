#!/usr/bin/env python3
"""Running Docker containers, read with the `docker` CLI: name, image, status and, from
`docker stats`, CPU and memory. An error tile when the Docker daemon is not running.

The Python copy of the built-in `docker` module (crates/modules-ops/src/docker.rs), for the
native-vs-plugin benchmark. It builds the same view from the same docker output.

    dashboard plugin-check -- python3 plugins/ext/docker-py/main.py

    modules:
      docker-py:
        plugin: [python3, plugins/ext/docker-py/main.py]
        config: { stats: true, limit: 20 }
"""

import json
import os
import subprocess
import sys
from typing import Any, Dict, List, Optional, Union

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from dashboard_plugin import Column, Row, View, serve, with_defaults  # noqa: E402

# How long one docker command may take, in seconds. A render runs up to two
# (ps, then stats), so 2 x 10 s stays under the plugin host's 30 s limit
# (crates/host/src/plugin.rs). Keep in step with the built-in module.
TIMEOUT = 10

config: Dict[str, Any] = {"stats": True, "limit": 20, "docker": "docker"}

SCHEMA = {
    "type": "object",
    "additionalProperties": False,
    "properties": {
        "stats": {"type": "boolean", "description": "Show CPU and memory, from `docker stats` (about 2 s per refresh)."},
        "limit": {"type": "integer", "minimum": 0, "description": "Rows shown."},
        "docker": {"type": "string", "description": "The docker program, e.g. a full path."},
    },
}


class Failure:
    """Why a docker command gave no output: "not_found", "timed_out" or "failed"."""

    def __init__(self, kind: str, line: str = ""):
        self.kind = kind
        self.line = line


def docker(*args: str) -> Union[str, Failure]:
    """Runs `docker <args>` and returns its stdout, or why it failed."""
    try:
        out = subprocess.run(
            [config["docker"], *args],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            encoding="utf-8",
            errors="replace",
            timeout=TIMEOUT,
        )
    except FileNotFoundError:
        return Failure("not_found")
    except subprocess.TimeoutExpired:
        return Failure("timed_out")
    except OSError as e:
        return Failure("failed", f"running docker: {e}")
    if out.returncode != 0:
        return Failure("failed", last_line(out.stderr, args[0]))
    return out.stdout


def last_line(stderr: str, command: str) -> str:
    """The last non-empty line, or a stand-in when there is none."""
    lines = [l.strip() for l in stderr.splitlines() if l.strip()]
    return lines[-1] if lines else f"docker {command} failed"


def daemon_down(stderr: str) -> bool:
    """What the docker CLI says when it can't reach the daemon."""
    s = stderr.lower()
    return (
        "cannot connect to the docker daemon" in s
        or "failed to connect to the docker api" in s
        or "is the docker daemon running" in s
    )


def failure_view(f: Failure) -> View:
    """The error tile. A view, not an error, so the tile doesn't keep the last containers."""
    if f.kind == "not_found":
        summary = "docker not found"
        text = ("The docker command was not found. Install Docker, or set `docker:` in the "
                "config to the docker program.")
    elif f.kind == "timed_out":
        summary = "docker timed out"
        text = "docker did not answer."
    else:
        summary = "Docker is not running" if daemon_down(f.line) else "docker failed"
        text = f.line
    return {"status": "error", "summary": summary, "root": {"type": "text", "text": text, "markdown": False}}


def parse_lines(text: str, keys: List[str]) -> List[Dict[str, str]]:
    """One JSON object per line; lines that aren't one are skipped."""
    out = []
    for line in text.splitlines():
        try:
            v = json.loads(line.strip())
        except ValueError:
            continue
        if isinstance(v, dict) and all(isinstance(v.get(k), str) for k in keys):
            out.append(v)
    return out


def unhealthy(c: Dict[str, str]) -> bool:
    return "(unhealthy)" in c["Status"]


def restarting(c: Dict[str, str]) -> bool:
    return c["State"] == "restarting"


def view(containers: List[Dict[str, str]], stats: Optional[List[Dict[str, str]]], limit: int) -> View:
    """The tile. `stats` is None when CPU and memory are off or unavailable."""
    sick = sum(1 for c in containers if unhealthy(c))
    looping = sum(1 for c in containers if restarting(c))
    summary = ["1 container" if len(containers) == 1 else f"{len(containers)} containers"]
    if sick:
        summary.append(f"{sick} unhealthy")
    if looping:
        summary.append(f"{looping} restarting")
    status = "warn" if sick + looping else "ok"
    if not containers:
        return {
            "status": status,
            "summary": "no containers running",
            "root": {"type": "text", "text": "No containers running.", "markdown": False},
        }

    columns: List[Column] = [
        {"key": "name", "label": "Name"},
        {"key": "image", "label": "Image"},
        {"key": "status", "label": "Status"},
    ]
    if stats is not None:
        columns += [{"key": "cpu", "label": "CPU"}, {"key": "mem", "label": "Memory"}]
    rows: List[Row] = []
    for c in containers[:limit]:
        cells = {"name": c["Names"], "image": c["Image"], "status": c["Status"]}
        if stats is not None:
            # Matched on the short ID: ps and stats may print it at different lengths.
            s = next((s for s in stats if s["ID"][:12] == c["ID"][:12]), None)
            cells["cpu"] = s["CPUPerc"] if s else "-"
            # "10.2MiB / 7.66GiB" -> "10.2MiB"
            cells["mem"] = s["MemUsage"].split(" / ")[0] if s else "-"
        rows.append({"cells": cells, "status": "warn" if unhealthy(c) or restarting(c) else "ok"})
    return {"status": status, "summary": " · ".join(summary), "root": {"type": "table", "columns": columns, "rows": rows}}


def init(c):
    config.update(with_defaults(c, config))


def render() -> View:
    ps = docker("ps", "--format", "{{json .}}")
    if isinstance(ps, Failure):
        return failure_view(ps)
    containers = parse_lines(ps, ["ID", "Names", "Image", "State", "Status"])
    stats = None
    if config["stats"] and containers:
        # Without stats the tile still shows the containers.
        out = docker("stats", "--no-stream", "--format", "{{json .}}")
        if not isinstance(out, Failure):
            stats = parse_lines(out, ["ID", "CPUPerc", "MemUsage"])
    return view(containers, stats, config["limit"])


serve(
    manifest={
        "id": "docker-py",
        "name": "Docker (Python)",
        "refresh": {"mode": "poll", "interval_ms": 15000, "min_interval_ms": 5000},
        "layout": {"size": "wide", "priority": 20, "min_size": "small"},
        "config_schema": SCHEMA,
    },
    init=init,
    render=render,
)
