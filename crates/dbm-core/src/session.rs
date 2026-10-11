use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::export::{EXPORT_PAGE_SIZE, RowSink};
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
        let target = format!("“{}” ({}:{})", profile.name, profile.host, profile.port);
        tokio::time::timeout(CONNECT_TIMEOUT, Self::open(profile, password))
            .await
            .map_err(|_| {
                AppError::Database(format!(
                    "timed out after {} seconds connecting to {target}",
                    CONNECT_TIMEOUT.as_secs()
                ))
            })?
            .map_err(|error| match error {
                AppError::Database(message) => {
                    AppError::Database(format!("could not connect to {target}: {message}"))
                }
                other => other,
            })
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

    /// Sends every row matching `request`'s filters and order (its offset and
    /// limit are ignored) to `sink` and returns how many were sent. SQL engines
    /// stream one statement; Redis pages through its key-ordered views.
    pub async fn export_rows(
        &self,
        request: &TablePageRequest,
        sink: &mut RowSink<'_>,
    ) -> AppResult<u64> {
        match self {
            Self::Postgres(session) => session.export_rows(request, sink).await,
            Self::Mysql(session) => session.export_rows(request, sink).await,
            Self::Redis(session) => {
                let mut sent = 0u64;
                loop {
                    let page = session
                        .table_page(&TablePageRequest {
                            offset: u32::try_from(sent).map_err(|_| {
                                AppError::Unsupported("too many rows to export".into())
                            })?,
                            limit: EXPORT_PAGE_SIZE,
                            include_total: Some(false),
                            ..request.clone()
                        })
                        .await?;
                    let more = page.has_more && !page.rows.is_empty();
                    for row in page.rows {
                        sink(row)?;
                        sent += 1;
                    }
                    if !more {
                        return Ok(sent);
                    }
                }
            }
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
        if batch.database != self.profile().default_database {
            return Err(AppError::InvalidInput(
                "the active database changed; reload the table before saving changes".into(),
            ));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn failed_connections_identify_the_profile_and_host_without_credentials() {
        for (engine, port, database) in [
            (DatabaseEngine::Postgres, 5432, "postgres"),
            (DatabaseEngine::Mysql, 3306, "mysql"),
            (DatabaseEngine::Redis, 6379, "0"),
        ] {
            let now = chrono::Utc::now();
            let profile = ConnectionProfile {
                id: uuid::Uuid::new_v4(),
                name: "Startup connection".into(),
                color: None,
                engine,
                host: "dbm-test.invalid".into(),
                port,
                username: "private-user".into(),
                default_database: database.into(),
                tls_mode: crate::models::TlsMode::Disabled,
                ca_cert_path: None,
                ssh: None,
                read_only: true,
                open_on_startup: true,
                created_at: now,
                updated_at: now,
            };
            let error = match DbSession::connect(profile, Some("private-password".into())).await {
                Ok(_) => panic!("a reserved .invalid hostname must not connect"),
                Err(error) => error.to_string(),
            };
            assert!(
                error.starts_with(&format!(
                    "database error: could not connect to “Startup connection” (dbm-test.invalid:{port}): "
                )),
                "{engine:?}: {error}"
            );
            assert!(!error.contains("private-user"), "{error}");
            assert!(!error.contains("private-password"), "{error}");
        }
    }
}
