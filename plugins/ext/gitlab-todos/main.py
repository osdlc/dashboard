#!/usr/bin/env python3
"""Your pending GitLab to-do items (`GET /api/v4/todos`): review requests, mentions, failed
pipelines, assignments. Clicking one opens what it points at.

Untested live: built from the GitLab REST API docs and a hand-written sample response
(sample.json). The token comes from $GITLAB_TOKEN, another variable (`token_env`) or a
command (`token_command`), never from the config file.

    dashboard plugin-check -- python3 plugins/ext/gitlab-todos/main.py --sample

    modules:
      todos:
        package: gitlab-todos
        config: { url: https://gitlab.example.com, type: MergeRequest }

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
from dashboard_plugin import ListItem, View, config_error, serve, upstream_error, with_defaults  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
# How long one request may take, in seconds.
TIMEOUT = 20
# Ages in --sample mode are as of this time (2026-10-27T12:00:00Z), so the view never changes.
SAMPLE_NOW = 1793102400

# What each `action_name` says, after "@author" when someone else caused it, and its color:
# a failed pipeline or a merge request that can't merge is red, someone waiting on you yellow.
ACTIONS = {
    "assigned": ("assigned you", None),
    "mentioned": ("mentioned you", None),
    "directly_addressed": ("addressed you", None),
    "build_failed": ("pipeline failed", "error"),
    "unmergeable": ("can't be merged", "error"),
    "merge_train_removed": ("removed from the merge train", "error"),
    "approval_required": ("asked for your approval", "warn"),
    "review_requested": ("asked for your review", "warn"),
    "review_submitted": ("reviewed it", None),
    "member_access_requested": ("asked for access", "warn"),
    "marked": ("to-do added", None),
}
# Actions you caused yourself, or the system did, where "@you" says nothing.
NO_AUTHOR = ("build_failed", "unmergeable", "marked", "merge_train_removed")
TARGETS = {"MergeRequest": "MR", "Issue": "issue", "Commit": "commit", "Epic": "epic", "DesignManagement::Design": "design", "AlertManagement::Alert": "alert"}

config: Dict[str, Any] = {
    "url": "https://gitlab.com",
    "token_env": None,
    "token_command": None,
    "limit": 20,
    "type": None,
    "action": None,
}
SAMPLE = "--sample" in sys.argv[1:]

# A GET of an API path (like "/api/v4/todos") with query parameters, returning parsed JSON.
Get = Callable[[str, Dict[str, Any]], Any]


def fetch(get: Get) -> Any:
    params: Dict[str, Any] = {"state": "pending", "per_page": config["limit"]}
    for key in ("type", "action"):
        if config[key] is not None:
            params[key] = config[key]
    return get("/api/v4/todos", params)


def view(reply: Any, limit: int, now: int) -> View:
    """The tile: one list, newest first, as the API sends it."""
    if not isinstance(reply, list):
        raise upstream_error("expected a list from /api/v4/todos")
    items = [i for i in (todo(t, now) for t in reply[:limit]) if i is not None]
    waiting = sum(1 for i in items if i.get("status") in ("warn", "error"))
    summary = "1 to-do" if len(items) == 1 else f"{len(items)} to-dos"
    if waiting:
        summary += f" · {waiting} need you"
    return {
        "status": "ok",
        "summary": summary,
        "root": {"type": "list", "items": items, "empty_text": "Nothing to do"},
    }


def todo(t: Any, now: int) -> Optional[ListItem]:
    """One to-do item. Without a link it is left out."""
    if not isinstance(t, dict) or not isinstance(t.get("target_url"), str):
        return None
    target = t.get("target") if isinstance(t.get("target"), dict) else {}
    title = target.get("title") if isinstance(target.get("title"), str) else t.get("body")
    if not isinstance(title, str) or not title:
        return None
    action = t.get("action_name")
    label, status = ACTIONS.get(action, (str(action or "to-do").replace("_", " "), None))
    author = (t.get("author") or {}).get("username")
    if isinstance(author, str) and action not in NO_AUTHOR:
        label = f"@{author} {label}"
    sub = []
    ref = reference(t, target)
    if ref:
        sub.append(ref)
    sub.append(label)
    if isinstance(t.get("created_at"), str):
        sub.append(ago(t["created_at"], now))
    kind = TARGETS.get(t.get("target_type"), None)
    item: ListItem = {
        "title": title,
        "subtitle": " · ".join(sub),
        "tags": [kind] if kind else [],
        "action": {"type": "open_url", "url": t["target_url"]},
    }
    if status:
        item["status"] = status
    return item


def reference(t: Dict[str, Any], target: Dict[str, Any]) -> Optional[str]:
    """ "acme/web!42" for a merge request, "acme/web#7" for an issue, else the project."""
    full = (target.get("references") or {}).get("full")
    if isinstance(full, str):
        return full
    project = (t.get("project") or {}).get("path_with_namespace")
    iid = target.get("iid")
    if isinstance(project, str) and isinstance(iid, int):
        sign = "!" if t.get("target_type") == "MergeRequest" else "#"
        return f"{project}{sign}{iid}"
    return project if isinstance(project, str) else None


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
        headers={"PRIVATE-TOKEN": key, "Accept": "application/json", "User-Agent": "dashboard-plugin gitlab-todos"},
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
    if config["type"] is not None and config["type"] not in TARGETS:
        raise config_error(f"type: expected one of {', '.join(TARGETS)}")
    if config["action"] is not None and config["action"] not in ACTIONS:
        raise config_error(f"action: expected one of {', '.join(ACTIONS)}")


def render() -> View:
    get = sample_get if SAMPLE else getter(token())
    now = SAMPLE_NOW if SAMPLE else int(time.time())
    return view(fetch(get), config["limit"], now)


if __name__ == "__main__":
    serve(
        manifest={
            "id": "gitlab-todos",
            "name": "GitLab to-dos",
            "refresh": {"mode": "poll", "interval_ms": 120000, "min_interval_ms": 30000},
            "layout": {"size": "medium", "priority": 20, "min_size": "small"},
        },
        init=init,
        render=render,
    )
