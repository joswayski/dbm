#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$ROOT/../../.." && pwd)"
MODE="${1:-simulator}"
XCODEGEN_COMMIT=21ac9944b0ab546a07422dbed86f33dd2ebd76f8
XCODEGEN="$ROOT/.build/xcodegen-$XCODEGEN_COMMIT"
export DBM_IOS_BUNDLE_ID="${DBM_IOS_BUNDLE_ID:-app.dbm.ios}"

case "$MODE" in simulator|test) rust_target=aarch64-apple-ios-sim; platform=iphonesimulator ;; device) rust_target=aarch64-apple-ios; platform=iphoneos ;; *) echo "Usage: $0 simulator|test|device" >&2; exit 2 ;; esac
command -v cargo >/dev/null && command -v swift >/dev/null && command -v xcodebuild >/dev/null
rustup target add "$rust_target"
cargo build --manifest-path "$REPO/apps/native/bridge/Cargo.toml" --locked --release --no-default-features --target "$rust_target"
mkdir -p "$ROOT/.build/$platform"
cp "$REPO/target/$rust_target/release/libdbm_native_bridge.a" "$ROOT/.build/$platform/"

if [[ ! -d "$XCODEGEN/.git" ]]; then git clone --quiet https://github.com/yonaskolb/XcodeGen.git "$XCODEGEN"; fi
git -C "$XCODEGEN" checkout --quiet --detach "$XCODEGEN_COMMIT"
test "$(git -C "$XCODEGEN" rev-parse HEAD)" = "$XCODEGEN_COMMIT"
swift run --package-path "$XCODEGEN" xcodegen --spec "$ROOT/project.yml" --project "$ROOT"

if [[ "$MODE" == test ]]; then
  rm -rf "$ROOT/DerivedData-Tests/DBM.xcresult"
  xcodebuild -project "$ROOT/DBM.xcodeproj" -scheme DBM -configuration Debug -destination "${DBM_IOS_TEST_DESTINATION:-platform=iOS Simulator,name=iPhone 16,OS=latest}" -derivedDataPath "$ROOT/DerivedData-Tests" -resultBundlePath "$ROOT/DerivedData-Tests/DBM.xcresult" DBM_IOS_BUNDLE_ID="$DBM_IOS_BUNDLE_ID" CODE_SIGNING_ALLOWED=NO test
elif [[ "$MODE" == simulator ]]; then
  xcodebuild -project "$ROOT/DBM.xcodeproj" -scheme DBM -configuration Debug -destination 'generic/platform=iOS Simulator' -derivedDataPath "$ROOT/DerivedData" DBM_IOS_BUNDLE_ID="$DBM_IOS_BUNDLE_ID" ARCHS=arm64 ONLY_ACTIVE_ARCH=YES CODE_SIGNING_ALLOWED=NO build
else
  : "${DEVELOPMENT_TEAM:?Device builds require your Apple development team ID}"
  xcodebuild -project "$ROOT/DBM.xcodeproj" -scheme DBM -configuration Debug -destination 'generic/platform=iOS' -derivedDataPath "$ROOT/DerivedData-Device" DBM_IOS_BUNDLE_ID="$DBM_IOS_BUNDLE_ID" DEVELOPMENT_TEAM="$DEVELOPMENT_TEAM" ARCHS=arm64 ONLY_ACTIVE_ARCH=YES build
fi
