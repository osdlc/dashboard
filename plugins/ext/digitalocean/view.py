"""DigitalOcean droplets to a view. No I/O here, so the tests can feed it sample.json."""

from typing import Any, Dict, List, Optional

from dashboard_plugin import ListItem, View, upstream_error
from token_api import is_int, plural


def public_ip(d: Dict[str, Any]) -> Optional[str]:
    networks = d.get("networks") if isinstance(d.get("networks"), dict) else {}
    for net in networks.get("v4") or []:
        if isinstance(net, dict) and net.get("type") == "public" and isinstance(net.get("ip_address"), str):
            return net["ip_address"]
    return None


def price(d: Dict[str, Any]) -> float:
    size = d.get("size") if isinstance(d.get("size"), dict) else {}
    p = size.get("price_monthly")
    return float(p) if isinstance(p, (int, float)) and not isinstance(p, bool) else 0.0


def droplet(d: Dict[str, Any]) -> ListItem:
    state = d.get("status") if isinstance(d.get("status"), str) else "unknown"
    sub = [] if state == "active" else [state]
    if isinstance(d.get("size_slug"), str):
        sub.append(d["size_slug"])
    region = d.get("region") if isinstance(d.get("region"), dict) else {}
    if isinstance(region.get("slug"), str):
        sub.append(region["slug"])
    ip = public_ip(d)
    if ip:
        sub.append(ip)
    out: ListItem = {
        "title": str(d.get("name") or d.get("id")),
        "subtitle": " · ".join(sub),
        "status": "ok" if state == "active" else "unknown",
        "tags": [t for t in d.get("tags") or [] if isinstance(t, str)],
    }
    if is_int(d.get("id")):
        out["action"] = {"type": "open_url", "url": f"https://cloud.digitalocean.com/droplets/{d['id']}"}
    else:
        out["action"] = {"type": "open_url", "url": "https://cloud.digitalocean.com/droplets"}
    return out


def view(reply: Any, limit: int) -> View:
    """The tile: the droplets in the API's order (oldest first), with a count and the monthly
    cost in the summary."""
    droplets: Optional[List[Any]] = reply.get("droplets") if isinstance(reply, dict) else None
    if not isinstance(droplets, list):
        message = reply.get("message") if isinstance(reply, dict) else None
        raise upstream_error(f"DigitalOcean: {message}" if message else "unexpected DigitalOcean reply: no droplets")
    droplets = [d for d in droplets if isinstance(d, dict)]
    active = sum(1 for d in droplets if d.get("status") == "active")
    off = sum(1 for d in droplets if d.get("status") == "off")
    archived = sum(1 for d in droplets if d.get("status") == "archive")
    summary = [plural(len(droplets), "droplet")]
    if active != len(droplets):
        summary.append(f"{active} active")
    if off:
        summary.append(f"{off} off")
    if archived:
        summary.append(f"{archived} archived")
    cost = sum(price(d) for d in droplets)
    if cost:
        summary.append(f"${cost:,.0f}/mo")
    return {
        # A droplet that is off or archived is worth a look; one being created is not.
        "status": "warn" if off or archived else "ok",
        "summary": " · ".join(summary),
        "root": {"type": "list", "items": [droplet(d) for d in droplets[:limit]], "empty_text": "no droplets"},
    }
