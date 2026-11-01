"""Turns Asana's task list (`GET /tasks`) into the tile's view. No I/O here, so the tests and
the --sample mode run the same code as the real tile."""

import datetime
from typing import Any, Dict, List, Optional

from dashboard_plugin import ListItem, Status, View

# The fields we ask for with `opt_fields=`. `due_on` is YYYY-MM-DD; Asana sets it for tasks
# with a due time too.
FIELDS = "name,due_on,permalink_url,projects.name"
# Tasks per request; the tile reads one page.
PAGE = 100

# The fixed date for --sample and the tests, so "due today" always reads the same.
SAMPLE_TODAY = "2026-10-27"


def days(date: str, today: str) -> int:
    """Whole days from `today` to `date` (both YYYY-MM-DD): negative when overdue."""
    return (datetime.date.fromisoformat(date) - datetime.date.fromisoformat(today)).days


def due(date: Optional[str], today: str) -> str:
    if not date:
        return "no due date"
    d = days(date, today)
    if d < 0:
        return f"overdue {-d}d"
    if d == 0:
        return "due today"
    if d == 1:
        return "due tomorrow"
    return f"due in {d}d"


def view(response: List[Dict[str, Any]], today: str, limit: int) -> View:
    # Overdue first, then by due date; tasks with no date last.
    tasks = sorted(response, key=lambda t: (not t.get("due_on"), t.get("due_on") or "", t["name"].lower()))
    overdue = sum(1 for t in tasks if t.get("due_on") and days(t["due_on"], today) < 0)
    due_today = sum(1 for t in tasks if t.get("due_on") and days(t["due_on"], today) == 0)

    items: List[ListItem] = []
    for t in tasks[:limit]:
        item: ListItem = {
            "title": t["name"],
            "subtitle": " · ".join([p["name"] for p in t.get("projects") or []] + [due(t.get("due_on"), today)]),
            "action": {"type": "open_url", "url": t["permalink_url"]},
        }
        if t.get("due_on"):
            d = days(t["due_on"], today)
            if d <= 0:
                item["status"] = "error" if d < 0 else "warn"
        items.append(item)

    status: Status = "error" if overdue else "warn" if due_today else "ok"
    # The API sends one page; a full page means there may be more.
    open_count = f"{len(tasks)}+" if len(tasks) >= PAGE else str(len(tasks))
    return {
        "status": status,
        "summary": f"{overdue} overdue · {due_today} today · {open_count} open",
        "root": {
            "type": "column",
            "children": [
                {
                    "type": "row",
                    "children": [
                        {"type": "stat", "label": "Overdue", "value": str(overdue), "status": "error" if overdue else "ok"},
                        {"type": "stat", "label": "Today", "value": str(due_today), "status": "warn" if due_today else "ok"},
                        {"type": "stat", "label": "Open", "value": open_count},
                    ],
                },
                {"type": "list", "empty_text": "No open tasks", "items": items},
            ],
        },
    }
