#!/usr/bin/env bash
set -euo pipefail
if [[ "$(uname -s)" != Darwin ]]; then
  echo "The AppKit host must be built on macOS with Xcode Command Line Tools." >&2
  exit 1
fi
root="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
output="$root/target/native/DBM.app"
export MACOSX_DEPLOYMENT_TARGET=13.0
# DBM_UNIVERSAL=1 (release builds) produces one bundle for Apple Silicon and
# Intel; development builds target only this Mac's architecture.
if [[ "${DBM_UNIVERSAL:-}" == 1 ]]; then
  arches=(arm64 x86_64)
else
  arches=("$(uname -m)")
fi
rm -rf "$output"
mkdir -p "$output/Contents/MacOS" "$output/Contents/Frameworks" "$output/Contents/Resources"
dylibs=()
executables=()
for arch in "${arches[@]}"; do
  triple="${arch/arm64/aarch64}-apple-darwin"
  cargo build --manifest-path "$root/Cargo.toml" --target-dir "$root/target" --locked --release \
    -p dbm-native-bridge --target "$triple"
  dylib_dir="$root/target/$triple/release"
  xcrun install_name_tool -id @rpath/libdbm_native_bridge.dylib "$dylib_dir/libdbm_native_bridge.dylib"
  xcrun swiftc -swift-version 5 -O -framework AppKit -target "$arch-apple-macosx13.0" \
    -import-objc-header "$root/apps/native/bridge/include/dbm_bridge.h" \
    -L "$dylib_dir" -ldbm_native_bridge \
    -Xlinker -rpath -Xlinker @executable_path/../Frameworks \
    "$root"/apps/native/macos/Sources/*.swift \
    -o "$dylib_dir/dbm"
  dylibs+=("$dylib_dir/libdbm_native_bridge.dylib")
  executables+=("$dylib_dir/dbm")
done
xcrun lipo -create "${dylibs[@]}" -output "$output/Contents/Frameworks/libdbm_native_bridge.dylib"
xcrun lipo -create "${executables[@]}" -output "$output/Contents/MacOS/dbm"
cp "$root/apps/native/macos/Info.plist" "$output/Contents/Info.plist"
cp "$root/apps/native/icons/icon.icns" "$output/Contents/Resources/icon.icns"
# Geist is bundled (OFL 1.1) and registered at launch; nothing is fetched.
cp "$root"/apps/native/workbench/assets/geist*.ttf "$root"/apps/native/workbench/assets/*-LICENSE "$output/Contents/Resources/"
# Release builds carry their number in the bundle version (DBM_NATIVE_BUILD is
# also compiled into the bridge for the updater).
if [[ -n "${DBM_NATIVE_BUILD:-}" ]]; then
  /usr/libexec/PlistBuddy -c "Set :CFBundleVersion ${DBM_NATIVE_BUILD}" "$output/Contents/Info.plist"
fi
# Release builds also carry the app version (2026.9.2802) and the release
# name shown in the update control (2026.09.28.2).
if [[ -n "${DBM_APP_VERSION:-}" ]]; then
  /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString ${DBM_APP_VERSION}" "$output/Contents/Info.plist"
fi
if [[ -n "${DBM_NATIVE_VERSION:-}" ]]; then
  /usr/libexec/PlistBuddy -c "Add :DBMVersion string ${DBM_NATIVE_VERSION}" "$output/Contents/Info.plist"
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
  printf 'Built development app (not installed): %s\n' "$output"
fi
