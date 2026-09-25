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
as Tauri. Existing core read-only and concurrency checks apply. Queries are
capped at 10,000 rows; the AppKit bridge limits requests to 1 MiB. Neither limit
bounds memory consumed by very large individual cells.

On Linux, CSV export in the egui client asks for a destination through the
desktop portal (`xdg-desktop-portal`), which GNOME, KDE, and most desktops
provide. Without a portal the save dialog cannot open and the export is
reported as canceled.

## Shared behavior

Editor and grid rules live in `crates/dbm-core` so every host behaves the same
as the React app: `sql_text` (statement under cursor or selection, destructive
query confirmation, `SELECT *` table resolution, SQL/Redis highlighting tokens,
schema filtering, CSV encoding), `cell_values` (typed cell parsing; an empty
field is NULL for nullable columns), `connection_url` (URL import), and
`export` (streaming full-table CSV). The egui client calls them directly; the
AppKit client reaches them through bridge commands (`executionTarget`,
`requiresConfirmation`, `resolveFullTableSelect`, `highlight`, `parseCell`,
`editableText`, `csv`, `parseConnectionUrl`, `exportCsv`), which take UTF-16
offsets as `NSString` does.

## Parity checklist

Feature and visual parity is the migration acceptance criterion, not optional
follow-up polish. Status against the React application:

| Area | egui (Windows/Linux) | AppKit (macOS) |
| --- | --- | --- |
| Graphite layout: sidebar, top bar, connection-colored tab strip, toolbars, cards | Matches the dark React layout; compared by screenshot on Linux | Partial: tab dropdown, system fonts |
| Profiles: engine picker, URL import, colors, TLS/CA, read-only, test before save | Done | Partial: text-field alert (engine, TLS, color typed as text); no URL import or test |
| Sidebar: connection subtitles, database picker, filterable recursive schema/keyspace tree | Done | Partial: profile list and schema outline; no subtitles or filter |
| Query: statement under cursor or selection, Redis line mode, Cmd/Ctrl+Enter | Done | Partial: runs the selection or the whole editor |
| Query: SQL/Redis highlighting and active-statement outline, line numbers | Done | Not yet |
| Query: completion | Not yet (CodeMirror keyword completion) | Not yet |
| Query: confirmation before destructive statements | Done | Not yet |
| Query: per-profile+database history, refresh, `SELECT *` opens editable viewer | Done | Partial: history in a picker dialog, refresh |
| Table: all filter operators, multiple filters, sort, preview limit, paging | Done | Partial: one filter, 9 of 13 operators, header sort, paging |
| Table: CSV copy (visible page, selection) and full filtered export | Done | Not yet in the UI; `csv`/`exportCsv` bridge commands exist |
| Table: multi-row selection, inline and inspector edits, typed/NULL parsing | Done | Partial: single-row inline edits; no inspector |
| Table: staged deletes, pending bar, confirm, conflict reload | Done | Partial |
| Redis: typed key views and edits (strings, hashes, lists, sets, sorted sets), key index | Done through the shared table view | Partial |
| Column collapse, persisted sidebar width, tab rename/collapse | Tab rename only | Not yet |
| Keyboard: Delete stages deletion, Escape clears/reverts, Enter commits edits | Done | Partial: standard table editing keys |
| Accessibility | AccessKit enabled; screen readers not yet validated | Native AppKit controls; not yet validated |
| Light theme | Not in the React app either | — |

Also required before replacing Tauri, on every host:

- Native accessibility, screen readers, IME, clipboard, resizing, and real
  credential-store testing on each supported OS.
- Query cancellation and stress testing of large results, reconnects, errors,
  conflicting edits, and shutdown during active work.
- Signing, notarization, installers, updates, and migration/release acceptance.

Demo results are synthetic; they do not validate SQL semantics or database
writes (the egui demo applies text filters and ordering only so the controls
can be exercised). Automated fixtures and Linux screenshots are not a
substitute for macOS/Windows hardware validation.

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

The Linux C ABI integration tests isolate XDG storage and exercise session
ownership, response freeing, input errors, recovery, and the shared editor
helpers with UTF-16 offsets. Workbench unit tests cover request routing and
queueing, stale replies after closing a profile, dirty-state guards, failed
page loads, typed staging, CSV copy, PK/xmin preservation, and the embedded
table viewer.
macOS CI compiles the actual Swift host and captures an isolated AppKit fixture.
Redis adapter tests start a disposable server when `redis-server` is available;
PostgreSQL/MySQL live tests skip unless `DBM_TEST_POSTGRES_PORT` or
`DBM_TEST_MYSQL_PORT` points to the documented disposable local server.
