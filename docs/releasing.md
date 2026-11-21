# Releasing to crates.io

`cargo install sitroom` builds the `sitroom` crate from crates.io, so every crate it depends on is
published too: `sitroom-cli-kit`, `-config`, `-contract`, `-host`, `-modules` and the seven
`-modules-*` crates, `-runtime`, `-tui`, `-web` and `-cswap`. They share the workspace version and
go out together. The desktop apps and `bench` have `publish = false`.

## What a published crate can't see

A crate on crates.io holds only the files in its own folder. So nothing a crate builds from may
live outside it:

- **The SDK files** `sitroom plugin new` writes are copies in `crates/cli/sdk/`.
- **The demo config** is a copy in `crates/config/demo.yaml`.
- **The web UI** is built from `web/` in the repository. The published `sitroom-web` carries the
  built files in `crates/web/ui/` instead (git-ignored; `include` in its `Cargo.toml` lists it).
  Its `build.rs` uses `web/` when it is there, else `ui/`, else a placeholder page.

`crates/cli/tests/copies.rs` fails when a copy differs from its original. The `web` job in
`rust-ci.yml` runs `cargo package --workspace`, which builds every crate from its packed copy, so a
new file read from outside a crate's folder fails there first.

## A release

From a clean checkout of `main`, at the version to release (`version` in the root `Cargo.toml`,
and a `CHANGELOG.md` section for it):

```sh
(cd web && pnpm install --frozen-lockfile && pnpm build)
rm -rf crates/web/ui && cp -r web/dist crates/web/ui
cargo publish --workspace --dry-run   # packs and builds every crate; uploads nothing
cargo publish --workspace
git tag v0.8.0 && git push origin v0.8.0
```

`cargo publish --workspace` publishes the crates in dependency order and waits for each to appear
in the index before the next. A version can't be replaced once published, only yanked.

The first release has to be published by hand like this, with a crates.io API token
(`cargo login`), because crates.io sets up trusted publishing only for crates that exist. Then,
for each crate, add osdlc/sitroom as a trusted publisher on crates.io, so later releases can go out
from a workflow without a stored token. osdlc/osdlc's `release-plz.yml` does that for its own
crates.
