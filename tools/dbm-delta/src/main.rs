//! Builds the patch updates the native DBM apps download
//! (`crates/dbm-update/src/patch.rs`).
//!
//!   dbm-delta bundle APP OUT        writes APP's bundle tar, the macOS file
//!                                   that patches rebuild
//!   dbm-delta diff OLD NEW OUT      writes a bsdiff patch from OLD to NEW,
//!                                   after checking it rebuilds NEW exactly

use std::path::Path;

fn main() {
    if let Err(error) = run() {
        eprintln!("dbm-delta: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["bundle", app, out] => {
            let tar = dbm_update::bundle_tar(Path::new(app))
                .map_err(|error| format!("couldn't archive {app}: {error}"))?;
            write(out, &tar)
        }
        ["diff", old, new, out] => {
            let old = read(old)?;
            let new = read(new)?;
            let patch =
                dbm_update::diff(&old, &new).map_err(|error| format!("couldn't diff: {error}"))?;
            let rebuilt = dbm_update::apply(&old, &patch, u64::MAX)
                .map_err(|error| format!("the patch doesn't apply: {error}"))?;
            if rebuilt != new {
                return Err("the patch doesn't rebuild the new file".into());
            }
            write(out, &patch)
        }
        _ => Err("usage: dbm-delta bundle APP OUT | dbm-delta diff OLD NEW OUT".into()),
    }
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| format!("couldn't read {path}: {error}"))
}

/// Writes `data` beside `path` first, so a failed run never leaves a partial
/// file where the release looks for one.
fn write(path: &str, data: &[u8]) -> Result<(), String> {
    let partial = format!("{path}.partial");
    std::fs::write(&partial, data)
        .and_then(|()| std::fs::rename(&partial, path))
        .map_err(|error| format!("couldn't write {path}: {error}"))?;
    println!("Wrote {path} ({} bytes)", data.len());
    Ok(())
}
