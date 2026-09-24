use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};
use crate::models::{
    ConnectionProfile, DatabaseRef, FilterCondition, FilterOperator, MutationBatch, MutationResult,
    OrderSpec, QueryColumn, QueryResponse, RowMutation, SchemaNode, TableColumn, TableMetadata,
    TablePage, TablePageRequest, TlsMode, escape_like, json_integer,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde_json::Value;
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
            if let Err(error) = client
                .batch_execute("SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY")
                .await
            {
                tracing::warn!(%error, "could not make the read-only session read-only on the server");
            }
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
        let schema_rows = self
            .client
            .query(
                "SELECT schema_name
                 FROM information_schema.schemata
                 WHERE schema_name NOT LIKE 'pg_%'
                   AND schema_name <> 'information_schema'
                 ORDER BY schema_name",
                &[],
            )
            .await?;
        let table_rows = self
            .client
            .query(
                "SELECT table_schema, table_name, table_type
                 FROM information_schema.tables
                 WHERE table_schema NOT LIKE 'pg_%'
                   AND table_schema <> 'information_schema'
                 ORDER BY table_schema, table_name",
                &[],
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
        for row in table_rows {
            let schema: String = row.try_get(0)?;
            let table: String = row.try_get(1)?;
            let table_type: String = row.try_get(2)?;
            if let Some(schema_node) = schemas
                .iter_mut()
                .find(|node| node.schema.as_deref() == Some(schema.as_str()))
            {
                schema_node.children.push(SchemaNode {
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
        let rows = self
            .client
            .query(
                "SELECT ordinal_position, column_name, data_type, is_nullable, column_default
                 FROM information_schema.columns
                 WHERE table_schema = $1 AND table_name = $2
                 ORDER BY ordinal_position",
                &[&schema, &table],
            )
            .await?;
        if rows.is_empty() {
            return Err(AppError::InvalidInput(format!(
                "table {schema}.{table} was not found"
            )));
        }
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
        let primary_key = self
            .client
            .query(
                "SELECT a.attname
                 FROM pg_index i
                 JOIN pg_class c ON c.oid = i.indrelid
                 JOIN pg_namespace n ON n.oid = c.relnamespace
                 JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY(i.indkey)
                 WHERE i.indisprimary AND n.nspname = $1 AND c.relname = $2
                 ORDER BY a.attnum",
                &[&schema, &table],
            )
            .await?
            .into_iter()
            .map(|row| row.try_get(0))
            .collect::<Result<Vec<String>, _>>()?;
        // Views and foreign tables have no system columns, so selecting xmin from
        // them fails. Only heap-backed relations get optimistic xmin checks.
        let relkind: Option<i8> = self
            .client
            .query_opt(
                "SELECT c.relkind
                 FROM pg_class c
                 JOIN pg_namespace n ON n.oid = c.relnamespace
                 WHERE n.nspname = $1 AND c.relname = $2",
                &[&schema, &table],
            )
            .await?
            .map(|row| row.try_get(0))
            .transpose()?;
        let has_xmin = relkind.is_some_and(|kind| matches!(kind as u8, b'r' | b'p' | b'm'));
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
        let statement = self
            .client
            .prepare(&page_sql(quoted_columns.clone()))
            .await?;
        let column_types = statement
            .columns()
            .iter()
            .map(|column| column.type_().clone())
            .collect::<Vec<_>>();
        let rows = if column_types.iter().all(decodes_natively) {
            self.client.query(&statement, &[]).await?
        } else {
            // Types without a native decoder (numeric, uuid, enums, arrays, ...) are
            // read in their text form rather than showing up as NULL.
            // The alias must differ from the column name: ORDER BY resolves a bare
            // name to an output column first and would otherwise sort the text.
            let columns = quoted_columns
                .into_iter()
                .zip(&column_types)
                .enumerate()
                .map(|(index, (column, ty))| {
                    if decodes_natively(ty) {
                        column
                    } else {
                        format!("{column}::text AS \"__dbm_text_{index}\"")
                    }
                })
                .collect();
            self.client.query(&page_sql(columns), &[]).await?
        };
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
        let total_rows = if request.include_total.unwrap_or(true) {
            let count_sql = format!("SELECT COUNT(*)::bigint FROM {table_name}{predicate}");
            self.client
                .query_one(&count_sql, &[])
                .await
                .ok()
                .and_then(|row| row.try_get::<_, i64>(0).ok())
                .and_then(|count| u64::try_from(count).ok())
        } else {
            None
        };
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
        let messages = self.client.simple_query(sql).await?;
        let mut result = SimpleResult::default();
        let mut current: Option<SimpleResult> = None;
        for message in messages {
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
        self.client.batch_execute("BEGIN").await?;
        let mut applied = 0;
        let mut conflicts = Vec::new();
        for mutation in &batch.mutations {
            let predicate = mutation_predicate(&metadata, mutation)?;
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
                    let _ = self.client.batch_execute("ROLLBACK").await;
                    return Err(error.into());
                }
            };
            if changed == 0 {
                conflicts.push(mutation.primary_key.clone());
            } else {
                applied += 1;
            }
        }
        if let Err(error) = self.client.batch_execute("COMMIT").await {
            let _ = self.client.batch_execute("ROLLBACK").await;
            return Err(error.into());
        }
        Ok(MutationResult { applied, conflicts })
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

fn is_multiple_commands_error(error: &tokio_postgres::Error) -> bool {
    error
        .as_db_error()
        .is_some_and(|error| error.message().contains("multiple commands"))
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
        let certificate = native_tls::Certificate::from_pem(&pem).map_err(|error| {
            AppError::InvalidInput(format!("could not parse CA certificate {path}: {error}"))
        })?;
        connector.add_root_certificate(certificate);
    }
    let connector = connector
        .build()
        .map_err(|error| AppError::database(&error))?;
    let mut config = base_config(profile, password);
    let make_connector = postgres_native_tls::MakeTlsConnector::new(connector);
    match config.connect(make_connector).await {
        Ok((client, connection)) => {
            tokio::spawn(async move {
                if let Err(error) = connection.await {
                    tracing::error!(%error, "postgres connection closed");
                }
            });
            Ok(client)
        }
        Err(_error) if matches!(profile.tls_mode, TlsMode::Preferred) => {
            config = base_config(profile, password);
            let (client, connection) = config.connect(NoTls).await?;
            tokio::spawn(async move {
                if let Err(error) = connection.await {
                    tracing::error!(%error, "postgres connection closed");
                }
            });
            Ok(client)
        }
        Err(error) => Err(AppError::from(error)),
    }
}

async fn connect_without_tls(
    profile: &ConnectionProfile,
    password: Option<&str>,
) -> AppResult<Client> {
    let (client, connection) = base_config(profile, password).connect(NoTls).await?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            tracing::error!(%error, "postgres connection closed");
        }
    });
    Ok(client)
}

fn base_config(profile: &ConnectionProfile, password: Option<&str>) -> Config {
    let mut config = Config::new();
    config
        .host(&profile.host)
        .port(profile.port)
        .user(&profile.username)
        .dbname(&profile.default_database)
        .application_name("DBM")
        .connect_timeout(Duration::from_secs(10))
        .keepalives(true)
        .keepalives_idle(Duration::from_secs(60));
    if let Some(password) = password {
        config.password(password);
    }
    config
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn is_mutating_statement(sql: &str) -> bool {
    matches!(
        sql.split_whitespace()
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
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
            assert_eq!(page.total_rows, Some(2));
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
            writer
                .client
                .batch_execute(&format!("DROP TABLE {table}"))
                .await
                .expect("cleanup");
        }
    }
}
