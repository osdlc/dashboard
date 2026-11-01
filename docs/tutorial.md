# Tutorial: build your own dashboard

This tutorial takes you from nothing to a dashboard of your own. You will run the demo, write a
config file, add widgets, connect one that needs a login, write a widget of your own, and keep
more than one dashboard. It takes about an hour.

Every command and config below has been run against the dashboard in this repository. If one does
not work for you, [open an issue](https://github.com/osdlc/dashboard/issues).

You need:

- [Rust](https://rustup.rs) and [pnpm](https://pnpm.io), to build the dashboard.
- Node 24 (or Bun), for the widget you write in step 5. Python 3.9 or later works too.
- The [GitHub CLI](https://cli.github.com) (`gh`), for step 4. You can skip that part.

## 1. Install and run the demo

There are no packages yet, so you build from source. `cargo install` builds the web view too,
and puts `dashboard` in `~/.cargo/bin`, which is on your `PATH` once Rust is installed.

```sh
git clone https://github.com/osdlc/dashboard
cd dashboard
cargo install --path crates/cli
```

Start the terminal view:

```sh
dashboard tui
```

You have no config file yet, so it shows the built-in demo: eight screens of made-up data. Press
`1` to `8` or `Tab` to switch screens, `?` for help and `q` to quit.

Now the web view:

```sh
dashboard serve
```

It opens `http://127.0.0.1:7878/?boot=…` in your browser. That link signs in one tab and expires
after a minute. For another tab, run `dashboard open`. Press `Ctrl-C` to stop the server.

To keep the server running in the background:

```sh
dashboard serve -d    # start it and return
dashboard status      # is it running, and where
dashboard stop        # stop it
```

The web view only listens on `127.0.0.1`, so other machines can't reach it.

## 2. Your first config file

The config is one YAML file. Ask the dashboard where it looks:

```sh
dashboard config
```

```text
/home/you/.config/dashboard/config.yaml
(no file; using the built-in demo, examples/demo.yaml)
```

On macOS and Linux it is `~/.config/dashboard/config.yaml`. On Windows it is
`%APPDATA%\dashboard\config.yaml`. `--config <path>` uses another file.

Create the file with one screen of built-in modules. Change the `repos:` path to a git repository
of yours.

```yaml
# ~/.config/dashboard/config.yaml
theme: auto            # auto | light | dark

modules:
  clock:
    config:
      zones: [local, UTC, Asia/Tokyo]
  git:
    config:
      repos: [~/code/my-project]

screens:
  - name: main
    tiles:
      - { module: clock, size: wide }
      - { module: cpu, size: medium }
      - { module: memory, size: small }
      - { module: disk, size: small }
      - { module: git, size: large }
```

- `modules:` holds each module's settings. `config:` is passed to the module as it is.
- `screens:` lists the screens, each with its tiles in order.
- A module runs only when the config names it. `cpu`, `memory` and `disk` need no settings, so
  they only appear as tiles.

Check the file:

```sh
dashboard config
```

```text
machine    /home/you/.config/dashboard/config.yaml
ok
```

Then run `dashboard tui` or `dashboard serve` again. You see your screen instead of the demo.

<!-- Screenshot: the "main" screen from this step, in the web view. To be added. -->

### Tile sizes

Screens are a grid 12 cells wide. Each size takes a fixed number of cells:

| Size | Width × height |
|---|---|
| `small` | 3 × 1 |
| `medium` | 4 × 2 |
| `large` | 6 × 3 |
| `wide` | 8 × 2 |
| `tall` | 4 × 4 |

Tiles are placed in order, left to right, and fill gaps where they fit. In the file above, `wide`
and `medium` make the first row (8 + 4), and `small`, `small` and `large` make the second
(3 + 3 + 6). Leave `size:` out to use the module's suggested size.

### Live edits and mistakes

Leave the dashboard running and edit the file. Changes apply when you save. Only modules whose
settings changed restart.

If the file has a mistake, the dashboard keeps the last good config and shows the error on the
settings screen (press `s`). `dashboard config` shows it too:

```text
dashboard: in /home/you/.config/dashboard/config.yaml: screen "main": unknown module "memroy" (available: cpu, memory, gpu, ...)
```

For completion and checks in your editor, save the config's JSON Schema next to the file and point
the first line at it. This works in editors that use yaml-language-server, such as VS Code with
the YAML extension.

```sh
dashboard schema config > ~/.config/dashboard/config.schema.json
```

```yaml
# yaml-language-server: $schema=config.schema.json
theme: auto
```

## 3. Find and add widgets

`dashboard widgets` lists everything you can add: built-in modules, bundled plugins and plugin
packages you have installed. Give it words to search for, or a tag:

```sh
dashboard widgets                 # everything
dashboard widgets weather         # every word must match the ID, name, description or tags
dashboard widgets --tag github
```

```text
weather  builtin  Weather  weather, personal, web
    Current weather and a short forecast from Open-Meteo (no key needed), for a place name or a latitude and longitude.
```

`dashboard widgets` lists every widget, and `dashboard widgets <id> --json` shows one with its
settings. The web editor's Add dialog shows each one's demo and settings too.

Add a second screen with the weather, Hacker News and a Jira tile. Put the new modules under your
`modules:` and the new screen after `main`:

```yaml
modules:
  # ... clock and git from before ...
  weather:
    config: { place: Berlin }
  jira:
    demo: true          # made-up data: no account, no token

screens:
  # ... the main screen from before ...
  - name: today
    tiles:
      - { module: weather, size: medium }
      - { module: hackernews, size: medium }
      - { module: jira, size: medium }
```

Weather and Hacker News need no account. Jira does, so this tile uses `demo: true`: it shows the
module's demo view, made-up data from its catalog entry. Nothing runs, nothing is sent, and the
tile is marked `demo`. Use it to try a layout before you set up an account. Remove the line to run
the module for real.

Press `2` (or `]` in the web view) to see the new screen.

### Add widgets in the web view

You can also add widgets without editing the file:

1. Run `dashboard serve` and press `e` to open the screen editor.
2. In **Add component**, search or pick a tag.
3. Pick a widget. You see its demo, what it needs (programs, environment variables, hosts it
   calls) and a form for its settings. Tick **Demo data only** to add it with made-up data.
4. Press **Add**. It writes the module and a tile into your config file. Your comments stay.

In the editor you can also drag tiles to reorder them, and right-click a tile to resize, move or
remove it. Press **Save** to write the screen to the file.

## 4. A widget that needs a login or a token

### GitHub, through `gh`

The GitHub modules use the GitHub CLI and its login. There is no token to set.

```sh
gh auth login      # once
gh auth status     # check it
```

Then add the tiles:

```yaml
modules:
  github-prs:
    config:
      limit: 5                     # items per section
      # filter: ["--owner=my-org"] # extra `gh search` flags
screens:
  - name: code
    tiles:
      - { module: github-prs, size: wide }
      - { module: github-issues, size: medium }
```

`github-prs` shows pull requests waiting for your review, and your own. `github-issues` shows
issues assigned to you and the ones you opened.

### Services with an API token

Tokens never go in the config file. A module that needs one reads it from an environment
variable, or runs a command you choose and uses what it prints. Linear, for example, reads
`$LINEAR_API_KEY`:

```sh
export LINEAR_API_KEY=lin_api_...   # in your shell profile, then start the dashboard
dashboard serve
```

```yaml
modules:
  linear: {}
screens:
  - name: code
    tiles:
      - { module: linear, size: medium }
```

Set the variable in the shell that starts the dashboard. Without it the tile says:

```text
no API token: set $LINEAR_API_KEY before starting the dashboard, or set token_env / token_command in the config
```

To read a different variable, set `token_env`. To get the token from a password manager, set
`token_command`, a program and its arguments. The dashboard runs it and uses what it prints:

```yaml
modules:
  linear:
    config:
      token_command: [op, read, "op://Private/Linear/credential"]   # 1Password CLI
      # or: token_env: MY_LINEAR_KEY
```

The web editor's Add dialog lists the variable each widget reads, and so does `dashboard widgets linear --json`
under `requirements`.

## 5. Your own widget

A widget you write is a plugin: a program that reads requests on stdin and writes answers on
stdout, one JSON message per line. The dashboard starts it, asks for a manifest once (its ID, name
and refresh period), then asks for a view on every refresh. You can use any language. For
TypeScript and Python there is a small SDK that handles the protocol for you.

### Make it

Make the plugin in the config folder, so the config can find it by a short path:

```sh
cd ~/.config/dashboard
dashboard plugin new --lang ts folder
```

```text
created folder
  main.ts
  dashboard-plugin.ts
  package.json

try it:
  dashboard plugin-check -- node folder/main.ts
```

`main.ts` is your plugin. `dashboard-plugin.ts` is the SDK, copied in: one file, no packages. Try
it as it is:

```sh
dashboard plugin-check -- node folder/main.ts
```

`plugin-check` starts the plugin the way the dashboard does, checks what it sends and prints the
tile as the terminal view would draw it.

### Show something real

This widget watches one folder: how many files it holds, their size, how full the disk is, and the
newest files. Replace `folder/main.ts` with:

```ts
// Folder: what is in one folder, and how full its disk is.
import { readdirSync, statSync, statfsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { configError, liveSeries, serve, withDefaults } from "./dashboard-plugin.ts";
import type { View } from "./dashboard-plugin.ts";

// Settings from the tile's `config:` block, with their defaults.
let config = { path: "~/Downloads", newest: 3 };
let dir = "";

const size = (bytes: number) =>
  bytes < 2 ** 20 ? `${Math.ceil(bytes / 2 ** 10)} KiB` : `${(bytes / 2 ** 20).toFixed(1)} MiB`;
const ago = (ms: number) => {
  const min = Math.round((Date.now() - ms) / 60000);
  if (min < 60) return `${min}m ago`;
  if (min < 1440) return `${Math.round(min / 60)}h ago`;
  return `${Math.round(min / 1440)}d ago`;
};

serve({
  manifest: {
    id: "folder",
    name: "Folder",
    refresh: { mode: "poll", interval_ms: 10000, min_interval_ms: 2000 },
    layout: { size: "medium" },
  },

  // Check the config once, at the start. A configError shows on the tile.
  init(c) {
    config = withDefaults(c, config);
    dir = config.path.replace(/^~(?=\/|$)/, homedir());
    try {
      statSync(dir);
    } catch {
      throw configError(`no such folder: ${dir}`);
    }
  },

  // Called on every refresh. Returns what the tile shows.
  render(): View {
    const files = readdirSync(dir, { withFileTypes: true })
      .filter((e) => e.isFile() && !e.name.startsWith("."))
      .map((e) => ({ name: e.name, ...statSync(join(dir, e.name)) }))
      .sort((a, b) => b.mtimeMs - a.mtimeMs);
    const total = files.reduce((sum, f) => sum + f.size, 0);

    const fs = statfsSync(dir);
    const disk = fs.blocks * fs.bsize;
    const used = disk - fs.bavail * fs.bsize;
    const percent = Math.round((used / disk) * 100);

    return {
      status: percent >= 90 ? "error" : percent >= 80 ? "warn" : "ok",
      summary: `${files.length} files · ${size(total)}`,
      root: {
        type: "column",
        children: [
          {
            type: "row",
            children: [
              { type: "stat", label: "Files", value: String(files.length) },
              { type: "stat", label: "Size", value: size(total) },
            ],
          },
          {
            type: "gauge",
            label: "Disk",
            percent,
            thresholds: { warn: 80, error: 90 },
            caption: `${Math.round(used / 2 ** 30)} of ${Math.round(disk / 2 ** 30)} GiB used`,
          },
          liveSeries({ id: "size", label: "Folder size", format: "bytes", style: "area" }, { size: total }),
          {
            type: "list",
            empty_text: "No files",
            items: files.slice(0, config.newest).map((f) => ({
              title: f.name,
              subtitle: `${size(f.size)} · ${ago(f.mtimeMs)}`,
            })),
          },
        ],
      },
    };
  },
});
```

The SDK does the protocol. You write two things:

- `init` gets the tile's `config:` block. `withDefaults` fills in the defaults and rejects keys
  it doesn't know, which are usually typos. Throw `configError` when a setting is wrong.
- `render` returns a view: a `status` (`ok`, `warn`, `error` or `unknown`, which colors the
  tile's border), a one-line `summary`, and a `root` widget.

### Compose the view

The tile is built from a few widgets. `row` puts its children side by side, `column` stacks them.
Inside are a `stat` (a label and a value), a `gauge` (a percent bar that turns yellow and red at
its thresholds), a `series` (a chart) and a `list`. This is the view the plugin sends, as JSON:

```json
{
  "status": "error",
  "summary": "3 files · 2.5 MiB",
  "root": {
    "type": "column",
    "children": [
      {
        "type": "row",
        "children": [
          { "type": "stat", "label": "Files", "value": "3" },
          { "type": "stat", "label": "Size", "value": "2.5 MiB" }
        ]
      },
      {
        "type": "gauge",
        "label": "Disk",
        "percent": 93,
        "thresholds": { "warn": 80, "error": 90 },
        "caption": "1721 of 1858 GiB used"
      },
      {
        "type": "series",
        "id": "size",
        "label": "Folder size",
        "format": "bytes",
        "style": "area",
        "series": [{ "name": "size", "points": [[1793164127820, 2580003]] }]
      },
      {
        "type": "list",
        "empty_text": "No files",
        "items": [
          { "title": "notes.txt", "subtitle": "1 KiB · 0m ago" },
          { "title": "photo.jpg", "subtitle": "176 KiB · 0m ago" },
          { "title": "report.pdf", "subtitle": "2.3 MiB · 0m ago" }
        ]
      }
    ]
  }
}
```

The chart sends only one point each time. Because the series has an `id`, the dashboard keeps
its history and draws the whole line, so the plugin keeps no state. That is what `liveSeries`
does. Points are `[unix milliseconds, value]`.

The other widgets are `text` (with optional markdown), `bars`, `meter_grid`, `table`, `log` and
`uptime`. [docs/plugins.md](https://github.com/osdlc/dashboard/blob/main/docs/plugins.md#widgets)
has an example of each.

[examples/blocks/](../examples/blocks/) has a JSON example of each widget type, and ways to
combine them: stats over a chart, a pie next to stats, widgets inside table cells.

### Check it

`--plugin-config` passes a `config:` block, as JSON:

```sh
dashboard plugin-check --plugin-config '{"path": "~/Downloads"}' -- node folder/main.ts
```

```text
plugin    node folder/main.ts
manifest  folder "Folder" · every 10s · size Medium · answered in 166 ms
render 1  Error · "3 files · 2.5 MiB" · 2 ms
render 2  Error · "3 files · 2.5 MiB" · 5 ms
render 3  Error · "3 files · 2.5 MiB" · 1 ms

┌ folder ──────────────────────────────────────────────────────────────────────┐
│Files 3                                 Size 2.5 MiB                          │
│                                                                              │
│Disk ████████████████████████████████████████▉···  93% · 1720 of 1858 GiB used│
│                                                                              │
│Folder size  2.5 MiB                                                          │
│                                                                            ⢸⣿│
│                                                                            ⢸⣿│
│                                                                            ⢸⣿│
│                                                                            ⢸⣿│
│                                                                              │
│● notes.txt                                                                   │
│  1 KiB · 0m ago                                                              │
│● photo.jpg                                                                   │
│  176 KiB · 0m ago                                                            │
│● report.pdf                                                                  │
│  2.3 MiB · 0m ago                                                            │
└──────────────────────────────────────────────────────────────────────────────┘

no problems found
```

(The status is `Error` here because that disk is 93% full.) `plugin-check` also catches mistakes
the JSON alone won't show, such as percents outside 0 to 100 or chart times in seconds instead of
milliseconds. A bad setting shows the plugin's message:

```sh
dashboard plugin-check --plugin-config '{"path": "~/nope"}' -- node folder/main.ts
```

```text
dashboard: initialize failed: no such folder: /home/you/nope (code -32002)
```

### Add it to your config

```yaml
modules:
  folder:
    plugin: [node, folder/main.ts]    # program and arguments
    config: { path: ~/Downloads, newest: 5 }
screens:
  - name: today
    tiles:
      - { module: folder, size: medium }
```

A plugin started with `plugin:` runs in the config file's folder, so `folder/main.ts` is found
next to `config.yaml`. Edit `main.ts` and the change shows at the next restart of the tile:
change its `config:` or restart the dashboard.

In Python, use `dashboard plugin new --lang py folder`. The SDK has the same parts:
`serve(manifest=..., init=..., render=...)`, `with_defaults`, `config_error` and `live_series`.
See [sdk/python](https://github.com/osdlc/dashboard/blob/main/sdk/python/README.md) and
[sdk/typescript](https://github.com/osdlc/dashboard/blob/main/sdk/typescript/README.md).

To share your widget, make it a package: its folder plus a `dashboard-plugin.yaml` with a
description, tags and a demo view. See
[docs/plugins.md](https://github.com/osdlc/dashboard/blob/main/docs/plugins.md#package-it).

## 6. More than one dashboard

### Saved dashboards

A saved dashboard is a file with the same keys as `config.yaml`, plus an optional `description:`.
Its screens replace the ones in `config.yaml`. Its modules are added to yours, so `clock` still
has the time zones you set. Write one:

```yaml
# focus.yaml
description: Heads down
screens:
  - name: focus
    tiles:
      - { module: clock, size: wide }
      - { module: github-prs, size: medium }
```

Save it and open it:

```sh
dashboard import focus.yaml          # saved as "focus"
dashboard dashboards list
dashboard --dashboard focus tui      # or: dashboard --dashboard focus serve
```

`import` checks the file, saves it in `~/.config/dashboard/dashboards/focus.yaml`, and lists what
it needs that you don't have and the programs it runs. You can also write the file in that folder
yourself. In the web view, a switcher next to the title picks a saved dashboard.

To share one, export it and import the file on the other machine:

```sh
dashboard export focus                      # print it
dashboard export focus focus.yaml --force   # write it to a file
```

`import` also takes an `https://` URL, and `--name` to save it under another name. `import` and
`export` never replace a file unless you pass `--force`.

### A dashboard for a repository

A `.dashboard.yaml` file in a repository adds that repository's modules and screens when you run
the dashboard inside it. Its screens come first.

```yaml
# my-project/.dashboard.yaml
modules:
  repo:
    builtin: git          # a second copy of a built-in, under its own ID
    config: { repos: [.] }
  todo-notes:
    builtin: textfile
    config: { path: TODO.md }
screens:
  - name: my-project
    tiles:
      - { module: repo, size: wide }
      - { module: todo-notes, size: medium }
```

`builtin:` runs a built-in module under a new ID, so you can show one module more than once with
different settings. Relative paths start in the folder of the file that sets them, here the
repository's top folder, so the dashboard shows the same thing from any folder inside it.

A project file can run programs, and it comes with any repository you clone. So it is not used
until you trust it:

```sh
dashboard config      # project ... (not trusted, not used; see `dashboard trust`)
cat .dashboard.yaml   # read it first
dashboard trust       # prints the programs it runs, and trusts the file as it is now
```

Any change to the file, yours or from a `git pull`, needs `dashboard trust` again.
`dashboard trust --remove` stops using it, and `--no-project` skips it for one run.

## 7. Next steps

### Keys

Both views work from the keyboard. Press `?` for the full list.

| Key | What it does |
|---|---|
| `1`–`9`, `Tab` (terminal), `[` `]` (web) | Switch screens |
| arrows or `h` `j` `k` `l` | Move to the nearest tile that way |
| `Enter` | Go into a tile. On a list item, open its link |
| `Esc` | Step back out |
| `s` | Settings: config files in use, errors, theme, refresh periods |
| `e` (web) | Screen editor |
| `d` (web) | Compact spacing |
| `q` (terminal) | Quit |

Add `?mode=ambient` to the web view's address for a wall display: large type, rotating screens,
nothing to click.

### Install plugin packages

Plugin packages are widgets other people wrote. `dashboard plugin install` adds one to
`~/.local/share/dashboard/plugins/`. It checks the package, shows what it runs and needs, and asks
first. There is no shared marketplace yet. The repository has packages in `plugins/ext`, which you
can install from your clone:

```sh
dashboard plugin install plugins/ext/clock-py
dashboard plugin list
dashboard plugin remove clock-py
```

Then add it by its ID with `package:`:

```yaml
modules:
  clock-py:
    package: clock-py
    config: { zones: [local, Europe/Paris] }
screens:
  - name: main
    tiles:
      - { module: clock-py, size: wide }
```

`scripts/build-marketplace.sh` builds a sample index from those packages. Then
`dashboard plugin install clock-py --index marketplace/index.json` installs by ID, and the web
editor's catalog shows what the index offers.

### Read more

- `dashboard widgets`: every widget; add `<id> --json` for its settings.
- [docs/plugins.md](https://github.com/osdlc/dashboard/blob/main/docs/plugins.md): writing
  plugins, packages and every widget type.
- [docs/contract.md](https://github.com/osdlc/dashboard/blob/main/docs/contract.md): the plugin
  contract in full.
- [docs/dashboards.md](https://github.com/osdlc/dashboard/blob/main/docs/dashboards.md): how
  config files, saved dashboards and project files merge.
- [docs/marketplace.md](https://github.com/osdlc/dashboard/blob/main/docs/marketplace.md):
  installing packages and making an index.
- [README](https://github.com/osdlc/dashboard/blob/main/README.md): every built-in module and its
  settings.
