use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::keyring_store::CredentialStore;
use crate::models::{ConnectionProfile, ProfileSummary, SaveProfileInput};
use crate::postgres::PgSession;
use crate::storage::LocalStore;

pub struct AppState {
    pub store: LocalStore,
    pub credentials: CredentialStore,
    pub sessions: Mutex<HashMap<Uuid, Arc<PgSession>>>,
}

impl AppState {
    pub fn new() -> AppResult<Self> {
        Ok(Self {
            store: LocalStore::new()?,
            credentials: CredentialStore,
            sessions: Mutex::new(HashMap::new()),
        })
    }

    pub async fn session(&self, profile_id: Uuid) -> AppResult<Arc<PgSession>> {
        self.sessions
            .lock()
            .await
            .get(&profile_id)
            .cloned()
            .ok_or(AppError::NotConnected)
    }

    pub fn profile(&self, profile_id: Uuid) -> AppResult<ConnectionProfile> {
        self.store
            .get_profile(profile_id)?
            .ok_or(AppError::ProfileNotFound)
    }

    pub fn profile_summaries(&self) -> AppResult<Vec<ProfileSummary>> {
        self.store
            .list_profiles()?
            .into_iter()
            .map(|profile| {
                let has_password = self
                    .credentials
                    .get_password(profile.id)
                    .unwrap_or_default()
                    .is_some();
                Ok(ProfileSummary {
                    profile,
                    has_password,
                })
            })
            .collect()
    }

    pub async fn connect(&self, profile: ConnectionProfile) -> AppResult<Arc<PgSession>> {
        let password = self.credentials.get_password(profile.id)?;
        let session = Arc::new(PgSession::connect(profile.clone(), password).await?);
        self.sessions
            .lock()
            .await
            .insert(profile.id, session.clone());
        Ok(session)
    }

    pub async fn disconnect(&self, profile_id: Uuid) {
        self.sessions.lock().await.remove(&profile_id);
    }

    pub fn save_profile(&self, input: SaveProfileInput) -> AppResult<ConnectionProfile> {
        self.store.save_profile(&input)
    }
}
