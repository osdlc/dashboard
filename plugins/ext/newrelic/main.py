#!/usr/bin/env python3
"""New Relic alerts: open violations, critical first, each linking to its incident. Built from
the public REST API v2 docs (GET /v2/alerts_violations.json); untested live.

    modules:
      newrelic:
        package: newrelic
        config: { region: eu, account_id: 1234567 }

Needs a user API key: $NEW_RELIC_API_KEY, or a command that prints it (token_command).
"""

from typing import Any, Dict, List, Optional

from dashboard_plugin import ListItem, Status, View, config_error, serve, upstream_error, with_defaults
from ops import ago, base_url, count, env_name, get_json, join, read_token, strings

DEFAULTS: Dict[str, Any] = {
    "region": "us",
    "base_url": None,
    "account_id": None,
    "policies": None,
    "limit": 10,
    "token_env": "NEW_RELIC_API_KEY",
    "token_command": None,
}
config = dict(DEFAULTS)
api = ""


def item_status(v: Dict[str, Any]) -> Status:
    p = v.get("priority")
    return "error" if p == "Critical" else "warn" if p == "Warning" else "unknown"


def link(v: Dict[str, Any]) -> str:
    incident = (v.get("links") or {}).get("incident_id")
    if config["account_id"] is not None and incident is not None:
        host = "alerts.eu.newrelic.com" if config["region"] == "eu" else "alerts.newrelic.com"
        return f"https://{host}/accounts/{config['account_id']}/incidents/{incident}"
    return "https://one.eu.newrelic.com/alerts-ai" if config["region"] == "eu" else "https://one.newrelic.com/alerts-ai"


def opened(v: Dict[str, Any]) -> float:
    t = v.get("opened_at")
    return t if isinstance(t, (int, float)) and not isinstance(t, bool) else 0


def view(violations: List[Dict[str, Any]], now: Optional[float] = None) -> View:
    """The view for the open violations, as GET /v2/alerts_violations.json returns them."""
    policies = config["policies"]
    open_ = [v for v in violations if policies is None or v.get("policy_name") in policies]
    critical = sum(1 for v in open_ if v.get("priority") == "Critical")
    warning = sum(1 for v in open_ if v.get("priority") == "Warning")
    policy_count = len({v.get("policy_name") for v in open_})
    shown = sorted(open_, key=lambda v: (v.get("priority") != "Critical", -opened(v)))
    items: List[ListItem] = []
    for v in shown[: config["limit"]]:
        entity = v.get("entity") or {}
        if v.get("condition_name") and entity.get("name"):
            title = f"{v['condition_name']}: {entity['name']}"
        else:
            title = str(v.get("label") or f"violation {v.get('id')}")
        items.append(
            {
                "title": title,
                "subtitle": join([v.get("policy_name"), v.get("priority"), opened(v) and f"opened {ago(opened(v), now)}"]),
                "status": item_status(v),
                "tags": [entity["type"]] if isinstance(entity.get("type"), str) else [],
                "action": {"type": "open_url", "url": link(v)},
            }
        )
    status: Status = "error" if critical else "warn" if open_ else "ok"
    summary = f"{critical} critical · {warning} warning" if open_ else "No open violations"
    return {
        "status": status,
        "summary": summary,
        "root": {
            "type": "column",
            "children": [
                {
                    "type": "row",
                    "children": [
                        {"type": "stat", "label": "Critical", "value": str(critical), "status": "error" if critical else "ok"},
                        {"type": "stat", "label": "Warning", "value": str(warning), "status": "warn" if warning else "ok"},
                        {"type": "stat", "label": "Policies", "value": str(policy_count)},
                    ],
                },
                {"type": "list", "items": items, "empty_text": "No open violations"},
            ],
        },
    }


def init(c: Any) -> None:
    global api
    config.update(with_defaults(c, DEFAULTS))
    if config["region"] not in ("us", "eu"):
        raise config_error("region must be us or eu")
    host = "https://api.eu.newrelic.com" if config["region"] == "eu" else "https://api.newrelic.com"
    api = host if config["base_url"] is None else base_url(config["base_url"], "base_url")
    account = config["account_id"]
    if account is not None and (not isinstance(account, int) or isinstance(account, bool)):
        raise config_error("account_id must be your New Relic account number")
    strings(config["policies"], "policies")
    strings(config["token_command"], "token_command")
    count(config["limit"], "limit")
    env_name(config["token_env"], "token_env")


def render() -> View:
    key = read_token(config["token_env"], config["token_command"], "New Relic API key")
    reply = get_json("New Relic", f"{api}/v2/alerts_violations.json?only_open=true", {"Api-Key": key})
    violations = reply.get("violations") if isinstance(reply, dict) else None
    if not isinstance(violations, list):
        raise upstream_error("New Relic: the reply has no violations list")
    return view([v for v in violations if isinstance(v, dict)])


if __name__ == "__main__":
    serve(
        manifest={
            "id": "newrelic",
            "name": "New Relic alerts",
            "refresh": {"mode": "poll", "interval_ms": 60000, "min_interval_ms": 30000},
            "layout": {"size": "medium", "priority": 0, "min_size": "small"},
        },
        init=init,
        render=render,
    )
