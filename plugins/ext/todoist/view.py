"""Turns Todoist's filtered task list (`GET /api/v1/tasks/filter`) into the tile's view. No I/O
here, so the tests and the --sample mode run the same code as the real tile."""

import datetime
import math
import urllib.parse
from typing import Any, Dict, List, Optional

from dashboard_plugin import ListItem, Status, View

# The fixed date for --sample and the tests, so "due today" always reads the same.
SAMPLE_TODAY = "2026-10-27"


def days(task: Dict[str, Any], today: str) -> Optional[int]:
    """Whole days from `today` to the task's due date: negative when overdue, None with no
    date. `due.date` is YYYY-MM-DD, or YYYY-MM-DDTHH:MM:SS for a task with a time."""
    if not task.get("due"):
        return None
    date = datetime.date.fromisoformat(task["due"]["date"][:10])
    return (date - datetime.date.fromisoformat(today)).days


def label(priority: int) -> str:
    """p1 (urgent) .. p4 (normal), as the Todoist app writes it. The API's priority 4 is p1."""
    return f"p{5 - priority}"


def due(task: Dict[str, Any], today: str) -> str:
    d = days(task, today)
    if d is None:
        return "no due date"
    date = task["due"]["date"]
    time = f" {date[11:16]}" if len(date) > 10 else ""
    if d < 0:
        return f"overdue {-d}d"
    if d == 0:
        return f"today{time}"
    if d == 1:
        return f"tomorrow{time}"
    return f"in {d}d"


def view(response: List[Dict[str, Any]], today: str, limit: int) -> View:
    def order(t: Dict[str, Any]) -> Any:
        # Overdue first, then by date; within a day the most urgent first.
        d = days(t, today)
        return (math.inf if d is None else d, -t["priority"], t["content"].lower())

    tasks = sorted(response, key=order)
    overdue = sum(1 for t in tasks if (days(t, today) or 0) < 0)
    due_today = sum(1 for t in tasks if days(t, today) == 0)
    urgent = sum(1 for t in tasks if t["priority"] == 4)

    items: List[ListItem] = []
    for t in tasks[:limit]:
        d = days(t, today)
        repeats = [t["due"].get("string") or "repeats"] if t.get("due") and t["due"].get("is_recurring") else []
        item: ListItem = {
            "title": t["content"],
            "subtitle": " · ".join([due(t, today)] + repeats),
            "tags": ([label(t["priority"])] if t["priority"] > 1 else []) + list(t.get("labels") or []),
            "action": {"type": "open_url", "url": f"https://app.todoist.com/app/task/{urllib.parse.quote(t['id'], safe='')}"},
        }
        if d is not None and d < 0:
            item["status"] = "error"
        elif t["priority"] == 4:
            item["status"] = "warn"
        items.append(item)

    status: Status = "error" if overdue else "warn" if urgent else "ok"
    return {
        "status": status,
        "summary": f"{overdue} overdue · {due_today} today",
        "root": {
            "type": "column",
            "children": [
                {
                    "type": "row",
                    "children": [
                        {"type": "stat", "label": "Overdue", "value": str(overdue), "status": "error" if overdue else "ok"},
                        {"type": "stat", "label": "Today", "value": str(due_today)},
                        {"type": "stat", "label": "Priority 1", "value": str(urgent), "status": "warn" if urgent else "ok"},
                    ],
                },
                {"type": "list", "empty_text": "Nothing to do", "items": items},
            ],
        },
    }
