//! Save dialogs for CSV exports. They run on their own threads, so neither
//! the UI nor the database worker waits on the dialog.

use std::path::PathBuf;

use dbm_desktop::worker::ResultExport;
use eframe::egui;

/// Asks for a CSV destination on its own thread and reports the choice
/// (`None` when canceled) through `done`.
pub fn pick_export_path(
    context: egui::Context,
    file_name: String,
    done: impl FnOnce(Option<PathBuf>) + Send + 'static,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("dbm-export-dialog".into())
        .spawn(move || {
            let path = rfd::FileDialog::new()
                .set_file_name(&file_name)
                .add_filter("CSV", &["csv"])
                .save_file();
            done(path);
            context.request_repaint();
        })
        .map(drop)
}

/// Saves already-loaded query rows as CSV, without replaying the SQL or
/// Redis command: asks for a destination and writes the file on its own
/// thread, so neither the UI nor the database worker waits on the dialog.
pub fn save_result_csv(
    context: egui::Context,
    file_name: String,
    columns: Vec<String>,
    rows: Vec<Vec<serde_json::Value>>,
    done: impl FnOnce(ResultExport) + Send + 'static,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("dbm-result-export".into())
        .spawn(move || {
            let result = rfd::FileDialog::new()
                .set_file_name(&file_name)
                .add_filter("CSV", &["csv"])
                .save_file()
                .map(|path| {
                    let rows = dbm_core::export::export_result_csv(&path, &columns, &rows)?;
                    Ok((path, rows))
                })
                .transpose();
            done(result);
            context.request_repaint();
        })
        .map(drop)
}
