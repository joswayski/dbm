use std::collections::HashMap;
use std::pin::pin;
use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};
use crate::export::RowSink;
use crate::models::{
    ConnectionProfile, DatabaseEngine, DatabaseRef, FilterCondition, FilterOperator, MutationBatch,
    MutationResult, OrderSpec, QueryColumn, QueryResponse, RowMutation, SchemaNode, TableColumn,
    TableMetadata, TablePage, TablePageRequest, TlsMode, escape_like, json_integer,
    reenables_writes, sql_statement_words,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use futures_util::TryStreamExt;
use serde_json::Value;
use tokio_postgres::config::SslMode;
use tokio_postgres::error::SqlState;
use tokio_postgres::types::{FromSql, Type};
use tokio_postgres::{Client, Config, NoTls, Row, SimpleQueryMessage};

const MAX_PAGE_SIZE: u32 = 1_000;
const DEFAULT_QUERY_ROWS: u32 = 10_000;

#[derive(Debug)]
pub struct PgSession {
    profile: ConnectionProfile,
    client: Client,
}

impl PgSession {
    pub async fn connect(profile: ConnectionProfile, password: Option<String>) -> AppResult<Self> {
        if profile.ssh.is_some() {
            return Err(AppError::Unsupported(
                "SSH tunneling is not supported for this connection".into(),
            ));
        }
        let client = connect_client(&profile, password.as_deref()).await?;
        if profile.read_only {
            // Let the server reject writes the statement-keyword check cannot see,
            // such as data-modifying CTEs, COPY, or functions with side effects.
            // Without it the profile would not really be read-only, so refuse.
            client
                .batch_execute("SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY")
                .await
                .map_err(|error| {
                    AppError::Database(format!(
                        "could not make the read-only session read-only on the server: {}",
                        database_message(error)
                    ))
                })?;
        }
        Ok(Self { profile, client })
    }

    pub fn profile(&self) -> &ConnectionProfile {
        &self.profile
    }

    pub async fn list_databases(&self) -> AppResult<Vec<DatabaseRef>> {
        let rows = self
            .client
            .query(
                "SELECT datname, datistemplate, datallowconn
                 FROM pg_database ORDER BY datname",
                &[],
            )
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(DatabaseRef {
                    name: row.try_get(0)?,
                    is_template: row.try_get(1)?,
                    is_connectable: row.try_get(2)?,
                })
            })
            .collect()
    }

    pub async fn schema_tree(&self) -> AppResult<Vec<SchemaNode>> {
        // Both catalog queries are pipelined on the connection.
        let (schema_rows, table_rows) = futures_util::future::try_join(
            self.client.query(
                "SELECT schema_name
                 FROM information_schema.schemata
                 WHERE schema_name NOT LIKE 'pg_%'
                   AND schema_name <> 'information_schema'
                 ORDER BY schema_name",
                &[],
            ),
            self.client.query(
                "SELECT table_schema, table_name, table_type
                 FROM information_schema.tables
                 WHERE table_schema NOT LIKE 'pg_%'
                   AND table_schema <> 'information_schema'
                 ORDER BY table_schema, table_name",
                &[],
            ),
        )
        .await?;

        let mut schemas = schema_rows
            .into_iter()
            .map(|row| {
                let name: String = row.try_get(0)?;
                Ok(SchemaNode {
                    name: name.clone(),
                    kind: "schema".into(),
                    schema: Some(name),
                    table: None,
                    children: Vec::new(),
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        let positions = schemas
            .iter()
            .enumerate()
            .map(|(index, node)| (node.name.clone(), index))
            .collect::<HashMap<_, _>>();
        for row in table_rows {
            let schema: String = row.try_get(0)?;
            let table: String = row.try_get(1)?;
            let table_type: String = row.try_get(2)?;
            if let Some(&index) = positions.get(&schema) {
                schemas[index].children.push(SchemaNode {
                    name: table.clone(),
                    kind: if table_type == "VIEW" {
                        "view".into()
                    } else {
                        "table".into()
                    },
                    schema: Some(schema),
                    table: Some(table),
                    children: Vec::new(),
                });
            }
        }
        Ok(schemas)
    }

    pub async fn table_metadata(&self, schema: &str, table: &str) -> AppResult<TableMetadata> {
        // One round trip: the columns, plus the relation kind and primary key
        // computed once as uncorrelated subqueries.
        let rows = self
            .client
            .query_typed(
                "WITH rel AS (
                     SELECT c.oid, c.relkind
                     FROM pg_class c
                     JOIN pg_namespace n ON n.oid = c.relnamespace
                     WHERE n.nspname = $1 AND c.relname = $2
                 ),
                 pk AS (
                     SELECT array_agg(a.attname::text ORDER BY a.attnum) AS columns
                     FROM rel
                     JOIN pg_index i ON i.indrelid = rel.oid AND i.indisprimary
                     JOIN pg_attribute a ON a.attrelid = rel.oid AND a.attnum = ANY(i.indkey)
                 )
                 SELECT ordinal_position, column_name, data_type, is_nullable, column_default,
                        (SELECT relkind FROM rel), (SELECT columns FROM pk)
                 FROM information_schema.columns
                 WHERE table_schema = $1 AND table_name = $2
                 ORDER BY ordinal_position",
                &[(&schema, Type::NAME), (&table, Type::NAME)],
            )
            .await?;
        let Some(first) = rows.first() else {
            return Err(AppError::InvalidInput(format!(
                "table {schema}.{table} was not found"
            )));
        };
        // Views and foreign tables have no system columns, so selecting xmin from
        // them fails. Only heap-backed relations get optimistic xmin checks.
        let relkind: Option<i8> = first.try_get(5)?;
        let has_xmin = relkind.is_some_and(|kind| matches!(kind as u8, b'r' | b'p' | b'm'));
        let primary_key = first
            .try_get::<_, Option<Vec<String>>>(6)?
            .unwrap_or_default();
        let columns = rows
            .into_iter()
            .map(|row| {
                Ok(TableColumn {
                    ordinal: row.try_get::<_, i32>(0)?,
                    name: row.try_get(1)?,
                    data_type: row.try_get(2)?,
                    nullable: row.try_get::<_, String>(3)? == "YES",
                    default_value: row.try_get(4)?,
                })
            })
            .collect::<Result<Vec<_>, tokio_postgres::Error>>()?;
        Ok(TableMetadata {
            schema: schema.to_owned(),
            table: table.to_owned(),
            columns,
            primary_key,
            has_xmin,
        })
    }

    pub async fn table_page(&self, request: &TablePageRequest) -> AppResult<TablePage> {
        let metadata = self.table_metadata(&request.schema, &request.table).await?;
        let limit = request.limit.clamp(1, MAX_PAGE_SIZE);
        let offset = request.offset;
        let table_name = qualified_name(&request.schema, &request.table)?;
        let predicate = build_predicate(&metadata, &request.filters)?;
        let order_by = build_order_by(&metadata, request.order_by.as_ref())?;
        let page_sql = |columns: Vec<String>| {
            let mut columns = columns;
            if metadata.has_xmin {
                columns.push("xmin::text AS \"__dbm_xmin\"".to_owned());
            }
            format!(
                "SELECT {} FROM {table_name}{predicate}{order_by} LIMIT {} OFFSET {}",
                columns.join(", "),
                limit + 1,
                offset
            )
        };
        let quoted_columns = metadata
            .columns
            .iter()
            .map(|column| quote_identifier(&column.name))
            .collect::<Vec<_>>();
        // Preparing (without executing) reveals the result column types.
        let statement = self
            .client
            .prepare(&page_sql(quoted_columns.clone()))
            .await?;
        let column_types = statement
            .columns()
            .iter()
            .map(|column| column.type_().clone())
            .collect::<Vec<_>>();
        let page = async {
            if column_types.iter().all(decodes_natively) {
                self.client.query(&statement, &[]).await
            } else {
                let columns = text_cast_columns(quoted_columns, &column_types);
                // Unprepared, in one round trip.
                self.client.query_typed(&page_sql(columns), &[]).await
            }
        };
        let count = async {
            if !request.include_total.unwrap_or(true) {
                return None;
            }
            let count_sql = format!("SELECT COUNT(*)::bigint FROM {table_name}{predicate}");
            self.client
                .query_typed(&count_sql, &[])
                .await
                .ok()
                .and_then(|rows| rows.first()?.try_get::<_, i64>(0).ok())
                .and_then(|count| u64::try_from(count).ok())
        };
        // The page and the count are pipelined on the connection.
        let (rows, total_rows) = futures_util::future::join(page, count).await;
        let rows = rows?;
        let has_more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
        let rows = rows
            .into_iter()
            .take(usize::try_from(limit).unwrap_or_default())
            .map(|row| {
                (0..row.len())
                    .map(|index| value_from_row(&row, index))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let columns = metadata
            .columns
            .iter()
            .map(|column| column.name.clone())
            .chain(metadata.has_xmin.then(|| "__dbm_xmin".into()))
            .collect();
        Ok(TablePage {
            metadata,
            columns,
            rows,
            total_rows,
            offset,
            limit,
            has_more,
        })
    }

    /// Streams every row matching `request`'s filters and order (its offset
    /// and limit are ignored) to `sink`, from one statement. Unlike paging,
    /// this sees one consistent snapshot and needs no stable order, so tables
    /// without a primary key export each row exactly once.
    pub async fn export_rows(
        &self,
        request: &TablePageRequest,
        sink: &mut RowSink<'_>,
    ) -> AppResult<u64> {
        let metadata = self.table_metadata(&request.schema, &request.table).await?;
        let table_name = qualified_name(&request.schema, &request.table)?;
        let predicate = build_predicate(&metadata, &request.filters)?;
        let order_by = build_order_by(&metadata, request.order_by.as_ref())?;
        let select = |columns: Vec<String>| {
            format!(
                "SELECT {} FROM {table_name}{predicate}{order_by}",
                columns.join(", ")
            )
        };
        let quoted_columns = metadata
            .columns
            .iter()
            .map(|column| quote_identifier(&column.name))
            .collect::<Vec<_>>();
        let column_types = self
            .client
            .prepare(&select(quoted_columns.clone()))
            .await?
            .columns()
            .iter()
            .map(|column| column.type_().clone())
            .collect::<Vec<_>>();
        let sql = select(text_cast_columns(quoted_columns, &column_types));
        let no_params = std::iter::empty::<(&(dyn tokio_postgres::types::ToSql + Sync), Type)>();
        let mut rows = pin!(self.client.query_typed_raw(&sql, no_params).await?);
        let mut written = 0;
        while let Some(row) = rows.try_next().await? {
            sink(
                (0..row.len())
                    .map(|index| value_from_row(&row, index))
                    .collect(),
            )?;
            written += 1;
        }
        Ok(written)
    }

    pub async fn run_query(&self, sql: &str, max_rows: Option<u32>) -> AppResult<QueryResponse> {
        let sql = sql.trim();
        if sql.is_empty() {
            return Err(AppError::InvalidInput("query is empty".into()));
        }
        if self.profile.read_only && is_mutating_statement(sql) {
            return Err(AppError::Unsupported("profile is read-only".into()));
        }
        let started = Instant::now();
        let max_rows = usize::try_from(
            max_rows
                .unwrap_or(DEFAULT_QUERY_ROWS)
                .clamp(1, DEFAULT_QUERY_ROWS),
        )
        .unwrap_or(usize::MAX);
        // Preparing a row-returning statement reveals its column types so text
        // results can be typed. Scripts with several statements cannot be
        // prepared; their results stay as text.
        let column_types = if returns_rows(sql) {
            match self.client.prepare(sql).await {
                Ok(statement) => Some(
                    statement
                        .columns()
                        .iter()
                        .map(|column| column.type_().clone())
                        .collect::<Vec<_>>(),
                ),
                Err(error) if is_multiple_commands_error(&error) => None,
                Err(error) => return Err(error.into()),
            }
        } else {
            None
        };
        // The simple query protocol returns every value as text, so any column
        // type can be shown, and multi-statement scripts run in one round trip.
        // Stream instead of collecting the entire response before applying the
        // row cap. Drain it fully so later commands and errors are still handled.
        let mut messages = pin!(self.client.simple_query_raw(sql).await?);
        let mut result = SimpleResult::default();
        let mut current: Option<SimpleResult> = None;
        while let Some(message) = messages.try_next().await? {
            match message {
                SimpleQueryMessage::RowDescription(description) => {
                    current = Some(SimpleResult {
                        columns: Some(
                            description
                                .iter()
                                .map(|column| column.name().to_owned())
                                .collect(),
                        ),
                        ..SimpleResult::default()
                    });
                }
                SimpleQueryMessage::Row(row) => {
                    let current = current.get_or_insert_with(SimpleResult::default);
                    if current.rows.len() < max_rows {
                        current.rows.push(
                            (0..row.len())
                                .map(|index| {
                                    let ty = column_types
                                        .as_ref()
                                        .filter(|types| types.len() == row.len())
                                        .map(|types| &types[index]);
                                    value_from_text(row.get(index), ty)
                                })
                                .collect(),
                        );
                    } else {
                        current.truncated = true;
                    }
                }
                SimpleQueryMessage::CommandComplete(count) => {
                    let mut finished = current.take().unwrap_or_default();
                    finished.command_count = Some(count);
                    result = finished;
                }
                _ => {}
            }
        }
        let duration_ms = started.elapsed().as_millis();
        let Some(column_names) = result.columns else {
            return Ok(QueryResponse {
                columns: Vec::new(),
                rows: Vec::new(),
                row_count: 0,
                affected_rows: result.command_count,
                duration_ms,
                truncated: false,
                notices: Vec::new(),
            });
        };
        let columns = column_names
            .into_iter()
            .enumerate()
            .map(|(index, name)| QueryColumn {
                name,
                data_type: column_types
                    .as_ref()
                    .and_then(|types| types.get(index))
                    .map_or_else(|| "text".to_owned(), |ty| ty.name().to_owned()),
            })
            .collect();
        Ok(QueryResponse {
            columns,
            row_count: result.rows.len(),
            rows: result.rows,
            affected_rows: None,
            duration_ms,
            truncated: result.truncated,
            notices: Vec::new(),
        })
    }

    pub async fn apply_mutations(&self, batch: &MutationBatch) -> AppResult<MutationResult> {
        if self.profile.read_only {
            return Err(AppError::Unsupported("profile is read-only".into()));
        }
        let metadata = self.table_metadata(&batch.schema, &batch.table).await?;
        if metadata.primary_key.is_empty() {
            return Err(AppError::Unsupported(
                "only tables with a primary key can be edited".into(),
            ));
        }
        let table_name = qualified_name(&batch.schema, &batch.table)?;
        // Validate the whole batch before writing: an invalid later key must not
        // leave earlier edits pending in this persistent workbench connection.
        let predicates = batch
            .mutations
            .iter()
            .map(|mutation| mutation_predicate(&metadata, mutation))
            .collect::<AppResult<Vec<_>>>()?;
        let scope = self.begin_grid_write().await?;
        let mut applied = 0;
        let mut conflicts = Vec::new();
        for (mutation, predicate) in batch.mutations.iter().zip(predicates) {
            let statement = if mutation.deleted {
                format!("DELETE FROM {table_name} WHERE {predicate}")
            } else {
                let assignments = metadata
                    .columns
                    .iter()
                    .enumerate()
                    .filter_map(|(index, column)| {
                        if metadata.primary_key.iter().any(|key| key == &column.name)
                            || mutation.original.get(index) == mutation.changes.get(index)
                        {
                            None
                        } else {
                            mutation.changes.get(index).map(|value| {
                                format!(
                                    "{} = {}",
                                    quote_identifier(&column.name),
                                    json_to_sql(value)
                                )
                            })
                        }
                    })
                    .collect::<Vec<_>>();
                if assignments.is_empty() {
                    continue;
                }
                format!(
                    "UPDATE {table_name} SET {} WHERE {predicate}",
                    assignments.join(", ")
                )
            };
            let changed = match self.client.execute(&statement, &[]).await {
                Ok(changed) => changed,
                Err(error) => {
                    let _ = self.client.batch_execute(scope.rollback()).await;
                    return Err(error.into());
                }
            };
            if changed == 0 {
                conflicts.push(mutation.primary_key.clone());
            } else {
                applied += 1;
            }
        }
        if let Err(error) = self.client.batch_execute(scope.commit()).await {
            let _ = self.client.batch_execute(scope.rollback()).await;
            return Err(error.into());
        }
        Ok(MutationResult { applied, conflicts })
    }

    /// Grid writes share the SQL workbench connection, where the user may have
    /// an open transaction. BEGIN/COMMIT there would commit (or roll back) the
    /// user's own work, so inside a transaction the batch runs in a savepoint
    /// and becomes permanent when the user's transaction commits. A statement
    /// outside a transaction block starts its own, so its timestamp equals the
    /// transaction's; probing this way leaves no errors in the server log.
    async fn begin_grid_write(&self) -> AppResult<GridWriteScope> {
        // A single simple-protocol message, so outside a block the statement
        // and its implicit transaction start together. (Extended-protocol
        // messages each restart the statement clock.) Fails with 25P02 if the
        // user's transaction already failed.
        let probe = self
            .client
            .simple_query("SELECT transaction_timestamp() <> statement_timestamp()")
            .await?;
        let in_transaction = probe.iter().any(|message| {
            matches!(message, tokio_postgres::SimpleQueryMessage::Row(row) if row.get(0) == Some("t"))
        });
        if in_transaction {
            self.client
                .batch_execute("SAVEPOINT dbm_grid_write")
                .await?;
            Ok(GridWriteScope::Savepoint)
        } else {
            self.client.batch_execute("BEGIN").await?;
            Ok(GridWriteScope::Transaction)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GridWriteScope {
    /// No transaction was open; the batch has its own.
    Transaction,
    /// The batch is nested in the user's open workbench transaction.
    Savepoint,
}

impl GridWriteScope {
    fn commit(self) -> &'static str {
        match self {
            Self::Transaction => "COMMIT",
            Self::Savepoint => "RELEASE SAVEPOINT dbm_grid_write",
        }
    }

    fn rollback(self) -> &'static str {
        match self {
            Self::Transaction => "ROLLBACK",
            // Undo only the batch and leave the user's transaction usable.
            Self::Savepoint => {
                "ROLLBACK TO SAVEPOINT dbm_grid_write; RELEASE SAVEPOINT dbm_grid_write"
            }
        }
    }
}

#[derive(Default)]
struct SimpleResult {
    columns: Option<Vec<String>>,
    rows: Vec<Vec<Value>>,
    truncated: bool,
    command_count: Option<u64>,
}

fn returns_rows(sql: &str) -> bool {
    let keyword = sql
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        keyword.as_str(),
        "select" | "with" | "show" | "values" | "explain" | "table"
    ) || sql.to_ascii_lowercase().contains("returning")
}

/// Scripts with several statements cannot be prepared; the server reports
/// SQLSTATE 42601 ("cannot insert multiple commands into a prepared
/// statement", translated per server locale). Genuine syntax errors share the
/// code, but the simple protocol parses the whole script before running any of
/// it, so they are reported the same way without executing anything.
fn is_multiple_commands_error(error: &tokio_postgres::Error) -> bool {
    error.code() == Some(&SqlState::SYNTAX_ERROR)
}

/// Types `value_from_row` decodes from the binary protocol. Everything else is
/// selected as text.
fn decodes_natively(ty: &Type) -> bool {
    [
        Type::BOOL,
        Type::INT2,
        Type::INT4,
        Type::INT8,
        Type::OID,
        Type::FLOAT4,
        Type::FLOAT8,
        Type::JSON,
        Type::JSONB,
        Type::DATE,
        Type::TIME,
        Type::TIMESTAMP,
        Type::TIMESTAMPTZ,
    ]
    .contains(ty)
        || <String as FromSql>::accepts(ty)
}

fn value_from_text(text: Option<&str>, ty: Option<&Type>) -> Value {
    let Some(text) = text else {
        return Value::Null;
    };
    let text_value = || Value::String(text.to_owned());
    match ty {
        Some(ty) if *ty == Type::BOOL => Value::Bool(text == "t"),
        Some(ty) if [Type::INT2, Type::INT4, Type::INT8, Type::OID].contains(ty) => text
            .parse::<i64>()
            .map_or_else(|_| text_value(), json_integer),
        Some(ty) if [Type::FLOAT4, Type::FLOAT8].contains(ty) => text
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map_or_else(text_value, Value::Number),
        Some(ty) if [Type::JSON, Type::JSONB].contains(ty) => {
            serde_json::from_str(text).unwrap_or_else(|_| text_value())
        }
        _ => text_value(),
    }
}

async fn connect_client(profile: &ConnectionProfile, password: Option<&str>) -> AppResult<Client> {
    if matches!(profile.tls_mode, TlsMode::Disabled) {
        return connect_without_tls(profile, password).await;
    }
    let mut connector = native_tls::TlsConnector::builder();
    if let Some(path) = profile.ca_cert_path.as_deref() {
        let pem = std::fs::read(path).map_err(|error| {
            AppError::InvalidInput(format!("could not read CA certificate {path}: {error}"))
        })?;
        let certificates = pem::parse_many(&pem).map_err(|error| {
            AppError::InvalidInput(format!("could not parse CA certificate {path}: {error}"))
        })?;
        if certificates.is_empty() {
            return Err(AppError::InvalidInput(
                "CA bundle contains no certificates".into(),
            ));
        }
        for certificate in certificates {
            if certificate.tag() != "CERTIFICATE" {
                return Err(AppError::InvalidInput(
                    "CA bundle must contain certificates only".into(),
                ));
            }
            connector.add_root_certificate(
                native_tls::Certificate::from_der(certificate.contents()).map_err(|error| {
                    AppError::InvalidInput(format!("could not parse CA certificate: {error}"))
                })?,
            );
        }
    } else if matches!(profile.tls_mode, TlsMode::Preferred) {
        // Preferred TLS falls back to plaintext anyway, so like libpq's
        // sslmode=prefer (and MySQL and Redis here) it encrypts without
        // verifying the server. Required, or a configured CA, verifies.
        connector
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true);
    }
    let connector = connector
        .build()
        .map_err(|error| AppError::database(&error))?;
    let make_connector = postgres_native_tls::MakeTlsConnector::new(connector);
    match base_config(profile, password).connect(make_connector).await {
        Ok((client, connection)) => {
            spawn_connection(connection);
            Ok(client)
        }
        Err(tls_error)
            if matches!(profile.tls_mode, TlsMode::Preferred)
                && plaintext_fallback_may_help(&tls_error) =>
        {
            let mut config = base_config(profile, password);
            config.ssl_mode(SslMode::Disable);
            match config.connect(NoTls).await {
                Ok((client, connection)) => {
                    spawn_connection(connection);
                    Ok(client)
                }
                // A server answer (bad password, unknown database, pg_hba
                // rejecting plaintext) explains the failure on its own.
                Err(plain_error) if plain_error.as_db_error().is_some() => Err(plain_error.into()),
                Err(plain_error) => Err(AppError::Database(format!(
                    "{}; plaintext fallback also failed: {}",
                    database_message(tls_error),
                    database_message(plain_error)
                ))),
            }
        }
        Err(error) => Err(AppError::from(error)),
    }
}

fn database_message(error: tokio_postgres::Error) -> String {
    match AppError::from(error) {
        AppError::Database(message) => message,
        other => other.to_string(),
    }
}

/// Whether a failed TLS attempt for a Preferred profile is worth retrying
/// without TLS. Server answers such as authentication failures carry a
/// SQLSTATE and would only fail again, and an unreachable or refusing host
/// would double the wait. TLS handshake failures, and servers that drop the
/// connection during TLS negotiation, may still accept plaintext.
fn plaintext_fallback_may_help(error: &tokio_postgres::Error) -> bool {
    if error.as_db_error().is_some() {
        return false;
    }
    let mut source = std::error::Error::source(error);
    while let Some(current) = source {
        if current.downcast_ref::<native_tls::Error>().is_some() {
            return true;
        }
        if let Some(io) = current.downcast_ref::<std::io::Error>() {
            return matches!(
                io.kind(),
                std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
            );
        }
        source = current.source();
    }
    false
}

fn spawn_connection<S, T>(connection: tokio_postgres::Connection<S, T>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            tracing::error!(%error, "postgres connection closed");
        }
    });
}

async fn connect_without_tls(
    profile: &ConnectionProfile,
    password: Option<&str>,
) -> AppResult<Client> {
    let (client, connection) = base_config(profile, password).connect(NoTls).await?;
    spawn_connection(connection);
    Ok(client)
}

fn base_config(profile: &ConnectionProfile, password: Option<&str>) -> Config {
    let mut config = Config::new();
    config
        .host(&profile.host)
        .port(profile.port)
        .user(&profile.username)
        .dbname(&profile.default_database)
        .application_name("Anybase")
        .connect_timeout(Duration::from_secs(10))
        .keepalives(true)
        .keepalives_idle(Duration::from_secs(60))
        // tokio-postgres defaults to `prefer`, which silently continues in
        // plaintext when the server, or anyone in between, declines TLS.
        .ssl_mode(match profile.tls_mode {
            TlsMode::Disabled => SslMode::Disable,
            TlsMode::Preferred => SslMode::Prefer,
            TlsMode::Required => SslMode::Require,
        });
    if let Some(password) = password {
        config.password(password);
    }
    config
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// Whether a read-only profile must refuse `sql`: any statement starts with a
/// writing keyword or would switch the session back to read-write. Comments
/// and quoted text are ignored. The server-side read-only session still
/// rejects writes this cannot recognize.
fn is_mutating_statement(sql: &str) -> bool {
    sql_statement_words(DatabaseEngine::Postgres, sql)
        .iter()
        .any(|words| {
            reenables_writes(words)
                || words.first().is_some_and(|word| {
                    matches!(
                        word.as_str(),
                        "insert"
                            | "update"
                            | "delete"
                            | "alter"
                            | "drop"
                            | "truncate"
                            | "create"
                            | "grant"
                            | "revoke"
                    )
                })
        })
}

fn qualified_name(schema: &str, table: &str) -> AppResult<String> {
    if schema.contains('\0') || table.contains('\0') {
        return Err(AppError::InvalidInput(
            "identifier contains a NUL byte".into(),
        ));
    }
    Ok(format!(
        "{}.{}",
        quote_identifier(schema),
        quote_identifier(table)
    ))
}

fn build_predicate(metadata: &TableMetadata, filters: &[FilterCondition]) -> AppResult<String> {
    if filters.is_empty() {
        return Ok(String::new());
    }
    let mut clauses = Vec::with_capacity(filters.len());
    for filter in filters {
        if !metadata
            .columns
            .iter()
            .any(|column| column.name == filter.column)
        {
            return Err(AppError::InvalidInput(format!(
                "unknown filter column {}",
                filter.column
            )));
        }
        let column = quote_identifier(&filter.column);
        let text_column = format!("{column}::text");
        let value = filter.value.as_deref().unwrap_or_default();
        let clause = match filter.operator {
            FilterOperator::Equals => {
                format!("{column} IS NOT DISTINCT FROM {}", quote_literal(value))
            }
            FilterOperator::NotEquals => {
                format!("{column} IS DISTINCT FROM {}", quote_literal(value))
            }
            FilterOperator::Contains => format!(
                "{text_column} ILIKE {} ESCAPE '!'",
                quote_literal(&format!("%{}%", escape_like(value)))
            ),
            FilterOperator::StartsWith => format!(
                "{text_column} ILIKE {} ESCAPE '!'",
                quote_literal(&format!("{}%", escape_like(value)))
            ),
            FilterOperator::EndsWith => format!(
                "{text_column} ILIKE {} ESCAPE '!'",
                quote_literal(&format!("%{}", escape_like(value)))
            ),
            FilterOperator::GreaterThan => format!("{column} > {}", quote_literal(value)),
            FilterOperator::GreaterThanOrEqual => {
                format!("{column} >= {}", quote_literal(value))
            }
            FilterOperator::LessThan => format!("{column} < {}", quote_literal(value)),
            FilterOperator::LessThanOrEqual => {
                format!("{column} <= {}", quote_literal(value))
            }
            FilterOperator::In => format!("{column} IN ({})", filter_list(value)?),
            FilterOperator::NotIn => format!("{column} NOT IN ({})", filter_list(value)?),
            FilterOperator::IsNull => format!("{column} IS NULL"),
            FilterOperator::IsNotNull => format!("{column} IS NOT NULL"),
        };
        clauses.push(clause);
    }
    Ok(format!(" WHERE {}", clauses.join(" AND ")))
}

fn build_order_by(metadata: &TableMetadata, order_by: Option<&OrderSpec>) -> AppResult<String> {
    let mut clauses = Vec::new();
    if let Some(order_by) = order_by {
        if !metadata
            .columns
            .iter()
            .any(|column| column.name == order_by.column)
        {
            return Err(AppError::InvalidInput(format!(
                "unknown order column {}",
                order_by.column
            )));
        }
        clauses.push(format!(
            "{} {}",
            quote_identifier(&order_by.column),
            if order_by.descending { "DESC" } else { "ASC" }
        ));
    }
    clauses.extend(
        metadata
            .primary_key
            .iter()
            .filter(|column| match order_by {
                Some(order) => order.column.as_str() != column.as_str(),
                None => true,
            })
            .map(|column| format!("{} ASC", quote_identifier(column))),
    );
    if clauses.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!(" ORDER BY {}", clauses.join(", ")))
    }
}

fn filter_list(value: &str) -> AppResult<String> {
    let values = value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(quote_literal)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Err(AppError::InvalidInput(
            "IN filters require one or more comma-separated values".into(),
        ));
    }
    Ok(values.join(", "))
}

fn mutation_predicate(metadata: &TableMetadata, mutation: &RowMutation) -> AppResult<String> {
    if mutation.primary_key.len() != metadata.primary_key.len() {
        return Err(AppError::InvalidInput(
            "primary key values do not match table".into(),
        ));
    }
    let mut clauses = metadata
        .primary_key
        .iter()
        .zip(&mutation.primary_key)
        .map(|(column, value)| {
            format!(
                "{} IS NOT DISTINCT FROM {}",
                quote_identifier(column),
                json_to_sql(value)
            )
        })
        .collect::<Vec<_>>();
    if let Some(xmin) = mutation.xmin.as_deref() {
        clauses.push(format!("xmin::text = {}", quote_literal(xmin)));
    }
    Ok(clauses.join(" AND "))
}

fn quote_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn json_to_sql(value: &Value) -> String {
    match value {
        Value::Null => "NULL".into(),
        Value::Bool(value) => value.to_string().to_ascii_uppercase(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => quote_literal(value),
        Value::Array(_) | Value::Object(_) => quote_literal(&value.to_string()),
    }
}

/// Selects types without a native decoder (numeric, uuid, enums, arrays, ...)
/// in their text form rather than letting them show up as NULL. The alias must
/// differ from the column name: ORDER BY resolves a bare name to an output
/// column first and would otherwise sort the text.
fn text_cast_columns(quoted_columns: Vec<String>, column_types: &[Type]) -> Vec<String> {
    quoted_columns
        .into_iter()
        .zip(column_types)
        .enumerate()
        .map(|(index, (column, ty))| {
            if decodes_natively(ty) {
                column
            } else {
                format!("{column}::text AS \"__dbm_text_{index}\"")
            }
        })
        .collect()
}

fn value_from_row(row: &Row, index: usize) -> Value {
    let ty = row.columns()[index].type_();
    if *ty == Type::BOOL {
        return row
            .try_get::<_, bool>(index)
            .map(Value::Bool)
            .unwrap_or(Value::Null);
    }
    if *ty == Type::INT2 {
        return row
            .try_get::<_, i16>(index)
            .map(|value| Value::Number(serde_json::Number::from(i64::from(value))))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::INT4 {
        return row
            .try_get::<_, i32>(index)
            .map(|value| Value::Number(serde_json::Number::from(i64::from(value))))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::INT8 {
        return row
            .try_get::<_, i64>(index)
            .map(json_integer)
            .unwrap_or(Value::Null);
    }
    if *ty == Type::OID {
        return row
            .try_get::<_, u32>(index)
            .map(|value| Value::Number(serde_json::Number::from(u64::from(value))))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::FLOAT4 {
        return row
            .try_get::<_, f32>(index)
            .ok()
            .and_then(|value| serde_json::Number::from_f64(f64::from(value)))
            .map(Value::Number)
            .unwrap_or(Value::Null);
    }
    if *ty == Type::FLOAT8 {
        return row
            .try_get::<_, f64>(index)
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::Null);
    }
    if [Type::JSON, Type::JSONB].contains(ty) {
        return row.try_get::<_, Value>(index).unwrap_or(Value::Null);
    }
    if *ty == Type::DATE {
        return row
            .try_get::<_, NaiveDate>(index)
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::TIME {
        return row
            .try_get::<_, NaiveTime>(index)
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::TIMESTAMP {
        return row
            .try_get::<_, NaiveDateTime>(index)
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::TIMESTAMPTZ {
        return row
            .try_get::<_, DateTime<Utc>>(index)
            .map(|value| Value::String(value.to_rfc3339()))
            .unwrap_or(Value::Null);
    }
    row.try_get::<_, Option<String>>(index)
        .ok()
        .flatten()
        .map(Value::String)
        .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_and_literals_are_quoted() {
        assert_eq!(quote_identifier("user\"name"), "\"user\"\"name\"");
        assert_eq!(quote_literal("O'Reilly"), "'O''Reilly'");
    }

    #[test]
    fn predicates_only_accept_known_columns() {
        let metadata = TableMetadata {
            schema: "public".into(),
            table: "users".into(),
            columns: vec![TableColumn {
                name: "name".into(),
                data_type: "text".into(),
                nullable: true,
                default_value: None,
                ordinal: 1,
            }],
            primary_key: Vec::new(),
            has_xmin: true,
        };
        let result = build_predicate(
            &metadata,
            &[FilterCondition {
                column: "missing".into(),
                operator: FilterOperator::Equals,
                value: Some("x".into()),
            }],
        );
        assert!(result.is_err());
    }

    #[test]
    fn predicates_support_comparisons_and_lists() {
        let metadata = TableMetadata {
            schema: "public".into(),
            table: "users".into(),
            columns: vec![
                TableColumn {
                    name: "id".into(),
                    data_type: "integer".into(),
                    nullable: false,
                    default_value: None,
                    ordinal: 1,
                },
                TableColumn {
                    name: "status".into(),
                    data_type: "text".into(),
                    nullable: false,
                    default_value: None,
                    ordinal: 2,
                },
            ],
            primary_key: vec!["id".into()],
            has_xmin: true,
        };
        let predicate = build_predicate(
            &metadata,
            &[
                FilterCondition {
                    column: "id".into(),
                    operator: FilterOperator::GreaterThanOrEqual,
                    value: Some("10".into()),
                },
                FilterCondition {
                    column: "status".into(),
                    operator: FilterOperator::In,
                    value: Some("active, pending".into()),
                },
            ],
        )
        .expect("predicate");

        assert_eq!(
            predicate,
            " WHERE \"id\" >= '10' AND \"status\" IN ('active', 'pending')"
        );
    }

    #[test]
    fn ordering_uses_primary_keys_as_stable_tiebreakers() {
        let metadata = TableMetadata {
            schema: "public".into(),
            table: "users".into(),
            columns: vec![
                TableColumn {
                    name: "id".into(),
                    data_type: "integer".into(),
                    nullable: false,
                    default_value: None,
                    ordinal: 1,
                },
                TableColumn {
                    name: "status".into(),
                    data_type: "text".into(),
                    nullable: false,
                    default_value: None,
                    ordinal: 2,
                },
            ],
            primary_key: vec!["id".into()],
            has_xmin: true,
        };
        let order = build_order_by(
            &metadata,
            Some(&OrderSpec {
                column: "status".into(),
                descending: true,
            }),
        )
        .expect("order");

        assert_eq!(order, " ORDER BY \"status\" DESC, \"id\" ASC");
    }

    #[test]
    fn read_only_detection_covers_common_writes() {
        assert!(is_mutating_statement("DELETE FROM users"));
        assert!(is_mutating_statement(
            "ALTER TABLE users ADD COLUMN note text"
        ));
        assert!(!is_mutating_statement("SELECT * FROM users"));
        assert!(is_mutating_statement("/* note */\n  delete from users"));
        assert!(is_mutating_statement("SELECT 1; DROP TABLE users"));
        assert!(is_mutating_statement("BEGIN READ WRITE"));
        assert!(is_mutating_statement(
            "SET SESSION CHARACTERISTICS AS TRANSACTION READ WRITE"
        ));
        assert!(is_mutating_statement("RESET ALL"));
        assert!(is_mutating_statement("DISCARD ALL"));
        assert!(!is_mutating_statement(
            "SELECT 'drop'; -- delete\nSHOW search_path"
        ));
        assert!(!is_mutating_statement("BEGIN READ ONLY"));
    }

    fn test_profile(tls_mode: TlsMode) -> ConnectionProfile {
        let now = Utc::now();
        ConnectionProfile {
            id: uuid::Uuid::nil(),
            name: "test".into(),
            color: None,
            engine: crate::models::DatabaseEngine::Postgres,
            host: "db.example.com".into(),
            port: 5432,
            username: "postgres".into(),
            default_database: "postgres".into(),
            tls_mode,
            ca_cert_path: None,
            ssh: None,
            read_only: false,
            open_on_startup: false,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn ssl_mode_follows_the_profile_tls_mode() {
        for (tls_mode, ssl_mode) in [
            (TlsMode::Disabled, SslMode::Disable),
            (TlsMode::Preferred, SslMode::Prefer),
            (TlsMode::Required, SslMode::Require),
        ] {
            assert_eq!(
                base_config(&test_profile(tls_mode.clone()), None).get_ssl_mode(),
                ssl_mode,
                "{tls_mode:?}"
            );
        }
    }

    #[tokio::test]
    async fn preferred_tls_does_not_fall_back_when_the_host_is_unreachable() {
        // Nothing listens on port 1, so the attempt fails before any TLS or
        // server answer; retrying in plaintext would only double the wait.
        let mut profile = test_profile(TlsMode::Preferred);
        profile.host = "127.0.0.1".into();
        profile.port = 1;
        let Err(error) = base_config(&profile, None).connect(NoTls).await else {
            panic!("nothing listens on port 1");
        };
        assert!(!plaintext_fallback_may_help(&error), "{error}");
    }

    #[test]
    fn text_results_keep_their_types() {
        assert_eq!(
            value_from_text(Some("t"), Some(&Type::BOOL)),
            Value::Bool(true)
        );
        assert_eq!(
            value_from_text(Some("42"), Some(&Type::INT4)),
            Value::from(42)
        );
        assert_eq!(
            value_from_text(Some("9223372036854775807"), Some(&Type::INT8)),
            Value::String("9223372036854775807".into())
        );
        assert_eq!(
            value_from_text(Some("{\"a\":1}"), Some(&Type::JSONB)),
            serde_json::json!({ "a": 1 })
        );
        assert_eq!(
            value_from_text(Some("12.50"), Some(&Type::NUMERIC)),
            Value::String("12.50".into())
        );
        assert_eq!(value_from_text(None, Some(&Type::TEXT)), Value::Null);
        assert!(decodes_natively(&Type::VARCHAR));
        assert!(!decodes_natively(&Type::UUID));
        assert!(!decodes_natively(&Type::NUMERIC));
    }

    /// Live checks against a real server. Set `DBM_TEST_POSTGRES_PORT` to the port
    /// of a local PostgreSQL that trusts the `postgres` user on 127.0.0.1.
    mod live {
        use super::*;
        use crate::models::{DatabaseEngine, FilterCondition, OrderSpec};
        use uuid::Uuid;

        fn profile(read_only: bool) -> Option<ConnectionProfile> {
            let port = std::env::var("DBM_TEST_POSTGRES_PORT").ok()?.parse().ok()?;
            let now = Utc::now();
            Some(ConnectionProfile {
                id: Uuid::new_v4(),
                name: "test-postgres".into(),
                color: None,
                engine: DatabaseEngine::Postgres,
                host: "127.0.0.1".into(),
                port,
                username: "postgres".into(),
                default_database: "postgres".into(),
                tls_mode: TlsMode::Disabled,
                ca_cert_path: None,
                ssh: None,
                read_only,
                open_on_startup: true,
                created_at: now,
                updated_at: now,
            })
        }

        fn page_request(
            schema: &str,
            table: &str,
            filters: Vec<FilterCondition>,
        ) -> TablePageRequest {
            TablePageRequest {
                profile_id: Uuid::nil(),
                schema: schema.into(),
                table: table.into(),
                offset: 0,
                limit: 50,
                filters,
                order_by: Some(OrderSpec {
                    column: "price".into(),
                    descending: true,
                }),
                include_total: Some(true),
            }
        }

        #[tokio::test]
        async fn grid_writes_do_not_end_an_open_workbench_transaction() {
            let Some(profile) = profile(false) else {
                return;
            };
            let session = PgSession::connect(profile, None).await.expect("connect");
            let table = format!("dbm_test_{}", Uuid::new_v4().simple());
            session
                .client
                .batch_execute(&format!(
                    "CREATE TABLE public.{table} (id int PRIMARY KEY, label text);
                     INSERT INTO public.{table} VALUES (1, 'original'), (2, 'other')"
                ))
                .await
                .expect("fixture");
            let edit = |id: i32, label: &str| MutationBatch {
                profile_id: Uuid::nil(),
                database: session.profile().default_database.clone(),
                schema: "public".into(),
                table: table.clone(),
                mutations: vec![RowMutation {
                    original: vec![Value::from(id), Value::Null],
                    changes: vec![Value::from(id), Value::from(label)],
                    primary_key: vec![Value::from(id)],
                    xmin: None,
                    deleted: false,
                }],
            };
            let labels = || async {
                session
                    .client
                    .query(
                        &format!("SELECT label FROM public.{table} ORDER BY id"),
                        &[],
                    )
                    .await
                    .expect("labels")
                    .iter()
                    .map(|row| row.get::<_, String>(0))
                    .collect::<Vec<_>>()
            };

            session.run_query("BEGIN", None).await.expect("begin");
            session
                .run_query(
                    &format!("UPDATE public.{table} SET label = 'workbench' WHERE id = 2"),
                    None,
                )
                .await
                .expect("workbench update");
            let result = session
                .apply_mutations(&edit(1, "grid"))
                .await
                .expect("grid edit inside the open transaction");
            assert_eq!(result.applied, 1);
            // A failing grid write rolls back only its own savepoint.
            let mut failing = edit(1, "never");
            failing.mutations[0].changes[1] = Value::from("x".repeat(10));
            session
                .client
                .batch_execute(&format!(
                    "ALTER TABLE public.{table} ADD CONSTRAINT short CHECK (length(label) < 10)"
                ))
                .await
                .expect("constraint");
            session
                .apply_mutations(&failing)
                .await
                .expect_err("check constraint");
            assert_eq!(labels().await, vec!["grid", "workbench"]);
            // The user's transaction is still open and decides the outcome.
            session.run_query("ROLLBACK", None).await.expect("rollback");
            assert_eq!(labels().await, vec!["original", "other"]);

            // Without an open transaction the grid commits on its own.
            session
                .apply_mutations(&edit(1, "saved"))
                .await
                .expect("grid edit");
            session.run_query("ROLLBACK", None).await.expect("no-op");
            assert_eq!(labels().await, vec!["saved", "other"]);
            session
                .client
                .batch_execute(&format!("DROP TABLE public.{table}"))
                .await
                .expect("cleanup");
        }

        #[tokio::test]
        async fn invalid_mutation_batch_does_not_leave_partial_edits_in_a_transaction() {
            let Some(profile) = profile(false) else {
                return;
            };
            let session = PgSession::connect(profile, None).await.expect("connect");
            session
                .client
                .batch_execute(
                    "CREATE TEMP TABLE batch_validation (id int PRIMARY KEY, label text);
                 INSERT INTO batch_validation VALUES (1, 'original')",
                )
                .await
                .expect("fixture");
            let schema: String = session
                .client
                .query_one(
                    "SELECT nspname FROM pg_namespace WHERE oid = pg_my_temp_schema()",
                    &[],
                )
                .await
                .expect("temporary schema")
                .get(0);
            let batch = MutationBatch {
                profile_id: Uuid::nil(),
                database: session.profile().default_database.clone(),
                schema,
                table: "batch_validation".into(),
                mutations: vec![
                    RowMutation {
                        original: vec![Value::from(1), Value::from("original")],
                        changes: vec![Value::from(1), Value::from("edited")],
                        primary_key: vec![Value::from(1)],
                        xmin: None,
                        deleted: false,
                    },
                    RowMutation {
                        original: Vec::new(),
                        changes: Vec::new(),
                        primary_key: Vec::new(),
                        xmin: None,
                        deleted: true,
                    },
                ],
            };
            let error = session
                .apply_mutations(&batch)
                .await
                .expect_err("invalid key");
            assert!(matches!(error, AppError::InvalidInput(_)), "{error}");
            assert!(
                error
                    .to_string()
                    .contains("primary key values do not match table")
            );
            // A later console COMMIT must not persist part of a rejected batch.
            session
                .client
                .batch_execute("COMMIT")
                .await
                .expect("commit");
            let row = session
                .client
                .query_one("SELECT label FROM batch_validation", &[])
                .await
                .expect("read row");
            assert_eq!(row.get::<_, String>(0), "original");
        }

        #[tokio::test]
        async fn capped_queries_drain_scripts_and_preserve_result_boundaries() {
            let Some(profile) = profile(false) else {
                return;
            };
            let session = PgSession::connect(profile, None).await.expect("connect");
            for (count, truncated) in [(2, false), (3, false), (4, true)] {
                let response = session
                    .run_query(
                        &format!("SELECT n FROM generate_series(1, {count}) n ORDER BY n"),
                        Some(3),
                    )
                    .await
                    .expect("capped query");
                let expected = (1..=count.min(3))
                    .map(|n| vec![Value::from(n)])
                    .collect::<Vec<_>>();
                assert_eq!(response.rows, expected);
                assert_eq!(response.truncated, truncated);
            }
            let response = session
                .run_query(
                    "SELECT n FROM generate_series(1, 20000) n; SELECT 42 AS answer",
                    Some(3),
                )
                .await
                .expect("script after large result");
            assert_eq!(response.rows, vec![vec![Value::from("42")]]);
            assert_eq!(response.columns[0].name, "answer");
            assert!(!response.truncated);
            let response = session
                .run_query("SELECT 1; SELECT 42 AS empty WHERE false", Some(3))
                .await
                .expect("empty final set");
            assert_eq!(response.columns[0].name, "empty");
            assert!(response.rows.is_empty());
            assert_eq!(response.affected_rows, None);
            let response = session
                .run_query(
                    "SELECT n FROM generate_series(1, 20000) n; SET application_name = 'dbm-test'",
                    Some(3),
                )
                .await
                .expect("non-row final command");
            assert!(response.columns.is_empty());
            assert_eq!(response.affected_rows, Some(0));
            assert!(!response.truncated);
            assert!(
                session
                    .run_query(
                        "SELECT n FROM generate_series(1, 20000) n; SELECT 1 / 0",
                        Some(3),
                    )
                    .await
                    .is_err(),
                "errors after the cap must not be discarded"
            );
            let response = session
                .run_query("SELECT 7 AS recovered", Some(3))
                .await
                .expect("connection remains usable");
            assert_eq!(response.rows, vec![vec![Value::from(7)]]);
            // Syntax errors share SQLSTATE 42601 with unpreparable scripts; the
            // simple-protocol retry still reports them without running anything.
            let error = session
                .run_query("SELECT 1 AS first; SELEC 2", Some(3))
                .await
                .expect_err("syntax error");
            assert!(error.to_string().contains("SQLSTATE 42601"), "{error}");
            let error = session
                .run_query("SELECT FROM WHERE", Some(3))
                .await
                .expect_err("syntax error");
            assert!(error.to_string().contains("SQLSTATE 42601"), "{error}");
        }

        #[tokio::test]
        async fn decodes_every_column_type_and_edits_by_uuid() {
            let Some(profile) = profile(false) else {
                return;
            };
            let session = PgSession::connect(profile, None).await.expect("connect");
            let schema = format!("dbm_test_{}", Uuid::new_v4().simple());
            session
                .client
                .batch_execute(&format!(
                    "CREATE SCHEMA {schema};
                     CREATE TYPE {schema}.mood AS ENUM ('happy', 'sad');
                     CREATE TABLE {schema}.items (
                         id uuid PRIMARY KEY,
                         label text NOT NULL,
                         price numeric(10, 2),
                         mood {schema}.mood,
                         tags int[],
                         big bigint,
                         active boolean
                     );
                     INSERT INTO {schema}.items VALUES
                         ('00000000-0000-0000-0000-000000000001', '100% cotton', 12.50, 'happy', '{{1,2}}', 9007199254740993, true),
                         ('00000000-0000-0000-0000-000000000002', '100 percent', 3.00, 'sad', NULL, 7, false);
                     CREATE VIEW {schema}.item_labels AS SELECT label, price FROM {schema}.items;"
                ))
                .await
                .expect("fixture");

            let page = session
                .table_page(&page_request(&schema, "items", Vec::new()))
                .await
                .expect("table page");
            assert!(page.metadata.has_xmin);
            assert_eq!(page.metadata.primary_key, vec!["id".to_owned()]);
            assert_eq!(page.total_rows, Some(2));
            let tree = session.schema_tree().await.expect("schema tree");
            let node = tree
                .iter()
                .find(|node| node.name == schema)
                .expect("test schema in tree");
            assert_eq!(
                node.children
                    .iter()
                    .map(|child| (child.name.as_str(), child.kind.as_str()))
                    .collect::<Vec<_>>(),
                vec![("item_labels", "view"), ("items", "table")]
            );
            let first = &page.rows[0];
            assert_eq!(
                first[0],
                Value::String("00000000-0000-0000-0000-000000000001".into())
            );
            assert_eq!(first[2], Value::String("12.50".into()));
            assert_eq!(first[3], Value::String("happy".into()));
            assert_eq!(first[4], Value::String("{1,2}".into()));
            assert_eq!(first[5], Value::String("9007199254740993".into()));
            assert_eq!(first[6], Value::Bool(true));

            let view = session
                .table_page(&page_request(&schema, "item_labels", Vec::new()))
                .await
                .expect("views load without xmin");
            assert!(!view.metadata.has_xmin);
            assert_eq!(view.columns, vec!["label".to_owned(), "price".to_owned()]);

            let filtered = session
                .table_page(&page_request(
                    &schema,
                    "items",
                    vec![FilterCondition {
                        column: "label".into(),
                        operator: FilterOperator::Contains,
                        value: Some("100%".into()),
                    }],
                ))
                .await
                .expect("filtered page");
            assert_eq!(filtered.total_rows, Some(1), "% is matched literally");

            let xmin = first[7].as_str().map(ToOwned::to_owned);
            let mut changes = first[..7].to_vec();
            changes[2] = Value::String("13.75".into());
            let result = session
                .apply_mutations(&MutationBatch {
                    profile_id: Uuid::nil(),
                    database: session.profile().default_database.clone(),
                    schema: schema.clone(),
                    table: "items".into(),
                    mutations: vec![RowMutation {
                        original: first[..7].to_vec(),
                        changes,
                        primary_key: vec![first[0].clone()],
                        xmin,
                        deleted: false,
                    }],
                })
                .await
                .expect("mutation");
            assert_eq!(result.applied, 1);

            let sum = session
                .run_query(&format!("SELECT sum(price) AS total, count(*) AS n, bool_and(active) AS all_active FROM {schema}.items"), None)
                .await
                .expect("aggregate");
            assert_eq!(
                sum.rows,
                vec![vec![
                    Value::String("16.75".into()),
                    Value::from(2),
                    Value::Bool(false)
                ]]
            );
            assert_eq!(sum.columns[0].data_type, "numeric");

            let script = session
                .run_query(
                    &format!("SET search_path = {schema}; SELECT label FROM items WHERE label = 'a;b' OR price > 10"),
                    None,
                )
                .await
                .expect("script");
            assert_eq!(script.rows, vec![vec![Value::String("100% cotton".into())]]);

            let update = session
                .run_query(
                    &format!("UPDATE {schema}.items SET label = 'x;y' WHERE big = 7"),
                    None,
                )
                .await
                .expect("update with a semicolon in a literal");
            assert_eq!(update.affected_rows, Some(1));

            session
                .client
                .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
                .await
                .expect("cleanup");
        }

        #[tokio::test]
        async fn read_only_sessions_reject_hidden_writes() {
            let (Some(writer), Some(reader)) = (profile(false), profile(true)) else {
                return;
            };
            let writer = PgSession::connect(writer, None).await.expect("connect");
            let table = format!("dbm_test_{}", Uuid::new_v4().simple());
            writer
                .client
                .batch_execute(&format!(
                    "CREATE TABLE {table} (id int PRIMARY KEY); INSERT INTO {table} VALUES (1)"
                ))
                .await
                .expect("fixture");
            let reader = PgSession::connect(reader, None)
                .await
                .expect("connect read-only");
            let error = reader
                .run_query(
                    &format!("WITH gone AS (DELETE FROM {table} RETURNING id) SELECT * FROM gone"),
                    None,
                )
                .await
                .expect_err("read-only transaction rejects the CTE delete");
            assert!(error.to_string().contains("read-only"), "{error}");
            for sql in [
                "BEGIN READ WRITE",
                "SET SESSION CHARACTERISTICS AS TRANSACTION READ WRITE",
                "SET default_transaction_read_only = off",
                "RESET ALL",
                "DISCARD ALL",
            ] {
                let error = reader
                    .run_query(sql, None)
                    .await
                    .expect_err("read-only profiles cannot switch the session to read-write");
                assert!(error.to_string().contains("read-only"), "{sql}: {error}");
            }
            let mode = reader
                .run_query("SHOW default_transaction_read_only", None)
                .await
                .expect("show");
            assert_eq!(mode.rows, vec![vec![Value::String("on".into())]]);
            writer
                .client
                .batch_execute(&format!("DROP TABLE {table}"))
                .await
                .expect("cleanup");
        }
    }
}
