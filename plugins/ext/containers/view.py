"""Turns the output of the container CLIs into the tile. No I/O: main.py runs the
commands and passes their text in, so this is what the unit tests cover."""

import re
import json
from typing import Any, Dict, List, Optional, Tuple

View = Dict[str, Any]
Row = Dict[str, Any]
Engine = Tuple[Row, List[Row]]


def json_records(text: str) -> List[Dict[str, Any]]:
    """Parse one JSON array, or one JSON object per line. Items that are not
    objects are dropped, so an odd reply gives fewer rows, not a failed tile."""
    text = text.strip()
    if not text:
        return []
    if text.startswith("["):
        items = json.loads(text)
        if not isinstance(items, list):
            raise ValueError("not a JSON array")
    else:
        items = [json.loads(line) for line in text.splitlines() if line.strip()]
    return [x for x in items if isinstance(x, dict)]


def _str(v: Any, default: str = "?") -> str:
    """A field as text: missing, null or empty gives `default`."""
    return default if v is None or v == "" else str(v)


def size(n: Any) -> str:
    if isinstance(n, bool) or not isinstance(n, (int, float)) or n <= 0:
        return "?"
    gib = n / 1024**3
    return f"{gib:.0f} GiB" if gib >= 1 else f"{n / 1024**2:.0f} MiB"


def vm_status(s: Any) -> str:
    s = s.lower() if isinstance(s, str) else ""
    if s.startswith("running"):
        return "ok"
    if s in ("broken", "error", "degraded"):
        return "error"
    return "unknown"


def _vm(kind: str, name: Any, status: Any, res: str, extra: Any) -> Row:
    status = _str(status)
    return {"kind": kind, "name": _str(name), "status": status, "res": res,
            "extra": _str(extra, ""), "tone": vm_status(status)}


def vm_error(kind: str, err: str) -> Row:
    """A source that failed: shown as a row, but not counted as a VM."""
    return {"kind": kind, "name": kind, "status": err, "res": "", "extra": "", "tone": "error",
            "failed": True}


def _spaced(v: Any) -> Any:
    """Podman writes sizes as "2GiB"; the other sources give "2 GiB"."""
    return re.sub(r"^(\d+(?:\.\d+)?)([KMGT]i?B)$", r"\1 \2", v) if isinstance(v, str) else v


def _res(cpus: Any, memory: Any, disk: Any) -> str:
    return f"{_str(cpus)} · {_spaced(_str(memory))} · {_spaced(_str(disk))}"


def _lima_like_rows(kind: str, extra_key: str, text: str) -> List[Row]:
    """`limactl list --json` and `colima list --json` share one shape."""
    return [_vm(kind, vm.get("name"), vm.get("status"),
                _res(vm.get("cpus"), size(vm.get("memory")), size(vm.get("disk"))),
                vm.get(extra_key))
            for vm in json_records(text)]


def lima_rows(text: str) -> List[Row]:
    return _lima_like_rows("lima", "vmType", text)


def colima_rows(text: str) -> List[Row]:
    return _lima_like_rows("colima", "runtime", text)


def podman_rows(text: str) -> List[Row]:
    rows = []
    for vm in json_records(text):
        status = "Running" if vm.get("Running") else ("Starting" if vm.get("Starting") else "Stopped")
        rows.append(_vm("podman", vm.get("Name"), status,
                        _res(vm.get("CPUs"), vm.get("Memory"), vm.get("DiskSize")), vm.get("VMType")))
    return rows


def socket_path(ctx: Dict[str, Any]) -> Optional[str]:
    """The socket file of a unix-socket context, or None for any other endpoint."""
    endpoint = ctx.get("DockerEndpoint") or ""
    return endpoint[len("unix://"):] if isinstance(endpoint, str) and endpoint.startswith("unix://") else None


def _engine_base(ctx: Dict[str, Any]) -> Row:
    return {
        "engine": _str(ctx.get("Name")),
        "endpoint": socket_path(ctx) or _str(ctx.get("DockerEndpoint"), ""),
        "current": "●" if ctx.get("Current") else "",
        "count": "",
    }


def engine_down(ctx: Dict[str, Any]) -> Engine:
    return {**_engine_base(ctx), "state": "not running", "status": "unknown"}, []


def engine_error(ctx: Dict[str, Any], err: str) -> Engine:
    """A context whose engine did not answer. With `ctx["Listing"]` set, the
    context list itself failed: shown as a row, but not counted as an engine."""
    return {**_engine_base(ctx), "state": err, "status": "error", "failed": bool(ctx.get("Listing"))}, []


# `docker ps` states: running counts; paused and restarting are live but need a look.
_CONTAINER_TONE = {"running": "ok", "paused": "warn", "restarting": "warn"}


def engine_up(ctx: Dict[str, Any], ps_text: str) -> Engine:
    """The engine row and container rows from `docker ps --format json` output."""
    name = _str(ctx.get("Name"))
    rows = [{
        "engine": name,
        "name": _str(c.get("Names")),
        "image": _str(c.get("Image"), ""),
        "status": _str(c.get("Status"), ""),
        "ports": _str(c.get("Ports"), ""),
        "tone": _CONTAINER_TONE.get(c.get("State"), "unknown"),
    } for c in json_records(ps_text)]
    running = sum(1 for r in rows if r["tone"] == "ok")
    return {**_engine_base(ctx), "state": "up", "count": f"{running} running", "status": "ok"}, rows


def build(vms: List[Row], engines: List[Engine], show_exited: bool,
          show_stopped_vms: bool, limit: int) -> View:
    """The tile. VMs sort running first, then by kind and name; containers sort
    running first, then by engine and name. Error rows show even when
    show_stopped_vms is false. A source that failed to list (vm_error, or a
    failed `docker context ls`) is shown but not counted. limit caps the
    container rows only, and says how many it left out."""
    real_vms = [v for v in vms if not v.get("failed")]
    vms_up = sum(1 for v in real_vms if v["tone"] == "ok")
    shown_vms = vms if show_stopped_vms else [v for v in vms if v["tone"] in ("ok", "error")]
    shown_vms = sorted(shown_vms, key=lambda v: (v["tone"] != "ok", v["kind"], v["name"]))

    engine_rows = [e for e, _ in engines]
    real_engines = [e for e in engine_rows if not e.get("failed")]
    engines_up = sum(1 for e in real_engines if e["status"] == "ok")
    containers = [c for _, cs in engines for c in cs]
    containers.sort(key=lambda c: (c["tone"] != "ok", c["engine"], c["name"]))
    n_running = sum(1 for c in containers if c["tone"] == "ok")

    errors = any(v["tone"] == "error" for v in vms) or any(e["status"] == "error" for e in engine_rows)
    summary = f"{vms_up}/{len(real_vms)} VMs up · {engines_up}/{len(real_engines)} engines · {n_running} containers"

    children: List[Dict[str, Any]] = [{
        "type": "row",
        "children": [
            {"type": "stat", "label": "VMs up", "value": f"{vms_up}/{len(real_vms)}"},
            {"type": "stat", "label": "Engines up", "value": f"{engines_up}/{len(real_engines)}"},
            {"type": "stat", "label": "Containers running", "value": str(n_running)},
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
                "cells": {k: v[k] for k in ("kind", "name", "status", "res", "extra")},
                "status": v["tone"],
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
    # The containers are known only when some engine answered.
    if engines_up:
        if containers:
            children.append({
                "type": "table",
                "columns": [
                    {"key": "name", "label": "Container"},
                    {"key": "engine", "label": "Engine", "priority": 2},
                    {"key": "image", "label": "Image", "priority": 1},
                    {"key": "status", "label": "Status", "priority": 3},
                    {"key": "ports", "label": "Ports", "priority": -1},
                ],
                "rows": [{
                    "cells": {k: c[k] for k in ("name", "engine", "image", "status", "ports")},
                    "status": c["tone"],
                } for c in containers[:limit]],
            })
            if len(containers) > limit:
                children.append({"type": "text", "text": f"{len(containers) - limit} more not shown (limit {limit})"})
        else:
            children.append({"type": "text", "text": "No containers" if show_exited else "No running containers"})

    return {
        "status": "warn" if errors else "ok",
        "summary": summary,
        "root": {"type": "column", "children": children},
    }
