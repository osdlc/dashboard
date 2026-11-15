---
type: milestone
id: M-0006
title: Network
state: open/draft
created: 2026-11-14
version: 0.14.0
tags: [network, experimental]
description: Dashboards on a network find each other and use each other's screens and modules.
---

# Network (0.14)

Dashboards on a network find each other. One can show another's shared screens, and put another's
modules on its own screens as tiles. The design is the
[network-mode experiment](../../docs/experiments/network-mode.md). This milestone is that
experiment's steps, built as experimental and off unless the config turns it on.

## Work

- **Discovery:** mDNS on the local network. `dashboard peers` lists the dashboards it finds, and
  the web view's settings show them under "Network".
- **Shared screens:** a dashboard names the screens and modules it shares. Nothing else is visible
  from the network. A peer sees them after entering the password, read-only and encrypted.
- **Remote modules:** the `remote` built-in puts a peer's tile on a local screen, in the TUI and
  the web view. The module runs on the peer. Its config and tokens stay there; the viewer gets
  only its views.
- **Remote tiles are tiles:** they go stale like any tile when the peer goes away, and they raise
  alerts ([M-0005](M-0005-alerts-and-history.md)) like any tile.
- **The protocol written down:** a new row in the roadmap's surfaces table, experimental. It
  sends the same `View` JSON as every API level, so a remote tile is close to the declarative
  level: the viewer runs no process for it.
- **Measured** with the [M-0002](M-0002-core-and-plugins.md) benchmark: binary size, idle CPU with
  network mode on and off, and the cost of each remote tile.

## Done when

- Two machines on one network see each other. One shows the other's shared screen after the
  password is entered, and a packet capture shows no plaintext.
- A peer's module runs as a tile on a local screen in both views.
- With network mode off, nothing listens on the network.

## Depends on

- [M-0003](M-0003-api-levels.md): the `View` JSON and the levels are written down first.

## Open questions

The experiment's questions, and:

- Can a viewer take a level-3 action ([M-0004](M-0004-interactive.md)) on a remote tile? The
  approval would have to happen on the machine that runs the module. Until that is settled, only
  `open_url` goes over the network.
- Across networks through a relay: not in this milestone, but the protocol should not rule it out.
