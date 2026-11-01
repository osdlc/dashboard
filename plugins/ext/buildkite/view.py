"""Turns Buildkite's builds response (`GET /v2/.../builds`) into the tile's view. No I/O here,
so the tests and the --sample mode run the same code as the real tile."""

import datetime
from typing import Any, Dict, List, Optional

from dashboard_plugin import ListItem, Status, View

# The fixed "now" for --sample and the tests, so ages always read the same.
SAMPLE_NOW = datetime.datetime(2026, 10, 27, 12, 0, tzinfo=datetime.timezone.utc).timestamp()

STATUS: Dict[str, Status] = {"passed": "ok", "failed": "error", "running": "warn", "other": "unknown"}


def seconds(iso: str) -> float:
    """An ISO time from the API (`2026-10-27T11:57:40.000Z`) as unix seconds."""
    return datetime.datetime.fromisoformat(iso.replace("Z", "+00:00")).timestamp()


def state(build: Dict[str, Any]) -> str:
    """Buildkite has many states; the tile needs four."""
    s = build.get("state")
    if s == "passed":
        return "passed"
    if s in ("failed", "failing"):
        return "failed"
    if s in ("running", "scheduled", "creating"):
        return "running"
    # Someone chose these: a canceled build is not a failure, and a build blocked on a
    # manual step can wait for days, so neither turns the tile red or yellow.
    return "other"  # canceled, canceling, blocked, skipped, not_run, finished


def ago(iso: str, now: float) -> str:
    s = max(0.0, now - seconds(iso))
    if s < 60:
        return "just now"
    if s < 3600:
        return f"{int(s // 60)}m ago"
    if s < 86400:
        return f"{int(s // 3600)}h ago"
    return f"{int(s // 86400)}d ago"


def took(build: Dict[str, Any]) -> Optional[str]:
    if not build.get("started_at") or not build.get("finished_at"):
        return None
    s = int(seconds(build["finished_at"]) - seconds(build["started_at"]) + 0.5)
    return f"{s // 60}m {s % 60}s" if s >= 60 else f"{s}s"


def view(response: List[Dict[str, Any]], now: float) -> View:
    builds = sorted(response, key=lambda b: -seconds(b["created_at"]))
    counts = {s: sum(1 for b in builds if state(b) == s) for s in STATUS}
    passed, failed, running = counts["passed"], counts["failed"], counts["running"]
    # The tile's status follows each pipeline's newest finished build. A blocked build
    # hasn't finished, so it doesn't hide an earlier failure.
    latest: Dict[str, Dict[str, Any]] = {}
    for b in builds:
        if state(b) != "running" and b.get("state") != "blocked":
            latest.setdefault(b["pipeline"]["slug"], b)
    red = [b for b in latest.values() if state(b) == "failed"]

    items: List[ListItem] = []
    for b in builds:
        t = took(b)
        title = (b.get("message") or "").split("\n")[0] or f"Build #{b['number']}"
        subtitle = f"{b['pipeline']['name']} · #{b['number']} · {ago(b['created_at'], now)}"
        items.append(
            {
                "title": title,
                "subtitle": subtitle + (f" · took {t}" if t else ""),
                "status": STATUS[state(b)],
                "tags": [b["branch"]] + ([] if b["state"] in ("passed", "failed") else [b["state"]]),
                "action": {"type": "open_url", "url": b["web_url"]},
            }
        )

    if red:
        status: Status = "error"
    elif running:
        status = "warn"
    else:
        status = "ok" if builds else "unknown"
    return {
        "status": status,
        "summary": f"failing: {', '.join(b['pipeline']['name'] for b in red)}"
        if red
        else f"{passed} passed · {running} running",
        "root": {
            "type": "column",
            "children": [
                {
                    "type": "row",
                    "children": [
                        {"type": "stat", "label": "Passed", "value": str(passed), "status": "ok"},
                        {"type": "stat", "label": "Failed", "value": str(failed), "status": "error" if failed else "ok"},
                        {"type": "stat", "label": "Running", "value": str(running), "status": "warn" if running else "ok"},
                    ],
                },
                {"type": "list", "empty_text": "No builds yet", "items": items},
            ],
        },
    }
