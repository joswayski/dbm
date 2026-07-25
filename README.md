# DBM

DBM is a local-first desktop database manager for macOS, Windows, and Linux.
The first milestone targets PostgreSQL: saved connections, database and schema
exploration, paginated table browsing, SQL execution, and safe local profile
storage. Redis and other protocol adapters are planned after the PostgreSQL
workflow is stable.

## Development

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

One local build only targets the current operating system. Pushing a version
tag runs the release workflow on macOS, Windows, and Linux and creates a draft
GitHub release with all three platforms' installers.

DBM never uploads connection profiles, query history, or database results.
Passwords are stored in the operating system credential store when available.

## What is implemented

- PostgreSQL direct connections with disabled, preferred, or required TLS.
- Local connection profiles and query history in an application SQLite database.
- Passwords through the macOS Keychain, Windows Credential Manager, or Linux
  secret service via `keyring`.
- Database list, schemas, tables/views, configurable previews up to 200 rows,
  structured multi-filtering, ordering, visible-page CSV copy, and full filtered
  CSV export.
- Resizable sidebars and columns, collapsible wide fields, and multi-row
  selection for staged edits and deletes.
- Inline edits and staged deletes for primary-key-backed tables, guarded by
  PostgreSQL `xmin` optimistic concurrency checks and a read-only profile mode.
- SQL tabs using CodeMirror, query result grids, a 10,000-row safety cap, and
  per-profile history.

The browser preview used by Vite has a small in-memory mock so the layout can be
worked on without launching Tauri. The real desktop app uses the Rust commands.

## Deliberate follow-ups

SSH jump-host transport, query cancellation with dedicated sessions, Redis
(including Sentinel/Cluster), encrypted profile sync, and signed auto-updates
are kept out of this first vertical slice. The profile model already reserves
the SSH shape, but the backend returns a clear unsupported-transport error
until the forwarding implementation is added and tested on all three OSes.

## Local data

The app stores non-secret profile metadata, settings, and up to 500 history
entries in the platform application data directory. Passwords are never put in
that SQLite file. There is no telemetry, account, or sync service.
