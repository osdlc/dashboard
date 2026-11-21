# Releasing

A version tag releases sitroom to crates.io (`cargo install sitroom`), as binaries on the GitHub
Release, and to Homebrew (`brew install osdlc/tap/sitroom`), all from `.github/workflows/release.yml`.

## crates.io

`cargo install sitroom` builds the `sitroom` crate from crates.io, so the five crates it is built
from are published too: `sitroom-contract` (the types plugins and renderers share),
`sitroom-core` (config, host, CLI conventions), `sitroom-modules` (the built-in modules, the
bundled cswap plugin and the runtime), `sitroom-tui` and `sitroom-web`. They share the workspace
version and go out together. The desktop apps and `bench` have `publish = false`.

## What a published crate can't see

A crate on crates.io holds only the files in its own folder. So nothing a crate builds from may
live outside it:

- **The SDK files** `sitroom plugin new` writes are copies in `crates/cli/sdk/`.
- **The demo config** is a copy in `crates/core/demo.yaml`.
- **The web UI** is built from `web/` in the repository. The published `sitroom-web` carries the
  built files in `crates/web/ui/` instead (git-ignored; `include` in its `Cargo.toml` lists it).
  Its `build.rs` uses `web/` when it is there, else `ui/`, else a placeholder page. Cargo counts
  the ignored `ui/` as uncommitted, hence `--allow-dirty` below; start from a clean checkout so
  it is the only such change.

`crates/cli/tests/copies.rs` fails when a copy differs from its original. The `web` job in
`rust-ci.yml` runs `cargo package --workspace`, which builds every crate from its packed copy, so a
new file read from outside a crate's folder fails there first.

## A release

Bump `version` in the root `Cargo.toml` and in every `sitroom-*` path dependency (they name it
too), turn the changelog's Unreleased section into the version's, and merge that. Then push the
tag:

```sh
git tag v0.9.0 && git push origin v0.9.0
```

`.github/workflows/release.yml` then:

1. `publish`: checks the tag matches the version, builds the UI into `crates/web/ui`, runs
   `cargo publish --workspace --dry-run` (packs and builds every crate), waits for approval of
   the `crates-io` environment, then publishes.
2. `binaries`, at the same time: builds `sitroom` and `sit` (profile `dist`) for
   `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu` and
   `aarch64-unknown-linux-gnu`, one `sitroom-<version>-<target>.tar.gz` each.
3. `github`, once both are done: the GitHub Release of the tag, with the archives, `SHA256SUMS`
   and the changelog's section as the notes.
4. `homebrew`: writes `Formula/sitroom.rb` in osdlc/homebrew-tap for those archives and pushes
   it as the osdlc-release GitHub App, the way osdlc/skillwright updates its formula. `cargo publish --workspace` goes in
dependency order and waits for each crate to appear in the index before the next. A version
can't be replaced once published, only yanked. Run the workflow by hand (Actions, release, Run
workflow) for a dry run only.

## Setting it up (once)

1. **The environment.** Settings, Environments, New environment `crates-io`. Add yourself as a
   required reviewer, and under deployment branches and tags allow only tags matching `v*`.
2. **A token for the first release.** crates.io allows trusted publishing only for a crate that
   exists, so the first release needs an API token. On crates.io, Account Settings, API Tokens:
   scopes `publish-new` and `publish-update`, crates `sitroom*`, a short expiry. Save it as the
   secret `CARGO_REGISTRY_TOKEN` of the `crates-io` environment (not a repository secret).
3. **The Homebrew environment.** New environment `homebrew-tap`, tags matching `v*` only, with
   the secret `RELEASE_PLZ_APP_PRIVATE_KEY`: the osdlc-release App's private key, the same one
   osdlc/skillwright's `homebrew-tap` environment holds (a new key from the App's settings works
   too). The App must be installed on osdlc/homebrew-tap; it already is, for skillwright.
4. **Release** as above.
5. **Trusted publishing.** For each of the six crates on crates.io, Settings, Trusted
   Publishing, add GitHub: owner `osdlc`, repository `sitroom`, workflow `release.yml`,
   environment `crates-io`. Then delete the `CARGO_REGISTRY_TOKEN` secret and revoke the token.
   Later releases get a short-lived token from crates.io instead.

To publish from a machine instead (it needs a few GB free for the build), from a clean checkout:

```sh
(cd web && pnpm install --frozen-lockfile && pnpm build)
rm -rf crates/web/ui && cp -r web/dist crates/web/ui
cargo publish --workspace --allow-dirty --dry-run
cargo publish --workspace --allow-dirty
```
