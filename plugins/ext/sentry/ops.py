"""Helpers shared by the ops plugins in plugins/ext (datadog, newrelic, opsgenie, pagerduty,
sentry): tokens, HTTP GET with a timeout, config checks and ages. Each package carries a copy
so it can be installed on its own; a test checks the copies match."""

import json
import os
import re
import subprocess
import time
import urllib.error
import urllib.request
from datetime import datetime
from typing import Any, Dict, List, Optional, Union

from dashboard_plugin import config_error, upstream_error

# How long one API call and one token command may take, in seconds. The host waits 30 s for
# a render; the slowest render (two token commands and one call, or one token command and two
# calls) must still answer inside that with its own clear error.
TIMEOUT = 10
TOKEN_TIMEOUT = 5


def read_token(env: str, command: Optional[List[str]], what: str = "API token") -> str:
    """A secret: what `command` prints when it is set, else the environment variable `env`.
    `what` names it in errors ("API key"). The value itself never goes in a message."""
    if command:
        program = command[0]
        try:
            # stdin is the host's JSON-RPC pipe; the command must not read from it.
            out = subprocess.run(
                command, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=TOKEN_TIMEOUT
            )
        except (OSError, subprocess.TimeoutExpired) as e:
            raise upstream_error(f"the command for the {what} ({program}) failed: {e}")
        if out.returncode != 0:
            why = out.stderr.strip() or f"exit code {out.returncode}"
            raise upstream_error(f"the command for the {what} ({program}) failed: {why}")
        value = out.stdout.strip()
        if not value:
            raise upstream_error(f"the command for the {what} ({program}) printed nothing")
        return value
    value = os.environ.get(env, "").strip()
    if not value:
        raise config_error(
            f"no {what}: set ${env} before starting the dashboard, or set the token command in the config"
        )
    return value


def get_json(service: str, url: str, headers: Dict[str, str]) -> Any:
    """GETs `url` and parses the JSON reply. Failures become upstream errors that name `service`."""
    req = urllib.request.Request(url, headers={"Accept": "application/json", **headers})
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
            body = resp.read()
    except urllib.error.HTTPError as e:
        if e.code in (401, 403):
            raise upstream_error(f"{service}: HTTP {e.code}: check the token and its permissions")
        if e.code == 429:
            raise upstream_error(f"{service}: rate limited (429), will retry")
        raise upstream_error(f"{service}: HTTP {e.code} {e.reason}")
    except (OSError, ValueError) as e:
        raise upstream_error(f"{service}: {getattr(e, 'reason', e)}")
    try:
        return json.loads(body)
    except ValueError:
        raise upstream_error(f"{service}: the reply is not JSON")


def strings(value: Any, name: str) -> Optional[List[str]]:
    """Checks that `value` is None or a list of strings."""
    if value is None:
        return None
    if not isinstance(value, list) or not all(isinstance(v, str) for v in value):
        raise config_error(f"{name} must be a list of strings")
    return value


def env_name(value: Any, name: str) -> str:
    """Checks that `value` is an environment variable name."""
    if not isinstance(value, str) or not re.match(r"^[A-Za-z_][A-Za-z0-9_]*$", value):
        raise config_error(f"{name} must be an environment variable name, e.g. API_TOKEN")
    return value


def count(value: Any, name: str, least: int = 1) -> int:
    """Checks that `value` is a whole number of at least `least`."""
    if not isinstance(value, int) or isinstance(value, bool) or value < least:
        raise config_error(f"{name} must be a whole number of at least {least}")
    return value


def base_url(value: Any, name: str) -> str:
    """Checks that `value` is an http(s) URL, and drops a trailing slash."""
    if not isinstance(value, str) or not re.match(r"^https?://[^/]", value):
        raise config_error(f"{name} must be an http(s) URL")
    return value.rstrip("/")


def parse_time(value: Any) -> Optional[float]:
    """Unix seconds for an ISO time ("2026-10-27T08:27:00.000Z") or unix milliseconds."""
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return value / 1000
    if not isinstance(value, str):
        return None
    # Python 3.9 reads neither "Z" nor more or fewer than 6 fraction digits.
    text = re.sub(r"Z$", "+00:00", value.strip())
    text = re.sub(r"\.(\d+)", lambda m: "." + (m.group(1) + "000000")[:6], text)
    try:
        return datetime.fromisoformat(text).timestamp()
    except ValueError:
        return None


def ago(value: Union[str, int, float, None], now: Optional[float] = None) -> str:
    """ "just now", "5m ago", "3h ago", "2d ago" for an ISO time or unix milliseconds."""
    then = parse_time(value)
    if then is None:
        return "" if value is None else str(value)
    secs = max(0, int((time.time() if now is None else now) - then))
    if secs < 60:
        return "just now"
    if secs < 3600:
        return f"{secs // 60}m ago"
    if secs < 86400:
        return f"{secs // 3600}h ago"
    return f"{secs // 86400}d ago"


def upto(n: int, page: int = 100) -> str:
    """A count read from one page of `page` results: "100+" when the page was full."""
    return f"{n}+" if n >= page else str(n)


def plural(n: int, word: str) -> str:
    """ "1 alert", "3 alerts"."""
    return f"{n} {word}" if n == 1 else f"{n} {word}s"


def join(parts: List[Any]) -> str:
    """The parts that are not empty, joined with " · "."""
    return " · ".join(str(p) for p in parts if p)
