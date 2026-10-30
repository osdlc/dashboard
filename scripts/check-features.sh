#!/usr/bin/env bash
# Builds the dashboard binary with no built-in modules, with each bundle on
# its own, and with each module on its own, so a bundle never depends on
# another by accident, and runs the tests for no modules and for each bundle.
# Run from anywhere: scripts/check-features.sh
# Pass --bundles to skip the per-module builds (faster).
set -euo pipefail
cd "$(dirname "$0")/.."

bundles=(system dev ops fun agents personal)
# Module features, read from crates/cli/Cargo.toml.
modules=$(sed -n 's/^\(module-[a-z0-9-]*\) = .*/\1/p' crates/cli/Cargo.toml)

# Its own target dir: the changed RUSTFLAGS would otherwise rebuild target/ from scratch.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/check-features}"

build() {
    echo "== ${*:-no modules}"
    # -D warnings: a combination that leaves helpers unused should still build clean.
    RUSTFLAGS="${RUSTFLAGS:-} -D warnings" cargo build --quiet -p dashboard --no-default-features "$@"
}

# Also runs the tests that depend on which modules are in the build.
test_build() {
    echo "== test ${*:-no modules}"
    RUSTFLAGS="${RUSTFLAGS:-} -D warnings" cargo test --quiet -p dashboard -p dashboard-modules --no-default-features "$@"
}

build
test_build
for b in "${bundles[@]}"; do
    build --features "bundle-$b"
    test_build --features "bundle-$b"
done
if [[ "${1:-}" != "--bundles" ]]; then
    for m in $modules; do
        build --features "$m"
    done
fi
echo "all feature builds passed"
