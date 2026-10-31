"""Rollbar items to a view. No I/O here, so the tests can feed it sample.json."""

from typing import Any, Dict, List, Optional

from dashboard_plugin import ListItem, View, upstream_error
from token_api import ago, count, is_int, plural

# The time the sample ages are measured from (2026-10-27T02:00:00Z), so `--sample` shows
# the same view whenever it runs.
SAMPLE_NOW = 1793066400

# Rollbar's levels, lowest first. The API sends a name or its number (10 to 50).
LEVELS = ["debug", "info", "warning", "error", "critical"]


def level_name(v: Any) -> str:
    if isinstance(v, str) and v in LEVELS:
        return v
    if is_int(v) and 10 <= v <= 50 and v % 10 == 0:
        return LEVELS[v // 10 - 1]
    return "error"


def item(it: Dict[str, Any], account: str, project: str, now: int) -> ListItem:
    level = level_name(it.get("level"))
    counter = it.get("counter")
    sub = []
    if is_int(counter):
        sub.append(f"#{counter}")
    total = it.get("total_occurrences")
    if is_int(total):
        sub.append(f"{count(total)} {'occurrence' if total == 1 else 'occurrences'}")
    env = it.get("environment")
    if isinstance(env, str) and env:
        sub.append(env)
    last = it.get("last_occurrence_timestamp")
    if is_int(last):
        sub.append(ago(now - last))
    out: ListItem = {"title": str(it.get("title") or "(no title)"), "subtitle": " · ".join(sub), "tags": [level]}
    if level == "critical" or level == "error":
        out["status"] = "error" if level == "critical" else "warn"
    if is_int(counter):
        url = f"https://rollbar.com/{account}/{project}/items/{counter}/"
    else:
        url = f"https://rollbar.com/{account}/{project}/"
    out["action"] = {"type": "open_url", "url": url}
    return out


def view(reply: Any, account: str, project: str, min_level: str, limit: int, now: int) -> View:
    """The tile: active items at `min_level` or above, most recent first."""
    if isinstance(reply, dict) and reply.get("err"):
        raise upstream_error(f"Rollbar: {reply.get('message') or 'error'}")
    result = reply.get("result") if isinstance(reply, dict) else None
    items: Optional[List[Any]] = result.get("items") if isinstance(result, dict) else None
    if not isinstance(items, list):
        raise upstream_error("unexpected Rollbar reply: no result.items")
    floor = LEVELS.index(min_level)
    kept = [i for i in items if isinstance(i, dict) and LEVELS.index(level_name(i.get("level"))) >= floor]
    kept.sort(key=lambda i: i.get("last_occurrence_timestamp") if is_int(i.get("last_occurrence_timestamp")) else 0, reverse=True)
    critical = sum(1 for i in kept if level_name(i.get("level")) == "critical")
    errors = sum(1 for i in kept if level_name(i.get("level")) == "error")
    summary = [plural(len(kept), "active item")]
    if critical:
        summary.append(f"{critical} critical")
    status = "error" if critical else "warn" if errors else "ok"
    return {
        "status": status,
        "summary": " · ".join(summary),
        "root": {
            "type": "list",
            "items": [item(i, account, project, now) for i in kept[:limit]],
            "empty_text": "no active items",
        },
    }
