#!/usr/bin/env python3
"""PagerDuty: open incidents (triggered first) and who is on call now. Built from the public
REST API v2 docs (GET /incidents and GET /oncalls); untested live.

    modules:
      pagerduty:
        package: pagerduty
        config: { team_ids: [PTEAM01], escalation_policy_ids: [PPOL001] }

Needs a REST API key (read-only is enough): $PAGERDUTY_TOKEN, or a command that prints it
(token_command).
"""

import urllib.parse
from typing import Any, Dict, List, Optional, Tuple

from dashboard_plugin import ListItem, Status, View, config_error, serve, upstream_error, with_defaults
from ops import ago, base_url, count, env_name, get_json, join, parse_time, read_token, strings

DEFAULTS: Dict[str, Any] = {
    "base_url": "https://api.pagerduty.com",
    "service_ids": None,
    "team_ids": None,
    "urgencies": None,
    "escalation_policy_ids": None,
    "oncall": True,
    "limit": 10,
    "token_env": "PAGERDUTY_TOKEN",
    "token_command": None,
}
config = dict(DEFAULTS)
api = ""


def summary_of(obj: Any) -> Optional[str]:
    """The `summary` of a PagerDuty reference ({"id": ..., "summary": "Ana Ruiz"})."""
    s = obj.get("summary") if isinstance(obj, dict) else None
    return s if isinstance(s, str) and s else None


def first_on_call(oncalls: List[Any]) -> List[str]:
    """Who is first on call: the level-1 people, each once."""
    names: List[str] = []
    for o in oncalls:
        if not isinstance(o, dict) or o.get("escalation_level", 1) != 1:
            continue
        name = summary_of(o.get("user"))
        if name and name not in names:
            names.append(name)
    return names


def view(incidents: List[Dict[str, Any]], oncall: Optional[List[str]], now: Optional[float] = None) -> View:
    """The view for the open incidents, and the on-call people (None when not asked for)."""
    triggered = sum(1 for i in incidents if i.get("status") == "triggered")
    acked = sum(1 for i in incidents if i.get("status") == "acknowledged")
    shown = sorted(incidents, key=lambda i: (i.get("status") != "triggered", -(parse_time(i.get("created_at")) or 0)))
    items: List[ListItem] = []
    for i in shown[: config["limit"]]:
        who = [n for n in (summary_of(a.get("assignee")) for a in i.get("assignments") or [] if isinstance(a, dict)) if n]
        status = i.get("status")
        tags = ["acked" if status == "acknowledged" else str(status or "")]
        if i.get("urgency") == "high":
            tags.append("high")
        item: ListItem = {
            "title": f"#{i.get('incident_number') or i.get('id')} {i.get('title') or ''}".strip(),
            "subtitle": join([summary_of(i.get("service")), ", ".join(who) or "unassigned", ago(i.get("created_at"), now)]),
            "status": "error" if status == "triggered" else "warn",
            "tags": [t for t in tags if t],
        }
        if isinstance(i.get("html_url"), str):
            item["action"] = {"type": "open_url", "url": i["html_url"]}
        items.append(item)
    stats: List[Any] = [
        {"type": "stat", "label": "Triggered", "value": str(triggered), "status": "error" if triggered else "ok"},
        {"type": "stat", "label": "Acknowledged", "value": str(acked), "status": "warn" if acked else "ok"},
    ]
    if oncall is not None:
        stats.append({"type": "stat", "label": "On call", "value": ", ".join(oncall[:2]) or "nobody"})
    status_: Status = "error" if triggered else "warn" if acked else "ok"
    parts = [f"{triggered} triggered", f"{acked} acknowledged"] if incidents else ["No open incidents"]
    if oncall:
        parts.append(f"on call: {oncall[0]}")
    return {
        "status": status_,
        "summary": join(parts),
        "root": {
            "type": "column",
            "children": [
                {"type": "row", "children": stats},
                {"type": "list", "items": items, "empty_text": "No open incidents"},
            ],
        },
    }


def init(c: Any) -> None:
    global api
    config.update(with_defaults(c, DEFAULTS))
    api = base_url(config["base_url"], "base_url")
    for key in ("service_ids", "team_ids", "urgencies", "escalation_policy_ids", "token_command"):
        strings(config[key], key)
    if any(u not in ("high", "low") for u in config["urgencies"] or []):
        raise config_error("urgencies must hold high, low or both")
    if not isinstance(config["oncall"], bool):
        raise config_error("oncall must be true or false")
    count(config["limit"], "limit")
    env_name(config["token_env"], "token_env")


def query(pairs: List[Tuple[str, str]], lists: Dict[str, Optional[List[str]]]) -> str:
    """A query string; each list becomes repeated `name[]=value` pairs, as PagerDuty wants."""
    for name, values in lists.items():
        pairs += [(f"{name}[]", v) for v in values or []]
    return urllib.parse.urlencode(pairs)


def render() -> View:
    key = read_token(config["token_env"], config["token_command"], "PagerDuty API key")
    headers = {"Authorization": f"Token token={key}", "Accept": "application/vnd.pagerduty+json;version=2"}

    q = query(
        [("limit", "100"), ("sort_by", "created_at:desc")],
        {
            "statuses": ["triggered", "acknowledged"],
            "service_ids": config["service_ids"],
            "team_ids": config["team_ids"],
            "urgencies": config["urgencies"],
        },
    )
    reply = get_json("PagerDuty", f"{api}/incidents?{q}", headers)
    incidents = reply.get("incidents") if isinstance(reply, dict) else None
    if not isinstance(incidents, list):
        raise upstream_error("PagerDuty: the reply has no incidents list")

    oncall = None
    if config["oncall"]:
        o = query([("earliest", "true"), ("limit", "100")], {"escalation_policy_ids": config["escalation_policy_ids"]})
        r = get_json("PagerDuty", f"{api}/oncalls?{o}", headers)
        oncalls = r.get("oncalls") if isinstance(r, dict) else None
        oncall = first_on_call(oncalls if isinstance(oncalls, list) else [])
    return view([i for i in incidents if isinstance(i, dict)], oncall)


if __name__ == "__main__":
    serve(
        manifest={
            "id": "pagerduty",
            "name": "PagerDuty",
            "refresh": {"mode": "poll", "interval_ms": 60000, "min_interval_ms": 30000},
            "layout": {"size": "medium", "priority": 0, "min_size": "small"},
        },
        init=init,
        render=render,
    )
