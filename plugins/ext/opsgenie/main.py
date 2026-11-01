#!/usr/bin/env python3
"""Opsgenie alerts: open alerts by priority, unacknowledged first. Built from the public Alert
API docs (GET /v2/alerts); untested live.

    modules:
      opsgenie:
        package: opsgenie
        config: { account: acme, query: "status:open AND tag:prod" }

Needs an API key with read access: $OPSGENIE_API_KEY, or a command that prints it
(token_command).
"""

import re
import urllib.parse
from typing import Any, Dict, List, Optional

from dashboard_plugin import ListItem, Status, View, config_error, serve, upstream_error, with_defaults
from ops import ago, base_url, count, env_name, get_json, join, parse_time, read_token, strings, upto

DEFAULTS: Dict[str, Any] = {
    "region": "us",
    "base_url": None,
    "query": "status:open",
    "account": None,
    "limit": 10,
    "token_env": "OPSGENIE_API_KEY",
    "token_command": None,
}
config = dict(DEFAULTS)
api = ""


def urgent(a: Dict[str, Any]) -> bool:
    """P1 and P2 are urgent until someone acknowledges them."""
    return not a.get("acknowledged") and a.get("priority") in ("P1", "P2")


def item_status(a: Dict[str, Any]) -> Status:
    if urgent(a):
        return "error"
    if a.get("acknowledged") or a.get("priority") == "P3":
        return "warn"
    # Open P4 and P5 alerts that nobody has looked at yet are not healthy, just not urgent.
    return "unknown"


def view(alerts: List[Dict[str, Any]], now: Optional[float] = None) -> View:
    """The view for the alerts the query matched, as GET /v2/alerts returns them."""
    open_ = [a for a in alerts if a.get("status") != "closed"]
    unacked = sum(1 for a in open_ if not a.get("acknowledged"))
    p1 = sum(1 for a in open_ if a.get("priority") == "P1")
    shown = sorted(
        open_,
        key=lambda a: (bool(a.get("acknowledged")), str(a.get("priority") or "P9"), -(parse_time(a.get("createdAt")) or 0)),
    )
    items: List[ListItem] = []
    for a in shown[: config["limit"]]:
        n = a.get("count")
        tags = (["acked"] if a.get("acknowledged") else []) + (["snoozed"] if a.get("snoozed") else [])
        tags += [t for t in a.get("tags") or [] if isinstance(t, str)][:2]
        item: ListItem = {
            "title": str(a.get("message") or f"alert {a.get('tinyId') or a.get('id')}"),
            "subtitle": join(
                [
                    a.get("priority"),
                    a.get("owner") or "no owner",
                    isinstance(n, int) and n > 1 and f"{n}×",
                    ago(a.get("createdAt"), now),
                ]
            ),
            "status": item_status(a),
            "tags": tags,
        }
        if config["account"]:
            app = "app.eu.opsgenie.com" if config["region"] == "eu" else "app.opsgenie.com"
            url = f"https://{config['account']}.{app}/alert/detail/{a.get('id')}/details"
            item["action"] = {"type": "open_url", "url": url}
        items.append(item)
    status: Status = "error" if any(urgent(a) for a in open_) else "warn" if open_ else "ok"
    summary = f"{len(open_)} open · {unacked} unacked · {p1} P1" if open_ else "No open alerts"
    return {
        "status": status,
        "summary": summary,
        "root": {
            "type": "column",
            "children": [
                {
                    "type": "row",
                    "children": [
                        {"type": "stat", "label": "Open", "value": upto(len(open_)), "status": "warn" if open_ else "ok"},
                        {"type": "stat", "label": "Unacked", "value": str(unacked), "status": "warn" if unacked else "ok"},
                        {"type": "stat", "label": "P1", "value": str(p1), "status": "error" if p1 else "ok"},
                    ],
                },
                {"type": "list", "items": items, "empty_text": "No open alerts"},
            ],
        },
    }


def init(c: Any) -> None:
    global api
    config.update(with_defaults(c, DEFAULTS))
    if config["region"] not in ("us", "eu"):
        raise config_error("region must be us or eu")
    host = "https://api.eu.opsgenie.com" if config["region"] == "eu" else "https://api.opsgenie.com"
    api = host if config["base_url"] is None else base_url(config["base_url"], "base_url")
    if not isinstance(config["query"], str):
        raise config_error("query must be text, e.g. status:open")
    account = config["account"]
    if account is not None and (not isinstance(account, str) or not re.match(r"^[a-zA-Z0-9-]+$", account)):
        raise config_error("account must be your Opsgenie subdomain, e.g. acme for acme.app.opsgenie.com")
    strings(config["token_command"], "token_command")
    count(config["limit"], "limit")
    env_name(config["token_env"], "token_env")


def render() -> View:
    key = read_token(config["token_env"], config["token_command"], "Opsgenie API key")
    q = urllib.parse.urlencode({"query": config["query"], "limit": "100", "sort": "createdAt", "order": "desc"})
    reply = get_json("Opsgenie", f"{api}/v2/alerts?{q}", {"Authorization": f"GenieKey {key}"})
    alerts = reply.get("data") if isinstance(reply, dict) else None
    if not isinstance(alerts, list):
        raise upstream_error("Opsgenie: the reply has no data list")
    return view([a for a in alerts if isinstance(a, dict)])


if __name__ == "__main__":
    serve(
        manifest={
            "id": "opsgenie",
            "name": "Opsgenie alerts",
            "refresh": {"mode": "poll", "interval_ms": 60000, "min_interval_ms": 30000},
            "layout": {"size": "medium", "priority": 0, "min_size": "small"},
        },
        init=init,
        render=render,
    )
