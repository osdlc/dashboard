---
name: plugin
description: Write a dashboard plugin, a program in any language that answers JSON-RPC requests on stdin and stdout with a manifest and views (stats, gauges, charts, lists, tables, logs). Scaffold it with `dashboard plugin new`, check it with `dashboard plugin-check`, and add it to the config. Use when someone wants a tile the dashboard doesn't have built in.
---

# Dashboard plugin

A plugin is one program. The dashboard starts it, writes one JSON request per line to its
stdin, and reads one reply per line from its stdout. Any language works. TypeScript and Python
have a small SDK.

Steps: decide, scaffold, check, add to the config, iterate. The protocol and widgets are at the
end. `docs/plugins.md` in the repository is the full reference.

## Decide

- **Does it need a plugin at all?** If something can already write the data, use a built-in:
  `http` fetches a URL on each refresh and `file` watches a file. Both show a view in the JSON
  below, values picked out of any JSON with `fields`, or text. A cron job or CI step can write it.
- **What it shows:** pick widgets from [Widgets](#widgets).
- **How often it refreshes:** `interval_ms`. Poll a remote API no faster than its rate limit
  allows, and put that floor in `min_interval_ms`.
- **What the tile's status means:** `ok`, `warn`, `error` or `unknown`.
- **What the config takes,** and where its secrets come from. Secrets come from environment
  variables, never the config file.

## Scaffold

```sh
dashboard plugin new --lang ts my-plugin   # main.ts + dashboard-plugin.ts (Node 24 or Bun)
dashboard plugin new --lang py my-plugin   # main.py + dashboard_plugin.py (Python 3.9+)
```

The name is the plugin's ID: lowercase letters, digits and dashes, starting with a letter. An
optional third argument picks the folder (default `./my-plugin`). The new folder already passes
`plugin-check`, and the command prints the line to add to the config.

The SDK file runs the loop, fills in `contract_version` and `commands`, and turns errors into
the right codes:

| | TypeScript | Python |
|---|---|---|
| Run the loop | `serve({ manifest, init, render, shutdown })` | `serve(manifest=, render=, init=, shutdown=)` |
| Bad config (-32002) | `throw configError(msg)` | `raise config_error(msg)` |
| Service down (-32001) | `throw upstreamError(msg)` | `raise upstream_error(msg)` |
| Defaults, reject unknown keys | `withDefaults(config, defaults)` | `with_defaults(config, defaults)` |
| A line in the dashboard's log | `log("info", msg)` | `log("info", msg)` |
| Chart with host-kept history | `liveSeries({ id, label }, { name: value })` | `live_series(id, label, {name: value})` |

`console.log` and `print` go to stderr, so they can't break the protocol. Any other exception
becomes error -32603.

Without the SDK, follow [The protocol](#the-protocol). Rules: stdout carries only protocol lines,
debug output goes to stderr, and flush after every line.

Complete examples, each a package folder with a `dashboard-plugin.yaml`: `examples/plugins/hello`,
`git-repos` (config, a table, links, errors, a config schema), `gh-actions` (Node, a list from a
CLI) and `latency` (a live chart).

## Check

```sh
dashboard plugin-check -- node my-plugin/main.ts
dashboard plugin-check --plugin-config '{"key": "value"}' --renders 5 -- python3 my-plugin/main.py
```

It runs the plugin as the dashboard does, calls `initialize` and then `view/render` (3 times by
default), and prints the manifest, each render and the tile drawn as text. It lists problems the
JSON alone won't catch: percents outside 0–100, chart times in seconds instead of milliseconds,
table cells with no matching column, and links that aren't http(s). It exits non-zero on a
problem. Notes, about things the host ignores for now, don't fail it.

Fix every problem, then look at the drawn tile: is it readable at that size? `--width` (default
80) and `--height` try other sizes; on a dashboard a small tile is 5 rows, medium and wide 10,
large 15, tall 20. `--wait-ms` sets the wait between renders. `--json` prints the manifest,
renders, view and findings as one JSON document.

## Add

```yaml
modules:
  my-tile:
    plugin: [python3, /abs/path/my-plugin/main.py]   # relative paths start in the config file's folder
    refresh: 30s                                     # optional: overrides interval_ms
    config: { key: value }                           # arrives as params.config in initialize
screens:
  - name: main
    tiles:
      - { module: my-tile, size: medium }
```

The dashboard picks up the change while it runs. Run `dashboard config` to check the file.

To share it, make it a package: a folder named after the ID with a `dashboard-plugin.yaml`
(`id`, `version`, `name`, `description`, `run`, and optional `tags`, `runtime`, `binaries`, `env`,
`hosts`, `size`, `config_schema`, `demo`). `dashboard schema package` prints its schema. Use it
with `package: <id>`. Packages are found in `plugins/<id>/` next to the config file, then in
`~/.local/share/dashboard/plugins/`. A package runs in its own folder, so use absolute or `~`
paths in its config.

## Iterate

The dashboard restarts a plugin when its config entry changes. After editing the plugin's code,
change its config entry or restart the dashboard. If the plugin exits, or a request times out
(30 s) or fails to parse, it is started again at the next refresh, and the tile keeps its last
good view, marked stale.

## The protocol

Every message is one line of JSON with `"jsonrpc": "2.0"`. A request has `id`, `method` and
`params`; reply with the same `id` and `result` or `error`. A message without `id` gets no reply.

- **`initialize`** (once): `params: { contract_version, host, config }`. Reply `{ manifest }`:
  `id`, `name`, `contract_version: "0.1"`, `refresh: { mode: "poll", interval_ms,
  min_interval_ms }`, `layout: { size, priority, min_size }`, `commands: []`, and an optional
  `config_schema` (JSON Schema). Sizes: `small`, `medium`, `large`, `wide`, `tall`.
- **`view/render`** (every interval, never overlapping): no params. Reply
  `{ status, summary, root }`. `summary` is one line for the tile header and tiny tiles; `root`
  is a widget.
- **`log`** (plugin to host, a notification): `{ level: error|warn|info|debug, message }`.
- **`shutdown`**: reply `null`. **`exit`** (notification): stop.

Error codes: `-32001` something outside is unavailable (CLI missing, not logged in, API down),
`-32002` invalid config, `-32601` unknown method, `-32603` a bug. The tile shows the message.

Not built yet, so don't rely on them: push mode (`view/update`), running declared commands,
`run_command` actions, and clicks sent back to the plugin.

`dashboard schema manifest`, `view` and `initialize-params` print the JSON Schemas.

## Widgets

Every widget has a `type`. Leave out any optional field. Run `dashboard schema view` for every
field.

| type | Main fields |
|---|---|
| `text` | `text`, `markdown` (bold, code) |
| `stat` | `label`, `value` (a string), `unit`, `delta`, `status` |
| `gauge` | `label`, `percent` 0–100, `marker`, `thresholds {warn, error}`, `caption` |
| `series` | `label`, `unit`, `series: [{ name, points: [[ms, value], …] }]`, `style` line/area/bars, `color` accent/heat/cool/sunset, `format` plain/bytes/count, `y_min`, `y_max`, `fit`, `mirrored`, `id`, `history` |
| `meter_grid` | `label`, `meters: [{ label, percent }]`, `color` |
| `bars` | `label`, `bars: [{ label, value, status }]`, `format`, `unit`, `max` |
| `pie` | `label`, `slices: [{ label, value, status }]`, `donut`, `format`, `unit`, `color` |
| `display` | `text` as large as fits, `label`, `caption`, `unit`, `status` |
| `clock_face` | `hour`, `minute`, `second`, `label`, `caption`, `status` |
| `list` | `items: [{ title, subtitle, status, tags, action }]`, `empty_text` |
| `table` | `columns: [{ key, label, priority }]`, `rows: [{ cells: { key: text, number or widget }, status, action }]` |
| `log` | `label`, `lines: [{ text, level, time }]`, newest last |
| `uptime` | `checks: [{ name, status, uptime, latency_ms, detail, history: [status…], url }]` |
| `image` | `path` (a file on the dashboard's machine), `caption` |
| `row` / `column` | `children: [widgets]` (a row stacks when narrow) |

A table cell is text, a number, or a `stat`, `gauge`, `pie`, `display` or `clock_face` widget.
`action` is `{ "type": "open_url", "url": "https://…" }`. Chart times are unix **milliseconds**.
Widgets carry no colors of their own; some parts take a theme token with `tone`.

**Charts with host history:** give a `series` an `id`, and each render sends only the new points.
The host keeps the last `history` points (default 300) per series name, so the plugin can hold
no state.
