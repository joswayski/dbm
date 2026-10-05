# Anybase rename

DBM is being renamed **Anybase**, at [anyba.se](https://anyba.se). "DBM" collides
with the Unix dbm/gdbm libraries and Python's `dbm` module, and doesn't say that
the app speaks several databases. This page tracks the stages, which names change
when, and which identifiers must never change.

## Stages

| Stage | Scope | Status |
| --- | --- | --- |
| 1. Website and branding | anyba.se site in `apps/web`; README; josevalerio.com links to anyba.se | In progress |
| 2. App display name | What users see inside and around the installed app | Planned |
| 3. Download names | Anybase-named first-install downloads | Planned |
| 4. Repository rename | `joswayski/dbm` to `joswayski/anybase`, and every hard-coded reference | Planned |
| 5. Internal names | Crates, env vars, script names | Optional |

### 1. Website and branding

- `apps/web` builds anyba.se. See [Website](#website).
- The README leads with Anybase and says the apps still show DBM.
- josevalerio.com lists Anybase and links to anyba.se.

### 2. App display name

Change only what people read; keep every identifier in
[Never change](#never-change).

- macOS: `CFBundleName` (add `CFBundleDisplayName`), menu and About titles, window
  titles. Decide whether the bundle folder becomes `Anybase.app`: check how
  `crates/dbm-update` swaps the bundle in place before renaming it, and keep the
  in-place update path working for existing `DBM.app` installs.
- Windows (`apps/native/windows/installer.nsi`): `Name`, the Start menu shortcut,
  and the uninstall `DisplayName`/`Publisher`. Keep `InstallDir`
  (`%LOCALAPPDATA%\DBM`), the `UNINSTALL_KEY`, and `dbm.exe`, so upgrades land on
  top of existing installs instead of beside them. Delete the old `DBM.lnk` when
  writing the new shortcut.
- Linux: `Name=` in the AppImage desktop entry (`apps/native/workbench/appimage.sh`).
- egui window title and About text; update and release-note copy.
- Mobile display names (`app_name` on Android, `CFBundleDisplayName` on iPhone).
- Refresh `docs/screenshots` (and the website's WebP copies) once the sidebar
  shows Anybase.

The AppKit app only builds on macOS, so this stage needs a Mac run before merge.

### 3. Download names

Publish `Anybase-macOS.dmg`, `Anybase-Windows-x64-setup.exe`, and
`Anybase-Linux-x86_64.AppImage` **in addition to** the DBM-named files, then point
the website and README at them. Installed copies keep downloading the DBM-named
updater assets listed in [Never change](#never-change).

### 4. Repository rename

GitHub redirects git, API, and release-download URLs from a renamed repository as
long as nothing new is ever created at `joswayski/dbm`. Installed apps therefore
keep updating through the old manifest URL. In the same window as the rename:

- `apps/web/src/site.ts` (`REPOSITORY`) and the download URLs in
  `apps/web/src/downloads.ts`.
- `crates/dbm-update/src/lib.rs` manifest URL (new installs only; old ones rely on
  the redirect).
- Godis `src/deployments.rs` `DBM_REPOSITORY`. Keep the `dbm-mobile` button slug:
  buttons already posted in Discord carry it in their custom IDs.
- Infrastructure: `scripts/store-release-signing-secrets.sh`,
  `scripts/store-deploy-notification-webhook.sh`, `scripts/mac-ci-runner.sh`,
  `infra/environments/production/release-signing.tf`, and their docs and tests.
- Confirm the self-hosted `josemac` runner and the GitHub App installations still
  list the repository.
- Cloudflare Workers Builds: reconnect the repository if the build stops
  triggering.

### 5. Internal names

`dbm-core`, `dbm-update`, `dbm-sign`, `DBM_*` environment variables, and the
`dbm` executable names are invisible to users. Rename them only alongside other
work in the same files.

## Never change

These identify existing installs and their data. Changing one silently drops
users' saved state or strands them on an old version.

| Identifier | Where | Changing it would |
| --- | --- | --- |
| Keychain service `io.github.joswayski.dbm` | `crates/dbm-core/src/keyring_store.rs` | Lose every saved password |
| Data directory `ProjectDirs::from("io", "github", "dbm")` | `crates/dbm-core/src/storage.rs` | Lose connections, history, and startup settings |
| macOS bundle ID `io.github.joswayski.dbm` | `apps/native/macos/Info.plist` | Make macOS treat Anybase as a different app |
| `native-latest.json`, `latest.json` | Release assets | Stop native and former Tauri installs from updating |
| `DBM-macOS.zip`, `DBM-Windows-x64.exe`, `DBM-Linux-x64`, `DBM.app.tar.gz`, `DBM-Windows-x64-setup.exe`, `DBM-Linux-x86_64.AppImage` | Release assets ([releases](releases.md)) | Break the updaters that download them |

If one of these ever has to move, ship a migration that copies the old data first
and keep reading the old location for several releases.

## Decide before a public store release: mobile identifiers

Android `com.dbm.nativeapp` and iPhone `app.dbm.ios` are only used for Play
internal testing and TestFlight today. Google Play never allows a package name to
change once an app is published, and a new bundle ID is a new App Store app. A
rename is cheapest now: it means a new Play app and App Store Connect record and
new signing entries in the release pipeline, but no users to move.

## Website

`apps/web` is a static [TanStack Start](https://tanstack.com/start) site deployed as
the Cloudflare Worker `anybase-web`. `wrangler.jsonc` attaches the `anyba.se`
custom domain, so the first deploy creates its DNS record and certificate.
Development commands are in [Development](development.md#website).

**One-time setup** (Cloudflare dashboard, the account that holds the anyba.se
zone):

1. Workers & Pages → Create → Import a repository → `joswayski/dbm`.
2. Root directory `apps/web`, build command `npm run build`, deploy command
   `npx wrangler deploy`, production branch `main`.
3. Optional: add a `GITHUB_TOKEN` build variable (a fine-grained token with
   read-only access to public repositories). Unauthenticated GitHub API calls are
   limited to 60 an hour per IP, and build machines share IPs.

Every push to `main` then rebuilds the site, which refreshes **Latest changes**.
Leave build watch paths unrestricted so app-only merges still refresh the list.
To deploy by hand instead: `cd apps/web && npm ci && npx wrangler login && npm run deploy`.

`www.anyba.se` is not configured; add a Cloudflare redirect rule to the apex if
it should resolve. Site-only changes never publish a desktop release
(`release.yml` ignores `apps/web/**`).
