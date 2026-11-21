# TypeScript plugin SDK

[`sitroom-plugin.ts`](sitroom-plugin.ts) is the whole SDK: one file, no packages. Node 24
runs it as is (it strips the types), and so does Bun. It uses only erasable TypeScript syntax
(no enums, no namespaces), so there is no build step.

## Start

```sh
sitroom plugin new --lang ts my-plugin     # writes my-plugin/main.ts and a copy of the SDK
sitroom plugin-check -- node my-plugin/main.ts
```

Or copy `sitroom-plugin.ts` next to your plugin and import it:

```ts
import { serve, withDefaults, configError, upstreamError, log, liveSeries } from "./sitroom-plugin.ts";
import type { View } from "./sitroom-plugin.ts";
```

Put a `package.json` with `{ "type": "module" }` in the folder, or Node prints a warning.

## What is in it

- `serve({ manifest, init?, render, shutdown? })` answers sitroom on stdin and stdout.
  `manifest` is an object or a function of the config. `contract_version` and `commands` are
  filled in for you. `init(config)` gets the tile's `config:` block (`{}` when there is none).
  `render()` returns a `View`. Both may be async.
- `configError(msg)` (code -32002) and `upstreamError(msg)` (code -32001): throw them and the
  tile shows `msg`. Any other error becomes -32603, with the stack on stderr.
- `withDefaults(config, defaults)` merges the config over the defaults and rejects unknown keys
  (usually typos) with a config error.
- `log(level, msg)` writes a line to sitroom's log, not the tile.
- `liveSeries({ id, label, ... }, { name: value })` builds a `series` widget whose history
  the host keeps, so each render sends only the newest value. `null` sends no point.
- Types for the manifest, the view and every widget (`Manifest`, `View`, `Widget`,
  `StatWidget`, `TableWidget`, ...).

`console.log` goes to stderr once the SDK is imported, because stdout carries only protocol
messages.

## Examples

Run any of them with `sitroom plugin-check -- node examples/<file>` (or `bun`).

| File | Shows |
|---|---|
| [`examples/hello.ts`](examples/hello.ts) | The smallest plugin: config with defaults, a row of stats |
| [`examples/big_files.ts`](examples/big_files.ts) | A stats row and a table with gauges, a config schema, a config error, `log` |
| [`examples/load.ts`](examples/load.ts) | A live chart with `liveSeries` |

They are tested against the real host in `crates/core/tests/sdk_examples.rs`.

## The types

The types between the `GENERATED` markers come from the contract's JSON Schemas. After a
contract change, run `node sdk/gen-types.mjs` from the repository root. A test fails while they
are out of date. The protocol is described in [docs/plugins.md](../../docs/plugins.md).
