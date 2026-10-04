#!/usr/bin/env bash
# Build the in-process Rust core and generate the Xcode project for one SDK.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$ROOT/../../.." && pwd)"
PLATFORM="${1:-iphonesimulator}"
XCODEGEN_COMMIT=21ac9944b0ab546a07422dbed86f33dd2ebd76f8
XCODEGEN="$ROOT/.build/xcodegen-$XCODEGEN_COMMIT"
export DBM_IOS_BUNDLE_ID="${DBM_IOS_BUNDLE_ID:-app.dbm.ios}"

case "$PLATFORM" in
  iphonesimulator) rust_target=aarch64-apple-ios-sim ;;
  iphoneos) rust_target=aarch64-apple-ios ;;
  *) echo "Usage: $0 iphonesimulator|iphoneos" >&2; exit 2 ;;
esac
command -v cargo >/dev/null && command -v swift >/dev/null && command -v xcodebuild >/dev/null
rustup target add "$rust_target"
cargo build --manifest-path "$REPO/apps/native/bridge/Cargo.toml" --locked --release --no-default-features --target "$rust_target"
mkdir -p "$ROOT/.build/$PLATFORM"
cp "$REPO/target/$rust_target/release/libdbm_native_bridge.a" "$ROOT/.build/$PLATFORM/"

if [[ ! -d "$XCODEGEN/.git" ]]; then git clone --quiet https://github.com/yonaskolb/XcodeGen.git "$XCODEGEN"; fi
git -C "$XCODEGEN" checkout --quiet --detach "$XCODEGEN_COMMIT"
test "$(git -C "$XCODEGEN" rev-parse HEAD)" = "$XCODEGEN_COMMIT"
swift run --package-path "$XCODEGEN" xcodegen --spec "$ROOT/project.yml" --project "$ROOT"
