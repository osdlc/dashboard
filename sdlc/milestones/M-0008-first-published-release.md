---
type: milestone
id: M-0008
title: First published release
state: open/draft
created: 2026-11-14
version: 1.0.0
tags: [release, stability]
description: The first release with published surfaces and a deprecation policy.
---

# First published release (1.0)

## Work

- Published: levels 1 and 2, the `View` JSON, the plugin package format and the core of the
  config file. Level 3 and the network protocol are published only if
  [M-0007](M-0007-refine.md) found no changes left to make to them.
- The deprecation policy in the [roadmap](../../docs/roadmap.md#stability-tiers) is in effect.
- Release binaries for macOS and Linux (Windows once its CI job passes), a Homebrew formula in
  `osdlc/homebrew-tap`, and the SDKs on npm and PyPI.

## Done when

- 1.0.0 is tagged, the binaries and the formula install, and the SDKs are on npm and PyPI.
