# Repository guidance

## Product

- Anybase (formerly DBM) is a database client. AppKit macOS and egui Windows/Linux
  are released desktop apps; Compose Android and SwiftUI iPhone are development
  clients, not store releases or desktop-parity products.
- The current milestone is PostgreSQL, MySQL, and Redis: saved connections, schema or keyspace exploration, paginated table and key browsing, SQL/Redis workbench tabs, PK-backed or key-backed edits, and safe local profile storage.
- Clearly separate current features from roadmap ideas. Do not present Redis Sentinel/Cluster, SSH jump hosts, encrypted profile sync, or other follow-ups as shipped unless the repository already implements them.
- Prefer TablePlus/DataGrip-like defaults when UX is ambiguous: fast path to query, obvious refresh, non-destructive confirms for bulk writes.
- Public copy (README, anyba.se, store listings, the GitHub description) calls it "a fast database client for PostgreSQL, MySQL, and Redis". Don't market it as local-first or as a database manager; the local-only behavior below still applies.

## Repository map

- `apps/native/macos` is the macOS app (Swift/AppKit), built by `build.sh`; it calls Rust through the C ABI in `apps/native/bridge`.
- `apps/native/workbench` is the Windows and Linux app (Rust egui/wgpu) with its own Cargo workspace; `apps/native/windows` holds its installer, `apps/native/icons` the app icons.
- `apps/native/android` and `apps/native/ios` are development clients. Both call
  the mobile C ABI in-process; there is no hosted proxy. Their contract and
  validation status live in `docs/mobile.md`.
- `crates/dbm-core` owns database adapters, sessions, keyring, and local SQLite storage; `crates/dbm-update` is the self-updater; `tools/dbm-sign` signs release artifacts.
- `docs/` holds features, development setup, releases (signing, notarization, publishing), the native architecture, the design system, and screenshots.
- `scripts/release.mjs` picks release versions and notes for the release workflow; `scripts/release-highlights.mjs` writes the user-facing highlights the apps show (`docs/releases.md`).
- `apps/web` is the anyba.se website (static TanStack Start on a Cloudflare Worker). Changes there do not publish a desktop release.
- The app is named Anybase in everything users see; many internal identifiers still say DBM. Follow `docs/anybase-migration.md`, and never change the identifiers it lists under "Never change".
- The Tauri/React app was removed; its installs update onto the native apps through `latest.json` (see `docs/releases.md`). Keep that path working.

## Working conventions

- Keep changes focused on the request and preserve existing behavior unless a change is intentional.
- Do not overwrite, stage, or publish unrelated work already present in the checkout.
- Assume other agents may be working in this repository concurrently. Prefer a separate Git worktree for new work (it still lands on `main`; see Shipping), and never stash, switch, or overwrite another agent's checkout without explicit instruction.
- Reuse established patterns in the repository before introducing a new abstraction or dependency. Match existing naming (`profileId`, `workspace`, tab kinds `"table" | "query"`).
- Treat macOS, Windows, and Linux parity as the default. Prefer shared UI and logic that works on every supported platform; when a fix or feature must be platform-specific, implement or stub the equivalent path on the others (or explicitly gate with `cfg` / runtime checks), and document any unavoidable limitation in the commit message and README if user-facing. Do not assume “works on my Mac” is enough—call out what was and was not verified on other platforms.
- Do not add telemetry, cloud sync, or network calls that send connection profiles, query history, or database results off-device.
- Every push to `main` that changes the app is built and published as the latest release, and installed builds update to it. Keep `main` releasable.
- Keep this file concise and update it when a recurring repository convention or correction should persist across future work.

## Visual design

- Anybase uses the Graphite design system (`docs/design-system.md`): neutral graphite surfaces, hairline borders, Space Mono for UI and code (bundled, never fetched from a font service at runtime), and a system-blue accent for focus, selection, and the single primary action. Use real regular/bold faces; keep all clients, the website, and the doc in sync.
- Connection identity is multi-color: each profile has its own color for sidebar, tabs, and main-pane theming. Do not force a single accent across all connections.
- Establish hierarchy with typography, spacing, and dense-but-readable layout before adding color. Prefer restrained shadows, small corner radii, and concise UI copy.
- Preserve accessible contrast on dark surfaces. State colors keep stable meanings: `--modified` for staged edits, `--danger` for staged deletes and destructive actions, `--success` for success (and future inserts).

## Product behavior to preserve

- **Local-only:** connection profiles, query history, and results stay on the
  machine. Desktop passwords use the OS keyring / credential store; mobile
  passwords are memory-only and cleared on disconnect/background/disposal.
  Passwords are never written to the app SQLite file.
- **Connect → query:** opening a connection should land the user in a SQL query tab so they can run statements immediately (schema tree remains in the sidebar). Selecting an already-connected profile should keep or restore that profile's workbench, not dump the user on the empty welcome pane. Deleting or disconnecting a profile must close its tabs.
- **Table tabs:** paginated previews, filters, ordering, CSV copy/export, PK-backed edits (PostgreSQL `xmin` concurrency; MySQL primary-key matching), Redis key index and typed key views, read-only profiles.
- **Query tabs:** run statement under cursor or selection (⌘/Ctrl+Enter), history per profile+database, results capped (10k rows). Simple `SELECT * FROM table` can open the editable table viewer. Redis connections use a command workbench instead of SQL.
- **Refresh:** table and query result views should be re-fetchable without re-authoring filters or SQL (toolbar Refresh).
- SSH jump hosts, Redis Sentinel/Cluster, and encrypted profile sync are intentional non-goals until documented otherwise.
- Mobile additionally requires read-only profiles and verified Required TLS,
  caps query results at 1,000 rows and table pages at 25, and has no writes,
  exports, updater, custom-CA UI, SSH, or sync. Backgrounding invalidates UI and
  queues disposal behind active calls; it does not cancel server work. Keep
  networking guidance in `docs/mobile.md` and never suggest exposing a database
  port publicly.

## Documentation

- Every change must leave the root `README.md` and the docs it links to accurate. Update it when a change affects features, platform support, setup, build commands, privacy, networking, releases, or what is implemented vs planned.
- Keep the root README short and visual: screenshots, the download link, a few feature bullets, and links. Put details in `docs/`: features and follow-ups in `docs/features.md`, setup and builds in `docs/development.md`, releases in `docs/releases.md`.
- If a change does not need a README edit, still verify that its changes do not make the README inaccurate; do not add no-op wording solely to touch the file.
- Keep current behavior and roadmap / deliberate follow-ups distinct, especially for adapters and transports that are not implemented yet.
- Describe mobile CI as workflows that build/test/capture development clients,
  not proof that checks passed. iOS cannot be compiled on Linux; physical-device
  and live TLS database validation remain outstanding until explicitly recorded.

## Validation

- For Rust changes, run `cargo fmt --all -- --check`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings`, and the same three with `--manifest-path apps/native/workbench/Cargo.toml` for the egui app.
- Common local commands:

  ```sh
  cargo test --workspace
  cargo run --manifest-path apps/native/workbench/Cargo.toml --release -- --demo   # Windows/Linux app, fixture data
  bash apps/native/macos/build.sh && open target/native/Anybase.app                # macOS app
  npm --prefix apps/web ci && npm --prefix apps/web run check
  npm --prefix apps/web run test:all                                               # website and release versioning
  ```

- The AppKit app can only be built on macOS; say so when a change to it was not built.
- Report exactly which checks ran and any checks that could not run.

## Shipping

`main` is production: a push to `main` that changes the app becomes the latest
release, and installed apps update to it within minutes. Speed beats ceremony:
there are no real users yet, so ship small, watch it land, and fix forward.

- Before you push, run the local checks for what you changed and build what you
  touched. CI on `main` is a signal, not a gate: nothing waits for it.
- Push a branch and open a draft pull request instead of pushing to `main`
  when:
  - the work is unfinished and you have to stop (session ending, disk full,
    missing toolchain). The draft PR is the handoff: say what is done and what
    is left.
  - it needs a product decision the owner hasn't made.
- Otherwise commit and push straight to `main`: `git pull --rebase origin main`, then `git push origin HEAD:main`, retrying the same way if the push is rejected. Never force-push `main` or rewrite other agents' commits. Use `gh` for GitHub operations (or the GitHub MCP tools where `gh` is unavailable).
- Commit subjects feed the desktop release highlights and are the fallback release notes (`scripts/release.mjs`), so write them for users.
- Never put literal CI-skip directives in commit messages, even to explain them: GitHub honors them anywhere in the message, and the path-filtered `Release` and `Mobile development builds` runs would silently not happen.
- Pushes to `main` run `CI` (about 5 minutes); `Release` unless only Markdown, `docs/`, `apps/web`, agent tooling, mobile-only scripts or the CI/mobile/fallback workflows changed (it signs and publishes the latest desktop release, which installed apps update to, in about 5-10 minutes); `Mobile development builds` when mobile files changed (builds plus Android unit tests and lint; its UI suites run nightly, so don't start it for routine changes); and Cloudflare Workers Builds, which deploys anyba.se.
- Everything deploys automatically after a push; you watch it land and fix forward. After pushing, check each step in this order. `SHA` is your pushed commit; list its runs with `gh run list --repo joswayski/dbm --commit "$SHA"` and wait with `gh run watch <id> --repo joswayski/dbm --exit-status`. Report what shipped.
  1. Infrastructure or secrets the change needs (release-signing role, deploy webhook, self-hosted runners) in `joswayski/infrastructure`, following that repository's guidance. Anybase has no server, so no `deploy-*.yml` workflow there deploys this repository.
  2. Website: Workers Builds deploys `main` on its own. Confirm with `gh api "repos/joswayski/dbm/commits/$SHA/check-runs" --jq '.check_runs[] | select(.name == "Workers Builds: anybase") | .conclusion'`.
  3. Desktop apps: watch the automatic `Release` run for `SHA` (releases run one at a time, so it may wait for an earlier one). Start it by hand with `gh workflow run release.yml --repo joswayski/dbm --ref main` only when a needed release was skipped.
  4. Mobile apps, if mobile files changed: when `Mobile development builds` succeeds for `SHA`, `mobile-ready.yml` starts `Mobile release` for it automatically (about 15 minutes; signs and uploads to TestFlight and Google Play internal testing). Watch it with `gh run list --repo joswayski/dbm --workflow mobile-release.yml --limit 3`.
- If `CI`, a build or a release fails, fix it forward with another commit. A failed release removes its own draft and tag. A published desktop release cannot be taken back (installed apps already updated), so ship a fix or revert commit, which publishes a newer release.
- Still ask the owner first for anything irreversible: deleting releases, tags, data or infrastructure; profile-storage migrations that drop data; rotating signing keys or secrets; or changing the identifiers listed under "Never change" in `docs/anybase-migration.md`.
