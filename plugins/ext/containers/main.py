#!/usr/bin/env python3
"""Container runtimes on this machine: VMs (colima, lima, podman), Docker engines
(one per `docker context`) and the containers on each engine, in one tile.

Every source is optional: a CLI that is not on PATH is skipped, and one that fails
or times out shows as an error row (and a warning tile) instead of failing the tile.
Set a program to "" or false to turn its source off. Standard library only.

    modules:
      containers:
        package: containers
        config: { show_exited: true, podman: false }
"""

import os
import shutil
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from typing import Any, Dict, List, Optional, Tuple

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from dashboard_plugin import View, config_error, serve, with_defaults  # noqa: E402
import view  # noqa: E402

PROGRAMS = ("docker", "limactl", "colima", "podman")
settings: Dict[str, Any] = {
    "show_exited": False,
    "show_stopped_vms": True,
    "limit": 30,
    "timeout": 4,
    "docker": "docker",
    "limactl": "limactl",
    "colima": "colima",
    "podman": "podman",
}


def init(config: Any) -> None:
    settings.update(with_defaults(config, settings))
    for key in ("show_exited", "show_stopped_vms"):
        if not isinstance(settings[key], bool):
            raise config_error(f"{key} must be true or false")
    limit = settings["limit"]
    if isinstance(limit, bool) or not isinstance(limit, int) or limit < 0:
        raise config_error("limit must be a whole number")
    timeout = settings["timeout"]
    if isinstance(timeout, bool) or not isinstance(timeout, (int, float)) or timeout <= 0:
        raise config_error("timeout must be a positive number of seconds")
    for key in PROGRAMS:
        if settings[key] is not False and not isinstance(settings[key], str):
            raise config_error(f'{key} must be a program name or path, or "" / false to turn it off')


def program(key: str) -> Optional[str]:
    """The program to run for a setting, or None when it is off or not installed."""
    value = settings[key]
    return shutil.which(value) if value else None


def run(*argv: str) -> Tuple[str, Optional[str]]:
    """Run a command; return (stdout, error text or None)."""
    try:
        p = subprocess.run(argv, capture_output=True, text=True, timeout=settings["timeout"])
    except subprocess.TimeoutExpired:
        return "", "timed out"
    except OSError as e:
        return "", str(e)
    if p.returncode != 0:
        return p.stdout, (p.stderr.strip().splitlines() or [f"exit {p.returncode}"])[-1]
    return p.stdout, None


def vm_source(kind: str, key: str, args: List[str], parse: Any) -> List[Dict[str, Any]]:
    exe = program(key)
    if not exe:
        return []
    out, err = run(exe, *args)
    if err:
        return [view.vm_error(kind, err)]
    try:
        return parse(out)
    except ValueError:
        return [view.vm_error(kind, "unreadable output")]


def docker_contexts() -> List[Dict[str, Any]]:
    exe = program("docker")
    if not exe:
        return []
    out, err = run(exe, "context", "ls", "--format", "json")
    if err:
        return [{"Name": "docker", "Error": err}]
    try:
        return view.json_lines(out)
    except ValueError:
        return [{"Name": "docker", "Error": "unreadable output"}]


def engine(ctx: Dict[str, Any]) -> view.Engine:
    """One docker context: its engine row and its containers."""
    if ctx.get("Error"):
        return view.engine_error(ctx, ctx["Error"])
    sock = view.socket_path(ctx)
    if sock is not None and not os.path.exists(sock):
        return view.engine_down(ctx)
    argv = [program("docker") or "docker", "--context", str(ctx.get("Name")), "ps", "--format", "json"]
    if settings["show_exited"]:
        argv.insert(4, "-a")
    out, err = run(*argv)
    if err:
        return view.engine_error(ctx, err)
    try:
        return view.engine_up(ctx, out)
    except ValueError:
        return view.engine_error(ctx, "unreadable output")


def render() -> View:
    with ThreadPoolExecutor(max_workers=8) as pool:
        vm_jobs = [
            pool.submit(vm_source, "colima", "colima", ["list", "--json"], view.colima_rows),
            pool.submit(vm_source, "lima", "limactl", ["list", "--json"], view.lima_rows),
            pool.submit(vm_source, "podman", "podman", ["machine", "list", "--format", "json"], view.podman_rows),
        ]
        engine_jobs = [pool.submit(engine, c) for c in docker_contexts()]
        vms = [r for j in vm_jobs for r in j.result()]
        engines = [j.result() for j in engine_jobs]
    return view.build(vms, engines, settings["show_exited"], settings["show_stopped_vms"], settings["limit"])


serve(
    manifest=lambda _config: {
        "id": "containers",
        "name": "Containers and VMs",
        "refresh": {"mode": "poll", "interval_ms": 10000, "min_interval_ms": 3000},
        "layout": {"size": "wide"},
    },
    init=init,
    render=render,
)
