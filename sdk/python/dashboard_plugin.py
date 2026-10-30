"""A dashboard plugin SDK for Python. One file, standard library only, Python 3.9+.
Copy it next to your plugin.

    from dashboard_plugin import serve

    serve(
        manifest={"id": "hello", "name": "Hello",
                  "refresh": {"mode": "poll", "interval_ms": 5000}, "layout": {"size": "small"}},
        render=lambda: {"status": "ok", "summary": "hi", "root": {"type": "text", "text": "hi"}},
    )

The protocol is in docs/plugins.md. The widget types below are TypedDicts: plain dicts
at runtime, checked by a type checker such as mypy or pyright if you use one.
"""

from __future__ import annotations

import json
import sys
import time
import traceback
from typing import Any, Callable, Dict, List, Literal, Mapping, Optional, Tuple, TypedDict, Union

CONTRACT_VERSION = "0.1"

# Something outside the plugin is unavailable: a CLI is missing, not logged in, an API is down.
UPSTREAM_UNAVAILABLE = -32001
# The tile's `config:` block is invalid.
CONFIG_INVALID = -32002
METHOD_NOT_FOUND = -32601
INTERNAL_ERROR = -32603


class PluginError(Exception):
    """An error the tile shows. Raise it from `init` or `render`."""

    def __init__(self, code: int, message: str):
        super().__init__(message)
        self.code = code


def config_error(message: str) -> PluginError:
    """The config is wrong: the tile shows `message`."""
    return PluginError(CONFIG_INVALID, message)


def upstream_error(message: str) -> PluginError:
    """A CLI, service or API the plugin needs is unavailable: the tile shows `message`."""
    return PluginError(UPSTREAM_UNAVAILABLE, message)


# stdout carries only protocol messages, so print() goes to stderr once `serve` runs.
_out = sys.stdout


def _send(message: dict) -> None:
    _out.write(json.dumps({"jsonrpc": "2.0", **message}) + "\n")
    _out.flush()


def log(level: LogLevel, message: str) -> None:
    """Sends a line to the dashboard's log (not the tile)."""
    _send({"method": "log", "params": {"level": level, "message": message}})


def with_defaults(config: Any, defaults: Mapping[str, Any]) -> Dict[str, Any]:
    """Merges the user's config over `defaults`, and rejects keys that `defaults` doesn't
    have (usually a typo) with a config error."""
    if config is None:
        return dict(defaults)
    if not isinstance(config, dict):
        raise config_error("the config must be a mapping")
    unknown = sorted(set(config) - set(defaults))
    if unknown:
        raise config_error(f"unknown settings: {', '.join(unknown)}")
    return {**defaults, **config}


def live_series(
    id: str,
    label: str,
    values: Mapping[str, Optional[float]],
    now_ms: Optional[int] = None,
    **widget: Any,
) -> SeriesWidget:
    """A chart whose history the host keeps. Each render sends only the newest value per
    series (None sends no point, e.g. when a check failed); the host appends it and keeps
    the last `history` points (default 300). The plugin can stay stateless.

        live_series("latency", "Latency", {"api": 42, "web": None}, unit="ms")
    """
    now = int(time.time() * 1000) if now_ms is None else now_ms
    series = [{"name": name, "points": [] if v is None else [[now, v]]} for name, v in values.items()]
    return {"type": "series", "label": label, "id": id, **widget, "series": series}  # type: ignore[typeddict-item]


def serve(
    manifest: Union[Dict[str, Any], Callable[[Dict[str, Any]], Dict[str, Any]]],
    render: Callable[[], View],
    init: Optional[Callable[[Dict[str, Any]], None]] = None,
    shutdown: Optional[Callable[[], None]] = None,
) -> None:
    """Answers the dashboard's requests on stdin until it says `exit` or closes stdin.

    `manifest` is a dict or a function of the config that returns one; `contract_version`
    and `commands` are filled in. `init(config)` gets the tile's `config:` block (`{}` when
    there is none) before the manifest is sent; raise `config_error(...)` to reject it.
    `render()` returns the tile's view.
    """

    def initialize(params: dict) -> dict:
        config = params.get("config") or {}
        if init:
            init(config)
        m = manifest(config) if callable(manifest) else manifest
        return {"manifest": {"contract_version": CONTRACT_VERSION, "commands": [], **m}}

    def stop(_params: dict) -> None:
        if shutdown:
            shutdown()

    handlers = {"initialize": initialize, "view/render": lambda _: render(), "shutdown": stop}

    sys.stdout = sys.stderr
    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            msg = json.loads(line)
        except ValueError:
            print(f"dashboard-plugin: not JSON: {line[:200]}", file=sys.stderr)
            continue
        if msg.get("method") == "exit":
            break
        if "id" not in msg or "method" not in msg:
            continue  # a notification
        try:
            handler = handlers.get(msg["method"])
            if handler is None:
                raise PluginError(METHOD_NOT_FOUND, f"unknown method {msg['method']}")
            _send({"id": msg["id"], "result": handler(msg.get("params") or {})})
        except PluginError as e:
            _send({"id": msg["id"], "error": {"code": e.code, "message": str(e)}})
        except Exception as e:
            # A bug: the tile shows the error, the dashboard's log gets the traceback.
            traceback.print_exc()
            _send({"id": msg["id"], "error": {"code": INTERNAL_ERROR, "message": f"{type(e).__name__}: {e}"}})


# The contract's types. `node sdk/gen-types.mjs` writes everything between the markers.
# ---- BEGIN GENERATED (node sdk/gen-types.mjs) ----

Size = Literal["small", "medium", "large", "wide", "tall"]

LogLevel = Literal["error", "warn", "info", "debug"]

Status = Literal["ok", "warn", "error", "unknown"]


class _CommandDeclRequired(TypedDict):
    # Program and arguments. May contain `{name}` placeholders declared in `params`.
    argv: List[str]
    id: str
    label: str


class CommandDecl(_CommandDeclRequired, total=False):
    cwd: Optional[str]
    params: List["ParamDecl"]


class _LayoutHintsRequired(TypedDict):
    size: "Size"


class LayoutHints(_LayoutHintsRequired, total=False):
    min_size: Optional["Size"]
    priority: int


class ParamDecl(TypedDict):
    name: str
    # Regex the value must fully match.
    pattern: str


class _PollRefreshRequired(TypedDict):
    # Suggested period. Config can override it.
    interval_ms: int
    mode: Literal["poll"]


class PollRefresh(_PollRefreshRequired, total=False):
    # Floor for config overrides, e.g. to stay under an API rate limit.
    min_interval_ms: Optional[int]


class PushRefresh(TypedDict):
    mode: Literal["push"]


class _ManifestRequired(TypedDict):
    contract_version: str
    id: str
    layout: "LayoutHints"
    name: str
    refresh: "Refresh"


class Manifest(_ManifestRequired, total=False):
    commands: List["CommandDecl"]
    config_schema: Any


class OpenUrlAction(TypedDict):
    type: Literal["open_url"]
    url: str


class _RunCommandActionRequired(TypedDict):
    command: str
    type: Literal["run_command"]


class RunCommandAction(_RunCommandActionRequired, total=False):
    params: Dict[str, str]


class _BarRequired(TypedDict):
    label: str
    value: float


class Bar(_BarRequired, total=False):
    status: Optional["Status"]


class Column(TypedDict):
    key: str
    label: str


class _ListItemRequired(TypedDict):
    title: str


class ListItem(_ListItemRequired, total=False):
    action: Optional["Action"]
    status: Optional["Status"]
    subtitle: Optional[str]
    tags: List[str]


class _LogLineRequired(TypedDict):
    text: str


class LogLine(_LogLineRequired, total=False):
    level: Optional["LogLevel"]
    # Shown before the text, e.g. "14:02:11".
    time: Optional[str]


class Meter(TypedDict):
    label: str
    percent: float


class _RowRequired(TypedDict):
    # Keyed by `Column::key`.
    cells: Dict[str, "Cell"]


class Row(_RowRequired, total=False):
    action: Optional["Action"]
    status: Optional["Status"]


class SeriesData(TypedDict):
    name: str
    # (unix ms, value)
    points: List[Tuple[int, float]]


class Thresholds(TypedDict):
    error: float
    warn: float


class _UptimeCheckRequired(TypedDict):
    name: str
    # Result of the latest check.
    status: "Status"


class UptimeCheck(_UptimeCheckRequired, total=False):
    # e.g. "HTTP 200" or the error.
    detail: Optional[str]
    # Recent results, oldest first. Renderers show the newest that fit.
    history: List["Status"]
    # Response time of the latest check.
    latency_ms: Optional[float]
    # Share of recent checks that passed, 0-100.
    uptime: Optional[float]
    # Opened when the check is clicked.
    url: Optional[str]


class _TextWidgetRequired(TypedDict):
    text: str
    type: Literal["text"]


class TextWidget(_TextWidgetRequired, total=False):
    markdown: bool


class _StatWidgetRequired(TypedDict):
    label: str
    type: Literal["stat"]
    value: str


class StatWidget(_StatWidgetRequired, total=False):
    delta: Optional[str]
    status: Optional["Status"]
    unit: Optional[str]


class _GaugeWidgetRequired(TypedDict):
    label: str
    percent: float
    type: Literal["gauge"]


class GaugeWidget(_GaugeWidgetRequired, total=False):
    caption: Optional[str]
    marker: Optional[float]
    thresholds: Optional["Thresholds"]


class _SeriesWidgetRequired(TypedDict):
    label: str
    series: List["SeriesData"]
    type: Literal["series"]


class SeriesWidget(_SeriesWidgetRequired, total=False):
    color: "ColorScale"
    format: "ValueFormat"
    # Points the host keeps per series when `id` is set. Default 300.
    history: Optional[int]
    # When set, the host keeps history for this widget: each update's
    # `points` are new samples, appended to what the host already holds
    # for (tile, `id`, series name). Renderers always get the full history.
    id: Optional[str]
    # Draw the first series above the axis and the second below it,
    # e.g. network download and upload.
    mirrored: bool
    style: "SeriesStyle"
    unit: Optional[str]
    y_max: Optional[float]


class _MeterGridWidgetRequired(TypedDict):
    meters: List["Meter"]
    type: Literal["meter_grid"]


# Many small meters in a compact grid, e.g. one per CPU core.
class MeterGridWidget(_MeterGridWidgetRequired, total=False):
    color: "ColorScale"
    label: Optional[str]


class _BarsWidgetRequired(TypedDict):
    bars: List["Bar"]
    type: Literal["bars"]


# Labeled horizontal bars, in the order given.
class BarsWidget(_BarsWidgetRequired, total=False):
    format: "ValueFormat"
    label: Optional[str]
    # Value of a full bar. Default: the largest value.
    max: Optional[float]
    unit: Optional[str]


class _ListWidgetRequired(TypedDict):
    items: List["ListItem"]
    type: Literal["list"]


class ListWidget(_ListWidgetRequired, total=False):
    empty_text: Optional[str]


class TableWidget(TypedDict):
    columns: List["Column"]
    rows: List["Row"]
    type: Literal["table"]


class _LogWidgetRequired(TypedDict):
    lines: List["LogLine"]
    type: Literal["log"]


# Log output, oldest first. Renderers show the newest lines that fit.
class LogWidget(_LogWidgetRequired, total=False):
    label: Optional[str]


# Availability checks, one row each with a strip of recent results.
class UptimeWidget(TypedDict):
    checks: List["UptimeCheck"]
    type: Literal["uptime"]


class RowWidget(TypedDict):
    children: List["Widget"]
    type: Literal["row"]


class ColumnWidget(TypedDict):
    children: List["Widget"]
    type: Literal["column"]


class View(TypedDict):
    root: "Widget"
    status: "Status"
    # One line, used when the tile is too small.
    summary: str


class HostInfo(TypedDict):
    name: str
    version: str


class _InitializeParamsRequired(TypedDict):
    contract_version: str
    host: "HostInfo"


class InitializeParams(_InitializeParamsRequired, total=False):
    config: Any


Refresh = Union["PollRefresh", "PushRefresh"]

Action = Union["OpenUrlAction", "RunCommandAction"]

Cell = Union[str, float, "Widget"]

ColorScale = Union[Literal["accent"], Literal["heat"], Literal["cool"], Literal["sunset"]]

SeriesStyle = Union[Literal["line"], Literal["area"], Literal["bars"]]

ValueFormat = Union[Literal["plain"], Literal["bytes"], Literal["count"]]

Widget = Union["TextWidget", "StatWidget", "GaugeWidget", "SeriesWidget", "MeterGridWidget", "BarsWidget", "ListWidget", "TableWidget", "LogWidget", "UptimeWidget", "RowWidget", "ColumnWidget"]

# ---- END GENERATED ----
