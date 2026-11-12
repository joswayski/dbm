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
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()
    } else {
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .spawn()
    };
    drop(result);
}

/// Opens the system colour picker on its own thread and reports the chosen
/// colour (`None` when canceled; always `None` off Windows).
pub fn pick_color(
    initial: [u8; 3],
    wake: dbm_desktop::Wake,
    done: impl FnOnce(Option<[u8; 3]>) + Send + 'static,
) {
    let _ = std::thread::Builder::new()
        .name("dbm-color-dialog".into())
        .spawn(move || {
            done(choose_color(initial));
            wake();
        });
}

#[cfg(windows)]
fn choose_color([r, g, b]: [u8; 3]) -> Option<[u8; 3]> {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::UI::Controls::Dialogs::{
        CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW,
    };
    let mut custom = [COLORREF(0x00ff_ffff); 16];
    let mut cc = CHOOSECOLORW {
        lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
        rgbResult: COLORREF(u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    // SAFETY: `cc` and `custom` outlive the modal call.
    let ok = unsafe { ChooseColorW(&mut cc) }.as_bool();
    let c = cc.rgbResult.0;
    ok.then_some([
        (c & 0xff) as u8,
        (c >> 8 & 0xff) as u8,
        (c >> 16 & 0xff) as u8,
    ])
}

#[cfg(not(windows))]
fn choose_color(_: [u8; 3]) -> Option<[u8; 3]> {
    None
}
