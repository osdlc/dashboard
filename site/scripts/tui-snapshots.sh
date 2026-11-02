#!/bin/sh
# Renders demo screens of the terminal UI as text into src/data/tui/, using the
# TUI's own size-preview test (crates/tui/src/size_tests.rs). Run from site/:
#   sh scripts/tui-snapshots.sh            # default 120x34
#   SIZE=120x34 sh scripts/tui-snapshots.sh
set -eu
cd "$(dirname "$0")/.."
out=src/data/tui
mkdir -p "$out"
size=${SIZE:-120x34}
for screen in showcase meters lists logs; do
  (cd .. && DASHBOARD_SKIP_UI_BUILD=1 SIZE="$size" SCREEN="$screen" \
    cargo test -q -p dashboard-tui size_preview -- --ignored --nocapture 2>/dev/null) |
    awk '/^running 1 test/ { on = 1; next } on && /^$/ { exit } on' > "$out/$screen.txt"
  echo "$out/$screen.txt"
done
