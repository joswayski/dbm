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
# Channel builds carry their number in the bundle version (DBM_NATIVE_BUILD is
# also compiled into the bridge for the updater).
if [[ -n "${DBM_NATIVE_BUILD:-}" ]]; then
  /usr/libexec/PlistBuddy -c "Set :CFBundleVersion ${DBM_NATIVE_BUILD}" "$output/Contents/Info.plist"
  /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString 0.1.${DBM_NATIVE_BUILD}" "$output/Contents/Info.plist"
fi
# Release builds sign with the Developer ID and the hardened runtime, which
# notarization requires; development builds are signed ad hoc.
if [[ -n "${DBM_SIGNING_IDENTITY:-}" ]]; then
  sign=(codesign --force --timestamp --options runtime --sign "$DBM_SIGNING_IDENTITY")
else
  sign=(codesign --force --sign -)
fi
"${sign[@]}" "$output/Contents/Frameworks/libdbm_native_bridge.dylib"
"${sign[@]}" "$output"
if [[ -n "${DBM_SIGNING_IDENTITY:-}" ]]; then
  printf 'Built signed release: %s\n' "$output"
else
  printf 'Built development preview (not installed): %s\n' "$output"
fi
