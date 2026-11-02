# Writing a plugin

A plugin is any program that reads JSON-RPC requests on stdin and writes replies on stdout,
one JSON message per line. Use any language. The dashboard starts it, asks it for a
manifest once, then asks it for a view on a timer and draws that view in the terminal and
the browser.

**No plugin needed** when something can already write the data: the `http` built-in fetches
a URL on each refresh, and the `file` built-in watches a file and shows it again when it
changes (checking it every second where watching can't start, or with `watch: false`). Both
read the same data: a view or a widget in the JSON below, values picked out of any JSON with
`fields`, or text. A cron job, a CI step or another machine's dashboard can write it.

```yaml
modules:
  nightly:
    builtin: file
    config: { path: ~/.cache/backup/status.json }   # {"status": "ok", "summary": ..., "root": {...}}
  queue:
    builtin: http
    refresh: 15s
    config:
      url: http://buildbox:9000/stats             # any JSON
      token_env: QUEUE_TOKEN                       # sent as Authorization: Bearer ...
      fields:
        - { label: Waiting, pointer: /queue/waiting }
        - { label: p99, pointer: /latency/p99, unit: ms }
```

`format` is `auto` (a view or widget when the data is one, else indented JSON, else text),
`view`, `json` or `text`. A series widget with an `id` gets history kept by the host, so a
URL that returns one point per fetch draws a growing chart. All settings:
`dashboard widgets http --json` and `dashboard widgets file --json` (the `config_schema`).

Examples, all tested against the real host (`crates/host/tests/example_plugins.rs`). Each
is a [plugin package](#package-it): a folder with the program and a `dashboard-plugin.yaml`.

| Package | Shows |
|---|---|
| [`examples/plugins/hello`](../examples/plugins/hello) | The smallest plugin: the loop, `initialize`, `view/render` |
| [`examples/plugins/git-repos`](../examples/plugins/git-repos) | Config, a stats row and a table, `open_url` actions, `log` notifications, error codes, a config schema |
| [`examples/plugins/gh-actions`](../examples/plugins/gh-actions) | Node, no packages: async work, lists with tags and statuses, running a CLI |
| [`examples/plugins/latency`](../examples/plugins/latency) | A live chart using host-kept history, per-target stats, status from thresholds |
| [`plugins/cswap`](../plugins/cswap) | A Rust plugin: a table with gauges in cells |

Run them all on one screen: `cargo run -p dashboard -- --config examples/plugins.yaml serve`.
See them in the catalog: `cargo run -p dashboard -- --config examples/plugins.yaml widgets example`.

`plugins/ext/` holds the native-vs-plugin benchmark widgets: clock, textfile, git, docker,
hackernews, weather, feedreader and battery. Each one is a built-in module, written again as a
TypeScript (`*-ts`) and a Python (`*-py`) package with a copy of the SDK file.
`examples/bench.yaml` shows the three versions side by side, one screen per widget, and tests
check they show the same view. It also holds Python packages for services that need a token:
GitLab, Gerrit, Azure DevOps and CircleCI (`examples/widgets/w-dev-a.yaml`); Buildkite, Jenkins,
Asana, Todoist and Trello (`w-dev-b.yaml`); Datadog, New Relic, PagerDuty, Opsgenie and Sentry
(`w-ops-a.yaml`); Rollbar, Airbrake, UptimeRobot, DigitalOcean and Zendesk (`w-ops-b.yaml`);
and Healthchecks.io (`w-public.yaml`). These are untested live: built from the services' API docs
and tested against hand-written sample responses. See
[`plugins/ext/README.md`](../plugins/ext/README.md). `examples/plugins/` links to each of them,
so `examples/try.yaml` can show their demos by ID.

## Start from an SDK

For TypeScript and Python there is a small SDK: one file, no packages, that runs the loop,
fills in the manifest, turns thrown errors into the right error codes and has types for
every widget. Make a new plugin with it:

```sh
dashboard plugin new --lang ts my-plugin      # main.ts + dashboard-plugin.ts (Node 24 or Bun)
dashboard plugin new --lang py my-plugin      # main.py + dashboard_plugin.py (Python 3.9+)
dashboard plugin-check -- node my-plugin/main.ts
```

The new folder already passes `plugin-check`. A TypeScript plugin looks like this:

```ts
import { serve, withDefaults, configError } from "./dashboard-plugin.ts";

let config = { name: "world" };

serve({
  manifest: { id: "hello", name: "Hello", refresh: { mode: "poll", interval_ms: 5000 }, layout: { size: "small" } },
  init(c) { config = withDefaults(c, config); },   // throw configError("...") to reject
  render: () => ({ status: "ok", summary: `hello, ${config.name}`, root: { type: "text", text: "hi" } }),
});
```

| | TypeScript | Python |
|---|---|---|
| Run the loop | `serve({ manifest, init, render, shutdown })` | `serve(manifest=, render=, init=, shutdown=)` |
| Bad config (-32002) | `throw configError(msg)` | `raise config_error(msg)` |
| Service down (-32001) | `throw upstreamError(msg)` | `raise upstream_error(msg)` |
| Defaults, reject unknown keys | `withDefaults(config, defaults)` | `with_defaults(config, defaults)` |
| A line in the dashboard's log | `log("info", msg)` | `log("info", msg)` |
| Chart with host-kept history | `liveSeries({ id, label }, { name: value })` | `live_series(id, label, {name: value})` |

Any other exception becomes error -32603 with its message, and the stack goes to stderr.
`console.log` and `print` go to stderr too, so they can't break the protocol. More in
[`sdk/typescript`](../sdk/typescript/README.md) and [`sdk/python`](../sdk/python/README.md),
with examples (`hello`, `big_files` for stats and a table, `load` for a live chart) tested
against the real host in `crates/host/tests/sdk_examples.rs`.

## Check a plugin

```sh
dashboard plugin-check -- python3 my_plugin.py
dashboard plugin-check --plugin-config '{"repos": ["osdlc/dashboard"]}' --renders 5 -- node my_plugin.mjs
```

`plugin-check` starts the plugin the way the dashboard does and calls `initialize` and then
`view/render` a few times. It reports mistakes the JSON alone won't catch and prints the tile
as the terminal UI would draw it. Those mistakes include:
- percents outside 0–100;
- chart times in seconds instead of milliseconds;
- table cells with no matching column;
- links that aren't http(s).

It exits non-zero on a problem. Notes (things this host ignores for now) don't fail it.
With `--json` it prints one JSON document instead of the report: the manifest, each render, the
last view and the findings.

## Add it to a config

A plugin can run straight from a command:

```yaml
modules:
  my-tile:                       # the tile's ID in screens
    plugin: [python3, plugins/my_plugin.py]
    refresh: 30s                 # optional: overrides the manifest's interval_ms
    config: { repos: [a, b] }    # passed to initialize as params.config
screens:
  - name: main
    tiles:
      - { module: my-tile, size: medium }
```

A plugin started with `plugin:` runs in the config file's folder, so relative paths resolve
from there. On Windows the program is looked up in `PATH` with `PATHEXT` (so `npx` finds
`npx.cmd`), and `python3` is the first Python in `PATH`, or `py -3` instead of the Microsoft
Store alias; see [windows.md](windows.md). Changes to the file apply while the dashboard runs;
the plugin restarts only if its own settings changed.

Or, once it is a package, by its ID:

```yaml
modules:
  my-tile:
    package: my-plugin           # runs the package's `run:` in its folder
    config: { repos: [a, b] }
```

A package runs in **its own folder**, not the config file's. Relative paths in its `config:`
therefore resolve from the package folder (for an installed package, somewhere under
`~/.local/share/dashboard/plugins/`). Use absolute paths in a package's config, or `~` paths
if the plugin expands them (the git-repos example does).
`dashboard config` reports a `package:` that isn't installed.

With `demo: true` in its entry (beside `package:`, not inside `config:`), the tile shows the
package's `demo:` view instead of running it: no process starts, and the tile is tagged `demo`.
A plugin started with `plugin:` has no `demo:` view, so its demo tile says so.

A package tile restarts when its entry in the config changes. Upgrading the package in place
(new files or a new `run:`) takes effect once its config entry changes or the dashboard
restarts.

## Package it

A package is a folder named after the plugin's ID, holding the plugin's files and a
`dashboard-plugin.yaml`:

```yaml
id: my-plugin                    # lowercase letters, digits and dashes; also the folder name
version: 0.1.0
name: My plugin
description: What it shows, in a sentence or two.
tags: [dev, github]              # used by search and the --tag filter
run: [python3, my_plugin.py]     # program and arguments, run in the package folder
runtime: python3>=3.9            # the interpreter it needs, if any (node>=22, python3...)
binaries: [gh]                   # other programs it runs
env: [MY_SERVICE_TOKEN]          # environment variables it reads
hosts: [api.example.com]         # network hosts it calls
size: medium                     # suggested tile size (default medium)
config_schema:                   # JSON Schema for the tile's config: block
  type: object
  properties:
    repos: { type: array, items: { type: string } }
demo:                            # a view with made-up data, shown before it is set up
  status: ok
  summary: 2 repos
  root: { type: text, text: "Made-up data, **no account needed**", markdown: true }
```

Rules the loader checks:
- `id` matches the folder name;
- `version`, `name` and `description` are not empty;
- `version` holds only letters, digits, dots, dashes and plus (`1.2.0`, `1.0.0-beta.1`);
- `run:` names a program on PATH, or a path inside the package. A program given as a path
  (`./plugin`, `bin/plugin`) is relative to the folder;
- `runtime:` is a program name with an optional minimum, `name>=1.2`;
- `demo:` is a valid view, with no problems `plugin-check` would report.

Unknown keys are an error. `dashboard schema package` prints the JSON Schema.

The dashboard looks for packages in two places, first match wins:
- `plugins/<id>/` next to the config file (this is how `examples/plugins.yaml` finds the
  examples);
- `<data home>/dashboard/plugins/<id>/`: `~/.local/share/dashboard/plugins/` by default,
  moved by `XDG_DATA_HOME`.

To install one, use `dashboard plugin install` (see [marketplace.md](marketplace.md)), or copy
its folder there by hand. A folder that isn't a valid package is skipped with a message; it
doesn't stop the others.

## Find widgets

```sh
dashboard widgets                 # everything: built-ins, bundled plugins, packages, marketplace
dashboard widgets pull requests   # every word must appear in the ID, name, description or tags
dashboard widgets --tag dev
dashboard widgets jira --json     # {"schemaVersion", "kind": "widgets", "entries": [...]}: full entries, with config schema and demo view
```

Search ignores case. `--tag` keeps entries with exactly that tag. Built-in modules are added
under `modules:` by their ID (or with `builtin: <id>` for a second copy); packages with
`package: <id>`.

The web editor's "Add component" list shows the same catalog (`GET /api/catalog?q=&tag=`) and
writes these entries for you.

Marketplace packages that are listed but not installed come from a cached index,
`<data home>/dashboard/marketplace/index.json` (e.g. `~/.local/share/dashboard/marketplace/`):
a copy of the last index `dashboard plugin install`, `list` or `update` read. They show as
`market` in `dashboard widgets` and can't be added until installed. The index format is in
[marketplace.md](marketplace.md#the-index).

## The protocol

Every message is one line of JSON. A request has `id`, `method` and `params`; answer it with
the same `id` and either `result` or `error`. A message without an `id` is a notification and
gets no answer. `jsonrpc: "2.0"` goes on everything you send.

**stdout carries only protocol messages.** Write anything else to stderr; the dashboard puts
it in its log. Flush after each line (Python `print(..., flush=True)`; Node's
`process.stdout.write` is fine).

### `initialize` (host → plugin, once)

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"contract_version":"0.1","host":{"name":"dashboard","version":"0.1.0"},"config":{"repos":["a"]}}}
```

`params.config` is the tile's `config:` block (or `null`). Reply with the manifest:

```json
{"jsonrpc":"2.0","id":1,"result":{"manifest":{
  "id": "my-plugin",
  "name": "My plugin",
  "contract_version": "0.1",
  "refresh": {"mode": "poll", "interval_ms": 30000, "min_interval_ms": 5000},
  "layout": {"size": "medium", "priority": 0, "min_size": "small"},
  "commands": [],
  "config_schema": null
}}}
```

| Field | Meaning |
|---|---|
| `id`, `name` | The module's ID and display name |
| `contract_version` | `"0.1"`. The host refuses a different major version |
| `refresh` | Only `poll` for now: `interval_ms` is the suggested period, `min_interval_ms` the floor for a config override |
| `layout.size` | Suggested size: `small` (3×1), `medium` (4×2), `large` (6×3), `wide` (8×2), `tall` (4×4) |
| `layout.priority` | Higher is placed earlier when the dashboard places tiles itself |
| `commands` | Declared commands; not run by the host yet. Leave empty |
| `config_schema` | Optional JSON Schema for your `config:` block |

If the config is wrong, reply with an error (code `-32002`) so the tile shows why.

### `view/render` (host → plugin, every interval)

```json
{"jsonrpc":"2.0","id":2,"method":"view/render"}
```

Reply with a view:

```json
{"jsonrpc":"2.0","id":2,"result":{"status":"ok","summary":"3 repos · 1 dirty","root":{"type":"stat","label":"Dirty","value":"1"}}}
```

| Field | Meaning |
|---|---|
| `status` | `ok`, `warn`, `error` or `unknown`; colors the tile's border |
| `summary` | One line, shown when the tile is too small and in the tile header |
| `root` | A widget (below) |

Renders never overlap: the host waits for your reply, then waits the interval.

### `log` (plugin → host, any time)

```json
{"jsonrpc":"2.0","method":"log","params":{"level":"warn","message":"rate limited, retrying"}}
```

Levels: `error`, `warn`, `info`, `debug`. Goes to the dashboard's log, not the tile.

### `shutdown` and `exit`

Answer `shutdown` with `null`, and exit on the `exit` notification. Most plugins don't need
to: the dashboard kills the process when it stops the module.

### Errors

| Code | Use for |
|---|---|
| `-32001` | Something outside the plugin is unavailable: a CLI isn't installed, not logged in, an API is down |
| `-32002` | The config is invalid |
| `-32601` | Unknown method |
| `-32603` | A bug in the plugin |

The tile shows the error's `message`. A failed render keeps the last good view, marked stale.

### Lifecycle and limits

- **Timeouts:** each request times out after 30 s.
- **Restarts:** if the plugin exits, or a request times out or fails to parse, the process is
  dropped and started again (with a fresh `initialize`) at the next refresh. The refresh
  period works as the restart backoff.
- **Requests:** the plugin gets one request at a time.

## Widgets

A widget is a JSON object with a `type`. Optional fields can be left out.

```jsonc
{"type":"text","text":"**bold** and `code`","markdown":true}

{"type":"stat","label":"Errors","value":"1.2","unit":"%","delta":"+0.3","status":"warn"}

{"type":"gauge","label":"Disk","percent":72,"marker":50,"thresholds":{"warn":75,"error":90},"caption":"280 of 500 GB"}
// marker: a tick on the bar, e.g. where an even pace would be

{"type":"series","label":"Latency","unit":"ms","style":"line","color":"accent",
 "series":[{"name":"api","points":[[1793000000000,42.5],[1793000005000,40.1]]}]}
// points: [unix milliseconds, value], oldest first
// style: line | area | bars    color: accent | heat | cool | sunset
// format: plain | bytes | count   mirrored: true draws the 2nd series below the axis
// y_max: fixes the top of the scale

{"type":"meter_grid","label":"Cores","color":"heat","meters":[{"label":"0","percent":31},{"label":"1","percent":77}]}

{"type":"bars","label":"Builds","format":"count","bars":[{"label":"api","value":935},{"label":"web","value":120,"status":"warn"}]}

{"type":"pie","label":"Disk","format":"bytes","slices":[{"label":"photos","value":64424509440},
 {"label":"apps","value":32212254720},{"label":"free","value":10737418240,"status":"warn"}]}
// shares of the total, clockwise from the top; donut: false draws a full disc
// color: accent (a color per slice) | heat | cool | sunset; a slice's status wins

{"type":"list","empty_text":"Nothing to review","items":[
  {"title":"Fix login","subtitle":"repo#12 · 3h ago","status":"warn","tags":["draft"],
   "action":{"type":"open_url","url":"https://example.com/12"}}]}

{"type":"table","columns":[{"key":"name","label":"Name"},{"key":"load","label":"Load"}],
 "rows":[{"cells":{"name":"atlas","load":{"type":"gauge","label":"","percent":12}},"status":"ok",
          "action":{"type":"open_url","url":"https://example.com/atlas"}}]}
// a cell is text, a number, or a stat/gauge/pie widget
// a column's priority (default 0) says which to drop first when the tile is narrow:
// the lowest goes first, then the rightmost; the first column is always kept

{"type":"log","label":"api.log","lines":[{"text":"started","level":"info","time":"14:02:11"}]}

{"type":"uptime","checks":[{"name":"api","status":"ok","uptime":99.5,"latency_ms":120,
  "detail":"HTTP 200","history":["ok","ok","error","ok"],"url":"https://api.example.com"}]}

{"type":"row","children":[ ... ]}      // side by side (stacks when narrow)
{"type":"column","children":[ ... ]}   // top to bottom
```

Clicking a list item, table row or uptime check with a URL opens it (http and https only).

### Charts with history kept by the host

Give a `series` widget an `id`, and the host keeps its history. Each render sends only the
**new** points; the host appends them per (tile, `id`, series name), keeps the last
`history` points (default 300) and draws the whole line. The plugin can stay stateless. See
`examples/plugins/latency`. Without an `id`, send the full series each time.

## Schemas and types

```sh
dashboard schema manifest     # JSON Schema for the manifest
dashboard schema view         # ... for a view (all widgets)
dashboard schema initialize-params
dashboard schema package      # ... for dashboard-plugin.yaml
dashboard schema catalog-entry  # ... for one of the entries of `dashboard widgets --json`
```

The SDKs' types are generated from these by `node sdk/gen-types.mjs`; a test fails when they
fall behind. For other languages, generate types from them (`typify` for Rust, for example).
Rust plugins can use the `dashboard-contract` crate directly.

## Not yet

- **Push mode:** a plugin sending `view/update` on its own schedule. The host rejects it today.
- **Running declared commands:** with confirmation in the UI.
- **Clicks sent back to the plugin.**
- **WASM plugins:** sandboxed.
