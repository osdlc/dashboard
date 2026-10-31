"""Turns Jenkins' job list (`/api/json?tree=jobs[...]`) into the tile's view. No I/O here, so
the tests and the --sample mode run the same code as the real tile."""

import datetime
from typing import Any, Dict, List

from dashboard_plugin import Row, Status, View

# The fields we ask for. `color` is blue, red, yellow, grey, disabled, aborted or notbuilt,
# with `_anime` while building; folders have none. Times are unix milliseconds.
TREE = "jobs[name,url,color,lastBuild[number,result,timestamp,duration,building,url]]"

# The fixed "now" for --sample and the tests, so ages always read the same.
SAMPLE_NOW = datetime.datetime(2026, 10, 27, 12, 0, tzinfo=datetime.timezone.utc).timestamp()

STATUS: Dict[str, Status] = {"passing": "ok", "failing": "error", "unstable": "warn", "other": "unknown"}
ORDER = {"failing": 0, "unstable": 1, "passing": 2, "other": 3}


def state(job: Dict[str, Any]) -> str:
    color = (job.get("color") or "").replace("_anime", "")
    if color == "blue":
        return "passing"
    if color == "red":
        return "failing"
    if color == "yellow":
        return "unstable"
    return "other"  # grey, disabled, aborted, notbuilt, or a folder


def building(job: Dict[str, Any]) -> bool:
    return (job.get("color") or "").endswith("_anime")


def ago(ms: float, now: float) -> str:
    s = max(0.0, now - ms / 1000)
    if s < 60:
        return "just now"
    if s < 3600:
        return f"{int(s // 60)}m ago"
    if s < 86400:
        return f"{int(s // 3600)}h ago"
    return f"{int(s // 86400)}d ago"


def took(ms: float) -> str:
    s = int(ms / 1000 + 0.5)
    return f"{s // 60}m {s % 60}s" if s >= 60 else f"{s}s"


def result(job: Dict[str, Any]) -> str:
    """What the Result column says: the last build's result, or that it's running."""
    b = job.get("lastBuild")
    if not b:
        return "disabled" if job.get("color") == "disabled" else "never built"
    if b.get("building"):
        return "running"
    return (b.get("result") or "unknown").lower()


def view(jobs: List[Dict[str, Any]], now: float) -> View:
    # Folders and multibranch parents have no color: they aren't jobs to watch.
    watched = sorted(
        (j for j in jobs if j.get("color") is not None),
        key=lambda j: (ORDER[state(j)], j["name"].lower(), j["name"]),
    )
    passing = sum(1 for j in watched if state(j) == "passing")
    failing = [j for j in watched if state(j) == "failing"]
    unstable = sum(1 for j in watched if state(j) == "unstable")
    running = sum(1 for j in watched if building(j))

    rows: List[Row] = []
    for j in watched:
        b = j.get("lastBuild")
        when = ""
        if b:
            when = ago(b["timestamp"], now) + ("" if b.get("building") else f" · {took(b['duration'])}")
        rows.append(
            {
                "cells": {"job": j["name"], "build": f"#{b['number']}" if b else "", "result": result(j), "when": when},
                "status": "warn" if building(j) and state(j) == "passing" else STATUS[state(j)],
                "action": {"type": "open_url", "url": b["url"] if b else j["url"]},
            }
        )

    if failing:
        status: Status = "error"
    elif unstable:
        status = "warn"
    else:
        status = "ok" if watched else "unknown"
    return {
        "status": status,
        "summary": f"failing: {', '.join(j['name'] for j in failing)}"
        if failing
        else f"{passing} passing · {running} running",
        "root": {
            "type": "column",
            "children": [
                {
                    "type": "row",
                    "children": [
                        {"type": "stat", "label": "Passing", "value": str(passing), "status": "ok"},
                        {"type": "stat", "label": "Failing", "value": str(len(failing)), "status": "error" if failing else "ok"},
                        {"type": "stat", "label": "Unstable", "value": str(unstable), "status": "warn" if unstable else "ok"},
                    ],
                },
                {
                    "type": "table",
                    "columns": [
                        {"key": "job", "label": "Job"},
                        {"key": "build", "label": "Build"},
                        {"key": "result", "label": "Result"},
                        {"key": "when", "label": "When"},
                    ],
                    "rows": rows,
                },
            ],
        },
    }
