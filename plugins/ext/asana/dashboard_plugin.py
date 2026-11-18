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

class _CommandDeclRequired(TypedDict):
    # Program and arguments. May contain `{name}` placeholders declared in `params`.
    argv: List[str]
    # Names the command in `run_command` actions, e.g. "switch".
    id: str
    # Shown to the user, e.g. "Switch to account".
    label: str


# A command a module declares. The host runs it only after the user confirms.
class CommandDecl(_CommandDeclRequired, total=False):
    # Working directory to run in. `None`: the host's own.
    cwd: Optional[str]
    # Placeholders `argv` may use, each checked against its pattern.
    params: List["ParamDecl"]


class _LayoutHintsRequired(TypedDict):
    # The size the module suggests; a screen or the config can override it.
    size: "Size"


# Size and placement hints for a module's tile.
class LayoutHints(_LayoutHintsRequired, total=False):
    # The smallest size the view still works at. `None`: any size.
    min_size: Optional["Size"]
    # Higher is placed earlier when auto-placing tiles. Default 0.
    priority: int


# A `{name}` placeholder in a command's `argv`.
class ParamDecl(TypedDict):
    # The placeholder's name, without braces.
    name: str
    # Regex the value must fully match.
    pattern: str


class _PollRefreshRequired(TypedDict):
    # Suggested period. Config can override it.
    interval_ms: int
    mode: Literal["poll"]


# The host asks for a view (`view/render`) every `interval_ms`.
class PollRefresh(_PollRefreshRequired, total=False):
    # Floor for config overrides, e.g. to stay under an API rate limit.
    min_interval_ms: Optional[int]


# The module sends `view/update` notifications when it has a new view.
class PushRefresh(TypedDict):
    mode: Literal["push"]


class _ManifestRequired(TypedDict):
    # The contract version the module was built for, e.g. "0.1".
    contract_version: str
    # Unique module ID, e.g. `cswap`.
    id: str
    # Size and placement hints for the module's tile.
    layout: "LayoutHints"
    # Display name, shown in the tile header.
    name: str
    # How the module's view is refreshed: polled by the host or pushed by the module.
    refresh: "Refresh"


# What a module declares about itself, in reply to `initialize`.
class Manifest(_ManifestRequired, total=False):
    # Commands the host may run for this module after the user confirms.
    commands: List["CommandDecl"]
    # JSON Schema for the module's `config:` block. `None`: no schema.
    config_schema: Any


# Open a link in the browser.
class OpenUrlAction(TypedDict):
    type: Literal["open_url"]
    # The link: http or https only.
    url: str


class _RunCommandActionRequired(TypedDict):
    # The command's `id` from the manifest.
    command: str
    type: Literal["run_command"]


# Run a command the module declared, after the user confirms.
class RunCommandAction(_RunCommandActionRequired, total=False):
    # Values for the command's `{name}` placeholders. Each must match its pattern.
    params: Dict[str, str]


class _BarRequired(TypedDict):
    # Shown beside the bar.
    label: str
    # Zero or more; the bar's length is this over the widget's `max`.
    value: float


# One bar in a `bars` widget.
class Bar(_BarRequired, total=False):
    # Colors the bar ok, warn or error.
    status: Optional["Status"]
    # Fill color when `status` is not set. Default: `accent`.
    tone: Optional["Tone"]


class _ColumnRequired(TypedDict):
    # Names the cell in each row's `cells`.
    key: str
    # The column heading.
    label: str


# A table column.
class Column(_ColumnRequired, total=False):
    # Which columns to keep when a table is too narrow for all of them:
    # renderers drop the lowest priority first (the rightmost of equal
    # ones) and always keep the first column. Default 0.
    priority: Optional[int]


class _ListItemRequired(TypedDict):
    # The main line.
    title: str


# One item in a `list` widget.
class ListItem(_ListItemRequired, total=False):
    # What clicking the item does, e.g. open a link.
    action: Optional["Action"]
    # Colors the item's marker ok, warn or error.
    status: Optional["Status"]
    # A second, quieter line.
    subtitle: Optional[str]
    # Short labels shown as chips.
    tags: List[str]
    # Color of the item's marker when `status` is not set.
    tone: Optional["Tone"]


class _LogLineRequired(TypedDict):
    # The log message.
    text: str


# One line in a `log` widget.
class LogLine(_LogLineRequired, total=False):
    # error, warn, info or debug: colors the line.
    level: Optional["LogLevel"]
    # Shown before the text, e.g. "14:02:11".
    time: Optional[str]


class _MeterRequired(TypedDict):
    # Shown beside the meter, e.g. a core number.
    label: str
    # 0-100.
    percent: float


# One small meter in a `meter_grid`.
class Meter(_MeterRequired, total=False):
    # A fixed color for this meter instead of the grid's color scale.
    tone: Optional["Tone"]


class _RowRequired(TypedDict):
    # Keyed by the column's `key`.
    cells: Dict[str, "Cell"]


# A table row.
class Row(_RowRequired, total=False):
    # What clicking the row does, e.g. open a link.
    action: Optional["Action"]
    # Colors the row's marker ok, warn or error.
    status: Optional["Status"]


class _SeriesDataRequired(TypedDict):
    # Shown in the legend; also keys the history when the widget has an `id`.
    name: str
    # (unix ms, value)
    points: List[Tuple[int, float]]


# One line of a `series` chart.
class SeriesData(_SeriesDataRequired, total=False):
    # This series' color, instead of the widget's `color` (one series) or
    # the next `series-N` token (several).
    tone: Optional["Tone"]


class _SliceRequired(TypedDict):
    # Shown in the legend.
    label: str
    # Zero or more. Shown with the widget's `format` and `unit`.
    value: float


# One slice of a `pie`.
class Slice(_SliceRequired, total=False):
    # Colors the slice ok, warn or error.
    status: Optional["Status"]
    # Color when `status` is not set. Default: the next `series-N` token.
    tone: Optional["Tone"]


# Percents at which a gauge's fill changes color.
class Thresholds(TypedDict):
    # The percent at which the fill turns the error color.
    error: float
    # The percent at which the fill turns the warn color.
    warn: float


class _UptimeCheckRequired(TypedDict):
    # What is checked, e.g. a host or URL.
    name: str
    # Result of the latest check.
    status: "Status"


# One availability check in an `uptime` widget.
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
    # The text to show.
    text: str
    type: Literal["text"]


# A paragraph of text.
class TextWidget(_TextWidgetRequired, total=False):
    # Read markdown in `text`: `**bold**`, and code between backticks.
    markdown: bool


class _StatWidgetRequired(TypedDict):
    # Shown above or before the value.
    label: str
    type: Literal["stat"]
    # Already formatted, e.g. "3451" or "1.2k".
    value: str


# One number with a label, e.g. "p99 107 ms".
class StatWidget(_StatWidgetRequired, total=False):
    # A change to show beside the value, e.g. "+2.8%".
    delta: Optional[str]
    # Colors the value: ok, warn or error.
    status: Optional["Status"]
    # Shown after the value, smaller, e.g. "ms" or "/s".
    unit: Optional[str]


class _GaugeWidgetRequired(TypedDict):
    # Shown with the meter.
    label: str
    # How full, 0-100.
    percent: float
    type: Literal["gauge"]


# A horizontal meter from 0 to 100%.
class GaugeWidget(_GaugeWidgetRequired, total=False):
    # A line under the meter, e.g. "1721 of 1858 GiB used".
    caption: Optional[str]
    # A tick on the track at this percent, e.g. where usage should be by now.
    marker: Optional[float]
    # Percents at which the fill turns warn and error colored.
    thresholds: Optional["Thresholds"]


class _SeriesWidgetRequired(TypedDict):
    # The chart's title.
    label: str
    # One or more lines, each with its points.
    series: List["SeriesData"]
    type: Literal["series"]


# Values over time, as a line, a filled area or bars.
class SeriesWidget(_SeriesWidgetRequired, total=False):
    # The color of a single series.
    color: "ColorScale"
    # Scale to the data's own range with a little padding (for prices that
    # sit far from 0). Ignored when `y_min` or `y_max` is set, and when
    # `mirrored`.
    fit: bool
    # How values are written: plain, bytes or count.
    format: "ValueFormat"
    # Points the host keeps per series when `id` is set. Default 300.
    history: Optional[int]
    # When set, the host keeps history for this widget: each update's
    # `points` are new samples, appended to what the host already holds
    # for (tile, `id`, series name). Renderers always get the full history.
    id: Optional[str]
    # Draw the first series above the axis and the second below it,
    # e.g. network download and upload. Negative values count as 0 here
    # (`dashboard check` flags them).
    mirrored: bool
    # A line, a filled area or one bar per sample.
    style: "SeriesStyle"
    # Shown after values, e.g. "%" or "/s".
    unit: Optional[str]
    # The top of the chart. Default: the largest value (at least 1 when no
    # value is negative and `y_min` is unset).
    y_max: Optional[float]
    # The bottom of the chart. Default: 0, or the smallest value when that
    # is negative. Set it to chart values that never reach 0.
    y_min: Optional[float]


class _MeterGridWidgetRequired(TypedDict):
    # The meters, in order.
    meters: List["Meter"]
    type: Literal["meter_grid"]


# Many small meters in a compact grid, e.g. one per CPU core.
class MeterGridWidget(_MeterGridWidgetRequired, total=False):
    # The meters' color: `accent`, or a scale colored by each meter's value.
    color: "ColorScale"
    # A title above the grid.
    label: Optional[str]


class _BarsWidgetRequired(TypedDict):
    # The bars, top to bottom.
    bars: List["Bar"]
    type: Literal["bars"]


# Labeled horizontal bars, in the order given.
class BarsWidget(_BarsWidgetRequired, total=False):
    # How values are written: plain, bytes or count.
    format: "ValueFormat"
    # A title above the bars.
    label: Optional[str]
    # Value of a full bar. Default: the largest value.
    max: Optional[float]
    # Shown after each value.
    unit: Optional[str]


class _PieWidgetRequired(TypedDict):
    # The slices, in order. Values are zero or more.
    slices: List["Slice"]
    type: Literal["pie"]


# Shares of a whole, as a pie or donut with a legend. Slices keep the
# order given; each one's share is its value over the sum of all values.
class PieWidget(_PieWidgetRequired, total=False):
    # `accent` (the default) gives each slice its own color, the theme's
    # accent first. The other scales spread the slices along the scale
    # in order. A slice's `status` overrides its color.
    color: "ColorScale"
    # A ring with a hole in the middle. Default true; false draws a full disc.
    donut: bool
    # How values are written in the legend: plain, bytes or count.
    format: "ValueFormat"
    # A title above the chart.
    label: Optional[str]
    # Shown after each value in the legend.
    unit: Optional[str]


class _ClockFaceWidgetRequired(TypedDict):
    # 0-23.
    hour: int
    # 0-59.
    minute: int
    type: Literal["clock_face"]


# An analog clock face with hour and minute hands, and a second hand
# when `second` is given. The renderer draws it; it never reads a clock.
class ClockFaceWidget(_ClockFaceWidgetRequired, total=False):
    # Shown below the face, e.g. a date.
    caption: Optional[str]
    # Shown above the face, e.g. "Tokyo".
    label: Optional[str]
    # 0-59. No second hand when absent.
    second: Optional[int]
    # Colors the face's outline.
    status: Optional["Status"]


class _DisplayWidgetRequired(TypedDict):
    # Already formatted, e.g. "14:05". The TUI draws it in a block font
    # when it has a glyph for every character, else as plain text; see
    # docs/contract.md, "Clock face and display".
    text: str
    type: Literal["display"]


# One value as large as the space allows: a big number or a clock.
class DisplayWidget(_DisplayWidgetRequired, total=False):
    # Shown below the text.
    caption: Optional[str]
    # Shown above the text.
    label: Optional[str]
    # Colors the text.
    status: Optional["Status"]
    # Shown after the text, smaller, e.g. "PM".
    unit: Optional[str]


class _ListWidgetRequired(TypedDict):
    # The items, top to bottom.
    items: List["ListItem"]
    type: Literal["list"]


# Items with a title, a subtitle, a status marker and tags.
class ListWidget(_ListWidgetRequired, total=False):
    # Shown when there are no items.
    empty_text: Optional[str]


# Rows and columns. A cell holds text, a number, or a stat, gauge, pie, display or clock face.
class TableWidget(TypedDict):
    # The columns, left to right.
    columns: List["Column"]
    # The rows, top to bottom.
    rows: List["Row"]
    type: Literal["table"]


class _LogWidgetRequired(TypedDict):
    # The lines, oldest first.
    lines: List["LogLine"]
    type: Literal["log"]


# Log output, oldest first. Renderers show the newest lines that fit.
class LogWidget(_LogWidgetRequired, total=False):
    # A title above the lines.
    label: Optional[str]


# Availability checks, one row each with a strip of recent results.
class UptimeWidget(TypedDict):
    # The checks, one row each.
    checks: List["UptimeCheck"]
    type: Literal["uptime"]


class _ImageWidgetRequired(TypedDict):
    # Image file: PNG, JPEG, GIF (first frame), WebP or BMP.
    path: str
    type: Literal["image"]


# A picture from a file on the machine the dashboard runs on. The terminal
# draws it with the best graphics it supports (Kitty, iTerm2 or Sixel),
# or with colored half blocks. It is scaled to fit and keeps its shape.
class ImageWidget(_ImageWidgetRequired, total=False):
    # A line under the picture, e.g. the file name.
    caption: Optional[str]


# Widgets side by side, sharing the width.
class RowWidget(TypedDict):
    # The widgets, left to right.
    children: List["Widget"]
    type: Literal["row"]


# Widgets stacked top to bottom.
class ColumnWidget(TypedDict):
    # The widgets, top to bottom.
    children: List["Widget"]
    type: Literal["column"]


# What a tile shows: an overall status, a one-line summary and a widget tree.
class View(TypedDict):
    # The widget the tile shows; use `row` and `column` to combine several.
    root: "Widget"
    # The tile's overall health, shown as its status color.
    status: "Status"
    # One line, used when the tile is too small.
    summary: str


# Who is running the plugin.
class HostInfo(TypedDict):
    # The host's name, e.g. "dashboard".
    name: str
    # The host's version, e.g. "0.1.0".
    version: str


class _InitializeParamsRequired(TypedDict):
    # The host's contract version, e.g. "0.1".
    contract_version: str
    # The host running the plugin.
    host: "HostInfo"


# Params of the `initialize` request.
class InitializeParams(_InitializeParamsRequired, total=False):
    # The module's `config:` block from the config file. Null when absent.
    config: Any


# How a tile gets new views.
Refresh = Union["PollRefresh", "PushRefresh"]

# A tile's size class on the 12-column grid.
Size = Union[Literal["small"], Literal["medium"], Literal["large"], Literal["wide"], Literal["tall"]]

# What clicking a row or item does.
Action = Union["OpenUrlAction", "RunCommandAction"]

# A table cell: text, a number or a small widget.
Cell = Union[str, float, "Widget"]

# A color, or a scale that colors by value.
ColorScale = Union[Literal["accent"], Literal["heat"], Literal["cool"], Literal["sunset"]]

# How serious a log line is.
LogLevel = Union[Literal["error"], Literal["warn"], Literal["info"], Literal["debug"]]

# How a `series` chart draws its values.
SeriesStyle = Union[Literal["line"], Literal["area"], Literal["bars"]]

# Health of a tile, row or item: colors it ok, warn or error.
Status = Union[Literal["ok"], Literal["warn"], Literal["error"], Literal["unknown"]]

# A token a widget can color something with, e.g. one line of a chart.
Tone = Union[Literal["accent"], Literal["ok"], Literal["warn"], Literal["error"], Literal["info"], Literal["muted"], Literal["series-1"], Literal["series-2"], Literal["series-3"], Literal["series-4"], Literal["series-5"], Literal["series-6"]]

# How numbers are written.
ValueFormat = Union[Literal["plain"], Literal["bytes"], Literal["count"]]

# A piece of a view. `type` names the kind: `text`, `stat`, `gauge`, `table` and so on.
Widget = Union["TextWidget", "StatWidget", "GaugeWidget", "SeriesWidget", "MeterGridWidget", "BarsWidget", "PieWidget", "ClockFaceWidget", "DisplayWidget", "ListWidget", "TableWidget", "LogWidget", "UptimeWidget", "ImageWidget", "RowWidget", "ColumnWidget"]

# ---- END GENERATED ----
