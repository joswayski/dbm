//! Self-updates for the native DBM preview apps.
//!
//! The release workflow publishes every native build to one rolling GitHub
//! pre-release, `native-preview`, with a `native-latest.json` manifest. Each
//! artifact is signed with the same minisign key as the Tauri app's updater
//! (`tauri signer sign`), so installed copies only accept files produced by the
//! release workflow. The manifest itself is not trusted: an artifact whose
//! signature does not verify against [`PUBLIC_KEY`] is rejected.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// The channel's manifest. The rolling tag keeps this URL stable.
pub const MANIFEST_URL: &str =
    "https://github.com/joswayski/dbm/releases/download/native-preview/native-latest.json";

/// The Tauri updater's public key (`plugins.updater.pubkey` in
/// `apps/desktop/src-tauri/tauri.conf.json`): base64 of a minisign public key.
pub const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDc4QkFEMUU0MTIxREI1NjcKUldSbnRSMFM1Tkc2ZUNGYnNSRVhka2hIUTE3ak1BVVJyZTFxaVBTZ0UrWFk0c2VhVFpiWFdHenUK";

/// Downloads larger than this are refused.
const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;

/// This build's number on the preview channel, set by the release workflow
/// through `DBM_NATIVE_BUILD`. Development builds have none and never update.
pub fn current_build() -> Option<u64> {
    option_env!("DBM_NATIVE_BUILD").and_then(|build| build.trim().parse().ok())
}

/// The manifest key for this operating system and architecture.
pub fn platform() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "darwin-aarch64"
    } else if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else if cfg!(target_os = "windows") {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Manifest {
    /// Monotonic build number; newer builds have larger numbers.
    pub build: u64,
    /// Human-readable version, e.g. "Preview build 42 (abc1234)".
    pub version: String,
    pub notes: String,
    pub platforms: BTreeMap<String, Artifact>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Artifact {
    pub url: String,
    /// `tauri signer sign` output: base64 of a minisign signature file.
    pub signature: String,
}

/// A newer build for this platform.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Available {
    pub build: u64,
    pub version: String,
    pub notes: String,
    pub url: String,
    pub signature: String,
}

/// The newer build `manifest` offers this platform, if any.
pub fn select(manifest: &Manifest, current: u64, platform: &str) -> Option<Available> {
    if manifest.build <= current {
        return None;
    }
    let artifact = manifest.platforms.get(platform)?;
    Some(Available {
        build: manifest.build,
        version: manifest.version.clone(),
        notes: manifest.notes.clone(),
        url: artifact.url.clone(),
        signature: artifact.signature.clone(),
    })
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .user_agent(concat!("dbm-native/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Fetches the manifest and returns a newer build for this platform. Errors
/// only describe why the check failed; callers show them as a retry state.
pub fn check(current: u64) -> Result<Option<Available>, String> {
    let body = agent()
        .get(&debug_override("DBM_UPDATE_MANIFEST", MANIFEST_URL))
        .call()
        .map_err(|error| format!("Couldn't reach the update channel: {error}"))?
        .into_string()
        .map_err(|error| format!("Couldn't read the update manifest: {error}"))?;
    let manifest: Manifest = serde_json::from_str(&body)
        .map_err(|error| format!("The update manifest is invalid: {error}"))?;
    Ok(select(&manifest, current, platform()))
}

/// Verifies `data` against a `tauri signer` signature and [`PUBLIC_KEY`].
pub fn verify(data: &[u8], signature: &str) -> Result<(), String> {
    verify_with(
        data,
        signature,
        &debug_override("DBM_UPDATE_PUBLIC_KEY", PUBLIC_KEY),
    )
}

/// Debug builds can point at a local test channel and key; release builds
/// always use the published channel and the release key.
fn debug_override(name: &str, default: &str) -> String {
    if cfg!(debug_assertions)
        && let Ok(value) = std::env::var(name)
    {
        return value;
    }
    default.to_owned()
}

fn decode_text(encoded: &str, what: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|_| format!("The update {what} is not valid base64."))?;
    String::from_utf8(bytes).map_err(|_| format!("The update {what} is not text."))
}

fn verify_with(data: &[u8], signature: &str, public_key: &str) -> Result<(), String> {
    let key = minisign_verify::PublicKey::decode(&decode_text(public_key, "key")?)
        .map_err(|_| "The update key is invalid.".to_owned())?;
    let signature = minisign_verify::Signature::decode(&decode_text(signature, "signature")?)
        .map_err(|_| "The update signature is invalid.".to_owned())?;
    key.verify(data, &signature, false)
        .map_err(|_| "The update's signature does not match; it was not installed.".to_owned())
}

/// Downloads `update` into `directory`, verifies its signature, and returns
/// the file's path. Nothing is written unless the signature verifies.
pub fn download(update: &Available, directory: &Path) -> Result<PathBuf, String> {
    let response = agent()
        .get(&update.url)
        .call()
        .map_err(|error| format!("Couldn't download the update: {error}"))?;
    let mut data = Vec::new();
    response
        .into_reader()
        .take(MAX_DOWNLOAD_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|error| format!("The update download was interrupted: {error}"))?;
    if data.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err("The update is larger than expected; it was not installed.".into());
    }
    verify(&data, &update.signature)?;
    let name = update
        .url
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty() && !name.contains(['\\', ':']))
        .unwrap_or("dbm-update");
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("Couldn't prepare the update folder: {error}"))?;
    let path = directory.join(name);
    std::fs::write(&path, &data).map_err(|error| format!("Couldn't save the update: {error}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(build: u64) -> Manifest {
        Manifest {
            build,
            version: format!("Preview build {build}"),
            notes: "Faster grid".into(),
            platforms: BTreeMap::from([(
                "windows-x86_64".into(),
                Artifact {
                    url: "https://example.com/dbm-workbench.exe".into(),
                    signature: "sig".into(),
                },
            )]),
        }
    }

    #[test]
    fn only_newer_builds_for_this_platform_are_offered() {
        assert_eq!(select(&manifest(7), 7, "windows-x86_64"), None);
        assert_eq!(select(&manifest(6), 7, "windows-x86_64"), None);
        assert_eq!(select(&manifest(8), 7, "linux-x86_64"), None);
        let update = select(&manifest(8), 7, "windows-x86_64").unwrap();
        assert_eq!(update.build, 8);
        assert_eq!(update.url, "https://example.com/dbm-workbench.exe");
    }

    #[test]
    fn manifests_parse_from_the_published_shape() {
        let text = r#"{"build":3,"version":"Preview build 3 (abc1234)","notes":"",
            "platforms":{"darwin-aarch64":{"url":"https://x/DBM-Native-macOS.zip","signature":"s"}}}"#;
        let parsed: Manifest = serde_json::from_str(text).unwrap();
        assert_eq!(parsed.build, 3);
        assert!(parsed.platforms.contains_key("darwin-aarch64"));
    }

    #[test]
    fn the_bundled_key_decodes() {
        let text = decode_text(PUBLIC_KEY, "key").unwrap();
        assert!(minisign_verify::PublicKey::decode(&text).is_ok());
    }

    /// Signs like `tauri signer sign`: base64 of the minisign files.
    fn signed(data: &[u8]) -> (String, String) {
        let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let public = pair.pk.to_box().unwrap().to_string();
        let signature = minisign::sign(None, &pair.sk, data, None, None)
            .unwrap()
            .to_string();
        let encode = |text: String| base64::engine::general_purpose::STANDARD.encode(text);
        (encode(public), encode(signature))
    }

    #[test]
    fn signatures_from_the_release_key_verify_and_tampering_fails() {
        let data = b"dbm native build";
        let (public, signature) = signed(data);
        assert_eq!(verify_with(data, &signature, &public), Ok(()));
        assert!(verify_with(b"dbm native build!", &signature, &public).is_err());
        let (other_public, _) = signed(data);
        assert!(verify_with(data, &signature, &other_public).is_err());
        assert!(
            verify(data, &signature).is_err(),
            "only the release key is trusted"
        );
    }
}
