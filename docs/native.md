# Native architecture

Anybase ships browser-free desktop apps on **macOS, Windows, and Linux**: AppKit on
macOS, a native Win32 app on Windows, and GTK 4 on Linux, over one shared Rust core. Android Compose
and iPhone UIKit clients are in development and are not desktop-parity or
store releases. None uses Electron, JavaScript, or a WebView. See
[development](development.md) for desktop commands, [mobile](mobile.md) for the
mobile contract, and [releases](releases.md) for desktop packaging and updates.

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
- The Windows and Linux apps replaced an egui/wgpu app that shipped on both
  until v2026.11.11.5. It has been removed from the repository; the
  benchmarks below compare against that last release.
- `apps/native/win32` is the Windows app, which replaced egui there. It ships
  under the same executable, installer, settings file, and credential names,
  so egui installs update onto it. It is its own Cargo workspace: `dbm-ui`, a small
  immediate-mode toolkit rendering with Direct2D/DirectWrite into a
  DirectComposition swap chain and exposing UI Automation, and `dbm-win32`,
  the app on `dbm-desktop`. `--demo` uses the same fixture as egui. The
  `Win32 preview` workflow snapshots its screens on a hosted Windows runner and
  benchmarks it against egui; off Windows, the `raster` feature renders the
  same snapshots in software (`cargo run -p dbm-win32 --features raster --
  --snapshot-dir DIR` from `apps/native/win32`).
- `apps/native/gtk` (`dbm-gtk`) is the Linux app, which replaced egui there:
  Rust with gtk4-rs, GTK 4 widgets (`ColumnView` grids, `TextView` editor,
  popovers) styled with the Graphite CSS and the bundled Space Mono fonts,
  which it registers with fontconfig at startup. It is its own Cargo workspace
  and needs GTK 4.12 or newer on the host. It keeps the egui app's settings
  file (`app.ron` under the eframe `DBM` data directory), profile store, and
  keyring entries, so a Linux install that updates from egui keeps its
  profiles, passwords, window size, sidebar state, and What's new history.
- `apps/native/desktop` (`dbm-desktop`) is the UI-free part of the Windows and
  Linux apps: the serialized database worker and the self-updater. A UI
  toolkit passes it a wake callback that schedules its next frame; Win32 and
  GTK both use it unchanged.
- `apps/native/android` and `apps/native/ios` use Compose and UIKit. They call
  the mobile C ABI directly in-process (JNI on Android), not through a local or
  hosted HTTP proxy. The bridge uses host-managed SQLite metadata and
  memory-only passwords, and enforces read-only profiles with verified Required
  TLS. See [mobile](mobile.md) for its intentionally limited command surface and
  lifecycle semantics.
- `crates/dbm-update` checks the release manifest, downloads an update, and
  verifies its signature; both apps use it (AppKit through the bridge).
  `tools/dbm-sign` produces the signatures in the release workflow.
- `apps/native/icons` holds the app icon: `icon.icns` for the macOS bundle,
  `icon.ico` for the Windows executable and installer, and `icon.png` for the
  GTK window and the AppImage. All three use the same isolated database artwork
  with a transparent background, rather than an opaque square behind the icon.

Database I/O runs off the UI thread. Passwords remain in the OS credential store,
not SQLite. Profiles, queries, and results are not sent to an off-device service.
On mobile, passwords instead remain only in the Rust session memory and are
discarded with that session.

## Build outputs

- `bash apps/native/macos/build.sh` produces `target/native/Anybase.app`
  (executable `dbm`, bundle identifier `io.github.joswayski.dbm`). It builds
  for the current architecture and signs ad hoc; `DBM_UNIVERSAL=1` builds for
  Apple Silicon and Intel, and `DBM_SIGNING_IDENTITY` signs with a Developer ID
  and the hardened runtime. It bundles the bridge dylib, the icon, and the
  Space Mono fonts, and records `DBM_NATIVE_BUILD`, `DBM_APP_VERSION`, and
  `DBM_NATIVE_VERSION` in `Info.plist` when they are set.
- `cargo build --manifest-path apps/native/win32/Cargo.toml --locked --release`
  produces `apps/native/win32/target/release/dbm-win32.exe` with the icon
  embedded. Release packaging renames it `DBM-Windows-x64.exe` and installs it
  as `dbm.exe`, the name egui installs used.
- `cargo build --manifest-path apps/native/gtk/Cargo.toml --locked --release`
  produces `apps/native/gtk/target/release/dbm-gtk`. Release packaging ships it
  as `DBM-Linux-x64` and, with `apps/native/gtk/appimage.sh`, inside the
  AppImage as `usr/bin/dbm-workbench` (the egui name, so existing installs and
  desktop entries keep working). GTK 4 is not bundled; it comes from the host.
- `bash apps/native/android/build.sh` builds the two Rust static archives before
  Gradle builds/tests the Android development APK. iPhone simulator/device build
  commands and platform constraints are documented in [mobile](mobile.md).

Every desktop app accepts `--demo`, an isolated in-memory fixture that loads no local
profiles and connects to no database. The AppKit and Win32 apps also accept
`--demo --snapshot-dir DIR`, which walks the fixture through its main states
and writes a PNG of each; CI uses it to review the macOS UI, and the
`Win32 preview` workflow the Windows UI.

**Use disposable databases or read-only profiles when testing.** Outside
`--demo`, every build uses the real saved profiles and writes query history to
the same local store. Queries are capped at 10,000 rows; the AppKit bridge
limits database requests to 1 MiB and session-free helper requests, such as
resolving `SELECT *` against a large schema tree, to 256 MiB. Neither limit bounds memory consumed by very
large individual cells.

The Linux (GTK) app builds against `libgtk-4-dev` (4.12 or newer) and
OpenSSL/pkg-config, and runs on X11 or Wayland with the host's GTK 4 runtime
(`libgtk-4-1`). CSV export uses GTK's file dialog: the desktop portal's
chooser where GTK uses one (`xdg-desktop-portal`), otherwise GTK's own.

## Shared behavior

Editor and grid rules live in `crates/dbm-core` so every host behaves the same:
`sql_text` (statement under cursor or selection, destructive query
confirmation, `SELECT *` table resolution, SQL/Redis highlighting tokens,
keyword/command completion, schema filtering, schema-refresh wording, inline
change diffs, CSV encoding), `cell_values` (typed cell parsing; an empty field
is NULL for nullable columns), `connection_url` (URL import), `export`
(streaming full-table CSV), and `demo` (the `--demo` fixture). The Win32 and GTK
clients call them directly; the AppKit client reaches them through bridge commands.
Editor helpers run through `dbm_bridge_helper_call`, which needs no session and
takes UTF-16 offsets as `NSString` does, so highlighting and export progress
never wait behind a running query.

## Feature coverage

All hosts implement the same feature set, listed in [features](features.md).
On AppKit it means implemented and captured in the macOS CI
snapshots (`--demo --snapshot-dir`). On Win32 it means implemented, ported
from the egui app's state and rules, and its screens captured on a hosted
Windows runner (welcome, query results, table, inspector, staged edits,
connection dialog, collapsed sidebar, Redis); it has not been driven by hand
on a physical Windows machine or against live servers yet. On GTK it means
ported from the egui app with its state logic and unit tests, exercised on the
`--demo` fixture with screenshots of each state, and smoke-tested against
local PostgreSQL 17 and Redis on Xvfb: an egui-saved profile, keyring
password, window size and sidebar state opened in GTK; an inline edit saved to
PostgreSQL; a Redis profile created in GTK, a command run, and typed keys
browsed.

| Area | Win32 (Windows) | GTK 4 (Linux) | AppKit (macOS) |
| --- | --- | --- | --- |
| Graphite layout: sidebar, top bar, connection-colored tab strip, toolbars, cards, Space Mono fonts | Done | Done | Done |
| Welcome screen, sidebar collapse and resize (persisted) | Done | Done | Done |
| Profiles: engine picker, URL import, colors, TLS/CA, read-only, test before save | Done | Done | Done |
| Sidebar: connection subtitles, database picker, filterable schema/keyspace tree, refresh summary toast | Done | Done | Done |
| Query: statement under cursor or selection, Redis line mode, Cmd/Ctrl+Enter | Done | Done | Done |
| Query: SQL/Redis highlighting, active-statement outline, line numbers | Done | Done | Done |
| Query: keyword and Redis command completion | Done (same list) | Done (same list) | Done (same list) |
| Query: confirmation before destructive statements | Done | Done | Done |
| Query: per-profile+database history, refresh, `SELECT *` opens the editable viewer | Done | Done | Done |
| Table: all filter operators, multiple filters, sort, preview limit with stepper, paging | Done | Done | Done |
| Table: CSV copy (visible page, selection) and full filtered export with progress and Open / Show in folder | Done | Done | Done |
| Table: multi-row selection, inline and inspector edits, typed/NULL parsing | Done | Done | Done |
| Table: staged deletes, hover change preview, pending bar, conflict reload | Done | Done | Done |
| Table: column resize, collapse, Reset columns; delayed Refreshing overlay | Done | Done | Done |
| Messages: app error strip, inline query/table errors and notices, export result, toast | Done | Done | Done |
| Redis: typed key views and edits (strings, hashes, lists, sets, sorted sets), key index | Done through the shared table view | Done through the shared table view | Done through the shared table view |
| Tabs: rename, collapse, close guards for staged edits | Done | Done | Done |
| Keyboard: Delete stages deletion, Escape clears/reverts, Enter commits edits | Done | Done | Done |
| Keyboard: new connection, new query, close tab (⌘N/⌘T/⌘W, Ctrl+N/T/W) | Done | Done | Done |
| Updates: Check for updates / Update to …, signed download, in-place swap | Done | Done | Done |
| Accessibility | UI Automation provider (names, roles, value, invoke, toggle, focus); screen readers not yet validated | GTK accessibility (AT-SPI) on standard widgets; screen readers not yet validated | Native AppKit controls; not yet validated |
| Light theme | Not implemented | Not implemented | Not implemented |

Still open on every host:

- Screen reader, IME, clipboard, resizing, and credential-store testing on each
  supported OS.
- Query cancellation and stress testing of large results, reconnects, errors,
  conflicting edits, and shutdown during active work.

Demo results are synthetic; they do not validate SQL semantics or database
writes (the demo applies text filters and ordering only so the controls
can be exercised). Automated fixtures and Linux screenshots are not a
substitute for macOS/Windows hardware validation.

### Measurements (Windows, 2026-11-12)

egui vs the Win32 app, release builds, both `--demo`, run one after the other
on the same hosted `windows-2025` runner (4 vCPUs) by the `Win32 preview`
workflow (`apps/native/win32/bench/bench.ps1`), median of 3 runs each. The
runner has no GPU, so both render in software through WARP; treat the numbers
as relative. Memory is read after opening the demo `public.users` table tab;
scrolling sends a wheel notch every 16 ms over the grid for 15 s.

| | egui (wgpu) | Win32 (Direct2D) |
| --- | ---: | ---: |
| Time to first window | 129 ms | 58 ms |
| Working set, table open | 65.2 MB | 43.6 MB |
| Private bytes, table open | 410.5 MB | 15.1 MB |
| Idle CPU over 60 s (% of one core) | 0% | 0% |
| CPU while wheel-scrolling the grid (% of one core) | 344% | 41% |
| Frames presented while scrolling | not measurable | 38 fps |

egui has no frame counter to read from outside; the Win32 app writes one when
`DBM_FRAME_LOG` is set.

### Measurements: egui vs GTK (Linux, 2026-11-12)

`apps/native/gtk/bench.sh` runs release builds of both apps on the demo
fixture in a 1280×800 window and reports the median of five launches after a
warm-up. Measured in an Amp orb on Xvfb, where both render in software (egui
through lavapipe Vulkan, GTK through llvmpipe GL), so treat these as relative
numbers; rerun the script on real hardware.

| | egui | GTK 4 |
| --- | --- | --- |
| Launch to first window | 0.129 s | 0.165 s |
| Resident memory, `public.users` table open | 122.8 MB | 212.8 MB |
| Idle CPU over 60 s, table open | 0.00% | 0.00% |
| CPU while wheel-scrolling the grid for 10 s | 152% | 60% |

GTK's memory here is mostly the software GL driver: with
`GSK_RENDERER=cairo` it was 140.7 MB (and 81% CPU scrolling). A hardware
GPU driver replaces llvmpipe on a real desktop.

### Measurements: egui vs Tauri (Linux, 2026-09-25)

Release builds of the egui workbench and the former Tauri app, same VM, same
profile store and disposable PostgreSQL 16. The VM has no GPU (Xvfb, software
Vulkan, WebKitGTK without compositing), so treat these as relative numbers
only.

| | egui workbench | Tauri |
| --- | --- | --- | --- |
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
  a restart a table refresh reconnects and retries. User queries are never
  replayed automatically; a connection error attempts reconnection for the
  next explicit run, while preserving the failed query's error.
- Closing the window while a long query runs exits immediately; the server
  finishes that statement on its own. Closing is refused only with staged
  edits or while a save or export is writing.

## Validation

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --manifest-path apps/native/win32/Cargo.toml --all -- --check
cargo test --manifest-path apps/native/win32/Cargo.toml --locked
cargo clippy --manifest-path apps/native/win32/Cargo.toml --locked --all-targets -- -D warnings
cargo fmt --manifest-path apps/native/gtk/Cargo.toml --all -- --check
cargo test --manifest-path apps/native/gtk/Cargo.toml --locked
cargo clippy --manifest-path apps/native/gtk/Cargo.toml --locked --all-targets -- -D warnings
```

The Linux C ABI integration tests isolate XDG storage and exercise session
ownership, response freeing, input errors, recovery, and the shared editor
helpers with UTF-16 offsets. Workbench unit tests cover request routing and
queueing, stale replies after closing a profile, dirty-state guards, failed
page loads, typed staging, CSV copy, PK/xmin preservation, and the embedded
table viewer; the GTK app's state module carries the same tests. `crates/dbm-update` tests cover manifest parsing and signature
verification. macOS CI builds the universal Swift host and captures the
isolated AppKit fixture. That fixture also checks header-cell copy ownership
with long JSON-decoded column types, nested copies, and both destruction orders,
then refreshes a schema while a table has staged edits. For query results and
the embedded table viewer, it checks resize-strip hit testing and pane heights
after moving the divider and reloading, and captures the enlarged results.
These checks run in the optimized app and fail the snapshot process on a regression.

Run the AppKit checks on a Mac (not supported on Linux):

```sh
bash apps/native/macos/build.sh
mkdir -p target/native/snapshots
NSZombieEnabled=YES target/native/Anybase.app/Contents/MacOS/dbm \
  --demo --snapshot-dir target/native/snapshots
```

Zombies help diagnose over-released Objective-C objects during this isolated
fixture run; they are not enabled in normal app launches. AppKit can copy cells
with `NSCopyObject`, which does not retain Swift stored reference values
([SR-4756](https://github.com/swiftlang/swift/issues/47333)). `GridHeaderCell`
therefore keeps its type string in `NSCell.representedObject` and explicitly
restores that property when copying, since `NSCell` clears it in a copy.

Redis adapter tests start a disposable server when `redis-server` is available;
PostgreSQL/MySQL live tests skip unless `DBM_TEST_POSTGRES_PORT` or
`DBM_TEST_MYSQL_PORT` points to the documented disposable local server.
