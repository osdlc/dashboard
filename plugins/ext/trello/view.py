"""Turns a Trello board's lists with their cards (`GET /1/boards/{id}/lists?cards=open`) into
the tile's view. No I/O here, so the tests and the --sample mode run the same code as the real
tile."""

import datetime
import math
from typing import Any, Dict, List, Optional, Tuple

from dashboard_plugin import ListItem, Status, View

# The card fields we ask for. `due` is an ISO time, or null with no due date.
CARD_FIELDS = "name,due,dueComplete,shortUrl,labels"

# The fixed "now" for --sample and the tests, so due times always read the same.
SAMPLE_NOW = datetime.datetime(2026, 10, 27, 12, 0, tzinfo=datetime.timezone.utc).timestamp()

HOUR = 3600


def seconds(iso: str) -> float:
    """An ISO time from the API (`2026-10-26T16:00:00.000Z`) as unix seconds."""
    return datetime.datetime.fromisoformat(iso.replace("Z", "+00:00")).timestamp()


def due(card: Dict[str, Any], now: float) -> Tuple[str, Optional[Status]]:
    """The due date in words, and the status it gives the card."""
    if not card.get("due"):
        return "", None
    if card.get("dueComplete"):
        return "done", "ok"
    s = seconds(card["due"]) - now
    if s < 0:
        h = -s / HOUR
        return (f"overdue {math.ceil(h)}h" if h < 24 else f"overdue {int(h // 24)}d"), "error"
    if s < 24 * HOUR:
        return f"due in {math.ceil(s / HOUR)}h", "warn"
    return f"due in {int(s // (24 * HOUR))}d", None


def view(board: List[Dict[str, Any]], lists: List[str], now: float, limit: int) -> View:
    """`lists` names the lists whose cards to show (all when empty). Cards with a due date
    come first, soonest first; the bars count every list's cards."""
    wanted = {n.lower() for n in lists}
    cards = [(l["name"], c) for l in board if not wanted or l["name"].lower() in wanted for c in l["cards"]]
    cards.sort(key=lambda lc: seconds(lc[1]["due"]) if lc[1].get("due") and not lc[1].get("dueComplete") else math.inf)
    overdue = sum(1 for _, c in cards if due(c, now)[1] == "error")
    soon = sum(1 for _, c in cards if due(c, now)[1] == "warn")

    items: List[ListItem] = []
    for name, card in cards[:limit]:
        text, status = due(card, now)
        item: ListItem = {"title": card["name"], "subtitle": f"{name} · {text}" if text else name}
        if status:
            item["status"] = status
        item["tags"] = [l.get("name") or l.get("color") or "label" for l in card.get("labels") or []]
        item["action"] = {"type": "open_url", "url": card["shortUrl"]}
        items.append(item)

    summary = f"{len(cards)} card{'' if len(cards) == 1 else 's'}"
    if overdue:
        summary += f" · {overdue} overdue"
    if soon:
        summary += f" · {soon} due soon"
    tile: Status = "error" if overdue else "warn" if soon else "ok"
    return {
        "status": tile,
        "summary": summary,
        "root": {
            "type": "column",
            "children": [
                {
                    "type": "bars",
                    "label": "Cards per list",
                    "format": "count",
                    "bars": [{"label": l["name"], "value": len(l["cards"])} for l in board],
                },
                {"type": "list", "empty_text": "No open cards", "items": items},
            ],
        },
    }
