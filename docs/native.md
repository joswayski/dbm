# Native development clients

DBM is migrating toward browser-free clients on **macOS, Windows, and Linux**.
**These are development clients, not feature/UI-parity replacements yet.**
The downloadable, auto-updating application still uses Tauri on all platforms.
No CPU or memory improvement has been measured yet.

## Architecture

- `crates/dbm-core` shares the existing PostgreSQL, MySQL, Redis, sessions,
  query history, SQLite profile storage, and OS credential-store implementation
  between Tauri and the native clients. Storage paths and credential identities
  are unchanged.
- `apps/native/macos` uses Swift/AppKit windows, text views, tables, and controls.
  It calls the Rust library in-process through the C header in
  `apps/native/bridge/include`. A serial background queue owns an opaque session,
  calls, and disposal; Rust owns response allocations and exposes their free
  function. Requests/results currently use JSON across this boundary (including
  profile passwords when saving), not a subprocess or a network service.
- `apps/native/workbench` uses Rust with egui/wgpu on Windows and Linux, calling
  the core directly on a serialized worker with a two-thread Tokio runtime.
  Rendering uses native graphics backends and bundled Geist fonts. **egui draws
  its own controls, not Windows/GTK system widgets.** It has no browser engine.

Neither native client uses Tauri, Electron, JavaScript, or a WebView. Database
I/O runs off the UI thread. Passwords remain in the OS credential store, not
SQLite. Profiles, queries, and results are not sent to an off-device service.

## Build and try

On macOS 13+ with Xcode Command Line Tools and the repository Rust toolchain:

```sh
bash apps/native/macos/build.sh
open 'target/native/DBM Native.app'
# Isolated fixture, without loading local profiles or connecting to databases:
'target/native/DBM Native.app/Contents/MacOS/DBMNative' --demo
```

The script ad-hoc signs a separate development app. It does not overwrite an
installed DBM, notarize, publish, or change its updater. Gatekeeper/Keychain may
prompt for this separately signed application.

On Windows or Linux, with the repository Rust toolchain:

```sh
cargo run --manifest-path apps/native/workbench/Cargo.toml --locked --release
# Isolated in-memory fixture:
cargo run --manifest-path apps/native/workbench/Cargo.toml --locked --release -- --demo
```

Linux builds need OpenSSL/pkg-config and X11/Wayland development libraries;
running needs a working display server and Vulkan driver. CI lists the Ubuntu
packages. The separate Cargo workspace keeps egui's dependency graph out of
the shipping Tauri application. CI builds development artifacts for all three
platforms; these are not signed release installers.

**Use disposable databases or read-only profiles first.** Outside `--demo`, these
clients use real saved profiles and write query history to the same local store
as Tauri. They support profile forms, connection/database/schema navigation,
query and table tabs, history, refresh, paging, basic filters/order, and staged
PK-backed edits/deletes. Existing core read-only and concurrency checks apply.
Queries are capped at 10,000 rows; the AppKit bridge limits requests to 1 MiB.
Neither limit bounds memory consumed by very large individual cells.

## Required before replacing Tauri

Feature and visual parity is the migration acceptance criterion, not optional
follow-up polish. The current clients do not meet it. Outstanding work includes:

- Full Graphite layout, connection-color, tab-strip, light/dark, and interaction
  parity with the React application; AppKit and egui currently differ visually.
- SQL syntax highlighting, statement-under-cursor/selection execution parity,
  completion, keyboard navigation, and editor ergonomics.
- Full Redis typed-key browsing/editing and recursive keyspace navigation.
- CSV export/copy, all filter operators, full inspector/null/type editing, and
  confirmation behavior across every destructive/navigation path.
- Native accessibility, screen readers, IME, clipboard, resizing, and real
  credential-store testing on each supported OS.
- Query cancellation and stress testing of large results, reconnects, errors,
  conflicting edits, and shutdown during active work.
- Signing, notarization, installers, updates, and migration/release acceptance.

Demo results are synthetic; they do not validate SQL semantics or database
writes. Automated fixtures and Linux screenshots are not a substitute for
macOS/Windows hardware validation.

Before cutover, compare release builds on the **same machine and dataset**:
cold launch, idle CPU/wakeups, resident memory, connect/query latency, scrolling
10k rows, large fields, and repeated connection cycles. Record OS/hardware and
multiple runs; do not compare an optimized native client against debug Tauri.

## Validation

```sh
npm run check
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --manifest-path apps/native/workbench/Cargo.toml --all -- --check
cargo test --manifest-path apps/native/workbench/Cargo.toml --locked
cargo clippy --manifest-path apps/native/workbench/Cargo.toml --locked --all-targets -- -D warnings
```

The Linux C ABI integration test isolates XDG storage and exercises session
ownership, response freeing, input errors, and recovery. Workbench unit tests
cover request routing, dirty-state guards, PK/xmin preservation, and paging.
macOS CI compiles the actual Swift host and captures an isolated AppKit fixture.
Redis adapter tests start a disposable server when `redis-server` is available;
PostgreSQL/MySQL live tests skip unless `DBM_TEST_POSTGRES_PORT` or
`DBM_TEST_MYSQL_PORT` points to the documented disposable local server.
