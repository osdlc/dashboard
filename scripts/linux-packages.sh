#!/usr/bin/env bash
# Builds the Linux packages (.deb, .rpm and Alpine .apk) from a release
# archive's folder, with nfpm (https://nfpm.goreleaser.com).
#
#   scripts/linux-packages.sh <archive folder> <out dir>
#
# The archive folder is an unpacked sitroom-<version>-<target>, holding
# sitroom, sit, README.md and the licence files; the release uses the static
# musl builds, so the packages depend on nothing. The version and CPU come
# from the folder's name. Needs nfpm on PATH, or NFPM set to its path.
set -euo pipefail

src="${1:?usage: scripts/linux-packages.sh <archive folder> <out dir>}"
out="${2:?usage: scripts/linux-packages.sh <archive folder> <out dir>}"
nfpm="${NFPM:-nfpm}"

base="$(basename "$src")"
if [[ ! "$base" =~ ^sitroom-([0-9][^-]*)-(x86_64|aarch64)-unknown-linux-(gnu|musl)$ ]]; then
    echo "linux-packages.sh: $base is not a sitroom-<version>-<cpu>-unknown-linux-<libc> folder" >&2
    exit 2
fi
version="${BASH_REMATCH[1]}"
case "${BASH_REMATCH[2]}" in
    x86_64) arch=amd64 ;;
    aarch64) arch=arm64 ;;
esac
src="$(cd "$src" && pwd)"
for f in sitroom sit README.md LICENSE-MIT LICENSE-APACHE; do
    [[ -f "$src/$f" ]] || { echo "linux-packages.sh: $src has no $f" >&2; exit 1; }
done

mkdir -p "$out"
config="$(mktemp)"
trap 'rm -f "$config"' EXIT
cat > "$config" <<YAML
name: sitroom
version: "$version"
arch: $arch
platform: linux
section: utils
priority: optional
maintainer: osdlc <https://github.com/osdlc>
description: |
  Modular developer dashboard with a terminal UI and a web UI.
  Installs sitroom and sit, its short name.
vendor: osdlc
homepage: https://osdlc.github.io/sitroom/
license: MIT OR Apache-2.0
contents:
  - src: $src/sitroom
    dst: /usr/bin/sitroom
    file_info: { mode: 0755 }
  - src: $src/sit
    dst: /usr/bin/sit
    file_info: { mode: 0755 }
  - src: $src/README.md
    dst: /usr/share/doc/sitroom/README.md
    file_info: { mode: 0644 }
  - src: $src/LICENSE-MIT
    dst: /usr/share/doc/sitroom/LICENSE-MIT
    file_info: { mode: 0644 }
  - src: $src/LICENSE-APACHE
    dst: /usr/share/doc/sitroom/LICENSE-APACHE
    file_info: { mode: 0644 }
YAML

for packager in deb rpm apk; do
    "$nfpm" package --config "$config" --packager "$packager" --target "$out/" >&2
done
ls "$out"
