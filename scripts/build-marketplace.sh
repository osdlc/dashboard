#!/usr/bin/env bash
# Builds the sample marketplace: packs every package in plugins/ext into
# marketplace/packages/<id>-<version>.tar.gz and lists them, with their
# sha256, in marketplace/index.json. Both are build output (gitignored).
# Run from anywhere: scripts/build-marketplace.sh
# Then: dashboard plugin install clock-py --index marketplace/index.json
set -euo pipefail
cd "$(dirname "$0")/.."

rm -rf marketplace/index.json marketplace/packages
dirs=()
for dir in plugins/ext/*/; do
  [[ -f "$dir/dashboard-plugin.yaml" ]] && dirs+=("${dir%/}")
done
cargo run --quiet -p dashboard -- plugin pack "${dirs[@]}" --out marketplace
