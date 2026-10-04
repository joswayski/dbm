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
[iPhone guide](../apps/native/ios/README.md). TestFlight signing/upload and Play
distribution require separate setup and authorization. Existing Caper signing
secrets are not read or copied by DBM builds.

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

## Personal installs before store distribution

Unlike Caper, DBM needs no hosted API, gateway, server database, or infrastructure
rollout. The maintainer's personal use is the priority; the name, website, and
mobile distribution are still undecided. See the
[next decisions](releases.md#personal-use-direction-and-next-decisions) before
starting signing automation or store setup. Phones are optional, not a required
next phase of the desktop tool.

If personal phone use is wanted:

1. **Android:** install a locally built development APK. No Play account or
   public listing is needed. Keep the same application ID and signing key for
   updates: CI debug keys are not stable across runners, so another CI debug
   APK may require uninstalling the previous one and losing local profiles and
   history. Debug and release app IDs are separate installs. A durable private
   release would need explicit signing/versioning setup; release assembly
   currently stops with an error until signing is configured.
2. **iPhone:** use the [device build](../apps/native/ios/README.md) with a valid
   Apple development team, bundle ID, and device provisioning, then install
   through Xcode. Personal development installs still depend on Apple's
   [device signing/provisioning rules](https://developer.apple.com/documentation/xcode/running-your-app-on-simulated-or-physical-devices)
   and may need re-provisioning. TestFlight and an App Store Connect app record
   are not required for this local development route. Neither a Simulator app
   nor a compiled device Rust library is a signed, installable iPhone app by itself.
3. **Acceptance:** exercise the maintainer's actual phones against disposable
   databases through the existing private network/VPN using restricted users
   and verified TLS. The checks below apply before relying on a personal
   install too; fixtures do not validate real phone/database access.

## Optional store or tester distribution

Only after choosing durable application IDs and an installation route:

- Repeatable private Android APK delivery needs a stable signing key and
  increasing version codes. Play additionally needs its own app record and
  signed AAB; it is not necessary just to install an APK personally.
- TestFlight is an optional iPhone delivery channel. It needs DBM bundle
  registration, an App Store Connect app record, signed archive/upload
  automation, and increasing build numbers. It does not commit the project to
  a public App Store release.

Caper's release automation is a reference if this work is chosen later, not
something DBM inherits. Account/app registration, signing-secret access or
configuration, and uploads need separate authorization. No Caper credentials
are copied, and no store pipeline is currently configured.

## Before distributing to testers

Require exact-head native compilation, inspect connection/query/table/error
captures, then exercise physical Android and iPhone on disposable databases:
TLS/hostname rejection, private DNS/VPN and cellular switching, read-only
accounts, database switching, pagination, foreground/background during a query,
reconnect and password re-entry, Unicode/large values, screen readers and keyboard.
Build/lint or a demo result alone does not establish mobile readiness.
