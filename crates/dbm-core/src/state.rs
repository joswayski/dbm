use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::keyring_store::CredentialStore;
use crate::models::{
    ConnectionProfile, ProfileSummary, QueryHistoryEntry, QueryRequest, QueryResponse,
    SaveProfileInput,
};
use crate::session::DbSession;
use crate::storage::LocalStore;

pub struct AppState {
    pub store: LocalStore,
    pub credentials: CredentialStore,
    pub sessions: Mutex<HashMap<Uuid, Arc<DbSession>>>,
}

impl AppState {
    pub fn new() -> AppResult<Self> {
        Ok(Self {
            store: LocalStore::new()?,
            credentials: CredentialStore,
            sessions: Mutex::new(HashMap::new()),
        })
    }

    pub async fn session(&self, profile_id: Uuid) -> AppResult<Arc<DbSession>> {
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
        Ok(self
            .store
            .list_profiles()?
            .into_iter()
            .map(|profile| ProfileSummary { profile })
            .collect())
    }

    pub async fn connect(&self, profile: ConnectionProfile) -> AppResult<Arc<DbSession>> {
        let password = self.credentials.get_password(profile.id)?;
        let session = Arc::new(DbSession::connect(profile.clone(), password).await?);
        let previous = {
            let mut sessions = self.sessions.lock().await;
            sessions.insert(profile.id, session.clone())
        };
        if let Some(previous) = previous {
            previous.close().await;
        }
        Ok(session)
    }

    pub async fn with_session_retry<T, F, Fut>(
        &self,
        profile_id: Uuid,
        operation: F,
    ) -> AppResult<T>
    where
        F: Fn(Arc<DbSession>) -> Fut,
        Fut: Future<Output = AppResult<T>>,
    {
        let session = self.session(profile_id).await?;
        match operation(session.clone()).await {
            Err(error) if error.is_connection_lost() => {
                tracing::info!(%profile_id, "database connection lost; reconnecting");
                // Reconnect with the session's own profile so a database the user
                // switched to stays selected instead of reverting to the saved default.
                let session = self.connect(session.profile().clone()).await?;
                operation(session).await
            }
            result => result,
        }
    }

    pub async fn disconnect(&self, profile_id: Uuid) {
        let previous = self.sessions.lock().await.remove(&profile_id);
        if let Some(previous) = previous {
            previous.close().await;
        }
    }

    pub fn save_profile(&self, input: SaveProfileInput) -> AppResult<ConnectionProfile> {
        self.store.save_profile(&input)
    }

    pub async fn run_query(&self, request: QueryRequest) -> AppResult<QueryResponse> {
        // History follows the active database, not the saved default.
        let database = self
            .session(request.profile_id)
            .await?
            .profile()
            .default_database
            .clone();
        let response = if is_read_only_query(&request.sql) {
            self.with_session_retry(request.profile_id, |session| {
                let request = &request;
                async move { session.run_query(&request.sql, request.max_rows).await }
            })
            .await
        } else {
            self.session(request.profile_id)
                .await?
                .run_query(&request.sql, request.max_rows)
                .await
        };
        self.store.add_history(&QueryHistoryEntry {
            id: Uuid::new_v4(),
            profile_id: request.profile_id,
            database,
            sql: request.sql,
            executed_at: chrono::Utc::now(),
            duration_ms: response.as_ref().map_or(0, |result| result.duration_ms),
            success: response.is_ok(),
        })?;
        response
    }
}

fn is_read_only_query(sql: &str) -> bool {
    matches!(
        sql.split_whitespace()
            .next()
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("select" | "show" | "describe" | "desc" | "explain")
    )
}

#[cfg(test)]
mod tests {
    use super::is_read_only_query;

    #[test]
    fn retries_only_read_only_queries() {
        assert!(is_read_only_query("SELECT * FROM users"));
        assert!(is_read_only_query("  EXPLAIN SELECT * FROM users"));
        assert!(!is_read_only_query("UPDATE users SET active = true"));
        assert!(!is_read_only_query("DELETE FROM users"));
    }
}
