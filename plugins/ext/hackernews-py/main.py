#!/usr/bin/env python3
"""Hacker News: the top stories (or new, best, Ask HN, Show HN, jobs) from the public HN
Firebase API, as a list. Clicking a story opens its link, or its HN page when it has none.

The Python version of a native-vs-plugin benchmark widget. It follows the same rules as the
native module (crates/modules-fun/src/hackernews.rs) and the TypeScript plugin
(plugins/ext/hackernews-ts), and must show the same view for the same input.

    dashboard plugin-check -- python3 plugins/ext/hackernews-py/main.py
"""

import json
import os
import re
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from typing import Any, List, Optional

from dashboard_plugin import ListItem, View, config_error, serve, upstream_error, with_defaults

API = "https://hacker-news.firebaseio.com/v0"
# Most stories shown. Each one is a request per refresh, so keep it small.
MAX_LIMIT = 30

# Per feed: the API's list of story ids, and the label in the summary ("10 top stories").
FEEDS = {
    "top": ("topstories", "top stories"),
    "new": ("newstories", "new stories"),
    "best": ("beststories", "best stories"),
    "ask": ("askstories", "Ask HN posts"),
    "show": ("showstories", "Show HN posts"),
    "job": ("jobstories", "jobs"),
}

config = {"feed": "top", "limit": 10, "sample": None}


def read_sample(path: str) -> List[Any]:
    """A saved sample: a JSON array of HN items, in feed order."""
    try:
        with open(path, encoding="utf-8") as f:
            text = f.read()
    except OSError as e:
        raise upstream_error(f"reading {path}: {e}")
    try:
        items = json.loads(text)
    except ValueError:
        items = None
    if not isinstance(items, list):
        raise upstream_error(f"{path}: expected a JSON array of items")
    return items


def get_json(url: str) -> Any:
    req = urllib.request.Request(url, headers={"User-Agent": "dashboard-plugin hackernews-py"})
    try:
        with urllib.request.urlopen(req, timeout=20) as resp:
            return json.load(resp)
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", "replace")[:300]
        raise upstream_error(f"HTTP {e.code}: {body}")
    except (OSError, ValueError) as e:
        raise upstream_error(f"{url}: {e}")


def fetch_items(feed: str, limit: int) -> List[Any]:
    """The feed's first `limit` items, fetched at the same time, in feed order."""
    ids = get_json(f"{API}/{FEEDS[feed][0]}.json")
    if not isinstance(ids, list):
        raise upstream_error("the feed is not a list of ids")
    ids = ids[:limit]
    if not ids:
        return []
    with ThreadPoolExecutor(max_workers=min(len(ids), 16)) as pool:
        return list(pool.map(lambda id: get_json(f"{API}/item/{id}.json"), ids))


def is_int(v: Any) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


def story(item: Any, now: int) -> Optional[ListItem]:
    """One story, from an HN item. Deleted and dead items, and items without a title, give None."""
    if not isinstance(item, dict):
        return None

    def text(key: str) -> Optional[str]:
        v = item.get(key)
        return v if isinstance(v, str) else None

    def num(key: str) -> Optional[int]:
        v = item.get(key)
        return v if is_int(v) else None

    if item.get("deleted") is True or item.get("dead") is True:
        return None
    title = text("title")
    if title is None:
        return None
    url = text("url") or None

    sub = []
    score = num("score")
    if score is not None:
        sub.append(plural(score, "point"))
    comments = num("descendants")
    if comments is not None:
        sub.append(plural(comments, "comment"))
    by = text("by")
    if by is not None:
        sub.append(f"by {by}")
    t = num("time")
    if t is not None:
        sub.append(ago(now - t))
    id = num("id")
    if url is not None:
        link = url
    elif id is not None:
        link = f"https://news.ycombinator.com/item?id={id}"
    else:
        link = "https://news.ycombinator.com/"
    host = None if url is None else domain(url)
    out: ListItem = {"title": title, "tags": [] if host is None else [host], "action": {"type": "open_url", "url": link}}
    if sub:
        out["subtitle"] = " · ".join(sub)
    return out


def view(items: List[Any], feed: str, limit: int, now: int) -> View:
    """The tile, built exactly like the native module's. `now` is in unix seconds."""
    stories = [s for s in (story(item, now) for item in items[:limit]) if s is not None]
    return {
        "status": "ok",
        "summary": f"{len(stories)} {FEEDS[feed][1]}",
        "root": {"type": "list", "items": stories, "empty_text": "no stories"},
    }


def plural(n: int, word: str) -> str:
    return f"1 {word}" if n == 1 else f"{n} {word}s"


def ago(secs: int) -> str:
    """ "5m ago" style age from seconds."""
    s = max(0, secs)
    if s < 60:
        return "just now"
    if s < 3600:
        return f"{s // 60}m ago"
    if s < 86400:
        return f"{s // 3600}h ago"
    return f"{s // 86400}d ago"


def domain(url: str) -> Optional[str]:
    """ "https://www.Example.com:8080/a?b" -> "example.com". The same steps as the native module."""
    at = url.find("://")
    rest = url[at + 3 :] if at >= 0 else url
    host = re.split(r"[/?#]", rest, maxsplit=1)[0]
    host = host[host.rfind("@") + 1 :]
    host = host.split(":")[0].lower()
    if host.startswith("www."):
        host = host[4:]
    return host or None


def init(c):
    config.update(with_defaults(c, config))
    if not isinstance(config["feed"], str) or config["feed"] not in FEEDS:
        raise config_error(f"feed: expected one of {', '.join(FEEDS)}")
    if not is_int(config["limit"]) or config["limit"] < 0:
        raise config_error("limit: expected a whole number, 0 or more")
    if config["limit"] > MAX_LIMIT:
        raise config_error(f"limit: at most {MAX_LIMIT}")
    if config["sample"] is not None and not isinstance(config["sample"], str):
        raise config_error("sample: expected a file path")
    if isinstance(config["sample"], str) and config["sample"].startswith("~/"):
        config["sample"] = os.path.join(os.path.expanduser("~"), config["sample"][2:])


def render() -> View:
    if config["sample"] is not None:
        items = read_sample(config["sample"])
    else:
        items = fetch_items(config["feed"], config["limit"])
    return view(items, config["feed"], config["limit"], int(time.time()))


serve(
    manifest={
        "id": "hackernews-py",
        "name": "Hacker News (Python)",
        "refresh": {"mode": "poll", "interval_ms": 300000, "min_interval_ms": 60000},
        "layout": {"size": "medium", "priority": 30, "min_size": "small"},
    },
    init=init,
    render=render,
)
