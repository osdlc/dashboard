# Roadmap

Where the dashboard is going, in milestones, and what a user or plugin author can count on at each
step. Pre-1.0, so the milestones and their order can change. Each milestone is a file in
[sdlc/milestones/](../sdlc/milestones/). GitHub milestones of the same names, to attach issues
and pull requests to, are not created yet.

## Stability tiers

Every part of the dashboard that someone outside this repository could depend on has one of three
tiers.

| Tier | What it promises |
|---|---|
| **Published** | Changes only in backward-compatible ways within a major version: the contract version for the `View` JSON and the stdio protocol, the dashboard's own version for everything else. Something is removed only in a new major version, and is deprecated for at least one minor release before that: the CHANGELOG says so, and `dashboard config` and `plugin-check` warn where it is used. |
| **Experimental** | Can change or be removed in any release, with a CHANGELOG entry and no deprecation period. Use it, and expect to update. |
| **Internal** | No promise. Only this repository uses it. |

**Everything starts experimental.** The API is still being refined, so nothing is published
until 1.0 ([M-0008](../sdlc/milestones/M-0008-first-published-release.md)). Moving a part to
published is a decision recorded in this file and the CHANGELOG.

## API levels

The ways to put something on the dashboard, from the least code to the most. Each level adds to
the one before it, and the same `View` JSON is used at every level.

| Level | What you write | How it reaches the host |
|---|---|---|
| **1. Declarative** | A `View` as JSON (the widget tree in [contract.md](contract.md)), or any JSON with values picked out of it | A file (`file` built-in) or a URL (`http` built-in). No plugin process runs; at most a `token_command` runs to fetch a token. |
| **2. Applet** | A program that speaks JSON-RPC 2.0, one message per line, over stdin and stdout: `initialize`, `view/render`, `log` | The host starts it, polls it and stops it ([plugins.md](plugins.md)). The TypeScript and Python SDKs are helpers over this level, not a level of their own. |
| **3. Interactive** | An applet that also sends views when it wants (`view/update`), gets events back (a picked item, an action), and declares commands the user approves. Not built yet ([M-0004](../sdlc/milestones/M-0004-interactive.md)). | Same process as level 2, with more methods |
| Built-in module | A Rust `Module` in a bundle ([modules.md](modules.md)) | Compiled in |

Each level's tier is in the [surfaces table](#surfaces-and-their-tiers). Sandboxed WASM plugins
are not a level yet: see [experiments](#experiments).

## Surfaces and their tiers

| Surface | Defined in | Tier |
|---|---|---|
| `View` JSON and the widget types | `crates/contract`, [contract.md](contract.md) | Experimental |
| Stdio protocol (level 2), with its error codes | [contract.md](contract.md), [plugins.md](plugins.md) | Experimental |
| TypeScript and Python SDKs | `sdk/` | Experimental |
| Plugin package format (`dashboard-plugin.yaml`) and where packages are installed | [plugins.md](plugins.md) | Experimental |
| Marketplace index format | [marketplace.md](marketplace.md) | Experimental |
| Config file format, and the token `*_env` / `*_command` settings | README, [dashboards.md](dashboards.md) | Experimental |
| CLI commands, exit codes, `--json` output, the JSON Schemas from `dashboard schema` | [cli.md](cli.md) | Experimental |
| `DASHBOARD_*` environment variables | README, [cli.md](cli.md) | Experimental |
| Color theme tokens | `crates/contract/src/theme.rs` | Experimental |
| Each built-in module and its `config:` | catalog entries (`dashboard widgets`) | Experimental |
| Web view URL options (`?mode=ambient`) and signing in | README, [cli.md](cli.md#signing-in-to-the-web-view) | Experimental |
| Desktop app and its flags | [desktop.md](desktop.md) | Experimental |
| Built-in `Module` trait, renderer feed (WebSocket messages), the Rust crates | source | Internal |

## Milestones

The milestones are the files in [sdlc/milestones/](../sdlc/milestones/). Each one holds its
version, its work, what "done" means and its open questions, and this file does not repeat them.
Their ids run in release order:

1. [Foundation](../sdlc/milestones/M-0001-foundation.md)
2. [Core and plugins](../sdlc/milestones/M-0002-core-and-plugins.md)
3. [API levels written down](../sdlc/milestones/M-0003-api-levels.md)
4. [Interactive level and actions](../sdlc/milestones/M-0004-interactive.md)
5. [Alerts and history](../sdlc/milestones/M-0005-alerts-and-history.md)
6. [Network](../sdlc/milestones/M-0006-network.md)
7. [Refine](../sdlc/milestones/M-0007-refine.md)
8. [First published release](../sdlc/milestones/M-0008-first-published-release.md)

## Experiments

Ideas being tried, outside the milestones until one is planned as a milestone's work (network
mode is). An experiment is off unless the config turns it on, the CHANGELOG lists it under
"Experimental", and it can be dropped. One that works becomes work in a milestone; one that
doesn't gets a short write-up of why. An experiment that changes the contract, such as a new
widget type, cannot be turned off by the config, so it stays on its branch or in a draft pull
request until a milestone takes it on.

| Experiment | What it tries | Notes |
|---|---|---|
| Network mode | Dashboards on a network find each other and show each other's shared screens, behind a password, encrypted | [experiments/network-mode.md](experiments/network-mode.md); built in [M-0006](../sdlc/milestones/M-0006-network.md) |
| Image widget and screensaver module | An `image` widget and a module that shows pictures | On main since #6. It adds a widget type, so the config can't turn it off. |
| Sandboxed WASM plugins | A fourth level for untrusted plugins | Waits on WASI networking and subprocesses ([modules.md](modules.md)); re-measure then |
