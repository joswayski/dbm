# DBM → AnyBase migration checklist

Inventory date: October 4, 2026. **Planning only:** no repository, domain, app,
key, or infrastructure has been renamed or recreated. Check items off after
implementation and verification; add the PR/operator result beside each item.
Mark optional or absent integrations `N/A` rather than adding them to the rename.

The new product name is **AnyBase**. The intended domain is **`anyba.se`**
(“any” + “ba” + `.se`); Jose will purchase it. Domain, trademark, store-name,
and repository-name availability have not been verified.

## DBM's actual migration boundary

DBM is a **local-first database manager**: AppKit macOS and egui Windows/Linux
desktop apps, with Compose Android and SwiftUI iPhone development clients. Clients
connect directly to user-configured PostgreSQL, MySQL, and Redis. No DBM hosted
API, product website, account/login service, telemetry, or cloud sync is configured
in this repository. A new domain does not change database hosts or credentials.

Desktop releases/downloads and update manifests live on GitHub. Desktop updater
and macOS signing secrets live in GitHub's `release` environment. Mobile signing
uses a separate AWS role and the **shared** `production/signing/release` secret.
These are different signing paths; do not copy Caper's updater scheme onto DBM.

The owner is comfortable recreating identities because nobody is using the app.
Recommend fresh AnyBase **mobile app IDs**, allowing reinstalls, while preserving
desktop profiles/history, OS credentials, and existing updater trust by default.
Rebranding does not require deleting local data or rotating keys. A deliberate
clean-start alternative must explicitly account for lost settings/profiles and
manual reinstalls, not silently strand previous releases.

## Proposed final names

`se.anyba` is the reverse-DNS prefix for `anyba.se`. Confirm these once before
registering new apps; old internal storage names can remain during transition.

| Surface | Current, verified from source | Proposed target |
| --- | --- | --- |
| Product / app title | DBM | AnyBase |
| GitHub repository | `joswayski/dbm` | Rename existing repo to `joswayski/anybase` |
| Product domain | None configured | `anyba.se` for website/download/support/privacy pages |
| macOS bundle ID | `io.github.joswayski.dbm` | `se.anyba.macos` |
| iOS bundle ID | `app.dbm.ios` | `se.anyba.ios` |
| Android application ID / namespace | `com.dbm.nativeapp`, debug suffix `.debug` | `se.anyba.android`, debug suffix `.debug` |
| Desktop bundles / binaries | `DBM.app`, `dbm`, `dbm-workbench` | `AnyBase.app`, `anybase`, `anybase-workbench` |
| Rust crates / signing tool | `dbm-core`, `dbm-update`, `dbm-native-bridge`, `dbm-sign` | Corresponding `anybase-*` names, if renaming internals |
| Desktop credential service | `io.github.joswayski.dbm`, accounts `profile-{UUID}` | Retain initially; migrate to `se.anyba.credentials` only with tested credential transfer |
| Mobile AWS signing role | `production-dbm-release-signer` | Can retain initially; eventually `production-anybase-release-signer` |
| App-specific Play secret section | `dbm_google_play` | Can retain initially; eventually `anybase_google_play` |
| Discord deployment application ID | `dbm-mobile` | Eventually `anybase-mobile`, coordinated with Godis |

## 1. Jose: decisions and account setup

- [ ] Check name conflicts and purchase **`anyba.se`**. Enable registrar MFA,
  auto-renew, recovery contacts, and DNS access. Other defensive domains are
  optional; redirect any purchased aliases to one canonical domain.
- [ ] Confirm `joswayski/anybase`, the AnyBase store name, and the proposed app
  IDs are available. Reserve social handles only if wanted.
- [ ] Confirm fresh mobile identities/reinstalls and whether to preserve or
  explicitly reset existing mobile profiles/history. Changing IDs creates new
  sandboxes; this is not an in-place name change.
- [ ] Confirm desktop data/update continuity, which internal names will remain
  temporarily, and any app-specific keys that should be regenerated. Preserve
  other projects' signing material regardless of this app's clean-start choice.
- [ ] Inspect existing App Store Connect/TestFlight, Play Console, Google Cloud,
  GitHub `release` settings, and AWS signer configuration. Record which planned
  resources actually exist and which apps share credentials before recreating them.

## 2. Domain, website, privacy, and support

There is **no old product domain configured in this repo**. Only `anyba.se` needs
buying; subdomains are DNS records. This is website setup, not database-proxy or
backend deployment. Include any out-of-repo website found during the console audit.

| Proposed URL / address | Purpose / decision |
| --- | --- |
| `https://anyba.se` | Canonical product landing/download page, if wanted |
| `https://www.anyba.se` | Optional HTTPS redirect to apex |
| `https://anyba.se/privacy` | Stable privacy-policy URL for store listings; accurately describe local storage, credentials, and direct database connections |
| `https://anyba.se/support` | Support/contact page for store listings and app links |
| `support@anyba.se`, `privacy@anyba.se` | Receiving inboxes or forwarding aliases; choose a mail provider |

- [ ] Choose DNS/site hosting, delegate nameservers, and verify HTTPS before
  adding the URLs to app/store metadata. If using Cloudflare, create the new zone
  and appropriate records; do not attach it to Caper's app routes by assumption.
- [ ] If building a landing page, make that a separate implementation task.
  Keep database profiles, query history/results, and credentials off the site.
  Downloads can continue to link to GitHub Releases; do not move update hosting
  merely because a new domain exists.
- [ ] Publish accurate privacy/support pages and update GitHub homepage,
  store privacy/support/marketing URLs, and any newly added app help links.
- [ ] Configure receiving email and required provider MX/SPF/DKIM/DMARC records;
  test the inboxes. No DBM login email/SES identity or API subdomain is required.

## 3. GitHub, CI, AWS OIDC, and Discord

- [ ] Prepare a short rename/release window. Rename the **existing** GitHub
  repository, preserving its history, issues, releases, and numeric repository
  ID `1300057641`; do not create a replacement repo with empty history.
- [ ] Update clone remotes, repo description/homepage, README download links,
  update endpoints, script defaults, badges, attestation verification commands,
  and external integration repo references. GitHub redirects Git/web URLs, but
  hosted-action references do not redirect; do not reuse the old `dbm` repo name.
- [ ] Re-read the exact GitHub OIDC configuration after renaming and update
  AWS mobile signer trust **if needed**. Current live configuration reports
  `use_immutable_subject: true` and prefix
  `repo:joswayski@22891173/dbm@1300057641`. Do not guess the post-rename prefix or
  assume redirects prove AWS access. Preserve `main` / protected `release` limits.
- [ ] If renaming the AWS signer role, coordinate infrastructure map/resource
  addresses, IAM policy/output names, and `RELEASE_SIGNER_ROLE`. Provision/test
  the replacement before retiring the old role. DBM needs **no ECR repository,
  k3s workload, runtime app secret, or hosted-app deployment catalog entry**.
- [ ] Audit GitHub repo/environment/organization secret names, variables,
  GitHub App access, webhooks, rulesets, required checks, and release-environment
  policies. Keep desktop signing isolated to the protected `release` environment.
- [ ] Coordinate Godis's `dbm-mobile` allowlisted route to the renamed repository
  and `mobile-release.yml`, plus the `production-deploy:v1:dbm-mobile:<SHA>` button,
  notification labels and webhook sync-script repo list. If changing the route
  ID, update both producer and dispatcher together. Keep exact-tested-SHA approval.
- [ ] Update the Amp project's repository association/name and saved links.
  The current umbrella project is `dbm-mobile-caper`; preserve its other repo
  associations rather than treating that project name as DBM's GitHub repo name.
- [ ] Verify desktop protected-environment access and mobile AWS role assumption
  after renaming. Verify Discord dispatch reaches the intended repo/workflow.

Owners: `.github/workflows/{release,mobile,mobile-ready,mobile-release}.yml`,
`scripts/{release.mjs,play-upload.py,testflight-distribute.py,update-discord-mobile-release.sh}`;
in `joswayski/infrastructure`, `infra/environments/production/release-signing.tf`,
`scripts/store-deploy-notification-webhook.sh`, and `docs/gitops-runbook.md`.
Inspect Godis's owning code before modifying its dispatch protocol.

## 4. Local profiles, passwords, and settings

These are the main data-loss risks of a desktop rename, even without public users.

| Persisted identity | Current location / owner |
| --- | --- |
| Desktop profiles and query history | `ProjectDirs::from("io", "github", "dbm")` + `dbm.sqlite3`; `crates/dbm-core/src/storage.rs` |
| Desktop passwords | OS keyring service `io.github.joswayski.dbm`, account `profile-{UUID}`; `crates/dbm-core/src/keyring_store.rs` |
| macOS sidebar/window preferences | Bundle-scoped UserDefaults, `dbm.sidebarWidth`, `dbm.sidebarCollapsed`, frame name `DBMNativeMain` |
| Windows/Linux layout preferences | eframe app identity `DBM`, including `dbm.sidebarCollapsed` |
| iOS profiles/history | Protected, backup-excluded `Application Support/DBM/dbm.sqlite` inside the app sandbox |
| Android profiles/history | App-private `filesDir/dbm-mobile.sqlite3`; Android backup disabled |
| Mobile passwords | Memory-only; no persistent Keychain/Android Keystore password migration |

- [ ] Retain the desktop storage path/filename initially, or implement a
  one-time, non-destructive migration with a consistent SQLite backup while the
  app is closed. Preserve profile UUIDs, history, selections, and open-on-startup
  settings; do not overwrite an existing destination store.
- [ ] Retain the credential service initially, or transfer entries by existing
  profile UUID. Copy passwords only between OS credential stores, never into
  SQLite, exports, logs, or migration reports. Verify Keychain access/permission
  behavior under the newly signed macOS bundle identity.
- [ ] Migrate or deliberately reset UserDefaults/eframe layout preferences when
  changing app identities and preference keys. Record the expected behavior.
- [ ] Document fresh mobile sandbox behavior: new app IDs do not inherit local
  profiles/history. Export/import is not a shipped mobile feature; do not promise
  automatic transfer. Keep passwords memory-only and cleared on background/disposal.
- [ ] Test with an existing multi-profile desktop store and credential entries
  on macOS, Windows, and Linux, plus fresh mobile installs. Verify stored data,
  read-only flags, TLS settings, and password access without exposing database ports.

## 5. Desktop downloads, packaging, and updater continuity

- [ ] Rename app/window/menu titles, macOS `.app` and bundle ID, Windows installer
  labels/shortcuts/publisher/uninstall registration, Linux desktop entry/AppImage
  metadata, and icon references. Update archive contents and executable paths,
  not just release filenames.
- [ ] Coordinate `%LOCALAPPDATA%\DBM\dbm.exe`, the Windows `Uninstall\DBM` key,
  and legacy Tauri `/P` / `/R` installer handling. Preserve update detection or
  implement a tested move; do not leave duplicate app/uninstall entries or delete
  the local profile store on uninstall.
- [ ] Rename DMG, universal macOS zip, legacy app tarball, Windows executable /
  installer, Linux bare executable/AppImage, `.sig` files and checksum/provenance
  entries consistently. Keep the no-admin Windows install behavior and macOS
  signing/notarization/stapling checks. There is no current native `.deb` package.
- [ ] Update `crates/dbm-update`'s GitHub manifest URL and user agent. Preserve
  **both** `native-latest.json` and legacy Tauri-format `latest.json`, their
  platform keys, and the `native-preview` transition path. Test old DBM installers
  updating into AnyBase; retain legacy-compatible payload/layout where necessary.
- [ ] Keep the dedicated **minisign** updater key by default and sign the actual
  artifact bytes with it. DBM does not use Caper's `caper_update` Ed25519-manifest
  secret. Renaming a GitHub secret label is not key rotation. If a new key is
  wanted, ship a trusted transition first or explicitly choose manual reinstall.
- [ ] Keep date-based versions/build numbers increasing and mobile downloads
  isolated in the `mobile-latest` prerelease. That prerelease must never become
  GitHub's desktop latest target. Check versionCode/build numbering if workflows
  are renamed or their run counters reset.
- [ ] Verify first install, old→new update, new→new update, signature rejection
  for tampered/wrong-key artifacts, restart and uninstall on each desktop OS.

Owners: [release mechanics](releases.md), `.github/workflows/release.yml`,
`crates/dbm-update`, `tools/dbm-sign`, `apps/native/macos/{Info.plist,build.sh}`,
`apps/native/windows/installer.nsi`, `apps/native/workbench/appimage.sh`, and
`scripts/release.mjs`.

## 6. Credentials: what needs changing or can be regenerated

Never include private keys, passwords, service-account JSON, database URLs,
credential dumps, or user profile contents in this document/PR. Store backups
privately. Rename `DBM_*` build variables only with all consumers updated.

| Current credential / config | Required treatment |
| --- | --- |
| GitHub `release`: `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | DBM-only minisign updater key. Keep material for existing installs; labels may stay or be renamed with workflow consumers. New material requires a trusted rotation transition or manual reinstalls. |
| GitHub `release`: `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD` | Desktop Developer ID `.p12` + password. A new product/bundle name does not require a new valid team certificate. Reuse unless independently rotating. |
| GitHub `release`: `KEYCHAIN_PASSWORD` | Temporary CI keychain password; may be regenerated independently. It is not a user's database Keychain password or updater key. |
| GitHub `release`: `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_PRIVATE_KEY` | Desktop notarization API identity. Can stay; check permissions/expiry and securely update together if rotated. |
| AWS shared secret: `apple` | Mobile App Store Connect private key/key ID/issuer/team ID. Keep shared material; new bundle ID needs registration/provisioning and appropriate app access, not automatically a new team/key. |
| AWS shared secret: `android` | Shared upload keystore/passwords/alias. Can sign the fresh AnyBase package. If wanting a new AnyBase-only keystore, first add an app-specific section and consumer; do not overwrite this shared section. |
| AWS shared secret: `dbm_google_play` | DBM-only Play uploader service-account JSON/client email. Reuse with access restricted to the new AnyBase app, or create a new app-specific account/key and migrate to `anybase_google_play`. No fallback to Caper's `google_play`. |
| AWS `production-dbm-release-signer` | IAM role/trust policy, not a long-lived AWS access key. Re-check repo OIDC subject; optional role rename requires coordinated infrastructure/workflow changes. |
| GitHub `DEPLOY_NOTIFICATION_WEBHOOK_URL` | Shared release notification hook. Update branding/dispatch routing; no rotation required solely for repo rename. |
| Users' database passwords, TLS certificates, private-network/Tailscale access | Unchanged by product/domain rename. Preserve OS credential-store access and user-configured endpoints; do not rotate shared database/cache credentials. |
| Azure Artifact Signing config in release docs | Optional future Windows signing; not configured in CI today. If separately provisioned, audit exact renamed-repo OIDC subject. Keep Windows unsigned/SmartScreen limitations accurate until implemented. |

- [ ] Record keep/rename/regenerate decisions and secret **locations** for each
  applicable row. Back up app-specific signing material privately before changes.
- [ ] Preserve all unrelated sections in `production/signing/release`, including
  Caper/Captures/DBM shared Apple/Android material. DBM desktop signing stays on
  its existing GitHub path unless a separate, reviewed storage migration is wanted.
- [ ] Update storage helpers/workflows before introducing app-specific Android
  keys. Existing infrastructure `google-play --app ...` support scopes Play
  sections; it does not make `android` writes app-specific.
- [ ] Verify new consumers/signatures before revoking superseded app-specific
  credentials. Revoke nothing belonging to another application.

## 7. Apple: macOS, iPhone, and TestFlight

- [ ] Update macOS bundle/product/executable names, Info.plist, UI/menu copy,
  icon/export metadata, updater bundle paths, and preference migrations. Rebuild
  universal Apple Silicon/Intel output and sign/notarize/staple the app and DMG.
- [ ] Register `se.anyba.ios` and update iOS display/product name, XcodeGen
  prefix/targets/schemes/tests, bridging-header/source references, and
  `DBM_IOS_BUNDLE_ID` consumers. Preserve direct networking and the accurately
  worded local-network permission prompt under the AnyBase name.
- [ ] Inspect whether an iOS build was uploaded: after upload, App Store Connect
  cannot change that record's bundle ID, so use a new app record. Before any
  upload, confirm whether switching the existing record's bundle ID is possible.
  Recreate provisioning as needed; keep the same Apple team and valid credentials.
- [ ] Update App Store Connect name/new SKU, privacy/support/marketing URLs,
  screenshots/icons, review contact, TestFlight groups/public links, upload and
  `scripts/testflight-distribute.py` bundle lookups. No numeric app ID is committed.
- [ ] Verify the signed physical iPhone build/TestFlight assignment and clean
  macOS installs/updates. Simulator output is not physical-device acceptance.
  Do not add Apple Sign-In, push, Associated Domains, or app groups just to rename.

Owners: `apps/native/macos`, `apps/native/ios/{project.yml,prepare.sh,build.sh,upload-testflight.sh}`,
`apps/native/ios/Configuration/Info.plist`, `scripts/testflight-distribute.py`, and
`.github/workflows/{release,mobile-release}.yml`.

## 8. Android and Google Play

- [ ] Change `com.dbm.nativeapp` to `se.anyba.android` in applicationId,
  namespace, Kotlin/Java packages/paths, JNI symbols/class lookups, ProGuard rules,
  test/smoke selectors, and build outputs. Preserve `.debug` isolation, Required
  TLS, disabled backups, memory-only passwords, and read-only mobile behavior.
- [ ] Rename app label/icons, Gradle `DBMAndroid` project, native library /
  bridge names if chosen, and APK/AAB/artifact references together. Renaming a
  library requires matching CMake, `System.loadLibrary`, and packaging assertions.
- [ ] Create a fresh Play app for the changed application ID; it is a different
  app, not an update to `com.dbm.nativeapp`. Set AnyBase listing, privacy/support
  URLs, Data safety/content rating, icons/screenshots, and internal-test access.
- [ ] Configure Play App Signing and the selected upload key; distinguish the
  upload certificate from the certificate Google uses on delivered apps. Reuse
  the DBM-only bot with new app permissions or provision a new AnyBase-only bot.
  Google Cloud project/service-account IDs cannot be cosmetically renamed into
  new identities; creating a new account/project is optional, not required.
- [ ] Update `PLAY_PACKAGE`, `scripts/play-upload.py`, the app-specific Play
  secret section, tester links, `DBM_ANDROID_*` / build-number consumers, and
  release notifications. Verify signed APK installs and Play internal-track AAB
  delivery with the correct package/certificate/version code.
- [ ] Verify on a real Android device that direct TLS database access and
  background credential disposal still work. There is no Firebase/FCM setup,
  Android App Links, or persistent Android password keystore to migrate today.

Owners: `apps/native/android/{settings.gradle.kts,build.sh}`, app Gradle/manifest,
`app/src/main/java/com/dbm/nativeapp`, `app/src/main/cpp/dbm_jni.cpp`,
`scripts/play-upload.py`, and `.github/workflows/mobile-release.yml`.

## 9. Code names, branding, and documentation

- [ ] Update visible DBM names on all platforms, welcome/about/error screens,
  permission prompts, icons, screenshots, release notes and store/download copy.
  A logo redesign is optional; regenerate platform assets consistently if changed.
- [ ] Rename first-party Rust packages/modules, bridge C ABI symbols/headers,
  dylib/static-library/JNI references, build flags (`DBM_*`), demo fixtures, scripts,
  tests and lockfiles only as coordinated units. Retained internal names are
  acceptable during transition; avoid blind replacement of third-party names.
- [ ] Update README, AGENTS guidance, development/native/design/mobile/release
  docs and commands when implementation ships. Keep released desktop versus
  development-only mobile status and Windows signing limitations explicit.
- [ ] Search first-party source/config for old identifiers and record retained
  compatibility names rather than deleting legacy updater support indiscriminately.

## 10. Execution order and acceptance

**This planning PR requires no deployment or post-merge operator command.**
Markdown/docs changes are excluded from desktop auto-release. Implementation
PRs must specify any infrastructure init/plan/inspect/apply commands, expected
resource changes, release sequencing, and rollback steps for their actual diff.

1. Purchase the domain, confirm final names/IDs, inventory console settings,
   choose data/credential/update continuity and any fresh-install exceptions.
2. Prepare website/privacy/support URLs, new mobile app records and provisioning,
   selected app-specific credentials, and repo/dispatcher trust changes.
3. Coordinate the GitHub rename, re-read OIDC configuration, apply only reviewed
   signer changes if needed, and verify workflow/environment/dispatch access.
4. Implement/test code, packaging and local-data changes on a branch. Remember:
   merging app changes to `main` **automatically publishes desktop updates**.
   Have signing and legacy/new updater paths ready before that merge; a repo
   rename or passing PR checks does not publish mobile apps.
5. Verify the published desktop transition and website/download links. Release
   mobile separately from an exact `main` SHA with successful mobile tests;
   Android Play/APK and iPhone TestFlight do not need a hosted backend deployment.
6. Complete the gates below, retain old repo redirects/downloads and private
   backups through the chosen rollback period, then retire only approved
   app-specific records/keys. Keep shared signing resources untouched.

- [ ] Domain HTTPS/privacy/support links work; download links resolve to the
  intended AnyBase assets, and `mobile-latest` is still not desktop latest.
- [ ] Existing desktop profiles/history/UUIDs/TLS/read-only settings and OS
  passwords survive the chosen transition, or any deliberate reset is documented.
- [ ] Old DBM native/preview/Tauri update paths reach trusted working builds;
  subsequent AnyBase updates work, while tampered/wrong-key artifacts fail.
- [ ] macOS universal bundle/DMG passes signature, notarization and clean-launch
  checks; Windows installer and Linux AppImage/bare executable install/update /
  restart/uninstall correctly without deleting profile data.
- [ ] TestFlight and Play internal builds use new app IDs, correct signing
  identities and increasing versions; physical-device TLS database tests pass.
- [ ] Inspect representative rendered native welcome, connections, workbench,
  about/update and permission states for intended names/icons and no regressions.
- [ ] Local-only privacy, memory-only mobile passwords, TLS and read-only
  protections remain unchanged; no new profile/query-data network destinations.
- [ ] Record final URLs, identities, retained compatibility names, secret
  locations, implementation PRs, and actual verification results here.

Rollback: retain old local stores/credential entries and installer/download
assets until verification. Restore compatible releases without decreasing
updater build numbers. A newly registered mobile app ID is a separate app;
reinstall deliberately. A replaced updater trust key cannot be undone by
changing a secret label, and published app records may not be deletable/reusable.

## Inventory limits and provider constraints

Verified from DBM source, infrastructure source, and live DBM repository/OIDC
metadata. GitHub denied Actions secret/variable metadata listing, so actual
configured values/availability still require a console audit. No secret values,
live cloud/store inventory, DNS setup or availability checks were performed.
No end-user OAuth, passkeys, Firebase, push or deep-link integration was found;
audit separately if configured outside source, otherwise mark N/A.

- [Apple bundle-ID changes](https://developer.apple.com/documentation/xcode/changing-the-bundle-identifier):
  after an uploaded build, a new ID needs a new App Store Connect record.
- [Android application ID / namespace](https://developer.android.com/build/configure-app-module):
  a changed published application ID is a different app; same-ID updates need
  the existing signing identity.
- [GitHub repo renames](https://docs.github.com/en/repositories/creating-and-managing-repositories/renaming-a-repository):
  Git/web redirects exist, but hosted-action references do not redirect, and
  reusing the old repo name removes the redirect.
