# DBM for Android

Native Kotlin/Jetpack Compose database manager backed in-process by DBM's Rust bridge. There is no WebView, local server, account, telemetry, or cloud sync. Profiles and query history use the app-private `dbm-mobile.sqlite3`; credentials exist only in bridge memory and are discarded on disconnect/background/session disposal.

## Requirements and build

- JDK 17, Android SDK/build tools 36
- NDK `27.2.12479018`, CMake 3.22.1
- Rust Android targets, and `cargo-ndk` on `PATH`

```bash
apps/native/android/build.sh
# Instrumented native Compose/JNI demo checks (connected Android device/runner):
apps/native/android/gradlew -p apps/native/android connectedDebugAndroidTest
```

`build.sh` invokes `cargo ndk` twice, producing the `arm64-v8a` and `x86_64` Rust archives before it starts Gradle. CMake then statically links the prebuilt archive into `libdbm_android.so`; Gradle does not invoke Cargo. Re-run the script after Rust/core changes. The wrapper is pinned to Gradle 8.13 with checksum validation; AGP is 8.13.0, Kotlin 2.2.20, and the Compose BOM is 2025.09.01.

Connections are deliberately read-only and TLS-required. The app exports Android's `TrustManagerFactory` accepted issuers to an app-private PEM bundle and passes its absolute path as `caCertPath`; it never disables certificate validation. Moving DBM to the background immediately invalidates and clears query, result, and password UI state, then queues session disposal behind any active bridge call; it does not cancel server work. Return to the app to start a fresh local bridge session. For safe private-endpoint access, limits, and lifecycle details, see [`docs/mobile.md`](../../../docs/mobile.md).

The fixture/demo bridge is only reachable with the explicit `demo` activity extra in a debug build, as used by instrumentation. Release builds require the four `DBM_ANDROID_KEYSTORE`, `DBM_ANDROID_KEYSTORE_PASSWORD`, `DBM_ANDROID_KEY_ALIAS`, and `DBM_ANDROID_KEY_PASSWORD` variables. Set `DBM_BUILD_NUMBER` to a positive, increasing version code and run `bash apps/native/android/build.sh assembleRelease bundleRelease`. The signed APK and AAB are under `app/build/outputs/apk/release` and `app/build/outputs/bundle/release`. CI uses the same AWS upload key as Caper; the [mobile release setup](../../../docs/mobile.md#signed-mobile-releases-capers-distribution-path) covers downloads and optional Play internal delivery.

Space Mono is bundled under the SIL Open Font License 1.1 for UI, SQL, and data. Regular, bold, italic, and bold italic font files live in `app/src/main/res/font`; the notice is packaged from `app/src/main/assets`.
