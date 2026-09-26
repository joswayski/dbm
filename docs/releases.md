# DBM releases

DBM distributes installers directly through GitHub Releases:

- a DMG for macOS;
- an NSIS `.exe` installer for Windows; and
- `.deb` and AppImage packages for Linux.

Every merge to `main` that changes the app runs `.github/workflows/release.yml`,
which builds all three platforms and publishes the result as the latest GitHub
Release. Installed official builds find it through the updater manifest and
update in place. Changes that only touch Markdown, `docs/`, or agent tooling do
not release. Run the workflow manually from the Actions tab (**Release → Run
workflow** on `main`) to release without a new merge.

These automatic releases are for the maintainer's own machines. Windows
installers are not Authenticode-signed yet, so the gates below still apply
before DBM is promoted to a wider audience.

## How a release is built

1. **prepare** picks the next version, pushes its tag, and creates a draft
   release whose notes list the pull requests merged since the previous
   release (Dependabot bumps are omitted).
2. **build** packages macOS (signed and notarized DMG), Windows (NSIS), and
   Linux (`.deb` and AppImage) into the draft with Tauri updater signatures
   and a merged `latest.json`. If any platform fails, the others stop.
3. **publish** rewrites `latest.json` so every download URL points at the
   public `releases/download/<tag>/` path, rejects a manifest whose version,
   notes, signatures, or assets do not match, writes `SHA256SUMS`, attests
   build provenance for every asset, and then publishes the draft as the
   latest release.
4. **cleanup** deletes the draft and its tag if anything failed or was
   cancelled before publishing.

Only one release runs at a time; a merge that lands during a release waits
for it and then releases everything merged since.

### Versions

Tags use the release date in New York time plus a daily revision:
`v2026.09.24.1`, `v2026.09.24.2`, and so on. The packaged app version packs
that into SemVer as `YEAR.MONTH.DAY×100+REVISION` (`2026.9.2401`), so versions
always increase for the updater. `scripts/release.mjs` computes both and is
covered by `npm run test:release`, which `npm run check` includes.

### In-app updates

Official builds (compiled with `DBM_OFFICIAL_RELEASE=1`) check
`https://github.com/joswayski/dbm/releases/latest/download/latest.json` 15
seconds after launch and every 4 hours, and the top bar's **Check for
updates** button checks on demand. When a newer release exists the button
becomes **Update to …**; installing downloads the signed update and restarts.
The macOS app, Windows installer, and AppImage update in place; `.deb` installs
open the release page to download the new package. Local and development
builds never check for updates.

## Public-release gates

| Platform or concern | Required before publishing | Current state |
| --- | --- | --- |
| macOS | Sign with a Developer ID Application certificate, notarize with Apple, staple the notarization ticket, and validate the DMG on a clean Mac | CI signs and notarizes the app, notarizes and staples the DMG, and verifies both; clean-Mac validation remains manual |
| Windows | Authenticode-sign and RFC 3161-timestamp both `dbm.exe` and the NSIS installer with a publicly trusted code-signing identity | Not configured in CI |
| Linux | Publish the `.deb` and AppImage with `SHA256SUMS` and GitHub build-provenance attestations | CI publishes `SHA256SUMS` and attests every asset |
| All platforms | Tie every artifact to the tagged commit, reject an incomplete draft, and test installation on clean supported systems | CI builds from the merged commit, validates the updater manifest for all three platforms, and deletes incomplete drafts; clean-machine testing remains manual |

Tauri updater signatures, Apple signatures, Windows Authenticode signatures,
and GitHub attestations solve different problems. One does not replace another:

- Apple and Authenticode signatures establish the operating-system publisher.
- An updater signature lets an installed application authenticate an update.
- A GitHub attestation establishes which repository, commit, and workflow built
  a downloaded artifact.
- `SHA256SUMS` detects accidental or malicious file changes after publication.

## GitHub release environment

Create a GitHub environment named `release`. Release builds run from the `main`
branch, so the environment's deployment branch rule must allow `main` (a rule
limited to `v*` tags blocks every release). The build job references this
environment to access signing credentials.

Keep private keys and passwords in environment secrets. Store non-secret Azure
resource identifiers as environment variables. Do not commit credentials,
exported certificates, or temporary signing files.

## Updater signing

DBM uses a dedicated Tauri updater keypair; it does not reuse Captures' key.
The updater public key is committed in `tauri.conf.json`. Add these private
values to the `release` environment:

| Secret | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Complete contents of DBM's dedicated updater private key |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password protecting that private key |

Back up the private key and its password separately in encrypted storage.
Losing the private key prevents every installed DBM release from authenticating
future updates. Do not replace the public key after shipping unless an existing
trusted release first implements a deliberate key rotation.

The workflow injects the release version into the packaged app, creates and signs updater artifacts for macOS, Windows,
and AppImage, and generates `latest.json`. A final job rejects a manifest that
does not contain matching signed entries for all three platforms. Local builds
use `tauri.local.conf.json` and omit updater artifacts unless an updater private
key is explicitly supplied.

The update endpoint uses GitHub's latest release URL. Drafts are absent from
it, so installed apps only see a release once the publish job has validated
the manifest and made it the latest release. AppImage installations can update
in place; `.deb` installations open the matching GitHub Release for a manual
package update.

## macOS signing and notarization

Direct distribution outside the Mac App Store requires a Developer ID
Application signature and Apple notarization. A Developer ID Installer
certificate is not needed for the DMG; it is used for signed `.pkg` installers.
The same Developer ID Application identity can sign DBM and Captures, although
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
selects the `Developer ID Application` identity, and lets Tauri sign, notarize,
and staple the app. It then separately notarizes and staples the final DMG,
replaces the initially uploaded DMG with that final artifact, and fails when
any credential or validation is missing.

Before publishing, verify the signature, Gatekeeper assessment, and stapled
ticket:

```sh
codesign --verify --deep --strict --verbose=2 DBM.app
spctl --assess --type execute --verbose=2 DBM.app
xcrun stapler validate DBM.app
codesign --verify --strict --verbose=2 DBM.dmg
spctl --assess --type open --context context:primary-signature --verbose=2 DBM.dmg
xcrun stapler validate DBM.dmg
```

Also install the DMG on a clean supported Mac and launch the installed copy
without using a Gatekeeper bypass.

## Windows Authenticode signing

Use a publicly trusted code-signing service before publishing Windows
downloads. The preferred CI route is **Microsoft Artifact Signing Public
Trust** because its private signing keys stay in Microsoft's managed service
instead of being exported into GitHub.

One Artifact Signing account, validated identity, and Public Trust certificate
profile can serve both DBM and Captures.

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

The Windows release job must request `id-token: write`, authenticate to Azure
with OIDC, and integrate Artifact Signing with Tauri so that both the
application executable and the final NSIS installer are signed. Sign with
SHA-256 and use the Microsoft RFC 3161 timestamp service. Timestamping is a
release requirement because Artifact Signing certificates are intentionally
short-lived.

Validate both files before upload:

```powershell
Get-AuthenticodeSignature .\dbm.exe |
  Format-List Status, StatusMessage, SignerCertificate, TimeStamperCertificate

Get-AuthenticodeSignature .\DBM_*_x64-setup.exe |
  Format-List Status, StatusMessage, SignerCertificate, TimeStamperCertificate
```

Both results must report `Valid`, include the expected publisher, and include a
timestamp. Test the installer on a clean Windows 11 system and confirm the UAC
dialog displays the expected verified publisher.

If Microsoft Artifact Signing is unavailable, use a publicly trusted
OV/EV code-signing certificate from a certificate authority. Follow that
provider's current hardware-token or cloud-HSM instructions; do not assume an
exportable `.pfx` is permitted.

## Linux publication integrity

Linux has no single platform-wide publisher certificate comparable to Apple
Developer ID or Windows Authenticode. For DBM's direct GitHub Release downloads,
the publication gate is verifiable integrity and provenance. GitHub
attestations apply to every platform, so generate them for the macOS and
Windows artifacts as well:

1. Build every release artifact only in the release workflow for the tagged
   commit.
2. Generate `SHA256SUMS` over the final artifacts selected for upload.
3. Generate a GitHub artifact attestation for the DMG, NSIS installer, `.deb`,
   AppImage, and checksum manifest.
4. Upload the packages and checksum manifest, then confirm GitHub can retrieve
   and verify each artifact's attestation before making the draft public.
5. Verify the packages from a clean Ubuntu system:

   ```sh
   sha256sum --check SHA256SUMS
   gh attestation verify ./DBM_VERSION_amd64.deb --repo joswayski/dbm
   gh attestation verify ./DBM_VERSION_amd64.AppImage --repo joswayski/dbm
   sudo apt install ./DBM_VERSION_amd64.deb
   chmod +x ./DBM_VERSION_amd64.AppImage
   ./DBM_VERSION_amd64.AppImage
   ```

The attestation job must grant `id-token: write` and `attestations: write` and
use GitHub's official attestation action (`actions/attest-build-provenance`).

An embedded GPG signature may also be added to the AppImage, but AppImage does
not automatically verify it. Do not use an embedded AppImage signature as a
replacement for checksums and build provenance.

If DBM later operates an APT repository, that repository must publish signed
`InRelease` metadata or `Release` plus `Release.gpg`. Distribute the repository
public key through an authenticated channel and configure users with a
repository-specific keyring and `signed-by=`. Signing a standalone `.deb` is
not a substitute for signing APT repository metadata.

## Promoting to a public release

Automatic releases skip the checks below. Before recommending DBM to others:

1. Configure Windows Authenticode signing in the build job.
2. Perform the clean-machine installation checks for macOS, Windows, and
   Ubuntu against a published release.
3. Verify `SHA256SUMS` and `gh attestation verify` for each downloaded
   artifact.

## References

- [Apple: Developer ID certificates](https://developer.apple.com/help/account/certificates/create-developer-id-certificates)
- [Apple: notarizing macOS software](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)
- [Tauri: macOS code signing](https://v2.tauri.app/distribute/sign/macos/)
- [Tauri: updater](https://v2.tauri.app/plugin/updater/)
- [Microsoft: set up Artifact Signing](https://learn.microsoft.com/azure/artifact-signing/quickstart)
- [Microsoft: Artifact Signing integrations](https://learn.microsoft.com/azure/artifact-signing/how-to-signing-integrations)
- [Azure: Artifact Signing GitHub Action](https://github.com/Azure/artifact-signing-action)
- [Tauri: Windows code signing](https://v2.tauri.app/distribute/sign/windows/)
- [GitHub: artifact attestations](https://docs.github.com/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations)
- [Tauri: Linux code signing](https://v2.tauri.app/distribute/sign/linux/)
- [Debian: package and repository signing](https://www.debian.org/doc/manuals/securing-debian-manual/deb-pack-sign.en.html)

## Native preview channel

The native apps (AppKit on macOS, egui on Windows and Linux) ship separately
from the Tauri release, through one rolling pre-release tagged
`native-preview`. `.github/workflows/native-preview.yml` runs on every merge to
`main` that touches `apps/native/`, `crates/`, or the Cargo manifests, and on
manual dispatch. It uses the same `release` environment secrets as the Tauri
release; no new secrets are needed.

- **Build number:** the workflow's run number, compiled in as
  `DBM_NATIVE_BUILD` and written to the app bundle's `CFBundleVersion`.
  Development builds have none and never check for updates.
- **macOS:** `build.sh` signs the bridge dylib and the app with the Developer
  ID and the hardened runtime. The app is notarized and stapled, zipped for
  the updater (`DBM-Native-macOS.zip`), and packaged into a signed, notarized,
  stapled `DBM-Native-macOS.dmg` for first installs. Gatekeeper opens it
  without a warning.
- **Windows and Linux:** the release `dbm-workbench` executables, published as
  `DBM-Native-Windows-x64.exe` and `DBM-Native-Linux-x64`. The Windows build is
  not Authenticode-signed yet, so SmartScreen may still warn on first run.
- **Updater signatures:** every updater artifact is signed with the Tauri
  updater key (`npx tauri signer sign`). `crates/dbm-update` verifies it
  against the public key from `tauri.conf.json` before installing anything.
- **Manifest:** `native-latest.json` lists the build number, version text,
  notes (the merge commit's subject), and each platform's URL and signature.
  The workflow uploads the binaries first and the manifest last.

Installed native builds check the manifest five seconds after launch and every
30 minutes. The top bar then offers **Update to build N**. Installing asks for
confirmation, because unsaved query text and staged edits are discarded. It
then downloads and verifies the build, swaps it in, and restarts.

- On macOS the new bundle must also pass `codesign --verify`. The app must sit
  in a folder the user can write to, such as `/Applications`.
- On Windows the running executable is renamed aside and removed on the next
  launch.

The channel never becomes the latest release, so the Tauri app's updater, which
reads `releases/latest/download/latest.json`, is unaffected.

The first native preview must be installed by hand from the `native-preview`
release page; later builds arrive automatically. Debug builds can test the
updater against a local channel by setting `DBM_UPDATE_MANIFEST` and
`DBM_UPDATE_PUBLIC_KEY`; release builds ignore both.
