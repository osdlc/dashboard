#!/usr/bin/env python3
"""Azure DevOps (Azure Boards and Repos) for one project: work items from a WIQL query and the
active pull requests. Priority 1 work items and rejected or conflicting pull requests show
red, priority 2 and "waiting for author" yellow, approved pull requests green. Clicking an
item opens it.

Untested live: built from the Azure DevOps REST API docs (api-version 7.1) and hand-written
sample responses (sample.json). The personal access token (Work Items and Code, read) comes
from $AZURE_DEVOPS_EXT_PAT (the variable the az CLI uses), another variable (`token_env`) or a
command (`token_command`), never from the config file.

    dashboard plugin-check -- python3 plugins/ext/azure-devops/main.py --sample

    modules:
      azure:
        package: azure-devops
        config: { org: acme, project: Web }

`--sample` reads sample.json instead of calling Azure DevOps, with ages as of a fixed time.
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
API_VERSION = "7.1"

DEFAULT_WIQL = (
    "SELECT [System.Id] FROM WorkItems WHERE [System.TeamProject] = @project"
    " AND [System.AssignedTo] = @Me AND [System.State] NOT IN ('Closed', 'Done', 'Removed')"
    " ORDER BY [System.ChangedDate] DESC"
)
FIELDS = [
    "System.Id",
    "System.Title",
    "System.State",
    "System.WorkItemType",
    "System.ChangedDate",
    "System.Tags",
    "Microsoft.VSTS.Common.Priority",
]
# Reviewer votes: 10 approved, 5 approved with suggestions, -5 waiting for author, -10 rejected.
VOTES = {10: "approved", 5: "approved", -5: "waiting for author", -10: "rejected"}

config: Dict[str, Any] = {
    "url": "https://dev.azure.com",
    "org": None,
    "project": None,
    "token_env": None,
    "token_command": None,
    "limit": 10,
    "work_items": DEFAULT_WIQL,
    "pull_requests": True,
    "reviewer_id": None,
    "creator_id": None,
}
SAMPLE = "--sample" in sys.argv[1:]
# What --sample runs with when the config leaves these out.
SAMPLE_CONFIG = {"org": "acme", "project": "Web"}

# A call to a path under the project (like "_apis/wit/wiql") with query parameters and, for a
# POST, a JSON body. Returns parsed JSON.
Call = Callable[[str, Dict[str, Any], Optional[Any]], Any]


def fetch(call: Call) -> Dict[str, Any]:
    """The replies the view needs: the WIQL result, the work items' fields, the pull requests."""
    out: Dict[str, Any] = {}
    limit = config["limit"]
    if config["work_items"]:
        found = call("_apis/wit/wiql", {"$top": limit}, {"query": config["work_items"]})
        ids = [w.get("id") for w in (found or {}).get("workItems") or [] if isinstance(w, dict)][:limit]
        ids = [i for i in ids if isinstance(i, int)]
        out["ids"] = ids
        if ids:
            params = {"ids": ",".join(str(i) for i in ids), "fields": ",".join(FIELDS)}
            out["work_items"] = call("_apis/wit/workitems", params, None)
        else:
            out["work_items"] = {"count": 0, "value": []}
    if config["pull_requests"]:
        params = {"searchCriteria.status": "active", "$top": limit}
        if config["reviewer_id"]:
            params["searchCriteria.reviewerId"] = config["reviewer_id"]
        if config["creator_id"]:
            params["searchCriteria.creatorId"] = config["creator_id"]
        out["pull_requests"] = call("_apis/git/pullrequests", params, None)
    return out


def view(replies: Dict[str, Any], base: str, limit: int, now: int) -> View:
    """The tile: work items, then pull requests, each under a heading."""
    children: List[Widget] = []
    counts = []
    if "work_items" in replies:
        value = (replies["work_items"] or {}).get("value")
        if not isinstance(value, list):
            raise upstream_error("expected a list of work items from _apis/wit/workitems")
        # The API doesn't promise the order asked for; keep the query's.
        order = {id: n for n, id in enumerate(replies.get("ids") or [])}
        value = sorted((w for w in value if isinstance(w, dict)), key=lambda w: order.get(w.get("id"), len(order)))
        items = [i for i in (work_item(w, base, now) for w in value[:limit]) if i is not None]
        counts.append(f"{len(items)} work items")
        children.append({"type": "text", "text": "**Work items**", "markdown": True})
        children.append({"type": "list", "items": items, "empty_text": "none"})
    if "pull_requests" in replies:
        value = (replies["pull_requests"] or {}).get("value")
        if not isinstance(value, list):
            raise upstream_error("expected a list of pull requests from _apis/git/pullrequests")
        items = [i for i in (pull_request(p, base, now) for p in value[:limit]) if i is not None]
        counts.append(f"{len(items)} pull requests")
        children.append({"type": "text", "text": "**Pull requests**", "markdown": True})
        children.append({"type": "list", "items": items, "empty_text": "none"})
    return {"status": "ok", "summary": " · ".join(counts), "root": {"type": "column", "children": children}}


def work_item(w: Dict[str, Any], base: str, now: int) -> Optional[ListItem]:
    """One work item: "Bug 1234 · Active · updated 2h ago", its tags, and priority as color."""
    f = w.get("fields") if isinstance(w.get("fields"), dict) else {}
    id = w.get("id")
    if not isinstance(id, int) or not isinstance(f.get("System.Title"), str):
        return None
    kind = f.get("System.WorkItemType") if isinstance(f.get("System.WorkItemType"), str) else "Item"
    sub = [f"{kind} {id}"]
    if isinstance(f.get("System.State"), str):
        sub.append(f["System.State"])
    if isinstance(f.get("System.ChangedDate"), str):
        sub.append(f"updated {ago(f['System.ChangedDate'], now)}")
    tags = [t.strip() for t in str(f.get("System.Tags") or "").split(";") if t.strip()][:3]
    item: ListItem = {
        "title": f["System.Title"],
        "subtitle": " · ".join(sub),
        "tags": tags,
        "action": {"type": "open_url", "url": f"{base}/_workitems/edit/{id}"},
    }
    priority = f.get("Microsoft.VSTS.Common.Priority")
    if priority == 1:
        item["status"] = "error"
    elif priority == 2:
        item["status"] = "warn"
    return item


def pull_request(p: Any, base: str, now: int) -> Optional[ListItem]:
    """One pull request: "web !812 · feature/x · by Dana · 3h ago", votes as tags."""
    if not isinstance(p, dict) or not isinstance(p.get("pullRequestId"), int) or not isinstance(p.get("title"), str):
        return None
    repo = (p.get("repository") or {}).get("name")
    repo = repo if isinstance(repo, str) else ""
    number = p["pullRequestId"]
    sub = [f"{repo} !{number}".strip()]
    source = branch(p.get("sourceRefName"))
    if source:
        sub.append(source)
    by = (p.get("createdBy") or {}).get("displayName")
    if isinstance(by, str):
        sub.append(f"by {by}")
    if isinstance(p.get("creationDate"), str):
        sub.append(ago(p["creationDate"], now))

    votes = [r.get("vote") for r in p.get("reviewers") or [] if isinstance(r, dict)]
    tags = []
    if p.get("isDraft") is True:
        tags.append("draft")
    conflicts = p.get("mergeStatus") == "conflicts"
    if conflicts:
        tags.append("conflicts")
    for vote in (-10, -5, 10, 5):
        if vote in votes and VOTES[vote] not in tags:
            tags.append(VOTES[vote])
    item: ListItem = {
        "title": p["title"],
        "subtitle": " · ".join(sub),
        "tags": tags,
        "action": {"type": "open_url", "url": f"{base}/_git/{urllib.parse.quote(repo)}/pullrequest/{number}"},
    }
    if conflicts or -10 in votes:
        item["status"] = "error"
    elif -5 in votes:
        item["status"] = "warn"
    elif any(v in (10, 5) for v in votes):
        item["status"] = "ok"
    return item


def branch(ref: Any) -> Optional[str]:
    """ "refs/heads/feature/x" -> "feature/x"."""
    if not isinstance(ref, str):
        return None
    return ref[len("refs/heads/") :] if ref.startswith("refs/heads/") else ref


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
    """Unix seconds from "2026-10-27T10:00:00.1234567Z" or with a +hh:mm offset (none = UTC)."""
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
    """The personal access token: from `token_command`, else the variable (default
    AZURE_DEVOPS_EXT_PAT)."""
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
    var = config["token_env"] or "AZURE_DEVOPS_EXT_PAT"
    value = os.environ.get(var, "").strip()
    if not value:
        raise upstream_error(
            f"no API token: set ${var} before starting the dashboard, or set token_env / token_command in the config"
        )
    return value


def project_url() -> str:
    """ "https://dev.azure.com/acme/Web", the base of every API path and link."""
    quote = urllib.parse.quote
    return f"{config['url'].rstrip('/')}/{quote(config['org'])}/{quote(config['project'])}"


def http_call(path: str, params: Dict[str, Any], body: Optional[Any], key: str) -> Any:
    """GET (or POST, with a body) under the project, with basic auth: no user, the token."""
    query = urllib.parse.urlencode({**params, "api-version": API_VERSION})
    auth = base64.b64encode(f":{key}".encode()).decode()
    headers = {"Authorization": f"Basic {auth}", "Accept": "application/json", "User-Agent": "dashboard-plugin azure-devops"}
    data = None
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(f"{project_url()}/{path}?{query}", data=data, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
            return json.load(resp)
    except urllib.error.HTTPError as e:
        text = e.read().decode("utf-8", "replace")[:300]
        raise upstream_error(f"Azure DevOps: HTTP {e.code}: {text}")
    except ValueError:
        # A bad or expired token gets a 203 with the sign-in page, not JSON.
        raise upstream_error("Azure DevOps sent a sign-in page instead of JSON: check the token and its scopes")
    except OSError as e:
        raise upstream_error(f"Azure DevOps {path}: {e}")


def caller(key: str) -> Call:
    """http_call with `key`, so token_command runs once per render, not once per request."""
    return lambda path, params, body: http_call(path, params, body, key)


def sample_call(path: str, _params: Dict[str, Any], _body: Optional[Any]) -> Any:
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
    if not isinstance(config["url"], str) or not re.match(r"^https?://", config["url"]):
        raise config_error("url: expected an http(s) URL, like https://dev.azure.com")
    for key in ("org", "project"):
        if not isinstance(config[key], str) or not config[key]:
            raise config_error(f"{key}: expected the {'organization' if key == 'org' else 'project'} name")
    limit = config["limit"]
    if not isinstance(limit, int) or isinstance(limit, bool) or not 1 <= limit <= 200:
        raise config_error("limit: expected a whole number from 1 to 200")
    if config["work_items"] is not None and not isinstance(config["work_items"], str):
        raise config_error("work_items: expected a WIQL query, or null to leave work items out")
    if not isinstance(config["pull_requests"], bool):
        raise config_error("pull_requests: expected true or false")
    if not config["work_items"] and not config["pull_requests"]:
        raise config_error("nothing to show: set work_items, pull_requests or both")
    for key in ("token_env", "reviewer_id", "creator_id"):
        if config[key] is not None and not isinstance(config[key], str):
            raise config_error(f"{key}: expected a string")
    argv = config["token_command"]
    if argv is not None and (not isinstance(argv, list) or not argv or not all(isinstance(a, str) for a in argv)):
        raise config_error("token_command: expected a program and its arguments, like [op, read, op://dev/azure]")


def render() -> View:
    call = sample_call if SAMPLE else caller(token())
    now = SAMPLE_NOW if SAMPLE else int(time.time())
    return view(fetch(call), project_url(), config["limit"], now)


if __name__ == "__main__":
    serve(
        manifest={
            "id": "azure-devops",
            "name": "Azure DevOps",
            "refresh": {"mode": "poll", "interval_ms": 120000, "min_interval_ms": 30000},
            "layout": {"size": "tall", "priority": 20, "min_size": "medium"},
        },
        init=init,
        render=render,
    )
