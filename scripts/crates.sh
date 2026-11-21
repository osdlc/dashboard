#!/usr/bin/env bash
# Packs and publishes the crates that go to crates.io, the ones without
# `publish = false` (read from cargo metadata, so a new crate needs nothing
# here). Run from anywhere: scripts/crates.sh <command>
#
#   package   build the web UI into crates/web/ui, then `cargo package` each
#             crate: packs it and builds it from the packed copy (CI)
#   dry-run   the same, through `cargo publish --dry-run`
#   publish   the same, then upload (needs `cargo login` or
#             CARGO_REGISTRY_TOKEN); skips a version already published
#   status    prints `published`, `unpublished` or, with only some crates
#             at the version, says which and exits 1
#
# The published sitroom-web has no web/ beside it, so it carries the built
# UI in ui/ (docs/releasing.md). That folder is git-ignored, which cargo
# counts as uncommitted: hence --allow-dirty, and the check that nothing
# else changed. ui/ is removed again on exit.
#
# Set SITROOM_UI_BUILT=1 when web/dist is already built (CI's web job).
set -euo pipefail
cd "$(dirname "$0")/.."

command="${1:-}"
case "$command" in
    package | dry-run | publish | status) ;;
    *)
        sed -n '2,13s/^# \{0,1\}//p' "$0" >&2
        exit 2
        ;;
esac

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
# cargo metadata says `"publish": []` for publish = false.
mapfile -t crates < <(cargo metadata --no-deps --format-version 1 | node -e '
    let s = ""; process.stdin.on("data", d => s += d).on("end", () => {
        for (const p of JSON.parse(s).packages)
            if (!(Array.isArray(p.publish) && p.publish.length === 0)) console.log(p.name)
    })')
packages=()
for c in "${crates[@]}"; do packages+=(-p "$c"); done

# How many of the crates are at $version on crates.io.
published_count() {
    local n=0 code
    for c in "${crates[@]}"; do
        code=$(curl -s -o /dev/null -w '%{http_code}' -A "osdlc/sitroom scripts/crates.sh" \
            "https://crates.io/api/v1/crates/$c/$version")
        [ "$code" = 200 ] && n=$((n + 1))
    done
    echo "$n"
}

status() {
    local n
    n=$(published_count)
    if [ "$n" = "${#crates[@]}" ]; then
        echo published
    elif [ "$n" = 0 ]; then
        echo unpublished
    else
        echo "$n of ${#crates[@]} crates are at $version on crates.io: publish the rest" >&2
        return 1
    fi
}

if [ "$command" = status ]; then
    status
    exit
fi

if [ "$command" = publish ]; then
    state=$(status) || exit 1
    if [ "$state" = published ]; then
        echo "every crate is at $version on crates.io already; nothing to publish"
        exit 0
    fi
fi

if [ -n "$(git status --porcelain)" ]; then
    echo "uncommitted changes; ${command} needs a clean checkout:" >&2
    git status --short >&2
    exit 1
fi

trap 'rm -rf crates/web/ui' EXIT
if [ -z "${SITROOM_UI_BUILT:-}" ]; then
    (cd web && pnpm install --frozen-lockfile && pnpm build)
fi
rm -rf crates/web/ui
cp -r web/dist crates/web/ui

# Only the git-ignored ui/ may be new: the UI build must not touch tracked files.
if [ -n "$(git status --porcelain)" ]; then
    echo "the UI build changed the checkout:" >&2
    git status --short >&2
    exit 1
fi

echo "== ${command} ${crates[*]} at $version"
case "$command" in
    package) cargo package --locked --allow-dirty "${packages[@]}" ;;
    dry-run) cargo publish --locked --allow-dirty --dry-run "${packages[@]}" ;;
    publish) cargo publish --locked --allow-dirty "${packages[@]}" ;;
esac
