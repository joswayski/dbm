use std::collections::BTreeSet;
use std::time::Instant;

use redis::aio::MultiplexedConnection;
use redis::{
    AsyncCommands, Client, Cmd, ConnectionAddr, ConnectionInfo, RedisConnectionInfo, Value,
};
use serde_json::{Number, Value as JsonValue};
use tokio::sync::Mutex;

use crate::error::{AppError, AppResult};
use crate::models::{
    ConnectionProfile, DatabaseRef, FilterCondition, FilterOperator, MutationBatch, MutationResult,
    QueryColumn, QueryResponse, SchemaNode, TableColumn, TableMetadata, TablePage,
    TablePageRequest, TlsMode, parse_redis_database,
};

const MAX_PAGE_SIZE: u32 = 1_000;
const DEFAULT_QUERY_ROWS: u32 = 10_000;
const MAX_SCAN_KEYS: usize = 10_000;
const VALUE_PREVIEW_CHARS: usize = 256;
const KEYS_SCHEMA: &str = "keys";
const KEY_TYPES: [&str; 6] = ["string", "hash", "list", "set", "zset", "stream"];
const BLOCKED_SESSION_COMMANDS: [&str; 9] = [
    "select",
    "swapdb",
    "auth",
    "hello",
    "quit",
    "monitor",
    "subscribe",
    "psubscribe",
    "ssubscribe",
];

pub struct RedisSession {
    profile: ConnectionProfile,
    connection: Mutex<MultiplexedConnection>,
}

impl RedisSession {
    pub async fn connect(profile: ConnectionProfile, password: Option<String>) -> AppResult<Self> {
        if profile.ssh.is_some() {
            return Err(AppError::Unsupported(
                "SSH tunneling is not supported for this connection".into(),
            ));
        }
        profile.validate()?;
        let connection = connect_client(&profile, password.as_deref()).await?;
        Ok(Self {
            profile,
            connection: Mutex::new(connection),
        })
    }

    pub fn profile(&self) -> &ConnectionProfile {
        &self.profile
    }

    pub async fn close(&self) {}

    pub async fn list_databases(&self) -> AppResult<Vec<DatabaseRef>> {
        let mut conn = self.connection.lock().await;
        let current = current_database(&self.profile)?;
        let count = database_count(&mut conn)
            .await
            .unwrap_or(16)
            .max(current + 1);
        Ok((0..count)
            .map(|index| DatabaseRef {
                name: index.to_string(),
                is_template: false,
                is_connectable: true,
            })
            .collect())
    }

    pub async fn schema_tree(&self) -> AppResult<Vec<SchemaNode>> {
        let mut children = vec![key_type_node("all")];
        children.extend(KEY_TYPES.iter().copied().map(key_type_node));
        Ok(vec![SchemaNode {
            name: "keys".into(),
            kind: "schema".into(),
            schema: Some(KEYS_SCHEMA.into()),
            table: None,
            children,
        }])
    }

    pub async fn table_page(&self, request: &TablePageRequest) -> AppResult<TablePage> {
        let limit = request.limit.clamp(1, MAX_PAGE_SIZE);
        if request.schema == KEYS_SCHEMA {
            return self.key_listing(request, limit).await;
        }
        self.key_contents(request, limit).await
    }

    pub async fn run_query(&self, sql: &str, max_rows: Option<u32>) -> AppResult<QueryResponse> {
        let command = sql.trim();
        if command.is_empty() {
            return Err(AppError::InvalidInput("command is empty".into()));
        }
        let args = parse_redis_cli(command)?;
        let name = args[0].to_ascii_lowercase();
        if BLOCKED_SESSION_COMMANDS.contains(&name.as_str()) {
            return Err(AppError::Unsupported(
                "use the database picker to change Redis databases; AUTH, SELECT, and pub/sub commands are not available in the workbench".into(),
            ));
        }
        if self.profile.read_only && is_write_command(&name) {
            return Err(AppError::Unsupported("profile is read-only".into()));
        }
        let max_rows = max_rows
            .unwrap_or(DEFAULT_QUERY_ROWS)
            .clamp(1, DEFAULT_QUERY_ROWS);
        let started = Instant::now();
        let mut conn = self.connection.lock().await;
        select_database(&mut conn, &self.profile).await?;
        let mut cmd = Cmd::new();
        for arg in &args {
            cmd.arg(arg);
        }
        let value: Value = cmd.query_async(&mut *conn).await.map_err(AppError::from)?;
        let mut response = value_to_query_response(value);
        if response.rows.len() > usize::try_from(max_rows).unwrap_or(usize::MAX) {
            response
                .rows
                .truncate(usize::try_from(max_rows).unwrap_or_default());
            response.truncated = true;
        }
        response.row_count = response.rows.len();
        response.duration_ms = started.elapsed().as_millis();
        Ok(response)
    }

    pub async fn apply_mutations(&self, batch: &MutationBatch) -> AppResult<MutationResult> {
        if self.profile.read_only {
            return Err(AppError::Unsupported("profile is read-only".into()));
        }
        let mut conn = self.connection.lock().await;
        select_database(&mut conn, &self.profile).await?;
        if batch.schema == KEYS_SCHEMA {
            return apply_key_listing_mutations(&mut conn, batch).await;
        }
        apply_key_content_mutations(&mut conn, batch).await
    }

    async fn key_listing(&self, request: &TablePageRequest, limit: u32) -> AppResult<TablePage> {
        let type_filter = if request.table == "all" {
            None
        } else if KEY_TYPES.contains(&request.table.as_str()) {
            Some(request.table.as_str())
        } else {
            return Err(AppError::InvalidInput(format!(
                "unknown Redis key type {}",
                request.table
            )));
        };
        let metadata = key_listing_metadata(&request.table);
        let pattern = scan_pattern(&request.filters);
        let mut conn = self.connection.lock().await;
        select_database(&mut conn, &self.profile).await?;
        let keys = scan_keys(&mut conn, &pattern, MAX_SCAN_KEYS).await?;
        let mut rows = Vec::new();
        for chunk in keys.chunks(50) {
            rows.extend(describe_keys(&mut conn, chunk).await?);
        }
        let mut rows = rows
            .into_iter()
            .filter(|row| type_filter.is_none_or(|expected| row[1].as_str() == Some(expected)))
            .filter(|row| row_matches_filters(row, &metadata, &request.filters))
            .collect::<Vec<_>>();
        sort_rows(&mut rows, &metadata, request.order_by.as_ref());
        paginate_rows(metadata, rows, request.offset, limit)
    }

    async fn key_contents(&self, request: &TablePageRequest, limit: u32) -> AppResult<TablePage> {
        let key_type = normalize_key_type(&request.schema)?;
        let key = request.table.as_str();
        let mut conn = self.connection.lock().await;
        select_database(&mut conn, &self.profile).await?;
        let actual = type_of_key(&mut conn, key).await?;
        if actual == "none" {
            return Err(AppError::InvalidInput(format!("key {key} was not found")));
        }
        if actual != key_type {
            return Err(AppError::InvalidInput(format!(
                "key {key} is a {actual}, not a {key_type}"
            )));
        }
        let metadata = key_content_metadata(key_type, key);
        let mut rows = load_key_rows(&mut conn, key, key_type).await?;
        rows.retain(|row| row_matches_filters(row, &metadata, &request.filters));
        sort_rows(&mut rows, &metadata, request.order_by.as_ref());
        paginate_rows(metadata, rows, request.offset, limit)
    }
}

fn key_type_node(name: &str) -> SchemaNode {
    SchemaNode {
        name: name.to_owned(),
        kind: "table".into(),
        schema: Some(KEYS_SCHEMA.into()),
        table: Some(name.to_owned()),
        children: Vec::new(),
    }
}

fn key_listing_metadata(table: &str) -> TableMetadata {
    TableMetadata {
        schema: KEYS_SCHEMA.into(),
        table: table.to_owned(),
        columns: vec![
            column("key", "string", false, 1),
            column("type", "string", false, 2),
            column("ttl", "integer", true, 3),
            column("length", "integer", true, 4),
            column("value", "string", true, 5),
        ],
        primary_key: vec!["key".into()],
        has_xmin: false,
    }
}

fn key_content_metadata(key_type: &str, key: &str) -> TableMetadata {
    let columns = match key_type {
        "hash" => vec![
            column("field", "string", false, 1),
            column("value", "string", true, 2),
        ],
        "list" => vec![
            column("index", "integer", false, 1),
            column("value", "string", true, 2),
        ],
        "set" => vec![column("member", "string", false, 1)],
        "zset" => vec![
            column("member", "string", false, 1),
            column("score", "number", false, 2),
        ],
        "stream" => vec![
            column("id", "string", false, 1),
            column("fields", "json", true, 2),
        ],
        _ => vec![
            column("field", "string", false, 1),
            column("value", "string", true, 2),
        ],
    };
    let primary_key = match key_type {
        "list" => vec!["index".into()],
        "set" | "zset" => vec!["member".into()],
        "stream" => vec!["id".into()],
        _ => vec!["field".into()],
    };
    TableMetadata {
        schema: key_type.to_owned(),
        table: key.to_owned(),
        columns,
        primary_key,
        has_xmin: false,
    }
}

fn column(name: &str, data_type: &str, nullable: bool, ordinal: i32) -> TableColumn {
    TableColumn {
        name: name.to_owned(),
        data_type: data_type.to_owned(),
        nullable,
        default_value: None,
        ordinal,
    }
}

async fn connect_client(
    profile: &ConnectionProfile,
    password: Option<&str>,
) -> AppResult<MultiplexedConnection> {
    if matches!(profile.tls_mode, TlsMode::Disabled) {
        return open_connection(profile, password, false).await;
    }
    match open_connection(profile, password, true).await {
        Ok(connection) => Ok(connection),
        Err(_error) if matches!(profile.tls_mode, TlsMode::Preferred) => {
            open_connection(profile, password, false).await
        }
        Err(error) => Err(error),
    }
}

async fn open_connection(
    profile: &ConnectionProfile,
    password: Option<&str>,
    use_tls: bool,
) -> AppResult<MultiplexedConnection> {
    let db = current_database(profile)?;
    let host = profile.host.clone();
    let addr = if use_tls {
        ConnectionAddr::TcpTls {
            host,
            port: profile.port,
            insecure: !matches!(profile.tls_mode, TlsMode::Required)
                && profile.ca_cert_path.is_none(),
            tls_params: None,
        }
    } else {
        ConnectionAddr::Tcp(host, profile.port)
    };
    let mut redis_settings = RedisConnectionInfo::default()
        .set_db(db)
        .set_lib_name("dbm", env!("CARGO_PKG_VERSION"));
    if !profile.username.is_empty() {
        redis_settings = redis_settings.set_username(&profile.username);
    }
    if let Some(password) = password.filter(|value| !value.is_empty()) {
        redis_settings = redis_settings.set_password(password);
    }
    let scheme = if use_tls { "rediss" } else { "redis" };
    let encoded_host = if profile.host.contains(':') && !profile.host.starts_with('[') {
        format!("[{}]", profile.host)
    } else {
        profile.host.clone()
    };
    let info = format!("{scheme}://{encoded_host}:{}/{db}", profile.port)
        .parse::<ConnectionInfo>()
        .map_err(AppError::from)?
        .set_addr(addr)
        .set_redis_settings(redis_settings);
    let client = Client::open(info).map_err(AppError::from)?;
    let mut connection = client
        .get_multiplexed_async_connection()
        .await
        .map_err(AppError::from)?;
    let _: String = redis::cmd("PING")
        .query_async(&mut connection)
        .await
        .map_err(AppError::from)?;
    Ok(connection)
}

fn current_database(profile: &ConnectionProfile) -> AppResult<i64> {
    parse_redis_database(&profile.default_database).map_err(|()| {
        AppError::InvalidInput("Redis database must be a non-negative integer".into())
    })
}

async fn select_database(
    conn: &mut MultiplexedConnection,
    profile: &ConnectionProfile,
) -> AppResult<()> {
    let db = current_database(profile)?;
    redis::cmd("SELECT")
        .arg(db)
        .query_async::<()>(conn)
        .await
        .map_err(AppError::from)
}

async fn database_count(conn: &mut MultiplexedConnection) -> AppResult<i64> {
    let values: Vec<String> = redis::cmd("CONFIG")
        .arg("GET")
        .arg("databases")
        .query_async(conn)
        .await
        .map_err(AppError::from)?;
    values
        .windows(2)
        .find(|pair| pair[0].eq_ignore_ascii_case("databases"))
        .and_then(|pair| pair[1].parse::<i64>().ok())
        .or_else(|| values.last().and_then(|value| value.parse().ok()))
        .filter(|count| *count > 0)
        .ok_or_else(|| AppError::Database("CONFIG GET databases returned no usable value".into()))
}

async fn scan_keys(
    conn: &mut MultiplexedConnection,
    pattern: &str,
    max_keys: usize,
) -> AppResult<Vec<String>> {
    let mut cursor: u64 = 0;
    let mut keys = Vec::new();
    let mut seen = BTreeSet::new();
    loop {
        let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg(pattern)
            .arg("COUNT")
            .arg(200)
            .query_async(conn)
            .await
            .map_err(AppError::from)?;
        for key in batch {
            if seen.insert(key.clone()) {
                keys.push(key);
                if keys.len() >= max_keys {
                    return Ok(keys);
                }
            }
        }
        cursor = next;
        if cursor == 0 {
            return Ok(keys);
        }
    }
}

async fn describe_keys(
    conn: &mut MultiplexedConnection,
    keys: &[String],
) -> AppResult<Vec<Vec<JsonValue>>> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let mut pipe = redis::pipe();
    for key in keys {
        pipe.cmd("TYPE").arg(key).cmd("TTL").arg(key);
    }
    let typed: Vec<Value> = pipe.query_async(conn).await.map_err(AppError::from)?;
    let mut rows = Vec::with_capacity(keys.len());
    let mut length_pipe = redis::pipe();
    let mut length_needed = false;
    for (index, key) in keys.iter().enumerate() {
        let type_name = redis_type_name(typed.get(index * 2).cloned().unwrap_or(Value::Nil))?;
        let ttl = json_from_redis(typed.get(index * 2 + 1).cloned().unwrap_or(Value::Nil));
        if type_name == "string" {
            length_pipe.cmd("STRLEN").arg(key).cmd("GET").arg(key);
            length_needed = true;
        } else if let Some(command) = length_command(&type_name) {
            length_pipe.cmd(command).arg(key).cmd("ECHO").arg("");
            length_needed = true;
        } else {
            length_pipe.cmd("ECHO").arg("").cmd("ECHO").arg("");
        }
        rows.push(vec![
            JsonValue::String(key.clone()),
            JsonValue::String(type_name),
            ttl_value(ttl),
            JsonValue::Null,
            JsonValue::Null,
        ]);
    }
    if length_needed {
        let extras: Vec<Value> = length_pipe
            .query_async(conn)
            .await
            .map_err(AppError::from)?;
        for (index, row) in rows.iter_mut().enumerate() {
            let length = extras.get(index * 2).cloned().unwrap_or(Value::Nil);
            let preview = extras.get(index * 2 + 1).cloned().unwrap_or(Value::Nil);
            row[3] = json_from_redis(length);
            if row[1].as_str() == Some("string") {
                row[4] = preview_value(preview);
            }
        }
    }
    Ok(rows)
}

fn length_command(key_type: &str) -> Option<&'static str> {
    match key_type {
        "hash" => Some("HLEN"),
        "list" => Some("LLEN"),
        "set" => Some("SCARD"),
        "zset" => Some("ZCARD"),
        "stream" => Some("XLEN"),
        _ => None,
    }
}

fn ttl_value(value: JsonValue) -> JsonValue {
    match value {
        JsonValue::Number(number) if number.as_i64() == Some(-1) || number.as_i64() == Some(-2) => {
            JsonValue::Null
        }
        other => other,
    }
}

fn preview_value(value: Value) -> JsonValue {
    let rendered = display_redis_value(&value);
    if rendered.chars().count() > VALUE_PREVIEW_CHARS {
        JsonValue::String(format!(
            "{}…",
            rendered
                .chars()
                .take(VALUE_PREVIEW_CHARS)
                .collect::<String>()
        ))
    } else if rendered.is_empty() && matches!(value, Value::Nil) {
        JsonValue::Null
    } else {
        JsonValue::String(rendered)
    }
}

async fn load_key_rows(
    conn: &mut MultiplexedConnection,
    key: &str,
    key_type: &str,
) -> AppResult<Vec<Vec<JsonValue>>> {
    match key_type {
        "string" => {
            let value: Value = conn.get(key).await.map_err(AppError::from)?;
            Ok(vec![vec![
                JsonValue::String("value".into()),
                json_from_redis(value),
            ]])
        }
        "hash" => {
            let pairs: Vec<(String, String)> = conn.hgetall(key).await.map_err(AppError::from)?;
            Ok(pairs
                .into_iter()
                .map(|(field, value)| vec![JsonValue::String(field), JsonValue::String(value)])
                .collect())
        }
        "list" => {
            let values: Vec<String> = conn.lrange(key, 0, -1).await.map_err(AppError::from)?;
            Ok(values
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    vec![
                        JsonValue::Number(Number::from(index as i64)),
                        JsonValue::String(value),
                    ]
                })
                .collect())
        }
        "set" => {
            let members: Vec<String> = conn.smembers(key).await.map_err(AppError::from)?;
            Ok(members
                .into_iter()
                .map(|member| vec![JsonValue::String(member)])
                .collect())
        }
        "zset" => {
            let members: Vec<(String, f64)> = redis::cmd("ZRANGE")
                .arg(key)
                .arg(0)
                .arg(-1)
                .arg("WITHSCORES")
                .query_async(conn)
                .await
                .map_err(AppError::from)?;
            Ok(members
                .into_iter()
                .map(|(member, score)| {
                    vec![
                        JsonValue::String(member),
                        Number::from_f64(score)
                            .map(JsonValue::Number)
                            .unwrap_or(JsonValue::String(score.to_string())),
                    ]
                })
                .collect())
        }
        "stream" => {
            let entries: Value = redis::cmd("XRANGE")
                .arg(key)
                .arg("-")
                .arg("+")
                .query_async(conn)
                .await
                .map_err(AppError::from)?;
            Ok(stream_rows(entries))
        }
        other => Err(AppError::Unsupported(format!(
            "Redis type {other} is not browsable yet"
        ))),
    }
}

fn stream_rows(value: Value) -> Vec<Vec<JsonValue>> {
    let Value::Array(entries) = value else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|entry| {
            let Value::Array(parts) = entry else {
                return None;
            };
            let id = parts.first().map(display_redis_value).unwrap_or_default();
            let fields = parts.get(1).cloned().unwrap_or(Value::Nil);
            Some(vec![JsonValue::String(id), json_from_redis(fields)])
        })
        .collect()
}

async fn apply_key_listing_mutations(
    conn: &mut MultiplexedConnection,
    batch: &MutationBatch,
) -> AppResult<MutationResult> {
    let mut applied = 0;
    let mut conflicts = Vec::new();
    for mutation in &batch.mutations {
        let Some(key) = mutation.primary_key.first().and_then(JsonValue::as_str) else {
            conflicts.push(mutation.primary_key.clone());
            continue;
        };
        let exists: bool = conn.exists(key).await.map_err(AppError::from)?;
        if !exists {
            conflicts.push(mutation.primary_key.clone());
            continue;
        }
        if mutation.deleted {
            let _: i32 = conn.del(key).await.map_err(AppError::from)?;
            applied += 1;
            continue;
        }
        let original = &mutation.original;
        let changes = &mutation.changes;
        if changes.get(4) != original.get(4) {
            let type_name = type_of_key(conn, key).await?;
            if type_name != "string" {
                conflicts.push(mutation.primary_key.clone());
                continue;
            }
            let value = json_to_string(changes.get(4).unwrap_or(&JsonValue::Null));
            let _: () = conn.set(key, value).await.map_err(AppError::from)?;
        }
        if changes.get(2) != original.get(2) {
            match changes.get(2) {
                None | Some(JsonValue::Null) => {
                    let _: bool = redis::cmd("PERSIST")
                        .arg(key)
                        .query_async(conn)
                        .await
                        .map_err(AppError::from)?;
                }
                Some(JsonValue::Number(number)) => {
                    let ttl = number.as_i64().unwrap_or(-1);
                    if ttl < 0 {
                        let _: bool = redis::cmd("PERSIST")
                            .arg(key)
                            .query_async(conn)
                            .await
                            .map_err(AppError::from)?;
                    } else {
                        let _: bool = conn.expire(key, ttl).await.map_err(AppError::from)?;
                    }
                }
                _ => {
                    conflicts.push(mutation.primary_key.clone());
                    continue;
                }
            }
        }
        applied += 1;
    }
    Ok(MutationResult { applied, conflicts })
}

async fn apply_key_content_mutations(
    conn: &mut MultiplexedConnection,
    batch: &MutationBatch,
) -> AppResult<MutationResult> {
    let key_type = normalize_key_type(&batch.schema)?;
    let key = batch.table.as_str();
    let actual = type_of_key(conn, key).await?;
    if actual != key_type {
        return Err(AppError::InvalidInput(format!(
            "key {key} is a {actual}, not a {key_type}"
        )));
    }
    let mut applied = 0;
    let mut conflicts = Vec::new();
    for mutation in &batch.mutations {
        let result = match key_type {
            "string" => mutate_string(conn, key, mutation).await,
            "hash" => mutate_hash(conn, key, mutation).await,
            "list" => mutate_list(conn, key, mutation).await,
            "set" => mutate_set(conn, key, mutation).await,
            "zset" => mutate_zset(conn, key, mutation).await,
            "stream" => mutate_stream(conn, key, mutation).await,
            _ => Err(AppError::Unsupported(format!(
                "Redis type {key_type} cannot be edited"
            ))),
        };
        match result {
            Ok(true) => applied += 1,
            Ok(false) => conflicts.push(mutation.primary_key.clone()),
            Err(error) => return Err(error),
        }
    }
    Ok(MutationResult { applied, conflicts })
}

async fn mutate_string(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutation: &crate::models::RowMutation,
) -> AppResult<bool> {
    if mutation.deleted {
        return Err(AppError::Unsupported(
            "delete the string key from the key list".into(),
        ));
    }
    let value = json_to_string(mutation.changes.get(1).unwrap_or(&JsonValue::Null));
    let _: () = conn.set(key, value).await.map_err(AppError::from)?;
    Ok(true)
}

async fn mutate_hash(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutation: &crate::models::RowMutation,
) -> AppResult<bool> {
    let Some(field) = mutation.primary_key.first().and_then(JsonValue::as_str) else {
        return Ok(false);
    };
    if mutation.deleted {
        let removed: i32 = conn.hdel(key, field).await.map_err(AppError::from)?;
        return Ok(removed > 0);
    }
    let value = json_to_string(mutation.changes.get(1).unwrap_or(&JsonValue::Null));
    let _: () = conn.hset(key, field, value).await.map_err(AppError::from)?;
    Ok(true)
}

async fn mutate_list(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutation: &crate::models::RowMutation,
) -> AppResult<bool> {
    let Some(index) = mutation.primary_key.first().and_then(JsonValue::as_i64) else {
        return Ok(false);
    };
    if mutation.deleted {
        let marker = format!("__dbm_deleted_{}", uuid::Uuid::new_v4());
        redis::cmd("LSET")
            .arg(key)
            .arg(index)
            .arg(&marker)
            .query_async::<()>(conn)
            .await
            .map_err(AppError::from)?;
        let _: i32 = redis::cmd("LREM")
            .arg(key)
            .arg(1)
            .arg(&marker)
            .query_async(conn)
            .await
            .map_err(AppError::from)?;
        return Ok(true);
    }
    let value = json_to_string(mutation.changes.get(1).unwrap_or(&JsonValue::Null));
    redis::cmd("LSET")
        .arg(key)
        .arg(index)
        .arg(value)
        .query_async::<()>(conn)
        .await
        .map_err(AppError::from)?;
    Ok(true)
}

async fn mutate_set(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutation: &crate::models::RowMutation,
) -> AppResult<bool> {
    let Some(member) = mutation.primary_key.first().and_then(JsonValue::as_str) else {
        return Ok(false);
    };
    if mutation.deleted {
        let removed: i32 = conn.srem(key, member).await.map_err(AppError::from)?;
        return Ok(removed > 0);
    }
    Ok(false)
}

async fn mutate_zset(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutation: &crate::models::RowMutation,
) -> AppResult<bool> {
    let Some(member) = mutation.primary_key.first().and_then(JsonValue::as_str) else {
        return Ok(false);
    };
    if mutation.deleted {
        let removed: i32 = conn.zrem(key, member).await.map_err(AppError::from)?;
        return Ok(removed > 0);
    }
    let score = match mutation.changes.get(1) {
        Some(JsonValue::Number(number)) => number.as_f64().unwrap_or_default(),
        Some(JsonValue::String(value)) => value.parse().unwrap_or_default(),
        _ => return Ok(false),
    };
    let _: i32 = conn
        .zadd(key, member, score)
        .await
        .map_err(AppError::from)?;
    Ok(true)
}

async fn mutate_stream(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutation: &crate::models::RowMutation,
) -> AppResult<bool> {
    if !mutation.deleted {
        return Err(AppError::Unsupported(
            "stream entries can only be deleted".into(),
        ));
    }
    let Some(id) = mutation.primary_key.first().and_then(JsonValue::as_str) else {
        return Ok(false);
    };
    let removed: i32 = redis::cmd("XDEL")
        .arg(key)
        .arg(id)
        .query_async(conn)
        .await
        .map_err(AppError::from)?;
    Ok(removed > 0)
}

fn normalize_key_type(value: &str) -> AppResult<&str> {
    match value {
        "string" | "hash" | "list" | "set" | "zset" | "stream" => Ok(value),
        "sorted set" | "sortedset" | "zset_sorted" => Ok("zset"),
        other => Err(AppError::InvalidInput(format!(
            "unknown Redis key type {other}"
        ))),
    }
}

async fn type_of_key(conn: &mut MultiplexedConnection, key: &str) -> AppResult<String> {
    redis::cmd("TYPE")
        .arg(key)
        .query_async(conn)
        .await
        .map_err(AppError::from)
        .and_then(redis_type_name)
}

fn redis_type_name(value: Value) -> AppResult<String> {
    match value {
        Value::Okay => Ok("ok".into()),
        Value::SimpleString(value) => Ok(if value.is_empty() {
            "none".into()
        } else {
            value.to_ascii_lowercase()
        }),
        Value::BulkString(bytes) => Ok(String::from_utf8_lossy(&bytes).to_ascii_lowercase()),
        Value::Nil => Ok("none".into()),
        other => Err(AppError::Database(format!(
            "unexpected TYPE response: {}",
            display_redis_value(&other)
        ))),
    }
}

fn scan_pattern(filters: &[FilterCondition]) -> String {
    let key_filters = filters
        .iter()
        .filter(|filter| filter.column == "key")
        .collect::<Vec<_>>();
    if key_filters.len() != 1 {
        return "*".into();
    }
    let filter = key_filters[0];
    let value = filter.value.as_deref().unwrap_or_default();
    match filter.operator {
        FilterOperator::Equals => value.to_owned(),
        FilterOperator::StartsWith => format!("{value}*"),
        FilterOperator::EndsWith => format!("*{value}"),
        FilterOperator::Contains => format!("*{value}*"),
        _ => "*".into(),
    }
}

fn row_matches_filters(
    row: &[JsonValue],
    metadata: &TableMetadata,
    filters: &[FilterCondition],
) -> bool {
    filters.iter().all(|filter| {
        let Some(index) = metadata
            .columns
            .iter()
            .position(|column| column.name == filter.column)
        else {
            return false;
        };
        let cell = row.get(index).unwrap_or(&JsonValue::Null);
        cell_matches(cell, filter)
    })
}

fn cell_matches(cell: &JsonValue, filter: &FilterCondition) -> bool {
    let value = filter.value.as_deref().unwrap_or_default();
    let text = match cell {
        JsonValue::Null => String::new(),
        JsonValue::String(value) => value.clone(),
        other => other.to_string(),
    };
    let normalized_cell = text.to_ascii_lowercase();
    let normalized_value = value.to_ascii_lowercase();
    match filter.operator {
        FilterOperator::Equals => text == value,
        FilterOperator::NotEquals => text != value,
        FilterOperator::Contains => normalized_cell.contains(&normalized_value),
        FilterOperator::StartsWith => normalized_cell.starts_with(&normalized_value),
        FilterOperator::EndsWith => normalized_cell.ends_with(&normalized_value),
        FilterOperator::GreaterThan => compare_json(cell, value) > 0,
        FilterOperator::GreaterThanOrEqual => compare_json(cell, value) >= 0,
        FilterOperator::LessThan => compare_json(cell, value) < 0,
        FilterOperator::LessThanOrEqual => compare_json(cell, value) <= 0,
        FilterOperator::In => value.split(',').map(str::trim).any(|item| item == text),
        FilterOperator::NotIn => !value.split(',').map(str::trim).any(|item| item == text),
        FilterOperator::IsNull => cell.is_null(),
        FilterOperator::IsNotNull => !cell.is_null(),
    }
}

fn compare_json(cell: &JsonValue, value: &str) -> i32 {
    if let Some(left) = cell.as_f64()
        && let Ok(right) = value.parse::<f64>()
    {
        return left
            .partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal) as i32;
    }
    display_json(cell).cmp(&value.to_owned()) as i32
}

fn sort_rows(
    rows: &mut [Vec<JsonValue>],
    metadata: &TableMetadata,
    order_by: Option<&crate::models::OrderSpec>,
) {
    let Some(order_by) = order_by else {
        return;
    };
    let Some(index) = metadata
        .columns
        .iter()
        .position(|column| column.name == order_by.column)
    else {
        return;
    };
    rows.sort_by(|left, right| {
        let ordering = display_json(left.get(index).unwrap_or(&JsonValue::Null))
            .cmp(&display_json(right.get(index).unwrap_or(&JsonValue::Null)));
        if order_by.descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
}

fn paginate_rows(
    metadata: TableMetadata,
    rows: Vec<Vec<JsonValue>>,
    offset: u32,
    limit: u32,
) -> AppResult<TablePage> {
    let offset = usize::try_from(offset).unwrap_or_default();
    let limit = usize::try_from(limit).unwrap_or_default().max(1);
    let total_rows = u64::try_from(rows.len()).ok();
    let page = rows
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let has_more = total_rows
        .is_some_and(|total| u64::try_from(offset + page.len()).unwrap_or(u64::MAX) < total);
    Ok(TablePage {
        columns: metadata
            .columns
            .iter()
            .map(|column| column.name.clone())
            .collect(),
        metadata,
        total_rows,
        offset: u32::try_from(offset).unwrap_or_default(),
        limit: u32::try_from(limit).unwrap_or_default(),
        has_more,
        rows: page,
    })
}

pub fn parse_redis_cli(input: &str) -> AppResult<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut chars = input.trim().chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    while let Some(char) = chars.next() {
        match char {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '\\' if in_double => {
                if let Some(escaped) = chars.next() {
                    current.push(match escaped {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                }
            }
            char if char.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            other => current.push(other),
        }
    }
    if in_single || in_double {
        return Err(AppError::InvalidInput(
            "unterminated quote in Redis command".into(),
        ));
    }
    if !current.is_empty() {
        args.push(current);
    }
    if args.is_empty() {
        return Err(AppError::InvalidInput("command is empty".into()));
    }
    Ok(args)
}

fn is_write_command(name: &str) -> bool {
    !matches!(
        name,
        "ping"
            | "echo"
            | "info"
            | "time"
            | "role"
            | "lolwut"
            | "dbsize"
            | "lastsave"
            | "command"
            | "client"
            | "config"
            | "acl"
            | "memory"
            | "object"
            | "slowlog"
            | "scan"
            | "keys"
            | "randomkey"
            | "type"
            | "ttl"
            | "pttl"
            | "expiretime"
            | "pexpiretime"
            | "exists"
            | "touch"
            | "dump"
            | "strlen"
            | "get"
            | "getrange"
            | "getbit"
            | "mget"
            | "substr"
            | "hexists"
            | "hget"
            | "hgetall"
            | "hkeys"
            | "hlen"
            | "hmget"
            | "hrandfield"
            | "hscan"
            | "hstrlen"
            | "hvals"
            | "lindex"
            | "llen"
            | "lpos"
            | "lrange"
            | "scard"
            | "sdiff"
            | "sinter"
            | "sismember"
            | "smembers"
            | "smismember"
            | "srandmember"
            | "sscan"
            | "sunion"
            | "zcard"
            | "zcount"
            | "zdiff"
            | "zinter"
            | "zintercard"
            | "zlexcount"
            | "zmscore"
            | "zrange"
            | "zrangebylex"
            | "zrangebyscore"
            | "zrank"
            | "zrevrange"
            | "zrevrangebylex"
            | "zrevrangebyscore"
            | "zrevrank"
            | "zscan"
            | "zscore"
            | "zunion"
            | "xinfo"
            | "xlen"
            | "xpending"
            | "xrange"
            | "xread"
            | "xrevrange"
            | "json.get"
            | "json.mget"
            | "json.type"
            | "json.strlen"
            | "json.objkeys"
            | "json.objlen"
            | "json.arrlen"
            | "ft.search"
            | "ft.aggregate"
            | "ft.info"
            | "ft._list"
    )
}

fn value_to_query_response(value: Value) -> QueryResponse {
    if let Some(pairs) = map_pairs(&value) {
        return QueryResponse {
            columns: vec![
                QueryColumn {
                    name: "field".into(),
                    data_type: "string".into(),
                },
                QueryColumn {
                    name: "value".into(),
                    data_type: "string".into(),
                },
            ],
            rows: pairs,
            row_count: 0,
            affected_rows: None,
            duration_ms: 0,
            truncated: false,
            notices: Vec::new(),
        };
    }
    match value {
        Value::Nil => QueryResponse {
            columns: vec![QueryColumn {
                name: "value".into(),
                data_type: "string".into(),
            }],
            rows: vec![vec![JsonValue::Null]],
            row_count: 1,
            affected_rows: None,
            duration_ms: 0,
            truncated: false,
            notices: Vec::new(),
        },
        Value::Array(items) => {
            let rows = items
                .into_iter()
                .map(|item| vec![json_from_redis(item)])
                .collect::<Vec<_>>();
            QueryResponse {
                columns: vec![QueryColumn {
                    name: "value".into(),
                    data_type: "string".into(),
                }],
                rows,
                row_count: 0,
                affected_rows: None,
                duration_ms: 0,
                truncated: false,
                notices: Vec::new(),
            }
        }
        Value::Set(items) => {
            let rows = items
                .into_iter()
                .map(|item| vec![json_from_redis(item)])
                .collect::<Vec<_>>();
            QueryResponse {
                columns: vec![QueryColumn {
                    name: "value".into(),
                    data_type: "string".into(),
                }],
                rows,
                row_count: 0,
                affected_rows: None,
                duration_ms: 0,
                truncated: false,
                notices: Vec::new(),
            }
        }
        Value::Okay => status_response("OK"),
        other => {
            let rendered = json_from_redis(other);
            QueryResponse {
                columns: vec![QueryColumn {
                    name: "result".into(),
                    data_type: "string".into(),
                }],
                rows: vec![vec![rendered]],
                row_count: 1,
                affected_rows: None,
                duration_ms: 0,
                truncated: false,
                notices: Vec::new(),
            }
        }
    }
}

fn map_pairs(value: &Value) -> Option<Vec<Vec<JsonValue>>> {
    if let Value::Map(pairs) = value {
        return Some(
            pairs
                .iter()
                .map(|(field, item)| {
                    vec![
                        json_from_redis(field.clone()),
                        json_from_redis(item.clone()),
                    ]
                })
                .collect(),
        );
    }
    let Value::Array(items) = value else {
        return None;
    };
    if items.len() < 2 || items.len() % 2 != 0 {
        return None;
    }
    if !items.iter().all(|item| {
        matches!(
            item,
            Value::BulkString(_) | Value::SimpleString(_) | Value::Nil | Value::Int(_)
        )
    }) {
        return None;
    }
    Some(
        items
            .chunks(2)
            .map(|pair| {
                vec![
                    json_from_redis(pair[0].clone()),
                    json_from_redis(pair[1].clone()),
                ]
            })
            .collect(),
    )
}

fn status_response(status: &str) -> QueryResponse {
    QueryResponse {
        columns: vec![QueryColumn {
            name: "result".into(),
            data_type: "string".into(),
        }],
        rows: vec![vec![JsonValue::String(status.to_owned())]],
        row_count: 1,
        affected_rows: None,
        duration_ms: 0,
        truncated: false,
        notices: Vec::new(),
    }
}

fn json_from_redis(value: Value) -> JsonValue {
    match value {
        Value::Nil => JsonValue::Null,
        Value::Int(value) => JsonValue::Number(Number::from(value)),
        Value::BulkString(bytes) => bytes_to_json(&bytes),
        Value::SimpleString(value) => JsonValue::String(value),
        Value::Okay => JsonValue::String("OK".into()),
        Value::Boolean(value) => JsonValue::Bool(value),
        Value::Double(value) => Number::from_f64(value)
            .map(JsonValue::Number)
            .unwrap_or_else(|| JsonValue::String(value.to_string())),
        Value::Array(items) | Value::Set(items) => {
            JsonValue::Array(items.into_iter().map(json_from_redis).collect())
        }
        Value::Map(pairs) => {
            let mut object = serde_json::Map::new();
            for (key, value) in pairs {
                object.insert(display_redis_value(&key), json_from_redis(value));
            }
            JsonValue::Object(object)
        }
        Value::VerbatimString { text, .. } => JsonValue::String(text),
        Value::Attribute { data, .. } => json_from_redis(*data),
        Value::ServerError(error) => JsonValue::String(error.to_string()),
        other => JsonValue::String(display_redis_value(&other)),
    }
}

fn bytes_to_json(bytes: &[u8]) -> JsonValue {
    match std::str::from_utf8(bytes) {
        Ok(text) => JsonValue::String(text.to_owned()),
        Err(_) => JsonValue::String(format!("\\x{}", hex_encode(bytes))),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn display_redis_value(value: &Value) -> String {
    match json_from_redis(value.clone()) {
        JsonValue::Null => String::new(),
        JsonValue::String(value) => value,
        other => other.to_string(),
    }
}

fn display_json(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => String::new(),
        JsonValue::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn json_to_string(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => String::new(),
        JsonValue::String(value) => value.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::OrderSpec;

    #[test]
    fn parses_quoted_redis_commands() {
        assert_eq!(
            parse_redis_cli(r#"SET user:1 "Jose Valerio""#).expect("parse"),
            vec!["SET", "user:1", "Jose Valerio"]
        );
        assert_eq!(
            parse_redis_cli("HSET session:1 name 'hello world' count 2").expect("parse"),
            vec!["HSET", "session:1", "name", "hello world", "count", "2"]
        );
    }

    #[test]
    fn rejects_unterminated_quotes() {
        assert!(parse_redis_cli(r#"SET foo "bar"#).is_err());
    }

    #[test]
    fn scan_patterns_come_from_key_filters() {
        assert_eq!(
            scan_pattern(&[FilterCondition {
                column: "key".into(),
                operator: FilterOperator::StartsWith,
                value: Some("user:".into()),
            }]),
            "user:*"
        );
        assert_eq!(
            scan_pattern(&[FilterCondition {
                column: "type".into(),
                operator: FilterOperator::Equals,
                value: Some("hash".into()),
            }]),
            "*"
        );
    }

    #[test]
    fn read_only_detection_covers_common_writes() {
        assert!(is_write_command("set"));
        assert!(is_write_command("del"));
        assert!(is_write_command("flushall"));
        assert!(!is_write_command("get"));
        assert!(!is_write_command("hgetall"));
        assert!(!is_write_command("scan"));
        assert!(!is_write_command("xrange"));
    }

    #[test]
    fn even_bulk_arrays_render_as_field_value_tables() {
        let response = value_to_query_response(Value::Array(vec![
            Value::SimpleString("name".into()),
            Value::SimpleString("Jose".into()),
            Value::SimpleString("city".into()),
            Value::SimpleString("Detroit".into()),
        ]));
        assert_eq!(
            response
                .columns
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            ["field", "value"]
        );
        assert_eq!(response.rows.len(), 2);
        assert_eq!(response.rows[0][0], JsonValue::String("name".into()));
    }

    #[test]
    fn key_listing_filters_and_sorts_in_memory() {
        let metadata = key_listing_metadata("all");
        let mut rows = vec![
            vec![
                JsonValue::String("user:2".into()),
                JsonValue::String("hash".into()),
                JsonValue::Null,
                JsonValue::Number(Number::from(2)),
                JsonValue::Null,
            ],
            vec![
                JsonValue::String("session:1".into()),
                JsonValue::String("string".into()),
                JsonValue::Number(Number::from(30)),
                JsonValue::Number(Number::from(5)),
                JsonValue::String("hello".into()),
            ],
        ];
        rows.retain(|row| {
            row_matches_filters(
                row,
                &metadata,
                &[FilterCondition {
                    column: "type".into(),
                    operator: FilterOperator::Equals,
                    value: Some("string".into()),
                }],
            )
        });
        assert_eq!(rows.len(), 1);
        sort_rows(
            &mut rows,
            &metadata,
            Some(&OrderSpec {
                column: "key".into(),
                descending: false,
            }),
        );
        assert_eq!(rows[0][0], JsonValue::String("session:1".into()));
    }
}
