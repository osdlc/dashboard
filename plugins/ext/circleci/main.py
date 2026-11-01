#!/usr/bin/env python3
"""The latest CircleCI pipelines of one or more projects (API v2), with each pipeline's
workflows: failed in red, on hold in yellow, passed in green. The tile is red while any
project's newest pipeline is failing. Clicking a pipeline opens it in the CircleCI app.

Untested live: built from the CircleCI API v2 docs and hand-written sample responses
(sample.json). The token comes from $CIRCLECI_TOKEN, another variable (`token_env`) or a
command (`token_command`), never from the config file.

    dashboard plugin-check -- python3 plugins/ext/circleci/main.py --sample

    modules:
      ci:
        package: circleci
        config: { projects: [gh/acme/api, gh/acme/web], branch: main }

`--sample` reads sample.json instead of calling CircleCI, with ages as of a fixed time.
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
from concurrent.futures import ThreadPoolExecutor
from typing import Any, Callable, Dict, List, Optional

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from dashboard_plugin import ListItem, View, config_error, serve, upstream_error, with_defaults  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
# How long one request may take, in seconds.
TIMEOUT = 20
# Ages in --sample mode are as of this time (2026-10-27T12:00:00Z), so the view never changes.
SAMPLE_NOW = 1793102400

# Workflow status -> (color, how bad: the worst workflow speaks for the pipeline).
STATUSES = {
    "failed": ("error", 5),
    "error": ("error", 5),
    "failing": ("error", 5),
    "unauthorized": ("error", 5),
    "on_hold": ("warn", 4),
    "running": (None, 3),
    "not_run": (None, 2),
    "canceled": (None, 1),
    "success": ("ok", 0),
}
# The app's name for each VCS in a project slug.
VCS = {"gh": "github", "github": "github", "bb": "bitbucket", "bitbucket": "bitbucket", "circleci": "circleci"}

config: Dict[str, Any] = {
    "url": "https://circleci.com",
    "app_url": "https://app.circleci.com",
    "projects": None,
    "branch": None,
    "limit": 3,
    "token_env": None,
    "token_command": None,
}
SAMPLE = "--sample" in sys.argv[1:]
# What --sample runs with when the config leaves these out.
SAMPLE_CONFIG = {"projects": ["gh/acme/api", "gh/acme/web"]}

# A GET of an API path (like "/api/v2/pipeline/<id>/workflow") with query parameters,
# returning parsed JSON.
Get = Callable[[str, Dict[str, Any]], Any]


def fetch(get: Get, projects: List[str], branch: Optional[str], limit: int) -> Dict[str, Any]:
    """Each project's pipelines, and each pipeline's workflows by pipeline id."""
    params = {"branch": branch} if branch else {}
    pipelines = {slug: get(f"/api/v2/project/{slug}/pipeline", params) for slug in projects}
    ids = [p["id"] for reply in pipelines.values() for p in items(reply)[:limit] if isinstance(p.get("id"), str)]
    with ThreadPoolExecutor(max_workers=max(1, min(len(ids), 8))) as pool:
        workflows = dict(zip(ids, pool.map(lambda id: get(f"/api/v2/pipeline/{id}/workflow", {}), ids)))
    return {"pipelines": pipelines, "workflows": workflows}


def items(reply: Any) -> List[Dict[str, Any]]:
    """The `items` of a paged reply."""
    if not isinstance(reply, dict) or not isinstance(reply.get("items"), list):
        raise upstream_error("expected a page of items from the CircleCI API")
    return [i for i in reply["items"] if isinstance(i, dict)]


def view(data: Dict[str, Any], app_url: str, limit: int, now: int) -> View:
    """The tile: one list of pipelines, project by project, newest first."""
    out: List[ListItem] = []
    failing = []
    for slug, reply in data["pipelines"].items():
        for n, p in enumerate(items(reply)[:limit]):
            wfs = data["workflows"].get(p.get("id"))
            item = pipeline(slug, p, items(wfs) if wfs is not None else [], app_url, now)
            if n == 0 and item.get("status") == "error":
                failing.append(short(slug))
            out.append(item)
    summary = "1 pipeline" if len(out) == 1 else f"{len(out)} pipelines"
    if failing:
        summary += f" · {', '.join(failing)} failing"
    return {
        "status": "error" if failing else "ok",
        "summary": summary,
        "root": {"type": "list", "items": out, "empty_text": "no pipelines"},
    }


def pipeline(slug: str, p: Dict[str, Any], workflows: List[Dict[str, Any]], app_url: str, now: int) -> ListItem:
    """One pipeline: its commit as the title, and "api #1432 · main · build failed · 2h ago"."""
    number = p.get("number")
    vcs = p.get("vcs") if isinstance(p.get("vcs"), dict) else {}
    subject = (vcs.get("commit") or {}).get("subject")
    title = subject if isinstance(subject, str) and subject else f"Pipeline #{number}"
    sub = [f"{short(slug)} #{number}"]
    branch = vcs.get("branch") or vcs.get("tag")
    if isinstance(branch, str):
        sub.append(branch)

    worst: Optional[Dict[str, Any]] = None
    for w in workflows:
        if worst is None or rank(w) > rank(worst):
            worst = w
    color = None
    if p.get("state") == "errored":
        # The config didn't compile, so no workflow ran.
        sub.append("config error")
        color = "error"
    elif worst is not None:
        status = str(worst.get("status"))
        color = STATUSES.get(status, (None, 0))[0]
        failed = [str(w.get("name")) for w in workflows if STATUSES.get(str(w.get("status")), (None, 0))[0] == "error"]
        if failed:
            sub.append(f"{', '.join(failed)} failed")
        else:
            sub.append("passed" if status == "success" else status.replace("_", " "))
    if isinstance(p.get("created_at"), str):
        sub.append(ago(p["created_at"], now))

    link = f"{app_url.rstrip('/')}/pipelines/{app_slug(slug)}/{number}"
    if worst is not None and isinstance(worst.get("id"), str):
        link += f"/workflows/{worst['id']}"
    actor = ((p.get("trigger") or {}).get("actor") or {}).get("login")
    item: ListItem = {
        "title": title,
        "subtitle": " · ".join(sub),
        "tags": [f"@{actor}"] if isinstance(actor, str) else [],
        "action": {"type": "open_url", "url": link},
    }
    if color:
        item["status"] = color
    return item


def rank(w: Dict[str, Any]) -> int:
    return STATUSES.get(str(w.get("status")), (None, 0))[1]


def short(slug: str) -> str:
    """ "gh/acme/api" -> "api"."""
    return slug.rstrip("/").rsplit("/", 1)[-1]


def app_slug(slug: str) -> str:
    """ "gh/acme/api" -> "github/acme/api", as the app's links spell it."""
    vcs, _, rest = slug.partition("/")
    return f"{VCS.get(vcs, vcs)}/{rest}"


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
    """The API token: from `token_command`, else the variable (default CIRCLECI_TOKEN)."""
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
    var = config["token_env"] or "CIRCLECI_TOKEN"
    value = os.environ.get(var, "").strip()
    if not value:
        raise upstream_error(
            f"no API token: set ${var} before starting the dashboard, or set token_env / token_command in the config"
        )
    return value


def http_get(path: str, params: Dict[str, Any], key: str) -> Any:
    """GET from the CircleCI API with the token in the Circle-Token header."""
    url = config["url"].rstrip("/") + urllib.parse.quote(path) + ("?" + urllib.parse.urlencode(params) if params else "")
    req = urllib.request.Request(
        url,
        headers={"Circle-Token": key, "Accept": "application/json", "User-Agent": "dashboard-plugin circleci"},
    )
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
            return json.load(resp)
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", "replace")[:300]
        raise upstream_error(f"CircleCI: HTTP {e.code}: {body}")
    except (OSError, ValueError) as e:
        raise upstream_error(f"CircleCI {path}: {e}")


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
    if SAMPLE:
        c = {**SAMPLE_CONFIG, **(c or {})}
    config.update(with_defaults(c, config))
    for key in ("url", "app_url"):
        if not isinstance(config[key], str) or not re.match(r"^https?://", config[key]):
            raise config_error(f"{key}: expected an http(s) URL")
    projects = config["projects"]
    if (
        not isinstance(projects, list)
        or not projects
        or not all(isinstance(p, str) and re.match(r"^[\w.-]+/[\w.-]+/[\w.-]+$", p) for p in projects)
    ):
        raise config_error("projects: expected project slugs, like [gh/acme/api, bb/acme/web]")
    if config["branch"] is not None and not isinstance(config["branch"], str):
        raise config_error("branch: expected a branch name")
    limit = config["limit"]
    if not isinstance(limit, int) or isinstance(limit, bool) or not 1 <= limit <= 20:
        raise config_error("limit: expected a whole number from 1 to 20")
    if config["token_env"] is not None and not isinstance(config["token_env"], str):
        raise config_error("token_env: expected a variable name")
    argv = config["token_command"]
    if argv is not None and (not isinstance(argv, list) or not argv or not all(isinstance(a, str) for a in argv)):
        raise config_error("token_command: expected a program and its arguments, like [op, read, op://dev/circleci]")


def render() -> View:
    get = sample_get if SAMPLE else getter(token())
    now = SAMPLE_NOW if SAMPLE else int(time.time())
    data = fetch(get, config["projects"], config["branch"], config["limit"])
    return view(data, config["app_url"], config["limit"], now)


if __name__ == "__main__":
    serve(
        manifest={
            "id": "circleci",
            "name": "CircleCI",
            "refresh": {"mode": "poll", "interval_ms": 60000, "min_interval_ms": 15000},
            "layout": {"size": "medium", "priority": 20, "min_size": "small"},
        },
        init=init,
        render=render,
    )
