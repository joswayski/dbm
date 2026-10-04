# DBM for iPhone

Native SwiftUI client for iOS 17+. It invokes `dbm-native-bridge` in-process: no HTTP server, web view, telemetry, or cloud sync. Profile metadata and history use a protected SQLite file under Application Support (excluded from iCloud backup). Passwords are passed to Rust only for the current session and are cleared when the form/session closes.

Mobile connections are deliberately **read-only**, require TLS with Apple platform CA trust, and cap query results at 1,000 rows. Mutation, export and updater commands are not exposed by this app.

## Build on macOS

Prerequisites: an Apple Silicon Mac, Xcode with iOS 17+ SDK, Swift, Rust/rustup. Tests also need an installed iOS Simulator runtime and an iPhone 16 simulator. Set `DBM_IOS_TEST_DESTINATION` to an Xcode destination specifier to use another installed iPhone. `build.sh` builds the Rust static library with `--no-default-features`, fetches XcodeGen at pinned commit `21ac9944b0ab546a07422dbed86f33dd2ebd76f8`, and generates `DBM.xcodeproj`.

```sh
bash apps/native/ios/build.sh simulator
bash apps/native/ios/build.sh test
DBM_IOS_BUNDLE_ID=com.example.dbm DEVELOPMENT_TEAM=TEAMID bash apps/native/ios/build.sh device
```

Install a matching Simulator runtime through Xcode's Components settings or `xcodebuild -downloadPlatform iOS -buildVersion "$(xcrun --sdk iphonesimulator --show-sdk-version)"`, then create/select a simulator in Xcode. CI provisions an SDK-matched runtime and a dedicated test simulator; it does not depend on preinstalled device names.

For the explicitly labelled, Debug-only Rust `DemoStore` launch, set the `DBM_DEMO=1` environment variable in the test plan or scheme. Release builds ignore it. The UI test does this and retains an `iphone-connection-form` screenshot in its `.xcresult`.

The device workflow requires a valid signing team/profile; it does not install the resulting app automatically.

## TestFlight

`upload-testflight.sh` follows Caper's cloud-managed signing: it prepares the
device Rust library/project, archives a **Release** app, and uploads through
App Store Connect. It needs Xcode 26, `APPLE_TEAM_ID`, `NOTARY_KEY_PATH` (the
API `.p8` file), `NOTARY_KEY_ID`, `NOTARY_ISSUER`, and a unique increasing
`BUILD_NUMBER` in `<run number>.<attempt>` form. `DBM_IOS_BUNDLE_ID` defaults to
`app.dbm.ios`; keep it consistent with the registered Apple app. Release builds
ignore `DBM_DEMO`.

The Mobile release workflow fetches those inputs from the same AWS signing
secret as Caper, then waits for processing and assigns tester groups. No
distribution certificate/profile is stored in Git. Register the DBM app and
TestFlight group before uploading; see [one-time setup](../../../docs/mobile.md#one-time-setup-before-the-first-release).
