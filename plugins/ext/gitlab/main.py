#!/usr/bin/env python3
"""GitLab merge requests and issues, one list per section. Each section is a call to the
REST API (`/api/v4/merge_requests` or `/api/v4/issues`) with your own query parameters.
Clicking an item opens it on GitLab. Works with gitlab.com and self-managed GitLab.

Untested live: built from the GitLab REST API docs and a hand-written sample response
(sample.json). The token comes from $GITLAB_TOKEN, another variable (`token_env`) or a
command (`token_command`), never from the config file.

    dashboard plugin-check -- python3 plugins/ext/gitlab/main.py --sample

    modules:
      gitlab:
        package: gitlab
        config:
          url: https://gitlab.example.com
          sections:
            - { label: Review, type: merge_requests, params: { reviewer_username: dana, state: opened } }
            - { label: Issues, type: issues, params: { scope: assigned_to_me, state: opened } }

`--sample` reads sample.json instead of calling GitLab, with ages as of a fixed time.
"""

import calendar
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Callable, Dict, List, Optional

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from dashboard_plugin import ListItem, View, Widget, config_error, serve, upstream_error, with_defaults  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
# How long one request may take, in seconds.
TIMEOUT = 20
# Ages in --sample mode are as of this time (2026-10-27T12:00:00Z), so the view never changes.
SAMPLE_NOW = 1793102400
TYPES = ("merge_requests", "issues")

DEFAULT_SECTIONS = [
    {"label": "My merge requests", "type": "merge_requests", "params": {"scope": "created_by_me", "state": "opened"}},
    {"label": "Assigned issues", "type": "issues", "params": {"scope": "assigned_to_me", "state": "opened"}},
]

config: Dict[str, Any] = {
    "url": "https://gitlab.com",
    "token_env": None,
    "token_command": None,
    "limit": 10,
    "sections": DEFAULT_SECTIONS,
}
SAMPLE = "--sample" in sys.argv[1:]

# A GET of an API path (like "/api/v4/issues") with query parameters, returning parsed JSON.
Get = Callable[[str, Dict[str, Any]], Any]


def fetch(get: Get, sections: List[Dict[str, Any]], limit: int) -> List[Any]:
    """One API reply per section, in order."""
    out = []
    for s in sections:
        params = {"per_page": limit, "order_by": "updated_at", "sort": "desc", **s["params"]}
        out.append(get(f"/api/v4/{s['type']}", params))
    return out


def view(sections: List[Dict[str, Any]], replies: List[Any], limit: int, now: int) -> View:
    """The tile: a heading and a list per section, like the built-in trackers."""
    children: List[Widget] = []
    counts = []
    for s, reply in zip(sections, replies):
        if not isinstance(reply, list):
            raise upstream_error(f"{s['label']}: expected a list from /api/v4/{s['type']}")
        parse = merge_request if s["type"] == "merge_requests" else issue
        items = [i for i in (parse(r, now) for r in reply[:limit]) if i is not None]
        counts.append(f"{len(items)} {s['label'].lower()}")
        children.append({"type": "text", "text": f"**{s['label']}**", "markdown": True})
        children.append({"type": "list", "items": items, "empty_text": "none"})
    return {"status": "ok", "summary": " · ".join(counts), "root": {"type": "column", "children": children}}


def merge_request(mr: Any, now: int) -> Optional[ListItem]:
    """One merge request. Conflicts color it red, ready to merge green."""
    if not isinstance(mr, dict) or not isinstance(mr.get("title"), str) or not isinstance(mr.get("web_url"), str):
        return None
    sub = [reference(mr, "!")]
    author = (mr.get("author") or {}).get("username")
    if isinstance(author, str):
        sub.append(f"by @{author}")
    if isinstance(mr.get("updated_at"), str):
        sub.append(f"updated {ago(mr['updated_at'], now)}")
    tags = []
    if mr.get("draft") is True or mr.get("work_in_progress") is True:
        tags.append("draft")
    if mr.get("has_conflicts") is True:
        tags.append("conflicts")
    tags.extend(labels(mr))
    item: ListItem = {
        "title": mr["title"],
        "subtitle": " · ".join(sub),
        "tags": tags,
        "action": {"type": "open_url", "url": mr["web_url"]},
    }
    status = mr.get("detailed_merge_status")
    if mr.get("has_conflicts") is True or status in ("conflict", "broken_status"):
        item["status"] = "error"
    elif status == "mergeable":
        item["status"] = "ok"
    elif status in ("ci_must_pass", "discussions_not_resolved", "need_rebase", "not_approved", "requested_changes"):
        item["status"] = "warn"
    return item


def issue(i: Any, now: int) -> Optional[ListItem]:
    """One issue. A due date in the past colors it red."""
    if not isinstance(i, dict) or not isinstance(i.get("title"), str) or not isinstance(i.get("web_url"), str):
        return None
    sub = [reference(i, "#")]
    if isinstance(i.get("state"), str) and i["state"] != "opened":
        sub.append(i["state"])
    if isinstance(i.get("updated_at"), str):
        sub.append(f"updated {ago(i['updated_at'], now)}")
    due = i.get("due_date")
    tags = ([f"due {due}"] if isinstance(due, str) else []) + labels(i)
    item: ListItem = {
        "title": i["title"],
        "subtitle": " · ".join(sub),
        "tags": tags[:3],
        "action": {"type": "open_url", "url": i["web_url"]},
    }
    if isinstance(due, str):
        end = parse_time(due + "T23:59:59Z")
        if end is not None and end < now:
            item["status"] = "error"
    return item


def reference(item: Dict[str, Any], sign: str) -> str:
    """ "group/project!12". Falls back to the bare number."""
    full = (item.get("references") or {}).get("full")
    if isinstance(full, str):
        return full
    return f"{sign}{item.get('iid', '?')}"


def labels(item: Dict[str, Any]) -> List[str]:
    """The first three labels. Scoped labels ("priority::high") keep their scope."""
    out = [label for label in item.get("labels") or [] if isinstance(label, str)]
    return out[:3]


def ago(ts: str, now: int) -> str:
    """ "5m ago" style age, like the built-in modules. Unreadable times are shown as they are."""
    then = parse_time(ts)
    if then is None:
        return ts
    s = max(0, now - then)
    if s < 60:
        return "just now"
    if s < 3600:
        return f"{s // 60}m ago"
    if s < 86400:
        return f"{s // 3600}h ago"
    return f"{s // 86400}d ago"


def parse_time(ts: str) -> Optional[int]:
    """Unix seconds from "2026-10-27T10:00:00.123Z" or with a +hh:mm offset (none = UTC)."""
    m = re.match(r"^(\d{4})-(\d\d)-(\d\d)[T ](\d\d):(\d\d):(\d\d)(?:\.\d+)?(Z|[+-]\d\d:?\d\d)?$", ts.strip())
    if not m:
        return None
    y, mo, d, h, mi, s = (int(x) for x in m.groups()[:6])
    t = calendar.timegm((y, mo, d, h, mi, s))
    off = m.group(7)
    if off and off != "Z":
        digits = off[1:].replace(":", "")
        secs = int(digits[:2]) * 3600 + int(digits[2:]) * 60
        t -= secs if off[0] == "+" else -secs
    return t


def token() -> str:
    """The API token: from `token_command`, else the variable (default GITLAB_TOKEN)."""
    argv = config["token_command"]
    if argv is not None:
        try:
            out = subprocess.run(argv, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=TIMEOUT)
        except (OSError, subprocess.TimeoutExpired) as e:
            raise upstream_error(f"running token_command {argv[0]!r}: {e}")
        if out.returncode != 0:
            raise upstream_error(f"token_command {argv[0]!r} failed: {out.stderr.strip()[:300]}")
        if not out.stdout.strip():
            raise upstream_error(f"token_command {argv[0]!r} printed nothing")
        return out.stdout.strip()
    var = config["token_env"] or "GITLAB_TOKEN"
    value = os.environ.get(var, "").strip()
    if not value:
        raise upstream_error(
            f"no API token: set ${var} before starting the dashboard, or set token_env / token_command in the config"
        )
    return value


def http_get(path: str, params: Dict[str, Any], key: str) -> Any:
    """GET from the GitLab API with the token in the PRIVATE-TOKEN header."""
    url = config["url"].rstrip("/") + path + "?" + urllib.parse.urlencode(params)
    req = urllib.request.Request(
        url,
        headers={"PRIVATE-TOKEN": key, "Accept": "application/json", "User-Agent": "dashboard-plugin gitlab"},
    )
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
            return json.load(resp)
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", "replace")[:300]
        raise upstream_error(f"GitLab: HTTP {e.code}: {body}")
    except (OSError, ValueError) as e:
        raise upstream_error(f"GitLab {path}: {e}")


def getter(key: str) -> Get:
    """http_get with `key`, so token_command runs once per render, not once per request."""
    return lambda path, params: http_get(path, params, key)


def sample_get(path: str, _params: Dict[str, Any]) -> Any:
    """The reply for `path` from sample.json, a hand-written API response per path."""
    with open(os.path.join(HERE, "sample.json"), encoding="utf-8") as f:
        replies = json.load(f)
    if path not in replies:
        raise upstream_error(f"sample.json has no reply for {path}")
    return replies[path]


def init(c: Dict[str, Any]) -> None:
    config.update(with_defaults(c, config))
    url = config["url"]
    if not isinstance(url, str) or not re.match(r"^https?://", url):
        raise config_error("url: expected an http(s) URL, like https://gitlab.com")
    limit = config["limit"]
    if not isinstance(limit, int) or isinstance(limit, bool) or not 1 <= limit <= 100:
        raise config_error("limit: expected a whole number from 1 to 100")
    if config["token_env"] is not None and not isinstance(config["token_env"], str):
        raise config_error("token_env: expected a variable name")
    argv = config["token_command"]
    if argv is not None and (not isinstance(argv, list) or not argv or not all(isinstance(a, str) for a in argv)):
        raise config_error("token_command: expected a program and its arguments, like [op, read, op://dev/gitlab]")
    sections = config["sections"]
    if not isinstance(sections, list) or not sections:
        raise config_error("sections: expected a list of { label, type, params }")
    for n, s in enumerate(sections):
        where = f"sections[{n}]"
        if not isinstance(s, dict) or set(s) - {"label", "type", "params"}:
            raise config_error(f"{where}: expected {{ label, type, params }}")
        if not isinstance(s.get("label"), str) or not s["label"]:
            raise config_error(f"{where}.label: expected a name")
        if s.get("type") not in TYPES:
            raise config_error(f"{where}.type: expected merge_requests or issues")
        params = s.setdefault("params", {})
        if not isinstance(params, dict) or not all(isinstance(v, (str, int)) for v in params.values()):
            raise config_error(f"{where}.params: expected query parameters, like {{ scope: assigned_to_me }}")


def render() -> View:
    get = sample_get if SAMPLE else getter(token())
    now = SAMPLE_NOW if SAMPLE else int(time.time())
    replies = fetch(get, config["sections"], config["limit"])
    return view(config["sections"], replies, config["limit"], now)


if __name__ == "__main__":
    serve(
        manifest={
            "id": "gitlab",
            "name": "GitLab",
            "refresh": {"mode": "poll", "interval_ms": 120000, "min_interval_ms": 30000},
            "layout": {"size": "tall", "priority": 20, "min_size": "medium"},
        },
        init=init,
        render=render,
    )
