"""Turns the output of the container CLIs into the tile. No I/O: main.py runs the
commands and passes their text in, so this is what the unit tests cover."""

import json
from typing import Any, Dict, List, Optional, Tuple

View = Dict[str, Any]
Row = Dict[str, Any]
Engine = Tuple[Row, List[Row]]


def json_lines(text: str) -> List[Dict[str, Any]]:
    """Parse output that is either one JSON array or one JSON object per line."""
    text = text.strip()
    if not text:
        return []
    if text.startswith("["):
        return json.loads(text)
    return [json.loads(line) for line in text.splitlines() if line.strip()]


def size(n: Any) -> str:
    if isinstance(n, bool) or not isinstance(n, (int, float)) or n <= 0:
        return "?"
    gib = n / 1024**3
    return f"{gib:.0f} GiB" if gib >= 1 else f"{n / 1024**2:.0f} MiB"


def vm_status(s: Any) -> str:
    s = (s or "").lower()
    if s.startswith("running"):
        return "ok"
    if s in ("broken", "error", "degraded"):
        return "error"
    return "unknown"


def vm_error(kind: str, err: str) -> Row:
    return {"kind": kind, "name": kind, "status": err, "res": "", "extra": "", "tone": "error"}


def _res(cpus: Any, memory: str, disk: str) -> str:
    return f"{cpus} · {memory} · {disk}"


def lima_rows(text: str) -> List[Row]:
    return [{
        "kind": "lima",
        "name": vm.get("name", "?"),
        "status": vm.get("status", "?"),
        "res": _res(vm.get("cpus", "?"), size(vm.get("memory")), size(vm.get("disk"))),
        "extra": vm.get("vmType", ""),
    } for vm in json_lines(text)]


def colima_rows(text: str) -> List[Row]:
    return [{
        "kind": "colima",
        "name": vm.get("name", "?"),
        "status": vm.get("status", "?"),
        "res": _res(vm.get("cpus", "?"), size(vm.get("memory")), size(vm.get("disk"))),
        "extra": vm.get("runtime", ""),
    } for vm in json_lines(text)]


def podman_rows(text: str) -> List[Row]:
    rows = []
    for vm in json_lines(text):
        status = "Running" if vm.get("Running") else ("Starting" if vm.get("Starting") else "Stopped")
        rows.append({
            "kind": "podman",
            "name": vm.get("Name", "?"),
            "status": status,
            "res": _res(vm.get("CPUs", "?"), vm.get("Memory", "?"), vm.get("DiskSize", "?")),
            "extra": vm.get("VMType", ""),
        })
    return rows


def _engine_base(ctx: Dict[str, Any]) -> Row:
    endpoint = ctx.get("DockerEndpoint", "") or ""
    return {
        "engine": ctx.get("Name", "?"),
        "endpoint": endpoint.replace("unix://", ""),
        "current": "●" if ctx.get("Current") else "",
        "count": "",
    }


def socket_path(ctx: Dict[str, Any]) -> Optional[str]:
    """The socket file of a unix-socket context, or None for any other endpoint."""
    endpoint = ctx.get("DockerEndpoint", "") or ""
    return endpoint[len("unix://"):] if endpoint.startswith("unix://") else None


def engine_down(ctx: Dict[str, Any]) -> Engine:
    return {**_engine_base(ctx), "state": "not running", "status": "unknown"}, []


def engine_error(ctx: Dict[str, Any], err: str) -> Engine:
    return {**_engine_base(ctx), "state": err, "status": "error"}, []


def engine_up(ctx: Dict[str, Any], ps_text: str) -> Engine:
    """The engine row and container rows from `docker ps --format json` output."""
    name = ctx.get("Name", "?")
    rows = [{
        "engine": name,
        "name": c.get("Names", "?"),
        "image": c.get("Image", ""),
        "status": c.get("Status", ""),
        "ports": c.get("Ports", ""),
        "running": c.get("State") == "running",
    } for c in json_lines(ps_text)]
    running = sum(1 for r in rows if r["running"])
    return {**_engine_base(ctx), "state": "up", "count": f"{running} running", "status": "ok"}, rows


def build(vms: List[Row], engines: List[Engine], show_exited: bool,
          show_stopped_vms: bool, limit: int) -> View:
    vms_up = sum(1 for v in vms if vm_status(v["status"]) == "ok")
    shown_vms = vms if show_stopped_vms else [
        v for v in vms if v.get("tone") == "error" or vm_status(v["status"]) == "ok"
    ]
    shown_vms = sorted(shown_vms, key=lambda v: (vm_status(v["status"]) != "ok", v["kind"], v["name"]))

    engine_rows = [e for e, _ in engines]
    engines_up = sum(1 for e in engine_rows if e["status"] == "ok")
    containers = [c for _, cs in engines for c in cs]
    containers.sort(key=lambda c: (not c["running"], c["engine"], c["name"]))
    n_running = sum(1 for c in containers if c["running"])

    errors = any(v.get("tone") == "error" for v in vms) or any(e["status"] == "error" for e in engine_rows)
    summary = f"{vms_up}/{len(vms)} VMs up · {engines_up}/{len(engine_rows)} engines · {n_running} containers"

    children: List[Dict[str, Any]] = [{
        "type": "row",
        "children": [
            {"type": "stat", "label": "VMs running", "value": f"{vms_up}/{len(vms)}"},
            {"type": "stat", "label": "Engines up", "value": f"{engines_up}/{len(engine_rows)}"},
            {"type": "stat", "label": "Containers", "value": str(n_running), "unit": "running"},
        ],
    }]
    if not vms and not engine_rows:
        children.append({"type": "text", "text": "No colima, lima, podman or docker found on PATH"})
    if shown_vms:
        children.append({
            "type": "table",
            "columns": [
                {"key": "kind", "label": "Kind", "priority": 2},
                {"key": "name", "label": "VM"},
                {"key": "status", "label": "Status", "priority": 3},
                {"key": "res", "label": "CPU · mem · disk", "priority": 1},
                {"key": "extra", "label": "Type", "priority": -1},
            ],
            "rows": [{
                "cells": {k: v.get(k, "") for k in ("kind", "name", "status", "res", "extra")},
                "status": "error" if v.get("tone") == "error" else vm_status(v["status"]),
            } for v in shown_vms],
        })
    if engine_rows:
        children.append({
            "type": "table",
            "columns": [
                {"key": "engine", "label": "Docker context"},
                {"key": "current", "label": "Current", "priority": 1},
                {"key": "state", "label": "Engine", "priority": 3},
                {"key": "count", "label": "Containers", "priority": 2},
                {"key": "endpoint", "label": "Endpoint", "priority": -1},
            ],
            "rows": [{
                "cells": {k: e[k] for k in ("engine", "current", "state", "count", "endpoint")},
                "status": e["status"],
            } for e in engine_rows],
        })
        children.append({
            "type": "table",
            "empty_text": "No containers" if show_exited else "No running containers",
            "columns": [
                {"key": "name", "label": "Container"},
                {"key": "engine", "label": "Engine", "priority": 2},
                {"key": "image", "label": "Image", "priority": 1},
                {"key": "status", "label": "Status", "priority": 3},
                {"key": "ports", "label": "Ports", "priority": -1},
            ],
            "rows": [{
                "cells": {k: c[k] for k in ("name", "engine", "image", "status", "ports")},
                "status": "ok" if c["running"] else "unknown",
            } for c in containers[:limit]],
        })

    return {
        "status": "warn" if errors else "ok",
        "summary": summary,
        "root": {"type": "column", "children": children},
    }
