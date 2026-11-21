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

1. `publish`: waits for approval of the `crates-io` environment, checks the tag matches the
   version, then runs `scripts/crates.sh dry-run` (builds the UI into `crates/web/ui`, packs and
   builds every crate) and `scripts/crates.sh publish`. A version already on crates.io (published
   by hand) is skipped; some crates published and some not stops the run.
2. `binaries`, at the same time: builds `sitroom` and `sit` (profile `dist`) for
   `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`,
   `aarch64-unknown-linux-gnu` and, static with cargo-zigbuild, `x86_64-unknown-linux-musl` and
   `aarch64-unknown-linux-musl`, one `sitroom-<version>-<target>.tar.gz` each.
   `cargo binstall sitroom` finds these through `[package.metadata.binstall]` in
   `crates/cli/Cargo.toml`.
3. `packages`: a `.deb`, `.rpm` and Alpine `.apk` for each CPU from the musl archives, with
   nfpm (`scripts/linux-packages.sh`, which runs the same on a machine with nfpm).
4. `github`, once those are done: the GitHub Release of the tag, with the archives, the packages, `SHA256SUMS`,
   `install.sh` and the changelog's section as the notes. The site serves the same
   `site/public/install.sh` at <https://osdlc.github.io/sitroom/install.sh>; it installs the latest
   release unless given `--version`, so it needs no change for a new one.
5. `homebrew`: writes `Formula/sitroom.rb` in osdlc/homebrew-tap for those archives and pushes
   it as the osdlc-release GitHub App, the way osdlc/skillwright updates its formula.

`cargo publish` goes in dependency order and waits for each crate to appear in the index before
the next. A version can't be replaced once published, only yanked. crates.io limits how many
*new* crates an account creates in a short time, so adding a crate may stop a publish partway
with `429 Too Many Requests`: wait until the time it gives, then run `cargo publish -p <crate>`
for the rest. Run the workflow by hand (Actions, release, Run workflow) for a dry run: it
builds the four binaries too, and uploads nothing.

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

### Publishing from a machine instead

The crates can be published by hand, the first release included; then no token goes in GitHub.
It needs Rust, pnpm and a few GB free for the build. From a clean checkout of `main` at the
release commit:

```sh
cargo login                  # a token with publish-new and publish-update
scripts/crates.sh dry-run    # builds the UI into crates/web/ui, packs and builds every crate
scripts/crates.sh publish
git tag v0.9.0 && git push origin v0.9.0
```

`scripts/crates.sh` is what CI and the release workflow run too: it finds the crates to publish
(every one without `publish = false`), builds the web UI and puts it in `crates/web/ui`, checks
nothing else changed, runs cargo, and removes `crates/web/ui` again. `scripts/crates.sh status`
says whether the version is on crates.io.

Then push the tag as usual. The workflow's `publish` job sees the version is on crates.io
already and skips the upload; the binaries, the GitHub Release and the formula go ahead. (If
only some crates made it, it stops: publish the rest by hand, then re-run it.)
