//! Self-updates from the native preview channel (`crates/dbm-update`).
//!
//! Checks run on a background thread shortly after launch and every 30
//! minutes. Installing downloads the signed executable, verifies it against
//! the release key, puts it in place of the running one, and restarts DBM.
//! Development builds have no channel number and never check.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use dbm_update::Available;
use eframe::egui;

const FIRST_CHECK_AFTER: f64 = 5.0;
const CHECK_EVERY: f64 = 30.0 * 60.0;

pub enum State {
    Idle,
    Checking,
    UpToDate,
    Available(Available),
    Installing,
    Failed(String),
}

enum Event {
    Checked {
        result: Result<Option<Available>, String>,
        quiet: bool,
    },
    Downloaded(Result<PathBuf, String>),
}

pub struct Updates {
    pub state: State,
    current: Option<u64>,
    next_check: f64,
    sender: Sender<Event>,
    receiver: Receiver<Event>,
}

impl Updates {
    pub fn new() -> Self {
        remove_previous_executable();
        let (sender, receiver) = mpsc::channel();
        Self {
            state: State::Idle,
            current: dbm_update::current_build(),
            next_check: FIRST_CHECK_AFTER,
            sender,
            receiver,
        }
    }

    pub fn enabled(&self) -> bool {
        self.current.is_some()
    }

    /// Applies finished work and runs the periodic quiet check.
    pub fn poll(&mut self, ctx: &egui::Context, now: f64) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                Event::Checked { result, quiet } => {
                    self.state = match (result, quiet) {
                        (Ok(Some(update)), _) => State::Available(update),
                        (Ok(None), false) => State::UpToDate,
                        (Err(error), false) => State::Failed(error),
                        // A quiet check that finds nothing leaves the control
                        // as it was.
                        (_, true) => match std::mem::replace(&mut self.state, State::Idle) {
                            State::Checking => State::Idle,
                            other => other,
                        },
                    };
                }
                Event::Downloaded(Ok(path)) => {
                    if let Err(error) = replace_and_restart(&path) {
                        self.state = State::Failed(error);
                    }
                }
                Event::Downloaded(Err(error)) => self.state = State::Failed(error),
            }
        }
        if self.enabled() {
            if now >= self.next_check {
                self.next_check = now + CHECK_EVERY;
                self.check(ctx, true);
            }
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                (self.next_check - now).max(1.0),
            ));
        }
    }

    pub fn check(&mut self, ctx: &egui::Context, quiet: bool) {
        let Some(current) = self.current else { return };
        if matches!(self.state, State::Checking | State::Installing) {
            return;
        }
        if !quiet {
            self.state = State::Checking;
        }
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = dbm_update::check(current);
            let _ = sender.send(Event::Checked { result, quiet });
            ctx.request_repaint();
        });
    }

    pub fn install(&mut self, ctx: &egui::Context, update: Available) {
        self.state = State::Installing;
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let directory = std::env::temp_dir().join(format!("dbm-update-{}", update.build));
            let result = dbm_update::download(&update, &directory);
            let _ = sender.send(Event::Downloaded(result));
            ctx.request_repaint();
        });
    }

    /// The desktop `UpdateControl`'s wording.
    pub fn label(&self) -> String {
        match &self.state {
            State::Idle => "Check for updates".into(),
            State::Checking => "Checking…".into(),
            State::UpToDate => "Up to date".into(),
            State::Available(update) => format!("Update to build {}", update.build),
            State::Installing => "Installing…".into(),
            State::Failed(_) => "Retry update".into(),
        }
    }

    pub fn detail(&self) -> String {
        match &self.state {
            State::Available(update) if !update.notes.is_empty() => update.notes.clone(),
            State::Available(update) => update.version.clone(),
            State::Failed(message) => message.clone(),
            _ => format!("Installed build {}", self.current.unwrap_or(0)),
        }
    }
}

/// Where Windows keeps the replaced executable until the next launch; a
/// running .exe can be renamed but not overwritten.
fn previous_executable(current: &Path) -> PathBuf {
    current.with_extension("previous.exe")
}

fn remove_previous_executable() {
    if cfg!(windows) {
        if let Ok(current) = std::env::current_exe() {
            let _ = std::fs::remove_file(previous_executable(&current));
        }
    }
}

/// Puts the verified download in place of this executable, starts it with the
/// same arguments, and exits. The user confirmed losing unsaved work.
fn replace_and_restart(download: &Path) -> Result<(), String> {
    let failed = |error: std::io::Error| format!("Couldn't install the update: {error}");
    let current = std::env::current_exe().map_err(failed)?;
    if cfg!(windows) {
        let previous = previous_executable(&current);
        let _ = std::fs::remove_file(&previous);
        std::fs::rename(&current, &previous).map_err(failed)?;
        if let Err(error) = std::fs::copy(download, &current) {
            let _ = std::fs::rename(&previous, &current);
            return Err(failed(error));
        }
    } else {
        // Copy beside the executable first so the final rename is atomic on
        // one filesystem; the running process keeps its open inode.
        let staged = current.with_extension("new");
        std::fs::copy(download, &staged).map_err(failed)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
                .map_err(failed)?;
        }
        std::fs::rename(&staged, &current).map_err(failed)?;
    }
    std::process::Command::new(&current)
        .args(std::env::args_os().skip(1))
        .spawn()
        .map_err(failed)?;
    std::process::exit(0);
}
