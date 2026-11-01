#!/usr/bin/env python3
"""Gerrit changes, one list per search query (`GET /changes/?q=...`), with their votes:
Code-Review and Verified as tags, a -2 or a failed Verified in red, a -1 in yellow, a +2 or a
submittable change in green. Clicking a change opens it.

Untested live with a login: built from the Gerrit REST API docs and a hand-written sample
response (sample.json). With `auth: none` it runs anonymous queries on a public server.
Logins use your Gerrit username and HTTP password (Settings > HTTP Credentials), from
$GERRIT_HTTP_PASSWORD, another variable (`token_env`) or a command (`token_command`), never
from the config file.

    dashboard plugin-check -- python3 plugins/ext/gerrit/main.py --sample

    modules:
      gerrit:
        package: gerrit
        config:
          url: https://review.example.com
          username: dana
          sections:
            - { label: Mine, query: "is:open owner:self" }
            - { label: To review, query: "is:open reviewer:self -owner:self -is:wip" }

`--sample` reads sample.json instead of calling Gerrit, with ages as of a fixed time.
"""

import base64
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
# The labels shown first, with their short name and top vote.
VOTES = {"Code-Review": ("CR", 2), "Verified": ("V", 1)}
# Gerrit starts every JSON reply with this line, against cross-site script inclusion.
MAGIC = ")]}'"

DEFAULT_SECTIONS = [
    {"label": "Mine", "query": "is:open owner:self"},
    {"label": "To review", "query": "is:open reviewer:self -owner:self -is:wip"},
]

config: Dict[str, Any] = {
    "url": None,
    "auth": "basic",
    "username": None,
    "token_env": None,
    "token_command": None,
    "limit": 10,
    "sections": DEFAULT_SECTIONS,
}
SAMPLE = "--sample" in sys.argv[1:]
# What --sample runs with when the config leaves these out.
SAMPLE_CONFIG = {"url": "https://review.example.com", "username": "dana"}

# A GET of an API path (like "/changes/") with query parameters, returning the reply's text.
Get = Callable[[str, List[Any]], str]


def fetch(get: Get, sections: List[Dict[str, Any]], limit: int) -> str:
    """All sections in one request: Gerrit answers several `q` with a list per query."""
    params: List[Any] = [("q", s["query"]) for s in sections]
    params += [("n", limit), ("o", "LABELS"), ("o", "DETAILED_ACCOUNTS"), ("o", "SUBMITTABLE")]
    path = "/a/changes/" if config["auth"] == "basic" else "/changes/"
    return get(path, params)


def parse(text: str, queries: int) -> List[Any]:
    """The reply's lists of changes, one per query. A single query gets one flat list."""
    text = text.lstrip()
    if text.startswith(MAGIC):
        text = text[len(MAGIC) :]
    try:
        data = json.loads(text)
    except ValueError:
        raise upstream_error("Gerrit sent something that isn't JSON")
    if not isinstance(data, list):
        raise upstream_error("expected a list of changes from /changes/")
    if queries == 1:
        data = [data]
    if len(data) != queries or not all(isinstance(d, list) for d in data):
        raise upstream_error(f"expected {queries} lists of changes from /changes/")
    return data


def view(sections: List[Dict[str, Any]], replies: List[Any], url: str, limit: int, now: int) -> View:
    """The tile: a heading and a list per section, like the built-in trackers."""
    children: List[Widget] = []
    counts = []
    for s, reply in zip(sections, replies):
        items = [i for i in (change(c, url, now) for c in reply[:limit]) if i is not None]
        counts.append(f"{len(items)} {s['label'].lower()}")
        children.append({"type": "text", "text": f"**{s['label']}**", "markdown": True})
        children.append({"type": "list", "items": items, "empty_text": "none"})
    return {"status": "ok", "summary": " · ".join(counts), "root": {"type": "column", "children": children}}


def change(c: Any, url: str, now: int) -> Optional[ListItem]:
    """One change. Its votes are tags ("CR+2", "V-1") and set its color."""
    if not isinstance(c, dict) or not isinstance(c.get("subject"), str) or not isinstance(c.get("_number"), int):
        return None
    project = c.get("project") if isinstance(c.get("project"), str) else ""
    number = c["_number"]
    sub = [f"{project} {number}".strip()]
    if isinstance(c.get("branch"), str):
        sub.append(c["branch"])
    owner = c.get("owner") or {}
    name = owner.get("name") or owner.get("username")
    if isinstance(name, str):
        sub.append(f"by {name}")
    unresolved = c.get("unresolved_comment_count")
    if isinstance(unresolved, int) and unresolved > 0:
        sub.append(f"{unresolved} unresolved")
    if isinstance(c.get("updated"), str):
        sub.append(f"updated {ago(c['updated'], now)}")

    tags = []
    if c.get("work_in_progress") is True:
        tags.append("wip")
    # Code-Review and Verified first, then any other labels. Only those two set the color: other
    # labels mean what each server decides (OpenStack's Workflow-1 is "work in progress").
    labels = c.get("labels") if isinstance(c.get("labels"), dict) else {}
    rejected = disliked = approved = False
    for label in sorted(labels, key=lambda name: (name not in VOTES, VOTES.get(name, (name,))[0])):
        info = labels[label]
        if not isinstance(info, dict):
            continue
        short, top = VOTES.get(label, (label, 1))
        core = label in VOTES
        if "rejected" in info:
            tags.append(f"{short}-{top}")
            rejected = rejected or core
        elif "approved" in info:
            tags.append(f"{short}+{top}")
            approved = approved or label == "Code-Review"
        elif "disliked" in info:
            tags.append(f"{short}-1")
            disliked = disliked or core
        elif "recommended" in info:
            tags.append(f"{short}+1")

    link = f"{url.rstrip('/')}/c/{urllib.parse.quote(project)}/+/{number}" if project else f"{url.rstrip('/')}/{number}"
    item: ListItem = {
        "title": c["subject"],
        "subtitle": " · ".join(sub),
        "tags": tags,
        "action": {"type": "open_url", "url": link},
    }
    if rejected:
        item["status"] = "error"
    elif disliked:
        item["status"] = "warn"
    elif approved or c.get("submittable") is True:
        item["status"] = "ok"
    return item


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
    """Unix seconds from Gerrit's "2026-10-27 10:00:00.000000000" (UTC) or an ISO time."""
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


def password() -> str:
    """The HTTP password: from `token_command`, else the variable (default GERRIT_HTTP_PASSWORD)."""
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
    var = config["token_env"] or "GERRIT_HTTP_PASSWORD"
    value = os.environ.get(var, "").strip()
    if not value:
        raise upstream_error(
            f"no HTTP password: set ${var} before starting the dashboard, or set token_env / token_command in the config"
        )
    return value


def http_get(path: str, params: List[Any], secret: Optional[str]) -> str:
    """GET from Gerrit, with HTTP basic auth unless `auth: none`."""
    url = config["url"].rstrip("/") + path + "?" + urllib.parse.urlencode(params)
    headers = {"Accept": "application/json", "User-Agent": "dashboard-plugin gerrit"}
    if config["auth"] == "basic":
        login = f"{config['username']}:{secret}".encode()
        headers["Authorization"] = "Basic " + base64.b64encode(login).decode()
    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
            return resp.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", "replace")[:300]
        raise upstream_error(f"Gerrit: HTTP {e.code}: {body}")
    except OSError as e:
        raise upstream_error(f"Gerrit {path}: {e}")


def getter(secret: Optional[str]) -> Get:
    """http_get with `secret`, so token_command runs once per render, not once per request."""
    return lambda path, params: http_get(path, params, secret)


def sample_get(path: str, _params: List[Any]) -> str:
    """The reply for `path` from sample.json (a hand-written API response per path), as Gerrit
    sends it: after the MAGIC line."""
    with open(os.path.join(HERE, "sample.json"), encoding="utf-8") as f:
        replies = json.load(f)
    if path not in replies:
        raise upstream_error(f"sample.json has no reply for {path}")
    return MAGIC + "\n" + json.dumps(replies[path])


def init(c: Dict[str, Any]) -> None:
    if SAMPLE:
        c = {**SAMPLE_CONFIG, **(c or {})}
    config.update(with_defaults(c, config))
    url = config["url"]
    if not isinstance(url, str) or not re.match(r"^https?://", url):
        raise config_error("url: expected the server's http(s) URL, like https://review.example.com")
    if config["auth"] not in ("basic", "none"):
        raise config_error("auth: expected basic (username and HTTP password) or none")
    if config["auth"] == "basic" and (not isinstance(config["username"], str) or not config["username"]):
        raise config_error("username: needed for auth: basic (or set auth: none for anonymous queries)")
    limit = config["limit"]
    if not isinstance(limit, int) or isinstance(limit, bool) or not 1 <= limit <= 100:
        raise config_error("limit: expected a whole number from 1 to 100")
    if config["token_env"] is not None and not isinstance(config["token_env"], str):
        raise config_error("token_env: expected a variable name")
    argv = config["token_command"]
    if argv is not None and (not isinstance(argv, list) or not argv or not all(isinstance(a, str) for a in argv)):
        raise config_error("token_command: expected a program and its arguments, like [op, read, op://dev/gerrit]")
    sections = config["sections"]
    if not isinstance(sections, list) or not sections:
        raise config_error("sections: expected a list of { label, query }")
    for n, s in enumerate(sections):
        if not isinstance(s, dict) or set(s) != {"label", "query"} or not all(isinstance(v, str) and v for v in s.values()):
            raise config_error(f"sections[{n}]: expected {{ label, query }}, like {{ label: Mine, query: 'is:open owner:self' }}")


def render() -> View:
    get = sample_get if SAMPLE else getter(password() if config["auth"] == "basic" else None)
    now = SAMPLE_NOW if SAMPLE else int(time.time())
    sections = config["sections"]
    replies = parse(fetch(get, sections, config["limit"]), len(sections))
    return view(sections, replies, config["url"], config["limit"], now)


if __name__ == "__main__":
    serve(
        manifest={
            "id": "gerrit",
            "name": "Gerrit",
            "refresh": {"mode": "poll", "interval_ms": 120000, "min_interval_ms": 30000},
            "layout": {"size": "tall", "priority": 20, "min_size": "medium"},
        },
        init=init,
        render=render,
    )
