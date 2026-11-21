#!/bin/sh
# Installs sitroom (and `sit`, its short name) from a GitHub Release of
# osdlc/sitroom.
#
#   curl -fsSL https://osdlc.github.io/sitroom/install.sh | sh
#   curl -fsSL https://osdlc.github.io/sitroom/install.sh | sh -s -- --version 0.9.0 --to /usr/local/bin
#   sh install.sh [--version <X.Y.Z>] [--to <dir>]
#
# It picks the archive for this OS and CPU (macOS or Linux, arm64 or x86_64;
# on Linux the static musl build when there is no glibc 2.35 or later), checks it against the release's SHA256SUMS and installs `sitroom` and `sit`
# in <dir>, default ~/.local/bin. Without --version it installs the latest
# release. The site serves this file from site/public/install.sh, and each
# release attaches a copy.
#
# SITROOM_DOWNLOAD_BASE overrides the download URL up to the tag, for tests.
set -eu

REPO='osdlc/sitroom'

version=''
to="${HOME}/.local/bin"

say() { printf '%s\n' "install.sh: $*" >&2; }
die() { say "$*"; exit 1; }

usage() {
  cat <<'USAGE'
Installs sitroom and sit from a GitHub Release of osdlc/sitroom.

  sh install.sh [--version <X.Y.Z>] [--to <dir>]

Picks the build for this OS and CPU (macOS or Linux, arm64 or x86_64), checks
it against the release's SHA256SUMS and installs <dir>/sitroom and <dir>/sit.
--to defaults to ~/.local/bin. Without --version it installs the latest release.
USAGE
}

while [ $# -gt 0 ]; do
  case "$1" in
    --version) [ $# -ge 2 ] || die '--version needs a value'; version="${2#v}"; shift 2 ;;
    --to) [ $# -ge 2 ] || die '--to needs a value'; to="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown argument: $1 (see --help)" ;;
  esac
done

fetch() { # url dest
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --retry 3 -o "$2" "$1"
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O "$2" "$1"
  else
    die 'needs curl or wget'
  fi
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  else
    die 'needs sha256sum or shasum to check the download'
  fi
}

case "$(uname -s)" in
  Darwin) os='apple-darwin' ;;
  Linux) os='linux' ;;
  *) die "no build for $(uname -s); see https://github.com/${REPO}#build-from-source" ;;
esac
case "$(uname -m)" in
  arm64|aarch64) cpu='aarch64' ;;
  x86_64|amd64) cpu='x86_64' ;;
  *) die "no build for CPU $(uname -m); see https://github.com/${REPO}#build-from-source" ;;
esac

# Linux: the glibc build needs glibc 2.35 or later (it is built on Ubuntu
# 22.04); anything else (musl, as on Alpine, or an older glibc) gets the
# static musl build.
libc='gnu'
if [ "$os" = linux ]; then
  glibc="$(getconf GNU_LIBC_VERSION 2>/dev/null | sed -n 's/^glibc \([0-9]*\)\.\([0-9]*\).*/\1 \2/p')"
  if [ -z "$glibc" ]; then
    libc='musl'
  else
    # shellcheck disable=SC2086 # split "2 35" into two numbers
    set -- $glibc
    if [ "$1" -lt 2 ] || { [ "$1" -eq 2 ] && [ "$2" -lt 35 ]; }; then libc='musl'; fi
  fi
  os="unknown-linux-$libc"
fi

tmp="$(mktemp -d 2>/dev/null || mktemp -d -t sitroom)"
trap 'rm -rf "$tmp"' EXIT INT TERM

download_base="${SITROOM_DOWNLOAD_BASE:-https://github.com/${REPO}/releases/download}"

if [ -z "$version" ]; then
  # The latest release that isn't a pre-release or a draft.
  fetch "https://api.github.com/repos/${REPO}/releases/latest" "$tmp/latest.json" \
    || die "couldn't ask GitHub for the latest release; pass --version"
  version="$(sed -n 's/.*"tag_name": *"v\([^"]*\)".*/\1/p' "$tmp/latest.json" | head -n 1)"
  [ -n "$version" ] || die "found no release; pass --version"
fi

base="${download_base}/v${version}"
fetch "${base}/SHA256SUMS" "$tmp/SHA256SUMS" || die "couldn't download ${base}/SHA256SUMS"

# Releases before the musl builds (0.9.0) have only glibc ones.
target="${cpu}-${os}"
if [ "$libc" = musl ] && ! grep -q "sitroom-${version}-${target}.tar.gz" "$tmp/SHA256SUMS"; then
  say "release ${version} has no static build; using the glibc one, which needs glibc 2.35"
  target="${cpu}-unknown-linux-gnu"
fi
name="sitroom-${version}-${target}"
asset="${name}.tar.gz"

say "downloading ${asset}"
fetch "${base}/${asset}" "$tmp/$asset" || die "couldn't download ${base}/${asset}"

want="$(awk -v f="$asset" '$2 == f || $2 == "*" f { print $1 }' "$tmp/SHA256SUMS")"
[ -n "$want" ] || die "${asset} is not listed in SHA256SUMS"
got="$(sha256 "$tmp/$asset")"
[ "$want" = "$got" ] || die "checksum mismatch for ${asset}: expected ${want}, got ${got}"

tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$to"
for bin in sitroom sit; do
  [ -f "$tmp/$name/$bin" ] || die "${asset} has no ${bin}"
  chmod 755 "$tmp/$name/$bin"
  mv -f "$tmp/$name/$bin" "$to/$bin"
done
say "installed ${to}/sitroom and ${to}/sit (${version})"

case ":${PATH}:" in
  *":${to}:"*) say "run: sitroom tui (or sit tui)" ;;
  *) say "${to} is not on PATH; add it, then run: sitroom tui" ;;
esac
