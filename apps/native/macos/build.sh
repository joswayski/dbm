#!/usr/bin/env bash
set -euo pipefail
if [[ "$(uname -s)" != Darwin ]]; then
  echo "The AppKit host must be built on macOS with Xcode Command Line Tools." >&2
  exit 1
fi
root="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
output="$root/target/native/DBM Native.app"
export MACOSX_DEPLOYMENT_TARGET=13.0
cargo build --manifest-path "$root/Cargo.toml" --target-dir "$root/target" --locked --release -p dbm-native-bridge
mkdir -p "$output/Contents/MacOS"
xcrun swiftc -swift-version 5 -O -framework AppKit -target "$(uname -m)-apple-macosx13.0" \
  "$root/apps/native/macos/Sources/Bridge.swift" \
  "$root/apps/native/macos/Sources/main.swift" \
  -o "$output/Contents/MacOS/DBMNative"
cp "$root/target/release/dbm-native-bridge" "$output/Contents/MacOS/"
cp "$root/apps/native/macos/Info.plist" "$output/Contents/Info.plist"
codesign --force --sign - "$output/Contents/MacOS/dbm-native-bridge"
codesign --force --sign - "$output"
printf 'Built development preview (not installed): %s\n' "$output"
