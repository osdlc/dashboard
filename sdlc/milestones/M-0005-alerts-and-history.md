---
type: milestone
id: M-0005
title: Alerts and history
state: open/draft
created: 2026-11-14
version: 0.13.0
tags: [alerts, history]
description: Tell the user when a tile goes bad, and keep history across restarts.
---

# Alerts and history (0.13)

Tell the user when something goes wrong without them watching the screen, and remember what
happened across restarts.

## Alerts

- A tile or item whose status changes to warn or error raises an alert: an OS notification from
  the desktop app and the CLI where the OS supports it, a badge in the web view, and the tray
  icon's color.
- Rules in the config: which modules or screens alert, on which changes, quiet hours, and how long
  a status must hold before it alerts, so a check that flaps does not send one alert a minute.
- A rollup per screen: the worst status on it, shown on the screen's tab.
- An alert never runs a command.

## History

- Series history kept across restarts. Today it is an in-memory buffer of 300 points per series.
- Status history per tile: when it went to warn or error and when it came back, shown on the tile.
- "Since you last looked": items that are new since the screen was last shown are marked.
- A size cap per tile, and one setting to turn history off.

## Done when

- A tile that goes to error raises one OS notification, and none during quiet hours.
- After a restart, series and status history are as they were.
- The cost of storing history is measured with the [M-0002](M-0002-core-and-plugins.md)
  benchmark and is inside its budgets.

## Depends on

Nothing after [M-0002](M-0002-core-and-plugins.md). This can run alongside
[M-0004](M-0004-interactive.md).

## Open questions

- How history is stored: SQLite, or append-only files. It changes the binary size.
- Does a peer on the network ([M-0006](M-0006-network.md)) get the history, or only new points?
