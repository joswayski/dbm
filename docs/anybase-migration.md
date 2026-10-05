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
| 4. Repository rename (optional) | `joswayski/dbm` to `joswayski/anybase`, and every hard-coded reference | Planned |
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
- Linux: `Name=` and `Comment=` (now "Database manager"; make it "Database client")
  in the AppImage desktop entry (`apps/native/workbench/appimage.sh`).
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

Optional: the product name doesn't depend on it. GitHub redirects git, web, and
release-download URLs from a renamed repository as long as nothing new is ever
created at `joswayski/dbm`, so installed apps keep updating through the old
manifest URL (the updater follows redirects). Repository settings, secrets,
runners, and GitHub App installations follow the repository.

Three things match the repository **name** and break on rename. Make each accept
both names and roll it out **before** renaming:

- **Mobile release signing.** The AWS trust policy in the infrastructure repo's
  `infra/environments/production/release-signing.tf` only accepts
  `repo:joswayski@22891173/dbm@1300057641`. After a rename GitHub sends the new
  name, AWS refuses the role, and Play/TestFlight releases fail. Add the new
  subject and `tofu apply`.
- **Self-hosted Mac runner.** The job hook written by `scripts/mac-ci-runner.sh`
  only allows `joswayski/dbm`, so main-branch macOS jobs, including release
  builds, fail on `josemac`. Add the new name and rerun the setup on the Mac.
- **Godis mobile Deploy button.** `src/deployments.rs` requests a GitHub App
  token scoped to the repository named `dbm` (`DBM_REPOSITORY`), which no longer
  exists after a rename. Update it and deploy Godis right after the rename. Keep
  the `dbm-mobile` button slug: buttons already posted in Discord carry it.

After the rename, update the remaining references (these keep working through
redirects meanwhile):

- `apps/web/src/site.ts` (`REPOSITORY`) and the download URLs in
  `apps/web/src/downloads.ts`.
- `crates/dbm-update/src/lib.rs` manifest URL (new installs only; old ones rely on
  the redirect).
- Infrastructure `scripts/store-release-signing-secrets.sh` and
  `scripts/store-deploy-notification-webhook.sh`, plus their docs and tests.
- Local clones: `git remote set-url origin https://github.com/joswayski/anybase`.
- Remove the old name from the signing trust policy and runner hook.

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
the Cloudflare Worker `anybase`. `wrangler.jsonc` attaches the `anyba.se`
custom domain, so the first deploy creates its DNS record and certificate.
Development commands are in [Development](development.md#website).

Tap a screenshot to open the in-page viewer. On mobile, pinch to zoom and drag
to pan while zoomed; swipe between screenshots when zoomed out.

**One-time setup** (Cloudflare dashboard, the account that holds the anyba.se
zone):

1. Workers & Pages → Create → Import a repository → `joswayski/dbm`.
2. Root directory `apps/web`, build command `npm run build`, deploy command
   `npx wrangler deploy`, production branch `main`. No build variables are
   needed: **Latest changes** comes from GitHub's public commit feed.

Keep `name` in `wrangler.jsonc` equal to the dashboard Worker's name (`anybase`).
Other branches and pull requests get a preview build: Workers Builds runs
`npx wrangler preview`, which needs the `previews` block in `wrangler.jsonc`.

Every push to `main` then rebuilds the site, which refreshes **Latest changes**.
Leave build watch paths unrestricted so app-only merges still refresh the list.
To deploy by hand instead: `cd apps/web && npm ci && npx wrangler login && npm run deploy`.

`www.anyba.se` is not configured; add a Cloudflare redirect rule to the apex if
it should resolve. Site-only changes never publish a desktop release
(`release.yml` ignores `apps/web/**`).
