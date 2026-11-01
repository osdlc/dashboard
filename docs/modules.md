# Built-in modules

Built-in modules are Rust modules compiled into the `dashboard` binary. They run in the
dashboard's own process, with no plugin to start. (Modules in other languages are plugins; see
[plugins.md](plugins.md).)

Like every module, a built-in one only runs when the config names it, under `modules:` or as a
screen tile. Leaving a module out of the build only changes what the config *can* name.

## Bundles

Built-in modules are grouped into bundles, one crate per bundle:

| Bundle | Crate | Modules |
|---|---|---|
| system | `crates/modules-system` | cpu, memory, gpu, network, disk, processes, log, uptime, textfile, battery, cmdrunner, security, http, file |
| dev | `crates/modules-dev` | github-prs, github-issues, jira, linear, forgejo, git |
| fun | `crates/modules-fun` | demo, clock, hackernews, feedreader, subreddit, devto, mempool, crypto, scores |
| agents | `crates/modules-agents` | codex, copilot, cursor, antigravity, gemini, opencode, pi |
| ops | `crates/modules-ops` | docker, kubernetes, ipinfo, pihole, transmission |
| personal | `crates/modules-personal` | weather, todo, lunar, calendar, org-agenda, markdown-tasks, todo-txt, taskwarrior, sdlc |

Two more crates hold the shared parts:

- `crates/modules-common` (`dashboard-modules-common`): the `Builtin` registration type,
  catalog entry helpers (`entry`, `schema`, `strings`), `manifest`, times (`time::ago`,
  `time::demo_time`), tokens from the environment or a command (`secret::Secret`) and,
  behind its `http` feature, an HTTP client (`http::client`, `http::check`).
- `crates/modules` (`dashboard-modules`): the facade the rest of the workspace uses. It
  gathers the modules of every bundle in the build: `all()`, `ids()`, `builtin(id)` and
  `catalog()`.

## Choosing what goes in the binary

Each bundle is a cargo feature, `bundle-<name>`, and each module is a feature,
`module-<id>`. A bundle feature turns on all of its modules. The default build has every
bundle. The same features exist on `dashboard-modules` and on the `dashboard` binary.

```sh
cargo build -p dashboard                                              # every bundle
cargo build -p dashboard --no-default-features --features bundle-system
cargo build -p dashboard --no-default-features --features bundle-system,module-github-prs
cargo build -p dashboard --no-default-features                        # no built-in modules
```

`dashboard widgets` and the web palette list only the modules in the build. A screen tile that
names a module the build left out makes the config fail to load, with an error that names the
module. A `modules:` entry for a left-out module is ignored: the module does not run.

The order in the catalog and palette is: the core modules (system and GitHub) first, then the
rest in bundle order (system, dev, ops, fun, agents, personal).

Heavy dependencies sit only in the bundles (and modules) that need them: `sysinfo` in the
system modules that read it, and `reqwest` only in modules that call a web API (it comes from
`dashboard-modules-common`'s `http` feature).

`scripts/check-features.sh` builds the binary with no modules, with each bundle alone and with
each module alone, with warnings as errors. For no modules and for each bundle it also runs the
tests of the `dashboard` and `dashboard-modules` crates. Run it after changing a bundle
(`scripts/check-features.sh --bundles` skips the per-module builds).

Binary size, `--profile small` on macOS arm64:

| Build | Size |
|---|---|
| every bundle (default) | 7.6 MB (7,562,432 bytes) |
| `bundle-system` only | 3.5 MB (3,542,688 bytes) |
| no bundles | 3.3 MB (3,339,184 bytes) |

Each bundle alone, and the same builds for Windows: [windows.md](windows.md#size).

Most of the gap between "every bundle" and "system only" is reqwest and its TLS stack, which
only the web-API modules (jira, linear, forgejo, copilot, cursor, antigravity, hackernews,
feedreader, weather, ipinfo, pihole, transmission, subreddit, crypto, calendar and the like)
pull in.

## Built in or plugin?

The rule comes from measuring the eight benchmark widgets three ways (built in, TypeScript plugin,
Python plugin); `bench/` has the harness.

1. **Build a widget in when it needs no account:** local data, a command on the machine, or a
   public API with no key. Built in is the cheapest on every measure: +0 to 200 KB of binary,
   under 5 MB of extra memory, no startup cost, and the same CPU or less.
2. **A new heavy dependency is the exception.** If a widget needs a dependency over about 500 KB
   that the binary doesn't have yet, build it in only when at least three built-in widgets would
   use it. Until then, ship it as a plugin.
3. **Ship a widget as a plugin when** it needs an account or token and nobody has run it live
   (marked "untested live"), few people will use it, it depends on one company's fast-changing
   API, it needs a library from another language, or someone outside this repository writes it.
   Write it in **Python** when it runs all the time (4 to 20 MB per process); a Node plugin takes
   45 to 140 MB.
4. **Compile time is not a reason to keep a widget out:** rebuilding after touching a module
   takes a few seconds.
5. **WASM is not worth it yet.** Most widgets need subprocesses or networking, which WASI
   doesn't give them, and embedding wasmtime adds 1 to 6 MB. It may be used later to sandbox
   untrusted third-party plugins.

## Adding a module to a bundle

Say the new module is `weather` and it goes in the `personal` bundle. If the bundle has no
crate yet, copy `crates/modules-fun` (the smallest), rename it, and add it to
`dashboard-modules` as an optional dependency with `default-features = false` and to the list
in `bundles()` in `crates/modules/src/lib.rs`.

1. **File.** Write `crates/modules-<bundle>/src/weather.rs`: a struct that implements
   `dashboard_host::Module` (`init` reads the `config:` block and returns a manifest, `render`
   returns a `View`). Copy the shape of a module close to yours: `uptime.rs` for a command or
   URL check, `jira.rs` for a token API, `cpu.rs` for a graph.
2. **Feature.** In the bundle's `Cargo.toml`, add a feature named after the id and add it to
   `default`. Put the module's own dependencies behind it (`weather = ["dep:x"]`, or
   `["dashboard-modules-common/http"]` for reqwest).
3. **Registration.** In the bundle's `lib.rs`, declare the file behind its feature
   (`#[cfg(feature = "weather")] pub mod weather;`) and add a `Builtin` to `MODULES`:

   ```rust
   #[cfg(feature = "weather")]
   Builtin {
       id: "weather",
       core: false, // true only for the system and GitHub tiles, listed first
       new: || Box::new(weather::Weather::default()),
       catalog: weather::catalog,
   },
   ```

4. **Facade features.** In `crates/modules/Cargo.toml` add
   `module-weather = ["dashboard-modules-personal/weather"]` and add `module-weather` to
   `bundle-personal`. In `crates/cli/Cargo.toml` add
   `module-weather = ["dashboard-modules/module-weather"]`.
5. **Catalog entry and demo view.** Add `pub fn catalog() -> CatalogEntry` to the file. Start
   from `crate::entry(id, name, size, demo)` and fill in `description`, `tags`,
   `config_schema` (`crate::schema::<Config>()`) and `requirements` (binaries, environment
   variables, hosts). The `demo` view is what the palette shows before the module is set up:
   build it with the same code `render` uses, fed with made-up data, so it can't drift from
   the real tile. For demo times use `time::demo_time(secs_ago)` so ages read the same
   whenever it is shown. A module for a token API that was never called live says
   "untested live" in its description.
6. **Tests.** Add a `#[cfg(test)] mod tests` to the file that parses a hand-written sample of
   the command output or API response and checks the view. Add the id to
   `default_build_has_every_module` in `crates/modules/src/lib.rs`; the other facade tests
   then check that the entry is complete and its demo view is valid.
7. **Check.** `cargo test --workspace`, `cargo clippy --workspace --all-targets`, and
   `scripts/check-features.sh`.

Tokens never go in the config file or the code: read them through `token_env` /
`token_command` (see `Secret` in `crates/modules-common/src/secret.rs`).

A setting that is a file or folder path gets `#[schemars(extend("x-path" = true))]` on its field
(a string or a list of strings). Before the module starts, the runtime makes relative paths in it
start from the folder of the config file that set them, so the module only expands `~/` with
`expand_home`. `path_settings_are_marked` in `crates/modules/src/lib.rs` checks the usual names.
