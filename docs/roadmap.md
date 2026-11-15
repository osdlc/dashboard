# Roadmap

Where the dashboard is going, in milestones, and what a user or plugin author can count on at each
step. Pre-1.0, so the milestones and their order can change. Each milestone is a file in
[sdlc/milestones/](../sdlc/milestones/) and a GitHub milestone of the same name; issues and pull
requests are attached there.

## Stability tiers

Every part of the dashboard that someone outside this repository could depend on has one of three
tiers.

| Tier | What it promises |
|---|---|
| **Published** | Changes only in backward-compatible ways within a contract major version. Something that goes away is deprecated first, for at least one minor release: the CHANGELOG says so, and `dashboard config` and `plugin-check` warn where it is used. |
| **Experimental** | Can change or be removed in any release, with a CHANGELOG entry and no deprecation period. Use it, and expect to update. |
| **Internal** | No promise. Only this repository uses it. |

**Everything starts experimental.** The API is still being refined, so nothing is published
until 1.0 ([M-0008](../sdlc/milestones/M-0008-first-published-release.md)). Moving a part to
published is a decision recorded in this file and the CHANGELOG.

## API levels

The ways to put something on the dashboard, from the least code to the most. Each level adds to
the one before it, and the same `View` JSON is used at every level.

| Level | What you write | How it reaches the host | Tier today |
|---|---|---|---|
| **1. Declarative** | A `View` as JSON (the widget tree in [contract.md](contract.md)), or any JSON with values picked out of it | A file (`file` built-in) or a URL (`http` built-in). No process of yours runs. | Experimental |
| **2. Applet** | A program that speaks JSON-RPC 2.0, one message per line, over stdin and stdout: `initialize`, `view/render`, `log` | The host starts it, polls it and stops it ([plugins.md](plugins.md)). The TypeScript and Python SDKs are helpers over this level, not a level of their own. | Experimental |
| **3. Interactive** | An applet that also sends views when it wants (`view/update`), gets events back (a picked item, an action), and declares commands the user approves | Same process as level 2, with more methods | Not built ([M-0004](../sdlc/milestones/M-0004-interactive.md)) |
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

One file each in [sdlc/milestones/](../sdlc/milestones/), with the work, what "done" means and
the open questions. In order:

| Milestone | Version | What it does |
|---|---|---|
| [M-0001 Foundation](../sdlc/milestones/M-0001-foundation.md) | 0.9 | CI on every pull request, tiers shown where each surface is documented, the open pull requests settled. |
| [M-0002 Core and plugins](../sdlc/milestones/M-0002-core-and-plugins.md) | 0.10 | Measure what each kind of plugin costs, alone and 100 at a time; decide what is core and what is a plugin (#19). |
| [M-0003 API levels written down](../sdlc/milestones/M-0003-api-levels.md) | 0.11 | One document for what each level is and promises (#18), the same widget at every level (#17). |
| [M-0004 Interactive level and actions](../sdlc/milestones/M-0004-interactive.md) | 0.12 | Level 3, and acting on items from their tiles: acknowledge an incident, re-run a CI job, close a todo. |
| [M-0005 Alerts and history](../sdlc/milestones/M-0005-alerts-and-history.md) | 0.13 | OS notifications when a tile goes bad, history kept across restarts. Can run alongside M-0004. |
| [M-0006 Network](../sdlc/milestones/M-0006-network.md) | 0.14 | Dashboards find each other on the network and use each other's screens and modules. |
| [M-0007 Refine](../sdlc/milestones/M-0007-refine.md) | 0.15 | Use the levels for real, change them, pick what 1.0 publishes. |
| [M-0008 First published release](../sdlc/milestones/M-0008-first-published-release.md) | 1.0 | Published surfaces, the deprecation policy, release binaries and packages. |

## Experiments

Ideas being tried, outside the milestones. An experiment is off unless the config turns it on,
the CHANGELOG lists it under "Experimental", and it can be dropped. One that works becomes work in
a milestone; one that doesn't gets a short write-up of why.

| Experiment | What it tries | Notes |
|---|---|---|
| Network mode | Dashboards on a network find each other and show each other's shared screens, behind a password, encrypted | [experiments/network-mode.md](experiments/network-mode.md); built in [M-0006](../sdlc/milestones/M-0006-network.md) |
| Image widget and screensaver module | An `image` widget and a module that shows pictures | Draft #6 |
| Sandboxed WASM plugins | A fourth level for untrusted plugins | Waits on WASI networking and subprocesses ([modules.md](modules.md)); re-measure then |
