#!/usr/bin/env bash
# Caper's cloud-managed signing path: archive, then upload via App Store Connect.
# No Apple Distribution certificate or provisioning profile is stored in DBM.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
export DBM_IOS_BUNDLE_ID="${DBM_IOS_BUNDLE_ID:-se.anyba.app}"
: "${APPLE_TEAM_ID:?}" "${NOTARY_KEY_PATH:?}" "${NOTARY_KEY_ID:?}" "${NOTARY_ISSUER:?}" "${BUILD_NUMBER:?}" "${DBM_APP_VERSION:?}"
# CalVer from scripts/mobile_release_version.py: build 2026112203.1 (attempt 1)
# and app version 2026.11.2203, which TestFlight shows as the version.
[[ "$BUILD_NUMBER" =~ ^[1-9][0-9]*\.[1-9][0-9]*$ ]] || { echo "BUILD_NUMBER must be <build>.<attempt>." >&2; exit 2; }
[[ "$DBM_APP_VERSION" =~ ^[1-9][0-9]*\.[1-9][0-9]*\.[1-9][0-9]*$ ]] || { echo "DBM_APP_VERSION must be YEAR.MONTH.DAYNN." >&2; exit 2; }
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
# The intermediate archive gets a local ad-hoc signature, keeping its
# entitlements without issuing an Apple Development certificate on every fresh
# runner (the team's certificate limit stops archives otherwise). Export signs
# it for distribution.
xcodebuild -project "$ROOT/DBM.xcodeproj" -scheme DBM -configuration Release \
  -destination 'generic/platform=iOS' -archivePath "$work/DBM.xcarchive" \
  -derivedDataPath "$ROOT/DerivedData-TestFlight" \
  DEVELOPMENT_TEAM="$APPLE_TEAM_ID" DBM_IOS_BUNDLE_ID="$DBM_IOS_BUNDLE_ID" \
  CURRENT_PROJECT_VERSION="$BUILD_NUMBER" MARKETING_VERSION="$DBM_APP_VERSION" \
  CODE_SIGN_IDENTITY=- AD_HOC_CODE_SIGNING_ALLOWED=YES archive

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
