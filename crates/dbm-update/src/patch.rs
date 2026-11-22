//! Patch updates: rebuild the new release from the installed app, so an
//! update downloads a small patch instead of the whole app.
//!
//! A patch turns one published build into a newer one for one [`Layout`]. It
//! lists every folder, file and link of the new release, with each file's size
//! and SHA-256, and where each file's bytes come from:
//! - copied from the installed app when they did not change,
//! - a bsdiff patch of the installed file, or
//! - carried whole when the installed app has nothing to patch from.
//!
//! Patch data is xz-compressed, which measured 8–24% smaller than bsdiff's
//! usual bzip2 on dbm's executables (see [`bsdiff`]).
//!
//! Every patch is signed with the release key like any artifact, so the hashes
//! it lists are trusted. [`apply`] rebuilds the release in a fresh stage
//! folder, rereads it, and checks that it holds exactly the listed entries with
//! exactly the listed bytes. Any failure falls back to the full download.
//!
//! Layout: `DBMPATCH1\n`, the header length (u32, little endian), the
//! xz-compressed JSON [`Header`], then the data its entries point into by
//! offset and length.
//!
//! Release tooling makes patches with `tools/dbm-delta`, which proves each one
//! by rebuilding the new release from the earlier one exactly as a client does.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 10] = b"DBMPATCH1\n";
const SCHEMA: u32 = 1;
const MAX_HEADER_BYTES: u32 = 4 * 1024 * 1024;
/// Largest single file a patch may rebuild in memory.
const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;
/// The folder a rebuilt macOS app gets, whatever the installed one is called.
const BUNDLE_ROOT: &str = "Anybase.app";
/// An AppImage's runtime, the executable before its squashfs.
#[cfg(target_os = "linux")]
const RUNTIME: &str = "runtime";
/// An AppImage's squashfs contents.
#[cfg(target_os = "linux")]
const ROOT: &str = "root";

/// What a patch rebuilds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    /// One executable (Windows, bare Linux), rebuilt from the running one.
    File,
    /// The macOS app bundle, rebuilt from the installed bundle.
    Bundle,
    /// The Linux AppImage, rebuilt from the installed image: `runtime` is its
    /// runtime and `root/…` its squashfs contents. The rebuilt image holds
    /// exactly the release's files, compressed by this copy.
    AppImage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    pub schema: u32,
    pub layout: Layout,
    pub from_build: u64,
    pub to_build: u64,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Entry {
    Dir {
        path: String,
        /// Unix permission bits; `None` where none are recorded.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<u32>,
    },
    File {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<u32>,
        size: u64,
        sha256: String,
        source: Source,
    },
    Symlink {
        path: String,
        target: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum Source {
    /// The installed file at `base` already has this entry's bytes.
    Copy { base: String },
    /// A bsdiff patch from the installed file at `base` (see [`bsdiff`]).
    Patch {
        base: String,
        base_size: u64,
        base_sha256: String,
        offset: u64,
        length: u64,
    },
    /// The whole file, xz-compressed.
    Full { offset: u64, length: u64 },
}

impl Entry {
    fn path(&self) -> &str {
        match self {
            Self::Dir { path, .. } | Self::File { path, .. } | Self::Symlink { path, .. } => path,
        }
    }
}

/// A folder, file or link as the release holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Dir {
        mode: Option<u32>,
    },
    File {
        mode: Option<u32>,
        size: u64,
        sha256: String,
    },
    Symlink {
        target: String,
    },
}

fn error(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// What the manifest and this install say a patch must be.
pub struct Expected {
    pub layout: Layout,
    pub from_build: u64,
    pub to_build: u64,
}

/// Rebuilds the release `patch` describes from `install` in a fresh `stage`
/// folder and returns what replaces the install: the executable, the app
/// bundle, or the AppImage. `install` is that same kind of thing.
pub fn apply(
    patch: &[u8],
    expected: &Expected,
    install: &Path,
    stage: &Path,
) -> io::Result<PathBuf> {
    let (header, data) = read_header(patch)?;
    if header.layout != expected.layout
        || header.from_build != expected.from_build
        || header.to_build != expected.to_build
    {
        return Err(error(
            "the patch is for a different build or kind of install",
        ));
    }
    validate(&header)?;
    if stage.exists() {
        fs::remove_dir_all(stage)?;
    }
    fs::create_dir_all(stage)?;
    let (base, root) = match header.layout {
        Layout::File => (install.to_path_buf(), stage.join("new")),
        Layout::Bundle => (install.to_path_buf(), stage.join(BUNDLE_ROOT)),
        Layout::AppImage => {
            let base = stage.join("base");
            unpack_appimage(install, &base)?;
            (base, stage.join("new"))
        }
    };
    let base_file = |path: &str| match header.layout {
        // The running executable, whatever it is called.
        Layout::File => base.clone(),
        Layout::Bundle | Layout::AppImage => join(&base, path),
    };
    fs::create_dir_all(&root)?;
    // Folders, then files, then links, so nothing is written through a link.
    for entry in &header.entries {
        if let Entry::Dir { path, .. } = entry {
            fs::create_dir_all(join(&root, path))?;
        }
    }
    for entry in &header.entries {
        let Entry::File {
            path,
            mode,
            size,
            sha256,
            source,
        } = entry
        else {
            continue;
        };
        let bytes = match source {
            Source::Copy { base } => read_limited(&base_file(base), *size)?,
            Source::Patch {
                base,
                base_size,
                base_sha256,
                offset,
                length,
            } => {
                let old = read_limited(&base_file(base), *base_size)?;
                if old.len() as u64 != *base_size || hex(&Sha256::digest(&old)) != *base_sha256 {
                    return Err(error(format!(
                        "the installed {base} is not the build this patch expects"
                    )));
                }
                bspatch(&old, slice(data, *offset, *length)?, *size)?
            }
            Source::Full { offset, length } => unxz(slice(data, *offset, *length)?, *size)?,
        };
        if bytes.len() as u64 != *size || hex(&Sha256::digest(&bytes)) != *sha256 {
            return Err(error(format!("{path} does not match the release")));
        }
        let target = join(&root, path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::File::create_new(&target)?.write_all(&bytes)?;
        set_mode(&target, *mode)?;
    }
    for entry in &header.entries {
        if let Entry::Symlink { path, target } = entry {
            symlink(target, &join(&root, path))?;
        }
    }
    // Deepest first, so a read-only folder never blocks its children.
    for entry in header.entries.iter().rev() {
        if let Entry::Dir { path, mode } = entry {
            set_mode(&join(&root, path), *mode)?;
        }
    }
    let expected_tree = nodes(&header.entries);
    verify_tree(&root, &expected_tree)?;
    Ok(match header.layout {
        Layout::File => join(&root, header.entries[0].path()),
        Layout::Bundle => root,
        Layout::AppImage => {
            let image = stage.join("update.AppImage");
            pack_appimage(&root, &image)?;
            // Read back what was written, so the image is checked too.
            let check = stage.join("check");
            unpack_appimage(&image, &check)?;
            verify_tree(&check, &expected_tree)?;
            image
        }
    })
}

fn read_header(patch: &[u8]) -> io::Result<(Header, &[u8])> {
    let rest = patch
        .strip_prefix(MAGIC.as_slice())
        .ok_or_else(|| error("not a DBM patch"))?;
    let (length, rest) = rest
        .split_first_chunk::<4>()
        .ok_or_else(|| error("the patch is cut short"))?;
    let length = u32::from_le_bytes(*length);
    if length > MAX_HEADER_BYTES || length as usize > rest.len() {
        return Err(error("the patch header is invalid"));
    }
    let (json, data) = rest.split_at(length as usize);
    let json = unxz(json, u64::from(MAX_HEADER_BYTES))?;
    let header: Header = serde_json::from_slice(&json).map_err(|e| error(e.to_string()))?;
    if header.schema != SCHEMA {
        return Err(error("unsupported patch schema"));
    }
    Ok((header, data))
}

fn slice(data: &[u8], offset: u64, length: u64) -> io::Result<&[u8]> {
    let start = usize::try_from(offset).ok();
    let end = start
        .zip(usize::try_from(length).ok())
        .and_then(|(s, l)| s.checked_add(l));
    match (start, end) {
        (Some(start), Some(end)) if end <= data.len() => Ok(&data[start..end]),
        _ => Err(error("patch data is out of range")),
    }
}

/// An installed regular file, never a link the old install may hold there,
/// read up to one byte past `size` so a longer file fails its check.
fn read_limited(path: &Path, size: u64) -> io::Result<Vec<u8>> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(error(format!("{} is not a file", path.display())));
    }
    if size > MAX_FILE_BYTES {
        return Err(error("the file is too large"));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(size + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Every path must stay inside the release and appear once (also ignoring
/// case, as Mac and Windows file systems do), links must point inside it, and
/// each layout has its own shape.
fn validate(header: &Header) -> io::Result<()> {
    let mut seen = BTreeSet::new();
    let mut links = Vec::new();
    for entry in &header.entries {
        let path = entry.path();
        if !safe_path(path) || !seen.insert(path.to_lowercase()) {
            return Err(error(format!(
                "the patch has an unsafe or repeated path {path:?}"
            )));
        }
        match entry {
            Entry::Symlink { target, .. } => {
                if cfg!(windows) || !link_stays_inside(path, target) {
                    return Err(error(format!("the patch has an unsupported link {path:?}")));
                }
                links.push(path.to_lowercase());
            }
            Entry::File {
                sha256,
                source,
                size,
                ..
            } => {
                let base_ok = match source {
                    Source::Copy { base } => safe_path(base),
                    Source::Patch {
                        base, base_sha256, ..
                    } => safe_path(base) && is_sha256(base_sha256),
                    Source::Full { .. } => true,
                };
                if !is_sha256(sha256) || !base_ok || *size > MAX_FILE_BYTES {
                    return Err(error(format!("the patch entry {path:?} is invalid")));
                }
            }
            Entry::Dir { .. } => {}
        }
    }
    // Nothing may be written beneath a link: links are created last.
    for path in &seen {
        if links
            .iter()
            .any(|link| path.starts_with(&format!("{link}/")))
        {
            return Err(error(format!(
                "the patch writes beneath a link at {path:?}"
            )));
        }
    }
    let shape_ok = match header.layout {
        Layout::File => matches!(
            header.entries.as_slice(),
            [Entry::File { path, .. }] if !path.contains('/')
        ),
        Layout::Bundle => true,
        Layout::AppImage => appimage_shape(&header.entries),
    };
    if !shape_ok {
        return Err(error("the patch does not have this install's shape"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn appimage_shape(entries: &[Entry]) -> bool {
    let has = |wanted: fn(&Entry) -> bool| entries.iter().any(wanted);
    has(|entry| matches!(entry, Entry::File { path, .. } if path == RUNTIME))
        && has(|entry| matches!(entry, Entry::Dir { path, .. } if path == ROOT))
        && entries.iter().all(|entry| {
            let path = entry.path();
            path == RUNTIME || path == ROOT || path.starts_with("root/")
        })
}

#[cfg(not(target_os = "linux"))]
fn appimage_shape(_: &[Entry]) -> bool {
    false
}

/// Relative, `/`-separated, without empty, `.` or `..` parts, and without
/// characters Windows reads as separators, drives or streams.
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".." && !part.contains(['\\', ':', '\0'])
        })
}

fn link_stays_inside(path: &str, target: &str) -> bool {
    if target.is_empty() || target.starts_with('/') || target.contains(['\\', ':', '\0']) {
        return false;
    }
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return false;
                }
            }
            part => parts.push(part),
        }
    }
    true
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn join(root: &Path, path: &str) -> PathBuf {
    path.split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

fn nodes(entries: &[Entry]) -> BTreeMap<String, Node> {
    entries
        .iter()
        .map(|entry| {
            let node = match entry {
                Entry::Dir { mode, .. } => Node::Dir { mode: *mode },
                Entry::File {
                    mode, size, sha256, ..
                } => Node::File {
                    mode: *mode,
                    size: *size,
                    sha256: sha256.clone(),
                },
                Entry::Symlink { target, .. } => Node::Symlink {
                    target: target.clone(),
                },
            };
            (entry.path().to_owned(), node)
        })
        .collect()
}

/// `root` must hold exactly `expected`, plus folders implied by its paths: no
/// extra or missing entries, and every file reread from disk and hashed.
fn verify_tree(root: &Path, expected: &BTreeMap<String, Node>) -> io::Result<()> {
    let mut wanted = expected.clone();
    for path in expected.keys() {
        let mut parent = path.as_str();
        while let Some((folder, _)) = parent.rsplit_once('/') {
            wanted
                .entry(folder.to_owned())
                .or_insert(Node::Dir { mode: None });
            parent = folder;
        }
    }
    let found = walk(root)?;
    if found.len() != wanted.len() {
        return Err(error("the rebuilt app has missing or extra files"));
    }
    for (path, node) in &wanted {
        let matches = match (node, found.get(path)) {
            (Node::Dir { mode }, Some(Node::Dir { mode: actual })) => mode_matches(*mode, *actual),
            (
                Node::File { mode, size, sha256 },
                Some(Node::File {
                    mode: actual_mode,
                    size: actual_size,
                    sha256: actual_sha256,
                }),
            ) => {
                mode_matches(*mode, *actual_mode) && size == actual_size && sha256 == actual_sha256
            }
            (Node::Symlink { target }, Some(Node::Symlink { target: actual })) => target == actual,
            _ => false,
        };
        if !matches {
            return Err(error(format!(
                "the rebuilt {path} does not match the release"
            )));
        }
    }
    Ok(())
}

fn mode_matches(expected: Option<u32>, actual: Option<u32>) -> bool {
    !cfg!(unix) || expected.is_none() || expected == actual
}

/// Every folder, file (hashed) and link under `root`, by `/`-separated path.
fn walk(root: &Path) -> io::Result<BTreeMap<String, Node>> {
    fn visit(folder: &Path, prefix: &str, found: &mut BTreeMap<String, Node>) -> io::Result<()> {
        for item in fs::read_dir(folder)? {
            let item = item?;
            let name = item
                .file_name()
                .into_string()
                .map_err(|_| error("the app has a file name that is not UTF-8"))?;
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let metadata = fs::symlink_metadata(item.path())?;
            let node = if metadata.file_type().is_symlink() {
                let target = fs::read_link(item.path())?;
                Node::Symlink {
                    target: target
                        .to_str()
                        .ok_or_else(|| error("a link target is not UTF-8"))?
                        .to_owned(),
                }
            } else if metadata.is_dir() {
                visit(&item.path(), &path, found)?;
                Node::Dir {
                    mode: unix_mode(&metadata),
                }
            } else {
                let mut hasher = Sha256::new();
                let size = io::copy(&mut fs::File::open(item.path())?, &mut hasher)?;
                Node::File {
                    mode: unix_mode(&metadata),
                    size,
                    sha256: hex(&hasher.finalize()),
                }
            };
            found.insert(path, node);
        }
        Ok(())
    }
    let mut found = BTreeMap::new();
    visit(root, "", &mut found)?;
    Ok(found)
}

#[cfg(unix)]
fn unix_mode(metadata: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(metadata.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn unix_mode(_: &fs::Metadata) -> Option<u32> {
    None
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: Option<u32>) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    match mode {
        Some(mode) => fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777)),
        None => Ok(()),
    }
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: Option<u32>) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn symlink(target: &str, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink(_: &str, _: &Path) -> io::Result<()> {
    Err(error("links are not supported on this platform"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Decompresses one xz stream, refusing more than `limit` bytes.
fn unxz(data: &[u8], limit: u64) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    lzma_rust2::XzReader::new(data, false)
        .take(limit + 1)
        .read_to_end(&mut out)?;
    if out.len() as u64 > limit {
        return Err(error("compressed patch data is larger than expected"));
    }
    Ok(out)
}

/// xz at its strongest setting, with a dictionary no larger than the data.
fn xz(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut options = lzma_rust2::XzOptions::with_preset(9);
    let lzma = &mut options.lzma_options;
    lzma.nice_len = lzma_rust2::LzmaOptions::NICE_LEN_MAX;
    lzma.depth_limit = 512;
    lzma.dict_size = u32::try_from(data.len())
        .unwrap_or(u32::MAX)
        .clamp(4096, lzma.dict_size);
    let mut writer = lzma_rust2::XzWriter::new(Vec::new(), options)?;
    writer.write_all(data)?;
    writer.finish()
}

/// Zero runs at least this long are stored as their length in a file
/// patch's diff stream. xz codes a match of at most 273 bytes, so megabytes of
/// unchanged bytes otherwise cost kilobytes; shorter runs compress better as
/// they are (measured on dbm's executables).
const ZERO_RUN: usize = 4096;

/// A bsdiff patch from `old` to `new`, as one xz stream of: the lengths of
/// the control, diff and extra streams (u64, little endian), then the
/// streams. As in bsdiff 4, the control stream holds (add, copy, seek) triples
/// of 8-byte sign-magnitude integers: add `add` bytes of the diff stream to as
/// many old bytes, copy `copy` bytes of the extra stream, then move `seek`
/// bytes in the old file. The diff stream, mostly zeros, alternates a literal
/// run (its length as a LEB128 varint, then its bytes) with the length of a
/// zero run (a varint).
fn bsdiff(old: &[u8], new: &[u8]) -> io::Result<Vec<u8>> {
    // qbsdiff writes bsdiff 4 (a 32-byte header, then bzip2 control, diff
    // and extra streams); the fastest bzip2 is enough, as it is undone here.
    let mut classic = Vec::new();
    qbsdiff::Bsdiff::new(old, new)
        .compression_level(1)
        .compare(io::Cursor::new(&mut classic))?;
    let length = |at: usize| -> io::Result<usize> {
        let bytes: [u8; 8] = classic[at..at + 8].try_into().map_err(io::Error::other)?;
        usize::try_from(u64::from_le_bytes(bytes)).map_err(io::Error::other)
    };
    if classic.get(..8) != Some(b"BSDIFF40".as_slice()) {
        return Err(io::Error::other("qbsdiff wrote an unexpected patch"));
    }
    let (diff_at, extra_at) = (32 + length(8)?, 32 + length(8)? + length(16)?);
    let raw = |stream: &[u8]| -> io::Result<Vec<u8>> {
        let mut raw = Vec::new();
        bzip2::read::BzDecoder::new(stream).read_to_end(&mut raw)?;
        Ok(raw)
    };
    let control = raw(&classic[32..diff_at])?;
    let diff = encode_zero_runs(&raw(&classic[diff_at..extra_at])?);
    let extra = raw(&classic[extra_at..])?;
    let mut payload = Vec::with_capacity(24 + control.len() + diff.len() + extra.len());
    for stream in [&control, &diff, &extra] {
        payload.extend_from_slice(&(stream.len() as u64).to_le_bytes());
    }
    for stream in [&control, &diff, &extra] {
        payload.extend_from_slice(stream);
    }
    xz(&payload)
}

fn encode_zero_runs(diff: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut literal_start = 0;
    let mut at = 0;
    while at < diff.len() {
        if diff[at] != 0 {
            at += 1;
            continue;
        }
        let run = diff[at..].iter().take_while(|byte| **byte == 0).count();
        if run >= ZERO_RUN {
            put_varint(&mut out, (at - literal_start) as u64);
            out.extend_from_slice(&diff[literal_start..at]);
            put_varint(&mut out, run as u64);
            literal_start = at + run;
        }
        at += run;
    }
    put_varint(&mut out, (diff.len() - literal_start) as u64);
    out.extend_from_slice(&diff[literal_start..]);
    put_varint(&mut out, 0);
    out
}

/// The diff stream [`encode_zero_runs`] wrote, refusing more than `limit` bytes.
fn decode_zero_runs(mut encoded: &[u8], limit: u64) -> io::Result<Vec<u8>> {
    let invalid = || error("the file patch is invalid");
    let mut diff = Vec::new();
    while !encoded.is_empty() {
        let literal = usize::try_from(take_varint(&mut encoded).ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
        let bytes = encoded.get(..literal).ok_or_else(invalid)?;
        diff.extend_from_slice(bytes);
        encoded = &encoded[literal..];
        let zeros = take_varint(&mut encoded).ok_or_else(invalid)?;
        if (diff.len() as u64).saturating_add(zeros) > limit {
            return Err(invalid());
        }
        diff.resize(diff.len() + zeros as usize, 0);
    }
    Ok(diff)
}

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn take_varint(input: &mut &[u8]) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = input.split_first()?;
        *input = rest;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// Applies a [`bsdiff`] patch to `old`, producing at most `size` bytes.
fn bspatch(old: &[u8], patch: &[u8], size: u64) -> io::Result<Vec<u8>> {
    let invalid = || error("the file patch does not fit the installed file");
    // The control stream holds a triple per step; the encoded diff stream is
    // at most about twice the file.
    let limit = size
        .saturating_add(1)
        .saturating_mul(28)
        .saturating_add(4096);
    let payload = unxz(patch, limit)?;
    let mut lengths = [0usize; 3];
    let mut rest = payload.as_slice();
    for length in &mut lengths {
        let (bytes, tail) = rest.split_first_chunk::<8>().ok_or_else(invalid)?;
        *length = usize::try_from(u64::from_le_bytes(*bytes)).map_err(|_| invalid())?;
        rest = tail;
    }
    let control = rest.get(..lengths[0]).ok_or_else(invalid)?;
    let rest = &rest[lengths[0]..];
    let diff = decode_zero_runs(rest.get(..lengths[1]).ok_or_else(invalid)?, size)?;
    let extra = rest.get(lengths[1]..).ok_or_else(invalid)?;
    if extra.len() != lengths[2] || control.len() % 24 != 0 {
        return Err(invalid());
    }
    let number = |bytes: &[u8]| -> i64 {
        let value = u64::from_le_bytes(bytes.try_into().unwrap_or_default());
        let magnitude = (value & !(1 << 63)) as i64;
        if value >> 63 == 1 {
            -magnitude
        } else {
            magnitude
        }
    };
    let mut new = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    let (mut old_at, mut diff_at, mut extra_at) = (0i64, 0usize, 0usize);
    for triple in control.chunks_exact(24) {
        let add = usize::try_from(number(&triple[..8])).map_err(|_| invalid())?;
        let copy = usize::try_from(number(&triple[8..16])).map_err(|_| invalid())?;
        let seek = number(&triple[16..]);
        if (new.len() as u64).saturating_add((add as u64).saturating_add(copy as u64)) > size {
            return Err(invalid());
        }
        let start = usize::try_from(old_at).map_err(|_| invalid())?;
        let old_bytes = old
            .get(start..start.saturating_add(add))
            .ok_or_else(invalid)?;
        let diff_bytes = diff.get(diff_at..diff_at + add).ok_or_else(invalid)?;
        new.extend(
            old_bytes
                .iter()
                .zip(diff_bytes)
                .map(|(o, d)| o.wrapping_add(*d)),
        );
        new.extend_from_slice(extra.get(extra_at..extra_at + copy).ok_or_else(invalid)?);
        diff_at += add;
        extra_at += copy;
        old_at = old_at
            .checked_add(add as i64)
            .and_then(|at| at.checked_add(seek))
            .ok_or_else(invalid)?;
    }
    Ok(new)
}

#[cfg(target_os = "linux")]
mod appimage {
    //! Reading and writing AppImages: an ELF runtime followed by a squashfs.

    use super::*;
    use backhand::compression::Compressor;
    use backhand::{
        FilesystemCompressor, FilesystemReader, FilesystemWriter, InnerNode, NodeHeader,
    };

    /// Where the squashfs starts: the end of the runtime's ELF section
    /// headers, as the AppImage runtime itself computes it.
    fn offset(image: &Path) -> io::Result<u64> {
        let mut head = [0u8; 64];
        fs::File::open(image)?.read_exact(&mut head)?;
        if &head[..4] != b"\x7fELF" || head[4] != 2 || head[5] != 1 {
            return Err(error("the AppImage runtime is not a 64-bit ELF"));
        }
        let shoff = u64::from_le_bytes(head[0x28..0x30].try_into().map_err(io::Error::other)?);
        let entsize = u64::from(u16::from_le_bytes([head[0x3a], head[0x3b]]));
        let count = u64::from(u16::from_le_bytes([head[0x3c], head[0x3d]]));
        Ok(shoff + entsize * count)
    }

    fn squashfs_error(error: backhand::BackhandError) -> io::Error {
        io::Error::other(format!("squashfs: {error}"))
    }

    /// Unpacks `image` into `destination` as `runtime` and `root/…`.
    pub fn unpack(image: &Path, destination: &Path) -> io::Result<()> {
        let offset = offset(image)?;
        let root = destination.join(ROOT);
        fs::create_dir_all(&root)?;
        let mut runtime = Vec::new();
        fs::File::open(image)?
            .take(offset)
            .read_to_end(&mut runtime)?;
        if runtime.len() as u64 != offset {
            return Err(error("the AppImage is cut short"));
        }
        let runtime_path = destination.join(RUNTIME);
        fs::write(&runtime_path, runtime)?;
        set_mode(&runtime_path, Some(0o644))?;
        let reader = io::BufReader::new(fs::File::open(image)?);
        let squashfs =
            FilesystemReader::from_reader_with_offset(reader, offset).map_err(squashfs_error)?;
        let mut folders = Vec::new();
        for node in squashfs.files() {
            let name = node
                .fullpath
                .to_str()
                .ok_or_else(|| error("an AppImage path is not UTF-8"))?
                .trim_start_matches('/');
            let mode = Some(u32::from(node.header.permissions) & 0o777);
            if name.is_empty() {
                folders.push((root.clone(), mode));
                continue;
            }
            if !safe_path(name) {
                return Err(error(format!("the AppImage has an unsafe path {name:?}")));
            }
            let path = join(&root, name);
            match &node.inner {
                InnerNode::Dir(_) => {
                    fs::create_dir_all(&path)?;
                    folders.push((path, mode));
                }
                InnerNode::File(file) => {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    let mut output = fs::File::create_new(&path)?;
                    io::copy(&mut squashfs.file(file).reader(), &mut output)?;
                    set_mode(&path, mode)?;
                }
                InnerNode::Symlink(link) => {
                    let target = link
                        .link
                        .to_str()
                        .ok_or_else(|| error("an AppImage link is not UTF-8"))?;
                    if !link_stays_inside(&format!("{ROOT}/{name}"), target) {
                        return Err(error(format!(
                            "the AppImage link {name:?} leaves the image"
                        )));
                    }
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    symlink(target, &path)?;
                }
                _ => {
                    return Err(error(format!(
                        "the AppImage has an unsupported entry {name:?}"
                    )));
                }
            }
        }
        // Deepest first, so a read-only folder never blocks its children.
        folders.sort_by(|a, b| b.0.cmp(&a.0));
        for (folder, mode) in folders {
            set_mode(&folder, mode)?;
        }
        Ok(())
    }

    /// Writes `source`'s `runtime` and zstd-compressed `root/…` as an
    /// executable AppImage at `image`. The zstd level only sets how much disk
    /// the image takes (downloads are patches or the published image), so it
    /// is fast rather than small.
    pub fn pack(source: &Path, image: &Path) -> io::Result<()> {
        let runtime = fs::read(source.join(RUNTIME))?;
        let root = source.join(ROOT);
        let tree = walk(&root)?;
        let header = |mode: Option<u32>, fallback: u32| {
            let mode = u16::try_from(mode.unwrap_or(fallback) & 0o777).unwrap_or(0o755);
            NodeHeader::new(mode, 0, 0, 0)
        };
        let mut squashfs = FilesystemWriter::default();
        squashfs.set_compressor(
            FilesystemCompressor::new(Compressor::Zstd, None).map_err(squashfs_error)?,
        );
        squashfs.set_root_mode(header(unix_mode(&fs::metadata(&root)?), 0o755).permissions);
        for (path, node) in &tree {
            match node {
                Node::Dir { mode } => squashfs.push_dir(path, header(*mode, 0o755)),
                Node::File { mode, .. } => {
                    squashfs.push_file_from_path(join(&root, path), path, header(*mode, 0o644))
                }
                Node::Symlink { target } => {
                    squashfs.push_symlink(target, path, header(None, 0o777))
                }
            }
            .map_err(squashfs_error)?;
        }
        let mut output = fs::File::create(image)?;
        output.write_all(&runtime)?;
        squashfs
            .write_with_offset(&mut output, runtime.len() as u64)
            .map_err(squashfs_error)?;
        output.sync_all()?;
        set_mode(image, Some(0o755))
    }
}

#[cfg(target_os = "linux")]
use appimage::{pack as pack_appimage, unpack as unpack_appimage};

#[cfg(not(target_os = "linux"))]
fn unpack_appimage(_: &Path, _: &Path) -> io::Result<()> {
    Err(error("AppImages are only patched on Linux"))
}

#[cfg(not(target_os = "linux"))]
fn pack_appimage(_: &Path, _: &Path) -> io::Result<()> {
    Err(error("AppImages are only patched on Linux"))
}

// Release tooling: making a patch from two published releases.

/// What [`generate`] reports about the patch it wrote.
#[derive(Debug, Serialize)]
pub struct Summary {
    pub size: u64,
    pub copied: usize,
    pub patched: usize,
    pub full: usize,
}

/// The layout of a release file or folder: an app bundle folder, an
/// AppImage, or a single executable.
pub fn layout_of(path: &Path) -> Layout {
    if path.is_dir() {
        Layout::Bundle
    } else if path
        .extension()
        .is_some_and(|extension| extension == "AppImage")
    {
        Layout::AppImage
    } else {
        Layout::File
    }
}

/// Writes the patch from `base` (build `from_build`) to `target` at `output`,
/// using `work` as scratch space, and proves it by rebuilding `target` from
/// `base` exactly as a client does. Both are release files or app folders of
/// the same [`layout_of`].
pub fn generate(
    base: &Path,
    from_build: u64,
    target: &Path,
    to_build: u64,
    output: &Path,
    work: &Path,
) -> io::Result<Summary> {
    let layout = layout_of(target);
    if layout_of(base) != layout {
        return Err(error("the base and target are different kinds of release"));
    }
    if from_build >= to_build {
        return Err(error("a patch must go to a newer build"));
    }
    fs::create_dir_all(work)?;
    let (base_root, base_tree, target_root, target_tree) = match layout {
        Layout::File => {
            let name = target
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| error("the target has no file name"))?;
            let file = |path: &Path| -> io::Result<BTreeMap<String, Node>> {
                let bytes = fs::read(path)?;
                let node = Node::File {
                    mode: None,
                    size: bytes.len() as u64,
                    sha256: hex(&Sha256::digest(&bytes)),
                };
                Ok(BTreeMap::from([(name.to_owned(), node)]))
            };
            let parent = |path: &Path| path.parent().unwrap_or(Path::new(".")).to_path_buf();
            // Read by the target's name in both, as a client reads its own
            // executable whatever it is called.
            let base_copy = work.join("base");
            fs::create_dir_all(&base_copy)?;
            fs::copy(base, base_copy.join(name))?;
            (base_copy, file(base)?, parent(target), file(target)?)
        }
        Layout::Bundle => (
            base.to_path_buf(),
            walk(base)?,
            target.to_path_buf(),
            walk(target)?,
        ),
        Layout::AppImage => {
            let (base_dir, target_dir) = (work.join("base"), work.join("target"));
            unpack_appimage(base, &base_dir)?;
            unpack_appimage(target, &target_dir)?;
            (
                base_dir.clone(),
                walk(&base_dir)?,
                target_dir.clone(),
                walk(&target_dir)?,
            )
        }
    };
    let base_files: HashMap<&str, &str> = base_tree
        .iter()
        .filter_map(|(path, node)| match node {
            Node::File { sha256, .. } => Some((sha256.as_str(), path.as_str())),
            _ => None,
        })
        .collect();

    let mut data = Vec::new();
    let mut entries = Vec::new();
    let (mut copied, mut patched, mut full) = (0, 0, 0);
    for (path, node) in &target_tree {
        entries.push(match node {
            Node::Dir { mode } => Entry::Dir {
                path: path.clone(),
                mode: *mode,
            },
            Node::Symlink { target } => Entry::Symlink {
                path: path.clone(),
                target: target.clone(),
            },
            Node::File { mode, size, sha256 } => {
                let same_path = matches!(
                    base_tree.get(path),
                    Some(Node::File { sha256: base, .. }) if base == sha256
                );
                let source = if same_path {
                    copied += 1;
                    Source::Copy { base: path.clone() }
                } else if let Some(base) = base_files.get(sha256.as_str()) {
                    copied += 1;
                    Source::Copy {
                        base: (*base).to_owned(),
                    }
                } else {
                    let bytes = fs::read(join(&target_root, path))?;
                    let whole = xz(&bytes)?;
                    let offset = data.len() as u64;
                    // A file patch only when it is smaller than the whole file.
                    let delta = match base_tree.get(path) {
                        Some(Node::File {
                            size: base_size,
                            sha256: base_sha256,
                            ..
                        }) => {
                            let delta = bsdiff(&fs::read(join(&base_root, path))?, &bytes)?;
                            (delta.len() < whole.len())
                                .then(|| (delta, *base_size, base_sha256.clone()))
                        }
                        _ => None,
                    };
                    if let Some((delta, base_size, base_sha256)) = delta {
                        patched += 1;
                        data.extend_from_slice(&delta);
                        Source::Patch {
                            base: path.clone(),
                            base_size,
                            base_sha256,
                            offset,
                            length: delta.len() as u64,
                        }
                    } else {
                        full += 1;
                        data.extend_from_slice(&whole);
                        Source::Full {
                            offset,
                            length: whole.len() as u64,
                        }
                    }
                };
                Entry::File {
                    path: path.clone(),
                    mode: *mode,
                    size: *size,
                    sha256: sha256.clone(),
                    source,
                }
            }
        });
    }
    let header = Header {
        schema: SCHEMA,
        layout,
        from_build,
        to_build,
        entries,
    };
    let json = serde_json::to_vec(&header).map_err(io::Error::other)?;
    if json.len() > MAX_HEADER_BYTES as usize {
        return Err(error("the patch header is too large"));
    }
    let json = xz(&json)?;
    let length = u32::try_from(json.len()).map_err(io::Error::other)?;
    let mut patch = Vec::with_capacity(MAGIC.len() + 4 + json.len() + data.len());
    patch.extend_from_slice(MAGIC);
    patch.extend_from_slice(&length.to_le_bytes());
    patch.extend_from_slice(&json);
    patch.extend_from_slice(&data);

    // The same code path a client runs, against the base as it installs.
    let install = match layout {
        Layout::File => base_root.join(header.entries[0].path()),
        Layout::Bundle | Layout::AppImage => base.to_path_buf(),
    };
    let expected = Expected {
        layout,
        from_build,
        to_build,
    };
    let rebuilt = apply(&patch, &expected, &install, &work.join("stage"))?;
    if layout == Layout::File && fs::read(&rebuilt)? != fs::read(target)? {
        return Err(error("the patch does not rebuild the target"));
    }
    fs::write(output, &patch)?;
    Ok(Summary {
        size: patch.len() as u64,
        copied,
        patched,
        full,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Large enough that a binary patch is much smaller than the file.
    fn program(seed: u8) -> Vec<u8> {
        (0..200_000u32)
            .map(|index| (index.wrapping_mul(2_654_435_761) >> 13) as u8 ^ seed)
            .collect()
    }

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "dbm-patch-{}-{name}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn expected(layout: Layout, from_build: u64, to_build: u64) -> Expected {
        Expected {
            layout,
            from_build,
            to_build,
        }
    }

    #[test]
    fn file_patches_rebuild_the_exact_bytes_and_refuse_other_bases() {
        let old: Vec<u8> = (0..200_000u32).flat_map(u32::to_le_bytes).collect();
        let mut new = old.clone();
        new[1000..1010].copy_from_slice(b"build two!");
        new.extend_from_slice(b"appended section");
        new.drain(500_000..500_100);
        let delta = bsdiff(&old, &new).unwrap();
        assert!(
            delta.len() < new.len() / 50,
            "patch is {} bytes",
            delta.len()
        );
        assert_eq!(bspatch(&old, &delta, new.len() as u64).unwrap(), new);
        // A different base rebuilds something else, which the hash rejects.
        let mut other = old.clone();
        other[1_000] ^= 0xff;
        assert_ne!(bspatch(&other, &delta, new.len() as u64).unwrap(), new);
        assert!(bspatch(&old, &delta, 1000).is_err(), "over the size limit");
        assert!(
            bspatch(&old[..100_000], &delta, new.len() as u64).is_err(),
            "base too short"
        );
        assert!(bspatch(&old, b"not a patch", 1 << 20).is_err());
    }

    #[test]
    fn diff_streams_keep_every_byte_through_zero_run_encoding() {
        let mut diff = vec![0u8; ZERO_RUN - 1];
        diff.extend_from_slice(&[1, 0, 2]);
        diff.extend(std::iter::repeat_n(0, ZERO_RUN * 3));
        diff.push(3);
        diff.extend(std::iter::repeat_n(0, ZERO_RUN));
        let encoded = encode_zero_runs(&diff);
        assert!(encoded.len() < 2 * ZERO_RUN, "{} bytes", encoded.len());
        assert_eq!(decode_zero_runs(&encoded, diff.len() as u64).unwrap(), diff);
        assert!(decode_zero_runs(&encoded, diff.len() as u64 - 1).is_err());
        for diff in [Vec::new(), vec![0; ZERO_RUN], vec![9; 10]] {
            assert_eq!(
                decode_zero_runs(&encode_zero_runs(&diff), 1 << 20).unwrap(),
                diff
            );
        }
    }

    #[test]
    fn executables_patch_from_the_running_copy_whatever_its_name() {
        let root = scratch("file");
        let old = program(3);
        let mut new = old.clone();
        new[50_000] ^= 0xff;
        let (base, target) = (
            root.join("old/DBM-Windows-x64.exe"),
            root.join("new/DBM-Windows-x64.exe"),
        );
        for (path, bytes) in [(&base, &old), (&target, &new)] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let patch = root.join("update.patch");
        let summary = generate(&base, 1, &target, 2, &patch, &root.join("work")).unwrap();
        assert_eq!((summary.copied, summary.patched, summary.full), (0, 1, 0));
        let patch = fs::read(&patch).unwrap();
        assert!(patch.len() < 5_000, "{} bytes", patch.len());

        let installed = root.join("install/dbm.exe");
        fs::create_dir_all(installed.parent().unwrap()).unwrap();
        fs::write(&installed, &old).unwrap();
        let stage = root.join("stage");
        let rebuilt = apply(&patch, &expected(Layout::File, 1, 2), &installed, &stage).unwrap();
        assert_eq!(fs::read(&rebuilt).unwrap(), new);
        // Another build, another kind of install, or a changed executable.
        for wrong in [
            expected(Layout::File, 0, 2),
            expected(Layout::File, 1, 3),
            expected(Layout::Bundle, 1, 2),
        ] {
            assert!(apply(&patch, &wrong, &installed, &stage).is_err());
        }
        let mut changed = old.clone();
        changed[10] ^= 1;
        fs::write(&installed, &changed).unwrap();
        assert!(apply(&patch, &expected(Layout::File, 1, 2), &installed, &stage).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    fn bundle(root: &Path, files: &[(&str, &[u8], u32)], links: &[(&str, &str)]) -> PathBuf {
        let app = root.join("Anybase.app");
        for (name, bytes, mode) in files {
            let path = join(&app, name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, bytes).unwrap();
            set_mode(&path, Some(*mode)).unwrap();
        }
        for (name, target) in links {
            symlink(target, &join(&app, name)).unwrap();
        }
        app
    }

    #[cfg(unix)]
    #[test]
    fn bundles_rebuild_every_file_mode_and_link_of_the_release() {
        let root = scratch("bundle");
        let old = program(0);
        let mut new = old.clone();
        new[1_000..1_040].copy_from_slice(&[7; 40]);
        new.extend_from_slice(b"one more function");
        let base = bundle(
            &root.join("base"),
            &[
                ("Contents/MacOS/dbm", &old, 0o755),
                ("Contents/Info.plist", b"plist", 0o644),
                ("Contents/Resources/LICENSE", b"license", 0o644),
                (
                    "Contents/Frameworks/Kit.framework/Versions/A/Kit",
                    b"kit",
                    0o755,
                ),
                (
                    "Contents/Resources/removed.txt",
                    b"gone in the update",
                    0o644,
                ),
            ],
            &[("Contents/Frameworks/Kit.framework/Versions/Current", "A")],
        );
        let target = bundle(
            &root.join("target"),
            &[
                ("Contents/MacOS/dbm", &new, 0o755),
                ("Contents/Info.plist", b"plist", 0o644),
                // Same bytes at a new path: copied, not downloaded.
                ("Contents/Resources/docs/LICENSE", b"license", 0o600),
                (
                    "Contents/Frameworks/Kit.framework/Versions/A/Kit",
                    b"kit",
                    0o755,
                ),
                ("Contents/Resources/new.txt", b"a brand new file", 0o644),
            ],
            &[("Contents/Frameworks/Kit.framework/Versions/Current", "A")],
        );
        let patch = root.join("update.patch");
        let summary = generate(&base, 7, &target, 9, &patch, &root.join("work")).unwrap();
        assert_eq!((summary.copied, summary.patched, summary.full), (3, 1, 1));
        assert!(summary.size < 20_000, "{} bytes", summary.size);
        let patch = fs::read(&patch).unwrap();

        // The installed copy may be called anything; the rebuild is Anybase.app.
        let installed = root.join("Applications/DBM.app");
        fs::create_dir_all(installed.parent().unwrap()).unwrap();
        fs::rename(&base, &installed).unwrap();
        let stage = root.join("stage");
        let rebuilt = apply(&patch, &expected(Layout::Bundle, 7, 9), &installed, &stage).unwrap();
        assert_eq!(rebuilt, stage.join("Anybase.app"));
        assert_eq!(
            walk(&rebuilt).unwrap(),
            walk(&target).unwrap(),
            "same entries, modes, links and bytes"
        );
        assert!(!rebuilt.join("Contents/Resources/removed.txt").exists());

        // A copied file and a patch base changed after install, one at a time.
        for changed in ["Contents/Info.plist", "Contents/MacOS/dbm"] {
            let path = join(&installed, changed);
            let original = fs::read(&path).unwrap();
            let mut edited = original.clone();
            edited[0] ^= 1;
            fs::write(&path, edited).unwrap();
            assert!(
                apply(&patch, &expected(Layout::Bundle, 7, 9), &installed, &stage).is_err(),
                "{changed}"
            );
            fs::write(&path, original).unwrap();
        }
        // A file replaced by a link to the right bytes elsewhere is not a base.
        let plist = join(&installed, "Contents/Info.plist");
        fs::rename(&plist, root.join("elsewhere")).unwrap();
        symlink(root.join("elsewhere").to_str().unwrap(), &plist).unwrap();
        assert!(apply(&patch, &expected(Layout::Bundle, 7, 9), &installed, &stage).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_paths_and_links_that_leave_or_alias_the_app() {
        let file = |path: &str| Entry::File {
            path: path.into(),
            mode: None,
            size: 1,
            sha256: "0".repeat(64),
            source: Source::Full {
                offset: 0,
                length: 1,
            },
        };
        let link = |path: &str, target: &str| Entry::Symlink {
            path: path.into(),
            target: target.into(),
        };
        let header = |layout, entries| Header {
            schema: SCHEMA,
            layout,
            from_build: 1,
            to_build: 2,
            entries,
        };
        // Windows installs never hold links, so every link is refused there.
        assert_eq!(
            validate(&header(
                Layout::Bundle,
                vec![file("a"), link("lib", "sub/../a")]
            ))
            .is_ok(),
            cfg!(unix)
        );
        for entries in [
            vec![file("../escaped")],
            vec![file("a/../../escaped")],
            vec![file("/etc/passwd")],
            vec![file("C:evil")],
            vec![file("a\\..\\b")],
            vec![file("Contents"), file("contents")],
            vec![link("lib", "../../outside")],
            vec![link("lib", "/usr/lib")],
            vec![link("lib", "a"), file("lib/inside")],
        ] {
            assert!(
                validate(&header(Layout::Bundle, entries.clone())).is_err(),
                "{entries:?}"
            );
        }
        // An executable patch holds exactly one file.
        assert!(validate(&header(Layout::File, vec![file("dbm.exe")])).is_ok());
        assert!(validate(&header(Layout::File, vec![file("a"), file("b")])).is_err());
        assert!(validate(&header(Layout::File, vec![file("bin/dbm")])).is_err());
        // An AppImage patch holds a runtime and the image's root.
        let appimage = vec![
            file("runtime"),
            Entry::Dir {
                path: "root".into(),
                mode: None,
            },
            file("root/AppRun"),
        ];
        assert_eq!(
            validate(&header(Layout::AppImage, appimage)).is_ok(),
            cfg!(target_os = "linux")
        );
        assert!(
            validate(&header(
                Layout::AppImage,
                vec![file("runtime"), file("elsewhere")]
            ))
            .is_err()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn appimages_rebuild_from_the_installed_image_into_a_working_image() {
        let root = scratch("appimage");
        // A real 64-bit ELF stands in for the AppImage runtime.
        let runtime = fs::read("/proc/self/exe").unwrap();
        let image = |name: &str, binary: &[u8], icon: &[u8]| {
            let source = root.join(format!("{name}-source"));
            fs::create_dir_all(source.join("root/usr/bin")).unwrap();
            fs::write(source.join(RUNTIME), &runtime).unwrap();
            fs::write(source.join("root/usr/bin/dbm-workbench"), binary).unwrap();
            set_mode(&source.join("root/usr/bin/dbm-workbench"), Some(0o755)).unwrap();
            fs::write(source.join("root/dbm.png"), icon).unwrap();
            symlink("usr/bin/dbm-workbench", &source.join("root/AppRun")).unwrap();
            let image = root.join(format!("{name}.AppImage"));
            pack_appimage(&source, &image).unwrap();
            image
        };
        let old = program(0);
        let mut new = old.clone();
        new[2_000..2_010].copy_from_slice(b"build two!");
        let base = image("base", &old, b"icon");
        let target = image("target", &new, b"new icon");
        let patch = root.join("update.patch");
        let summary = generate(&base, 1, &target, 2, &patch, &root.join("work")).unwrap();
        // The runtime is copied; the binary patched; the changed icon is
        // smaller whole than as a patch.
        assert_eq!((summary.copied, summary.patched, summary.full), (1, 1, 1));
        let patch = fs::read(&patch).unwrap();
        let stage = root.join("stage");
        let rebuilt = apply(&patch, &expected(Layout::AppImage, 1, 2), &base, &stage).unwrap();
        let (from_patch, from_release) = (root.join("from-patch"), root.join("from-release"));
        unpack_appimage(&rebuilt, &from_patch).unwrap();
        unpack_appimage(&target, &from_release).unwrap();
        assert_eq!(walk(&from_patch).unwrap(), walk(&from_release).unwrap());
        assert_eq!(
            fs::read(from_patch.join("root/usr/bin/dbm-workbench")).unwrap(),
            new
        );
        fs::remove_dir_all(root).unwrap();
    }
}
