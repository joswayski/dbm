# DBM — agent notes

DBM is a local-first desktop database manager (TablePlus / DataGrip style) built with **Tauri 2 + React + Rust**. The first vertical slice is PostgreSQL: saved connections, schema exploration, paginated table browsing, SQL workbench tabs, and safe local profile storage.

## Layout

| Path | Role |
|------|------|
| `apps/desktop/` | Tauri app package (`@dbm/desktop`) |
| `apps/desktop/ui/src/` | React UI (Vite) |
| `apps/desktop/src-tauri/src/` | Rust backend (commands, Postgres, keyring, storage) |
| `docs/` | Release / publishing docs |
| `scripts/` | Build and install helpers |

There is no separate monorepo package for the UI; frontend and backend live under `apps/desktop`.

## Stack and conventions

- **UI:** React 19, Zustand store (`store.ts`), TypeScript, CodeMirror for SQL.
- **Backend:** Rust, Tokio, PostgreSQL via the workspace crate graph; profiles and history in a local SQLite app DB; passwords only in the OS keyring.
- **IPC:** UI calls Tauri commands through `commands.ts`. Browser/Vite preview uses in-memory mocks in that same module so layout work does not require Tauri.
- Prefer small, focused changes. Match existing naming (`profileId`, `workspace`, tab kinds `"table" | "query"`).
- Do not add telemetry, cloud sync, or network calls that send connection data off-device.

## Common commands

```sh
npm install
cargo test --workspace
npm run check          # typecheck + lint + unit tests (desktop UI)
npm run test           # UI tests only
npm run dev            # Tauri dev
npm run build          # native build (+ install/launch on macOS by default)
```

UI tests use Vitest + Testing Library under `apps/desktop/ui/src/*.test.tsx`.

## Product behavior to preserve

- **Local-only:** connection profiles, query history, and results stay on the machine. Passwords never go into the app SQLite file.
- **Connect → query:** opening a connection should land the user in a SQL query tab so they can run statements immediately (schema tree remains in the sidebar).
- **Table tabs:** paginated previews, filters, ordering, CSV copy/export, PK-backed edits with `xmin` concurrency, read-only profiles.
- **Query tabs:** run statement under cursor or selection (⌘/Ctrl+Enter), history per profile+database, results capped (10k rows). Simple `SELECT * FROM table` can open the editable table viewer.
- **Refresh:** table and query result views should be re-fetchable without re-authoring filters or SQL (toolbar Refresh).
- SSH jump hosts, Redis, and encrypted profile sync are intentional non-goals until documented otherwise.

## Working style

- Prefer a GitHub PR over committing straight to `main` unless the user says otherwise.
- Use `gh` for GitHub operations (works better with private repos).
- Do not wait on CI unless the user asks; they will report failures.
- When unsure about UX for a desktop DB client, prefer TablePlus/DataGrip-like defaults: fast path to query, obvious refresh, non-destructive confirms for bulk writes.
