"""Airbrake error groups to a view. No I/O here, so the tests can feed it sample.json."""

from typing import Any, Dict

from dashboard_plugin import ListItem, View, upstream_error
from token_api import ago, count, is_int, parse_time, plural

# The time the sample ages are measured from (2026-10-27T02:00:00Z), so `--sample` shows
# the same view whenever it runs.
SAMPLE_NOW = 1793066400


def group(g: Dict[str, Any], project_id: int, now: int) -> ListItem:
    errors = g.get("errors")
    first = errors[0] if isinstance(errors, list) and errors and isinstance(errors[0], dict) else {}
    kind = first.get("type") if isinstance(first.get("type"), str) else ""
    message = first.get("message") if isinstance(first.get("message"), str) else ""
    title = f"{kind}: {message}" if kind and message else kind or message or "(no message)"

    ctx = g.get("context") if isinstance(g.get("context"), dict) else {}
    sub = []
    n = g.get("noticeCount")
    if is_int(n):
        sub.append(f"{count(n)} {'notice' if n == 1 else 'notices'}")
    env = ctx.get("environment")
    if isinstance(env, str) and env:
        sub.append(env)
    component, action = ctx.get("component"), ctx.get("action")
    if isinstance(component, str) and component:
        sub.append(f"{component}#{action}" if isinstance(action, str) and action else component)
    last = parse_time(g.get("lastNoticeAt"))
    if last is not None:
        sub.append(ago(now - last))

    out: ListItem = {"title": title, "subtitle": " · ".join(sub), "tags": ["muted"] if g.get("muted") is True else []}
    severity = ctx.get("severity")
    if severity in ("critical", "emergency", "alert"):
        out["status"] = "error"
    elif severity == "error":
        out["status"] = "warn"
    gid = g.get("id")
    url = f"https://airbrake.io/projects/{project_id}"
    if isinstance(gid, (str, int)) and str(gid):
        url += f"/groups/{gid}"
    out["action"] = {"type": "open_url", "url": url}
    return out


def view(reply: Any, project_id: int, limit: int, now: int) -> View:
    """The tile: unresolved error groups, most recent notice first."""
    groups = reply.get("groups") if isinstance(reply, dict) else None
    if not isinstance(groups, list):
        raise upstream_error("unexpected Airbrake reply: no groups")
    groups = [g for g in groups if isinstance(g, dict) and g.get("resolved") is not True]
    groups.sort(key=lambda g: parse_time(g.get("lastNoticeAt")) or 0, reverse=True)
    items = [group(g, project_id, now) for g in groups]
    shown = items[:limit]
    critical = sum(1 for i in items if i.get("status") == "error")
    summary = [plural(len(items), "open error")]
    if critical:
        summary.append(f"{critical} critical")
    status = "error" if critical else "warn" if any(i.get("status") == "warn" for i in items) else "ok"
    return {
        "status": status,
        "summary": " · ".join(summary),
        "root": {"type": "list", "items": shown, "empty_text": "no open errors"},
    }
