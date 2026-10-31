# Changelog

The dashboard is pre-1.0: the config format, the module contract and the CLI can still change
between minor versions. 0.4.0 is the first public release. The earlier versions were not
released on their own; they record the stages the project was built in.

## 0.6.0

- **Catalog in the web editor:** the add list is a searchable catalog (built-ins, installed
  packages and a cached marketplace index) with tag filters. Picking an entry shows its demo view
  and the settings it needs.
- **Keyboard focus** on tiles and their items in the TUI and the web view; Enter opens links.
- **Marketplace, local first:** `dashboard plugin install`, `list`, `update`, `remove` and `pack`.
- **Saved dashboards and config layers:** `--dashboard`, `dashboards list`, `import`, `export`,
  and a project `.dashboard.yaml`. A project file must be trusted with `dashboard trust` before it
  is used. `trust` and `import` list the programs a config would run.
- **Web dashboard switcher,** and config layers shown in settings.
- **CLI conventions:**
  - fixed exit codes (2 usage, 22 missing tool, 23–25 service errors);
  - a single `dashboard: message` error line with a hint;
  - `--json` documents for every command.
- **Safer config writes:**
  - writes from the web view and the TUI are compare-and-swap, so a change based on an older file
    is refused;
  - the trust list and the serve record are written atomically.
- **Web sign-in:** a one-time boot token in the opened URL is traded for a session token. The
  server checks the Host header as well as the Origin.
- **Windows support:**
  - plugin programs are resolved through PATHEXT, `.cmd` shims and `py -3`;
  - process trees end with job objects;
  - cmdrunner and codex work on Windows;
  - `docs/windows.md` covers what differs.

## 0.5.0

A large set of widgets, most of them built in and grouped in feature-gated bundles.

- **System:** gpu, battery, textfile, cmdrunner, security.
- **Dev:** git (branch, ahead/behind, changes per repository).
- **Ops:**
  - built in: docker, kubernetes, ipinfo, Pi-hole, Transmission;
  - Python plugin packages: Datadog, New Relic, PagerDuty, Opsgenie, Sentry, Rollbar, Airbrake,
    UptimeRobot, DigitalOcean, Zendesk and Healthchecks.io.
- **Fun:** clock, hackernews, feedreader (RSS and Atom), subreddit, dev.to, mempool, crypto prices,
  sports scores.
- **Personal:** weather (Open-Meteo), calendar (ICS feeds with recurrence rules), todo, lunar,
  org-agenda, markdown-tasks (Obsidian Tasks and Dataview formats), todo-txt, taskwarrior, sdlc.
- **Agents:** cursor, antigravity, gemini, opencode and pi usage.
- **Dev plugin packages:** GitLab, GitLab to-dos, Gerrit, Azure DevOps, CircleCI, Buildkite,
  Jenkins, Asana, Todoist and Trello.
- **Shared token helper:** token-API plugins share one helper (`apitoken.py`). It runs
  `token_command` once with no stdin and never puts command output in errors.
- **Native and plugin versions side by side:** clock, textfile, git, docker, hackernews, weather,
  feedreader and battery also come as TypeScript and Python plugins in `plugins/ext`. Tests check
  that all three give the same view. There is a harness in `bench/` to compare them.
- **Example configs** in `examples/widgets/`, and `examples/try.yaml` with every bundle.

## 0.4.0

First public release.

- **Catalog entries:** every built-in module has metadata (description, tags, settings) and a demo
  view. `dashboard widgets` lists them.
- **Plugin packages** (`dashboard-plugin.yaml`): `package:` in the config runs one, and
  `dashboard config` reports missing or broken packages.
- **Only the modules the config names run;** built-ins are no longer on by default.
- **TypeScript and Python plugin SDKs,** with contract types generated from the Rust types.
  `dashboard plugin new --lang ts|py` scaffolds a plugin.
- **Feature-gated bundles:** built-in modules are split into bundle crates, so a binary can
  include only what it needs.
- **`demo: true`** on a module shows its catalog demo view instead of running it.
- **`serve`** opens the browser by default; `serve --detach` runs it in the background, with `stop`
  and `status`.
- **License:** MIT OR Apache-2.0.

## 0.3.0

- **Stdio plugins** (JSON-RPC 2.0, newline-delimited); cswap is bundled as
  `dashboard plugin cswap`.
- **GitHub PR and issue modules** (via `gh`); Jira, Linear and Forgejo modules. Tokens come from
  env vars or a `token_command`.
- **A demo module** with `examples/demo.yaml`, shown by default when there is no config file.
- **TUI:**
  - mouse support;
  - double-click a tile for fullscreen;
  - the page scrolls when tiles don't fit.
- **Web:**
  - screen editor (create, rename and delete screens; add, resize, reorder and remove tiles);
  - smooth transitions;
  - compact density.
- **Hover** outlines the tile and the clickable row in both renderers.
- **Log widget,** and a log module that tails a file.
- **Uptime checks** with a status strip.
- **Agent usage:** codex (live limits from the codex app-server) and copilot.
- **Builds and tooling:**
  - a size-optimized build profile (`cargo build --profile small`);
  - `dashboard plugin-check` runs a plugin like the host does and checks what it sends;
  - example plugins in several languages.

## 0.2.0

- **Refresh:** config can override a module's refresh period, down to the module's minimum.
- **Multiple screens** with tabs, and a help screen, in the TUI and the web view.
- **The host keeps series history;** modules send only new samples.
- **Richer charts:**
  - area, heat and mirrored graphs and braille charts;
  - meter grid and bar list.
- **The system tile split** into cpu, memory, network, disk and processes modules.
- **Automatic screen rotation.**
- **Hot reload:** config changes restart only the modules that changed.
- **Settings screen;** the theme can be changed from the UI, and the config opened in `$EDITOR`.

## 0.1.0

- **The module contract:**
  - typed views of semantic widgets (stat, gauge, series, list, table, text and more);
  - declared commands that only run after the user approves them.
- **The host,** a sysperf module, a terminal UI (ratatui) and a web view (Vue, embedded in the
  binary).
- **YAML config;** light, dark and auto themes.
- **cswap as the first plugin.**
