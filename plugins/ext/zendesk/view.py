"""Zendesk ticket search results to a view. No I/O here, so the tests can feed it
sample.json."""

from typing import Any, Dict, List, Optional

from dashboard_plugin import ListItem, View, upstream_error
from token_api import ago, is_int, parse_time, plural

# The time the sample ages are measured from (2026-10-27T02:00:00Z), so `--sample` shows
# the same view whenever it runs.
SAMPLE_NOW = 1793066400

# Tags shown per ticket, at most.
MAX_TAGS = 3


def ticket(t: Dict[str, Any], subdomain: str, now: int) -> ListItem:
    sub = []
    if is_int(t.get("id")):
        sub.append(f"#{t['id']}")
    for key in ("status", "priority"):
        if isinstance(t.get(key), str) and t[key] and t[key] != "normal":
            sub.append(t[key])
    updated = parse_time(t.get("updated_at"))
    if updated is not None:
        sub.append(f"updated {ago(now - updated)}")
    out: ListItem = {
        "title": str(t.get("subject") or t.get("raw_subject") or "(no subject)"),
        "subtitle": " · ".join(sub),
        "tags": [tag for tag in t.get("tags") or [] if isinstance(tag, str)][:MAX_TAGS],
    }
    priority = t.get("priority")
    if priority == "urgent":
        out["status"] = "error"
    elif priority == "high":
        out["status"] = "warn"
    base = f"https://{subdomain}.zendesk.com/agent"
    out["action"] = {"type": "open_url", "url": f"{base}/tickets/{t['id']}" if is_int(t.get("id")) else base}
    return out


def view(reply: Any, subdomain: str, limit: int, now: int) -> View:
    """The tile: the tickets the search found, most recently updated first."""
    results: Optional[List[Any]] = reply.get("results") if isinstance(reply, dict) else None
    if not isinstance(results, list):
        err = reply.get("error") if isinstance(reply, dict) else None
        if isinstance(err, dict):
            err = err.get("message") or err.get("title")
        raise upstream_error(f"Zendesk: {err}" if err else "unexpected Zendesk reply: no results")
    tickets = [t for t in results if isinstance(t, dict) and t.get("result_type", "ticket") == "ticket"]
    tickets.sort(key=lambda t: parse_time(t.get("updated_at")) or 0, reverse=True)
    total = reply.get("count") if is_int(reply.get("count")) else len(tickets)
    urgent = sum(1 for t in tickets if t.get("priority") == "urgent")
    summary = [plural(total, "ticket")]
    if urgent:
        summary.append(f"{urgent} urgent")
    return {
        "status": "error" if urgent else "ok",
        "summary": " · ".join(summary),
        "root": {
            "type": "list",
            "items": [ticket(t, subdomain, now) for t in tickets[:limit]],
            "empty_text": "no tickets",
        },
    }
