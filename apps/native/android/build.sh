#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$ROOT/../../.." && pwd)"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
# Keep Rust/OpenSSL compilation separate from Gradle's JVM on small machines.
for abi in arm64-v8a x86_64; do
    case "$abi" in arm64-v8a) target=aarch64-linux-android ;; x86_64) target=x86_64-linux-android ;; esac
    cargo ndk -t "$abi" --platform 26 build --manifest-path "$REPO/apps/native/bridge/Cargo.toml" --target-dir "$REPO/target" --locked --release --no-default-features
    mkdir -p "$ROOT/app/build/rust/$abi"
    cp "$REPO/target/$target/release/libdbm_native_bridge.a" "$ROOT/app/build/rust/$abi/"
done
"$ROOT/gradlew" -p "$ROOT" --no-daemon --max-workers=2 testDebugUnitTest lintDebug assembleDebug assembleDebugAndroidTest "$@"
# Distribution bundles must include both JNI architectures and the font notices.
unzip -l "$ROOT/app/build/outputs/apk/debug/app-debug.apk" | grep 'lib/arm64-v8a/libdbm_android.so'
unzip -l "$ROOT/app/build/outputs/apk/debug/app-debug.apk" | grep 'lib/x86_64/libdbm_android.so'
unzip -l "$ROOT/app/build/outputs/apk/debug/app-debug.apk" | grep 'assets/Geist-LICENSE.txt'
