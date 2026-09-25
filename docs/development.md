# Developing DBM

## Setup

Prerequisites:

- Node.js 24 and npm 11
- Rust 1.94 with `rustfmt` and `clippy`
- Tauri's native dependencies for the operating system

```sh
npm install
cargo test --workspace
npm run check
npm run dev
```

The Redis tests start a throwaway `redis-server` when one is installed and skip
otherwise. The PostgreSQL and MySQL tests that need a live server run only when
`DBM_TEST_POSTGRES_PORT` (a local server trusting `postgres` on 127.0.0.1) or
`DBM_TEST_MYSQL_PORT` (a local MySQL or MariaDB allowing passwordless `root` on
127.0.0.1) is set.

## Amp orbs

Amp orbs run [`.agents/setup`](../.agents/setup) to prepare a fresh machine: it installs Tauri's
Linux build dependencies, `redis-server` (the live Redis tests skip themselves without it),
Node.js 24 with npm 11, the Rust toolchain pinned in `rust-toolchain.toml`, and the locked npm and
Cargo dependencies. [`.agents/resume`](../.agents/resume) only checks that the environment is still
intact when an orb wakes.

The Vite browser preview is declared in [`.amp/services.yaml`](../.amp/services.yaml). Inside an orb,
`amp orb services ensure` starts it supervised and prints its portal URL.

## Build and install

`npm run build` creates a native build for the operating system where the
command runs. It prints the absolute paths to the unpackaged executable and
every installer or app bundle it creates.

On macOS, a successful build also:

1. Quits any running DBM instance.
2. Replaces `/Applications/DBM.app` with the new build.
3. Launches the newly installed app.

The generated app bundle and DMG remain under `target/release/bundle`.
Local builds use an installed Apple Development signing identity when one is
available and otherwise use an ad-hoc signature.

An ad-hoc signature changes whenever DBM is rebuilt. Because DBM keeps database
passwords in macOS Keychain, macOS may ask for the login keychain password when
a newly built copy first reads an existing password. This is a macOS system
prompt—DBM never receives the login keychain password. A stable Apple
Development signing identity avoids that repeated approval.

```sh
# Build + install + launch (default on macOS)
npm run build

# Build only, without changing /Applications
DBM_SKIP_INSTALL=1 npm run build

# Install without launching
DBM_OPEN_AFTER_INSTALL=0 npm run build
```

On Windows, the build creates an NSIS installer under
`target/release/bundle/nsis` and an unpackaged executable at
`target/release/dbm.exe`. If that exact unpackaged executable is already
running, the build stops it first so it can be replaced.

On Linux, the build creates `.deb` and AppImage packages under
`target/release/bundle`, plus the unpackaged executable at
`target/release/dbm`.

One local build only targets the current operating system. Every merge to
`main` that changes the app runs the release workflow on macOS, Windows, and
Linux and publishes a GitHub release with all three platforms' installers,
signed updater artifacts, `SHA256SUMS`, and a validated `latest.json`
manifest. Install DBM once from the
[latest release](https://github.com/joswayski/dbm/releases/latest); after that,
official builds check for updates at launch and every 4 hours (or on demand
from the top bar's **Check for updates** button) and install authenticated
updates in place where the platform supports it. `.deb` installs open the
release page instead.

These per-merge releases are meant for the maintainer's own machines: the
macOS build is signed and notarized, but Windows installers are not yet
Authenticode-signed. Versioning, the workflow's steps, required secrets, and
the remaining gates for a wider public release are documented in
[docs/releases.md](releases.md).

DBM never uploads connection profiles, query history, or database results.
Passwords are stored in the operating system credential store when available.

## Browser preview and screenshots

The browser preview used by Vite has a small in-memory mock so the layout can be
worked on without launching Tauri. The real desktop app uses the Rust commands.
Add `?demo` to the preview URL to load a larger sample dataset (four
connections, a `public` schema with several tables, query results, and
history). `node scripts/screenshots.mjs` uses that dataset to regenerate
`docs/screenshots/`. It needs Playwright with Chromium installed
(`npm install --global playwright && npx playwright install chromium`).
