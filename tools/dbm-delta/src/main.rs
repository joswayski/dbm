//! Builds the patch updates the native DBM apps download
//! (`crates/dbm-update/src/patch.rs`).
//!
//!   dbm-delta diff OLD FROM NEW TO OUT
//!       writes the patch from build FROM to build TO at OUT, after checking
//!       that it rebuilds NEW from OLD exactly. OLD and NEW are both
//!       executables, AppImages, or unpacked macOS app bundles.
//!   dbm-delta apply PATCH FROM TO INSTALL STAGE
//!       rebuilds build TO from INSTALL (build FROM) into the folder STAGE as
//!       an installed copy does, and prints what would replace INSTALL.

use std::path::Path;

use dbm_update::patch;

fn main() {
    if let Err(error) = run() {
        eprintln!("dbm-delta: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["diff", old, from, new, to, out] => {
            let work = format!("{out}.work");
            let partial = format!("{out}.partial");
            let result = patch::generate(
                Path::new(old),
                build(from)?,
                Path::new(new),
                build(to)?,
                Path::new(&partial),
                Path::new(&work),
            );
            let _ = std::fs::remove_dir_all(&work);
            let summary = result.map_err(|error| format!("couldn't patch {old} to {new}: {error}"))?;
            // Renamed last, so a failed run never leaves a patch behind.
            std::fs::rename(&partial, out).map_err(|error| format!("couldn't write {out}: {error}"))?;
            println!(
                "{}",
                serde_json::to_string(&summary).map_err(|error| error.to_string())?
            );
            Ok(())
        }
        ["apply", patch_file, from, to, install, stage] => {
            let data = std::fs::read(patch_file)
                .map_err(|error| format!("couldn't read {patch_file}: {error}"))?;
            let expected = patch::Expected {
                layout: patch::layout_of(Path::new(install)),
                from_build: build(from)?,
                to_build: build(to)?,
            };
            let rebuilt = patch::apply(&data, &expected, Path::new(install), Path::new(stage))
                .map_err(|error| format!("couldn't apply {patch_file}: {error}"))?;
            println!("{}", rebuilt.display());
            Ok(())
        }
        _ => Err(
            "usage: dbm-delta diff OLD FROM NEW TO OUT | dbm-delta apply PATCH FROM TO INSTALL STAGE"
                .into(),
        ),
    }
}

fn build(text: &str) -> Result<u64, String> {
    text.parse()
        .map_err(|_| format!("{text:?} is not a build number"))
}
