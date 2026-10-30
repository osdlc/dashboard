# dashboard

Modular developer dashboard with a terminal UI and a web UI. See [docs/](docs/) for the design.

Requires Rust and pnpm. `cargo build` also builds the Vue app in `web/` (only when its files change) and embeds it in the binary. Set `DASHBOARD_SKIP_UI_BUILD=1` to skip that.

## Run

```sh
cargo run -p dashboard -- tui      # terminal UI (q to quit)
cargo run -p dashboard -- serve    # web UI: opens http://127.0.0.1:7878/?token=… in the browser (--no-open to skip)
cargo run -p dashboard -- serve -d # the same in the background; `dashboard status` / `dashboard stop`
```

## Demo

```sh
cargo run -p dashboard -- serve --config examples/demo.yaml   # or: tui --config examples/demo.yaml
```

Eight screens (`1`–`8` or `[` `]` to switch) using every tile size and widget style, filled with made-up animated data. Edit `examples/demo.yaml` while it runs to try other styles, colors and sizes; the file's header lists the options.

## Config

Optional. Lives at `~/.config/dashboard/config.yaml` (override with `--config <path>` or `$DASHBOARD_MACHINE_CONFIG`).

```yaml
theme: auto            # auto | light | dark
modules:
  cpu:
    size: large
```

Built-in modules: `cpu` (every 500ms), `memory` (1s), `network` (500ms), `disk` (2s), `processes` (2s), `github-prs` and `github-issues` (1m, via the `gh` CLI and its login). `cswap` is a bundled stdio plugin. A module runs only when the config names it, under `modules:` or as a tile in a screen. Each can be sized, refreshed and turned off on its own. To show a built-in more than once with different settings, give it a new ID and `builtin: <module>`:

```yaml
modules:
  cpu:
    refresh: 250ms     # overrides the module's suggested period
  disk:
    enabled: false
  processes:
    config:
      rows: 8
  syslog:              # tail a log file
    builtin: log
    config: { path: /var/log/system.log, lines: 300, match: error }
  github-prs:
    config:
      limit: 5
      filter: ["--owner=my-org"]   # extra `gh search` flags
```

### Demo mode

Add `demo: true` to any module's entry (built-in, bundled plugin or package) to show its demo view instead of running it. The demo view is the made-up data from the module's catalog entry (`dashboard widgets` lists them). Nothing runs: no process, no network calls, no token needed. The tile is tagged `demo` in the TUI and web. Use it to try a layout, take screenshots, or show a module you have no account for.

```yaml
modules:
  jira:
    demo: true         # sits beside refresh and size, not inside config:
  github-prs:
    demo: true
```

A plugin given only as a command (`plugin: [...]`) has no catalog entry, so its demo tile just says there is no demo view. Remove the line (or set `demo: false`) to run the module for real.

### Issue trackers

`jira`, `linear` and `forgejo` (also works with Gitea, e.g. Codeberg) run when listed under `modules`. API tokens never go in the config file: each module reads an environment variable (`token_env`, with the defaults below) or runs `token_command` and uses what it prints.

```yaml
modules:
  linear:                      # token: $LINEAR_API_KEY
    config: { sections: [assigned, created] }
  jira:                        # token: $JIRA_API_TOKEN
    config:
      url: https://acme.atlassian.net
      email: me@acme.com       # Jira Cloud; for Server / Data Center use `auth: bearer` instead
      sections:
        - { label: Mine, jql: "assignee = currentUser() AND statusCategory != Done ORDER BY updated DESC" }
  forgejo:                     # token: $FORGEJO_TOKEN
    config:
      url: https://codeberg.org
      sections: [review_requested, my_pulls, assigned, created]
      token_command: [op, read, "op://dev/codeberg/token"]   # instead of an env var
```

### AI agent usage

How much of each agent's limits you have used, with a marker for where an even pace would be:

- `cswap`: Claude Code accounts. Runs automatically when the `cswap` tool is installed.
- `codex`: OpenAI Codex limits, tokens per day and all-time tokens, live from `codex app-server` (Codex's own login; experimental). If that fails it reads Codex's session logs in `~/.codex/sessions`, which are as of your last Codex request. `source: logs` skips the live call.
- `copilot`: GitHub Copilot's monthly premium requests, using the `gh` CLI's login. It calls the same internal endpoint Copilot's own clients use, so it may change without notice.

```yaml
modules:
  codex: {}                    # or { config: { home: ~/.codex } }
  copilot: {}
```

### Uptime checks

`uptime` runs `curl` for each check and shows a strip of recent results, the uptime % and the response time. A check is a `url`, or a full `curl` command run with `sh` (so `$VARS` expand; keep tokens in the environment).

```yaml
modules:
  uptime:
    refresh: 30s
    config:
      timeout: 10                # seconds per check
      checks:
        - { name: site, url: https://example.com, slow_ms: 800 }   # slower than slow_ms is a warning
        - name: api health
          curl: 'curl -H "Authorization: Bearer $API_TOKEN" https://api.example.com/health'
          expect: [200, 204]     # default: any 2xx or 3xx
```

GitHub uses the `gh` CLI's login (`github-prs`, `github-issues`), so it needs no token setting.

Press `s` (TUI or web) for the settings screen: the config path, any config error, the theme (changeable there; written into the file with your comments kept), rotation, and each module's refresh period. In the TUI, `e` in settings opens the file in `$VISUAL` / `$EDITOR`.

Changes to the file apply while the dashboard runs. Only modules whose settings changed restart. If the file has an error, the previous config stays in effect and the error is shown in the settings screen.

- `dashboard config` shows the path in use and checks the file.
- `dashboard schema config` prints a JSON Schema for editor completion (yaml-language-server).
- `dashboard widgets [words] [--tag t]` lists what can be added: built-in modules, bundled plugins
  and installed plugin packages (see [docs/plugins.md](docs/plugins.md)).

## Develop the web UI with live reload

```sh
cargo run -p dashboard -- serve --dev-origin http://localhost:5173   # terminal 1
cd web && pnpm dev                                                   # terminal 2
```

Open the printed `http://localhost:5173/?token=…` link.

After changing the Rust contract types, regenerate the TypeScript types with `cd web && pnpm gen:types`.

## Plugins

`plugins/cswap` is a stdio plugin (JSON-RPC, one message per line). Try it directly:

```sh
cargo build -p dashboard-cswap
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"view/render"}' | ./target/debug/dashboard-cswap
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT), at your option.
