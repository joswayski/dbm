use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[cfg(not(any(target_os = "ios", target_os = "android")))]
use keyring::Entry;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[cfg(not(any(target_os = "ios", target_os = "android")))]
const SERVICE: &str = "io.github.joswayski.dbm";

#[derive(Clone)]
#[cfg_attr(not(any(target_os = "ios", target_os = "android")), derive(Default))]
pub enum CredentialStore {
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    #[default]
    Keyring,
    Memory(Arc<Mutex<HashMap<Uuid, String>>>),
}

#[cfg(any(target_os = "ios", target_os = "android"))]
impl Default for CredentialStore {
    fn default() -> Self {
        Self::in_memory()
    }
}

impl CredentialStore {
    /// Session-only credentials. Never fall back to a persistent store.
    pub fn in_memory() -> Self {
        Self::Memory(Arc::new(Mutex::new(HashMap::new())))
    }

    pub fn save_password(&self, profile_id: Uuid, password: &str) -> AppResult<()> {
        match self {
            #[cfg(not(any(target_os = "ios", target_os = "android")))]
            Self::Keyring => entry_for(profile_id)
                .ok_or_else(|| AppError::Credential(NO_STORE.into()))?
                .set_password(password)
                .map_err(AppError::from),
            Self::Memory(passwords) => {
                passwords
                    .lock()
                    .map_err(|_| unavailable())?
                    .insert(profile_id, password.to_owned());
                Ok(())
            }
        }
    }

    pub fn get_password(&self, profile_id: Uuid) -> AppResult<Option<String>> {
        match self {
            #[cfg(not(any(target_os = "ios", target_os = "android")))]
            Self::Keyring => {
                // Nothing can have been saved without a store, and connections
                // that need no password must still work on such a machine.
                let Some(entry) = entry_for(profile_id) else {
                    return Ok(None);
                };
                match entry.get_password() {
                    Ok(password) => Ok(Some(password)),
                    Err(keyring::Error::NoEntry) => Ok(None),
                    Err(error) => Err(AppError::from(error)),
                }
            }
            Self::Memory(passwords) => Ok(passwords
                .lock()
                .map_err(|_| unavailable())?
                .get(&profile_id)
                .cloned()),
        }
    }

    pub fn delete_password(&self, profile_id: Uuid) -> AppResult<()> {
        match self {
            #[cfg(not(any(target_os = "ios", target_os = "android")))]
            Self::Keyring => {
                let Some(entry) = entry_for(profile_id) else {
                    return Ok(());
                };
                match entry.delete_credential() {
                    Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                    Err(error) => Err(AppError::from(error)),
                }
            }
            Self::Memory(passwords) => {
                passwords
                    .lock()
                    .map_err(|_| unavailable())?
                    .remove(&profile_id);
                Ok(())
            }
        }
    }
}

fn unavailable() -> AppError {
    AppError::Credential("session credential store is unavailable".into())
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
const NO_STORE: &str = "no system keyring is running, so the password cannot be saved. \
     Start a Secret Service provider (GNOME Keyring, KWallet, or KeePassXC), then restart Anybase";
#[cfg(any(target_os = "macos", target_os = "windows"))]
const NO_STORE: &str =
    "the system credential store is unavailable, so the password cannot be saved";

/// `None` when the platform store cannot be opened, such as a Linux session
/// with no Secret Service. The keyring crate opens it once per process, so it
/// stays unavailable until a restart.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn entry_for(profile_id: Uuid) -> Option<Entry> {
    Entry::new(SERVICE, &format!("profile-{profile_id}"))
        .inspect_err(|error| tracing::warn!(%error, "system credential store is unavailable"))
        .ok()
}
