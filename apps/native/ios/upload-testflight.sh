#!/usr/bin/env bash
# Caper's cloud-managed signing path: archive, then upload via App Store Connect.
# No Apple Distribution certificate or provisioning profile is stored in DBM.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
export DBM_IOS_BUNDLE_ID="${DBM_IOS_BUNDLE_ID:-app.dbm.ios}"
: "${APPLE_TEAM_ID:?}" "${NOTARY_KEY_PATH:?}" "${NOTARY_KEY_ID:?}" "${NOTARY_ISSUER:?}" "${BUILD_NUMBER:?}"
[[ "$BUILD_NUMBER" =~ ^[1-9][0-9]*\.[1-9][0-9]*$ ]] || { echo "BUILD_NUMBER must be <run number>.<attempt>." >&2; exit 2; }
[[ -s "$NOTARY_KEY_PATH" ]] || { echo "App Store Connect key file is missing." >&2; exit 2; }

auth=(
  -allowProvisioningUpdates
  -authenticationKeyPath "$NOTARY_KEY_PATH"
  -authenticationKeyID "$NOTARY_KEY_ID"
  -authenticationKeyIssuerID "$NOTARY_ISSUER"
)
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

bash "$ROOT/prepare.sh" iphoneos
xcodebuild -project "$ROOT/DBM.xcodeproj" -scheme DBM -configuration Release \
  -destination 'generic/platform=iOS' -archivePath "$work/DBM.xcarchive" \
  -derivedDataPath "$ROOT/DerivedData-TestFlight" \
  DEVELOPMENT_TEAM="$APPLE_TEAM_ID" DBM_IOS_BUNDLE_ID="$DBM_IOS_BUNDLE_ID" \
  CURRENT_PROJECT_VERSION="$BUILD_NUMBER" "${auth[@]}" archive

# plistlib handles escaping rather than interpolating values into XML.
python3 - "$work/ExportOptions.plist" <<'PY'
import os, plistlib, sys
with open(sys.argv[1], "wb") as output:
    plistlib.dump({
        "method": "app-store-connect", "destination": "upload",
        "teamID": os.environ["APPLE_TEAM_ID"], "signingStyle": "automatic",
        "manageAppVersionAndBuildNumber": False, "uploadSymbols": True,
    }, output)
PY
xcodebuild -exportArchive -archivePath "$work/DBM.xcarchive" \
  -exportOptionsPlist "$work/ExportOptions.plist" -exportPath "$work/export" "${auth[@]}"
echo "Uploaded $DBM_IOS_BUNDLE_ID build $BUILD_NUMBER; Apple processing and tester delivery follow."
