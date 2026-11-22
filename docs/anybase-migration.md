# Anybase rename

DBM is being renamed **Anybase**, at [anyba.se](https://anyba.se). "DBM" collides
with the Unix dbm/gdbm libraries and Python's `dbm` module, and doesn't say that
the app speaks several databases. This page tracks the stages, which names change
when, and which identifiers must never change.

## Stages

| Stage | Scope | Status |
| --- | --- | --- |
| 1. Website and branding | anyba.se site in `apps/web`; README; josevalerio.com links to anyba.se | Done |
| 2. App display name | What users see inside and around the installed app | Done, except store listings |
| 3. Download names | Anybase-named first-install downloads | Done |
| 4. Repository rename | `joswayski/dbm` to `joswayski/anybase`, and every hard-coded reference | Done |
| 5. Internal names | Crates, env vars, script names | Optional |

### 1. Website and branding

- `apps/web` builds anyba.se. See [Website](#website).
- The README leads with Anybase.
- josevalerio.com lists Anybase and links to anyba.se.

### 2. App display name

Everything users read now says Anybase; every identifier in
[Never change](#never-change) stays.

- macOS: `CFBundleName` and `CFBundleDisplayName`, the app menu (About, Hide,
  Quit), window title, sidebar, dialogs, and tooltips. New builds are
  `Anybase.app`; the DMG volume is "Anybase". The native updater installs
  whichever `.app` its zip holds at the existing install's path, so installs made
  before the rename keep their `DBM.app` folder (and the Finder and Dock name
  that comes from it) while showing Anybase inside. `DBM.app.tar.gz` keeps its
  `DBM.app` folder name for the Tauri updater.
- Windows (Win32) and Linux (GTK): window title, sidebar, dialogs, and tooltips.
- Windows installer: `Name`, the Start menu shortcut (`Anybase.lnk`, replacing
  `DBM.lnk`), and the uninstall `DisplayName`/`Publisher`. The native updater
  swaps only `dbm.exe`, so existing installs keep their DBM Start menu entry until
  the installer runs again.
- Linux AppImage desktop entry: `Name=Anybase`,
  `Comment=Database client for PostgreSQL, MySQL, and Redis`.
- Android: `android:label` and the Compose screens. iPhone: `CFBundleDisplayName`,
  `CFBundleName`, the local network prompt, and the UIKit screens.
- PostgreSQL connections report `application_name` "Anybase"; the demo fixture's
  query result says "Anybase demo".
- GitHub release titles: "Anybase <version>" and "Anybase mobile (latest)".
- README and website screenshots.

Still to do:

- Rename the Play Console app and the App Store Connect record (store-side names,
  not in this repository).
- Optional: have the macOS updater move a `DBM.app` install to `Anybase.app`.
  Skipped for now because it would break Dock and login-item references.

### 3. Download names

Every release publishes `Anybase-macOS.dmg`, `Anybase-Windows-x64-setup.exe`, and
`Anybase-Linux-x86_64.AppImage`, byte-identical copies of the DBM-named files,
and the website (`apps/web/src/downloads.ts`) and README link to them. Installed
copies keep downloading the DBM-named updater assets listed in
[Never change](#never-change), and the DBM-named installers stay for links made
before the rename.

### 4. Repository rename

The repository is now `joswayski/anybase` (formerly `joswayski/dbm`, same
repository ID 1300057641). GitHub redirects git, web, and release-download URLs
from the old name as long as nothing new is ever created at `joswayski/dbm`, so
installs from before the rename keep updating through the old manifest URL (the
updater follows redirects). Repository settings, secrets, runners, and GitHub App
installations followed the repository.

Three things in the infrastructure repo matched the repository **name**:

- **Mobile release signing.** The AWS trust policy in
  `infra/environments/production/release-signing.tf` accepted both names during
  the rename and now trusts only `repo:joswayski@22891173/anybase@1300057641`.
  The role keeps its name, `production-dbm-release-signer`.
- **Self-hosted runner hooks.** The job hooks written by `scripts/mac-ci-runner.sh`,
  `scripts/windows-ci-runner.ps1`, and `scripts/windows-ci/linux-runner.sh` now
  allow only `joswayski/anybase`.
- **Runner registration.** The runner's own registration (`config.sh --url`)
  does not follow GitHub's rename redirect, so the runner scripts register
  against `joswayski/anybase` (`ci-mac-2-anybase`, `desktop-anybase-1`,
  `ci-linux-anybase-1`). The old `dbm` runners are removed.

Every hard-coded `joswayski/dbm` reference in this repository now says
`joswayski/anybase`, including the updater's manifest URL (new builds only).

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
| `DBM.app` folder inside `DBM.app.tar.gz` | Release workflow (`tar -s`) | Break former Tauri installs' updates |
| Windows `%LOCALAPPDATA%\DBM`, `dbm.exe`, uninstall key `...\Uninstall\DBM` | `apps/native/windows/installer.nsi` | Install a second copy beside the old one |
| The `DBM` settings path (eframe's app name in the removed egui app) that the GTK app reads | `apps/native/gtk/src/settings.rs` | Reset saved window size, sidebar state, and What's new history |
| Win32 settings file `%APPDATA%\DBM\data\app.ron` and its `dbm.*` keys (eframe's file, shared with egui installs) | `apps/native/win32/app/src/store.rs` | Reset the sidebar and re-show What's new after the egui → Win32 update |
| Linux binary name `dbm-workbench` inside the AppImage | `apps/native/gtk/appimage.sh` | Break the desktop entry of existing AppImage installs |
| Window autosave name `DBMNativeMain` | `apps/native/macos/Sources/main.swift` | Reset the saved macOS window frame |
| iPhone `Application Support/DBM/dbm.sqlite` | `apps/native/ios/Sources/DBM/Bridge.swift` | Lose iPhone profiles and history |
| Android `dbm-mobile.sqlite3` | `apps/native/android/.../NativeBridge.kt` | Lose Android profiles and history |

If one of these ever has to move, ship a migration that copies the old data first
and keep reading the old location for several releases.

## Mobile identifiers

Android and iPhone both use `se.anyba.app`, in new Play Console and App Store
Connect apps named Anybase. They replaced Android `com.dbm.nativeapp` and iPhone
`app.dbm.ios` before any public store release; testers reinstall once from the
new apps. Google Play never allows a package name to change once an app exists,
and a new bundle ID is a new App Store app, so treat `se.anyba.app` as permanent.

The Kotlin source package and Android `namespace` stay `com.dbm.nativeapp`: they
are internal and never shown to users.

## Website

`apps/web` is a static [TanStack Start](https://tanstack.com/start) site deployed as
the Cloudflare Worker `anybase`. `wrangler.jsonc` attaches the `anyba.se`
custom domain, so the first deploy creates its DNS record and certificate.
Development commands are in [Development](development.md#website).

Tap a screenshot to open the in-page viewer. On mobile, pinch to zoom and drag
to pan while zoomed; swipe between screenshots when zoomed out.

**One-time setup** (Cloudflare dashboard, the account that holds the anyba.se
zone):

1. Workers & Pages → Create → Import a repository → `joswayski/anybase`.
2. Root directory `apps/web`, build command `npm run build`, deploy command
   `npx wrangler deploy`, production branch `main`. No build variables are
   needed: **Latest changes** comes from GitHub's public commit feed.

Keep `name` in `wrangler.jsonc` equal to the dashboard Worker's name (`anybase`).
Other branches and pull requests get a preview build: Workers Builds runs
`npx wrangler preview`, which needs the `previews` block in `wrangler.jsonc`.

Every push to `main` then rebuilds the site, which refreshes **Latest changes**.
Leave build watch paths unrestricted so app-only merges still refresh the list.
To deploy by hand instead: `cd apps/web && npm ci && npx wrangler login && npm run deploy`.

`https://anyba.se/privacy` is the privacy policy URL given to Google Play and the
App Store; update it (and its date) whenever what the apps store or send changes.

`www.anyba.se` is not configured; add a Cloudflare redirect rule to the apex if
it should resolve. Site-only changes never publish a desktop release
(`release.yml` ignores `apps/web/**`).
