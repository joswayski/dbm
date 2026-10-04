#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
MODE="${1:-simulator}"
export DBM_IOS_BUNDLE_ID="${DBM_IOS_BUNDLE_ID:-app.dbm.ios}"

case "$MODE" in simulator|test) platform=iphonesimulator ;; device) platform=iphoneos ;; *) echo "Usage: $0 simulator|test|device" >&2; exit 2 ;; esac
bash "$ROOT/prepare.sh" "$platform"

if [[ "$MODE" == test ]]; then
  rm -rf "$ROOT/DerivedData-Tests/DBM.xcresult"
  xcodebuild -project "$ROOT/DBM.xcodeproj" -scheme DBM -configuration Debug -destination "${DBM_IOS_TEST_DESTINATION:-platform=iOS Simulator,name=iPhone 16,OS=latest}" -derivedDataPath "$ROOT/DerivedData-Tests" -resultBundlePath "$ROOT/DerivedData-Tests/DBM.xcresult" DBM_IOS_BUNDLE_ID="$DBM_IOS_BUNDLE_ID" CODE_SIGNING_ALLOWED=NO test
elif [[ "$MODE" == simulator ]]; then
  xcodebuild -project "$ROOT/DBM.xcodeproj" -scheme DBM -configuration Debug -destination 'generic/platform=iOS Simulator' -derivedDataPath "$ROOT/DerivedData" DBM_IOS_BUNDLE_ID="$DBM_IOS_BUNDLE_ID" ARCHS=arm64 ONLY_ACTIVE_ARCH=YES CODE_SIGNING_ALLOWED=NO build
else
  : "${DEVELOPMENT_TEAM:?Device builds require your Apple development team ID}"
  xcodebuild -project "$ROOT/DBM.xcodeproj" -scheme DBM -configuration Debug -destination 'generic/platform=iOS' -derivedDataPath "$ROOT/DerivedData-Device" DBM_IOS_BUNDLE_ID="$DBM_IOS_BUNDLE_ID" DEVELOPMENT_TEAM="$DEVELOPMENT_TEAM" ARCHS=arm64 ONLY_ACTIVE_ARCH=YES build
fi
