#!/usr/bin/env python3
"""Text file: shows a text or markdown file, and shows it again when it changes.

The Python version of a native-vs-plugin benchmark widget. It follows the same rules as the
native module (crates/modules-system/src/textfile.rs) and the TypeScript plugin
(plugins/ext/textfile-ts), and must show the same view for the same file.

    dashboard plugin-check --plugin-config '{"path": "README.md"}' -- python3 plugins/ext/textfile-py/main.py
"""

import os
from typing import Optional, Tuple

from dashboard_plugin import View, config_error, serve, upstream_error, with_defaults

# Larger files are refused: this is for notes, not logs.
MAX_BYTES = 1024 * 1024

config = {"path": None, "markdown": None, "lines": 200}
state = {"path": "", "name": "", "markdown": False}
# Modified time and size the last view was built from, and that view.
seen: Optional[Tuple[int, int]] = None
last: Optional[View] = None


def view(text: str, name: str, markdown: bool, max_lines: int) -> View:
    """The tile for a file's text. All three versions of the widget follow these rules: drop a
    leading byte order mark, turn CRLF into LF, drop trailing blanks (spaces, tabs, line
    breaks), and keep the first `max_lines` lines."""
    if text.startswith("\ufeff"):
        text = text[1:]
    text = text.replace("\r\n", "\n").rstrip(" \t\r\n")
    if text == "":
        return {"status": "ok", "summary": f"{name} · empty", "root": {"type": "text", "text": "(empty)", "markdown": False}}
    # split("\n"), not splitlines(): that also splits on form feeds and other breaks.
    lines = text.split("\n")
    if len(lines) > max_lines:
        return {
            "status": "ok",
            "summary": f"{name} · first {max_lines} of {len(lines)} lines",
            "root": {"type": "text", "text": "\n".join(lines[:max_lines]) + "\n…", "markdown": markdown},
        }
    count = "1 line" if len(lines) == 1 else f"{len(lines)} lines"
    return {"status": "ok", "summary": f"{name} · {count}", "root": {"type": "text", "text": text, "markdown": markdown}}


def init(c):
    config.update(with_defaults(c, config))
    if not isinstance(config["path"], str):
        raise config_error("the textfile plugin needs `config: { path: <file> }`")
    if config["markdown"] is not None and not isinstance(config["markdown"], bool):
        raise config_error("`markdown` must be true or false")
    lines = config["lines"]
    if isinstance(lines, bool) or not isinstance(lines, int) or lines < 1:
        raise config_error("`lines` must be at least 1")
    path = config["path"]
    if path.startswith("~/"):
        path = os.path.expanduser(path)
    state["path"] = path
    state["name"] = os.path.basename(os.path.normpath(path)) or "Text file"
    md = config["markdown"]
    state["markdown"] = md if md is not None else os.path.splitext(path)[1].lower() in (".md", ".markdown")


def render() -> View:
    """Checks the file's modified time and size, and reads it again only when one of them
    changed."""
    global seen, last
    path = state["path"]
    try:
        st = os.stat(path)
    except OSError as e:
        raise upstream_error(f"reading {path}: {e.strerror}")
    key = (st.st_mtime_ns, st.st_size)
    if last is not None and seen == key:
        return last
    if st.st_size > MAX_BYTES:
        raise upstream_error(f"{path} is over 1 MB")
    try:
        with open(path, "rb") as f:
            data = f.read()
    except OSError as e:
        raise upstream_error(f"reading {path}: {e.strerror}")
    last = view(data.decode("utf-8", "replace"), state["name"], state["markdown"], config["lines"])
    seen = key
    return last


serve(
    manifest=lambda _c: {
        "id": "textfile-py",
        "name": f"{state['name']} (Python)",
        "refresh": {"mode": "poll", "interval_ms": 1000, "min_interval_ms": 250},
        "layout": {"size": "tall", "priority": 0},
    },
    init=init,
    render=render,
)
