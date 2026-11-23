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
  popovers) styled with the Graphite CSS and the bundled Red Hat Mono fonts,
  which it registers with fontconfig at startup. It is its own Cargo workspace
  and needs GTK 4.12 or newer on the host. It keeps the egui app's settings
  file (`app.ron` under the eframe `DBM` data directory), profile store, and
  keyring entries, so a Linux install that updates from egui keeps its
  profiles, passwords, window size, sidebar state, and What's new history.
- `apps/native/desktop` (`dbm-desktop`) is the UI-free part of the Windows and
  Linux apps: the database worker and the self-updater. The worker runs each
  connection's requests in the order they were sent, since they share its
  session, and different connections at the same time. A UI
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
  Red Hat Mono fonts, and records `DBM_NATIVE_BUILD`, `DBM_APP_VERSION`, and
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
| Graphite layout: sidebar, top bar, connection-colored tab strip, toolbars, cards, Red Hat Mono fonts | Done | Done | Done |
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
| Messages: a failed query's error stays until the next run or a dismiss | Done | Done | Not implemented (it fades) |
| Redis: typed key views and edits (strings, hashes, lists, sets, sorted sets), key index | Done through the shared table view | Done through the shared table view | Done through the shared table view |
| Tabs: rename, collapse, close guards for staged edits | Done | Done | Done |
| Keyboard: Delete stages deletion, Escape clears/reverts, Enter commits edits | Done | Done | Done |
| Keyboard: new connection, new query, close tab (⌘N/⌘T/⌘W, Ctrl+N/T/W) | Done | Done | Done |
| Keyboard: refresh (F5, Ctrl+R), save staged changes (Ctrl+S), next and previous tab (Ctrl+Tab, Ctrl+Shift+Tab) | Done | Done | Not implemented |
| Keyboard: a new or newly selected query tab puts the caret in its editor; Ctrl+Enter also runs when the editor does not have the keyboard | Done | Done | Not implemented |
| Keyboard: dialogs keep Tab and typing to themselves, one Escape closes them; confirmations open on their default button (Cancel when the action is destructive) | Done | Done for Escape and the default button; Tab not checked | Not checked |
| Editor: Tab indents the selected lines, Shift+Tab outdents them | Done | Not implemented | Not checked |
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

### Measurements: Win32 against PostgreSQL on real hardware (Windows, 2026-11-22)

`apps/native/win32/bench/states.ps1` walks the Win32 release build through
each state against a local PostgreSQL 17 (`events`: 300,000 rows, 8 columns
with wide text and JSON) and records memory per state. It drives the app only
through UI Automation and wheel messages posted to its window, with a
throwaway profile store, and was run on a separate desktop so it took no one's
foreground. Machine: Ryzen 7 5800X, RTX 3080 Ti, one 3840×2160 monitor at
150%, Windows 11. Median (min–max) of 3 runs each, alternating builds, no
compiler running. "Before" is `721c39c`.

| State | Private before | Private after | Working set before | Working set after |
| --- | ---: | ---: | ---: | ---: |
| Launch, no connection | 60.8 MB (60.3–62.0) | 57.9 MB (57.0–58.4) | 56.7 MB | 53.7 MB |
| Connected, query tab | 67.8 MB | 62.7 MB | 75.7 MB | 70.7 MB |
| `events` table tab open | 68.9 MB | 63.1 MB | 76.7 MB | 72.1 MB |
| After scrolling it 15 s | 69.4 MB | 63.8 MB | 77.3 MB | 72.7 MB |
| 10,000-row wide result | 91.9 MB | 87.0 MB | 99.8 MB | 95.5 MB |
| Six tabs (three 10,000-row results, three tables) | 138.2 MB | 133.1 MB | 145.1 MB | 140.4 MB |
| All tabs closed | 72.7 MB* | 67.5 MB | 80.4 MB | 76.3 MB |
| Disconnected, then idle 60 s | 73.3 MB | 67.9 MB | 80.1 MB | 76.1 MB |

| | Before | After |
| --- | ---: | ---: |
| GPU dedicated memory, window at monitor size (3840×2160 px) | 52.7 MB | 43.3 MB |
| GPU dedicated memory, then minimized | 75.0 MB | 11.5 MB |
| Threads, launch / table open | 62 / 74 | 28 / 39 |
| Context switches/s while wheel-scrolling | 361 (346–376) | 190 (182–216) |
| Frames per 10 s, editor focused in a window opened behind others | 26 | 0 |
| Time to first window (5 launches) | 175 ms (172–196) | 186 ms (174–191) |
| Idle CPU, table open | 0% | 0% |
| CPU while wheel-scrolling | 6.0% (5.9–6.1) | 8.2% (4.6–8.2) |
| Frames presented while scrolling | 38 fps | 39 fps |

\* Only because the benchmark avoids walking the accessibility tree. Before
the fix, two labels drawn with the same text (the connection name in the top
bar and on the welcome pane, shown once the last tab closes) got the same
accessibility id, and UI Automation's sibling walk looped forever: any client
that walked the window (Narrator, the benchmark) hung, and Anybase's memory
climbed past 3 GB within a minute. Ids are now unique within a frame.

What else changed:

- Minimizing shrinks the swap chain to one pixel and stops presenting until
  the window is restored; resizing flushes so old buffers are freed at once.
- The D3D11 device is created with `PREVENT_INTERNAL_THREADING_OPTIMIZATIONS`:
  one thread draws, and NVIDIA's driver no longer starts ~34 worker threads.
- A window that opens without the keyboard now knows it, so a focused editor's
  caret does not redraw it twice a second.
- The text layout cache drops layouts the last two frames did not use once it
  holds well over what a frame draws (it used to grow to 6,000 entries):
  scrolling a 10,000-row result 1,200 notches grew private bytes 6 MB before,
  1 MB after.

Checked and left alone: the swap chain and Direct2D targets are 8-bit BGRA on
this display (no HDR format); menus and dropdowns draw into the window's one
swap chain, so they keep no surfaces; the extra bitmaps in `render.rs` exist
only while writing a `--snapshot-dir` PNG; the glyph-advance cache holds one
entry per distinct character and size. With WARP instead of the GPU, private
bytes at launch are 9.8 MB: most of the rest is the NVIDIA driver. A
10,000-row result costs about 23 MB, held as JSON values by the shared
worker's response. The release exe is 12.8 MB and the installer 4.1 MB.

### Measurements: egui vs GTK (Linux, 2026-11-12)

`apps/native/gtk/bench.sh` runs release builds of both apps on the demo
fixture in a 1280×800 window and reports the median of five launches after a
warm-up. Measured in an Amp orb on Xvfb, where both render in software (egui
through lavapipe Vulkan, GTK through llvmpipe GL), so treat these as relative
numbers; rerun the script on real hardware.

| | egui | GTK 4 |
| --- | --- | --- |
| Launch to first window | 0.143 s | 0.170 s |
| Resident memory, `public.users` table open | 122.8 MB | 199.9 MB |
| Idle CPU over 60 s, table open | 0.02% | 0.00% |
| CPU while wheel-scrolling the grid for 10 s | 149% | 48% |

GTK's memory here is mostly the software GL driver. With the table open,
heaptrack attributes 138 MB of the 153 MB heap peak to llvmpipe (libgallium)
and about 2 MB to the app's grid widgets; with `GSK_RENDERER=cairo` resident
memory is 142 MB. A hardware GPU driver replaces llvmpipe on a real desktop.
Grid cells build their in-grid editor only while editing and their copy button
only while hovered, which took 13 MB off the first GTK measurement (212.8 MB).

### Measurements: GTK memory against PostgreSQL (Linux, 2026-11-22)

Resident memory of GTK release builds walking the same states against a local
PostgreSQL 17 table (300,000 rows, 8 columns), driven with xdotool in a
1280×800 window. Same orb and caveat as above: no GPU, so the GL column
includes llvmpipe. "Before" is commit `05e5c33`.

| State | GL before | GL after | Cairo before | Cairo after |
| --- | ---: | ---: | ---: | ---: |
| Connected, query tab | 202 MB | 192 MB | 125 MB | 123 MB |
| Table tab open | 221–241 MB | 217 MB | 159 MB | 157 MB |
| `SELECT *` run five times | 314–317 MB | 265 MB | 230 MB | 193 MB |
| Five tabs open: three 10,000-row results, two tables | 442–454 MB | 383 MB | 315 MB | 312 MB |
| All tabs closed, 30 s later | 443–445 MB | 216 MB | 306 MB | 137 MB |
| Table tab open, then seven popovers and a tooltip opened once | 361 MB | 230 MB | — | — |

What changed:

- glibc kept freed memory in its heap. With every tab closed, `malloc_stats`
  showed 434 MB taken from the system and 120 MB in use. The app now calls
  `malloc_trim` three seconds after it last released something large (a closed
  tab, a replaced result, a popup).
- Under the GL renderer every dropdown, menu, completion list and staged-row
  preview kept its surface, and with it a GL context, after its first use:
  9–28 MB each here. Popovers are now unrealized when they close. Under Cairo
  they cost nothing; hardware GL drivers were not measured.
- GTK keeps about 200 rows of cell widgets per list whether or not the list is
  on screen, roughly 20 KB a cell. A tab in the background for 60 seconds now
  drops its grid's row widgets and rebuilds them, at the same scroll offset and
  selection, when it is shown again. With four 10,000-row result tabs open,
  three of them in the background, that took 353 MB to 291 MB; showing a parked
  tab costs one rebuild (0.8 s of CPU in this orb).

Still open: a grid on screen costs 20–35 MB for eight columns and grows with
the column count, because every cell is a widget. The Win32 grid draws its
cells itself; doing the same in GTK is the remaining large saving.

### Measurements: AppKit memory against PostgreSQL (macOS, 2026-11-22)

Physical footprint from `footprint` for `build.sh` builds on an Apple Silicon
MacBook Pro with an XDR display, walking the states below against a local
PostgreSQL 18 table (300,000 wide rows) with a throwaway profile store and
keychain. Two passes per build; "before" is commit `05e5c33`, "after" is
`326dc22`.

| State | Before | After |
| --- | ---: | ---: |
| Launch, no connection | 27.6–31.7 MB | 13.6–13.7 MB |
| Connected, query tab | 57.4–59.1 MB | 26.7–26.8 MB |
| Table tab open | 75.1–75.5 MB | 38.5–38.6 MB |
| After scrolling five table pages | 126.5–128.8 MB | 37.3–37.8 MB |
| 10,000-row wide result | 149.0–152.8 MB | 49.8–50.1 MB |
| That result scrolled top to bottom | 237.1–254.6 MB | 50.4–50.8 MB |
| Four table tabs and three 10,000-row result tabs | 259.7–286.4 MB | 78.9 MB (one pass) |
| Those tabs closed | 201.5–230.9 MB | 52.5–53.7 MB |
| Disconnected | 83.3–83.5 MB | 32.8–36.1 MB |
| Idle 60 s later | 62.1–85.1 MB | 33.4–34.7 MB |

Idle CPU was 0.0% in both builds, and launch to first window about 170 ms in
both (median of eight alternating launches each).

What changed:

- The grids built a new row view for every row shown and `NSTableView` kept
  all of them, each with a layer backing store: 5,041 row views after
  scrolling a 10,000-row result. Row views are now reused, and a row draws its
  cells into one layer.
- The window inherited an extended-range color space on wide-gamut and XDR
  displays, which enlarged every backing store. It now uses sRGB; colors shift
  by at most 12/255.
- Panels, including the window-sized root, painted their fills and hairlines
  into full-size bitmaps. They now use layer properties and keep no backing
  store.

Still open: while anything redraws, the footprint rises by 85–100 MB of
graphics memory that macOS frees about a second after drawing stops, in both
builds, so a sample taken mid-draw reads that much higher (the second "after"
pass of the seven-tab state did). About 18 MB above the launch level remains
after disconnecting.

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
