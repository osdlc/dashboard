#!/usr/bin/env bash
# The native-vs-plugin benchmark, build side: binary size per module and per
# shared-dependency cluster, clean build time and incremental build time.
# Results go to bench/results/build.tsv (name, value) and to stdout.
#
#   bench/build.sh [sizes] [clean] [incremental] [clean-sccache]
#
# With no part named it runs sizes, clean and incremental. Run it on a quiet
# machine: build times move a lot with other load. The clean builds use their
# own target dir (target/bench-clean), removed after, and no compiler cache.
# clean-sccache times the same clean builds through sccache, for comparison;
# it needs sccache installed.
set -euo pipefail
cd "$(dirname "$0")/.."

out=bench/results/build.tsv
mkdir -p bench/results
parts=("$@")
# A full run starts the file again; a run of some parts adds to it.
if [[ ${#parts[@]} -eq 0 ]]; then
    parts=(sizes clean incremental)
    : >"$out"
fi
# Where the sizes are read from: cargo's target dir, honouring the user's own.
target=${CARGO_TARGET_DIR:-target}

# The eight benchmark widgets, and the ones that pull in the HTTP stack
# (reqwest, rustls, hyper).
widgets=(clock textfile git docker hackernews weather feedreader battery)
http=(hackernews weather feedreader)
# Every module feature, read from crates/cli/Cargo.toml.
all_modules=$(sed -n 's/^\(module-[a-z0-9-]*\) = .*/\1/p' crates/cli/Cargo.toml)

now() { perl -MTime::HiRes=time -e 'printf "%.3f\n", time'; }
record() {
    printf '%s\t%s\n' "$1" "$2" | tee -a "$out"
}
size_of() { stat -f %z "$1" 2>/dev/null || stat -c %s "$1"; }
join() { local IFS=,; echo "$*"; }

# Size of the small-profile binary with exactly these module features.
size() {
    local name=$1 features=$2
    if [[ -z $features ]]; then
        cargo build --quiet --profile small -p dashboard --no-default-features
    else
        cargo build --quiet --profile small -p dashboard --no-default-features --features "$features"
    fi
    record "size.$name" "$(size_of "$target/small/dashboard")"
}

# Seconds for one cargo build.
timed() {
    local name=$1
    shift
    local t0
    t0=$(now)
    cargo build --quiet "$@"
    record "time.$name" "$(echo "$(now) - $t0" | bc)"
}

record "run.date" "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
record "run.load" "$(sysctl -n vm.loadavg 2>/dev/null || cat /proc/loadavg)"
record "run.rustc" "$(rustc --version)"

for part in "${parts[@]}"; do
    case $part in
    sizes)
        size none ""
        for w in "${widgets[@]}"; do
            size "only.$w" "module-$w"
        done
        # The HTTP cluster: all three together, then each one left out, so
        # cluster cost = all three - none - the three per-module costs.
        size "http.all" "$(join "${http[@]/#/module-}")"
        for w in "${http[@]}"; do
            rest=()
            for o in "${http[@]}"; do [[ $o != "$w" ]] && rest+=("module-$o"); done
            size "http.without.$w" "$(join "${rest[@]}")"
        done
        # Each widget left out of the full build: what it adds there.
        size full "$(join $all_modules)"
        for w in "${widgets[@]}"; do
            size "full.without.$w" "$(join $(echo "$all_modules" | grep -vx "module-$w"))"
        done
        ;;
    clean | clean-sccache)
        # In a subshell, so the user's CARGO_TARGET_DIR and RUSTC_WRAPPER
        # come back for the later parts. No compiler cache for "clean": a
        # cached build is not a clean one.
        (
            export CARGO_TARGET_DIR=target/bench-clean
            if [[ $part == clean ]]; then
                export RUSTC_WRAPPER="" name=clean
            else
                export RUSTC_WRAPPER=sccache name=clean-with-sccache
            fi
            rm -rf "$CARGO_TARGET_DIR"
            timed "$name.small" --profile small -p dashboard
            rm -rf "$CARGO_TARGET_DIR"
            timed "$name.debug" -p dashboard
            rm -rf "$CARGO_TARGET_DIR"
        )
        ;;
    incremental)
        # Touch one module file and rebuild, in each profile, with every
        # bundle; then with only that module's bundle.
        files=(
            crates/modules-fun/src/clock.rs
            crates/modules-system/src/textfile.rs
            crates/modules-dev/src/git.rs
        )
        for profile in dev release small; do
            cargo build --quiet --profile "$profile" -p dashboard
            for f in "${files[@]}"; do
                touch "$f"
                timed "incremental.$profile.$(basename "$f" .rs)" --profile "$profile" -p dashboard
            done
        done
        for profile in dev release; do
            cargo build --quiet --profile "$profile" -p dashboard --no-default-features --features bundle-fun
            touch crates/modules-fun/src/clock.rs
            timed "incremental.$profile.clock.bundle-fun-only" \
                --profile "$profile" -p dashboard --no-default-features --features bundle-fun
        done
        ;;
    *)
        echo "unknown part $part (sizes, clean, incremental, clean-sccache)" >&2
        exit 1
        ;;
    esac
done
