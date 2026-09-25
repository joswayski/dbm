# Native development preview

DBM is migrating incrementally toward native UI. **Tauri remains the shipping
application on macOS, Windows, and Linux.** This is the first macOS workbench
slice, not a feature-complete replacement or a demonstrated performance win.

## Architecture

- `crates/dbm-core`: existing PostgreSQL, MySQL, Redis, session, SQLite, and
  credential-store implementation, now reused by both hosts. Query execution
  and history recording have one implementation. Storage paths, SQLite schema,
  keyring service/account names, and database safety behavior are unchanged.
- `apps/native/macos`: Swift/AppKit `NSWindow`, `NSTextView`, `NSTableView`,
  menus, and controls. No WebView or JavaScript. Uses macOS system appearance
  and fonts for this functional preview; full Graphite/connection-color parity
  is still pending.
- `apps/native/bridge`: a bundled Rust child process using the shared core.
  Newline-delimited JSON travels over private stdin/stdout pipes, never an HTTP
  server or listening socket. A serial Swift worker queue performs I/O and
  decoding off the UI thread; a two-thread Tokio runtime drives database I/O.
  There is no UI polling loop or new off-device service.

The reference migrations use Swift/AppKit plus a Rust C ABI on macOS, and
egui/wgpu on Windows/Linux. This preview deliberately starts with a safe Rust
process boundary instead of C pointers. That adds a process and JSON copies;
compare measured end-to-end costs before deciding whether an in-process bridge
is needed. egui/wgpu is browser-free but **not native OS widgets**. A Windows/Linux
native UI choice is still open; those platforms continue using Tauri, not a new
egui host that is being presented as system-native.

## Try it on macOS

Requires macOS 13+, Xcode Command Line Tools, and the repository's Rust toolchain.
From the repository root:

```sh
bash apps/native/macos/build.sh
open 'target/native/DBM Native.app'
```

The script builds for the current Mac architecture and ad-hoc signs a separate
`DBM Native.app`. It does not install over `/Applications/DBM.app`, register
associations, notarize, publish, or change the installed app's updater.
The macOS CI job compiles it and uploads a development ZIP, not a release.
Gatekeeper and Keychain may prompt for this separately signed development app.

Create profiles in the shipping DBM app first. The preview can reload those
profiles, connect to their default database, run SQL or Redis commands, display
results/notices/errors, and disconnect. Command-Return runs the selected text,
or the entire editor when there is no selection (not statement-under-cursor).
The editor opens with `SELECT 1;` or `PING`. Results are capped at 10,000 rows;
requests are limited to 1 MiB. Existing read-only profile rules still apply.

**Use disposable databases or read-only profiles first.** This is real database
access, not demo data. Query history is written to the same local SQLite store
as Tauri. Passwords stay in the OS credential store and never cross the Swift
pipe. Closing the window terminates the helper, but is not query cancellation
or a guarantee of rollback. Transport failures are never automatically replayed;
check a submitted write's outcome before rerunning it.

## Remaining migration and acceptance work

- Profile create/edit/test, schema/key browser, database switching, workbench
  tabs, statement-under-cursor, history UI, refresh, CSV, staged edits/deletes,
  connection colors, and keyboard/accessibility parity.
- Query cancellation and bounded handling of very large individual cell values.
  The existing row limit is not a byte/memory limit.
- Native Windows/Linux hosts and platform-specific credential/install testing.
- macOS compiled/runtime and visual validation, VoiceOver, IME, selection,
  clipboard, resizing, dark/light appearance, and repeated connect/disconnect.
- Signing, notarization, installation, update migration, and release acceptance.

Before any cutover, compare release builds on the **same machine and dataset**:
cold launch, idle CPU/wakeups, total resident memory of host plus all helpers,
connect/query latency, scrolling a 10k-row result, large fields, and repeated
connection cycles. Record OS/hardware and multiple runs; do not compare a debug
Tauri build against an optimized AppKit build or count only the Swift process.
No CPU/RAM reduction has been measured yet.

## Checks

```sh
npm run check
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
# On macOS, compile and sign the actual native app:
bash apps/native/macos/build.sh
```

The Rust tests retain adapter/storage coverage, including disposable live Redis
tests when available. The Linux bridge process test uses isolated XDG storage
and checks protocol ordering, malformed-input recovery, errors, and EOF exit.
They do not establish AppKit visual correctness or physical-platform acceptance.
