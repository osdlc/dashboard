#!/usr/bin/env python3
"""Sentry: unresolved issues for an organization or one project, newest activity first, each
linking to the issue. Built from the public API docs (GET /api/0/organizations/{org}/issues/
and /api/0/projects/{org}/{project}/issues/); untested live.

    modules:
      sentry:
        package: sentry
        config: { org: acme, project: web }

Needs an auth token with event:read: $SENTRY_AUTH_TOKEN, or a command that prints it
(token_command). Self-hosted Sentry: set base_url.
"""

import re
import urllib.parse
from typing import Any, Dict, List, Optional

from dashboard_plugin import ListItem, Status, View, config_error, serve, upstream_error, with_defaults
from ops import ago, base_url, count, env_name, get_json, join, plural, read_token, strings, upto

DEFAULTS: Dict[str, Any] = {
    "base_url": "https://sentry.io",
    "org": None,
    "project": None,
    "query": "is:unresolved",
    "environment": None,
    "sort": "date",
    "stats_period": "24h",
    "limit": 10,
    "token_env": "SENTRY_AUTH_TOKEN",
    "token_command": None,
}
config = dict(DEFAULTS)
api = ""

SLUG = re.compile(r"^[a-zA-Z0-9][a-zA-Z0-9_-]*$")


def item_status(level: Any) -> Status:
    if level in ("fatal", "error"):
        return "error"
    if level == "warning":
        return "warn"
    return "ok"


def number(value: Any) -> int:
    """A count; Sentry sends `count` as a string ("1284") and `userCount` as a number."""
    try:
        return int(value)
    except (TypeError, ValueError):
        return 0


def short(n: int) -> str:
    """ "1.2k" for counts the list has little room for."""
    if n >= 10000:
        return f"{round(n / 1000)}k"
    if n >= 1000:
        return f"{n / 1000:.1f}k"
    return str(n)


def view(issues: List[Dict[str, Any]], now: Optional[float] = None) -> View:
    """The view for the issues the query matched, in the order Sentry sent them."""
    events = sum(number(i.get("count")) for i in issues)
    users = sum(number(i.get("userCount")) for i in issues)
    fatal = sum(1 for i in issues if i.get("level") == "fatal")
    items: List[ListItem] = []
    for i in issues[: config["limit"]]:
        project = (i.get("project") or {}).get("slug")
        tags = (["unhandled"] if i.get("isUnhandled") else []) + (
            [project] if not config["project"] and isinstance(project, str) else []
        )
        item: ListItem = {
            "title": str(i.get("title") or i.get("shortId") or i.get("id")),
            "subtitle": join(
                [
                    i.get("shortId"),
                    i.get("culprit"),
                    f"{short(number(i.get('count')))} events",
                    number(i.get("userCount")) and f"{short(number(i.get('userCount')))} users",
                    ago(i.get("lastSeen"), now),
                ]
            ),
            "status": item_status(i.get("level")),
            "tags": tags,
        }
        if isinstance(i.get("permalink"), str):
            item["action"] = {"type": "open_url", "url": i["permalink"]}
        items.append(item)
    status: Status = "error" if fatal else "warn" if issues else "ok"
    summary = f"{plural(len(issues), 'issue')} · {short(events)} events" if issues else "No unresolved issues"
    return {
        "status": status,
        "summary": summary,
        "root": {
            "type": "column",
            "children": [
                {
                    "type": "row",
                    "children": [
                        {"type": "stat", "label": "Issues", "value": upto(len(issues)), "status": "warn" if issues else "ok"},
                        {"type": "stat", "label": "Events", "value": short(events)},
                        {"type": "stat", "label": "Users", "value": short(users)},
                    ],
                },
                {"type": "list", "items": items, "empty_text": "No unresolved issues"},
            ],
        },
    }


def init(c: Any) -> None:
    global api
    config.update(with_defaults(c, DEFAULTS))
    api = base_url(config["base_url"], "base_url")
    if not isinstance(config["org"], str) or not SLUG.match(config["org"]):
        raise config_error("org is required: your organization's slug, as in sentry.io/organizations/<org>/")
    if config["project"] is not None and (not isinstance(config["project"], str) or not SLUG.match(config["project"])):
        raise config_error("project must be a project slug")
    if config["sort"] not in ("date", "new", "freq", "user"):
        raise config_error("sort must be date, new, freq or user")
    if not isinstance(config["query"], str):
        raise config_error("query must be text, e.g. is:unresolved")
    if config["environment"] is not None and not isinstance(config["environment"], str):
        raise config_error("environment must be text, e.g. production")
    if not isinstance(config["stats_period"], str) or not re.match(r"^\d+[hdw]$", config["stats_period"]):
        raise config_error("stats_period must be like 24h or 14d")
    # The per-project endpoint takes only these two windows.
    if config["project"] is not None and config["stats_period"] not in ("24h", "14d"):
        raise config_error("stats_period must be 24h or 14d when project is set")
    strings(config["token_command"], "token_command")
    count(config["limit"], "limit")
    env_name(config["token_env"], "token_env")


def render() -> View:
    token = read_token(config["token_env"], config["token_command"], "Sentry auth token")
    q = {"query": config["query"], "statsPeriod": config["stats_period"], "sort": config["sort"], "limit": "100"}
    if config["environment"]:
        q["environment"] = config["environment"]
    if config["project"]:
        path = f"/api/0/projects/{config['org']}/{config['project']}/issues/"
    else:
        path = f"/api/0/organizations/{config['org']}/issues/"
    issues = get_json("Sentry", f"{api}{path}?{urllib.parse.urlencode(q)}", {"Authorization": f"Bearer {token}"})
    if not isinstance(issues, list):
        raise upstream_error("Sentry: expected a list of issues")
    return view([i for i in issues if isinstance(i, dict)])


if __name__ == "__main__":
    serve(
        manifest={
            "id": "sentry",
            "name": "Sentry issues",
            "refresh": {"mode": "poll", "interval_ms": 60000, "min_interval_ms": 30000},
            "layout": {"size": "wide", "priority": 0, "min_size": "small"},
        },
        init=init,
        render=render,
    )
