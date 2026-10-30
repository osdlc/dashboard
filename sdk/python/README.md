# Python plugin SDK

[`dashboard_plugin.py`](dashboard_plugin.py) is the whole SDK: one file, standard library only,
Python 3.9 or later.

## Start

```sh
dashboard plugin new --lang py my-plugin     # writes my-plugin/main.py and a copy of the SDK
dashboard plugin-check -- python3 my-plugin/main.py
```

Or copy `dashboard_plugin.py` next to your plugin and import it:

```python
from dashboard_plugin import View, serve, with_defaults, config_error, upstream_error, log, live_series
```

## What is in it

- `serve(manifest=, render=, init=None, shutdown=None)` answers the dashboard on stdin and
  stdout. `manifest` is a dict or a function of the config. `contract_version` and `commands`
  are filled in for you. `init(config)` gets the tile's `config:` block (`{}` when there is
  none). `render()` returns a `View`.
- `config_error(msg)` (code -32002) and `upstream_error(msg)` (code -32001): raise them and the
  tile shows `msg`. Any other exception becomes -32603, with the traceback on stderr.
- `with_defaults(config, defaults)` merges the config over the defaults and rejects unknown
  keys (usually typos) with a config error.
- `log(level, msg)` writes a line to the dashboard's log, not the tile.
- `live_series(id, label, {name: value}, **widget)` builds a `series` widget whose history the
  host keeps, so each render sends only the newest value. `None` sends no point.
- `TypedDict` types for the manifest, the view and every widget (`Manifest`, `View`, `Widget`,
  `StatWidget`, `TableWidget`, ...), for editors and type checkers. At run time they are plain
  dicts.

`print()` goes to stderr while `serve` runs, because stdout carries only protocol messages.

## Examples

Run any of them with `dashboard plugin-check -- python3 examples/<file>`.

| File | Shows |
|---|---|
| [`examples/hello.py`](examples/hello.py) | The smallest plugin: config with defaults, a row of stats |
| [`examples/big_files.py`](examples/big_files.py) | A stats row and a table with gauges, a config schema, a config error, `log` |
| [`examples/load.py`](examples/load.py) | A live chart with `live_series` |

The examples find the SDK one folder up; your own plugin keeps it next to the main file. They
are tested against the real host in `crates/host/tests/sdk_examples.rs`.

## The types

The types between the `GENERATED` markers come from the contract's JSON Schemas. After a
contract change, run `node sdk/gen-types.mjs` from the repository root. A test fails while they
are out of date. The protocol is described in [docs/plugins.md](../../docs/plugins.md).
