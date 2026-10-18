//! Self-updates from the native preview channel (`crates/dbm-update`).
//!
//! Checks run on a background thread shortly after launch and every 30
//! minutes. Installing downloads the signed executable (or, when running from
//! an AppImage, the signed AppImage), verifies it against the release key,
//! puts it in place of the running one, and restarts DBM.
//! Development builds have no channel number and never check.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use dbm_update::{Available, Day, Release};
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
    History {
        result: Result<Vec<Release>, String>,
        /// Opened after an update rather than from the Help menu.
        unread: bool,
    },
}

/// The What's new dialog: release highlights grouped by UTC day.
pub enum WhatsNew {
    Closed,
    Loading,
    Shown(Vec<Day>),
    Failed(String),
}

pub struct Updates {
    pub state: State,
    pub whats_new: WhatsNew,
    current: Option<u64>,
    /// The newest build whose highlights this install has shown or skipped.
    acknowledged: Option<u64>,
    acknowledged_changed: bool,
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
            whats_new: WhatsNew::Closed,
            current: dbm_update::current_build(),
            acknowledged: None,
            acknowledged_changed: false,
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
                Event::History { result, unread } => self.history_loaded(result, unread),
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
            let result = match appimage() {
                Some(_) => dbm_update::check_platform(current, dbm_update::LINUX_APPIMAGE),
                None => dbm_update::check(current),
            };
            let _ = sender.send(Event::Checked { result, quiet });
            ctx.request_repaint();
        });
    }

    pub fn install(&mut self, ctx: &egui::Context, update: Available) {
        self.mark_reviewed(&update);
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

    /// Starts What's new for this launch from the stored acknowledgement. A
    /// first launch only records the installed build; after an update, the
    /// highlights it has not shown yet open once.
    pub fn start_whats_new(&mut self, ctx: &egui::Context, acknowledged: Option<u64>) {
        let Some(current) = self.current else { return };
        self.acknowledged = acknowledged;
        match acknowledged {
            None => self.acknowledge(current),
            Some(seen) if seen < current => self.fetch_history(ctx, true),
            Some(_) => {}
        }
    }

    /// Every highlighted release up to this build, from the Help menu.
    pub fn open_whats_new(&mut self, ctx: &egui::Context) {
        if self.current.is_none() || matches!(self.whats_new, WhatsNew::Loading) {
            return;
        }
        self.whats_new = WhatsNew::Loading;
        self.fetch_history(ctx, false);
    }

    pub fn close_whats_new(&mut self) {
        self.whats_new = WhatsNew::Closed;
        if let Some(current) = self.current {
            self.acknowledge(current);
        }
    }

    /// A newly acknowledged build to store, once per change.
    pub fn take_acknowledged(&mut self) -> Option<u64> {
        std::mem::take(&mut self.acknowledged_changed)
            .then_some(self.acknowledged)
            .flatten()
    }

    /// The review just showed these highlights, so the restarted build does
    /// not open them again.
    fn mark_reviewed(&mut self, update: &Available) {
        if update.highlights.is_some() {
            self.acknowledge(update.build);
        }
    }

    fn acknowledge(&mut self, build: u64) {
        if self.acknowledged.is_none_or(|seen| seen < build) {
            self.acknowledged = Some(build);
            self.acknowledged_changed = true;
        }
    }

    fn fetch_history(&mut self, ctx: &egui::Context, unread: bool) {
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(Event::History {
                result: dbm_update::history(),
                unread,
            });
            ctx.request_repaint();
        });
    }

    fn history_loaded(&mut self, result: Result<Vec<Release>, String>, unread: bool) {
        let Some(current) = self.current else { return };
        if unread {
            // A failed quiet fetch tries again next launch.
            let Ok(releases) = result else { return };
            let days = dbm_update::days(&releases, self.acknowledged.unwrap_or(current), current);
            if days.is_empty() {
                self.acknowledge(current);
            } else if matches!(self.whats_new, WhatsNew::Closed) {
                self.whats_new = WhatsNew::Shown(days);
            }
            return;
        }
        if matches!(self.whats_new, WhatsNew::Loading) {
            self.whats_new = match result {
                Ok(releases) => WhatsNew::Shown(dbm_update::days(&releases, 0, current)),
                Err(error) => WhatsNew::Failed(error),
            };
        }
    }

    /// The desktop `UpdateControl`'s wording.
    pub fn label(&self) -> String {
        match &self.state {
            State::Idle => String::new(),
            State::Checking => "Checking…".into(),
            State::UpToDate => "Up to date".into(),
            State::Available(update) => format!("Update to v{}", update.version),
            State::Installing => "Installing…".into(),
            State::Failed(_) => "Retry update".into(),
        }
    }

    pub fn version_label(&self) -> String {
        format!(
            "Version {}",
            dbm_update::current_version().unwrap_or_default()
        )
    }

    pub fn detail(&self) -> String {
        match &self.state {
            State::Available(update) if !update.notes.is_empty() => update.notes.clone(),
            State::Available(update) => update.version.clone(),
            State::Failed(message) => message.clone(),
            _ => format!(
                "Installed {}",
                dbm_update::current_version().unwrap_or_default()
            ),
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

/// The AppImage this process runs from. Its executable lives in a read-only
/// mount, so updates replace the image itself.
fn appimage() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

/// Puts the verified download in place of this executable (or AppImage),
/// starts it with the same arguments, and exits. The user confirmed losing
/// unsaved work.
fn replace_and_restart(download: &Path) -> Result<(), String> {
    let failed = |error: std::io::Error| format!("Couldn't install the update: {error}");
    let current = match appimage() {
        Some(image) => image,
        None => std::env::current_exe().map_err(failed)?,
    };
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
        .args(
            std::env::args_os()
                .skip(1)
                .filter(|arg| arg != "--after-update"),
        )
        .arg("--after-update")
        .spawn()
        .map_err(failed)?;
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_labels_distinguish_versions_from_status() {
        let mut updates = Updates::new();
        assert!(updates.label().is_empty());
        assert!(updates.version_label().starts_with("Version "));
        updates.state = State::UpToDate;
        assert_eq!(updates.label(), "Up to date");
        updates.state = State::Available(available(None));
        assert_eq!(updates.label(), "Update to v2026.10.08.1");
    }

    fn available(highlights: Option<Vec<dbm_update::Highlight>>) -> Available {
        Available {
            build: 2026100801,
            version: "2026.10.08.1".into(),
            notes: String::new(),
            url: String::new(),
            signature: String::new(),
            highlights,
            released: None,
            commit: None,
        }
    }

    fn release(build: u64, title: &str) -> Release {
        Release {
            build,
            version: build.to_string(),
            commit: String::new(),
            published_at: "2026-10-18T12:00:00Z".into(),
            highlights: vec![dbm_update::Highlight {
                title: title.into(),
                body: String::new(),
            }],
        }
    }

    #[test]
    fn whats_new_opens_once_for_unseen_highlights() {
        let mut updates = Updates::new();
        updates.current = Some(30);
        // First launch: record the installed build without fetching.
        updates.start_whats_new(&egui::Context::default(), None);
        assert_eq!(updates.take_acknowledged(), Some(30));
        assert_eq!(updates.take_acknowledged(), None);

        // After an update from build 10, only releases since then open.
        updates.acknowledged = Some(10);
        let releases = vec![
            release(10, "Seen"),
            release(20, "New"),
            release(40, "Later"),
        ];
        updates.history_loaded(Ok(releases.clone()), true);
        let WhatsNew::Shown(days) = &updates.whats_new else {
            panic!("expected unread highlights")
        };
        let builds: Vec<u64> = days[0].releases.iter().map(|r| r.build).collect();
        assert_eq!(builds, [20]);
        updates.close_whats_new();
        assert_eq!(updates.take_acknowledged(), Some(30));

        // Nothing unseen: stay closed and acknowledge quietly.
        updates.acknowledged = Some(20);
        updates.history_loaded(Ok(releases), true);
        assert!(matches!(updates.whats_new, WhatsNew::Closed));
        assert_eq!(updates.take_acknowledged(), Some(30));
    }

    #[test]
    fn installing_from_the_review_marks_its_highlights_read() {
        let mut updates = Updates::new();
        updates.acknowledged = Some(1);
        // Plain notes alone are not highlights the build would reopen.
        updates.mark_reviewed(&available(None));
        assert_eq!(updates.take_acknowledged(), None);
        updates.mark_reviewed(&available(Some(Vec::new())));
        assert_eq!(updates.take_acknowledged(), Some(2026100801));
        // An older build never lowers the acknowledgement.
        updates.acknowledge(5);
        assert_eq!(updates.take_acknowledged(), None);
    }
}
