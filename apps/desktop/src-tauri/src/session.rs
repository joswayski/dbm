use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::models::{
    ConnectionProfile, DatabaseEngine, DatabaseRef, MutationBatch, MutationResult, QueryResponse,
    SchemaNode, TablePage, TablePageRequest,
};
use crate::mysql::MysqlSession;
use crate::postgres::PgSession;
use crate::redis::RedisSession;

/// Upper bound for opening a session, including a TLS attempt and any plaintext
/// fallback, so an unreachable host fails promptly instead of waiting on the OS.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

pub enum DbSession {
    Postgres(PgSession),
    Mysql(MysqlSession),
    Redis(RedisSession),
}

impl DbSession {
    pub async fn connect(profile: ConnectionProfile, password: Option<String>) -> AppResult<Self> {
        let target = format!("{}:{}", profile.host, profile.port);
        tokio::time::timeout(CONNECT_TIMEOUT, Self::open(profile, password))
            .await
            .map_err(|_| {
                AppError::Database(format!(
                    "timed out after {} seconds connecting to {target}",
                    CONNECT_TIMEOUT.as_secs()
                ))
            })?
    }

    async fn open(profile: ConnectionProfile, password: Option<String>) -> AppResult<Self> {
        match profile.engine {
            DatabaseEngine::Postgres => {
                Ok(Self::Postgres(PgSession::connect(profile, password).await?))
            }
            DatabaseEngine::Mysql => {
                Ok(Self::Mysql(MysqlSession::connect(profile, password).await?))
            }
            DatabaseEngine::Redis => {
                Ok(Self::Redis(RedisSession::connect(profile, password).await?))
            }
        }
    }

    /// The profile this session connected with, including the database the user
    /// switched to, which may differ from the saved default.
    pub fn profile(&self) -> &ConnectionProfile {
        match self {
            Self::Postgres(session) => session.profile(),
            Self::Mysql(session) => session.profile(),
            Self::Redis(session) => session.profile(),
        }
    }

    pub async fn list_databases(&self) -> AppResult<Vec<DatabaseRef>> {
        match self {
            Self::Postgres(session) => session.list_databases().await,
            Self::Mysql(session) => session.list_databases().await,
            Self::Redis(session) => session.list_databases().await,
        }
    }

    pub async fn schema_tree(&self) -> AppResult<Vec<SchemaNode>> {
        match self {
            Self::Postgres(session) => session.schema_tree().await,
            Self::Mysql(session) => session.schema_tree().await,
            Self::Redis(session) => session.schema_tree().await,
        }
    }

    pub async fn table_page(&self, request: &TablePageRequest) -> AppResult<TablePage> {
        match self {
            Self::Postgres(session) => session.table_page(request).await,
            Self::Mysql(session) => session.table_page(request).await,
            Self::Redis(session) => session.table_page(request).await,
        }
    }

    pub async fn run_query(&self, sql: &str, max_rows: Option<u32>) -> AppResult<QueryResponse> {
        match self {
            Self::Postgres(session) => session.run_query(sql, max_rows).await,
            Self::Mysql(session) => session.run_query(sql, max_rows).await,
            Self::Redis(session) => session.run_query(sql, max_rows).await,
        }
    }

    pub async fn apply_mutations(&self, batch: &MutationBatch) -> AppResult<MutationResult> {
        match self {
            Self::Postgres(session) => session.apply_mutations(batch).await,
            Self::Mysql(session) => session.apply_mutations(batch).await,
            Self::Redis(session) => session.apply_mutations(batch).await,
        }
    }

    pub async fn close(&self) {
        if let Self::Mysql(session) = self {
            session.close().await;
        }
    }
}
