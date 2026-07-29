use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::{Update, UpdaterExt};

const UPDATE_EVENT: &str = "dbm:update-status-changed";
const RELEASES_URL: &str = "https://github.com/joswayski/dbm/releases/latest";
const INITIAL_CHECK_DELAY: Duration = Duration::from_secs(15);
const CHECK_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateStatus {
    Idle {
        current_version: String,
    },
    Checking {
        current_version: String,
    },
    UpToDate {
        current_version: String,
    },
    Available {
        current_version: String,
        version: String,
        notes: Option<String>,
        installable: bool,
        manual_download_url: Option<String>,
    },
    Downloading {
        current_version: String,
        version: String,
        downloaded: u64,
        total: Option<u64>,
    },
    Error {
        current_version: String,
        message: String,
    },
}

pub struct UpdateCoordinator {
    status: Mutex<UpdateStatus>,
    pending: Mutex<Option<Update>>,
    checking: AtomicBool,
    installing: AtomicBool,
}

impl Default for UpdateCoordinator {
    fn default() -> Self {
        Self {
            status: Mutex::new(UpdateStatus::Idle {
                current_version: String::new(),
            }),
            pending: Mutex::new(None),
            checking: AtomicBool::new(false),
            installing: AtomicBool::new(false),
        }
    }
}

struct AtomicFlagGuard<'a>(&'a AtomicBool);

impl<'a> AtomicFlagGuard<'a> {
    fn acquire(flag: &'a AtomicBool) -> Option<Self> {
        (!flag.swap(true, Ordering::AcqRel)).then_some(Self(flag))
    }
}

impl Drop for AtomicFlagGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub fn initialize(app: &AppHandle) {
    set_status(
        app,
        UpdateStatus::Idle {
            current_version: current_version(app),
        },
    );

    if cfg!(debug_assertions) || !official_release_build() {
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(INITIAL_CHECK_DELAY).await;
        loop {
            if let Err(error) = check_for_updates_inner(&app, false).await {
                tracing::warn!(%error, "background update check failed");
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

#[tauri::command]
pub fn get_update_status(state: tauri::State<'_, UpdateCoordinator>) -> UpdateStatus {
    state.status.lock().clone()
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<UpdateStatus, String> {
    check_for_updates_inner(&app, true).await
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let coordinator = app.state::<UpdateCoordinator>();
    let status = coordinator.status.lock().clone();
    let installable = matches!(
        status,
        UpdateStatus::Available {
            installable: true,
            ..
        } | UpdateStatus::Downloading { .. }
    );

    if !installable {
        if matches!(status, UpdateStatus::Available { .. }) {
            app.opener()
                .open_url(RELEASES_URL, None::<&str>)
                .map_err(|error| error.to_string())?;
            return Ok(());
        }
        return Err("there is no installable update available".to_owned());
    }

    let Some(_install_guard) = AtomicFlagGuard::acquire(&coordinator.installing) else {
        return Err("an update is already being installed".to_owned());
    };
    let update = coordinator
        .pending
        .lock()
        .clone()
        .ok_or_else(|| "the available update needs to be checked again".to_owned())?;
    let version = update.version.clone();
    let current_version = current_version(&app);
    set_status(
        &app,
        UpdateStatus::Downloading {
            current_version: current_version.clone(),
            version: version.clone(),
            downloaded: 0,
            total: None,
        },
    );

    let progress_app = app.clone();
    let progress_current_version = current_version.clone();
    let progress_version = version.clone();
    let mut downloaded = 0_u64;
    let result = update
        .download_and_install(
            move |chunk_length, total| {
                downloaded = downloaded.saturating_add(chunk_length as u64);
                set_status(
                    &progress_app,
                    UpdateStatus::Downloading {
                        current_version: progress_current_version.clone(),
                        version: progress_version.clone(),
                        downloaded,
                        total,
                    },
                );
            },
            || {},
        )
        .await;

    if let Err(error) = result {
        let message = format!("Could not install the update: {error}");
        set_status(
            &app,
            UpdateStatus::Error {
                current_version,
                message: message.clone(),
            },
        );
        return Err(message);
    }

    app.restart();
}

async fn check_for_updates_inner(app: &AppHandle, manual: bool) -> Result<UpdateStatus, String> {
    if !official_release_build() {
        let message = "Update checks are available only in official DBM releases.".to_owned();
        if manual {
            set_status(
                app,
                UpdateStatus::Error {
                    current_version: current_version(app),
                    message: message.clone(),
                },
            );
        }
        return Err(message);
    }

    let coordinator = app.state::<UpdateCoordinator>();
    if coordinator.installing.load(Ordering::Acquire) {
        return Ok(coordinator.status.lock().clone());
    }
    let Some(_check_guard) = AtomicFlagGuard::acquire(&coordinator.checking) else {
        return Ok(coordinator.status.lock().clone());
    };

    let current_version = current_version(app);
    if manual {
        set_status(
            app,
            UpdateStatus::Checking {
                current_version: current_version.clone(),
            },
        );
    }

    let checked = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(error) => Err(error),
    };
    let update = match checked {
        Ok(update) => update,
        Err(error) => {
            let message = error.to_string();
            if manual {
                set_status(
                    app,
                    UpdateStatus::Error {
                        current_version,
                        message: message.clone(),
                    },
                );
            }
            return Err(message);
        }
    };

    let status = if let Some(update) = update {
        let version = update.version.clone();
        let notes = update.body.clone().filter(|notes| !notes.trim().is_empty());
        let installable = platform_update_is_installable();
        *coordinator.pending.lock() = Some(update);
        UpdateStatus::Available {
            current_version,
            version,
            notes,
            installable,
            manual_download_url: (!installable).then(|| RELEASES_URL.to_owned()),
        }
    } else {
        *coordinator.pending.lock() = None;
        UpdateStatus::UpToDate { current_version }
    };
    set_status(app, status.clone());
    Ok(status)
}

fn set_status(app: &AppHandle, status: UpdateStatus) {
    *app.state::<UpdateCoordinator>().status.lock() = status.clone();
    if let Err(error) = app.emit(UPDATE_EVENT, status) {
        tracing::warn!(%error, "failed to emit update status");
    }
}

fn current_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

fn official_release_build() -> bool {
    release_channel_enabled(option_env!("DBM_OFFICIAL_RELEASE"))
}

fn release_channel_enabled(value: Option<&str>) -> bool {
    value == Some("1")
}

fn platform_update_is_installable() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("APPIMAGE").is_some()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::{AtomicFlagGuard, release_channel_enabled};

    #[test]
    fn enables_updates_only_for_official_release_builds() {
        assert!(release_channel_enabled(Some("1")));
        assert!(!release_channel_enabled(None));
        assert!(!release_channel_enabled(Some("0")));
        assert!(!release_channel_enabled(Some("true")));
    }

    #[test]
    fn suppresses_duplicate_update_operations() {
        let active = AtomicBool::new(false);
        let guard = AtomicFlagGuard::acquire(&active).expect("first operation should start");
        assert!(AtomicFlagGuard::acquire(&active).is_none());
        drop(guard);
        assert!(AtomicFlagGuard::acquire(&active).is_some());
    }
}
