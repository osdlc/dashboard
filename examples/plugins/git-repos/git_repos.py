#!/usr/bin/env python3
"""Local git repositories at a glance: branch, uncommitted changes, commits not
pushed or not pulled, and the age of the last commit. Clicking a row opens the
repository on its forge (GitHub, Codeberg, GitLab...).

Config (all optional):

    git-repos:
      plugin: [python3, examples/plugins/git-repos/git_repos.py]
      config:
        roots: [~/dev]          # folders searched for repositories
        depth: 3                # how deep to look under each root
        repos: [~/notes]        # extra repositories, by path
        limit: 15               # rows shown, most recently committed first
        fetch: false            # `git fetch` first, so "behind" is current (slower)

Shows: a row of stats, then a table whose cells are text, a gauge-free status
column and an open_url action per row. Uses only the standard library and the
`git` command.

Check it with:

    dashboard plugin-check --plugin-config '{"roots": ["."], "depth": 1}' -- python3 examples/plugins/git-repos/git_repos.py
"""

import json
import os
import re
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor

UPSTREAM_UNAVAILABLE = -32001
CONFIG_INVALID = -32002

config = {"roots": ["~"], "depth": 3, "repos": [], "limit": 15, "fetch": False}


class RpcError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def log(level, message):
    """A `log` notification: shows up in the dashboard's log, not the tile."""
    send({"jsonrpc": "2.0", "method": "log", "params": {"level": level, "message": message}})


def send(msg):
    print(json.dumps(msg), flush=True)


def git(path, *args):
    out = subprocess.run(["git", "-C", path, *args], capture_output=True, text=True, timeout=20)
    return out.stdout.strip() if out.returncode == 0 else None


def find_repos():
    found = {os.path.realpath(os.path.expanduser(p)) for p in config["repos"]}
    for root in config["roots"]:
        root = os.path.expanduser(root)
        base = root.rstrip(os.sep).count(os.sep)
        for dirpath, dirnames, _ in os.walk(root):
            if ".git" in dirnames or os.path.isfile(os.path.join(dirpath, ".git")):
                found.add(os.path.realpath(dirpath))
                dirnames[:] = []  # don't look inside a repository
                continue
            if dirpath.count(os.sep) - base >= config["depth"]:
                dirnames[:] = []
            # Skip hidden folders and the usual heavy ones.
            dirnames[:] = [d for d in dirnames if not d.startswith(".") and d not in ("node_modules", "target")]
    return sorted(found)


def web_url(remote):
    """git@github.com:o/r.git or https://github.com/o/r.git -> https://github.com/o/r"""
    if not remote:
        return None
    m = re.match(r"^(?:git@|ssh://git@)([^:/]+)[:/](.+?)(?:\.git)?/?$", remote)
    if m:
        return f"https://{m.group(1)}/{m.group(2)}"
    m = re.match(r"^https?://(?:[^@/]+@)?(.+?)(?:\.git)?/?$", remote)
    return f"https://{m.group(1)}" if m else None


def inspect(path):
    if config["fetch"]:
        git(path, "fetch", "--quiet")
    branch = git(path, "rev-parse", "--abbrev-ref", "HEAD") or "?"
    changes = git(path, "status", "--porcelain")
    changed = len(changes.splitlines()) if changes else 0
    ahead = behind = None
    counts = git(path, "rev-list", "--left-right", "--count", "@{upstream}...HEAD")
    if counts:
        behind, ahead = (int(n) for n in counts.split())
    last = git(path, "log", "-1", "--format=%ct")
    return {
        "path": path,
        "name": os.path.basename(path),
        "branch": "detached" if branch == "HEAD" else branch,
        "changed": changed,
        "ahead": ahead,
        "behind": behind,
        "last": int(last) if last else 0,
        "url": web_url(git(path, "remote", "get-url", "origin")),
    }


def ago(ts):
    if not ts:
        return "no commits"
    s = max(0, int(time.time()) - ts)
    for unit, n in (("d", 86400), ("h", 3600), ("m", 60)):
        if s >= n:
            return f"{s // n}{unit} ago"
    return "just now"


def row(r):
    sync = "no upstream" if r["ahead"] is None else (
        "in sync" if r["ahead"] == 0 and r["behind"] == 0
        else " ".join(p for p in (f"↑{r['ahead']}" if r["ahead"] else "", f"↓{r['behind']}" if r["behind"] else "") if p)
    )
    if r["behind"]:
        status = "warn"
    elif r["changed"] or r["ahead"]:
        status = "warn" if r["changed"] > 20 else "ok"
    else:
        status = "ok"
    out = {
        "cells": {
            "repo": r["name"],
            "branch": r["branch"],
            "changes": f"{r['changed']} changed" if r["changed"] else "clean",
            "sync": sync,
            "last": ago(r["last"]),
        },
        "status": status,
    }
    if r["url"]:
        out["action"] = {"type": "open_url", "url": r["url"]}
    return out


def initialize(params):
    user = params.get("config") or {}
    unknown = set(user) - set(config)
    if unknown:
        raise RpcError(CONFIG_INVALID, f"unknown settings: {', '.join(sorted(unknown))}")
    config.update(user)
    if subprocess.run(["git", "--version"], capture_output=True).returncode != 0:
        raise RpcError(UPSTREAM_UNAVAILABLE, "git is not installed")
    return {
        "manifest": {
            "id": "git-repos",
            "name": "Git repositories",
            "contract_version": "0.1",
            "refresh": {"mode": "poll", "interval_ms": 30000, "min_interval_ms": 5000},
            "layout": {"size": "wide", "priority": 20},
            "commands": [],
            # JSON Schema for this plugin's config block (editor help; optional).
            "config_schema": {
                "type": "object",
                "additionalProperties": False,
                "properties": {
                    "roots": {"type": "array", "items": {"type": "string"}},
                    "depth": {"type": "integer", "minimum": 0},
                    "repos": {"type": "array", "items": {"type": "string"}},
                    "limit": {"type": "integer", "minimum": 1},
                    "fetch": {"type": "boolean"},
                },
            },
        }
    }


def render(_params):
    paths = find_repos()
    if not paths:
        raise RpcError(UPSTREAM_UNAVAILABLE, f"no git repositories under {', '.join(config['roots'])}")
    with ThreadPoolExecutor(max_workers=8) as pool:
        repos = [r for r in pool.map(inspect, paths) if r]
    repos.sort(key=lambda r: r["last"], reverse=True)
    shown = repos[: config["limit"]]
    if len(repos) > len(shown):
        log("debug", f"showing {len(shown)} of {len(repos)} repositories")

    dirty = sum(1 for r in repos if r["changed"])
    unpushed = sum(1 for r in repos if r["ahead"])
    behind = sum(1 for r in repos if r["behind"])
    columns = [("repo", "Repository"), ("branch", "Branch"), ("changes", "Changes"), ("sync", "Upstream"), ("last", "Last commit")]
    return {
        "status": "warn" if behind else "ok",
        "summary": f"{len(repos)} repos · {dirty} with changes · {unpushed} unpushed",
        "root": {
            "type": "column",
            "children": [
                {
                    "type": "row",
                    "children": [
                        {"type": "stat", "label": "Repositories", "value": str(len(repos))},
                        {"type": "stat", "label": "With changes", "value": str(dirty), "status": "warn" if dirty else "ok"},
                        {"type": "stat", "label": "Unpushed", "value": str(unpushed), "status": "warn" if unpushed else "ok"},
                        {"type": "stat", "label": "Behind", "value": str(behind), "status": "warn" if behind else "ok"},
                    ],
                },
                {
                    "type": "table",
                    "columns": [{"key": k, "label": l} for k, l in columns],
                    "rows": [row(r) for r in shown],
                },
            ],
        },
    }


HANDLERS = {"initialize": initialize, "view/render": render, "shutdown": lambda _: None}

for line in sys.stdin:
    msg = json.loads(line)
    if msg.get("method") == "exit":
        break
    if "id" not in msg:
        continue
    handler = HANDLERS.get(msg["method"])
    try:
        if not handler:
            raise RpcError(-32601, f"unknown method {msg['method']}")
        send({"jsonrpc": "2.0", "id": msg["id"], "result": handler(msg.get("params") or {})})
    except RpcError as e:
        send({"jsonrpc": "2.0", "id": msg["id"], "error": {"code": e.code, "message": str(e)}})
    except Exception as e:  # report the bug in the tile instead of crashing
        send({"jsonrpc": "2.0", "id": msg["id"], "error": {"code": -32603, "message": f"{type(e).__name__}: {e}"}})
