# Native architecture

DBM ships as browser-free native apps on **macOS, Windows, and Linux**: AppKit
on macOS and egui on Windows and Linux, over one shared Rust core. Neither uses
Electron, JavaScript, or a WebView. See [development](development.md) for
prerequisites and commands and [releases](releases.md) for packaging and
updates.

## Architecture

- `crates/dbm-core` holds the PostgreSQL, MySQL, and Redis adapters, sessions,
  query history, SQLite profile storage, and the OS credential-store
  integration. Storage paths and credential identities are the same ones the
  former Tauri app used, so its data carries over.
- `apps/native/macos` uses Swift/AppKit windows, text views, tables, and controls.
  It calls the Rust library in-process through the C header in
  `apps/native/bridge/include`. A serial background queue owns an opaque session,
  calls, and disposal; Rust owns response allocations and exposes their free
  function. Requests/results use JSON across this boundary (including profile
  passwords when saving), not a subprocess or a network service.
- `apps/native/workbench` uses Rust with egui/wgpu on Windows and Linux, calling
  the core directly on a serialized worker with a two-thread Tokio runtime.
  Rendering uses native graphics backends and bundled Geist fonts. **egui draws
  its own controls, not Windows/GTK system widgets.** It is a separate Cargo
  workspace, so egui's dependency graph stays out of the root workspace.
- `crates/dbm-update` checks the release manifest, downloads an update, and
  verifies its signature; both apps use it (AppKit through the bridge).
  `tools/dbm-sign` produces the signatures in the release workflow.
- `apps/native/icons` holds the app icon: `icon.icns` for the macOS bundle,
  `icon.ico` for the Windows executable and installer, and `icon.png` for the
  egui window and the AppImage. All three use the same isolated database artwork
  with a transparent background, rather than an opaque square behind the icon.

Database I/O runs off the UI thread. Passwords remain in the OS credential store,
not SQLite. Profiles, queries, and results are not sent to an off-device service.

## Build outputs

- `bash apps/native/macos/build.sh` produces `target/native/DBM.app`
  (executable `dbm`, bundle identifier `io.github.joswayski.dbm`). It builds
  for the current architecture and signs ad hoc; `DBM_UNIVERSAL=1` builds for
  Apple Silicon and Intel, and `DBM_SIGNING_IDENTITY` signs with a Developer ID
  and the hardened runtime. It bundles the bridge dylib, the icon, and the
  Geist fonts, and records `DBM_NATIVE_BUILD`, `DBM_APP_VERSION`, and
  `DBM_NATIVE_VERSION` in `Info.plist` when they are set.
- `cargo build --manifest-path apps/native/workbench/Cargo.toml --locked --release`
  produces `apps/native/workbench/target/release/dbm-workbench` (`.exe` on
  Windows). Release packaging renames it and, on Linux, wraps it with
  `apps/native/workbench/appimage.sh`.

Both apps accept `--demo`, an isolated in-memory fixture that loads no local
profiles and connects to no database. The AppKit app also accepts
`--demo --snapshot-dir DIR`, which walks the fixture through its main states
and writes a PNG of each; CI uses it to review the macOS UI.

**Use disposable databases or read-only profiles when testing.** Outside
`--demo`, every build uses the real saved profiles and writes query history to
the same local store. Queries are capped at 10,000 rows; the AppKit bridge
limits requests to 1 MiB. Neither limit bounds memory consumed by very large
individual cells.

Linux builds need OpenSSL/pkg-config and X11/Wayland development libraries;
running needs a working display server and Vulkan driver. On Linux, CSV export
asks for a destination through the desktop portal (`xdg-desktop-portal`), which
GNOME, KDE, and most desktops provide. Without a portal the save dialog cannot
open and the export is reported as canceled.

## Shared behavior

Editor and grid rules live in `crates/dbm-core` so every host behaves the same:
`sql_text` (statement under cursor or selection, destructive query
confirmation, `SELECT *` table resolution, SQL/Redis highlighting tokens,
keyword/command completion, schema filtering, schema-refresh wording, inline
change diffs, CSV encoding), `cell_values` (typed cell parsing; an empty field
is NULL for nullable columns), `connection_url` (URL import), `export`
(streaming full-table CSV), and `demo` (the `--demo` fixture). The egui client
calls them directly; the AppKit client reaches them through bridge commands.
Editor helpers run through `dbm_bridge_helper_call`, which needs no session and
takes UTF-16 offsets as `NSString` does, so highlighting and export progress
never wait behind a running query.

## Feature coverage

Both hosts implement the same feature set, listed in [features](features.md).
"Done" on egui means exercised by hand on Linux against disposable PostgreSQL
and Redis servers. On AppKit it means implemented and captured in the macOS CI
snapshots (`--demo --snapshot-dir`).

| Area | egui (Windows/Linux) | AppKit (macOS) |
| --- | --- | --- |
| Graphite layout: sidebar, top bar, connection-colored tab strip, toolbars, cards, Geist fonts | Done | Done |
| Welcome screen, sidebar collapse and resize (persisted) | Done | Done |
| Profiles: engine picker, URL import, colors, TLS/CA, read-only, test before save | Done | Done |
| Sidebar: connection subtitles, database picker, filterable schema/keyspace tree, refresh summary toast | Done | Done |
| Query: statement under cursor or selection, Redis line mode, Cmd/Ctrl+Enter | Done | Done |
| Query: SQL/Redis highlighting, active-statement outline, line numbers | Done | Done |
| Query: keyword and Redis command completion | Done (shared keyword list, not schema-aware) | Done (same list) |
| Query: confirmation before destructive statements | Done | Done |
| Query: per-profile+database history, refresh, `SELECT *` opens the editable viewer | Done | Done |
| Table: all filter operators, multiple filters, sort, preview limit with stepper, paging | Done | Done |
| Table: CSV copy (visible page, selection) and full filtered export with progress and Open / Show in folder | Done | Done |
| Table: multi-row selection, inline and inspector edits, typed/NULL parsing | Done | Done |
| Table: staged deletes, hover change preview, pending bar, conflict reload | Done | Done |
| Table: column resize, collapse, Reset columns; delayed Refreshing overlay | Done | Done |
| Messages: app error strip, inline query/table errors and notices, export result, toast | Done | Done |
| Redis: typed key views and edits (strings, hashes, lists, sets, sorted sets), key index | Done through the shared table view | Done through the shared table view |
| Tabs: rename, collapse, close guards for staged edits | Done | Done |
| Keyboard: Delete stages deletion, Escape clears/reverts, Enter commits edits | Done | Done |
| Updates: Check for updates / Update to …, signed download, in-place swap | Done | Done |
| Accessibility | AccessKit enabled; screen readers not yet validated | Native AppKit controls; not yet validated |
| Light theme | Not implemented | Not implemented |

Still open on every host:

- Screen reader, IME, clipboard, resizing, and credential-store testing on each
  supported OS.
- Query cancellation and stress testing of large results, reconnects, errors,
  conflicting edits, and shutdown during active work.

Demo results are synthetic; they do not validate SQL semantics or database
writes (the egui demo applies text filters and ordering only so the controls
can be exercised). Automated fixtures and Linux screenshots are not a
substitute for macOS/Windows hardware validation.

### Measurements (Linux, 2026-09-25)

Release builds of the egui workbench and the former Tauri app, same VM, same
profile store and disposable PostgreSQL 16. The VM has no GPU (Xvfb, software
Vulkan, WebKitGTK without compositing), so treat these as relative numbers
only.

| | egui workbench | Tauri |
| --- | --- | --- |
| Resident memory, idle (all processes) | 145 MB (3 processes) | 380 MB (5 processes) |
| Idle CPU over 20 s | ~0% | ~0.05% |
| Warm launch to window | under 0.5 s | under 0.5 s |
| First (cold) launch | 3.5 s | 2.0 s |

egui workbench under load:
- A 10,000-row result (4 columns, 200-byte text) returned in 368 ms at
  213 MB. Scrolling stays smooth because only visible rows are laid out.
- 20 rows of 1 MiB text render truncated in the grid at 218 MB.
- A value above 64 KiB is shown as a read-only preview in the inspector and
  refused by the inline editor, with a note to update it with a query. An
  editable 1 MiB field took 3.4 s of CPU and grew memory to 786 MB. The
  AppKit host applies the same limit.
- Stopping PostgreSQL mid-session shows the connection error inline; after
  a restart the next refresh reconnects.
- Closing the window while a long query runs exits immediately; the server
  finishes that statement on its own. Closing is refused only with staged
  edits or while a save or export is writing.

## Validation

```sh
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
table viewer. `crates/dbm-update` tests cover manifest parsing and signature
verification. macOS CI builds the universal Swift host and captures the
isolated AppKit fixture.
Redis adapter tests start a disposable server when `redis-server` is available;
PostgreSQL/MySQL live tests skip unless `DBM_TEST_POSTGRES_PORT` or
`DBM_TEST_MYSQL_PORT` points to the documented disposable local server.
