# Anybase features

What the app does today, what is deliberately not built yet, and what it stores locally.

## Released desktop apps

- PostgreSQL, MySQL, and Redis direct connections with disabled, preferred, or
  required TLS. Connection attempts give up after 20 seconds.
- Local connection profiles and query history in an application SQLite database.
- Each connection remembers the database last successfully selected in the
  sidebar, including Redis database indices. That selection becomes the saved
  database used when reconnecting; failed switches leave it unchanged.
- **Open on startup** in the connection editor automatically connects and opens
  a SQL or Redis workbench tab when Anybase launches, without running workbench queries.
  It defaults to on for new and existing desktop profiles and can be turned off
  per connection. Startup connection errors use the normal error banner; other
  profiles still open. Mobile clients continue to connect manually.
- Passwords through the macOS Keychain, Windows Credential Manager, or Linux
  secret service via `keyring`.
- The connection editor displays the current URL with its password masked.
  **Show password** reveals it; the password field has its own Show/Hide toggle.
  **Copy URL** always copies the full URL, including the entered or saved
  password, regardless of display visibility. Treat that clipboard content as a secret: clipboard
  managers and OS clipboard sync may retain or share it. URLs encode credentials,
  database names, ports, and TLS settings. MySQL TLS option names vary by client;
  Redis CA certificates must be configured separately in the destination app.
  Redis Preferred TLS exports as `rediss://` (no plaintext fallback). Anybase's
  read-only profile setting is not a URL option; use a restricted database user
  when connecting from another app.
- Native apps: AppKit on macOS (Apple Silicon and Intel), egui on Windows and
  Linux.
- Windows and Linux start at 1280 × 800 logical pixels and remember normal
  window sizes between launches. A startup window below the 900 × 600 minimum
  recovers to the default size, capped to the display size; maximized and
  fullscreen windows are left unchanged.
- Signed in-app updates from the GitHub Release published for each merge to
  `main`.
- Database list, schemas, tables/views, a sidebar filter for tables and keys,
  configurable previews up to 200 rows,
  structured multi-filtering, ordering, visible-page CSV copy, and full filtered
  CSV export. Redis connections show numbered databases, a SCAN-backed key
  index, and per-type key folders (strings, hashes, lists, sets, sorted sets,
  streams).
- CSV exports replace the chosen file only after every page is written
  successfully. A failed export leaves an existing file unchanged.
- Redis key-index pages scan the complete matching keyspace, deduplicate keys,
  and default to key-name order; browsing and export have no 5,000-key cutoff.
  Unfiltered key-name pages fetch type and TTL only for visible keys. Filtering
  or sorting by metadata inspects all matching keys, so large keyspaces can take
  longer. Pages and exports are not snapshots of concurrent database changes.
- Resizable sidebars and columns, collapsible wide fields, and multi-row
  selection for staged edits and deletes.
- A row inspector next to table grids that shows every field of the selected
  row and can stage edits or a delete for it, plus a pending-changes bar with
  Discard and Save.
- The Graphite dark interface described in
  [docs/design-system.md](design-system.md). Its Space Mono fonts are bundled
  with the app and never downloaded at runtime.
- Inline edits and staged deletes for primary-key-backed tables. PostgreSQL
  edits are guarded by `xmin` optimistic concurrency; MySQL edits match on the
  primary key. Redis table views edit strings, hashes, lists, sets, and sorted
  sets in place, and can delete keys from the key index. Read-only profile
  mode blocks GUI writes on every engine; PostgreSQL and MySQL sessions for
  read-only profiles are also marked read-only on the server, so writes the
  app cannot recognize are rejected too.
- Table writes carry their target database and are rejected if the connection
  switched databases while the write was queued. Redis list batches apply
  higher indices first so staged deletes do not shift later edits onto other
  elements; this does not protect against concurrent edits by other clients.
- PostgreSQL values of any type display, including `numeric`, `uuid`, enums,
  and arrays. Integers beyond ±2^53 are shown and edited as exact text.
- SQL tabs with syntax highlighting and keyword completion, query result
  grids, a 10,000-row safety cap, and per-profile history. Connecting a profile opens a query tab so you can run
  SQL immediately. Redis connections open a command workbench (`PING` by
  default) instead of SQL. Scripts with several statements show the last
  result set. The MySQL workbench keeps one connection, so `USE`, session
  variables, and explicit transactions carry over between runs.
  PostgreSQL and MySQL retain only capped query results in memory while
  consuming the remaining rows, so later statements and errors still run
  through the normal result handling. The cap does not limit server execution
  or network transfer; add SQL `LIMIT` when you need to bound those too.
- Automatically reconnects once after an idle connection closes, staying on the
  selected database, then retries read-only browsing and SQL statements. Writes
  are never retried automatically.
- Refresh on table previews and query results: reload the current page and
  filters, or re-run the last executed statement, without re-authoring them.

## Mobile development clients

The Android Compose and iPhone SwiftUI clients are development builds, not
shipped store apps. They provide direct in-process Rust connections for
PostgreSQL, MySQL, and Redis, with read-only profiles and verified Required TLS.
Queries are capped at 1,000 rows and read-only table/key pages at 25 rows.

They intentionally omit writes, exports, the updater, custom-CA UI, SSH, and
sync, and keep passwords in memory rather than the desktop OS keyring. See the
[mobile contract](mobile.md) for exact capabilities, private-network guidance,
lifecycle behavior, build workflows, and outstanding validation.

## Desktop deliberate follow-ups

SSH jump-host transport, query cancellation with dedicated sessions, Redis
Sentinel/Cluster, and encrypted profile sync are kept out of this vertical
slice. The profile model already reserves the SSH shape, but the backend
returns a clear unsupported-transport error until the forwarding
implementation is added and tested on all three OSes.

## Local data

The app stores non-secret profile metadata, settings, and up to 500 history
entries in the platform application data directory. Passwords are never put in
that SQLite file. There is no telemetry, account, or sync service.
