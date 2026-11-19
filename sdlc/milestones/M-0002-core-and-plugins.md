---
type: milestone
id: M-0002
title: Core and plugins
state: open/draft
created: 2026-11-14
version: 0.10.0
tags: [performance, modules, plugins]
description: Measure what each kind of plugin costs, and decide what is core and what is a plugin.
---

# Core and plugins (0.10)

Decide what ships in the binary (core) and what is a plugin, from numbers anyone can reproduce.
Issue #19.

## Measure each kind of plugin

The [benchmark](../../bench/README.md) leaves out some kinds of plugin, and it measures one widget
at a time. Add:

- **The declarative kinds:** the `http` and `file` built-ins showing the same widgets from a URL
  or a file. No process of the plugin's runs, so the cost is all the host's.
- **A Rust stdio plugin:** a compiled program with no runtime, to split the cost of the stdio
  protocol from the cost of a language runtime.
- **Host cost per plugin:** the host's own CPU and memory for each plugin it runs (supervising the
  process, parsing its JSON, the renderer feed), on top of the plugin's own process.
- **Scale:** a screen of 50 and then 100 tiles, mixed kinds, measuring host CPU, memory, and the
  time from a module's update to the renderers.
- **Recorded numbers:** commit `bench/results/runtime.json` and `build.tsv` from one run, with the
  machine, OS, toolchain and commit. The docs quote that run.
- **A regression gate:** a short benchmark in CI that fails on a budget, not an exact number,
  for the numbers a shared runner measures the same each time: binary size per bundle and per
  module. Startup and render p95 with the default bundles, and host memory with a typical
  screen, are reported on each run but don't fail it: timings on shared runners are too noisy.

## Decide core and plugins

- Turn the rule in [modules.md](../../docs/modules.md#built-in-or-plugin) into a policy with both
  sides: what is built in, what is never built in, and how a built-in moves out to a plugin.
  Confirm or change the rule with the numbers above.
- A verdict for each of the ~50 built-ins: stays built in, moves to a plugin, or goes. Modules
  marked "untested live" are tested against a real account or moved out.
- A task for each module that moves. The moves can land in later milestones.

## Done when

- The policy is published, with every current built-in and its verdict.
- Benchmark results for every kind are committed, and the docs quote them.
- The CI job enforces the budgets.
- The scale test has run once and its results are recorded.

## Open questions

- Is "core" one line or three: the default binary, optional bundles built in at build time
  (bundles exist today), and plugins?
- Where do modules that move out live: `plugins/ext` in this repository, or repositories of their
  own, listed in the marketplace?
