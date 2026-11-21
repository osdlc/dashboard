# Dashboards and config layers

A **dashboard** is a named set of screens and module settings, kept as its own file. A
**project file** adds a repository's own modules and screens when you run sitroom inside
that repository. Both are laid over your normal config file.

## Saved dashboards

Saved dashboards live in `~/.config/sitroom/dashboards/<name>.yaml` (the folder moves with
`XDG_CONFIG_HOME` or `SITROOM_CONFIG_DIR`). A dashboard file has the same keys as
`config.yaml`, plus an optional `description:`:

```yaml
# ~/.config/sitroom/dashboards/work.yaml
description: Day job
modules:
  github-prs:
    config: { limit: 10 }
screens:
  - name: review
    tiles:
      - module: github-prs
        size: wide
      - module: cpu
```

Names are lowercase letters, digits and dashes (`work`, `on-call`).

```sh
sitroom --dashboard work serve      # or: tui
sitroom dashboards list             # saved dashboards, and the project file if one applies here
sitroom import ~/Downloads/on-call.yaml          # saved as "on-call"
sitroom import https://example.com/team.yaml --name team
sitroom export work                 # to stdout
sitroom export work work.yaml       # to a file (--force to replace it)
```

`import` checks that the file is a valid config and saves it. It does not replace a saved
dashboard of the same name unless you pass `--force`. Then it lists what the dashboard needs
that isn't here: plugin packages that aren't installed, plugin programs not found on `PATH`,
and modules this build doesn't have. Install a missing package with `sitroom plugin install`
(see [marketplace.md](marketplace.md)). It also lists the programs the dashboard runs (plugins, and commands set in
built-in modules' settings): opening a dashboard runs them, so read them first. Importing from a URL downloads with `curl`,
and only `https://` URLs are accepted.

To make one from the app, use **+ New** or **Save as…** next to the title (the web view and the
desktop app). **Blank** starts with one empty screen and opens the screen editor; **Copy of
current** copies the file settings are saved to, comments included, and then changes go to the
copy. Either way the dashboard opens at once. A blank dashboard still has the modules from
`config.yaml` running, as `modules:` merge (see [Precedence](#precedence)); only the screens
start empty. A copy keeps relative paths as written, and they then start in `dashboards/`, so
use `~/` or absolute paths in a file you copy.

The web view has a switcher next to the title when there is at least one saved dashboard.
Switching starts and stops only the modules that differ. The choice lasts while the server
runs; start with `--dashboard` to pick one at startup.

## The project file

`.sitroom.yaml` is found by walking up from the working directory, so `sitroom serve`
anywhere inside a repository uses the repository's file. It has the same keys as
`config.yaml`. Use it for modules and screens that only make sense in that repository:

```yaml
# my-repo/.sitroom.yaml
modules:
  ci:
    plugin: [python3, tools/ci_status.py]   # relative to this file's folder
  repo:
    builtin: git
    config: { repos: [.] }                  # this repository, from any folder in it
screens:
  - name: my-repo
    tiles:
      - module: ci
      - module: repo
```

`--no-project` skips it.

### Trusting a project file

A project file can start programs: `plugin:` commands, plugin packages in a `plugins/` folder
next to it, `token_command`, and built-in modules that run commands. It comes with whatever
folder you are in, including a repository you just cloned, and the search goes past the
repository up to `/`. So a project file is **not used until you trust it**, like
`direnv allow`:

```sh
cat .sitroom.yaml          # read it first
sitroom trust              # trust the .sitroom.yaml found from here upward
sitroom trust path/to/.sitroom.yaml
sitroom trust --remove     # stop using it
```

`sitroom trust` prints the programs the file runs: plugins, cmdrunner commands, the
program set for docker, kubernetes and taskwarrior, and any `token_command` or `url_command`.
That list covers the settings it knows, so read the file too. Trust is for the file as it is now: the
list in `~/.local/share/sitroom/trusted-projects` keeps the file's path and a SHA-256 of its
content, so after any change to the file (yours, or from a `git pull`) it is skipped again
until you run `sitroom trust` again.

An untrusted project file is skipped, not an error. `serve` and `tui` print the files in use
and any skipped project file when they start; `sitroom config`, `sitroom dashboards list`
and the settings screen say so too.

## Precedence

The files are read in this order. A later file wins:

1. **Machine file**: `~/.config/sitroom/config.yaml`, or `--config`.
2. **Saved dashboard**: the one named by `--dashboard` (or picked in the web switcher).
3. **Project file**: `.sitroom.yaml` from the working directory upward, once trusted.

How each key merges:

| Key | Rule |
|---|---|
| `modules:` | Per module ID. A module's settings come whole from the last file that names it. Modules from earlier files stay. |
| `screens:` | A saved dashboard's screens **replace** the machine file's. A project file's screens are **added in front** (so the repository's screen shows first); a project screen with the same name as an earlier one replaces it. |
| `color_themes:` | Per theme name, like `modules:`. |
| `theme:`, `color_theme:`, `rotate:`, `description:` | The last file that sets it wins. |

Each file must be valid on its own. An error names the file it is in.

Relative paths start in the folder of the file that set that module, in every file: the
machine file, a saved dashboard and the project file. That covers a plugin's command
(`plugin: [./run.sh]`, which also runs in that folder) and the paths in a built-in module's
settings (`textfile`'s `path: TODO.md`, `git`'s `repos: [.]`). So `.sitroom.yaml` works the
same from any folder inside the repository. Paths that start with `~/`, absolute paths and URLs
are used as they are. A built-in marks which settings are paths in its config schema
(`"x-path": true`). A built-in module that is only named on a screen has no settings, so has no
paths. A saved dashboard's paths start in its folder, `~/.config/sitroom/dashboards/`, so
`sitroom import` keeps relative paths relative to where it saves the file: use `~/` or
absolute paths in a dashboard you share. Plugin packages are looked for in `plugins/` next to each file (project first), then in
`~/.local/share/sitroom/plugins/`.

When no file exists at all, the built-in demo is shown, as before.

## Where changes are saved

The settings screen (theme and colors) and the screen editor save to the saved dashboard in use, or to
`config.yaml` when none is. They never write the project file, which is usually checked in:
screens that came from it are left out when the editor saves, and the settings screen says
so if you changed one. A screen of yours that a project screen of the same name hides is kept
in your file as it was. Edit `.sitroom.yaml` by hand, then run `sitroom trust` again.

Changes to any of the files apply while sitroom runs. That includes a `.sitroom.yaml`
created in the working directory and a project file being trusted. A new project file in a
folder above the working directory needs a restart.

`sitroom config` lists the files in use, in order, and checks them. The settings screen in
the web view shows the same list.
