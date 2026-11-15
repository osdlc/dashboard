# Roadmap

Where the dashboard is going, in milestones, and what a user or plugin author can count on at each
step. Pre-1.0, so the milestones and their order can change. Each milestone is a GitHub milestone
of the same name; issues and pull requests are attached there.

## Stability tiers

Every part of the dashboard that someone outside this repository could depend on has one of three
tiers.

| Tier | What it promises |
|---|---|
| **Published** | Changes only in backward-compatible ways within a contract major version. Something that goes away is deprecated first, for at least one minor release: the CHANGELOG says so, and `dashboard config` and `plugin-check` warn where it is used. |
| **Experimental** | Can change or be removed in any release, with a CHANGELOG entry and no deprecation period. Use it, and expect to update. |
| **Internal** | No promise. Only this repository uses it. |

**Everything starts experimental.** The API is still being refined, so nothing is published
until 1.0 (milestone 5). Moving a part to published is a decision recorded in this file and the
CHANGELOG.

## API levels

The ways to put something on the dashboard, from the least code to the most. Each level adds to
the one before it, and the same `View` JSON is used at every level.

| Level | What you write | How it reaches the host | Tier today |
|---|---|---|---|
| **1. Declarative** | A `View` as JSON (the widget tree in [contract.md](contract.md)), or any JSON with values picked out of it | A file (`file` built-in) or a URL (`http` built-in). No process of yours runs. | Experimental |
| **2. Applet** | A program that speaks JSON-RPC 2.0, one message per line, over stdin and stdout: `initialize`, `view/render`, `log` | The host starts it, polls it and stops it ([plugins.md](plugins.md)). The TypeScript and Python SDKs are helpers over this level, not a level of their own. | Experimental |
| **3. Interactive** | An applet that also sends views when it wants (`view/update`), gets events back (a picked item, an action), and declares commands the user approves | Same process as level 2, with more methods | Not built (milestone 3) |
| Built-in module | A Rust `Module` in a bundle ([modules.md](modules.md)) | Compiled in | Internal |

Sandboxed WASM plugins are not a level yet: see [experiments](#experiments).

## Surfaces and their tiers

| Surface | Defined in | Tier |
|---|---|---|
| `View` JSON and the widget types | `crates/contract`, [contract.md](contract.md) | Experimental |
| Stdio protocol (level 2) | [contract.md](contract.md), [plugins.md](plugins.md) | Experimental |
| TypeScript and Python SDKs | `sdk/` | Experimental |
| Plugin package format (`dashboard-plugin.yaml`) | [plugins.md](plugins.md) | Experimental |
| Marketplace index format | [marketplace.md](marketplace.md) | Experimental |
| Config file format | README, [dashboards.md](dashboards.md) | Experimental |
| CLI commands, exit codes, `--json` output | [cli.md](cli.md) | Experimental |
| Color theme tokens | `crates/contract/src/theme.rs` | Experimental |
| Each built-in module and its `config:` | catalog entries (`dashboard widgets`) | Experimental |
| Web view URL options (`?mode=ambient`) | README | Experimental |
| Desktop app and its flags | [desktop.md](desktop.md) | Experimental |
| Built-in `Module` trait, renderer feed (WebSocket messages), the Rust crates | source | Internal |

## Milestones

### 1. Foundation (0.9)

Make changes safe to land, and mark what is experimental.

- Rust CI: format, clippy and tests on Linux and macOS, each bundle on its own, the web build and
  the generated types, all on every pull request. Windows runs without gating.
- This file, and the tier shown where people meet each surface: a note at the top of each doc, a
  `stability` field on catalog entries, shown by `dashboard widgets` and in the web catalog.
- Settle the open pull requests (#12 to #16) and draft #6: merge each one, or move it to an
  experiment.
- A committed benchmark run with its machine, toolchain and commit, which the docs quote (#19,
  first part).
- The code-health part of osdlc's Rust package standard (`S-package-shape-rs`): workspace lints,
  a `description` and `repository` on every crate.

### 2. API levels written down (0.10)

One place that says what each level is and what it promises (#18).

- `docs/api-levels.md`: for each level, what the host guarantees (process lifecycle, timeouts,
  history, error codes), what the plugin must do, and what would make it published.
- The same widget written at each level and in several languages, tested to render the same view
  (#17).
- `dashboard plugin-check` reports which level a plugin uses.
- The "Not yet" lists in [contract.md](contract.md) and [plugins.md](plugins.md) point here
  instead, so there is one list.

### 3. Interactive level (0.11)

Build level 3, as experimental.

- Push mode for applets: `view/update`, which the host refuses today.
- Events sent back to the applet: a picked item, an action on it.
- Declared commands that run only after the user approves them, and the `run_command` action.

### 4. Refine (0.12)

Use the levels for real and change them before anything is published.

- Port the plugins in `plugins/ext` to the documented levels, run them for a while, and collect
  what should change in the API.
- A verdict for each built-in module: stays built in, moves to a plugin, or goes (#19). Modules
  marked "untested live" are tested against a real account or moved out.
- Pick what 1.0 publishes. Deprecate what is going away.

### 5. First published release (1.0)

- Published: levels 1 and 2, the `View` JSON, the plugin package format and the core of the
  config file. Level 3 is published only if milestone 4 found no changes left to make to it.
- The deprecation policy above is in effect.
- Release binaries for macOS and Linux (Windows once its CI job passes), a Homebrew formula in
  `osdlc/homebrew-tap`, and the SDKs on npm and PyPI.

## Experiments

Ideas being tried, outside the milestones. An experiment is off unless the config turns it on,
the CHANGELOG lists it under "Experimental", and it can be dropped. One that works becomes work in
a milestone; one that doesn't gets a short write-up of why.

| Experiment | What it tries | Notes |
|---|---|---|
| Network mode | Dashboards on a network find each other and show each other's shared screens, behind a password, encrypted | [experiments/network-mode.md](experiments/network-mode.md) |
| Image widget and screensaver module | An `image` widget and a module that shows pictures | Draft #6 |
| Sandboxed WASM plugins | A fourth level for untrusted plugins | Waits on WASI networking and subprocesses ([modules.md](modules.md)); re-measure then |
