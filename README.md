# dashboard

Modular developer dashboard with a terminal UI and a web UI. See [docs/](docs/) for the design.

Requires Rust and pnpm. `cargo build` also builds the Vue app in `web/` (only when its files change) and embeds it in the binary. Set `DASHBOARD_SKIP_UI_BUILD=1` to skip that.

## Run

```sh
cargo run -p dashboard -- tui      # terminal UI (q to quit)
cargo run -p dashboard -- serve    # web UI: opens http://127.0.0.1:7878/?boot=… in the browser (--no-open to skip)
cargo run -p dashboard -- serve -d # the same in the background; `dashboard status` / `dashboard stop`
cargo run -p dashboard -- open     # sign another browser tab in to the running server
cargo run -p dashboard-desktop     # desktop app: a window, --wallpaper or --screensaver (docs/desktop.md)
```

Add `mode=ambient` to the web page's URL (`?mode=ambient`) for ambient mode: no controls, large type, screens rotating.

### Keys

Both views work from the keyboard. Press `?` for the full list.

| Key | What it does |
| --- | --- |
| `←` `↑` `↓` `→` or `h` `j` `k` `l` | Move to the nearest tile that way. The focused tile gets an accent border. |
| `Enter` | Go into the focused tile. Lists, table rows, uptime checks and bars can be picked one by one. |
| `↑` `↓` / `j` `k` (inside a tile) | Move between items. The picked item is highlighted. |
| `Enter` (on an item) | Run the item's action: open its link (web: new tab; TUI: default browser). |
| `Esc` | Step back: out of the item, then out of fullscreen, then focus off. Also closes help and settings. |
| `1`–`9`, `Tab` (TUI), `[` `]` (web) | Switch screens. Focus is cleared. |
| `PgUp` `PgDn` `Space` `Home` `End` | Scroll a page that is taller than the window or terminal. |

Focus stays on the same tile and item while data updates; if the item goes away, the nearest one is picked.
Clicking a tile focuses it. Keyboard focus is off in ambient mode (`?mode=ambient`) and while help,
settings or the screen editor are open.

## Demo

```sh
cargo run -p dashboard -- serve --config examples/demo.yaml   # or: tui --config examples/demo.yaml
```

Eight screens (`1`–`8` or `[` `]` to switch) using every tile size and widget style, filled with made-up animated data. Edit `examples/demo.yaml` while it runs to try other styles, colors and sizes; the file's header lists the options.

## Config

Optional. Lives at `~/.config/dashboard/config.yaml` (`%APPDATA%\dashboard\config.yaml` on Windows; override with `--config <path>` or `$DASHBOARD_MACHINE_CONFIG`).

```yaml
theme: auto            # auto | light | dark
color_theme: default   # default | contrast | paper | ember, or one of your own (see Themes)
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

### Themes

`theme:` picks light or dark (`auto` follows the OS in the web view and the terminal's background in
the TUI). `color_theme:` picks the colors. Every color theme has a light and a dark variant, and the
web view and the TUI take every color from the same named tokens, so they match. Both settings
screens can switch theme and colors; the choice is saved to the config file and shows at once.

Built-in color themes:

| Name | Look |
|---|---|
| `default` | The original: cool grays with a blue accent. |
| `contrast` | Black on white and white on black, strong borders and saturated colors. |
| `paper` | Warm off-white and soft charcoal, quiet ink colors. |
| `ember` | Cream and coal with a burnt-orange accent. |

Tokens:

| Token | Used for |
|---|---|
| `bg`, `surface`, `surface-2` | Page, tiles and panels, hovered rows and buttons |
| `text`, `muted`, `border`, `track` | Body text, quiet text, borders, the empty part of meters and gauges |
| `accent`, `accent-soft`, `on-accent` | Highlights; the keyboard-selected item; text on an accent button |
| `ok`, `warn`, `error`, `info` | Status colors |
| `series-1` … `series-6` | Lines, bars and slices, in order |
| `heat-*`, `cool-*`, `sunset-*` (`-low`, `-mid`, `-high`) | The stops of each color scale |

To make your own, start from a built-in and change some tokens, per variant. Values are hex colors
(`#rrggbb` or `#rgb`); an unknown token or a value that is not a color is an error that names it.

```yaml
color_theme: mine
color_themes:
  mine:
    base: paper          # a built-in; default: default
    light:
      accent: "#2f6f4f"
      series-1: "#2f6f4f"
    dark:
      accent: "#8fd0a8"
```

Widgets never carry colors of their own: a module can name a token with `tone:` (see
[docs/contract.md](docs/contract.md)). The TUI leaves the terminal's own background and text
color alone, darkens (or lightens) colors drawn as text until they read well, and uses the nearest
of 256 or 16 colors when the terminal has no truecolor (`COLORTERM` is not `truecolor`/`24bit`).

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

### Git repositories

`git` shows each repository's branch, commits ahead of and behind its upstream, and uncommitted changes. It runs `git status` in each one; nothing is fetched.

```yaml
modules:
  git:
    config: { repos: [~/dev/api, ~/dev/web] }
```

### AI agent usage

How much of each agent's limits you have used, with a marker for where an even pace would be:

- `cswap`: Claude Code accounts. Runs automatically when the `cswap` tool is installed.
- `codex`: OpenAI Codex limits, tokens per day and all-time tokens, live from `codex app-server` (Codex's own login; experimental). If that fails it reads Codex's session logs in `~/.codex/sessions`, which are as of your last Codex request. `source: logs` skips the live call.
- `copilot`: GitHub Copilot's monthly premium requests, using the `gh` CLI's login. It calls the same internal endpoint Copilot's own clients use, so it may change without notice.
- `cursor`: Cursor's included usage and on-demand spend this billing cycle. It reads the Cursor app's login from its local database (with `sqlite3`) and calls the endpoint Cursor's own dashboard uses. A free or canceled account shows its plan and no gauge. Paid plans are untested live.
- `antigravity`: Google Antigravity's model quotas, asked from the app's local language server. It only works while Antigravity is open; otherwise the tile says "not running". Untested live.
- `gemini`: Gemini CLI requests against its daily quota (an estimate: replies saved since midnight Pacific time; the per-day bars use UTC days), and tokens per day, from the chats in `~/.gemini/tmp`. Set `daily_limit` to your plan's limit (default 1000).
- `opencode`: OpenCode tokens and cost per day, from its database `~/.local/share/opencode/opencode.db` (read with `sqlite3`), or the message files older versions wrote in `storage/`.
- `pi`: Pi tokens and cost per day, from its session logs in `~/.pi/agent/sessions`.

The last three read local files only. Their days are UTC days, so for a US user "today" starts in the afternoon or evening. None of these modules shows or stores a token.

```yaml
modules:
  codex: {}                    # or { config: { home: ~/.codex } }
  copilot: {}
  cursor: {}
  antigravity: {}
  gemini: { config: { daily_limit: 1000 } }
  opencode: {}
  pi: {}
```

`examples/widgets/w-agents.yaml` shows them all, most in demo mode.

### Uptime checks

`uptime` runs `curl` for each check and shows a strip of recent results, the uptime % and the response time. A check is a `url`, or a full `curl` command run with `sh` (so `$VARS` expand; keep tokens in the environment). On Windows there is no shell: the line is split into words and `curl.exe` runs directly ([docs/windows.md](docs/windows.md)).

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

### Clock

`clock` shows the time in one or more time zones as a row of stats, with the date under each. It uses the system's time zone database and makes no network calls.

```yaml
modules:
  clock:
    config:
      zones: [local, UTC, America/New_York, { zone: Asia/Tokyo, label: Tokyo office }]
      hour12: false              # true: 2:05 PM
      seconds: false             # true: also refreshes every second
```

GitHub uses the `gh` CLI's login (`github-prs`, `github-issues`), so it needs no token setting.

Press `s` (TUI or web) for the settings screen: the config path, any config error, the theme (changeable there; written into the file with your comments kept), rotation, and each module's refresh period. In the TUI, `e` in settings opens the file in `$VISUAL` / `$EDITOR`.

Changes to the file apply while the dashboard runs. Only modules whose settings changed restart. If the file has an error, the previous config stays in effect and the error is shown in the settings screen.

### Dashboards and project files

Keep named dashboards in `~/.config/dashboard/dashboards/<name>.yaml` and open one with
`dashboard --dashboard work serve`, or switch in the web view. A `.dashboard.yaml` in a
repository adds that repository's modules and screens when you run the dashboard inside it.
A project file can run programs, so it is only used after you read it and run
`dashboard trust`, and again after every change to it.
`dashboard dashboards list`, `dashboard import <file|url>` and `dashboard export NAME [FILE]`
manage saved dashboards. See [docs/dashboards.md](docs/dashboards.md) for how the files merge.

- `dashboard config` lists the config files in use, in order, and checks them.
- `dashboard schema config` prints a JSON Schema for editor completion (yaml-language-server).
- `dashboard widgets [words] [--tag t]` lists what can be added: built-in modules, bundled plugins
  and installed plugin packages (see [docs/plugins.md](docs/plugins.md)).
- `dashboard plugin install <id|file|url|folder>` installs a plugin package, checking its
  sha256 and asking first; `plugin list`, `update` and `remove` too (see
  [docs/marketplace.md](docs/marketplace.md)).

In the web view, `e` opens the screen editor. Its "Add component" list searches the same catalog,
with tag filters. Pick an entry to see its demo view, what it needs, and a settings form made from
its config schema. **Add** writes its `modules:` entry and a tile on the screen you are editing
into the config file, keeping your comments, and the module starts.

## Develop the web UI with live reload

```sh
cargo run -p dashboard -- serve --dev-origin http://localhost:5173   # terminal 1
cd web && pnpm dev                                                   # terminal 2
```

Open the printed `http://localhost:5173/?boot=…` link within a minute. `dashboard open` makes a new one,
but for port 7878; change its start to `http://localhost:5173` by hand.

After changing the Rust contract types, regenerate the TypeScript types with `cd web && pnpm gen:types`.

`cd web && pnpm test` runs the web view's unit tests (keyboard navigation) with `node --test`.

## Plugins

`plugins/cswap` is a stdio plugin (JSON-RPC, one message per line). Try it directly:

```sh
cargo build -p dashboard-cswap
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"view/render"}' | ./target/debug/dashboard-cswap
```

## Windows

The CLI cross-compiles for Windows; it has not been run there yet. See [docs/windows.md](docs/windows.md) for how to build it, its sizes, and what differs.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT), at your option.
