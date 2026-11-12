//! Bundled Space Mono for Pango, through fontconfig.
//!
//! The Pango in Ubuntu 24.04 (1.52) cannot add a font file to its font map,
//! so the bundled faces are written to the cache directory and added to the
//! process's fontconfig configuration before GTK starts. Nothing is installed
//! for other apps, and nothing is fetched from a font service.

use std::path::{Path, PathBuf};

const FONTS: [(&str, &[u8]); 4] = [
    (
        "SpaceMono-Regular.ttf",
        include_bytes!("../../fonts/SpaceMono-Regular.ttf"),
    ),
    (
        "SpaceMono-Bold.ttf",
        include_bytes!("../../fonts/SpaceMono-Bold.ttf"),
    ),
    (
        "SpaceMono-Italic.ttf",
        include_bytes!("../../fonts/SpaceMono-Italic.ttf"),
    ),
    (
        "SpaceMono-BoldItalic.ttf",
        include_bytes!("../../fonts/SpaceMono-BoldItalic.ttf"),
    ),
];

/// Makes Space Mono available to this process. When it cannot, the app
/// falls back to the system monospace font.
pub fn prepare() {
    let Some(directory) = directory() else {
        return;
    };
    if let Err(error) = install(&directory) {
        eprintln!("Space Mono is unavailable: {error}");
        return;
    }
    for (name, _) in FONTS {
        add_app_font(&directory.join(name));
    }
}

fn directory() -> Option<PathBuf> {
    Some(cache_directory()?.join("fonts"))
}

/// `$XDG_CACHE_HOME/dbm` (or `~/.cache/dbm`), where bundled assets are
/// written for GTK to load.
pub fn cache_directory() -> Option<PathBuf> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    Some(cache.join("dbm"))
}

fn install(directory: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(directory)?;
    for (name, bytes) in FONTS {
        let path = directory.join(name);
        if std::fs::read(&path).ok().as_deref() != Some(bytes) {
            // Write beside it and rename, so a concurrent launch never reads
            // half a font.
            let staged = path.with_extension(format!("{}.tmp", std::process::id()));
            std::fs::write(&staged, bytes)?;
            std::fs::rename(&staged, &path)?;
        }
    }
    Ok(())
}

#[allow(unsafe_code)]
mod ffi {
    use std::ffi::{c_char, c_int, c_void};

    #[link(name = "fontconfig")]
    unsafe extern "C" {
        /// Adds a font file to `config`, or the current configuration when
        /// `config` is null. Returns non-zero on success.
        pub fn FcConfigAppFontAddFile(config: *mut c_void, file: *const c_char) -> c_int;
    }
}

#[allow(unsafe_code)]
fn add_app_font(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let Ok(file) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return;
    };
    // SAFETY: a null configuration means fontconfig's current one, and the
    // path is a valid NUL-terminated string that outlives the call.
    let added = unsafe { ffi::FcConfigAppFontAddFile(std::ptr::null_mut(), file.as_ptr()) };
    if added == 0 {
        eprintln!(
            "Space Mono is unavailable: fontconfig rejected {}",
            path.display()
        );
    }
}
