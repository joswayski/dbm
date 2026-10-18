//! Self-updates for the native DBM apps.
//!
//! The release workflow publishes every build as the latest GitHub release,
//! with a `native-latest.json` manifest. Each artifact is signed with the
//! release updater key (`tools/dbm-sign`), so installed copies only accept
//! files produced by the release workflow. The manifest itself is not trusted:
//! an artifact whose signature does not verify against [`PUBLIC_KEY`] is
//! rejected.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// Commits behind each release, for its version link.
pub const REPOSITORY_URL: &str = "https://github.com/joswayski/dbm";

/// The latest release's manifest.
pub const MANIFEST_URL: &str =
    "https://github.com/joswayski/dbm/releases/latest/download/native-latest.json";

/// The release updater public key: base64 of a minisign public key. It is the
/// key the former Tauri app shipped with, so its installs accept native builds.
pub const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDc4QkFEMUU0MTIxREI1NjcKUldSbnRSMFM1Tkc2ZUNGYnNSRVhka2hIUTE3ak1BVVJyZTFxaVBTZ0UrWFk0c2VhVFpiWFdHenUK";

/// Downloads larger than this are refused.
const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;

/// This build's number, set by the release workflow through `DBM_NATIVE_BUILD`
/// (the release date and revision, e.g. 2026092802). Development builds have
/// none and never update.
pub fn current_build() -> Option<u64> {
    option_env!("DBM_NATIVE_BUILD").and_then(|build| build.trim().parse().ok())
}

/// This build's release name (e.g. "2026.09.28.2"), set through
/// `DBM_NATIVE_VERSION`; falls back to the build number.
pub fn current_version() -> Option<String> {
    option_env!("DBM_NATIVE_VERSION")
        .map(str::to_owned)
        .or_else(|| current_build().map(|build| format!("build {build}")))
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

/// The manifest key for the Linux AppImage, which replaces the whole image
/// rather than the bare executable inside it.
pub const LINUX_APPIMAGE: &str = "linux-x86_64-appimage";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Manifest {
    /// Monotonic build number; newer builds have larger numbers.
    pub build: u64,
    /// Human-readable version, e.g. "2026.09.28.2".
    pub version: String,
    pub notes: String,
    pub platforms: BTreeMap<String, Artifact>,
    /// Recent releases' highlights, newest first, including this one when
    /// they could be written. Older manifests have none.
    #[serde(default)]
    pub releases: Vec<Release>,
}

/// One release's user-facing highlights, as `scripts/release.mjs` writes them.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub build: u64,
    pub version: String,
    /// The full source commit the release was built from.
    #[serde(default)]
    pub commit: String,
    /// When it was published, in UTC (`2026-10-18T16:44:30Z`).
    pub published_at: String,
    /// Empty when a release had nothing user-facing to tell.
    #[serde(default)]
    pub highlights: Vec<Highlight>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Highlight {
    pub title: String,
    pub body: String,
}

/// Releases published on the same UTC day, newest first.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Day {
    /// The day as people read it, e.g. "October 18, 2026".
    pub date: String,
    pub releases: Vec<Release>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Artifact {
    pub url: String,
    /// `tools/dbm-sign` output: base64 of a minisign signature file.
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
    /// Highlights of every listed release newer than the installed build,
    /// newest first; a title an older release repeats is kept once. `None`
    /// when the offered release itself has none listed, so the plain notes
    /// stand in.
    #[serde(default)]
    pub highlights: Option<Vec<Highlight>>,
    /// The newest of those releases' publication day, e.g. "October 18, 2026".
    #[serde(default)]
    pub released: Option<String>,
    /// The source commit of the build being offered.
    #[serde(default)]
    pub commit: Option<String>,
}

/// The newer build `manifest` offers this platform, if any.
pub fn select(manifest: &Manifest, current: u64, platform: &str) -> Option<Available> {
    if manifest.build <= current {
        return None;
    }
    let artifact = manifest.platforms.get(platform)?;
    let mut newer: Vec<&Release> = manifest
        .releases
        .iter()
        .filter(|release| release.build > current && release.build <= manifest.build)
        .collect();
    newer.sort_by(|a, b| b.build.cmp(&a.build));
    let target_listed = newer
        .first()
        .is_some_and(|release| release.build == manifest.build);
    let highlights = target_listed.then(|| {
        let mut seen = std::collections::HashSet::new();
        newer
            .iter()
            .flat_map(|release| &release.highlights)
            .filter(|highlight| seen.insert(highlight.title.trim().to_lowercase()))
            .cloned()
            .collect()
    });
    let released = newer
        .iter()
        .find(|release| !release.highlights.is_empty())
        .or(newer.first())
        .and_then(|release| day_label(&release.published_at));
    let commit = manifest
        .releases
        .iter()
        .find(|release| release.build == manifest.build && is_commit(&release.commit))
        .map(|release| release.commit.clone());
    Some(Available {
        build: manifest.build,
        version: manifest.version.clone(),
        notes: manifest.notes.clone(),
        url: artifact.url.clone(),
        signature: artifact.signature.clone(),
        highlights,
        released,
        commit,
    })
}

/// Releases built after `after` up to and including `through` that have
/// highlights, grouped by UTC publication day, newest first.
pub fn days(releases: &[Release], after: u64, through: u64) -> Vec<Day> {
    let mut listed: Vec<&Release> = releases
        .iter()
        .filter(|release| {
            release.build > after && release.build <= through && !release.highlights.is_empty()
        })
        .collect();
    listed.sort_by(|a, b| b.build.cmp(&a.build));
    let mut days: Vec<(String, Day)> = Vec::new();
    for release in listed {
        let key = release
            .published_at
            .get(..10)
            .unwrap_or_default()
            .to_owned();
        match days.last_mut() {
            Some((last, day)) if *last == key => day.releases.push(release.clone()),
            _ => days.push((
                key,
                Day {
                    date: day_label(&release.published_at)
                        .unwrap_or_else(|| release.published_at.clone()),
                    releases: vec![release.clone()],
                },
            )),
        }
    }
    days.into_iter().map(|(_, day)| day).collect()
}

/// The GitHub page of a release's source commit.
pub fn commit_url(commit: &str) -> Option<String> {
    is_commit(commit).then(|| format!("{REPOSITORY_URL}/commit/{commit}"))
}

fn is_commit(text: &str) -> bool {
    text.len() == 40
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// "2026-10-18T16:44:30Z" as "October 18, 2026": the UTC day, so every
/// copy of Anybase groups a release under the same date.
pub fn day_label(timestamp: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let date = timestamp.get(..10)?;
    let mut parts = date.split('-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return None;
    }
    let year: u32 = year.parse().ok()?;
    let month: usize = month.parse().ok()?;
    let day: u32 = day.parse().ok()?;
    let name = MONTHS.get(month.checked_sub(1)?)?;
    (1..=31)
        .contains(&day)
        .then(|| format!("{name} {day}, {year}"))
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
    check_platform(current, platform())
}

/// [`check`] for an explicit manifest key, e.g. [`LINUX_APPIMAGE`].
pub fn check_platform(current: u64, platform: &str) -> Result<Option<Available>, String> {
    Ok(select(&fetch_manifest()?, current, platform))
}

/// The recent releases the latest manifest lists, for What's new.
pub fn history() -> Result<Vec<Release>, String> {
    Ok(fetch_manifest()?.releases)
}

fn fetch_manifest() -> Result<Manifest, String> {
    let body = agent()
        .get(&debug_override("DBM_UPDATE_MANIFEST", MANIFEST_URL))
        .call()
        .map_err(|error| format!("Couldn't reach the update channel: {error}"))?
        .into_string()
        .map_err(|error| format!("Couldn't read the update manifest: {error}"))?;
    serde_json::from_str(&body).map_err(|error| format!("The update manifest is invalid: {error}"))
}

/// Verifies `data` against a `tools/dbm-sign` signature and [`PUBLIC_KEY`].
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
    verify_signature(data, signature, public_key).map(drop)
}

/// Verifies `data` and returns the signature's trusted comment, which the
/// verified global signature covers.
fn verify_signature(data: &[u8], signature: &str, public_key: &str) -> Result<String, String> {
    let key = minisign_verify::PublicKey::decode(&decode_text(public_key, "key")?)
        .map_err(|_| "The update key is invalid.".to_owned())?;
    let signature = minisign_verify::Signature::decode(&decode_text(signature, "signature")?)
        .map_err(|_| "The update signature is invalid.".to_owned())?;
    key.verify(data, &signature, false)
        .map_err(|_| "The update's signature does not match; it was not installed.".to_owned())?;
    Ok(signature.trusted_comment().to_owned())
}

/// Verifies an artifact downloaded from `url`. `tools/dbm-sign` (like the
/// Tauri signer before it) signs `timestamp:<secs>\tfile:<name>` as the
/// trusted comment, using the same file name the release publishes, so a
/// signed artifact cannot be offered under another artifact's name, such as
/// the AppImage in place of the bare Linux executable. A signature without a
/// `file:` field is still accepted so nothing already published stops working.
fn verify_artifact(
    data: &[u8],
    signature: &str,
    public_key: &str,
    url: &str,
) -> Result<(), String> {
    let trusted = verify_signature(data, signature, public_key)?;
    let signed_name = trusted
        .split('\t')
        .find_map(|field| field.strip_prefix("file:"));
    match signed_name {
        Some(name) if Some(name) != url_file_name(url) => {
            Err("The update's signature is for a different file; it was not installed.".into())
        }
        _ => Ok(()),
    }
}

/// The last path segment of `url`, without a query or fragment.
fn url_file_name(url: &str) -> Option<&str> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit('/').next().filter(|name| !name.is_empty())
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
    verify_artifact(
        &data,
        &update.signature,
        &debug_override("DBM_UPDATE_PUBLIC_KEY", PUBLIC_KEY),
        &update.url,
    )?;
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
            releases: Vec::new(),
        }
    }

    fn release(build: u64, published_at: &str, titles: &[&str]) -> Release {
        Release {
            build,
            version: format!("2026.10.18.{build}"),
            commit: format!("{build:0>40}"),
            published_at: published_at.into(),
            highlights: titles
                .iter()
                .map(|title| Highlight {
                    title: (*title).into(),
                    body: format!("{title} body"),
                })
                .collect(),
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
    fn updates_stack_every_newer_release_and_keep_repeated_titles_once() {
        let mut offered = manifest(8);
        offered.releases = vec![
            release(5, "2026-10-17T10:00:00Z", &["Old news"]),
            release(6, "2026-10-17T20:00:00Z", &["Faster grid", "Saved filters"]),
            release(7, "2026-10-18T09:00:00Z", &[]),
            release(8, "2026-10-18T12:00:00Z", &["faster grid ", "Copy as CSV"]),
        ];
        let update = select(&offered, 5, "windows-x86_64").unwrap();
        let titles: Vec<_> = update
            .highlights
            .unwrap()
            .into_iter()
            .map(|highlight| highlight.title)
            .collect();
        // Newest first; the newest wording of a repeated title wins.
        assert_eq!(titles, ["faster grid ", "Copy as CSV", "Saved filters"]);
        assert_eq!(update.released.as_deref(), Some("October 18, 2026"));
        assert_eq!(update.commit, Some(format!("{:0>40}", 8)));

        // The offered release itself is not listed (its highlights could not
        // be written): the plain notes stand in, not a partial list.
        offered.releases.retain(|release| release.build < 8);
        let update = select(&offered, 5, "windows-x86_64").unwrap();
        assert_eq!(update.highlights, None);
        assert_eq!(update.commit, None);
        // Listed but with nothing user-facing: an empty list, not the notes.
        offered.releases = vec![release(8, "2026-10-18T12:00:00Z", &[])];
        let update = select(&offered, 5, "windows-x86_64").unwrap();
        assert_eq!(update.highlights, Some(Vec::new()));
        assert_eq!(update.released.as_deref(), Some("October 18, 2026"));
    }

    #[test]
    fn whats_new_groups_same_day_releases_under_one_utc_date() {
        let releases = [
            release(4, "2026-10-16T23:00:00Z", &["Too old"]),
            release(5, "2026-10-17T23:59:59Z", &["Late night"]),
            release(6, "2026-10-18T00:00:01Z", &["Just after midnight"]),
            release(7, "2026-10-18T09:00:00Z", &[]),
            release(8, "2026-10-18T12:00:00Z", &["Midday"]),
            release(9, "2026-10-19T12:00:00Z", &["Not installed yet"]),
        ];
        let days = days(&releases, 4, 8);
        let shape: Vec<(String, Vec<u64>)> = days
            .into_iter()
            .map(|day| (day.date, day.releases.iter().map(|r| r.build).collect()))
            .collect();
        assert_eq!(
            shape,
            [
                ("October 18, 2026".to_owned(), vec![8, 6]),
                ("October 17, 2026".to_owned(), vec![5]),
            ]
        );
    }

    #[test]
    fn dates_and_commit_links_are_strict() {
        assert_eq!(
            day_label("2026-01-05T00:00:00Z").as_deref(),
            Some("January 5, 2026")
        );
        assert_eq!(day_label("2026-13-05T00:00:00Z"), None);
        assert_eq!(day_label("soon"), None);
        let commit = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(
            commit_url(commit).as_deref(),
            Some(
                "https://github.com/joswayski/dbm/commit/0123456789abcdef0123456789abcdef01234567"
            )
        );
        assert_eq!(commit_url("abc1234"), None);
    }

    #[test]
    fn manifests_parse_from_the_published_shape() {
        let text = r#"{"build":3,"version":"Preview build 3 (abc1234)","notes":"",
            "platforms":{"darwin-aarch64":{"url":"https://x/DBM-Native-macOS.zip","signature":"s"}}}"#;
        let parsed: Manifest = serde_json::from_str(text).unwrap();
        assert_eq!(parsed.build, 3);
        assert!(parsed.platforms.contains_key("darwin-aarch64"));
        assert!(parsed.releases.is_empty());

        let text = r#"{"build":2026101801,"version":"2026.10.18.1","notes":"* Copy as CSV",
            "platforms":{},"releases":[{"build":2026101801,"version":"2026.10.18.1",
            "commit":"0123456789abcdef0123456789abcdef01234567","publishedAt":"2026-10-18T12:00:00Z",
            "highlights":[{"title":"Copy as CSV","body":"Copy rows straight into a spreadsheet."}]}]}"#;
        let parsed: Manifest = serde_json::from_str(text).unwrap();
        assert_eq!(parsed.releases[0].highlights[0].title, "Copy as CSV");
        // Hosts hand an offered update back to `download` as JSON, including
        // ones serialized before these fields existed.
        let old = r#"{"build":8,"version":"v","notes":"","url":"u","signature":"s"}"#;
        let available: Available = serde_json::from_str(old).unwrap();
        assert_eq!(available.highlights, None);
    }

    #[test]
    fn the_bundled_key_decodes() {
        let text = decode_text(PUBLIC_KEY, "key").unwrap();
        assert!(minisign_verify::PublicKey::decode(&text).is_ok());
    }

    /// Signs like `tools/dbm-sign`: base64 of the minisign files.
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

    /// Signs with a given trusted comment, like `tools/dbm-sign` does.
    fn signed_as(data: &[u8], trusted: &str) -> (String, String) {
        let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let public = pair.pk.to_box().unwrap().to_string();
        let signature = minisign::sign(
            None,
            &pair.sk,
            data,
            Some(trusted),
            Some("signature from dbm secret key"),
        )
        .unwrap()
        .to_string();
        let encode = |text: String| base64::engine::general_purpose::STANDARD.encode(text);
        (encode(public), encode(signature))
    }

    #[test]
    fn signatures_are_bound_to_the_artifact_name() {
        let data = b"dbm native build";
        let base = "https://github.com/joswayski/dbm/releases/download/v2026.10.11.1";
        let (public, signature) = signed_as(data, "timestamp:1760000000\tfile:DBM-Linux-x64");
        let check = |url: &str| verify_artifact(data, &signature, &public, url);
        assert_eq!(check(&format!("{base}/DBM-Linux-x64")), Ok(()));
        assert_eq!(check(&format!("{base}/DBM-Linux-x64?raw=1")), Ok(()));
        assert!(check(&format!("{base}/DBM-Linux-x86_64.AppImage")).is_err());
        assert!(check(&format!("{base}/")).is_err());

        // Signatures without a file field (none are published) still verify.
        let (public, signature) = signed_as(data, "timestamp:1760000000");
        assert_eq!(
            verify_artifact(data, &signature, &public, &format!("{base}/x")),
            Ok(())
        );
        // A tampered trusted comment fails the global signature.
        let (public, signature) = signed_as(data, "timestamp:1760000000\tfile:DBM-Linux-x64");
        let text = decode_text(&signature, "signature").unwrap();
        let forged = text.replace("file:DBM-Linux-x64", "file:DBM-Linux-x86_64.AppImage");
        let forged = base64::engine::general_purpose::STANDARD.encode(forged);
        assert!(
            verify_artifact(
                data,
                &forged,
                &public,
                &format!("{base}/DBM-Linux-x86_64.AppImage")
            )
            .is_err()
        );
    }
}
