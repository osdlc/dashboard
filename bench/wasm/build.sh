#!/usr/bin/env bash
# Builds the WASI plugins (clock, textfile) and precompiles them for wasmtime
# (`.cwasm`, loaded with --allow-precompiled). Needs the wasm32-wasip1 target
# (`rustup target add wasm32-wasip1`) and wasmtime.
set -euo pipefail
cd "$(dirname "$0")"

# Through rustup: a cargo or rustc from elsewhere (Homebrew) has no WASI std.
cargo=(cargo)
if command -v rustup >/dev/null; then
    toolchain=${RUSTUP_TOOLCHAIN:-stable}
    RUSTC=$(rustup which --toolchain "$toolchain" rustc)
    export RUSTC
    cargo=(rustup run "$toolchain" "$(rustup which --toolchain "$toolchain" cargo)")
fi
"${cargo[@]}" build --release --target wasm32-wasip1
for w in clock textfile; do
    wasmtime compile "target/wasm32-wasip1/release/$w.wasm" -o "target/wasm32-wasip1/release/$w.cwasm"
done
ls -l target/wasm32-wasip1/release/*.wasm target/wasm32-wasip1/release/*.cwasm
