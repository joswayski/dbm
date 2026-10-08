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
            credentials: CredentialStore::default(),
            sessions: Mutex::new(HashMap::new()),
        })
    }

    pub fn mobile(path: &std::path::Path) -> AppResult<Self> {
        Ok(Self {
            store: LocalStore::from_path(path)?,
            credentials: CredentialStore::in_memory(),
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

    pub fn connection_url(
        &self,
        input: &SaveProfileInput,
        include_password: bool,
    ) -> AppResult<String> {
        let password = if include_password {
            match &input.password {
                Some(password) => Some(password.clone()),
                None => input
                    .id
                    .map(|id| self.credentials.get_password(id))
                    .transpose()?
                    .flatten(),
            }
        } else {
            None
        };
        crate::connection_url::format_connection_url(input, password.as_deref())
            .map_err(AppError::InvalidInput)
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

    pub async fn connect_database(
        &self,
        profile_id: Uuid,
        database: &str,
    ) -> AppResult<Arc<DbSession>> {
        let mut profile = self.profile(profile_id)?;
        profile.default_database = database.trim().to_owned();
        profile.validate()?;
        let session = self.connect(profile).await?;
        // Only a successful switch replaces the saved connection target.
        self.store
            .remember_database(profile_id, &session.profile().default_database)?;
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
        let session = self.session(request.profile_id).await?;
        let database = session.profile().default_database.clone();
        // Even SELECT can invoke a writing function or begin a multi-statement
        // script. A lost response does not prove the server did not execute it.
        let response = session.run_query(&request.sql, request.max_rows).await;
        if response.as_ref().is_err_and(AppError::is_connection_lost) {
            // Prepare for the next explicit run, but never replay user SQL or
            // hide its original error if reconnecting also fails.
            let _ = self.connect(session.profile().clone()).await;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DatabaseEngine, TlsMode};
    use serde_json::json;

    #[tokio::test]
    async fn failed_user_queries_are_not_replayed_even_when_they_start_with_select() {
        let Ok(port) = std::env::var("DBM_TEST_POSTGRES_PORT") else {
            return;
        };
        let directory = tempfile::tempdir().unwrap();
        let state = AppState::mobile(&directory.path().join("state.sqlite3")).unwrap();
        let id = Uuid::new_v4();
        let profile = ConnectionProfile {
            id,
            name: "test-query-replay".into(),
            color: None,
            engine: DatabaseEngine::Postgres,
            host: "127.0.0.1".into(),
            port: port.parse().unwrap(),
            username: "postgres".into(),
            default_database: "postgres".into(),
            tls_mode: TlsMode::Disabled,
            ca_cert_path: None,
            ssh: None,
            read_only: false,
            open_on_startup: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let observer = DbSession::connect(profile.clone(), None).await.unwrap();
        state.connect(profile).await.unwrap();
        let sequence = format!("dbm_replay_{}", id.simple());
        observer
            .run_query(
                &format!(
                    "CREATE SEQUENCE {sequence};
             CREATE FUNCTION {sequence}_bump() RETURNS bigint LANGUAGE plpgsql AS $$
             BEGIN
                 PERFORM nextval('{sequence}');
                 RAISE EXCEPTION 'connection closed after side effect';
             END $$"
                ),
                None,
            )
            .await
            .unwrap();
        let mut counts = Vec::new();
        for prefix in ["SELECT", "EXPLAIN ANALYZE SELECT"] {
            observer
                .run_query(&format!("ALTER SEQUENCE {sequence} RESTART WITH 1"), None)
                .await
                .unwrap();
            let error = state
                .run_query(QueryRequest {
                    profile_id: id,
                    sql: format!("{prefix} {sequence}_bump()"),
                    max_rows: Some(10),
                })
                .await
                .unwrap_err();
            assert!(error.is_connection_lost(), "exercise the reconnect path");
            // Sequence increments survive failed transactions, revealing each
            // execution even though the query never returned a successful result.
            counts.push(
                observer
                    .run_query(&format!("SELECT last_value::int FROM {sequence}"), None)
                    .await
                    .unwrap()
                    .rows,
            );
        }
        observer
            .run_query(
                &format!("DROP FUNCTION {sequence}_bump(); DROP SEQUENCE {sequence}"),
                None,
            )
            .await
            .unwrap();
        assert_eq!(counts, vec![vec![vec![json!(1)]], vec![vec![json!(1)]]]);
        let history = state.store.list_history(id, "postgres", 10).unwrap();
        assert_eq!(history.len(), 2);
        assert!(history.iter().all(|entry| !entry.success));

        let pid = state
            .session(id)
            .await
            .unwrap()
            .run_query("SELECT pg_backend_pid()", None)
            .await
            .unwrap()
            .rows[0][0]
            .as_i64()
            .unwrap();
        observer
            .run_query(&format!("SELECT pg_terminate_backend({pid})"), None)
            .await
            .unwrap();
        let error = state
            .run_query(QueryRequest {
                profile_id: id,
                sql: "SELECT 41".into(),
                max_rows: Some(10),
            })
            .await
            .unwrap_err();
        assert!(error.is_connection_lost(), "{error}");
        // The failed run is not replayed, but reconnection makes the next
        // explicit run usable without requiring a schema or table refresh.
        let next = state
            .run_query(QueryRequest {
                profile_id: id,
                sql: "SELECT 73".into(),
                max_rows: Some(10),
            })
            .await
            .unwrap();
        assert_eq!(next.rows, vec![vec![json!(73)]]);
        state.disconnect(id).await;
        observer.close().await;
    }
}
