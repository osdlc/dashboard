#!/usr/bin/env python3
"""Local git repositories: branch, commits ahead of and behind the upstream, and
uncommitted changes. Runs `git status --porcelain=v2 --branch` once per repository.

The Python copy of the native `git` module (crates/modules-dev/src/git.rs), for the
native-vs-plugin benchmark. It shows the same view for the same repositories, so keep
the three (Rust, TypeScript, Python) in step. Standard library only.

    dashboard plugin-check --plugin-config '{"repos": ["."]}' -- python3 plugins/ext/git-py/git_py.py

    modules:
      git-py:
        plugin: [python3, plugins/ext/git-py/git_py.py]
        config: { repos: [~/dev/api, ~/dev/web] }
"""

import os
import subprocess
import sys
from typing import Dict, List, Union

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from dashboard_plugin import Row, View, config_error, serve, upstream_error, with_defaults  # noqa: E402

config = {"repos": []}

RANK = {"error": 3, "warn": 2, "ok": 1, "unknown": 0}


def init(c):
    config.update(with_defaults(c, config))
    if not isinstance(config["repos"], list) or not config["repos"]:
        raise config_error("the git module needs `config: { repos: [path, ...] }`")
    if not all(isinstance(r, str) for r in config["repos"]):
        raise config_error("`repos` must be a list of paths")


def expand_home(path: str) -> str:
    """`~/x` as `$HOME/x`; other paths as they are."""
    home = os.environ.get("HOME")
    return home + path[1:] if path.startswith("~/") and home else path


def repo_name(path: str) -> str:
    """The folder's name, after following links."""
    real = os.path.realpath(path) if os.path.exists(path) else path
    return os.path.basename(real.rstrip(os.sep)) or real


def parse(out: str) -> Dict:
    """Reads `git status --porcelain=v2 --branch` output."""
    repo = {"branch": "", "upstream": None, "changed": 0}
    has_upstream = False
    for line in out.split("\n"):
        if line.startswith("# branch.head "):
            head = line[len("# branch.head "):]
            repo["branch"] = "detached" if head == "(detached)" else head
        elif line.startswith("# branch.upstream "):
            has_upstream = True
        elif line.startswith("# branch.ab "):
            counts = []
            for n in line[len("# branch.ab "):].split():
                try:
                    counts.append(int(n.lstrip("+-")))
                except ValueError:
                    counts.append(0)
            counts += [0, 0]
            repo["upstream"] = (counts[0], counts[1])
        elif line and not line.startswith("#") and not line.startswith("!"):
            repo["changed"] += 1
    # An upstream with no ahead/behind line is one whose branch is gone.
    if has_upstream and repo["upstream"] is None:
        repo["upstream"] = "gone"
    return repo


def git_error(stderr: str) -> str:
    """The first line of git's complaint, without "fatal: "."""
    line = stderr.split("\n")[0].strip()
    if line.startswith("fatal: "):
        line = line[len("fatal: "):]
    return line or "git status failed"


def status(path: str) -> Union[Dict, str]:
    if not os.path.isdir(path):
        return "not a folder"
    try:
        out = subprocess.run(
            ["git", "-C", path, "status", "--porcelain=v2", "--branch"],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            # Don't take the index lock: the user's own git commands come first.
            env={**os.environ, "GIT_OPTIONAL_LOCKS": "0"},
        )
    except OSError:
        raise upstream_error("running git (is it installed?)")
    stdout = out.stdout.decode("utf-8", "replace")
    stderr = out.stderr.decode("utf-8", "replace")
    return parse(stdout) if out.returncode == 0 else git_error(stderr)


def changes(n: int) -> str:
    return "clean" if n == 0 else f"{n} changed"


def sync(upstream) -> str:
    if upstream is None:
        return "no upstream"
    if upstream == "gone":
        return "upstream gone"
    ahead, behind = upstream
    if ahead == 0 and behind == 0:
        return "in sync"
    parts = []
    if ahead > 0:
        parts.append(f"↑{ahead}")
    if behind > 0:
        parts.append(f"↓{behind}")
    return " ".join(parts)


def view(found: List[Dict]) -> View:
    """`found` holds {name, repo} or {name, error} per repository."""
    dirty = unpushed = behind = 0
    worst = "ok"
    rows: List[Row] = []
    for f in found:
        if "repo" in f:
            r = f["repo"]
            ahead, back = r["upstream"] if isinstance(r["upstream"], tuple) else (0, 0)
            dirty += r["changed"] > 0
            unpushed += ahead > 0
            behind += back > 0
            status = "warn" if r["changed"] > 0 or back > 0 else "ok"
            cells = {"repo": f["name"], "branch": r["branch"], "changes": changes(r["changed"]), "sync": sync(r["upstream"])}
        else:
            status = "error"
            cells = {"repo": f["name"], "branch": "-", "changes": f["error"], "sync": "-"}
        if RANK[status] > RANK[worst]:
            worst = status
        rows.append({"cells": cells, "status": status})
    return {
        "status": worst,
        "summary": f"{len(found)} {'repo' if len(found) == 1 else 'repos'} · {dirty} dirty · {unpushed} unpushed · {behind} behind",
        "root": {
            "type": "table",
            "columns": [
                {"key": "repo", "label": "Repository"},
                {"key": "branch", "label": "Branch"},
                {"key": "changes", "label": "Changes"},
                {"key": "sync", "label": "Upstream"},
            ],
            "rows": rows,
        },
    }


def render() -> View:
    found = []
    for p in config["repos"]:
        path = expand_home(p)
        repo = status(path)
        if isinstance(repo, str):
            found.append({"name": repo_name(path), "error": repo})
        else:
            found.append({"name": repo_name(path), "repo": repo})
    return view(found)


if __name__ == "__main__":
    serve(
        manifest={
            "id": "git-py",
            "name": "Git (Python)",
            "refresh": {"mode": "poll", "interval_ms": 30000, "min_interval_ms": 5000},
            "layout": {"size": "medium", "priority": 20, "min_size": "small"},
        },
        init=init,
        render=render,
    )
