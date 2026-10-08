# Anybase releases

Anybase distributes the native apps directly through GitHub Releases:

- macOS: the AppKit app as a DMG (Apple Silicon and Intel in one app);
- Windows: the egui app as an NSIS `.exe` installer; and
- Linux: the egui app as an AppImage.

Mobile distribution uses a separate, manually approved Caper-style workflow:
signed Android APK/AAB builds and iPhone TestFlight uploads. Its `mobile-latest`
prerelease cannot replace the desktop latest release or updater manifests.
See [mobile signing and first-release setup](mobile.md#signed-mobile-releases-capers-distribution-path).

Every merge to `main` that changes the app runs `.github/workflows/release.yml`,
which builds all three platforms and publishes the result as the latest GitHub
Release. Installed release builds find it through the updater manifest and
update in place. Changes that only touch Markdown, `docs/`, agent tooling, or
the CI workflow do not release. Run the workflow manually from the Actions tab
(**Release → Run workflow** on `main`) to release without a new merge.

These automatic releases are for the maintainer's own machines. Windows builds
are not Authenticode-signed yet, so the gates below still apply before Anybase is
promoted to a wider audience.

## How a release is built

1. **prepare** picks the next version and build number, pushes the tag, and
   creates a draft release whose notes list the pull requests merged since the
   previous release (Dependabot bumps are omitted).
2. **macOS** builds the universal app with `apps/native/macos/build.sh`
   (`DBM_UNIVERSAL=1`), signs it with the Developer ID and the hardened
   runtime, notarizes and staples it, and packages it three ways. The DMG is
   signed, notarized, and stapled too.
3. **workbench** builds the egui app on Windows and Linux. Linux is also
   packaged as an AppImage (`apps/native/workbench/appimage.sh`).
4. **windows-installer** wraps the Windows executable in an NSIS installer
   (`apps/native/windows/installer.nsi`).
5. Each build job signs its updater artifacts with the release updater key
   (`tools/dbm-sign`), writing a `.sig` beside each file. If any platform
   fails, the others stop.
6. **publish** writes both updater manifests and rejects one with a missing
   URL or signature, writes `SHA256SUMS`, attests build provenance for every
   asset, uploads the files (manifests last), and publishes the draft as the
   latest release.
7. **cleanup** deletes the draft and its tag if anything failed or was
   cancelled before publishing.

Only one release runs at a time; a merge that lands during a release waits
for it and then releases everything merged since.

### Release assets

| Asset | Use |
| --- | --- |
| `DBM-macOS.dmg` | First install on macOS (signed, notarized, stapled) |
| `DBM-macOS.zip` | Native updater on macOS |
| `DBM.app.tar.gz` | Updates for installs of the former Tauri app on macOS |
| `DBM-Windows-x64-setup.exe` | First install on Windows; also what former Tauri installs run to update |
| `DBM-Windows-x64.exe` | Native updater on Windows (bare executable) |
| `DBM-Linux-x86_64.AppImage` | Install on Linux, and updates for AppImage copies (native and former Tauri) |
| `DBM-Linux-x64` | Native updater for a bare Linux executable |
| `*.sig` | Updater signature for each updater artifact |
| `native-latest.json` | Native updater manifest |
| `latest.json` | Tauri-updater-format manifest, for installs of the former Tauri app |
| `SHA256SUMS` | Checksums of every asset |

The Windows installer needs no administrator rights: it installs to
`%LOCALAPPDATA%\DBM\dbm.exe`, adds an **Anybase** Start menu shortcut, and
registers an uninstaller. There is no `.deb` package.

Asset names, the Windows folder and executable, and the uninstall key keep
their DBM names from before the Anybase rename, so installed copies keep
updating (see [the rename plan](anybase-migration.md#never-change)). The
macOS archives hold `Anybase.app`, except `DBM.app.tar.gz`, which keeps the
`DBM.app` folder name the Tauri updater expects. Updates install over the
existing copy, so a macOS install made before the rename stays in
`DBM.app`, and a Windows one keeps its DBM Start menu entry until the
installer runs again; the app itself shows Anybase either way.

### Versions

Tags use the release date in New York time plus a daily revision:
`v2026.09.24.1`, `v2026.09.24.2`, and so on. `scripts/release.mjs` computes the
tag, the display version (`2026.09.24.2`), and an app version that packs the
date into SemVer as `YEAR.MONTH.DAY×100+REVISION` (`2026.9.2402`), so versions
always increase for the updaters. It is covered by
`node --test scripts/release.test.mjs`.

The build number spreads the app version into one integer, `YYYYMMDDNN`
(`2026092402`). It is compiled in as `DBM_NATIVE_BUILD` and, on macOS, written
to `CFBundleVersion`; the display version is compiled in as
`DBM_NATIVE_VERSION`. Development builds have neither.

## In-app updates

Release builds check
`https://github.com/joswayski/dbm/releases/latest/download/native-latest.json`
five seconds after launch and every 30 minutes. **Check for updates…** checks
on demand from the Anybase menu on macOS or the top bar's Help menu on
Windows/Linux. The top bar normally shows a small, non-clickable **Version
<version>** label; manual checks show non-clickable status text above it.
When the manifest lists a higher build number, an **Update to v<version>**
button appears above the installed version. Failed manual checks offer
**Retry update**, with the error in its tooltip. Installing asks for
confirmation, because unsaved query text and staged edits are discarded. It
then downloads the platform's artifact, verifies its minisign signature
against the public key in `crates/dbm-update`, swaps it in, and restarts.

- **macOS:** the new bundle must also pass `codesign --verify`, and is swapped
  in after Anybase quits. The app must sit in a folder the user can write to, such
  as `/Applications`.
- **Windows:** the running executable is renamed aside and removed on the next
  launch.
- **Linux:** the running executable is replaced; a copy running from an
  AppImage replaces the AppImage at `$APPIMAGE` instead, through the
  `linux-x86_64-appimage` manifest entry.

`native-latest.json` lists the build number, version text, notes, and each
platform's URL and signature. `darwin-aarch64` and `darwin-x86_64` both point
at the universal zip.

Development builds never check for updates. Debug builds can test the updater
against a local manifest by setting `DBM_UPDATE_MANIFEST` and
`DBM_UPDATE_PUBLIC_KEY`; release builds ignore both.

The update endpoint uses GitHub's latest release URL. Drafts are absent from
it, so installed apps only see a release once the publish job has validated
the manifests and made it the latest release.

## Migrating from the Tauri app

Anybase, as DBM, previously shipped as a Tauri app, while the native apps were published on
a separate `native-preview` channel. Installs from either move onto the native
releases without reinstalling:

- **Tauri installs** read `releases/latest/download/latest.json`. The release
  workflow still writes that manifest in the Tauri updater's format, pointing
  at `DBM.app.tar.gz` (macOS), `DBM-Windows-x64-setup.exe` (Windows), and the
  AppImage (Linux), signed with the same updater key. Its version is always
  newer than the last Tauri release (`2026.9.2801`). The Windows installer
  installs into the Tauri app's folder with the same executable name and
  uninstall key, and honors the Tauri updater's `/P` (silent) and `/R`
  (reopen) arguments. Tauri `.deb` installs are not updated automatically;
  switch them to the AppImage by hand.
- **Native preview installs** read the `native-preview` release's manifest.
  Each release run uploads its `native-latest.json` there too, so those copies
  update to a release build, which reads `releases/latest` from then on.

Both apps use `dbm-core`'s data directory and the keychain service
`io.github.joswayski.dbm`, so saved connections, query history, and passwords
carry over.

## Public-release gates

| Platform or concern | Required before publishing | Current state |
| --- | --- | --- |
| macOS | Sign with a Developer ID Application certificate, notarize with Apple, staple the notarization ticket, and validate the DMG on a clean Mac | CI signs and notarizes the app, notarizes and staples the DMG, and verifies both; clean-Mac validation remains manual |
| Windows | Authenticode-sign and RFC 3161-timestamp both `dbm.exe` and the NSIS installer with a publicly trusted code-signing identity | Not configured in CI; SmartScreen may warn on first run |
| Linux | Publish the AppImage with `SHA256SUMS` and GitHub build-provenance attestations | CI publishes `SHA256SUMS` and attests every asset |
| All platforms | Tie every artifact to the tagged commit, reject an incomplete draft, and test installation on clean supported systems | CI builds from the merged commit, validates both updater manifests, and deletes incomplete drafts; clean-machine testing remains manual |

Updater signatures, Apple signatures, Windows Authenticode signatures, and
GitHub attestations solve different problems. One does not replace another:

- Apple and Authenticode signatures establish the operating-system publisher.
- An updater signature lets an installed application authenticate an update.
- A GitHub attestation establishes which repository, commit, and workflow built
  a downloaded artifact.
- `SHA256SUMS` detects accidental or malicious file changes after publication.

## GitHub release environment

Create a GitHub environment named `release`. Release builds run from the `main`
branch, so the environment's deployment branch rule must allow `main` (a rule
limited to `v*` tags blocks every release). The build jobs reference this
environment to access signing credentials.

Keep private keys and passwords in environment secrets. Store non-secret Azure
resource identifiers as environment variables. Do not commit credentials,
exported certificates, or temporary signing files.

## Updater signing

Anybase uses a dedicated updater keypair (a minisign key, the format the Tauri
updater used); it does not reuse Captures' key. The public key is committed in
`crates/dbm-update/src/lib.rs`. It is the key the Tauri app shipped with, so
Tauri installs accept these updates too. The secrets keep their Tauri-era
names; the workflow passes them to
`tools/dbm-sign` as `DBM_UPDATE_SIGNING_KEY` and
`DBM_UPDATE_SIGNING_KEY_PASSWORD`.

| Secret | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Complete contents of Anybase's dedicated updater private key |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password protecting that private key |

Back up the private key and its password separately in encrypted storage.
Losing the private key prevents every installed Anybase release from authenticating
future updates. Do not replace the public key after shipping unless an existing
trusted release first implements a deliberate key rotation.

## macOS signing and notarization

Direct distribution outside the Mac App Store requires a Developer ID
Application signature and Apple notarization. A Developer ID Installer
certificate is not needed for the DMG; it is used for signed `.pkg` installers.
The same Developer ID Application identity can sign Anybase and Captures, although
each repository must independently protect its release environment and validate
its output.

### Account setup

1. Create a certificate signing request in Keychain Access.
2. Create a **Developer ID Application** certificate in the Apple Developer
   portal, download it, and install it in the login keychain.
3. Confirm that the identity and its private key appear under **My
   Certificates**, then export them as a password-protected `.p12`.
4. Create an App Store Connect **Team API key** with Developer access for
   notarization. Save the issuer ID, key ID, and downloaded `.p8`; Apple only
   allows the private key to be downloaded once.
5. Back up the `.p12`, `.p8`, and their recovery information in encrypted
   offline storage.

### Environment secrets

| Secret | Value |
| --- | --- |
| `APPLE_CERTIFICATE` | Base64-encoded Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | Export password for the `.p12` |
| `KEYCHAIN_PASSWORD` | Random password used only for CI's temporary keychain |
| `APPLE_API_ISSUER` | App Store Connect team API issuer ID |
| `APPLE_API_KEY` | App Store Connect team API key ID |
| `APPLE_API_PRIVATE_KEY` | Complete contents of the downloaded `.p8` |

The workflow decodes the certificate and API key only into the runner's
temporary directory, imports the certificate into a temporary keychain,
selects the `Developer ID Application` identity, and passes it to `build.sh`
as `DBM_SIGNING_IDENTITY`. It then notarizes and staples the app, builds the
DMG, and signs, notarizes, and staples that too. The job fails when any
credential or validation is missing.

To check a downloaded release by hand:

```sh
codesign --verify --deep --strict --verbose=2 Anybase.app
spctl --assess --type execute --verbose=2 Anybase.app
xcrun stapler validate Anybase.app
codesign --verify --strict --verbose=2 DBM-macOS.dmg
spctl --assess --type open --context context:primary-signature --verbose=2 DBM-macOS.dmg
xcrun stapler validate DBM-macOS.dmg
```

Also install the DMG on a clean supported Mac and launch the installed copy
without using a Gatekeeper bypass.

## Windows Authenticode signing

Use a publicly trusted code-signing service before publishing Windows
downloads. The preferred CI route is **Microsoft Artifact Signing Public
Trust** because its private signing keys stay in Microsoft's managed service
instead of being exported into GitHub.

One Artifact Signing account, validated identity, and Public Trust certificate
profile can serve both Anybase and Captures.

### Account setup

1. Create an Azure subscription and Microsoft Entra tenant, then make sure the
   legal name and address on the Azure billing profile are correct.
2. Register the `Microsoft.CodeSigning` resource provider.
3. Create an Artifact Signing account.
4. Complete **Individual Public Trust** identity validation. Microsoft notes
   that validation can take from 1 to 20 business days, so start it before a
   planned public release.
5. Create a **Public Trust** certificate profile. Do not use a Public Trust
   Test or Private Trust profile for public downloads.
6. Create a Microsoft Entra application or workload identity for GitHub
   Actions and grant it the **Artifact Signing Certificate Profile Signer**
   role scoped to the certificate profile.
7. Add a GitHub OIDC federated credential restricted to:

   ```text
   repo:joswayski/dbm:environment:release
   ```

   OIDC avoids storing a long-lived Azure client secret in GitHub.

### Environment variables

| Variable | Value |
| --- | --- |
| `AZURE_CLIENT_ID` | Entra application or workload identity client ID |
| `AZURE_TENANT_ID` | Entra tenant ID |
| `AZURE_SUBSCRIPTION_ID` | Azure subscription containing Artifact Signing |
| `AZURE_ARTIFACT_SIGNING_ENDPOINT` | Regional Artifact Signing endpoint |
| `AZURE_ARTIFACT_SIGNING_ACCOUNT` | Artifact Signing account name |
| `AZURE_ARTIFACT_SIGNING_PROFILE` | Public Trust certificate profile name |

The Windows jobs must request `id-token: write`, authenticate to Azure with
OIDC, and sign both the executable (before it is packaged into the installer
and before its updater signature is written) and the final NSIS installer.
Sign with SHA-256 and use the Microsoft RFC 3161 timestamp service.
Timestamping is a release requirement because Artifact Signing certificates are
intentionally short-lived.

Validate both files before upload:

```powershell
Get-AuthenticodeSignature .\DBM-Windows-x64.exe |
  Format-List Status, StatusMessage, SignerCertificate, TimeStamperCertificate

Get-AuthenticodeSignature .\DBM-Windows-x64-setup.exe |
  Format-List Status, StatusMessage, SignerCertificate, TimeStamperCertificate
```

Both results must report `Valid`, include the expected publisher, and include a
timestamp. Test the installer on a clean Windows 11 system.

If Microsoft Artifact Signing is unavailable, use a publicly trusted
OV/EV code-signing certificate from a certificate authority. Follow that
provider's current hardware-token or cloud-HSM instructions; do not assume an
exportable `.pfx` is permitted.

## Linux publication integrity

Linux has no single platform-wide publisher certificate comparable to Apple
Developer ID or Windows Authenticode. For Anybase's direct GitHub Release downloads,
the publication gate is verifiable integrity and provenance. The release
workflow builds every artifact from the tagged commit, writes `SHA256SUMS`
over the final assets, and attests build provenance for all of them with
`actions/attest-build-provenance` (the publish job grants `id-token: write`
and `attestations: write`).

Verify a download on a clean Ubuntu system:

```sh
sha256sum --check --ignore-missing SHA256SUMS
gh attestation verify ./DBM-Linux-x86_64.AppImage --repo joswayski/dbm
chmod +x ./DBM-Linux-x86_64.AppImage
./DBM-Linux-x86_64.AppImage
```

An embedded GPG signature may also be added to the AppImage, but AppImage does
not automatically verify it. Do not use an embedded AppImage signature as a
replacement for checksums and build provenance.

## Promoting to a public release

Automatic releases skip the checks below. Before recommending Anybase to others:

1. Configure Windows Authenticode signing in the build jobs.
2. Perform the clean-machine installation checks for macOS, Windows, and
   Ubuntu against a published release.
3. Verify `SHA256SUMS` and `gh attestation verify` for each downloaded
   artifact.

## References

- [Apple: Developer ID certificates](https://developer.apple.com/help/account/certificates/create-developer-id-certificates)
- [Apple: notarizing macOS software](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)
- [Microsoft: set up Artifact Signing](https://learn.microsoft.com/azure/artifact-signing/quickstart)
- [Microsoft: Artifact Signing integrations](https://learn.microsoft.com/azure/artifact-signing/how-to-signing-integrations)
- [Azure: Artifact Signing GitHub Action](https://github.com/Azure/artifact-signing-action)
- [GitHub: artifact attestations](https://docs.github.com/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations)
- [Minisign](https://jedisct1.github.io/minisign/)
