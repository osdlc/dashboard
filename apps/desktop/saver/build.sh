#!/bin/sh
# Build Dashboard.saver into target/saver/. Not signed.
# Install it yourself by double-clicking it, or copy it to ~/Library/Screen Savers.
set -e
here=$(cd "$(dirname "$0")" && pwd)
out="$here/../../../target/saver/Dashboard.saver"
rm -rf "$out"
mkdir -p "$out/Contents/MacOS"
cp "$here/Info.plist" "$out/Contents/Info.plist"
clang -fobjc-arc -bundle -arch arm64 -arch x86_64 -mmacosx-version-min=11.0 \
  -framework ScreenSaver -framework WebKit -framework Cocoa \
  -o "$out/Contents/MacOS/DashboardSaver" "$here/DashboardSaver.m"
echo "$out"
