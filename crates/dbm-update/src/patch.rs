//! Patch updates: rebuild a release file from the installed copy of an
//! earlier build, so an update downloads a small bsdiff patch instead of the
//! whole app.
//!
//! Each patch rebuilds one published file exactly: the Windows or Linux
//! executable, the AppImage, or on macOS `DBM-macOS.app.tar`, the
//! [`bundle_tar`] of the notarized app. The rebuilt bytes must match that
//! file's release signature, so a wrong installed copy or a damaged patch is
//! rejected like any unsigned file and the full download is used instead.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// The root folder of every [`bundle_tar`], whatever the installed app is
/// called, so archives of the same bundle always match.
const BUNDLE_ROOT: &str = "Anybase.app";

/// The file name of a bundle tar; patches that rebuild one start from the
/// installed app bundle rather than a single file.
const BUNDLE_SUFFIX: &str = ".app.tar";

/// A deterministic, uncompressed tar of the app bundle at `app`: entries in
/// byte order of their paths, symbolic links kept as links, and no owners,
/// times or extended attributes. The release and installed copies of the same
/// bundle give identical bytes, which is what makes their patches small.
pub fn bundle_tar(app: &Path) -> io::Result<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    builder.mode(tar::HeaderMode::Deterministic);
    builder.follow_symlinks(false);
    builder.append_path_with_name(app, BUNDLE_ROOT)?;
    append_children(&mut builder, app, Path::new(BUNDLE_ROOT))?;
    builder.into_inner()
}

fn append_children(
    builder: &mut tar::Builder<Vec<u8>>,
    folder: &Path,
    name: &Path,
) -> io::Result<()> {
    let mut children: Vec<_> = std::fs::read_dir(folder)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<_, _>>()?;
    children.sort();
    for child in children {
        let path = folder.join(&child);
        let child_name = name.join(&child);
        builder.append_path_with_name(&path, &child_name)?;
        if std::fs::symlink_metadata(&path)?.is_dir() {
            append_children(builder, &path, &child_name)?;
        }
    }
    Ok(())
}

/// A bsdiff patch that turns `old` into `new`. Release tooling only.
pub fn diff(old: &[u8], new: &[u8]) -> io::Result<Vec<u8>> {
    let mut patch = Vec::new();
    qbsdiff::Bsdiff::new(old, new).compare(io::Cursor::new(&mut patch))?;
    Ok(patch)
}

/// Applies a bsdiff `patch` to `old`, refusing results over `limit` bytes.
pub fn apply(old: &[u8], patch: &[u8], limit: u64) -> io::Result<Vec<u8>> {
    let patcher = qbsdiff::Bspatch::new(patch)?;
    let size = patcher.hint_target_size();
    if size > limit {
        return Err(io::Error::other("the patched file is larger than expected"));
    }
    let mut new = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    patcher.apply(old, io::Cursor::new(&mut new))?;
    Ok(new)
}

/// The installed copy a patch for the release file `target` starts from.
pub(crate) fn base(target: &str) -> io::Result<Vec<u8>> {
    let exe = std::env::current_exe()?;
    if target.ends_with(BUNDLE_SUFFIX) {
        // The outermost bundle: helpers can be apps nested inside it.
        let app = exe
            .ancestors()
            .filter(|path| path.extension().is_some_and(|extension| extension == "app"))
            .last()
            .ok_or_else(|| io::Error::other("this copy is not inside an app bundle"))?;
        return bundle_tar(app);
    }
    let path = match std::env::var_os("APPIMAGE") {
        Some(image) if target.ends_with(".AppImage") => PathBuf::from(image),
        _ => exe,
    };
    let mut data = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut data)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_rebuild_the_new_file_exactly() {
        let old: Vec<u8> = (0..200_000u32).flat_map(u32::to_le_bytes).collect();
        let mut new = old.clone();
        new[1000..1010].copy_from_slice(b"build two!");
        new.extend_from_slice(b"appended section");
        new.drain(500_000..500_100);
        let patch = diff(&old, &new).unwrap();
        assert!(
            patch.len() < new.len() / 50,
            "patch is {} bytes",
            patch.len()
        );
        assert_eq!(apply(&old, &patch, 1 << 30).unwrap(), new);
        // A different base rebuilds something else, which the release
        // signature then rejects.
        let mut other = old.clone();
        other[1_000] ^= 0xff;
        assert_ne!(apply(&other, &patch, 1 << 30).unwrap(), new);
        assert!(apply(&old, &patch, 1000).is_err(), "over the size limit");
        assert!(apply(&old, b"not a patch", 1 << 30).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn bundle_tars_are_identical_for_identical_bundles() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let make = |name: &str, order: &[&str]| {
            let root = std::env::temp_dir().join(format!(
                "dbm-bundle-{}-{name}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let app = root.join(name);
            let versions = app.join("Contents/Frameworks/Kit.framework/Versions");
            std::fs::create_dir_all(versions.join("A")).unwrap();
            // Created in a different order each time, so directory order
            // differs while the bundle is the same.
            for file in order {
                std::fs::write(app.join("Contents").join(file), *file).unwrap();
            }
            let exe = app.join("Contents/MacOS");
            std::fs::create_dir_all(&exe).unwrap();
            std::fs::write(exe.join("anybase"), b"\x7fbinary").unwrap();
            std::fs::set_permissions(exe.join("anybase"), PermissionsExt::from_mode(0o775))
                .unwrap();
            symlink("A", versions.join("Current")).unwrap();
            (root, app)
        };
        let (first_root, first) = make("Anybase.app", &["Info.plist", "PkgInfo", "z"]);
        let (second_root, second) = make("DBM.app", &["z", "PkgInfo", "Info.plist"]);
        let tar = bundle_tar(&first).unwrap();
        assert_eq!(tar, bundle_tar(&second).unwrap(), "same bundle, same bytes");

        let mut archive = tar::Archive::new(tar.as_slice());
        let entries: Vec<(String, tar::EntryType, u32)> = archive
            .entries()
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let header = entry.header();
                (
                    entry.path().unwrap().to_string_lossy().into_owned(),
                    header.entry_type(),
                    header.mode().unwrap(),
                )
            })
            .collect();
        let names: Vec<&str> = entries.iter().map(|(name, ..)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Anybase.app",
                "Anybase.app/Contents",
                "Anybase.app/Contents/Frameworks",
                "Anybase.app/Contents/Frameworks/Kit.framework",
                "Anybase.app/Contents/Frameworks/Kit.framework/Versions",
                "Anybase.app/Contents/Frameworks/Kit.framework/Versions/A",
                "Anybase.app/Contents/Frameworks/Kit.framework/Versions/Current",
                "Anybase.app/Contents/Info.plist",
                "Anybase.app/Contents/MacOS",
                "Anybase.app/Contents/MacOS/anybase",
                "Anybase.app/Contents/PkgInfo",
                "Anybase.app/Contents/z",
            ]
        );
        let kind = |name: &str| entries.iter().find(|entry| entry.0 == name).unwrap();
        assert_eq!(
            kind("Anybase.app/Contents/Frameworks/Kit.framework/Versions/Current").1,
            tar::EntryType::Symlink
        );
        assert_eq!(kind("Anybase.app/Contents/MacOS/anybase").2, 0o755);
        assert_eq!(kind("Anybase.app/Contents/PkgInfo").2, 0o644);

        // Any changed file changes the archive.
        std::fs::write(second.join("Contents/PkgInfo"), b"changed").unwrap();
        assert_ne!(tar, bundle_tar(&second).unwrap());
        std::fs::remove_dir_all(first_root).unwrap();
        std::fs::remove_dir_all(second_root).unwrap();
    }
}
