# DBV

DBV is a local-first desktop database viewer for macOS, Windows, and Linux.
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

DBV never uploads connection profiles, query history, or database results.
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
