# DBM for iPhone

Native SwiftUI client for iOS 17+. It invokes `dbm-native-bridge` in-process: no HTTP server, web view, telemetry, or cloud sync. Profile metadata and history use a protected SQLite file under Application Support (excluded from iCloud backup). Passwords are passed to Rust only for the current session and are cleared when the form/session closes.

Mobile connections are deliberately **read-only**, require TLS with Apple platform CA trust, and cap query results at 1,000 rows. Mutation, export and updater commands are not exposed by this app.

## Build on macOS

Prerequisites: Xcode with iOS 17+ SDK, Swift, Rust/rustup. `build.sh` builds the Rust static library with `--no-default-features`, fetches XcodeGen at pinned commit `21ac9944b0ab546a07422dbed86f33dd2ebd76f8`, and generates `DBM.xcodeproj`.

```sh
bash apps/native/ios/build.sh simulator
bash apps/native/ios/build.sh test
DBM_IOS_BUNDLE_ID=com.example.dbm DEVELOPMENT_TEAM=TEAMID bash apps/native/ios/build.sh device
```

For the explicitly labelled, Debug-only Rust `DemoStore` launch, set the `DBM_DEMO=1` environment variable in the test plan or scheme. Release builds ignore it. The UI test does this and retains an `iphone-connection-form` screenshot in its `.xcresult`.

The device workflow requires a valid signing team/profile; it does not install the resulting app automatically.
