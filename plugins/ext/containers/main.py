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
from typing import Any, Callable, Dict, List, Optional, Tuple, TypeVar

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from sitroom_plugin import View, config_error, serve, with_defaults  # noqa: E402
import view  # noqa: E402

T = TypeVar("T")
PROGRAMS = ("docker", "limactl", "colima", "podman")
# The host gives a render 30 s. One render makes two CLI calls in a row
# (`docker context ls`, then `docker ps`), so a call gets at most 10 s.
MAX_TIMEOUT = 10
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
    if isinstance(limit, bool) or not isinstance(limit, int) or limit < 1:
        raise config_error("limit must be a whole number, 1 or more")
    timeout = settings["timeout"]
    if isinstance(timeout, bool) or not isinstance(timeout, (int, float)) or not 0 < timeout <= MAX_TIMEOUT:
        raise config_error(f"timeout must be a number of seconds above 0, at most {MAX_TIMEOUT}")
    for key in PROGRAMS:
        if settings[key] is not False and not isinstance(settings[key], str):
            raise config_error(f'{key} must be a program name or path, or "" / false to turn it off')


def program(key: str) -> Tuple[Optional[str], Optional[str]]:
    """The program to run for a setting, and an error. Off, or a bare name not on
    PATH: (None, None), the source is skipped. A path that does not resolve:
    (None, error), since the user named it and likely mistyped it."""
    value = settings[key]
    if not value:
        return None, None
    exe = shutil.which(value)
    if exe is None and (os.sep in value or (os.altsep and os.altsep in value)):
        return None, f"{value}: not found"
    return exe, None


def run(*argv: str) -> Tuple[str, Optional[str]]:
    """Run a command. The error is the last stderr line, "exit N", "timed out" or
    the OSError text; stdout is returned even when the command fails. stdin is
    closed, so a CLI can't read the plugin's JSON-RPC input."""
    try:
        p = subprocess.run(argv, stdin=subprocess.DEVNULL, capture_output=True, encoding="utf-8",
                           errors="replace", timeout=settings["timeout"])
    except subprocess.TimeoutExpired:
        return "", "timed out"
    except OSError as e:
        return "", str(e)
    if p.returncode != 0:
        return p.stdout, (p.stderr.strip().splitlines() or [f"exit {p.returncode}"])[-1]
    return p.stdout, None


def call(argv: List[str], parse: Callable[[str], T]) -> Tuple[Optional[T], Optional[str]]:
    """Run a command and parse its output; (result, None) or (None, error)."""
    out, err = run(*argv)
    if err:
        return None, err
    try:
        return parse(out), None
    except (ValueError, TypeError, AttributeError):
        return None, "unreadable output"


def vm_source(kind: str, key: str, args: List[str], parse: Callable[[str], List[Dict[str, Any]]]
              ) -> List[Dict[str, Any]]:
    exe, err = program(key)
    if err:
        return [view.vm_error(kind, err)]
    if not exe:
        return []
    rows, err = call([exe, *args], parse)
    return [view.vm_error(kind, err)] if err else rows or []


def docker_contexts(exe: str) -> List[Dict[str, Any]]:
    contexts, err = call([exe, "context", "ls", "--format", "json"], view.json_records)
    return [{"Name": "docker", "Error": err, "Listing": True}] if err else contexts or []


def engine(exe: str, ctx: Dict[str, Any]) -> view.Engine:
    """One docker context: its engine row and its containers."""
    if ctx.get("Error"):
        return view.engine_error(ctx, str(ctx["Error"]))
    sock = view.socket_path(ctx)
    if sock is not None and not os.path.exists(sock):
        return view.engine_down(ctx)
    argv = [exe, "--context", str(ctx.get("Name")), "ps", *(["-a"] if settings["show_exited"] else []),
            "--format", "json"]
    result, err = call(argv, lambda out: view.engine_up(ctx, out))
    return view.engine_error(ctx, err) if err else result


def render() -> View:
    with ThreadPoolExecutor(max_workers=8) as pool:
        vm_jobs = [
            pool.submit(vm_source, "colima", "colima", ["list", "--json"], view.colima_rows),
            pool.submit(vm_source, "lima", "limactl", ["list", "--json"], view.lima_rows),
            pool.submit(vm_source, "podman", "podman", ["machine", "list", "--format", "json"], view.podman_rows),
        ]
        docker, err = program("docker")
        if err:
            contexts = [{"Name": "docker", "Error": err, "Listing": True}]
        else:
            contexts = docker_contexts(docker) if docker else []
        engine_jobs = [pool.submit(engine, docker or "docker", c) for c in contexts]
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
