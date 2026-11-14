"""Yahoo Finance chart replies to a view. No I/O here, so the tests can feed it samples/."""

import re
from typing import Any, Dict, List, Optional, Tuple

from dashboard_plugin import Column, Row, View

# The time the sample is measured from (the close of the sampled trading day), so `--sample`
# shows the same view whenever it runs.
SAMPLE_NOW = 1794603601

SYMBOL_RE = re.compile(r"^[A-Za-z0-9.^=\-]{1,15}$")

CURRENCIES = {"USD": "$", "EUR": "€", "GBP": "£", "JPY": "¥"}


class QuoteError(Exception):
    """One symbol that could not be shown; the message goes in its row."""


def number(v: Any) -> Optional[float]:
    if isinstance(v, (int, float)) and not isinstance(v, bool):
        return float(v)
    return None


def parse(reply: Any) -> Dict[str, Any]:
    """One `/v8/finance/chart/{symbol}` reply to a quote, or a QuoteError."""
    chart = reply.get("chart") if isinstance(reply, dict) else None
    if not isinstance(chart, dict):
        raise QuoteError("unexpected reply")
    err = chart.get("error")
    if err:
        err = err if isinstance(err, dict) else {}
        if err.get("code") == "Not Found":
            raise QuoteError("not found")
        raise QuoteError(str(err.get("description") or err.get("code") or "request failed"))
    results = chart.get("result")
    result = results[0] if isinstance(results, list) and results and isinstance(results[0], dict) else None
    meta = result.get("meta") if result else None
    if not isinstance(meta, dict):
        raise QuoteError("not found")
    price = number(meta.get("regularMarketPrice"))
    prev = number(meta.get("chartPreviousClose"))
    if prev is None:
        prev = number(meta.get("previousClose"))
    if price is None:
        raise QuoteError("no price")
    regular = ((meta.get("currentTradingPeriod") or {}).get("regular")) or {}
    return {
        "symbol": str(meta.get("symbol") or ""),
        "name": str(meta.get("shortName") or meta.get("longName") or ""),
        "currency": str(meta.get("currency") or ""),
        "price": price,
        "prev": prev,
        "open": (number(regular.get("start")), number(regular.get("end"))),
    }


def amount(value: float) -> str:
    return f"{value:,.2f}" if abs(value) >= 1 else f"{value:.4f}"


def money(value: float, currency: str) -> str:
    """"$356.27", "$1,234.50", "€0.9213", "12.30 CHF"."""
    number_ = amount(value)
    sym = CURRENCIES.get(currency)
    if sym:
        return sym + number_
    return f"{number_} {currency}".strip()


def pct(p: Optional[float], digits: int = 2) -> str:
    return "-" if p is None else f"{p:+.{digits}f}%"


def delta(q: Dict[str, Any]) -> Tuple[Optional[float], Optional[float]]:
    """(change, change in percent) from the previous close."""
    if not q["prev"]:
        return None, None
    change = q["price"] - q["prev"]
    return change, change / q["prev"] * 100


def link(symbol: str) -> Dict[str, str]:
    safe = re.sub(r"[^A-Za-z0-9.\-]", lambda m: "%{:02X}".format(ord(m.group())), symbol)
    return {"type": "open_url", "url": f"https://finance.yahoo.com/quote/{safe}"}


def closed(quotes: List[Dict[str, Any]], now: int) -> bool:
    for q in quotes:
        start, end = q["open"]
        if start is not None and end is not None:
            return not start <= now < end
    return False


def view(results: List[Tuple[str, Any]], warn_drop: float, now: int) -> View:
    """The tile. `results` is one (symbol, quote or QuoteError) per configured symbol, in order."""
    rows: List[Row] = []
    quotes: List[Dict[str, Any]] = []
    parts: List[str] = []
    warned = False
    for symbol, q in results:
        if isinstance(q, QuoteError):
            rows.append(
                {
                    "cells": {"symbol": symbol, "name": "", "price": str(q), "change": "-", "percent": "-"},
                    "status": "error",
                }
            )
            continue
        quotes.append(q)
        change, p = delta(q)
        fell = p is not None and p <= -warn_drop
        warned |= fell
        shown = money(q["price"], q["currency"])
        if len(parts) < 3:
            parts.append(f"{symbol} {amount(q['price'])} {pct(p, 1)}")
        row: Row = {
            "cells": {
                "symbol": symbol,
                "name": q["name"],
                "price": shown,
                "change": "-" if change is None else f"{change:+,.2f}" if abs(change) >= 0.01 else f"{change:+.4f}",
                "percent": pct(p),
            },
            "action": link(symbol),  # type: ignore[typeddict-item]
        }
        if fell:
            row["status"] = "warn"
        rows.append(row)
    col = lambda key, label, priority: {"key": key, "label": label, "priority": priority}  # noqa: E731
    table = {
        "type": "table",
        "columns": [
            col("symbol", "Symbol", 10),
            col("name", "Name", 1),
            col("price", "Price", 9),
            col("change", "Change", 3),
            col("percent", "%", 8),
        ],
        "rows": rows,
    }
    if not quotes:
        status, summary = "error", "no quotes"
        if rows:
            summary += f": {results[0][0]} {rows[0]['cells']['price']}"
    else:
        status = "warn" if warned else "ok"
        if len(quotes) < len(results):
            status = "warn"
        summary = " · ".join(parts)
        if closed(quotes, now):
            summary += " · closed"
    return {"status": status, "summary": summary, "root": table}  # type: ignore[typeddict-item]
