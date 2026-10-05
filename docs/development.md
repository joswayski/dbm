# Developing DBM

DBM's released desktop product is two native clients over one Rust core:

- **macOS:** a Swift/AppKit app in `apps/native/macos`, calling the Rust core
  through the C ABI bridge in `apps/native/bridge`.
- **Windows and Linux:** a Rust egui app in `apps/native/workbench`, its own
  Cargo workspace.

The database adapters, models, session state, credentials, and SQLite store
live in `crates/dbm-core`; the updater lives in `crates/dbm-update`. See
[native architecture](native.md) for how the pieces fit together.

Kotlin/Compose Android and SwiftUI iPhone development clients also call that
Rust code directly in-process. They have a smaller read-only contract and a
separate build workflow; see [mobile development](mobile.md) rather than using
the desktop setup below.

## Setup

Prerequisites:

- Rust 1.94 with `rustfmt` and `clippy` (pinned in `rust-toolchain.toml`)
- macOS: macOS 13+ with Xcode Command Line Tools
- Linux: the packages CI installs for the native workbench:

  ```sh
  sudo apt-get install -y build-essential libssl-dev pkg-config \
    libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxi-dev \
    libxrandr-dev libvulkan1 mesa-vulkan-drivers
  ```

  Running the app needs a display server and a Vulkan driver.
- Node.js 24, only to run the release-script tests.

## Build and run

On macOS:

```sh
bash apps/native/macos/build.sh
open target/native/DBM.app
# Isolated fixture, without loading local profiles or connecting to databases:
target/native/DBM.app/Contents/MacOS/dbm --demo
```

`build.sh` builds for the current Mac's architecture and signs ad hoc.
`DBM_UNIVERSAL=1` builds one app for Apple Silicon and Intel;
`DBM_SIGNING_IDENTITY` signs with a Developer ID and the hardened runtime
instead, as the release workflow does.

On Windows or Linux:

```sh
cargo run --manifest-path apps/native/workbench/Cargo.toml --locked --release
# Isolated in-memory fixture:
cargo run --manifest-path apps/native/workbench/Cargo.toml --locked --release -- --demo
```

`bash apps/native/workbench/demo.sh` is a shortcut for the debug `--demo` build.

Outside `--demo`, development builds use the same saved profiles, passwords,
and query history as an installed DBM. Use disposable databases or read-only
profiles when testing writes.

## Tests and checks

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --manifest-path apps/native/workbench/Cargo.toml --all -- --check
cargo test --manifest-path apps/native/workbench/Cargo.toml --locked
cargo clippy --manifest-path apps/native/workbench/Cargo.toml --locked --all-targets -- -D warnings
node --test scripts/release.test.mjs
```

The Redis tests start a throwaway `redis-server` when one is installed and skip
otherwise. The PostgreSQL and MySQL tests that need a live server run only when
`DBM_TEST_POSTGRES_PORT` (a local server trusting `postgres` on 127.0.0.1) or
`DBM_TEST_MYSQL_PORT` (a local MySQL or MariaDB allowing passwordless `root` on
127.0.0.1) is set.

CI (`.github/workflows/ci.yml`) runs these on macOS, Windows, and Linux, builds
the universal AppKit app with fixture snapshots, and builds the Windows NSIS
installer from a stand-in executable.

The separate [mobile workflow](../.github/workflows/mobile.yml) is configured to
build/lint/test Android, run its emulator UI test, build/test the iPhone
Simulator client, compile the iPhone Rust archive, and retain development
artifacts and UI captures. These are workflow definitions, not claims about a
particular run. SwiftUI is not compiled on Linux, and physical-device plus live
TLS database validation remains outstanding. Commands and prerequisites are in
[the mobile contract](mobile.md).

## Amp orbs

Amp orbs run [`.agents/setup`](../.agents/setup) to prepare a fresh machine: it
installs the native workbench's Linux build dependencies, `redis-server` (the
live Redis tests skip themselves without it), the Rust toolchain pinned in
`rust-toolchain.toml`, and the locked Cargo dependencies.
[`.agents/resume`](../.agents/resume) only checks that the environment is still
intact when an orb wakes. No long-running services are declared in
[`.amp/services.yaml`](../.amp/services.yaml).

## Local installs

Local builds are not installed automatically. On macOS, copy
`target/native/DBM.app` into `/Applications` yourself if you want to run it
from there. On Windows and Linux, the release binary is
`apps/native/workbench/target/release/dbm-workbench` (`.exe` on Windows).

Development builds have no build number, so they never check for updates.

An ad-hoc signature changes whenever the macOS app is rebuilt. Because DBM
keeps database passwords in the macOS Keychain, macOS may ask for the login
keychain password when a newly built copy first reads an existing password.
This is a macOS system prompt; DBM never receives the login keychain password.
Signing with a stable identity (`DBM_SIGNING_IDENTITY`) avoids the repeated
approval.

## Releases

Every merge to `main` that changes the app runs the release workflow and
publishes a GitHub release for macOS, Windows, and Linux. Install DBM once from
the [latest release](https://github.com/joswayski/dbm/releases/latest); after
that, release builds update themselves. The macOS app is signed and notarized;
Windows builds are not Authenticode-signed. Versioning, the workflow's steps,
required secrets, and the update flow are documented in
[docs/releases.md](releases.md).

That desktop release flow is unchanged. Mobile CI does not sign, upload, or
publish Play Store, App Store, or TestFlight releases.

DBM never uploads connection profiles, query history, or database results.
Desktop passwords use the operating system credential store; mobile passwords
remain in memory only. See [mobile privacy and lifecycle](mobile.md#storage-and-lifecycle).

## Website

`apps/web` is [anyba.se](https://anyba.se): a TanStack Start site prerendered to
static files and served by the `anybase-web` Cloudflare Worker. It uses the
Graphite tokens and the README screenshots (resized to WebP in
`apps/web/public/screenshots`). The build fetches the newest commits on `main`
from the GitHub API for **Latest changes**, so it needs network access; set
`GITHUB_TOKEN` to avoid the unauthenticated rate limit.

```sh
cd apps/web
npm ci
npm run dev        # http://localhost:5175
npm test
npm run typecheck
npm run build      # dist/client
```

Changes under `apps/web` do not publish a desktop release. Deployment is covered
in [the rename plan](anybase-migration.md#website).

## Screenshots

The README screenshots in `docs/screenshots/` are captured from the native
apps' `--demo` fixture. The AppKit app can also render the fixture's main
states to PNG files without interaction:

```sh
mkdir -p target/native/snapshots
target/native/DBM.app/Contents/MacOS/dbm --demo --snapshot-dir target/native/snapshots
```

The website serves 1600-pixel WebP copies. After replacing a README screenshot,
regenerate them with ImageMagick:

```sh
for f in docs/screenshots/*.png; do
  convert "$f" -resize 1600x -quality 84 -define webp:method=6 \
    "apps/web/public/screenshots/$(basename "${f%.png}").webp"
done
```
