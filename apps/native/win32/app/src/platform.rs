//! Windows shell services: save dialogs on their own threads (so neither
//! the UI nor the database worker waits on them) and opening or revealing
//! exported files. Off Windows (tests, previews) the dialog reports a cancel.

use std::path::{Path, PathBuf};

use dbm_desktop::worker::ResultExport;

fn ask(file_name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new()
            .set_file_name(file_name)
            .add_filter("CSV", &["csv"])
            .save_file()
    }
    #[cfg(not(windows))]
    {
        let _ = file_name;
        None
    }
}

/// Asks for a CSV destination and reports it (`None` when canceled).
pub fn pick_export_path(
    file_name: String,
    wake: dbm_desktop::Wake,
    done: impl FnOnce(Option<PathBuf>) + Send + 'static,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("dbm-export-dialog".into())
        .spawn(move || {
            done(ask(&file_name));
            wake();
        })
        .map(drop)
}

/// Saves already-loaded query rows as CSV without replaying the query.
pub fn save_result_csv(
    file_name: String,
    columns: Vec<String>,
    rows: Vec<Vec<serde_json::Value>>,
    wake: dbm_desktop::Wake,
    done: impl FnOnce(ResultExport) + Send + 'static,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("dbm-result-export".into())
        .spawn(move || {
            let result = ask(&file_name)
                .map(|path| {
                    let rows = dbm_core::export::export_result_csv(&path, &columns, &rows)?;
                    Ok((path, rows))
                })
                .transpose();
            done(result);
            wake();
        })
        .map(drop)
}

/// Opens an exported file with its default app, or selects it in Explorer.
pub fn open_path(path: &Path, reveal: bool) {
    let result = if reveal {
        std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn()
    } else {
        std::process::Command::new("cmd").args(["/C", "start", ""]).arg(path).spawn()
    };
    drop(result);
}
