use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};
use crate::models::{
    ConnectionProfile, DatabaseRef, FilterCondition, FilterOperator, MutationBatch, MutationResult,
    OrderSpec, QueryColumn, QueryResponse, RowMutation, SchemaNode, TableColumn, TableMetadata,
    TablePage, TablePageRequest, TlsMode, escape_like, json_integer, json_unsigned,
};
use mysql_async::consts::ColumnType;
use mysql_async::prelude::Queryable;
use mysql_async::{
    Conn, Opts, OptsBuilder, Pool, PoolConstraints, PoolOpts, Row, SslOpts, TxOpts, Value,
};
use serde_json::Value as JsonValue;
use tokio::sync::Mutex;

const MAX_PAGE_SIZE: u32 = 1_000;
const DEFAULT_QUERY_ROWS: u32 = 10_000;
const SYSTEM_SCHEMAS: &[&str] = &["information_schema", "mysql", "performance_schema", "sys"];

pub struct MysqlSession {
    profile: ConnectionProfile,
    pool: Pool,
    /// The SQL workbench keeps one connection so session state such as `USE`,
    /// user variables, and explicit transactions carries across statements the
    /// way it does in a SQL console. Pooled connections are reset between uses.
    workbench: Mutex<Option<Conn>>,
}

impl MysqlSession {
    pub async fn connect(profile: ConnectionProfile, password: Option<String>) -> AppResult<Self> {
        if profile.ssh.is_some() {
            return Err(AppError::Unsupported(
                "SSH tunneling is not supported for this connection".into(),
            ));
        }
        let pool = connect_pool(&profile, password.as_deref()).await?;
        Ok(Self {
            profile,
            pool,
            workbench: Mutex::new(None),
        })
    }

    pub fn profile(&self) -> &ConnectionProfile {
        &self.profile
    }

    pub async fn close(&self) {
        // The pool only finishes disconnecting once every connection is returned.
        drop(self.workbench.lock().await.take());
        let _ = self.pool.clone().disconnect().await;
    }

    async fn conn(&self) -> AppResult<Conn> {
        self.pool.get_conn().await.map_err(AppError::from)
    }

    pub async fn list_databases(&self) -> AppResult<Vec<DatabaseRef>> {
        let mut conn = self.conn().await?;
        let rows: Vec<Row> = conn
            .query("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME")
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| string_cell(&row, 0))
            .map(|name| {
                let system = SYSTEM_SCHEMAS
                    .iter()
                    .any(|schema| schema.eq_ignore_ascii_case(&name));
                DatabaseRef {
                    name,
                    is_template: system,
                    is_connectable: true,
                }
            })
            .collect())
    }

    pub async fn schema_tree(&self) -> AppResult<Vec<SchemaNode>> {
        let mut conn = self.conn().await?;
        let database = current_database(&mut conn, &self.profile.default_database).await?;
        let rows: Vec<Row> = conn
            .exec(
                "SELECT TABLE_NAME, TABLE_TYPE
                 FROM information_schema.TABLES
                 WHERE TABLE_SCHEMA = ?
                 ORDER BY TABLE_NAME",
                (&database,),
            )
            .await?;
        let children = rows
            .into_iter()
            .filter_map(|row| {
                let table = string_cell(&row, 0)?;
                let table_type = string_cell(&row, 1).unwrap_or_default();
                Some(SchemaNode {
                    name: table.clone(),
                    kind: if table_type.eq_ignore_ascii_case("VIEW")
                        || table_type.eq_ignore_ascii_case("SYSTEM VIEW")
                    {
                        "view".into()
                    } else {
                        "table".into()
                    },
                    schema: Some(database.clone()),
                    table: Some(table),
                    children: Vec::new(),
                })
            })
            .collect();
        Ok(vec![SchemaNode {
            name: database.clone(),
            kind: "schema".into(),
            schema: Some(database),
            table: None,
            children,
        }])
    }

    pub async fn table_metadata(&self, schema: &str, table: &str) -> AppResult<TableMetadata> {
        let mut conn = self.conn().await?;
        let rows: Vec<Row> = conn
            .exec(
                "SELECT ORDINAL_POSITION, COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT
                 FROM information_schema.COLUMNS
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
                 ORDER BY ORDINAL_POSITION",
                (schema, table),
            )
            .await?;
        if rows.is_empty() {
            return Err(AppError::InvalidInput(format!(
                "table {schema}.{table} was not found"
            )));
        }
        let columns = rows
            .into_iter()
            .map(|row| TableColumn {
                ordinal: i32_cell(&row, 0).unwrap_or(0),
                name: string_cell(&row, 1).unwrap_or_default(),
                data_type: string_cell(&row, 2).unwrap_or_else(|| "text".into()),
                nullable: string_cell(&row, 3)
                    .is_some_and(|value| value.eq_ignore_ascii_case("YES")),
                default_value: string_cell(&row, 4),
            })
            .collect::<Vec<_>>();
        let primary_key = conn
            .exec::<Row, _, _>(
                "SELECT COLUMN_NAME
                 FROM information_schema.STATISTICS
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND INDEX_NAME = 'PRIMARY'
                 ORDER BY SEQ_IN_INDEX",
                (schema, table),
            )
            .await?
            .into_iter()
            .filter_map(|row| string_cell(&row, 0))
            .collect();
        Ok(TableMetadata {
            schema: schema.to_owned(),
            table: table.to_owned(),
            columns,
            primary_key,
            has_xmin: false,
        })
    }

    pub async fn table_page(&self, request: &TablePageRequest) -> AppResult<TablePage> {
        let metadata = self.table_metadata(&request.schema, &request.table).await?;
        let limit = request.limit.clamp(1, MAX_PAGE_SIZE);
        let offset = request.offset;
        let selected_columns = metadata
            .columns
            .iter()
            .map(|column| quote_identifier(&column.name))
            .collect::<Vec<_>>();
        let table_name = qualified_name(&request.schema, &request.table)?;
        let (predicate, params) = build_predicate(&metadata, &request.filters)?;
        let order_by = build_order_by(&metadata, request.order_by.as_ref())?;
        let sql = format!(
            "SELECT {} FROM {table_name}{predicate}{order_by} LIMIT {} OFFSET {}",
            selected_columns.join(", "),
            limit + 1,
            offset
        );
        let mut conn = self.conn().await?;
        let rows: Vec<Row> = conn.exec(sql, params.clone()).await?;
        let has_more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
        let rows = rows
            .into_iter()
            .take(usize::try_from(limit).unwrap_or_default())
            .map(json_row)
            .collect::<Vec<_>>();
        let total_rows = if request.include_total.unwrap_or(true) {
            let count_sql = format!("SELECT COUNT(*) FROM {table_name}{predicate}");
            conn.exec_first::<Value, _, _>(count_sql, params)
                .await
                .ok()
                .flatten()
                .and_then(value_as_u64)
        } else {
            None
        };
        let columns = metadata
            .columns
            .iter()
            .map(|column| column.name.clone())
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
        let mut workbench = self.workbench.lock().await;
        if workbench.is_none() {
            *workbench = Some(self.conn().await?);
        }
        let conn = workbench
            .as_mut()
            .expect("workbench connection was just opened");
        let outcome = run_statements(conn, sql).await;
        if outcome
            .as_ref()
            .is_err_and(|error| error.is_connection_lost())
        {
            // Open a fresh workbench connection next time instead of reusing a dead one.
            *workbench = None;
        }
        let duration_ms = started.elapsed().as_millis();
        match outcome? {
            StatementOutcome::Rows(column_meta, rows) => {
                let columns = column_meta
                    .iter()
                    .map(|column| QueryColumn {
                        name: column.name_str().into_owned(),
                        data_type: mysql_type_name(column.column_type()),
                    })
                    .collect::<Vec<_>>();
                let truncated = rows.len() > max_rows;
                let rows = rows
                    .into_iter()
                    .take(max_rows)
                    .map(json_row)
                    .collect::<Vec<_>>();
                Ok(QueryResponse {
                    columns,
                    row_count: rows.len(),
                    rows,
                    affected_rows: None,
                    duration_ms,
                    truncated,
                    notices: Vec::new(),
                })
            }
            StatementOutcome::Affected(affected_rows) => Ok(QueryResponse {
                columns: Vec::new(),
                rows: Vec::new(),
                row_count: 0,
                affected_rows: Some(affected_rows),
                duration_ms,
                truncated: false,
                notices: Vec::new(),
            }),
        }
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
        let mut conn = self.conn().await?;
        let mut tx = conn.start_transaction(TxOpts::default()).await?;
        let mut applied = 0;
        let mut conflicts = Vec::new();
        for mutation in &batch.mutations {
            let (predicate, predicate_params) = mutation_predicate(&metadata, mutation)?;
            let (statement, params) = if mutation.deleted {
                (
                    format!("DELETE FROM {table_name} WHERE {predicate}"),
                    predicate_params,
                )
            } else {
                let mut assignments = Vec::new();
                let mut params = Vec::new();
                for (index, column) in metadata.columns.iter().enumerate() {
                    if metadata.primary_key.iter().any(|key| key == &column.name)
                        || mutation.original.get(index) == mutation.changes.get(index)
                    {
                        continue;
                    }
                    if let Some(value) = mutation.changes.get(index) {
                        assignments.push(format!("{} = ?", quote_identifier(&column.name)));
                        params.push(mysql_param(value, &column.data_type));
                    }
                }
                if assignments.is_empty() {
                    continue;
                }
                params.extend(predicate_params);
                (
                    format!(
                        "UPDATE {table_name} SET {} WHERE {predicate}",
                        assignments.join(", ")
                    ),
                    params,
                )
            };
            if let Err(error) = tx.exec_drop(&statement, params).await {
                let _ = tx.rollback().await;
                return Err(error.into());
            }
            if tx.affected_rows() == 0 {
                conflicts.push(mutation.primary_key.clone());
            } else {
                applied += 1;
            }
        }
        tx.commit().await?;
        Ok(MutationResult { applied, conflicts })
    }
}

enum StatementOutcome {
    Rows(Vec<mysql_async::Column>, Vec<Row>),
    Affected(u64),
}

/// Runs one or more statements and reports the last result set, matching what a
/// SQL console shows after a script finishes.
async fn run_statements(conn: &mut Conn, sql: &str) -> AppResult<StatementOutcome> {
    let mut result = conn.query_iter(sql).await?;
    loop {
        let columns = result
            .columns()
            .filter(|columns| !columns.is_empty())
            .map(|columns| columns.to_vec());
        let affected_rows = result.affected_rows();
        let rows: Vec<Row> = result.collect().await?;
        if result.is_empty() {
            return Ok(match columns {
                Some(columns) => StatementOutcome::Rows(columns, rows),
                None => StatementOutcome::Affected(affected_rows),
            });
        }
    }
}

async fn connect_pool(profile: &ConnectionProfile, password: Option<&str>) -> AppResult<Pool> {
    if matches!(profile.tls_mode, TlsMode::Disabled) {
        let pool = Pool::new(base_opts(profile, password, None));
        probe_pool(&pool).await?;
        return Ok(pool);
    }
    let ssl_opts = Some(ssl_opts(profile)?);
    let pool = Pool::new(base_opts(profile, password, ssl_opts.clone()));
    match probe_pool(&pool).await {
        Ok(()) => Ok(pool),
        Err(_error) if matches!(profile.tls_mode, TlsMode::Preferred) => {
            let _ = pool.disconnect().await;
            let fallback = Pool::new(base_opts(profile, password, None));
            probe_pool(&fallback).await?;
            Ok(fallback)
        }
        Err(error) => {
            let _ = pool.disconnect().await;
            Err(error)
        }
    }
}

async fn probe_pool(pool: &Pool) -> AppResult<()> {
    let _conn = pool.get_conn().await?;
    Ok(())
}

fn base_opts(
    profile: &ConnectionProfile,
    password: Option<&str>,
    ssl_opts: Option<SslOpts>,
) -> Opts {
    let constraints = PoolConstraints::new(1, 4).unwrap_or_default();
    // Setup commands also run after the pool resets a connection, so a
    // read-only profile stays read-only on the server for every statement.
    let setup = if profile.read_only {
        vec!["SET SESSION TRANSACTION READ ONLY"]
    } else {
        Vec::new()
    };
    OptsBuilder::default()
        .ip_or_hostname(profile.host.clone())
        .tcp_port(profile.port)
        .user(Some(profile.username.clone()))
        .pass(password.map(ToOwned::to_owned))
        .db_name(Some(profile.default_database.clone()))
        .prefer_socket(false)
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .setup(setup)
        .pool_opts(PoolOpts::default().with_constraints(constraints))
        .ssl_opts(ssl_opts)
        .into()
}

fn ssl_opts(profile: &ConnectionProfile) -> AppResult<SslOpts> {
    let mut opts = SslOpts::default();
    if let Some(path) = profile.ca_cert_path.as_deref() {
        opts = opts.with_root_certs(vec![std::path::PathBuf::from(path).into()]);
    } else if matches!(profile.tls_mode, TlsMode::Preferred) {
        opts = opts.with_danger_accept_invalid_certs(true);
    }
    Ok(opts)
}

async fn current_database(conn: &mut Conn, fallback: &str) -> AppResult<String> {
    let value: Option<Value> = conn.query_first("SELECT DATABASE()").await?;
    Ok(value
        .and_then(value_as_string)
        .unwrap_or_else(|| fallback.to_owned()))
}

fn quote_identifier(identifier: &str) -> String {
    format!("`{}`", identifier.replace('`', "``"))
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
            | "replace"
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

fn build_predicate(
    metadata: &TableMetadata,
    filters: &[FilterCondition],
) -> AppResult<(String, Vec<Value>)> {
    if filters.is_empty() {
        return Ok((String::new(), Vec::new()));
    }
    let mut clauses = Vec::with_capacity(filters.len());
    let mut params = Vec::new();
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
        let text_column = format!("CAST({column} AS CHAR)");
        let value = filter.value.as_deref().unwrap_or_default();
        let mut bind = |value: String| {
            params.push(Value::from(value));
            "?"
        };
        let clause = match filter.operator {
            FilterOperator::Equals => format!("{column} <=> {}", bind(value.to_owned())),
            FilterOperator::NotEquals => {
                format!("NOT ({column} <=> {})", bind(value.to_owned()))
            }
            FilterOperator::Contains => format!(
                "{text_column} LIKE {} ESCAPE '!'",
                bind(format!("%{}%", escape_like(value)))
            ),
            FilterOperator::StartsWith => format!(
                "{text_column} LIKE {} ESCAPE '!'",
                bind(format!("{}%", escape_like(value)))
            ),
            FilterOperator::EndsWith => format!(
                "{text_column} LIKE {} ESCAPE '!'",
                bind(format!("%{}", escape_like(value)))
            ),
            FilterOperator::GreaterThan => format!("{column} > {}", bind(value.to_owned())),
            FilterOperator::GreaterThanOrEqual => {
                format!("{column} >= {}", bind(value.to_owned()))
            }
            FilterOperator::LessThan => format!("{column} < {}", bind(value.to_owned())),
            FilterOperator::LessThanOrEqual => {
                format!("{column} <= {}", bind(value.to_owned()))
            }
            FilterOperator::In | FilterOperator::NotIn => {
                let values = filter_list(value)?;
                let placeholders = values
                    .into_iter()
                    .map(|value| bind(value.to_owned()))
                    .collect::<Vec<_>>()
                    .join(", ");
                let negation = if matches!(filter.operator, FilterOperator::NotIn) {
                    "NOT "
                } else {
                    ""
                };
                format!("{column} {negation}IN ({placeholders})")
            }
            FilterOperator::IsNull => format!("{column} IS NULL"),
            FilterOperator::IsNotNull => format!("{column} IS NOT NULL"),
        };
        clauses.push(clause);
    }
    Ok((format!(" WHERE {}", clauses.join(" AND ")), params))
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

fn filter_list(value: &str) -> AppResult<Vec<&str>> {
    let values = value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Err(AppError::InvalidInput(
            "IN filters require one or more comma-separated values".into(),
        ));
    }
    Ok(values)
}

fn mutation_predicate(
    metadata: &TableMetadata,
    mutation: &RowMutation,
) -> AppResult<(String, Vec<Value>)> {
    if mutation.primary_key.len() != metadata.primary_key.len() {
        return Err(AppError::InvalidInput(
            "primary key values do not match table".into(),
        ));
    }
    let mut clauses = Vec::new();
    let mut params = Vec::new();
    for (column, value) in metadata.primary_key.iter().zip(&mutation.primary_key) {
        let data_type = metadata
            .columns
            .iter()
            .find(|candidate| &candidate.name == column)
            .map_or("", |candidate| candidate.data_type.as_str());
        clauses.push(format!("{} <=> ?", quote_identifier(column)));
        params.push(mysql_param(value, data_type));
    }
    Ok((clauses.join(" AND "), params))
}

/// Converts an edited or key value to a statement parameter. Integer columns
/// get integer parameters even when the UI sent a string (large IDs arrive as
/// strings), because MySQL compares an integer column with a string as a double
/// and could match the wrong row.
fn mysql_param(value: &JsonValue, data_type: &str) -> Value {
    match value {
        JsonValue::Null => Value::NULL,
        JsonValue::Bool(value) => Value::Int(i64::from(*value)),
        JsonValue::Number(number) => number
            .as_i64()
            .map(Value::Int)
            .or_else(|| number.as_u64().map(Value::UInt))
            .or_else(|| number.as_f64().map(Value::Double))
            .unwrap_or_else(|| Value::from(number.to_string())),
        JsonValue::String(text) => {
            if is_integer_type(data_type) {
                if let Ok(integer) = text.parse::<i64>() {
                    return Value::Int(integer);
                }
                if let Ok(integer) = text.parse::<u64>() {
                    return Value::UInt(integer);
                }
            }
            Value::from(text.as_str())
        }
        JsonValue::Array(_) | JsonValue::Object(_) => Value::from(value.to_string()),
    }
}

fn is_integer_type(data_type: &str) -> bool {
    let data_type = data_type.trim_start().to_ascii_lowercase();
    [
        "tinyint",
        "smallint",
        "mediumint",
        "int",
        "bigint",
        "integer",
    ]
    .iter()
    .any(|prefix| {
        data_type
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['(', ' ']))
    })
}

fn json_row(row: Row) -> Vec<JsonValue> {
    (0..row.len())
        .map(|index| {
            row.as_ref(index)
                .map(value_from_mysql)
                .unwrap_or(JsonValue::Null)
        })
        .collect()
}

fn value_from_mysql(value: &Value) -> JsonValue {
    match value {
        Value::NULL => JsonValue::Null,
        Value::Bytes(bytes) => String::from_utf8(bytes.clone())
            .map(JsonValue::String)
            .unwrap_or_else(|_| JsonValue::String(format!("\\x{}", hex_encode(bytes)))),
        Value::Int(value) => json_integer(*value),
        Value::UInt(value) => json_unsigned(*value),
        // Round-trip through the shortest f32 text so 0.1 does not become 0.10000000149.
        Value::Float(value) => {
            serde_json::Number::from_f64(value.to_string().parse().unwrap_or(f64::NAN))
                .map(JsonValue::Number)
                .unwrap_or(JsonValue::Null)
        }
        Value::Double(value) => serde_json::Number::from_f64(*value)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Value::Date(year, month, day, hour, minute, second, micros) => {
            if *hour == 0 && *minute == 0 && *second == 0 && *micros == 0 {
                JsonValue::String(format!("{year:04}-{month:02}-{day:02}"))
            } else if *micros == 0 {
                JsonValue::String(format!(
                    "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}"
                ))
            } else {
                JsonValue::String(format!(
                    "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{micros:06}"
                ))
            }
        }
        Value::Time(negative, days, hour, minute, second, micros) => {
            // MySQL TIME values span -838:59:59 to 838:59:59; show them in that form.
            let sign = if *negative { "-" } else { "" };
            let hours = u64::from(*days) * 24 + u64::from(*hour);
            if *micros == 0 {
                JsonValue::String(format!("{sign}{hours:02}:{minute:02}:{second:02}"))
            } else {
                JsonValue::String(format!(
                    "{sign}{hours:02}:{minute:02}:{second:02}.{micros:06}"
                ))
            }
        }
    }
}

fn value_as_string(value: Value) -> Option<String> {
    match value {
        Value::NULL => None,
        Value::Bytes(bytes) => String::from_utf8(bytes).ok(),
        other => Some(other.as_sql(false).trim_matches('\'').to_owned()),
    }
}

fn value_as_u64(value: Value) -> Option<u64> {
    match value {
        Value::Int(value) => u64::try_from(value).ok(),
        Value::UInt(value) => Some(value),
        Value::Bytes(bytes) => String::from_utf8(bytes).ok()?.parse().ok(),
        _ => None,
    }
}

fn string_cell(row: &Row, index: usize) -> Option<String> {
    row.as_ref(index).cloned().and_then(value_as_string)
}

fn i32_cell(row: &Row, index: usize) -> Option<i32> {
    match row.as_ref(index)? {
        Value::Int(value) => i32::try_from(*value).ok(),
        Value::UInt(value) => i32::try_from(*value).ok(),
        Value::Bytes(bytes) => String::from_utf8_lossy(bytes).parse().ok(),
        _ => None,
    }
}

fn mysql_type_name(column_type: ColumnType) -> String {
    match column_type {
        ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => "decimal".into(),
        ColumnType::MYSQL_TYPE_TINY => "tinyint".into(),
        ColumnType::MYSQL_TYPE_SHORT => "smallint".into(),
        ColumnType::MYSQL_TYPE_LONG => "int".into(),
        ColumnType::MYSQL_TYPE_FLOAT => "float".into(),
        ColumnType::MYSQL_TYPE_DOUBLE => "double".into(),
        ColumnType::MYSQL_TYPE_NULL => "null".into(),
        ColumnType::MYSQL_TYPE_TIMESTAMP | ColumnType::MYSQL_TYPE_TIMESTAMP2 => "timestamp".into(),
        ColumnType::MYSQL_TYPE_LONGLONG => "bigint".into(),
        ColumnType::MYSQL_TYPE_INT24 => "mediumint".into(),
        ColumnType::MYSQL_TYPE_DATE | ColumnType::MYSQL_TYPE_NEWDATE => "date".into(),
        ColumnType::MYSQL_TYPE_TIME | ColumnType::MYSQL_TYPE_TIME2 => "time".into(),
        ColumnType::MYSQL_TYPE_DATETIME | ColumnType::MYSQL_TYPE_DATETIME2 => "datetime".into(),
        ColumnType::MYSQL_TYPE_YEAR => "year".into(),
        ColumnType::MYSQL_TYPE_VARCHAR | ColumnType::MYSQL_TYPE_VAR_STRING => "varchar".into(),
        ColumnType::MYSQL_TYPE_BIT => "bit".into(),
        ColumnType::MYSQL_TYPE_JSON => "json".into(),
        ColumnType::MYSQL_TYPE_TINY_BLOB => "tinyblob".into(),
        ColumnType::MYSQL_TYPE_MEDIUM_BLOB => "mediumblob".into(),
        ColumnType::MYSQL_TYPE_LONG_BLOB => "longblob".into(),
        ColumnType::MYSQL_TYPE_BLOB => "blob".into(),
        ColumnType::MYSQL_TYPE_STRING => "char".into(),
        ColumnType::MYSQL_TYPE_GEOMETRY => "geometry".into(),
        other => format!("{other:?}"),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_use_mysql_quoting() {
        assert_eq!(quote_identifier("user`name"), "`user``name`");
    }

    #[test]
    fn predicates_use_null_safe_equals_and_like() {
        let metadata = TableMetadata {
            schema: "app".into(),
            table: "users".into(),
            columns: vec![
                TableColumn {
                    name: "id".into(),
                    data_type: "int".into(),
                    nullable: false,
                    default_value: None,
                    ordinal: 1,
                },
                TableColumn {
                    name: "email".into(),
                    data_type: "varchar".into(),
                    nullable: false,
                    default_value: None,
                    ordinal: 2,
                },
            ],
            primary_key: vec!["id".into()],
            has_xmin: false,
        };
        let predicate = build_predicate(
            &metadata,
            &[
                FilterCondition {
                    column: "id".into(),
                    operator: FilterOperator::Equals,
                    value: Some("10".into()),
                },
                FilterCondition {
                    column: "email".into(),
                    operator: FilterOperator::Contains,
                    value: Some("ex.com".into()),
                },
            ],
        )
        .expect("predicate");
        assert_eq!(
            predicate,
            (
                " WHERE `id` <=> ? AND CAST(`email` AS CHAR) LIKE ? ESCAPE '!'".to_owned(),
                vec![Value::from("10"), Value::from("%ex.com%")]
            )
        );
    }

    #[test]
    fn mutations_match_primary_keys_without_xmin() {
        let metadata = TableMetadata {
            schema: "app".into(),
            table: "users".into(),
            columns: vec![TableColumn {
                name: "id".into(),
                data_type: "int".into(),
                nullable: false,
                default_value: None,
                ordinal: 1,
            }],
            primary_key: vec!["id".into()],
            has_xmin: false,
        };
        let predicate = mutation_predicate(
            &metadata,
            &RowMutation {
                original: vec![JsonValue::from(1)],
                changes: vec![JsonValue::from(1)],
                primary_key: vec![JsonValue::from(1)],
                xmin: None,
                deleted: false,
            },
        )
        .expect("predicate");
        assert_eq!(predicate, ("`id` <=> ?".to_owned(), vec![Value::Int(1)]));
    }

    #[test]
    fn integer_keys_bind_as_integers_even_when_sent_as_strings() {
        assert_eq!(
            mysql_param(&JsonValue::from("1234567890123456789"), "bigint unsigned"),
            Value::Int(1_234_567_890_123_456_789)
        );
        assert_eq!(
            mysql_param(
                &JsonValue::from("18446744073709551615"),
                "bigint(20) unsigned"
            ),
            Value::UInt(u64::MAX)
        );
        assert_eq!(
            mysql_param(&JsonValue::from("007"), "varchar(10)"),
            Value::from("007")
        );
        assert_eq!(mysql_param(&JsonValue::Null, "int"), Value::NULL);
        assert!(!is_integer_type("interval"));
        assert!(is_integer_type("int(11)"));
    }

    #[test]
    fn values_with_backslashes_and_quotes_are_bound_not_inlined() {
        let metadata = TableMetadata {
            schema: "app".into(),
            table: "files".into(),
            columns: vec![TableColumn {
                name: "path".into(),
                data_type: "varchar(255)".into(),
                nullable: false,
                default_value: None,
                ordinal: 1,
            }],
            primary_key: Vec::new(),
            has_xmin: false,
        };
        let (predicate, params) = build_predicate(
            &metadata,
            &[FilterCondition {
                column: "path".into(),
                operator: FilterOperator::In,
                value: Some("C:\\temp\\, it's".into()),
            }],
        )
        .expect("predicate");
        assert_eq!(predicate, " WHERE `path` IN (?, ?)");
        assert_eq!(params, vec![Value::from("C:\\temp\\"), Value::from("it's")]);
    }

    #[test]
    fn time_values_use_total_hours() {
        assert_eq!(
            value_from_mysql(&Value::Time(true, 1, 2, 3, 4, 0)),
            JsonValue::String("-26:03:04".into())
        );
        assert_eq!(value_from_mysql(&Value::Float(0.1)), serde_json::json!(0.1));
    }

    /// Live checks against a real server. Set `DBM_TEST_MYSQL_PORT` to the port of
    /// a local MySQL or MariaDB that lets `root` in from 127.0.0.1 without a password.
    mod live {
        use super::*;
        use crate::models::DatabaseEngine;
        use chrono::Utc;
        use uuid::Uuid;

        fn profile(database: &str, read_only: bool) -> Option<ConnectionProfile> {
            let port = std::env::var("DBM_TEST_MYSQL_PORT").ok()?.parse().ok()?;
            let now = Utc::now();
            Some(ConnectionProfile {
                id: Uuid::new_v4(),
                name: "test-mysql".into(),
                color: None,
                engine: DatabaseEngine::Mysql,
                host: "127.0.0.1".into(),
                port,
                username: "root".into(),
                default_database: database.into(),
                tls_mode: TlsMode::Disabled,
                ca_cert_path: None,
                ssh: None,
                read_only,
                created_at: now,
                updated_at: now,
            })
        }

        #[tokio::test]
        async fn edits_keep_backslashes_large_ids_and_workbench_state() {
            let Some(admin) = profile("mysql", false) else {
                return;
            };
            let database = format!("dbm_test_{}", Uuid::new_v4().simple());
            let admin = MysqlSession::connect(admin, None).await.expect("connect");
            admin
                .run_query(&format!("CREATE DATABASE {database}"), None)
                .await
                .expect("create database");
            let session = MysqlSession::connect(profile(&database, false).expect("profile"), None)
                .await
                .expect("connect to test database");
            session
                .run_query(
                    "CREATE TABLE files (
                         id BIGINT UNSIGNED PRIMARY KEY,
                         path VARCHAR(255) NOT NULL,
                         elapsed TIME,
                         ratio FLOAT
                     )",
                    None,
                )
                .await
                .expect("create table");
            session
                .run_query(
                    "INSERT INTO files VALUES
                         (9007199254740993, 'C:\\\\temp\\\\', '26:03:04', 0.1),
                         (9007199254740992, 'other', '00:00:01', 1.5)",
                    None,
                )
                .await
                .expect("insert");

            let request = TablePageRequest {
                profile_id: Uuid::nil(),
                schema: database.clone(),
                table: "files".into(),
                offset: 0,
                limit: 10,
                filters: vec![FilterCondition {
                    column: "path".into(),
                    operator: FilterOperator::Equals,
                    value: Some("C:\\temp\\".into()),
                }],
                order_by: None,
                include_total: Some(true),
            };
            let page = session.table_page(&request).await.expect("filtered page");
            assert_eq!(
                page.total_rows,
                Some(1),
                "backslashes are matched literally"
            );
            let row = &page.rows[0];
            assert_eq!(row[0], JsonValue::String("9007199254740993".into()));
            assert_eq!(row[1], JsonValue::String("C:\\temp\\".into()));
            assert_eq!(row[2], JsonValue::String("26:03:04".into()));
            assert_eq!(row[3], serde_json::json!(0.1));

            let mut changes = row.clone();
            changes[1] = JsonValue::String("D:\\it's\\".into());
            let result = session
                .apply_mutations(&MutationBatch {
                    profile_id: Uuid::nil(),
                    schema: database.clone(),
                    table: "files".into(),
                    mutations: vec![RowMutation {
                        original: row.clone(),
                        changes,
                        primary_key: vec![row[0].clone()],
                        xmin: None,
                        deleted: false,
                    }],
                })
                .await
                .expect("mutation");
            assert_eq!(result.applied, 1);
            let paths = session
                .run_query("SELECT id, path FROM files ORDER BY id", None)
                .await
                .expect("select");
            assert_eq!(
                paths.rows,
                vec![
                    vec![
                        JsonValue::String("9007199254740992".into()),
                        JsonValue::String("other".into())
                    ],
                    vec![
                        JsonValue::String("9007199254740993".into()),
                        JsonValue::String("D:\\it's\\".into())
                    ],
                ],
                "only the edited row changed"
            );

            session
                .run_query("SET @dbm_answer = 42", None)
                .await
                .expect("set variable");
            let answer = session
                .run_query("SELECT @dbm_answer AS answer", None)
                .await
                .expect("read variable");
            assert_eq!(answer.rows, vec![vec![JsonValue::String("42".into())]]);

            let script = session
                .run_query("SELECT 1; SELECT 2 AS two", None)
                .await
                .expect("script");
            assert_eq!(script.columns[0].name, "two");

            let reader = MysqlSession::connect(profile(&database, true).expect("profile"), None)
                .await
                .expect("connect read-only");
            let error = reader
                .run_query("/* hidden */ DELETE FROM files", None)
                .await
                .expect_err("read-only session rejects writes");
            assert!(
                error.to_string().to_ascii_lowercase().contains("read only"),
                "{error}"
            );
            reader.close().await;

            session.close().await;
            admin
                .run_query(&format!("DROP DATABASE {database}"), None)
                .await
                .expect("cleanup");
            admin.close().await;
        }
    }
}
