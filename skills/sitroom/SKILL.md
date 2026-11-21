---
name: sitroom
description: Install, run and configure sitroom, the osdlc developer dashboard: a terminal UI and local web view of system stats, pull requests, issues, logs, uptime checks, AI agent usage and plugin tiles, set up in YAML files. Use when someone wants to run sitroom, add or change a tile or screen, connect a tracker, or find out why a tile shows an error.
---

# Sitroom

This skill covers install, launch, config, modules and troubleshooting. To write a new tile
in another language, use the `plugin` skill.

## Install

There are no prebuilt packages yet. Build from source. It needs Rust and pnpm (pnpm builds
the web view, which is embedded in the binary):

```sh
git clone https://github.com/osdlc/sitroom && cd sitroom
cargo install --path crates/cli        # puts `sitroom` in ~/.cargo/bin
```

`cargo build --profile small` makes the smallest binary. Check the install with
`sitroom --help`.

## Launch

| Command | Does |
|---|---|
| `sitroom tui` | The terminal UI. `q` quits, `?` lists keys |
| `sitroom serve` | The web view on 127.0.0.1:7878, opened in the browser. Runs until stopped |
| `sitroom serve -d` | The same in the background. Returns once it is listening |
| `sitroom status` / `sitroom stop` | Show or stop the background server |
| `sitroom open` | Sign another browser tab in. `--no-open` prints the link instead |
| `sitroom serve --port 7900` | Another port |
| `sitroom --config path.yaml tui` | Use another config file |
| `sitroom --dashboard work serve` | Lay a saved dashboard over the config |

When you start it for someone, run `sitroom serve -d --no-open` and give them the printed
`http://127.0.0.1:7878/?boot=…` link. The link signs in one tab, works once, and expires after
a minute. If it expired, run `sitroom open --no-open` for a new one.

With no config file sitroom shows a demo of eight screens. It is `examples/demo.yaml` in
the repository; copy it as a starting point.

Add `?mode=ambient` to the web page's URL for ambient mode: no controls, large type, screens
rotating.

**Desktop app.** `apps/desktop` builds `sitroom-desktop`, a separate binary that shows the
same web view in a window. Run it from a checkout:
`cargo run -p sitroom-desktop -- --ambient`, `--wallpaper` or `--screensaver`. It uses the
same config file as `sitroom`. See `docs/desktop.md`.

**Keys.** Press `?` for the full list. The main ones:

- `1`–`9`: screens. `Tab` (TUI) or `[` `]` (web) also switch.
- arrows or `h` `j` `k` `l`: move between tiles. `Enter` goes into a tile, `Esc` steps back.
- `s`: settings. In the TUI, `e` inside settings opens the file in `$VISUAL` / `$EDITOR`.
- `e` (web): the screen editor. Its "Add component" list adds a module and a tile for you.
- `r`: pause or resume screen rotation. `d` (web): compact spacing.
- double-click (web): a tile fullscreen.

## Configure

The file is `~/.config/sitroom/config.yaml` (`%APPDATA%\sitroom\config.yaml` on Windows),
or `$SITROOM_MACHINE_CONFIG`, or `--config`. Changes apply while sitroom runs: only
modules whose settings changed restart. If the file has an error, the previous config stays in
effect and the error shows in the settings screen.

```yaml
theme: auto                      # auto | light | dark
color_theme: default             # default | contrast | paper | ember, or your own under color_themes:
modules:                         # keyed by tile ID
  cpu:
    refresh: 250ms               # override the module's suggested period
    size: large                  # small 3x1 | medium 4x2 | large 6x3 | wide 8x2 | tall 4x4
  processes:
    enabled: false
  api-log:                       # a second tile from a built-in: a new ID and builtin:
    builtin: log
    config: { path: ~/code/api/log/dev.log, lines: 300, match: error }
  ci:                            # a plugin: any program that speaks the protocol
    plugin: [python3, plugins/ci.py]   # runs in this file's folder
  clock-py:                      # an installed plugin package
    package: clock-py
  jira:
    demo: true                   # show made-up data; runs nothing
screens:                         # omit to show every module on one screen
  - name: main
    tiles:
      - { module: cpu, size: wide }
      - { module: api-log }
rotate:                          # optional
  interval: 30s
  screens: [main]                # default: all
  resume_after: 2m               # idle time after input before rotation resumes
tui:
  fps: 4                         # 1 to 60
```

To edit the config, read it first, change only the keys involved and keep comments. Then run
`sitroom config` to check it. `sitroom schema config` prints a JSON Schema for editor
completion.

**Secrets never go in the file.** Modules that need a token read an environment variable
(`token_env`, with a default per module) or run `token_command` and use what it prints, for
example `token_command: [op, read, "op://dev/linear/token"]`. If someone pastes a token into
the chat, don't write it into the config. Set up `token_env` or `token_command` instead.

**Saved dashboards** live in `~/.config/sitroom/dashboards/<name>.yaml`, with the same keys.
`sitroom dashboards list`, `sitroom import <file|url>` and `sitroom export NAME [FILE]`
manage them. A `.sitroom.yaml` in a repository adds its modules and screens when the
sitroom runs inside it, but only after `sitroom trust`, and again after every change to it.
It can run programs, so read it before you trust it. `docs/dashboards.md` explains the merge.

## Modules

A module runs only when the config names it, under `modules:` or as a tile in a screen.
Don't guess a module's settings. `sitroom widgets [words] [--tag t]` lists what can be added,
and `sitroom widgets <id> --json` shows each entry's `config_schema` and what it needs.

**No setup needed:** `cpu`, `memory`, `network`, `disk`, `processes`, `process`, `clock`, and
`github-prs` / `github-issues` through the `gh` CLI's login
(`config: { limit: 5, filter: ["--owner=my-org"] }` narrows them).

**Data without a plugin:** `http` fetches a URL on each refresh and `file` watches a file. Both
show a view in the plugin JSON, values picked out of any JSON with `fields`, or text.

**Common ones with settings:**

| ID | Shows | Setup |
|---|---|---|
| `log` | Tail of a file | `config: { path, lines, match }` |
| `git` | Branch, ahead/behind and uncommitted changes per repository | `config: { repos: [~/dev/api] }` |
| `uptime` | curl checks, a status strip, response time | `config: { checks: [{ name, url }, { name, curl: '…', expect: [200], slow_ms: 800 }] }` |
| `jira` | Your Jira issues | `config: { url, email, sections }`, token `$JIRA_API_TOKEN` |
| `linear` | Your Linear issues | token `$LINEAR_API_KEY` |
| `forgejo` | Forgejo / Gitea / Codeberg issues and PRs | `config: { url, sections }`, token `$FORGEJO_TOKEN` |
| `codex`, `copilot`, `cursor`, `gemini`, … | AI agent usage and limits | Each uses the tool's own login or local files |

The full list, by bundle, is in `docs/modules.md`. `cswap` (Claude Code accounts) is a bundled
plugin that runs when the `cswap` tool is installed.

**Plugin packages:** `sitroom plugin install <id|file|url|folder>` installs one. It checks the
sha256, shows what the package runs and needs, and asks first. `plugin list`, `plugin update`
and `plugin remove` manage them. Add one with `package: <id>`. See `docs/marketplace.md`.

## Troubleshoot

- **A tile shows an error:** its text is the module's error. Typical causes are a missing CLI
  (`gh`, `codex`, `curl`), a missing token variable, or a bad `config:` value. A tile marked
  **stale** shows its last good data after a failed refresh.
- **`sitroom config`:** lists the config files in use, in order, and checks them without
  starting anything. It also reports an untrusted `.sitroom.yaml` (skipped, not an error) and
  a `package:` that isn't installed.
- **Settings screen (`s`):** the config path, any config error, and each module's refresh period.
- **The web page is signed out:** the link was used or expired. Run `sitroom open`.
- **Logs:** a foreground `sitroom serve` logs to stderr, with each plugin's stderr. `serve -d`
  writes to a `serve.log` file and prints its path if the server fails to start. The TUI keeps
  logs off the screen; use `serve` to see them.
- **`--json`:** `config`, `status`, `stop`, `open`, `widgets` and `plugin-check` can print one JSON
  document for scripts.
