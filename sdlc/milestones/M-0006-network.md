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

Build the [network-mode experiment](../../docs/experiments/network-mode.md) as experimental, off
unless the config turns it on. The experiment has the design, what is sent and what never is.

## Work

- **Steps 1 to 3 of the experiment:** discovery, one shared screen, and the `remote` module that
  puts a peer's module on a local screen.
- **Remote tiles are tiles:** they go stale like any tile when the peer goes away, and they raise
  alerts ([M-0005](M-0005-alerts-and-history.md)) like any tile.
- **The protocol written down:** a new row in the roadmap's surfaces table, experimental. It
  sends the same `View` JSON as every API level, so a remote tile is close to the declarative
  level: the viewer runs no process for it.
- **Measured** with the [M-0002](M-0002-core-and-plugins.md) benchmark: binary size, idle CPU with
  network mode on and off, and the cost of each remote tile.

## Done when

The experiment's "Done when", and:

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
