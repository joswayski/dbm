# DBM releases

DBM distributes installers directly through GitHub Releases:

- a DMG for macOS;
- an NSIS `.exe` installer for Windows; and
- `.deb` and AppImage packages for Linux.

Pushing a `v*.*.*` tag runs `.github/workflows/release.yml` and creates a draft
release. A successful build only proves that the packages were created. Do not
publish the draft until every public-release gate below is enforced and passes.

## Public-release gates

| Platform or concern | Required before publishing | Current state |
| --- | --- | --- |
| macOS | Sign with a Developer ID Application certificate, notarize with Apple, staple the notarization ticket, and validate the DMG on a clean Mac | CI signs and notarizes the app, notarizes and staples the DMG, and verifies both; clean-Mac validation remains manual |
| Windows | Authenticode-sign and RFC 3161-timestamp both `dbm.exe` and the NSIS installer with a publicly trusted code-signing identity | Not configured in CI |
| Linux | Publish the `.deb` and AppImage with `SHA256SUMS` and GitHub build-provenance attestations | Packages are built; integrity metadata is not configured |
| All platforms | Tie every artifact to the tagged commit, reject an incomplete draft, and test installation on clean supported systems | Not yet enforced by the release workflow |

Tauri updater signatures, Apple signatures, Windows Authenticode signatures,
and GitHub attestations solve different problems. One does not replace another:

- Apple and Authenticode signatures establish the operating-system publisher.
- An updater signature lets an installed application authenticate an update.
- A GitHub attestation establishes which repository, commit, and workflow built
  a downloaded artifact.
- `SHA256SUMS` detects accidental or malicious file changes after publication.

DBM does not currently enable Tauri's updater plugin or create updater
artifacts, so it does not need `TAURI_SIGNING_PRIVATE_KEY` or
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. If an updater is added later, generate a
dedicated DBM updater keypair rather than reusing Captures' key.

## GitHub release environment

Create a GitHub environment named `release`. Because DBM releases are triggered
by tags, restrict the environment to tags matching `v*` rather than to the
`main` branch alone. The release job must reference this environment before it
can access signing credentials.

Keep private keys and passwords in environment secrets. Store non-secret Azure
resource identifiers as environment variables. Do not commit credentials,
exported certificates, or temporary signing files.

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

The attestation job must grant `contents: read`, `id-token: write`, and
`attestations: write` and use GitHub's official `actions/attest` action.

An embedded GPG signature may also be added to the AppImage, but AppImage does
not automatically verify it. Do not use an embedded AppImage signature as a
replacement for checksums and build provenance.

If DBM later operates an APT repository, that repository must publish signed
`InRelease` metadata or `Release` plus `Release.gpg`. Distribute the repository
public key through an authenticated channel and configure users with a
repository-specific keyring and `signed-by=`. Signing a standalone `.deb` is
not a substitute for signing APT repository metadata.

## Publishing checklist

1. Confirm the version tag points to the intended commit on `main`.
2. Run the frontend and Rust quality gates.
3. Let the workflow create a draft release; never upload locally produced
   replacement binaries.
4. Require successful macOS signing/notarization, Windows Authenticode signing,
   and Linux integrity/provenance jobs.
5. Confirm the draft contains the DMG, NSIS installer, `.deb`, AppImage, and
   `SHA256SUMS`, and that GitHub has a verifiable attestation for every
   artifact.
6. Perform the clean-machine installation checks for macOS, Windows, and
   Ubuntu.
7. Publish the draft only after every gate passes. If any platform fails,
   delete or replace the incomplete draft before retrying.

## References

- [Apple: Developer ID certificates](https://developer.apple.com/help/account/certificates/create-developer-id-certificates)
- [Apple: notarizing macOS software](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)
- [Tauri: macOS code signing](https://v2.tauri.app/distribute/sign/macos/)
- [Microsoft: set up Artifact Signing](https://learn.microsoft.com/azure/artifact-signing/quickstart)
- [Microsoft: Artifact Signing integrations](https://learn.microsoft.com/azure/artifact-signing/how-to-signing-integrations)
- [Azure: Artifact Signing GitHub Action](https://github.com/Azure/artifact-signing-action)
- [Tauri: Windows code signing](https://v2.tauri.app/distribute/sign/windows/)
- [GitHub: artifact attestations](https://docs.github.com/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations)
- [Tauri: Linux code signing](https://v2.tauri.app/distribute/sign/linux/)
- [Debian: package and repository signing](https://www.debian.org/doc/manuals/securing-debian-manual/deb-pack-sign.en.html)
