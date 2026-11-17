#!/usr/bin/env python3
"""Stocks: the price and change since the previous close of a few symbols, as a table, with
a chart of each one's day below it. Clicking a row opens the symbol on Yahoo Finance. No
account or key.

It reads Yahoo Finance's unofficial chart endpoint (one request per symbol), which can
change or start refusing requests without notice; that is why this is a plugin and not a
built-in module.

    modules:
      stocks:
        package: stocks
        config: { symbols: [SPY, AAPL, MSFT], warn_drop: 2 }

`--sample` shows the saved replies in samples/ instead of calling Yahoo:

    dashboard plugin-check -- python3 plugins/ext/stocks/main.py --sample
"""

import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from typing import Any, Dict, List, Tuple, Union

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from dashboard_plugin import PluginError, View, config_error, serve, upstream_error, with_defaults  # noqa: E402
from view import SAMPLE_NOW, SYMBOL_RE, QuoteError, parse, view  # noqa: E402

API = "https://query1.finance.yahoo.com/v8/finance/chart/"
# Yahoo answers 429 to an empty or tool-like User-Agent (curl, python-urllib), and, when
# last tried, to a full desktop Chrome string too; "Mozilla/5.0 ..." in this form works.
USER_AGENT = "Mozilla/5.0 (compatible; dashboard-stocks/0.1)"
SAMPLE = "--sample" in sys.argv[1:]
MAX_SYMBOLS = 25

config: Dict[str, Any] = {
    "symbols": ["SPY", "QQQ", "AAPL", "MSFT", "NVDA"],
    "warn_drop": 2.0,
    "chart": True,
    "sample": None,
    "timeout": 10,
}


def is_number(v: Any) -> bool:
    return isinstance(v, (int, float)) and not isinstance(v, bool)


def init(c: Any) -> None:
    config.update(with_defaults(c, config))
    symbols = config["symbols"]
    if not isinstance(symbols, list) or not 1 <= len(symbols) <= MAX_SYMBOLS:
        raise config_error(f"symbols: 1 to {MAX_SYMBOLS} ticker symbols, e.g. [SPY, AAPL]")
    for s in symbols:
        if not isinstance(s, str) or not SYMBOL_RE.match(s.strip()):
            raise config_error(f"symbols: {s!r} is not a ticker symbol (like AAPL, BRK-B or ^GSPC)")
    config["symbols"] = [s.strip().upper() for s in symbols]
    if not is_number(config["warn_drop"]) or config["warn_drop"] <= 0:
        raise config_error("warn_drop: a percent above 0, e.g. 2")
    if not isinstance(config["chart"], bool):
        raise config_error("chart: true or false")
    if config["sample"] is not None and not isinstance(config["sample"], str):
        raise config_error("sample: a path to a saved reply or a folder of them")
    if not is_number(config["timeout"]) or not 1 <= config["timeout"] <= 60:
        raise config_error("timeout: seconds, 1 to 60")


def fetch(symbol: str) -> Any:
    url = API + urllib.parse.quote(symbol, safe="") + "?range=1d&interval=5m"
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, "Accept": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=config["timeout"]) as resp:
            return json.load(resp)
    except urllib.error.HTTPError as e:
        # An unknown symbol is a 404 with the reason in a JSON body.
        try:
            return json.load(e)
        except ValueError:
            pass
        if e.code == 429:
            raise upstream_error("Yahoo Finance is refusing requests (HTTP 429); try a longer refresh")
        raise upstream_error(f"Yahoo Finance: HTTP {e.code}")
    except (OSError, ValueError) as e:
        raise upstream_error(f"Yahoo Finance unreachable: {e}")


def sample_reply(path: str, symbol: str) -> Any:
    """A folder holds one saved reply per symbol (SYMBOL.json); a file holds one reply."""
    file = os.path.join(path, symbol + ".json") if os.path.isdir(path) else path
    try:
        with open(file, encoding="utf-8") as f:
            reply = json.load(f)
    except FileNotFoundError:
        return {"chart": {"result": None, "error": {"code": "Not Found", "description": "no sample"}}}
    except (OSError, ValueError) as e:
        raise upstream_error(f"sample {file}: {e}")
    results = (reply.get("chart") or {}).get("result") if isinstance(reply, dict) else None
    if results and isinstance(results, list) and (results[0].get("meta") or {}).get("symbol", symbol).upper() != symbol:
        return {"chart": {"result": None, "error": {"code": "Not Found", "description": "no sample"}}}
    return reply


def quote(symbol: str, path: Union[str, None]) -> Tuple[str, Any]:
    try:
        reply = sample_reply(path, symbol) if path else fetch(symbol)
        return symbol, parse(reply)
    except QuoteError as e:
        return symbol, e
    except PluginError as e:  # one broken symbol must not take down the others
        return symbol, QuoteError(str(e))


def render() -> View:
    path = config["sample"] or (os.path.join(HERE, "samples") if SAMPLE else None)
    symbols: List[str] = config["symbols"]
    with ThreadPoolExecutor(max_workers=min(8, len(symbols))) as pool:
        results = list(pool.map(lambda s: quote(s, path), symbols))
    if all(isinstance(q, QuoteError) for _, q in results) and not path:
        # Every request failing together is the network or Yahoo, not the symbols.
        first = next(q for _, q in results)
        if "unreachable" in str(first) or "HTTP" in str(first):
            raise upstream_error(str(first))
    now = SAMPLE_NOW if SAMPLE and not config["sample"] else int(time.time())
    return view(results, float(config["warn_drop"]), config["chart"], now)


serve(
    manifest={
        "id": "stocks",
        "name": "Stocks",
        "refresh": {"mode": "poll", "interval_ms": 60000, "min_interval_ms": 15000},
        "layout": {"size": "medium", "priority": 30, "min_size": "small"},
    },
    init=init,
    render=render,
)
