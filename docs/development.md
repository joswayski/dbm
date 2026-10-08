# Developing Anybase

Anybase's released desktop product is two native clients over one Rust core:

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
- Node.js 24 and npm for the website and release-script tests.
- [uv](https://docs.astral.sh/uv/getting-started/installation/) 0.12.23 for
  Python release scripts; CI uses Python 3.12 and uv can provision it. The
  standalone scripts retain their existing Python 3.10+ syntax floor.

## Build and run

On macOS:

```sh
bash apps/native/macos/build.sh
open target/native/Anybase.app
# Isolated fixture, without loading local profiles or connecting to databases:
target/native/Anybase.app/Contents/MacOS/dbm --demo
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
and query history as an installed Anybase. Use disposable databases or read-only
profiles when testing writes.

## Tests and checks

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --manifest-path apps/native/workbench/Cargo.toml --all -- --check
cargo test --manifest-path apps/native/workbench/Cargo.toml --locked
cargo clippy --manifest-path apps/native/workbench/Cargo.toml --locked --all-targets -- -D warnings
npm --prefix apps/web ci
npm --prefix apps/web run check
npm --prefix apps/web run test:all
npm --prefix apps/web run build
uvx ruff==0.16.10 check scripts
uvx ruff==0.16.10 format --check scripts
uv run --locked --python 3.12 --script scripts/test_mobile_release.py -v
```

The website and release scripts share Vitest 5, with isolated web and release
projects; `npm --prefix apps/web run test:release` runs just the release tests.
Assertions stay intact and unit tests do not load the website's build-time
GitHub feed. Oxlint checks correctness and Oxfmt formats first-party JS/TS/CSS;
run `npm --prefix apps/web run fmt` to format. Generated routes and public assets
are excluded. TypeScript 7's `tsc` is the stable native Go compiler.

The three standalone Python scripts declare their existing PyJWT/PyYAML
dependencies inline (PEP 723), with per-script `.py.lock` files; no Python
package or root project environment is needed. uv installs isolated cached
environments rather than modifying the runner's global Python. Ruff provides
syntax/correctness and formatting gates; `uvx ruff==0.16.10 format scripts`
formats them. After deliberately changing a dependency, run
`uv lock --script scripts/NAME.py` and commit its lockfile. Tests use disposable
tools and mocked APIs; they never upload or release. The stdlib-only inline
Python in native build/snapshot helpers remains `python3` with no new dependency.

The Redis tests start a throwaway `redis-server` when one is installed and skip
otherwise. The PostgreSQL and MySQL tests that need a live server run only when
`DBM_TEST_POSTGRES_PORT` (a local server trusting `postgres` on 127.0.0.1) or
`DBM_TEST_MYSQL_PORT` (a local MySQL or MariaDB allowing passwordless `root` on
127.0.0.1) is set.

CI (`.github/workflows/ci.yml`) runs these on macOS, Windows, and Linux, builds
the universal AppKit app with fixture snapshots, and builds the Windows NSIS
installer from a stand-in executable.

CI runs on pushes to `main` only, not on pull requests, so run these checks
locally before merging. To run CI on another branch, use "Run workflow" on the
CI or mobile workflow in GitHub Actions.

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
`rust-toolchain.toml`, the locked Cargo dependencies, and npm dependencies for
the website and release tests (using the orb image's Node/npm). It also installs
the pinned uv version, syncs locked Python 3.12 script environments without
executing upload scripts, and caches Ruff.
[`.agents/resume`](../.agents/resume) only checks that the environment is still
intact when an orb wakes. No long-running services are declared in
[`.amp/services.yaml`](../.amp/services.yaml).

## Local installs

Local builds are not installed automatically. On macOS, copy
`target/native/Anybase.app` into `/Applications` yourself if you want to run it
from there. On Windows and Linux, the release binary is
`apps/native/workbench/target/release/dbm-workbench` (`.exe` on Windows).

Development builds have no build number, so they never check for updates.

An ad-hoc signature changes whenever the macOS app is rebuilt. Because Anybase
keeps database passwords in the macOS Keychain, macOS may ask for the login
keychain password when a newly built copy first reads an existing password.
This is a macOS system prompt; Anybase never receives the login keychain password.
Signing with a stable identity (`DBM_SIGNING_IDENTITY`) avoids the repeated
approval.

## Releases

Every merge to `main` that changes the app runs the release workflow and
publishes a GitHub release for macOS, Windows, and Linux. Install Anybase once from
the [latest release](https://github.com/joswayski/dbm/releases/latest); after
that, release builds update themselves. The macOS app is signed and notarized;
Windows builds are not Authenticode-signed. Versioning, the workflow's steps,
required secrets, and the update flow are documented in
[docs/releases.md](releases.md).

That desktop release flow is unchanged. Mobile CI does not sign, upload, or
publish Play Store, App Store, or TestFlight releases.

Anybase never uploads connection profiles, query history, or database results.
Desktop passwords use the operating system credential store; mobile passwords
remain in memory only. See [mobile privacy and lifecycle](mobile.md#storage-and-lifecycle).

## Website

`apps/web` is [anyba.se](https://anyba.se): a TanStack Start site prerendered to
static files and served by the `anybase` Cloudflare Worker. It uses the
Graphite tokens and the README screenshots (resized to WebP in
`apps/web/public/screenshots`). The build reads the newest commits on `main` from
GitHub's public commit feed for **Latest changes**, so it needs network access
but no token.

```sh
cd apps/web
npm ci
npm run dev        # http://localhost:5175
npm test
npm run check      # Oxlint, Oxfmt check, native TypeScript
npm run build      # dist/client
```

Vite 8 uses Rolldown/Oxc with the React 6 plugin. The browser build target stays
at Chrome/Edge 111, Firefox 114, Safari/iOS 16.4. npm and Node remain the package
manager and runtime; Wrangler's static asset deployment is unchanged.

Changes under `apps/web` do not publish a desktop release. Deployment is covered
in [the rename plan](anybase-migration.md#website).

## Screenshots

The README screenshots in `docs/screenshots/` are captured from the native
apps' `--demo` fixture. The AppKit app can also render the fixture's main
states to PNG files without interaction:

```sh
mkdir -p target/native/snapshots
target/native/Anybase.app/Contents/MacOS/dbm --demo --snapshot-dir target/native/snapshots
```

The website serves 1600-pixel WebP copies. After replacing a README screenshot,
regenerate them with ImageMagick:

```sh
for f in docs/screenshots/*.png; do
  convert "$f" -resize 1600x -quality 84 -define webp:method=6 \
    "apps/web/public/screenshots/$(basename "${f%.png}").webp"
done
```
