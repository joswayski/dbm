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
rm -rf "$output"
mkdir -p "$output/Contents/MacOS" "$output/Contents/Frameworks" "$output/Contents/Resources"
cp "$root/target/release/libdbm_native_bridge.dylib" "$output/Contents/Frameworks/"
xcrun install_name_tool -id @rpath/libdbm_native_bridge.dylib "$output/Contents/Frameworks/libdbm_native_bridge.dylib"
xcrun swiftc -swift-version 5 -O -framework AppKit -target "$(uname -m)-apple-macosx13.0" \
  -import-objc-header "$root/apps/native/bridge/include/dbm_bridge.h" \
  -L "$output/Contents/Frameworks" -ldbm_native_bridge \
  -Xlinker -rpath -Xlinker @executable_path/../Frameworks \
  "$root"/apps/native/macos/Sources/*.swift \
  -o "$output/Contents/MacOS/DBMNative"
cp "$root/apps/native/macos/Info.plist" "$output/Contents/Info.plist"
# Geist is bundled (OFL 1.1) and registered at launch; nothing is fetched.
cp "$root"/apps/native/workbench/assets/geist*.ttf "$root"/apps/native/workbench/assets/*-LICENSE "$output/Contents/Resources/"
codesign --force --sign - "$output/Contents/Frameworks/libdbm_native_bridge.dylib"
codesign --force --sign - "$output"
printf 'Built development preview (not installed): %s\n' "$output"
