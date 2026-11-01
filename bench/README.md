# Native vs plugin benchmark

Measures what a widget costs built into the `dashboard` binary versus run as a plugin
(TypeScript, Python, or Rust compiled to WASM). The rule it led to is in
[docs/modules.md](../docs/modules.md#built-in-or-plugin).

The eight widgets: clock, textfile, git, docker, hackernews, weather, feedreader, battery. Each
is a built-in module (feature `module-<id>`) and a plugin in `plugins/ext/<id>-ts` and
`plugins/ext/<id>-py`. Clock and textfile are also built for WASI, in `bench/wasm`.

Run on a quiet machine: other load moves every number here, build times most.

## Runtime: startup, render, memory, CPU

```sh
bench/wasm/build.sh                       # the WASI plugins (needs wasm32-wasip1 and wasmtime)
cargo run --release -p dashboard-bench    # all widgets, all implementations, about 25 minutes
cargo run --release -p dashboard-bench -- --only clock,git --impls native,py --idle 5 --live 0
```

It writes `bench/results/runtime.json` and prints a markdown table. For each widget and
implementation:

| Measure | How |
|---|---|
| Startup | Plugin: spawn to the `initialize` reply, p50 of 10 cold starts. Native: the `init` call. |
| Render | 100 `view/render` calls back to back, after one first render; p50 and p95. A plugin's time includes the JSON round trip over stdio. |
| Memory | Plugin: resident memory (RSS) of its process after the renders. Native: how much the dashboard's own RSS grew after `init` and the renders. |
| CPU per render | CPU time the process used over the renders, divided by their number. Commands a widget runs (git, docker, pmset) count. |
| Idle CPU | CPU time used while left alone for 20 s after start. |
| CPU per hour | Idle CPU × 3600 + CPU per render × renders per hour at the widget's own refresh. Widgets that refresh every 5 s or faster also run at that refresh for 60 s, as a check (the "live" column). |

A built-in module runs in a fresh child process of the benchmark (`dashboard-bench
native-child`), on a multi-threaded tokio runtime like the dashboard's, so each is measured on
its own.

No widget calls the internet. Weather and the feed reader fetch from a small HTTP server the
benchmark runs on 127.0.0.1, serving the recorded samples from the tests. Hacker News reads its
sample file (its `sample` setting), so it measures parsing, not HTTP. Docker runs the fake docker
program from the tests (`crates/modules-ops/testdata/docker/docker`). Git reads this repository.
Battery reads the real machine.

## Build: size and build time

```sh
bench/build.sh                 # sizes, clean builds, incremental builds: about 45 minutes
bench/build.sh sizes           # or one part: sizes, clean, incremental, clean-sccache
```

It writes `bench/results/build.tsv` (name, value) and prints the same.

- **Sizes** (`--profile small`): no modules; each widget alone; the three HTTP widgets together
  and each one left out of that set; every module; and every module but one, for each widget.
- **Clean build:** `cargo build --profile small -p dashboard` and `cargo build -p dashboard` in an
  empty target dir (`target/bench-clean`, removed after).
- **Incremental build:** touch one module file (clock, textfile, git), rebuild, in the dev,
  release and small profiles; and clock again in a build with only the fun bundle.

## WASM

`bench/wasm` builds the real clock and textfile module source for `wasm32-wasip1`, each as a
stdio plugin, against two stand-ins: `host` (the `Module` trait without tokio, which does not
build for WASI with the features the host uses) and `common` (dashboard-modules-common's own
source, built against that stand-in). `build.sh` also precompiles them for wasmtime
(`.cwasm`); the benchmark runs both (`wasm` and `wasm-aot`).

The host size cost of embedding wasmtime was measured once, in a throwaway crate outside the
repository; the method and numbers are in the report.
