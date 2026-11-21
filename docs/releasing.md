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
  Its `build.rs` uses `web/` when it is there, else `ui/`, else a placeholder page. Cargo counts
  the ignored `ui/` as uncommitted, hence `--allow-dirty` below; start from a clean checkout so
  it is the only such change.

`crates/cli/tests/copies.rs` fails when a copy differs from its original. The `web` job in
`rust-ci.yml` runs `cargo package --workspace`, which builds every crate from its packed copy, so a
new file read from outside a crate's folder fails there first.

## A release

From a clean checkout of `main`, at the version to release (`version` in the root `Cargo.toml`,
and a `CHANGELOG.md` section for it):

```sh
(cd web && pnpm install --frozen-lockfile && pnpm build)
rm -rf crates/web/ui && cp -r web/dist crates/web/ui
cargo publish --workspace --allow-dirty --dry-run   # packs and builds every crate; uploads nothing
cargo publish --workspace --allow-dirty
git tag v0.8.0 && git push origin v0.8.0
```

`cargo publish --workspace` publishes the crates in dependency order and waits for each to appear
in the index before the next. A version can't be replaced once published, only yanked.

The first release has to be published by hand like this, with a crates.io API token
(`cargo login`), because crates.io sets up trusted publishing only for crates that exist. Then,
for each crate, add a trusted publisher on crates.io (repository osdlc/sitroom, workflow
`release.yml`, environment `crates-io`), and create the `crates-io` environment in the repository
settings with a required reviewer.

After that a release is: bump the version, add its changelog section, merge, then push the tag
(`git tag v0.9.0 && git push origin v0.9.0`). `.github/workflows/release.yml` builds the UI, runs
the dry run, waits for approval of the `crates-io` environment and publishes with a short-lived
token from crates.io. Run it by hand (Actions, release, Run workflow) for a dry run only.
