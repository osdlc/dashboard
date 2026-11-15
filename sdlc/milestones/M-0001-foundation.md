---
type: milestone
id: M-0001
title: Foundation
state: open/planned
created: 2026-11-14
version: 0.9.0
tags: [ci, stability]
description: Make changes safe to land, and mark what is experimental.
---

# Foundation (0.9)

Make changes safe to land, and mark what is experimental, before the larger work starts.

## Work

- Rust CI: format, clippy and tests on Linux and macOS, each bundle on its own, the web build and
  the generated types, all on every pull request. Windows runs without gating.
- The stability tiers ([roadmap](../../docs/roadmap.md#stability-tiers)) shown where people meet
  each surface: a note at the top of each doc, a `stability` field on catalog entries, shown by
  `dashboard widgets` and in the web catalog.
- Settle the open pull requests: merge each one, or move it to an experiment.
  - #12 clock styles: column and table
  - #13 clock face and display widgets
  - #14 stocks plugin
  - #15 series: negative values, `y_min` and a fitted scale
  - #16 containers plugin
  - draft #6 image widget and screensaver module: stays an experiment
- The code-health part of osdlc's Rust package standard (`S-package-shape-rs`): workspace lints,
  a `description` and `repository` on every crate.

## Done when

- CI gates every pull request on Linux and macOS.
- Every surface in the roadmap's table shows its tier where it is documented.
- No pull request from #6 to #16 is open without a decision.
