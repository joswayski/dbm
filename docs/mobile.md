# Mobile development clients

DBM has browser-free **Kotlin/Compose Android** and **SwiftUI iPhone** development
clients. They reuse the desktop Rust database adapters in-process through JNI
and the C ABI. These are not Play Store/App Store releases or desktop-parity apps.

The setup follows Caper's separate native UI/build approach, not its hosted API.
DBM has no server, account, pairing service, telemetry, or connection sync.

## First slice

- PostgreSQL, MySQL and Redis connection forms, test/save/delete/connect.
- Direct, verified TLS connections, with read-only profiles required by the
  mobile Rust bridge. Use a database user/Redis ACL that actually forbids writes:
  the app's read-only mode is a safeguard, not an authorization boundary.
- Connect opens the SQL/Redis workbench. Query results are capped at 1,000 rows;
  schema/key exploration and read-only table/key pages use 25 rows per page.
- Database selection, refresh, loading/errors, and explicit local demo fixtures.
- The desktop Graphite workbench layout and bundled Geist/Geist Mono fonts:
  connection identity, colored tab edges, compact controls and dense grids.
  The source list opens as a drawer/sheet on portrait phones and stays visible
  at widths of 700 dp/pt or more. Orientation changes keep the workbench open;
  leaving the app still clears it.
- One query tab and one retained table/key tab per active connection. Switching
  tabs preserves SQL/command text, query results and the table's current page;
  changing databases clears both result sets. Table tabs contain no SQL editor.
- Run and Refresh share the toolbar above the query editor. Refresh reruns the
  last successful statement without replacing the editor draft; it stays
  disabled until a query succeeds. Grid headers stay visible while rows scroll
  vertically and move with their columns when scrolling horizontally.
- Capped query results explicitly report truncation. Refreshing the schema
  keeps expanded folders open. Connection fields and SQL/command editors
  disable autocorrection; Android also tells the keyboard the password field
  is a password, rather than only masking its display.

No row editing, exports, desktop history browser, URL import, custom CA import,
SSH tunnel, biometric unlock, profile sync or background queries are implemented.
Native CI and device acceptance are required before relying on these clients.

## Access away from home

The phone connects **directly to the database**, not through a DBM backend.
Use a routable database hostname and port, valid TLS certificates, and a dedicated
least-privilege database user. Do not open database ports to the public Internet
just to use this app.

For private endpoints, install/configure your VPN (for example Tailscale) on the
phone and verify its subnet routing and DNS first. DBM does not manage the VPN.
`localhost` on a phone is the phone itself, not your laptop. TLS hostname checking
still applies over a VPN. Existing laptop access does not prove phone access.

Android exports its platform `TrustManagerFactory` accepted issuers to a private
PEM bundle. PostgreSQL/MySQL use it with vendored OpenSSL; Redis on Android uses
rustls because redis-rs's native-tls connector cannot accept custom root bundles.
iPhone uses Apple's native certificate trust. No certificate/hostname-validation
bypass or plaintext fallback is available in mobile profiles.

## Storage and lifecycle

Each install has its own sandbox SQLite file containing connection metadata and
SQL/command history, **never database passwords**. History itself may contain
sensitive SQL. Passwords stay in Rust session memory; re-enter them when
connecting after disconnect, backgrounding or relaunch. This first slice does
not persist credentials in Keychain/Keystore. Desktop OS-keyring behavior is unchanged.

Backgrounding clears the UI and queues session disposal on the same worker as
database calls. Old completions cannot restore the previous workspace. Disposal
waits for an active query to return; it does **not** cancel work on the server,
and the OS may suspend the process. Query cancellation remains follow-up work.
Android blocks normal app screenshots; iPhone covers inactive content and
discards form state on background. Physical app-switcher/lock-screen behavior
still needs testing. Android backup is disabled; iPhone's protected DB directory
is excluded from iCloud backup. Neither client syncs with desktop.

## Build and test

Android needs JDK 17, SDK/build tools 36, NDK `27.2.12479018`, CMake 3.22.1,
Perl/make/unzip, Rust 1.94 and cargo-ndk:

```sh
cargo install cargo-ndk --version 4.1.2 --locked
rustup target add aarch64-linux-android x86_64-linux-android
bash apps/native/android/build.sh
# Connected device or emulator on a machine with virtualization:
apps/native/android/gradlew -p apps/native/android connectedDebugAndroidTest
```

Set `ANDROID_HOME` to your SDK and `ANDROID_NDK_HOME` to its
`ndk/27.2.12479018` directory. The canonical script builds Rust before starting
Gradle to avoid concurrent compiler/JVM memory peaks. Re-run it after core changes.
It tests/lints/builds the debug app and instrumentation APK. The normal APK is
`apps/native/android/app/build/outputs/apk/debug/app-debug.apk` (arm64/x86_64).
Android Studio/standalone Gradle requires the script's prebuilt Rust archives.

iPhone needs an Apple Silicon Mac with Xcode's iOS SDK and Rust 1.94:

```sh
bash apps/native/ios/build.sh simulator
bash apps/native/ios/build.sh test
```

The simulator app is `apps/native/ios/DerivedData/Build/Products/Debug-iphonesimulator/DBM.app`.
**It cannot be installed on a physical iPhone.** A local `device` build requires
your own bundle ID, Apple development team and provisioning; see the
[iPhone guide](../apps/native/ios/README.md). Development builds do not read
signing secrets or upload to stores. The separate release workflow below uses
the existing AWS signing material, as Caper does.

`.github/workflows/mobile.yml` builds Android and iPhone Simulator development
artifacts, records exact tested revisions/checksums, and runs native demo UI
tests with retained captures. PR jobs have no signing credentials. A successful
package step is not proof that the later UI tests pass. Downloads expire after
14 days. Simulator/emulator fixtures do not validate real databases or phones.
DBM's existing desktop release workflow remains unchanged; merging app changes
still publishes desktop updates, not mobile store releases.

These are configured workflows, not a record that any particular revision has
passed. SwiftUI is not compiled on Linux. Physical-device behavior and live,
verified-TLS database connections on both mobile platforms remain outstanding.

## Signed mobile releases (Caper's distribution path)

`.github/workflows/mobile-release.yml` follows Caper's release approval and
signing setup. It does not deploy a DBM server or change desktop distribution.
After **Mobile development builds** passes for a push to `main`,
`mobile-ready.yml` posts **Deploy DBM mobile** in the existing Discord deploys
channel. Godis dispatches the exact SHA; the release rejects commits outside
`main` or without a successful mobile test run for that SHA. Merging alone does
not upload or publish mobile apps.

The release reads `production/signing/release` through GitHub OIDC role
`production-dbm-release-signer`; no signing files belong in this repository:

- **Android:** the existing `android` upload key signs `com.dbm.nativeapp`.
  APK updates retain profiles/history. The version code is `run number × 100 +
  attempt` (attempts 1–99); even re-runs increase it. Release builds disable demo
  mode and refuse packaging without all four `DBM_ANDROID_KEYSTORE`,
  `DBM_ANDROID_KEYSTORE_PASSWORD`, `DBM_ANDROID_KEY_ALIAS`, and
  `DBM_ANDROID_KEY_PASSWORD` inputs. Build locally with these variables and
  `DBM_BUILD_NUMBER`, then `bash apps/native/android/build.sh assembleRelease bundleRelease`.
  Debug `.debug` installs are separate, and CI debug keys are not stable.
- **Google Play:** every approved Android release uploads the signed AAB to
  `com.dbm.nativeapp` on the **internal testing** track, using DBM's own service
  account in `dbm_google_play.service_account_json`. It never falls back to
  Caper's `google_play` key. No repository opt-in variable is needed.
  Missing credentials or denied access fail the Android job rather than silently
  skipping Play. Testers install and update through the Play Store after joining
  https://play.google.com/apps/testing/com.dbm.nativeapp with an enrolled account.
  If Google accepts only a draft, the job warns; finish its rollout in Play
  Console before expecting an install or update.
- **Downloads:** the signed `DBM-Android.apk`, revision/version `BUILD.json`
  and `SHA256SUMS` go in the `mobile-latest` **prerelease**, never GitHub's
  desktop `releases/latest` target. This is an alternative to Play testing.
  Once published, the APK is at
  https://github.com/joswayski/dbm/releases/download/mobile-latest/DBM-Android.apk.
  The signed Play AAB is retained as the run's `google-play-bundle` artifact for
  30 days, including when the subsequent Play upload fails during first-app setup.
- **iPhone:** `apps/native/ios/upload-testflight.sh` builds a Release device
  archive for `app.dbm.ios` with Xcode 26 and build number `<run number>.<attempt>`.
  The existing `apple` App Store Connect API key handles cloud-managed signing,
  upload, processing, What to Test, and tester-group assignment. External groups
  may still await Apple's beta review. Android publication does not wait for
  Apple; Discord reports failure if either platform fails, including partial
  uploads. A TestFlight success is not physical-device acceptance.

### One-time setup before the first release

1. Apply the DBM signer role in `joswayski/infrastructure` following its
   `docs/release-signing.md`. Reuse the stored Apple/Android signing material;
   do not regenerate or rotate Caper's keys. This creates no hosted DBM services.
2. Add `joswayski/dbm` to the existing Godis deploy GitHub App installation
   with **Actions: write** only, deploy Godis's `dbm-mobile` dispatch support,
   and sync the existing notification secret from the infrastructure checkout:
   `./scripts/store-deploy-notification-webhook.sh --profile production`.
3. Register Apple App ID `app.dbm.ios` and its App Store Connect app. The shared
   API key needs **Admin** access for cloud-managed distribution signing. Add
   yourself to a TestFlight internal group; a public link is not required.
   External testing additionally needs beta contact/review information.
4. Create DBM in the same Play Console developer account as Caper. The first
   bundle establishes package `com.dbm.nativeapp`; the display name can change,
   but the package cannot. Create a separate `dbm-play-release` service account
   in Google Cloud Console, in a project with **Google Play Android Developer API**
   enabled. Skip the optional Google Cloud IAM role grants. Under the service
   account's **Keys → Add key → Create new key → JSON**, download its key and
   save it as `~/Downloads/dbm-play-release.json`; never commit or share it.
   In Play Console **Users and permissions → Invite new users**, enter this
   new service account's email and select **only DBM** under **App permissions**.
   Grant **View app information (read-only)** and **Release apps to testing tracks**;
   leave account-wide permissions unset and do not grant Caper access.
   Add your own Google account under DBM's **Testing → Internal testing → Testers**.
   After updating the infrastructure checkout to the app-scoped storage script:

   ```sh
   aws sso login --profile production
   ./scripts/store-release-signing-secrets.sh google-play --app dbm \
     --service-account ~/Downloads/dbm-play-release.json --profile production
   aws --profile production --region us-east-1 secretsmanager get-secret-value \
     --secret-id production/signing/release --query SecretString --output text \
     --no-cli-pager | jq -er '.dbm_google_play.client_email'
   ```

   Verify the printed email matches the new DBM bot. This writes only
   `dbm_google_play`; Caper's key is unchanged. The separate Play credentials
   remain in the existing shared AWS secret; this is not per-app AWS secret
   access isolation. No additional Terraform resources or apply are needed.
5. Merge the app workflow, wait for its exact `main` mobile tests, then use
   **Deploy DBM mobile**, or dispatch the tested SHA explicitly:

   ```sh
   git fetch origin main
   gh workflow run mobile-release.yml --repo joswayski/dbm --ref main \
     -f git_sha="$(git rev-parse origin/main)"
   gh run list --repo joswayski/dbm --workflow mobile-release.yml --limit 5
   ```

6. For a new Play app, download that run's **google-play-bundle** artifact and
   upload `DBM-Android-Play.aab` under **Internal testing → Create new release**.
   Complete Play App Signing and roll out to internal testers, not production.
   Google requires this first Console upload before its publishing API can
   update an app, so the first workflow's Play step may fail until this is done.
   Rerun the workflow afterwards; the higher version code avoids reusing the
   manually uploaded build. Later approved releases update Play automatically.

Check both platform jobs, join the Play testing link and install DBM from the
Play Store on Android, and install DBM from TestFlight on the actual iPhone.
For a regression, revert the app change and release a new, higher build number;
do not rotate keys or downgrade installed versions.
App registration, infrastructure apply, secret sync, and the first upload are
operator actions, not steps performed by development CI.

## Before distributing to testers

Require exact-head native compilation, inspect connection/query/table/error
captures, then exercise physical Android and iPhone on disposable databases:
TLS/hostname rejection, private DNS/VPN and cellular switching, read-only
accounts, database switching, pagination, foreground/background during a query,
reconnect and password re-entry, Unicode/large values, screen readers and keyboard.
Build/lint or a demo result alone does not establish mobile readiness.
