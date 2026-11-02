#!/usr/bin/env python3
"""Load average as a dashboard tile. JSON-RPC 2.0, one message per line."""
import json, os, sys, time

MANIFEST = {
    "id": "load",
    "name": "Load average",
    "contract_version": "0.1",
    "refresh": {"mode": "poll", "interval_ms": 2000},
    "layout": {"size": "medium"},
}

def render():
    one, five, fifteen = os.getloadavg()
    busy = min(100.0, one / (os.cpu_count() or 1) * 100)
    now = int(time.time() * 1000)
    return {
        "status": "warn" if busy > 80 else "ok",
        "summary": f"load {one:.2f} {five:.2f} {fifteen:.2f}",
        "root": {"type": "column", "children": [
            {"type": "gauge", "label": "1 min, per core", "percent": busy,
             "thresholds": {"warn": 80, "error": 95}},
            # With an id, send only the new point; the host keeps the history.
            {"type": "series", "label": "Load", "id": "load", "style": "area",
             "series": [{"name": "1 min", "points": [[now, one]]}]},
        ]},
    }

for line in sys.stdin:
    msg = json.loads(line)
    if "id" not in msg:              # a notification: only `exit` matters
        if msg.get("method") == "exit":
            break
        continue
    method = msg["method"]
    if method == "initialize":
        reply = {"result": {"manifest": MANIFEST}}
    elif method == "view/render":
        reply = {"result": render()}
    elif method == "shutdown":
        reply = {"result": None}
    else:
        reply = {"error": {"code": -32601, "message": f"unknown method {method}"}}
    print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], **reply}), flush=True)
