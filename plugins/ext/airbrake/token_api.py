"""Helpers the ops token-API plugins share: JSON over HTTPS, and ages like "5m ago". The token
itself comes from apitoken.py. Standard library only, Python 3.9+.

Each package keeps its own copy (a package is a self-contained folder). The copies must stay
the same; crates/cli/tests/ext_python_plugins.rs checks that they do.
"""

import json
import re
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from typing import Any, Dict, Optional

import apitoken
from dashboard_plugin import config_error, upstream_error

# How long one request may take, in seconds.
TIMEOUT = 20


def request_json(url: str, headers: Optional[Dict[str, str]] = None, data: Optional[bytes] = None) -> Any:
    """GET (or POST, with `data`) and parse the JSON reply. HTTP errors become upstream
    errors with the start of the body."""
    req = urllib.request.Request(
        url,
        data=data,
        headers={"User-Agent": "dashboard-plugin", "Accept": "application/json", **(headers or {})},
    )
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
            return json.load(resp)
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", "replace")[:300]
        if e.code == 401:
            # The token may have changed: read it again on the next refresh.
            apitoken.forget()
        hint = " (check the token and its permissions)" if e.code in (401, 403) else ""
        raise upstream_error(f"HTTP {e.code}{hint}: {body}")
    except (OSError, ValueError) as e:
        host = urllib.parse.urlparse(url).netloc
        raise upstream_error(f"{host}: {e}")


_ISO = re.compile(r"(\d{4})-(\d\d)-(\d\d)[T ](\d\d):(\d\d):(\d\d)(?:\.\d+)?(Z|[+-]\d\d:?\d\d)?$")


def parse_time(ts: Any) -> Optional[int]:
    """Unix seconds from an RFC 3339 time ("2026-10-27T02:09:57.123Z", "+02:00", no offset =
    UTC), or None."""
    if not isinstance(ts, str):
        return None
    m = _ISO.match(ts.strip())
    if not m:
        return None
    y, mo, d, h, mi, s = (int(g) for g in m.groups()[:6])
    secs = int(datetime(y, mo, d, h, mi, s, tzinfo=timezone.utc).timestamp())
    off = m.group(7)
    if off and off != "Z":
        digits = off[1:].replace(":", "")
        delta = int(digits[:2]) * 3600 + int(digits[2:]) * 60
        secs -= delta if off[0] == "+" else -delta
    return secs


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


def plural(n: int, word: str) -> str:
    return f"1 {word}" if n == 1 else f"{n} {word}s"


def count(n: float) -> str:
    """Large counts short, like the dashboard's Count format: 950, 12.3k, 4.2M."""
    v = abs(n)
    steps = ["", "k", "M", "B"]
    i = 0
    while v >= 1000 and i < len(steps) - 1:
        v /= 1000
        i += 1
    sign = "-" if n < 0 else ""
    if i == 0 or v >= 100:
        return f"{sign}{v:.0f}{steps[i]}"
    return f"{sign}{v:.1f}{steps[i]}"


def is_int(v: Any) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


def check_limit(config: Dict[str, Any], most: int) -> None:
    limit = config.get("limit")
    if not is_int(limit) or limit < 1 or limit > most:
        raise config_error(f"limit: expected a whole number from 1 to {most}")


def read_sample(path: str) -> Any:
    """The package's sample.json: a hand-written API reply, shown instead of calling the API."""
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError) as e:
        raise upstream_error(f"reading {path}: {e}")
