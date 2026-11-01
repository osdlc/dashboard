# Command line conventions

The `dashboard` command follows the osdlc CLI standard (`standards/cli-interface.md` in
osdlc/osdlc). The shared parts live in `crates/cli-kit`, which knows nothing about the dashboard,
so it can move into a shared `cli-kit` crate once another Rust tool needs it.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Done |
| 1 | The command failed against the state it found (a bad config file, no server running) |
| 2 | The command can't run as typed: an unknown flag or command, a bad value, `--json` where there is none |
| 22 | A program it needs is not installed (`plugin-check` with a plugin program that doesn't exist) |
| 23 | A service refused the caller or gave an unusable answer |
| 24 | There is no repository or remote to work on |
| 25 | A service could not be reached, or limited the caller |

The code uses the named constants in `dashboard_cli_kit::exit`. A failure that has its own code is
a `dashboard_cli_kit::Failure` somewhere in the error chain; anything else exits 1.

## Errors and warnings

stdout carries the answer. Everything else goes to stderr:

```text
dashboard: no web server is running. Start one with `dashboard serve`.
dashboard: warning: skipped a plugin package: ...
```

An error is one line, the message then the hint. Terminal control characters are removed from it,
since it may quote a config file or a plugin.

## `--json`

`--json` prints one JSON document on stdout. It is an object that starts with `schemaVersion` and
`kind`:

| Command | `kind` | Fields |
|---|---|---|
| `config` | `config` | `path` (the file settings are saved to), `exists`, `layers` (`kind`, `path`, lowest first), `untrustedProject`, `skippedPackages` |
| `status` | `status` | `running`, and `url`, `pid` when running |
| `stop` | `stop` | `stopped`, and `url`, `pid` when one was stopped |
| `open` | `open` | `url` (the sign-in link), `pid` |
| `widgets` | `widgets` | `entries`: full catalog entries (`dashboard schema catalog-entry`) |
| `plugin-check` | `plugin-check` | `plugin`, `manifest`, `renders`, `view`, `findings` |

On failure it prints an error document instead, and the error line on stderr as usual:

```json
{ "schemaVersion": 1, "kind": "error", "code": "usage", "message": "...", "exit": 2, "hint": "..." }
```

`tui` and `serve` run until stopped and `schema` already prints JSON Schema, so they refuse
`--json` with exit 2. `plugin`, `dashboards`, `import`, `export` and `trust` have no JSON form yet
and refuse it the same way.

## Signing in to the web view

`dashboard serve` makes a session token for each run. Every `/api` request must carry it
(`Authorization: Bearer`, or the WebSocket subprotocol `bearer.<token>`), must name a loopback
`Host`, and a browser request must come from the page's own `Origin`.

The session token never goes in a URL, where it would end up in browser history and in the browser
opener's process list. The link the server prints and opens carries a boot token instead
(`?boot=…`). The page sends it once to `POST /api/session` and gets the session token, which it
keeps in the tab's sessionStorage. A boot token works once and expires a minute after it is shown.

For another tab, or when the link expired, `dashboard open` asks the running server for a new
link. It reads the server's record (pid, URL, token; readable only by you) from the runtime
directory. `serve --detach` always writes that record; a foreground `serve` writes it when no other
server has.

## Saving settings from the web view and TUI

Settings that the web view or TUI change (theme, screens) are written back with yaml-splice, so
comments stay, and with etag-write's compare-and-swap. Each settings update carries the etag of
the file settings are saved to (the saved dashboard in use, else `config.yaml`); a change sends back the etag it was made on. If the file changed since (you edited it
by hand), nothing is saved and the view shows: "… changed since this view read it, so nothing was
saved". The view then reloads the file as it is.
