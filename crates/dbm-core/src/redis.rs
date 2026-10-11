use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use redis_rs::aio::MultiplexedConnection;
use redis_rs::{Client, Cmd, ConnectionAddr, ConnectionInfo, RedisConnectionInfo, Value};
use serde_json::Value as JsonValue;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{
    ConnectionProfile, DatabaseRef, FilterCondition, FilterOperator, MutationBatch, MutationResult,
    OrderSpec, QueryColumn, QueryResponse, RowMutation, SchemaNode, TableColumn, TableMetadata,
    TablePage, TablePageRequest, TlsMode, parse_redis_database,
};

const MAX_PAGE_SIZE: u32 = 1_000;
const DEFAULT_QUERY_ROWS: u32 = 10_000;
const TREE_KEYS_PER_TYPE: usize = 250;
/// SCAN COUNT hint: fewer round trips than the server default of 10.
const SCAN_COUNT: usize = 1_000;
/// Keys per TYPE/TTL pipeline, so huge keyspaces are not sent in one request.
const PIPELINE_KEYS: usize = 1_000;
const MAX_VALUE_BYTES: usize = 256 * 1024;
const TRUNCATION_MARKER: &str = "… (truncated)";
/// How long Preferred TLS may take before falling back to plaintext.
const PREFERRED_TLS_TIMEOUT: Duration = Duration::from_secs(5);
const KEY_TYPES: [&str; 6] = ["string", "hash", "list", "set", "zset", "stream"];

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
        let connection = connect_client(&profile, password.as_deref()).await?;
        Ok(Self {
            profile,
            connection: Mutex::new(connection),
        })
    }

    pub fn profile(&self) -> &ConnectionProfile {
        &self.profile
    }

    async fn conn(&self) -> tokio::sync::MutexGuard<'_, MultiplexedConnection> {
        self.connection.lock().await
    }

    pub async fn list_databases(&self) -> AppResult<Vec<DatabaseRef>> {
        let mut conn = self.conn().await;
        if cluster_enabled(&mut conn).await? {
            return Ok(vec![DatabaseRef {
                name: "0".into(),
                is_template: false,
                is_connectable: true,
            }]);
        }
        let count = database_count(&mut conn).await?;
        Ok((0..count)
            .map(|index| DatabaseRef {
                name: index.to_string(),
                is_template: false,
                is_connectable: true,
            })
            .collect())
    }

    pub async fn schema_tree(&self) -> AppResult<Vec<SchemaNode>> {
        let mut conn = self.conn().await;
        let mut nodes = vec![SchemaNode {
            name: "Keys".into(),
            kind: "schema".into(),
            schema: Some("keys".into()),
            table: None,
            children: vec![SchemaNode {
                name: "all".into(),
                kind: "table".into(),
                schema: Some("keys".into()),
                table: Some("all".into()),
                children: Vec::new(),
            }],
        }];
        let folders = tree_keys(&mut conn).await?;
        for (key_type, keys) in KEY_TYPES.into_iter().zip(folders) {
            if keys.is_empty() {
                continue;
            }
            nodes.push(SchemaNode {
                name: type_label(key_type).into(),
                kind: "schema".into(),
                schema: Some(key_type.into()),
                table: None,
                children: keys
                    .into_iter()
                    .map(|key| SchemaNode {
                        name: key.clone(),
                        kind: "key".into(),
                        schema: Some(key_type.into()),
                        table: Some(key),
                        children: Vec::new(),
                    })
                    .collect(),
            });
        }
        Ok(nodes)
    }

    pub async fn table_page(&self, request: &TablePageRequest) -> AppResult<TablePage> {
        let mut conn = self.conn().await;
        if request.schema == "keys" && request.table == "all" {
            return keys_table_page(&mut conn, request).await;
        }
        key_table_page(&mut conn, request).await
    }

    pub async fn run_query(&self, sql: &str, max_rows: Option<u32>) -> AppResult<QueryResponse> {
        let sql = sql.trim();
        if sql.is_empty() {
            return Err(AppError::InvalidInput("query is empty".into()));
        }
        let argv = tokenize_redis_cli(sql)?;
        let command = argv[0].to_ascii_lowercase();
        if is_unsupported_command(&command, &argv) {
            return Err(AppError::Unsupported(format!(
                "{command} is not supported in the Redis workbench"
            )));
        }
        if let Some(message) = connection_state_command_error(&command, &argv) {
            return Err(AppError::Unsupported(message.into()));
        }
        if self.profile.read_only && !is_read_command(&argv) {
            return Err(AppError::Unsupported("profile is read-only".into()));
        }
        let max_rows = max_rows
            .unwrap_or(DEFAULT_QUERY_ROWS)
            .clamp(1, DEFAULT_QUERY_ROWS);
        let mut cmd = Cmd::new();
        for arg in &argv {
            cmd.arg(arg);
        }
        let started = Instant::now();
        let mut conn = self.conn().await;
        let value: Value = cmd.query_async(&mut *conn).await?;
        Ok(value_to_query_response(
            &argv,
            value,
            usize::try_from(max_rows).unwrap_or(usize::MAX),
            started.elapsed().as_millis(),
        ))
    }

    pub async fn apply_mutations(&self, batch: &MutationBatch) -> AppResult<MutationResult> {
        if self.profile.read_only {
            return Err(AppError::Unsupported("profile is read-only".into()));
        }
        let mut conn = self.conn().await;
        if batch.schema == "keys" && batch.table == "all" {
            return apply_key_index_mutations(&mut conn, batch).await;
        }
        apply_key_mutations(&mut conn, batch).await
    }
}

/// `Ok(Err(_))` is a failed connection attempt; `Err(_)` is a profile problem,
/// such as an unreadable CA file, that retrying without TLS cannot fix.
type ConnectAttempt = AppResult<redis_rs::RedisResult<MultiplexedConnection>>;

async fn connect_client(
    profile: &ConnectionProfile,
    password: Option<&str>,
) -> AppResult<MultiplexedConnection> {
    if matches!(profile.tls_mode, TlsMode::Disabled) {
        return Ok(open_connection(profile, password, false).await??);
    }
    let attempt = open_connection(profile, password, true);
    let attempt = if matches!(profile.tls_mode, TlsMode::Preferred) {
        // A plaintext Redis server can read a TLS ClientHello as a partial
        // inline command and wait for more input, so the handshake would
        // never finish. Bound it, leaving the plaintext retry time within
        // the overall connect timeout.
        tokio::time::timeout(PREFERRED_TLS_TIMEOUT, attempt)
            .await
            .unwrap_or_else(|_| {
                Ok(Err(redis_rs::RedisError::from(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "TLS handshake timed out",
                ))))
            })
    } else {
        attempt.await
    };
    let tls_error = match attempt? {
        Ok(connection) => return Ok(connection),
        Err(error) => error,
    };
    // Preferred TLS only falls back when the TLS transport failed. A server
    // reply such as WRONGPASS or NOAUTH arrived over working TLS, and
    // retrying in plaintext would only replace it with a less useful error.
    if !matches!(profile.tls_mode, TlsMode::Preferred) || !tls_error.is_io_error() {
        return Err(tls_error.into());
    }
    match open_connection(profile, password, false).await? {
        Ok(connection) => Ok(connection),
        Err(plain_error) if plain_error.is_io_error() => Err(AppError::Database(format!(
            "TLS connection failed: {tls_error}; plaintext fallback failed: {plain_error}"
        ))),
        Err(plain_error) => Err(plain_error.into()),
    }
}

async fn open_connection(
    profile: &ConnectionProfile,
    password: Option<&str>,
    tls: bool,
) -> ConnectAttempt {
    let info = connection_info(profile, password, tls)?;
    #[cfg(not(target_os = "android"))]
    let connection = match profile.ca_cert_path.as_deref().filter(|_| tls) {
        Some(path) => {
            let connector = native_tls_connector(path)?;
            connect_native_tls(&profile.host, profile.port, &info.redis, connector).await
        }
        None => Client::open(info)?.get_multiplexed_async_connection().await,
    };
    #[cfg(target_os = "android")]
    let connection = if tls {
        let path = profile.ca_cert_path.as_deref().ok_or_else(|| {
            AppError::InvalidInput("Android TLS requires the platform CA bundle".into())
        })?;
        let roots = std::fs::read(path).map_err(|error| {
            AppError::InvalidInput(format!("could not read CA bundle: {error}"))
        })?;
        Client::build_with_tls(
            info,
            redis_rs::TlsCertificates {
                client_tls: None,
                root_cert: Some(roots),
            },
        )?
        .get_multiplexed_async_connection()
        .await
    } else {
        Client::open(info)?.get_multiplexed_async_connection().await
    };
    let mut connection = match connection {
        Ok(connection) => connection,
        Err(error) => return Ok(Err(error)),
    };
    Ok(redis_rs::cmd("PING")
        .query_async::<String>(&mut connection)
        .await
        .map(|_| connection))
}

/// redis-rs's native-tls transport cannot take extra root certificates, so
/// profiles with a CA file connect their own TLS stream, verified against the
/// system roots plus that CA, like PostgreSQL profiles.
#[cfg(not(target_os = "android"))]
fn native_tls_connector(path: &str) -> AppResult<native_tls::TlsConnector> {
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
    let mut connector = native_tls::TlsConnector::builder();
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
    connector
        .build()
        .map_err(|error| AppError::InvalidInput(format!("could not use CA certificate: {error}")))
}

#[cfg(not(target_os = "android"))]
async fn connect_native_tls(
    host: &str,
    port: u16,
    redis: &RedisConnectionInfo,
    connector: native_tls::TlsConnector,
) -> redis_rs::RedisResult<MultiplexedConnection> {
    let tcp = tokio::net::TcpStream::connect((host, port)).await?;
    tcp.set_nodelay(true)?;
    let stream = tokio_native_tls::TlsConnector::from(connector)
        .connect(host, tcp)
        .await?;
    let (connection, driver) = MultiplexedConnection::new_with_config(
        redis,
        stream,
        redis_rs::AsyncConnectionConfig::new(),
    )
    .await?;
    tokio::spawn(driver);
    Ok(connection)
}

fn connection_info(
    profile: &ConnectionProfile,
    password: Option<&str>,
    tls: bool,
) -> AppResult<ConnectionInfo> {
    let db = parse_redis_database(&profile.default_database)?;
    let addr = if tls {
        ConnectionAddr::TcpTls {
            host: profile.host.clone(),
            port: profile.port,
            // Preferred TLS without a CA encrypts without verifying, as
            // before; a configured CA is always verified.
            insecure: matches!(profile.tls_mode, TlsMode::Preferred)
                && profile.ca_cert_path.is_none(),
            tls_params: None,
        }
    } else {
        ConnectionAddr::Tcp(profile.host.clone(), profile.port)
    };
    let mut redis = RedisConnectionInfo {
        db,
        username: None,
        // Passwords are sent exactly as entered; spaces can be part of one.
        password: password
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        protocol: redis_rs::ProtocolVersion::RESP2,
    };
    let username = profile.username.trim();
    if !username.is_empty() && !username.eq_ignore_ascii_case("default") {
        redis.username = Some(username.to_owned());
    }
    Ok(ConnectionInfo { addr, redis })
}

async fn cluster_enabled(conn: &mut MultiplexedConnection) -> AppResult<bool> {
    let info: Result<String, _> = redis_rs::cmd("INFO").arg("cluster").query_async(conn).await;
    Ok(info.is_ok_and(|value| {
        value
            .lines()
            .any(|line| line.trim().eq_ignore_ascii_case("cluster_enabled:1"))
    }))
}

async fn database_count(conn: &mut MultiplexedConnection) -> AppResult<i64> {
    let values: Result<Vec<String>, _> = redis_rs::cmd("CONFIG")
        .arg("GET")
        .arg("databases")
        .query_async(conn)
        .await;
    Ok(values
        .ok()
        .and_then(|values| values.get(1)?.parse::<i64>().ok())
        .filter(|count| *count > 0)
        .unwrap_or(16))
}

/// One SCAN pass that fills every sidebar folder with up to
/// `TREE_KEYS_PER_TYPE` keys, sorted. It stops once every folder is full or
/// the keyspace is exhausted, instead of scanning once per type.
async fn tree_keys(
    conn: &mut MultiplexedConnection,
) -> AppResult<[BTreeSet<String>; KEY_TYPES.len()]> {
    let mut folders: [BTreeSet<String>; KEY_TYPES.len()] = Default::default();
    let mut cursor = 0;
    loop {
        let (next, keys) = scan_batch(conn, cursor, "*", None).await?;
        cursor = next;
        let types = key_types(conn, &keys).await?;
        for (key, key_type) in keys.into_iter().zip(types) {
            if let Some(folder) = KEY_TYPES
                .iter()
                .position(|known| *known == key_type)
                .map(|index| &mut folders[index])
                && folder.len() < TREE_KEYS_PER_TYPE
            {
                folder.insert(key);
            }
        }
        if cursor == 0
            || folders
                .iter()
                .all(|folder| folder.len() >= TREE_KEYS_PER_TYPE)
        {
            return Ok(folders);
        }
    }
}

/// Scans every key matching `pattern`. `key_type` is pushed down as
/// `SCAN … TYPE` when the server supports it (Redis 6+); older servers return
/// keys of every type, so callers must still filter rows by type.
async fn scan_keys(
    conn: &mut MultiplexedConnection,
    pattern: &str,
    mut key_type: Option<&str>,
) -> AppResult<Vec<String>> {
    let mut cursor = 0;
    let mut keys = Vec::new();
    loop {
        match scan_batch(conn, cursor, pattern, key_type).await {
            Ok((next, batch)) => {
                cursor = next;
                keys.extend(batch);
                if cursor == 0 {
                    return Ok(keys);
                }
            }
            Err(error) if key_type.is_some() && is_scan_type_unsupported(&error) => {
                key_type = None;
                cursor = 0;
                keys.clear();
            }
            Err(error) => return Err(error.into()),
        }
    }
}

async fn scan_batch(
    conn: &mut MultiplexedConnection,
    cursor: u64,
    pattern: &str,
    key_type: Option<&str>,
) -> redis_rs::RedisResult<(u64, Vec<String>)> {
    let mut cmd = redis_rs::cmd("SCAN");
    cmd.arg(cursor)
        .arg("MATCH")
        .arg(pattern)
        .arg("COUNT")
        .arg(SCAN_COUNT);
    if let Some(key_type) = key_type {
        cmd.arg("TYPE").arg(key_type);
    }
    let (next, keys) = cmd.query_async::<(u64, Vec<Vec<u8>>)>(conn).await?;
    Ok((next, utf8_key_names(keys).collect()))
}

/// Redis before 6.0 rejects `SCAN … TYPE` as `ERR syntax error`; some newer
/// servers reject unknown type names. Other failures, such as lost
/// connections or ACL denials, are real errors.
fn is_scan_type_unsupported(error: &redis_rs::RedisError) -> bool {
    error.code() == Some("ERR")
        && error.detail().is_some_and(|detail| {
            let detail = detail.to_ascii_lowercase();
            detail.contains("syntax") || detail.contains("type")
        })
}

async fn key_types(conn: &mut MultiplexedConnection, keys: &[String]) -> AppResult<Vec<String>> {
    let mut types = Vec::with_capacity(keys.len());
    for chunk in keys.chunks(PIPELINE_KEYS) {
        let mut pipe = redis_rs::pipe();
        for key in chunk {
            pipe.cmd("TYPE").arg(key);
        }
        let values: Vec<Value> = pipe.query_async(conn).await?;
        types.extend(values.into_iter().map(value_as_string));
    }
    Ok(types)
}

async fn keys_table_page(
    conn: &mut MultiplexedConnection,
    request: &TablePageRequest,
) -> AppResult<TablePage> {
    let metadata = key_index_metadata();
    let limit = request.limit.clamp(1, MAX_PAGE_SIZE);
    let pattern = scan_pattern(&request.filters);
    let type_filter = type_filter(&request.filters);
    // Offset pagination needs the complete keyspace in a deterministic order.
    // SCAN can repeat keys, so deduplicate before counting or slicing.
    let mut keys = scan_keys(conn, &pattern, type_filter.as_deref()).await?;
    keys.sort_unstable();
    keys.dedup();
    let offset = usize::try_from(request.offset).unwrap_or(0);
    let take = usize::try_from(limit).unwrap_or(0);
    let key_order = request
        .order_by
        .as_ref()
        .is_none_or(|order| order.column == "key");
    let (page_rows, total) = if request.filters.is_empty() && key_order {
        // The common path only needs TYPE/TTL for the keys on this page.
        let total = keys.len();
        if request
            .order_by
            .as_ref()
            .is_some_and(|order| order.descending)
        {
            keys.reverse();
        }
        let keys: Vec<_> = keys.into_iter().skip(offset).take(take).collect();
        (key_index_rows(conn, &keys).await?, total)
    } else {
        let mut rows = key_index_rows(conn, &keys).await?;
        rows.retain(|row| row_matches(&metadata, row, &request.filters));
        sort_rows(&metadata, &mut rows, request.order_by.as_ref())?;
        let total = rows.len();
        (rows.into_iter().skip(offset).take(take).collect(), total)
    };
    let total_rows = request
        .include_total
        .unwrap_or(true)
        .then_some(total as u64);
    let has_more = offset.saturating_add(take) < total;
    Ok(TablePage {
        metadata,
        columns: vec!["key".into(), "type".into(), "ttl".into()],
        rows: page_rows,
        total_rows,
        offset: request.offset,
        limit,
        has_more,
    })
}

async fn key_index_rows(
    conn: &mut MultiplexedConnection,
    keys: &[String],
) -> AppResult<Vec<Vec<JsonValue>>> {
    let mut rows = Vec::with_capacity(keys.len());
    for chunk in keys.chunks(PIPELINE_KEYS) {
        let mut pipe = redis_rs::pipe();
        for key in chunk {
            pipe.cmd("TYPE").arg(key);
            pipe.cmd("TTL").arg(key);
        }
        let values: Vec<Value> = pipe.query_async(conn).await?;
        for (index, key) in chunk.iter().enumerate() {
            let key_type = values
                .get(index * 2)
                .cloned()
                .map(value_as_string)
                .unwrap_or_else(|| "none".into());
            let ttl = values
                .get(index * 2 + 1)
                .cloned()
                .and_then(value_as_i64)
                .unwrap_or(-2);
            if key_type == "none" {
                continue;
            }
            rows.push(vec![
                JsonValue::String(key.clone()),
                JsonValue::String(key_type),
                JsonValue::Number(ttl.into()),
            ]);
        }
    }
    Ok(rows)
}

async fn key_table_page(
    conn: &mut MultiplexedConnection,
    request: &TablePageRequest,
) -> AppResult<TablePage> {
    let key_type = inspect_type(conn, &request.table).await?;
    if key_type == "none" {
        return Err(AppError::InvalidInput(format!(
            "key {} was not found",
            request.table
        )));
    }
    if request.schema != key_type && request.schema != "keys" {
        return Err(AppError::InvalidInput(format!(
            "key {} is a {key_type}, not a {}",
            request.table, request.schema
        )));
    }
    let metadata = key_metadata(&key_type, &request.table);
    let limit = request.limit.clamp(1, MAX_PAGE_SIZE);
    let offset = usize::try_from(request.offset).unwrap_or(0);
    let take = usize::try_from(limit).unwrap_or(0);
    let (page_rows, total, has_more) = if key_type == "list"
        && request.filters.is_empty()
        && request
            .order_by
            .as_ref()
            .is_none_or(|order| order.column == "index" && !order.descending)
    {
        // Ordinary list browsing needs only this slice, not LRANGE 0 -1.
        // Fetch one extra element to detect another page, keeping absolute
        // indices so edits still address the original list positions.
        let (total, values): (u64, Vec<Vec<u8>>) = redis_rs::pipe()
            .cmd("LLEN")
            .arg(&request.table)
            .cmd("LRANGE")
            .arg(&request.table)
            .arg(request.offset)
            .arg(u64::from(request.offset) + u64::from(limit))
            .query_async(conn)
            .await?;
        let has_more = values.len() > take;
        let rows = values
            .into_iter()
            .take(take)
            .enumerate()
            .map(|(index, value)| {
                vec![
                    JsonValue::from(u64::from(request.offset) + index as u64),
                    bulk_to_json(&value),
                ]
            })
            .collect();
        (rows, total, has_more)
    } else {
        let mut rows = load_key_rows(conn, &key_type, &request.table).await?;
        rows.retain(|row| row_matches(&metadata, row, &request.filters));
        sort_rows(&metadata, &mut rows, request.order_by.as_ref())?;
        let total = rows.len() as u64;
        let has_more = u64::from(request.offset) + u64::from(limit) < total;
        (
            rows.into_iter().skip(offset).take(take).collect(),
            total,
            has_more,
        )
    };
    let total_rows = request.include_total.unwrap_or(true).then_some(total);
    let columns = metadata
        .columns
        .iter()
        .map(|column| column.name.clone())
        .collect();
    Ok(TablePage {
        metadata,
        columns,
        rows: page_rows,
        total_rows,
        offset: request.offset,
        limit,
        has_more,
    })
}

async fn inspect_type(conn: &mut MultiplexedConnection, key: &str) -> AppResult<String> {
    let key_type: String = redis_rs::cmd("TYPE").arg(key).query_async(conn).await?;
    Ok(key_type)
}

async fn load_key_rows(
    conn: &mut MultiplexedConnection,
    key_type: &str,
    key: &str,
) -> AppResult<Vec<Vec<JsonValue>>> {
    match key_type {
        "string" => {
            let value: Value = redis_rs::cmd("GET").arg(key).query_async(conn).await?;
            Ok(vec![vec![
                JsonValue::String(key.to_owned()),
                json_from_redis(value),
            ]])
        }
        "hash" => {
            let values: Vec<Vec<u8>> = redis_rs::cmd("HGETALL").arg(key).query_async(conn).await?;
            Ok(values
                .chunks(2)
                .filter_map(|chunk| {
                    Some(vec![
                        bulk_to_json(chunk.first()?),
                        bulk_to_json(chunk.get(1)?),
                    ])
                })
                .collect())
        }
        "list" => {
            let values: Vec<Vec<u8>> = redis_rs::cmd("LRANGE")
                .arg(key)
                .arg(0)
                .arg(-1)
                .query_async(conn)
                .await?;
            Ok(values
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    vec![
                        JsonValue::Number(i64::try_from(index).unwrap_or(i64::MAX).into()),
                        bulk_to_json(&value),
                    ]
                })
                .collect())
        }
        "set" => {
            let values: Vec<Vec<u8>> = redis_rs::cmd("SMEMBERS").arg(key).query_async(conn).await?;
            Ok(values
                .iter()
                .map(|member| vec![bulk_to_json(member)])
                .collect())
        }
        "zset" => {
            let values: Vec<Vec<u8>> = redis_rs::cmd("ZRANGE")
                .arg(key)
                .arg(0)
                .arg(-1)
                .arg("WITHSCORES")
                .query_async(conn)
                .await?;
            Ok(values
                .chunks(2)
                .filter_map(|chunk| {
                    Some(vec![
                        bulk_to_json(chunk.first()?),
                        score_json(&String::from_utf8_lossy(chunk.get(1)?)),
                    ])
                })
                .collect())
        }
        "stream" => {
            let entries: Vec<Value> = redis_rs::cmd("XRANGE")
                .arg(key)
                .arg("-")
                .arg("+")
                .query_async(conn)
                .await?;
            Ok(entries
                .into_iter()
                .filter_map(|entry| {
                    let Value::Array(parts) = entry else {
                        return None;
                    };
                    let id = json_from_redis(parts.first()?.clone());
                    let fields = parts.get(1).cloned().unwrap_or(Value::Nil);
                    Some(vec![id, json_from_redis(fields)])
                })
                .collect())
        }
        other => Err(AppError::Unsupported(format!(
            "Redis type {other} is not supported in the table viewer"
        ))),
    }
}

enum KeyIndexAction {
    Delete,
    Expire(i64),
    Persist,
    Check,
}

async fn apply_key_index_mutations(
    conn: &mut MultiplexedConnection,
    batch: &MutationBatch,
) -> AppResult<MutationResult> {
    // Validate the whole batch before writing so one bad row cannot leave it
    // partly applied.
    let mut actions = Vec::with_capacity(batch.mutations.len());
    for mutation in &batch.mutations {
        let key = json_to_string(mutation.primary_key.first().unwrap_or(&JsonValue::Null));
        if key.is_empty() {
            return Err(AppError::InvalidInput("Redis key is required".into()));
        }
        let action = if mutation.deleted {
            KeyIndexAction::Delete
        } else if mutation.original.get(1) != mutation.changes.get(1) {
            return Err(AppError::Unsupported(
                "Redis key types cannot be changed from the table editor".into(),
            ));
        } else if mutation.original.get(2) != mutation.changes.get(2) {
            match parse_ttl(mutation.changes.get(2))? {
                Some(seconds) => KeyIndexAction::Expire(seconds),
                None => KeyIndexAction::Persist,
            }
        } else {
            KeyIndexAction::Check
        };
        actions.push((key, action));
    }
    // One round trip. Each row's counted reply says whether its key still
    // existed; PERSIST also returns 0 for keys without a TTL, so EXISTS
    // decides those rows.
    let mut pipe = redis_rs::pipe();
    for (key, action) in &actions {
        match action {
            KeyIndexAction::Delete => pipe.cmd("DEL").arg(key),
            KeyIndexAction::Expire(seconds) => pipe.cmd("EXPIRE").arg(key).arg(seconds),
            KeyIndexAction::Persist => pipe.cmd("EXISTS").arg(key).cmd("PERSIST").arg(key).ignore(),
            KeyIndexAction::Check => pipe.cmd("EXISTS").arg(key),
        };
    }
    let found = row_replies(conn, &pipe).await?;
    Ok(mutation_result(batch.mutations.iter(), found))
}

async fn apply_key_mutations(
    conn: &mut MultiplexedConnection,
    batch: &MutationBatch,
) -> AppResult<MutationResult> {
    let key_type = inspect_type(conn, &batch.table).await?;
    if key_type == "none" {
        return Ok(MutationResult {
            applied: 0,
            conflicts: batch
                .mutations
                .iter()
                .map(|mutation| mutation.primary_key.clone())
                .collect(),
        });
    }
    if batch.schema != key_type {
        return Err(AppError::InvalidInput(format!(
            "key {} is a {key_type}, not a {}",
            batch.table, batch.schema
        )));
    }
    for mutation in &batch.mutations {
        ensure_lossless_edit(&key_type, mutation)?;
    }
    let key = batch.table.as_str();
    let mut mutations: Vec<_> = batch.mutations.iter().collect();
    let applied = match key_type.as_str() {
        "string" => apply_string_mutations(conn, key, &mutations).await?,
        "hash" => apply_hash_mutations(conn, key, &mutations).await?,
        "list" => {
            // A deletion shifts every later index. Apply higher original
            // indices first, including updates, so none of this batch targets
            // a shifted row.
            let mut indexed = mutations
                .into_iter()
                .map(|mutation| {
                    let index =
                        json_to_i64(mutation.primary_key.first().unwrap_or(&JsonValue::Null))?;
                    Ok((index, mutation))
                })
                .collect::<AppResult<Vec<_>>>()?;
            indexed.sort_by_key(|(index, _)| std::cmp::Reverse(*index));
            mutations = indexed.into_iter().map(|(_, mutation)| mutation).collect();
            // Index-addressed edits stay one row at a time: an out-of-range
            // LSET must be a conflict for that row, but an error reply fails
            // a whole redis-rs pipeline.
            let mut applied = Vec::with_capacity(mutations.len());
            for mutation in &mutations {
                applied.push(apply_list_mutation(conn, key, mutation).await?);
            }
            applied
        }
        "set" => apply_set_mutations(conn, key, &mutations).await?,
        "zset" => apply_zset_mutations(conn, key, &mutations).await?,
        "stream" => apply_stream_mutations(conn, key, &mutations).await?,
        other => {
            return Err(AppError::Unsupported(format!(
                "Redis type {other} cannot be edited"
            )));
        }
    };
    Ok(mutation_result(mutations, applied))
}

/// Counts applied rows and reports the rest as conflicts, in batch order.
fn mutation_result<'a>(
    mutations: impl IntoIterator<Item = &'a RowMutation>,
    applied: impl IntoIterator<Item = bool>,
) -> MutationResult {
    let mut result = MutationResult {
        applied: 0,
        conflicts: Vec::new(),
    };
    for (mutation, applied) in mutations.into_iter().zip(applied) {
        if applied {
            result.applied += 1;
        } else {
            result.conflicts.push(mutation.primary_key.clone());
        }
    }
    result
}

/// Runs a pipeline whose non-ignored replies each decide one row: nil or a
/// zero count means the key, field, or member was missing.
async fn row_replies(
    conn: &mut MultiplexedConnection,
    pipe: &redis_rs::Pipeline,
) -> AppResult<Vec<bool>> {
    if pipe.is_empty() {
        return Ok(Vec::new());
    }
    let replies: Vec<Value> = pipe.query_async(conn).await?;
    Ok(row_flags(&replies))
}

/// Whether each reply applied its row: `nil` and zero counts did not.
fn row_flags(replies: &[Value]) -> Vec<bool> {
    replies
        .iter()
        .map(|reply| match reply {
            Value::Nil => false,
            Value::Int(count) => *count > 0,
            _ => true,
        })
        .collect()
}

/// Table views show binary data as `\x…` hex and cut long values at
/// `MAX_VALUE_BYTES`. Saving that preview would replace the real data, and a
/// preview field or member name would address a different one, so such rows
/// cannot be saved. Literal text of the same shape is refused as well, since
/// the preview alone cannot tell them apart. Deleting a key, list element, or
/// field whose value is a preview is still allowed.
fn ensure_lossless_edit(key_type: &str, mutation: &RowMutation) -> AppResult<()> {
    let name_is_preview = matches!(key_type, "hash" | "set" | "zset")
        && mutation.primary_key.first().is_some_and(is_lossy_preview);
    let value_is_preview = !mutation.deleted
        && matches!(key_type, "string" | "hash" | "list")
        && mutation.original.get(1).is_some_and(is_lossy_preview);
    if name_is_preview || value_is_preview {
        return Err(AppError::Unsupported(
            "values shown as hex (binary data) or truncated cannot be saved from the table editor without losing data".into(),
        ));
    }
    Ok(())
}

fn is_lossy_preview(value: &JsonValue) -> bool {
    let JsonValue::String(text) = value else {
        return false;
    };
    text.ends_with(TRUNCATION_MARKER)
        || text.strip_prefix("\\x").is_some_and(|hex| {
            !hex.is_empty()
                && hex.len() % 2 == 0
                && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

async fn apply_string_mutations(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutations: &[&RowMutation],
) -> AppResult<Vec<bool>> {
    let pipeline = |keep_ttl: bool| {
        let mut pipe = redis_rs::pipe();
        for mutation in mutations {
            if mutation.deleted {
                pipe.cmd("DEL").arg(key);
            } else {
                // XX only overwrites an existing key, so a key deleted since
                // the page loaded is a conflict instead of being recreated.
                // KEEPTTL (Redis 6.0+) keeps the key's expiry through the edit.
                let value = json_to_string(mutation.changes.get(1).unwrap_or(&JsonValue::Null));
                let set = pipe.cmd("SET").arg(key).arg(value).arg("XX");
                if keep_ttl {
                    set.arg("KEEPTTL");
                }
            }
        }
        pipe
    };
    if mutations.is_empty() {
        return Ok(Vec::new());
    }
    match pipeline(true).query_async::<Vec<Value>>(conn).await {
        Ok(replies) => Ok(row_flags(&replies)),
        // Servers before 6.0 reject KEEPTTL without writing. A delete in the
        // same pipeline has already run, so only pure edits are retried.
        Err(error)
            if is_syntax_error(&error) && mutations.iter().all(|mutation| !mutation.deleted) =>
        {
            row_replies(conn, &pipeline(false)).await
        }
        Err(error) => Err(error.into()),
    }
}

fn is_syntax_error(error: &redis_rs::RedisError) -> bool {
    error.code() == Some("ERR")
        && error
            .detail()
            .is_some_and(|detail| detail.to_ascii_lowercase().contains("syntax"))
}

async fn apply_hash_mutations(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutations: &[&RowMutation],
) -> AppResult<Vec<bool>> {
    let field = |mutation: &RowMutation| {
        json_to_string(mutation.primary_key.first().unwrap_or(&JsonValue::Null))
    };
    // HSET would create a missing field, so updated fields are checked first,
    // all in one round trip. Deletes are decided by HDEL's count.
    let mut check = redis_rs::pipe();
    for mutation in mutations.iter().filter(|mutation| !mutation.deleted) {
        check.cmd("HEXISTS").arg(key).arg(field(mutation));
    }
    let mut exists = row_replies(conn, &check).await?.into_iter();
    let mut write = redis_rs::pipe();
    let mut applied = Vec::with_capacity(mutations.len());
    for mutation in mutations {
        if mutation.deleted {
            write.cmd("HDEL").arg(key).arg(field(mutation));
            applied.push(None);
        } else if exists.next() == Some(true) {
            let value = json_to_string(mutation.changes.get(1).unwrap_or(&JsonValue::Null));
            write
                .cmd("HSET")
                .arg(key)
                .arg(field(mutation))
                .arg(value)
                .ignore();
            applied.push(Some(true));
        } else {
            applied.push(Some(false));
        }
    }
    let mut deleted = row_replies(conn, &write).await?.into_iter();
    Ok(applied
        .into_iter()
        .map(|applied| applied.unwrap_or_else(|| deleted.next().unwrap_or(false)))
        .collect())
}

async fn apply_list_mutation(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutation: &RowMutation,
) -> AppResult<bool> {
    let index = json_to_i64(mutation.primary_key.first().unwrap_or(&JsonValue::Null))?;
    let current: Value = redis_rs::cmd("LINDEX")
        .arg(key)
        .arg(index)
        .query_async(conn)
        .await?;
    if matches!(current, Value::Nil) {
        return Ok(false);
    }
    if mutation.deleted {
        let sentinel = format!("dbm-deleted-{}", Uuid::new_v4());
        if redis_rs::cmd("LSET")
            .arg(key)
            .arg(index)
            .arg(&sentinel)
            .query_async::<Value>(conn)
            .await
            .is_err()
        {
            return Ok(false);
        }
        let _: i64 = redis_rs::cmd("LREM")
            .arg(key)
            .arg(1)
            .arg(&sentinel)
            .query_async(conn)
            .await?;
        return Ok(true);
    }
    let value = json_to_string(mutation.changes.get(1).unwrap_or(&JsonValue::Null));
    redis_rs::cmd("LSET")
        .arg(key)
        .arg(index)
        .arg(value)
        .query_async::<Value>(conn)
        .await
        .map_err(|_| AppError::Database("list index is out of range".into()))?;
    Ok(true)
}

async fn apply_set_mutations(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutations: &[&RowMutation],
) -> AppResult<Vec<bool>> {
    let mut pipe = redis_rs::pipe();
    for mutation in mutations {
        let member = json_to_string(mutation.primary_key.first().unwrap_or(&JsonValue::Null));
        // A set row has only its member, so a non-delete only confirms it
        // still exists.
        let command = if mutation.deleted {
            "SREM"
        } else {
            "SISMEMBER"
        };
        pipe.cmd(command).arg(key).arg(member);
    }
    row_replies(conn, &pipe).await
}

async fn apply_zset_mutations(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutations: &[&RowMutation],
) -> AppResult<Vec<bool>> {
    let mut pipe = redis_rs::pipe();
    for mutation in mutations {
        let member = json_to_string(mutation.primary_key.first().unwrap_or(&JsonValue::Null));
        if mutation.deleted {
            pipe.cmd("ZREM").arg(key).arg(member);
            continue;
        }
        let score = json_to_string(mutation.changes.get(1).unwrap_or(&JsonValue::Null));
        if score.parse::<f64>().map_or(true, f64::is_nan) {
            return Err(AppError::InvalidInput(format!(
                "sorted set score {score:?} is not a number"
            )));
        }
        // ZSCORE decides the row; ZADD XX never creates a missing member.
        pipe.cmd("ZSCORE")
            .arg(key)
            .arg(&member)
            .cmd("ZADD")
            .arg(key)
            .arg("XX")
            .arg(score)
            .arg(&member)
            .ignore();
    }
    row_replies(conn, &pipe).await
}

async fn apply_stream_mutations(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutations: &[&RowMutation],
) -> AppResult<Vec<bool>> {
    if mutations.iter().any(|mutation| !mutation.deleted) {
        return Err(AppError::Unsupported(
            "stream entries can only be deleted from the table editor".into(),
        ));
    }
    let mut pipe = redis_rs::pipe();
    for mutation in mutations {
        let id = json_to_string(mutation.primary_key.first().unwrap_or(&JsonValue::Null));
        pipe.cmd("XDEL").arg(key).arg(id);
    }
    row_replies(conn, &pipe).await
}

/// Parses an edited TTL cell. Blank or negative means no expiry, matching
/// the -1 Redis reports for persistent keys. Zero is refused because
/// `EXPIRE key 0` deletes the key; deletes use the explicit delete action.
fn parse_ttl(ttl: Option<&JsonValue>) -> AppResult<Option<i64>> {
    let text = ttl.map(json_to_string).unwrap_or_default();
    if text.is_empty() {
        return Ok(None);
    }
    let seconds = text
        .parse::<i64>()
        .map_err(|_| AppError::InvalidInput("TTL must be an integer number of seconds".into()))?;
    match seconds {
        0 => Err(AppError::InvalidInput(
            "TTL 0 would delete the key; use -1 to remove the expiry, or delete the key".into(),
        )),
        seconds if seconds < 0 => Ok(None),
        seconds => Ok(Some(seconds)),
    }
}

fn key_index_metadata() -> TableMetadata {
    TableMetadata {
        schema: "keys".into(),
        table: "all".into(),
        columns: vec![
            column("key", "string", 1, false),
            column("type", "string", 2, false),
            column("ttl", "integer", 3, false),
        ],
        primary_key: vec!["key".into()],
        has_xmin: false,
    }
}

fn key_metadata(key_type: &str, key: &str) -> TableMetadata {
    let (columns, primary_key) = match key_type {
        "string" => (
            vec![
                column("key", "string", 1, false),
                column("value", "string", 2, true),
            ],
            vec!["key".into()],
        ),
        "hash" => (
            vec![
                column("field", "string", 1, false),
                column("value", "string", 2, true),
            ],
            vec!["field".into()],
        ),
        "list" => (
            vec![
                column("index", "integer", 1, false),
                column("value", "string", 2, true),
            ],
            vec!["index".into()],
        ),
        "set" => (
            vec![column("member", "string", 1, false)],
            vec!["member".into()],
        ),
        "zset" => (
            vec![
                column("member", "string", 1, false),
                column("score", "number", 2, false),
            ],
            vec!["member".into()],
        ),
        _ => (
            vec![
                column("id", "string", 1, false),
                column("fields", "string", 2, true),
            ],
            vec!["id".into()],
        ),
    };
    TableMetadata {
        schema: key_type.to_owned(),
        table: key.to_owned(),
        columns,
        primary_key,
        has_xmin: false,
    }
}

fn column(name: &str, data_type: &str, ordinal: i32, nullable: bool) -> TableColumn {
    TableColumn {
        name: name.into(),
        data_type: data_type.into(),
        nullable,
        default_value: None,
        ordinal,
    }
}

fn type_label(key_type: &str) -> &'static str {
    match key_type {
        "string" => "Strings",
        "hash" => "Hashes",
        "list" => "Lists",
        "set" => "Sets",
        "zset" => "Sorted sets",
        "stream" => "Streams",
        _ => "Keys",
    }
}

/// A SCAN MATCH pattern that every key passing the first `key` filter
/// matches. It only narrows the scan: `row_matches` still applies every filter,
/// so operators SCAN cannot express scan the whole keyspace.
fn scan_pattern(filters: &[FilterCondition]) -> String {
    let Some(filter) = filters.iter().find(|filter| filter.column == "key") else {
        return "*".into();
    };
    let value = filter.value.as_deref().unwrap_or_default();
    match filter.operator {
        FilterOperator::Equals => glob_literal(value, false),
        // These match case-insensitively in `row_matches`, but SCAN MATCH is
        // case-sensitive, so letters become `[aA]` classes.
        FilterOperator::Contains => format!("*{}*", glob_literal(value, true)),
        FilterOperator::StartsWith => format!("{}*", glob_literal(value, true)),
        FilterOperator::EndsWith => format!("*{}", glob_literal(value, true)),
        _ => "*".into(),
    }
}

/// Escapes Redis glob syntax so `value` matches only itself.
fn glob_literal(value: &str, ascii_case_insensitive: bool) -> String {
    let mut pattern = String::with_capacity(value.len());
    for character in value.chars() {
        if ascii_case_insensitive && character.is_ascii_alphabetic() {
            pattern.push('[');
            pattern.push(character.to_ascii_lowercase());
            pattern.push(character.to_ascii_uppercase());
            pattern.push(']');
        } else {
            if matches!(character, '*' | '?' | '[' | ']' | '\\') {
                pattern.push('\\');
            }
            pattern.push(character);
        }
    }
    pattern
}

fn type_filter(filters: &[FilterCondition]) -> Option<String> {
    filters.iter().find_map(|filter| {
        if filter.column == "type" && matches!(filter.operator, FilterOperator::Equals) {
            filter.value.clone()
        } else {
            None
        }
    })
}

fn row_matches(metadata: &TableMetadata, row: &[JsonValue], filters: &[FilterCondition]) -> bool {
    filters.iter().all(|filter| {
        let Some(index) = metadata
            .columns
            .iter()
            .position(|column| column.name == filter.column)
        else {
            return false;
        };
        let cell = row.get(index).unwrap_or(&JsonValue::Null);
        let cell_text = json_to_string(cell);
        let value = filter.value.as_deref().unwrap_or_default();
        match filter.operator {
            FilterOperator::Equals => cell_text == value,
            FilterOperator::NotEquals => cell_text != value,
            FilterOperator::Contains => cell_text
                .to_ascii_lowercase()
                .contains(&value.to_ascii_lowercase()),
            FilterOperator::StartsWith => cell_text
                .to_ascii_lowercase()
                .starts_with(&value.to_ascii_lowercase()),
            FilterOperator::EndsWith => cell_text
                .to_ascii_lowercase()
                .ends_with(&value.to_ascii_lowercase()),
            FilterOperator::GreaterThan => compare_json(cell, value) == Ordering::Greater,
            FilterOperator::GreaterThanOrEqual => compare_json(cell, value) != Ordering::Less,
            FilterOperator::LessThan => compare_json(cell, value) == Ordering::Less,
            FilterOperator::LessThanOrEqual => compare_json(cell, value) != Ordering::Greater,
            FilterOperator::In => value
                .split(',')
                .map(str::trim)
                .any(|item| item == cell_text),
            FilterOperator::NotIn => value
                .split(',')
                .map(str::trim)
                .all(|item| item != cell_text),
            FilterOperator::IsNull => matches!(cell, JsonValue::Null),
            FilterOperator::IsNotNull => !matches!(cell, JsonValue::Null),
        }
    })
}

fn sort_rows(
    metadata: &TableMetadata,
    rows: &mut [Vec<JsonValue>],
    order_by: Option<&OrderSpec>,
) -> AppResult<()> {
    let Some(order_by) = order_by else {
        return Ok(());
    };
    let Some(index) = metadata
        .columns
        .iter()
        .position(|column| column.name == order_by.column)
    else {
        return Err(AppError::InvalidInput(format!(
            "unknown order column {}",
            order_by.column
        )));
    };
    rows.sort_by(|left, right| {
        let ordering = compare_cells(
            left.get(index).unwrap_or(&JsonValue::Null),
            right.get(index).unwrap_or(&JsonValue::Null),
        );
        if order_by.descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
    Ok(())
}

fn compare_json(cell: &JsonValue, value: &str) -> Ordering {
    if let (Some(left), Ok(right)) = (cell.as_i64(), value.parse::<i64>()) {
        return left.cmp(&right);
    }
    if let (Some(left), Ok(right)) = (cell.as_f64(), value.parse::<f64>()) {
        return left.partial_cmp(&right).unwrap_or(Ordering::Equal);
    }
    json_to_string(cell).cmp(&value.to_owned())
}

fn compare_cells(left: &JsonValue, right: &JsonValue) -> Ordering {
    if let (Some(left), Some(right)) = (left.as_i64(), right.as_i64()) {
        return left.cmp(&right);
    }
    if let (Some(left), Some(right)) = (left.as_f64(), right.as_f64()) {
        return left.partial_cmp(&right).unwrap_or(Ordering::Equal);
    }
    json_to_string(left).cmp(&json_to_string(right))
}

fn tokenize_redis_cli(input: &str) -> AppResult<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    while let Some(character) = chars.next() {
        match character {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            '\\' if in_double => match chars.next() {
                Some('n') => current.push('\n'),
                Some('r') => current.push('\r'),
                Some('t') => current.push('\t'),
                Some(escaped) => current.push(escaped),
                None => {
                    return Err(AppError::InvalidInput(
                        "Redis command has a trailing escape".into(),
                    ));
                }
            },
            character if character.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            character => current.push(character),
        }
    }
    if in_single || in_double {
        return Err(AppError::InvalidInput(
            "Redis command has an unterminated quote".into(),
        ));
    }
    if !current.is_empty() {
        args.push(current);
    }
    if args.is_empty() {
        return Err(AppError::InvalidInput("query is empty".into()));
    }
    Ok(args)
}

fn is_unsupported_command(command: &str, argv: &[String]) -> bool {
    matches!(
        command,
        "subscribe"
            | "psubscribe"
            | "unsubscribe"
            | "punsubscribe"
            | "ssubscribe"
            | "monitor"
            | "blpop"
            | "brpop"
            | "brpoplpush"
            | "blmove"
            | "blmpop"
            | "bzpopmin"
            | "bzpopmax"
            | "bzmpop"
    ) || (matches!(command, "xread" | "xreadgroup")
        && argv.iter().any(|arg| arg.eq_ignore_ascii_case("block")))
}

/// Commands that would change the shared connection's state out from under the
/// sidebar and table views, which assume the selected database and RESP2.
fn connection_state_command_error(command: &str, argv: &[String]) -> Option<&'static str> {
    match command {
        "select" => Some(
            "SELECT is not supported in the Redis workbench; choose the database index in the sidebar instead",
        ),
        "hello" if argv.len() > 1 => {
            Some("HELLO with arguments is not supported in the Redis workbench")
        }
        "quit" | "reset" => Some("QUIT and RESET are not supported in the Redis workbench"),
        "client"
            if argv
                .get(1)
                .is_some_and(|sub| sub.eq_ignore_ascii_case("reply")) =>
        {
            Some("CLIENT REPLY is not supported in the Redis workbench")
        }
        _ => None,
    }
}

fn is_read_command(argv: &[String]) -> bool {
    let command = argv[0].to_ascii_lowercase();
    let sub = argv.get(1).map(|value| value.to_ascii_lowercase());
    match command.as_str() {
        "config" => sub.as_deref() == Some("get"),
        "acl" => matches!(
            sub.as_deref(),
            Some("cat" | "genpass" | "getuser" | "help" | "list" | "log" | "whoami")
        ),
        "client" => matches!(sub.as_deref(), Some("getname" | "id" | "info" | "list")),
        "memory" => matches!(
            sub.as_deref(),
            Some("doctor" | "help" | "malloc-stats" | "stats" | "usage")
        ),
        "slowlog" => sub.as_deref() != Some("reset"),
        "script" => matches!(sub.as_deref(), Some("exists" | "help")),
        "module" => matches!(sub.as_deref(), Some("help" | "list")),
        "command" => true,
        "xinfo" => true,
        "object" => true,
        "latency" => sub.as_deref() != Some("reset"),
        _ => matches!(
            command.as_str(),
            "ping"
                | "echo"
                | "get"
                | "mget"
                | "getrange"
                | "substr"
                | "strlen"
                | "exists"
                | "type"
                | "ttl"
                | "pttl"
                | "scan"
                | "keys"
                | "dbsize"
                | "info"
                | "hget"
                | "hmget"
                | "hgetall"
                | "hkeys"
                | "hvals"
                | "hexists"
                | "hlen"
                | "hscan"
                | "lrange"
                | "lindex"
                | "llen"
                | "smembers"
                | "scard"
                | "sismember"
                | "srandmember"
                | "sinter"
                | "sunion"
                | "sdiff"
                | "sscan"
                | "zrange"
                | "zrevrange"
                | "zrangebyscore"
                | "zrevrangebyscore"
                | "zscore"
                | "zcard"
                | "zcount"
                | "zrank"
                | "zrevrank"
                | "zscan"
                | "xrange"
                | "xrevrange"
                | "xlen"
                | "dump"
                | "randomkey"
                | "time"
                | "lastsave"
                | "role"
                | "hello"
                | "lolwut"
                | "auth"
                | "readonly"
        ),
    }
}

fn value_to_query_response(
    argv: &[String],
    value: Value,
    max_rows: usize,
    duration_ms: u128,
) -> QueryResponse {
    let command = argv
        .first()
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    if matches!(value, Value::Okay) {
        return QueryResponse {
            columns: Vec::new(),
            rows: Vec::new(),
            row_count: 0,
            affected_rows: Some(1),
            duration_ms,
            truncated: false,
            notices: Vec::new(),
        };
    }
    if value.looks_like_cursor() {
        return cursor_query_response(value, max_rows, duration_ms);
    }
    if is_pair_command(&command, argv)
        && let Some(rows) = pair_rows(&value)
    {
        return capped_query_response(
            vec![
                QueryColumn {
                    name: "field".into(),
                    data_type: "string".into(),
                },
                QueryColumn {
                    name: "value".into(),
                    data_type: "string".into(),
                },
            ],
            rows,
            max_rows,
            duration_ms,
        );
    }
    match value {
        Value::Nil => QueryResponse {
            columns: vec![QueryColumn {
                name: "value".into(),
                data_type: "nil".into(),
            }],
            rows: vec![vec![JsonValue::Null]],
            row_count: 1,
            affected_rows: None,
            duration_ms,
            truncated: false,
            notices: Vec::new(),
        },
        Value::Array(items) if items.iter().all(|item| matches!(item, Value::Array(_))) => {
            let rows = items
                .into_iter()
                .filter_map(|item| {
                    if let Value::Array(cells) = item {
                        Some(cells.into_iter().map(json_from_redis).collect::<Vec<_>>())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            let width = rows.iter().map(Vec::len).max().unwrap_or(0);
            let columns = (0..width)
                .map(|index| QueryColumn {
                    name: format!("column{}", index + 1),
                    data_type: "string".into(),
                })
                .collect();
            capped_query_response(columns, rows, max_rows, duration_ms)
        }
        Value::Array(items) => {
            let rows = items
                .into_iter()
                .map(|item| vec![json_from_redis(item)])
                .collect();
            capped_query_response(
                vec![QueryColumn {
                    name: "value".into(),
                    data_type: "string".into(),
                }],
                rows,
                max_rows,
                duration_ms,
            )
        }
        Value::Map(pairs) => {
            let rows = pairs
                .into_iter()
                .map(|(field, value)| vec![json_from_redis(field), json_from_redis(value)])
                .collect();
            capped_query_response(
                vec![
                    QueryColumn {
                        name: "field".into(),
                        data_type: "string".into(),
                    },
                    QueryColumn {
                        name: "value".into(),
                        data_type: "string".into(),
                    },
                ],
                rows,
                max_rows,
                duration_ms,
            )
        }
        Value::Int(number) => QueryResponse {
            columns: vec![QueryColumn {
                name: "result".into(),
                data_type: "integer".into(),
            }],
            rows: vec![vec![JsonValue::Number(number.into())]],
            row_count: 1,
            affected_rows: Some(u64::try_from(number).unwrap_or(0)),
            duration_ms,
            truncated: false,
            notices: Vec::new(),
        },
        other => QueryResponse {
            columns: vec![QueryColumn {
                name: "value".into(),
                data_type: "string".into(),
            }],
            rows: vec![vec![json_from_redis(other)]],
            row_count: 1,
            affected_rows: None,
            duration_ms,
            truncated: false,
            notices: Vec::new(),
        },
    }
}

fn cursor_query_response(value: Value, max_rows: usize, duration_ms: u128) -> QueryResponse {
    let Value::Array(mut parts) = value else {
        return QueryResponse {
            columns: Vec::new(),
            rows: Vec::new(),
            row_count: 0,
            affected_rows: None,
            duration_ms,
            truncated: false,
            notices: Vec::new(),
        };
    };
    let cursor = parts
        .first()
        .cloned()
        .map(json_from_redis)
        .unwrap_or(JsonValue::String("0".into()));
    let items = if parts.len() > 1 {
        parts.swap_remove(1)
    } else {
        Value::Nil
    };
    let rows = if let Some(pairs) = pair_rows(&items) {
        let mut response = capped_query_response(
            vec![
                QueryColumn {
                    name: "field".into(),
                    data_type: "string".into(),
                },
                QueryColumn {
                    name: "value".into(),
                    data_type: "string".into(),
                },
            ],
            pairs,
            max_rows,
            duration_ms,
        );
        response.notices = vec![format!("cursor {cursor}")];
        return response;
    } else if let Value::Array(values) = items {
        values
            .into_iter()
            .map(|item| vec![json_from_redis(item)])
            .collect()
    } else {
        vec![vec![json_from_redis(items)]]
    };
    let mut response = capped_query_response(
        vec![QueryColumn {
            name: "value".into(),
            data_type: "string".into(),
        }],
        rows,
        max_rows,
        duration_ms,
    );
    response.notices = vec![format!("cursor {cursor}")];
    response
}

fn capped_query_response(
    columns: Vec<QueryColumn>,
    mut rows: Vec<Vec<JsonValue>>,
    max_rows: usize,
    duration_ms: u128,
) -> QueryResponse {
    let truncated = rows.len() > max_rows;
    rows.truncate(max_rows);
    QueryResponse {
        row_count: rows.len(),
        rows,
        columns,
        affected_rows: None,
        duration_ms,
        truncated,
        notices: Vec::new(),
    }
}

fn is_pair_command(command: &str, argv: &[String]) -> bool {
    matches!(command, "hgetall" | "config")
        || (command == "zrange"
            || command == "zrevrange"
            || command == "zrangebyscore"
            || command == "zrevrangebyscore")
            && argv
                .iter()
                .any(|arg| arg.eq_ignore_ascii_case("withscores"))
}

fn pair_rows(value: &Value) -> Option<Vec<Vec<JsonValue>>> {
    let Value::Array(items) = value else {
        return None;
    };
    if items.len() % 2 != 0 {
        return None;
    }
    Some(
        items
            .chunks(2)
            .map(|chunk| {
                vec![
                    json_from_redis(chunk[0].clone()),
                    json_from_redis(chunk[1].clone()),
                ]
            })
            .collect(),
    )
}

fn json_from_redis(value: Value) -> JsonValue {
    match value {
        Value::Nil => JsonValue::Null,
        Value::Int(value) => JsonValue::Number(value.into()),
        Value::BulkString(bytes) => bulk_to_json(&bytes),
        Value::Array(items) => JsonValue::Array(items.into_iter().map(json_from_redis).collect()),
        Value::SimpleString(value) => JsonValue::String(value),
        Value::Okay => JsonValue::String("OK".into()),
        Value::Map(pairs) => {
            let mut object = serde_json::Map::new();
            for (key, value) in pairs {
                object.insert(
                    json_to_string(&json_from_redis(key)),
                    json_from_redis(value),
                );
            }
            JsonValue::Object(object)
        }
        Value::Boolean(value) => JsonValue::Bool(value),
        Value::Double(value) => serde_json::Number::from_f64(value)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Value::Set(items) => JsonValue::Array(items.into_iter().map(json_from_redis).collect()),
        Value::VerbatimString { text, .. } => JsonValue::String(text),
        Value::Attribute { data, .. } => json_from_redis(*data),
        other => JsonValue::String(format!("{other:?}")),
    }
}

fn bulk_to_json(bytes: &[u8]) -> JsonValue {
    match String::from_utf8(bytes.to_vec()) {
        Ok(mut text) => {
            if text.len() > MAX_VALUE_BYTES {
                text.truncate(text.floor_char_boundary(MAX_VALUE_BYTES));
                text.push_str(TRUNCATION_MARKER);
            }
            JsonValue::String(text)
        }
        Err(_) => JsonValue::String(format!("\\x{}", hex_encode(bytes))),
    }
}

/// Key names are table names and mutation targets, so a lossy or hex preview
/// could open or write a different key. Names that are not UTF-8 are skipped.
fn utf8_key_names(keys: Vec<Vec<u8>>) -> impl Iterator<Item = String> {
    keys.into_iter()
        .filter_map(|key| String::from_utf8(key).ok())
}

fn value_as_string(value: Value) -> String {
    match json_from_redis(value) {
        JsonValue::String(value) => value,
        other => json_to_string(&other),
    }
}

fn value_as_i64(value: Value) -> Option<i64> {
    match value {
        Value::Int(value) => Some(value),
        Value::BulkString(bytes) => String::from_utf8(bytes).ok()?.parse().ok(),
        Value::SimpleString(value) => value.parse().ok(),
        _ => None,
    }
}

fn score_json(value: &str) -> JsonValue {
    value
        .parse::<f64>()
        .ok()
        .and_then(serde_json::Number::from_f64)
        .map(JsonValue::Number)
        .unwrap_or_else(|| JsonValue::String(value.to_owned()))
}

fn json_to_string(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => String::new(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::Number(value) => value.to_string(),
        JsonValue::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn json_to_i64(value: &JsonValue) -> AppResult<i64> {
    match value {
        JsonValue::Number(value) => value
            .as_i64()
            .ok_or_else(|| AppError::InvalidInput("list index must be an integer".into())),
        JsonValue::String(value) => value
            .parse()
            .map_err(|_| AppError::InvalidInput("list index must be an integer".into())),
        _ => Err(AppError::InvalidInput(
            "list index must be an integer".into(),
        )),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::DatabaseEngine;
    use chrono::Utc;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    #[test]
    fn bulk_previews_truncate_only_at_utf8_boundaries() {
        let exact = format!("{}é", "a".repeat(MAX_VALUE_BYTES - 2));
        assert_eq!(bulk_to_json(exact.as_bytes()), JsonValue::String(exact));
        for suffix in ["é", "🚀"] {
            let prefix = "a".repeat(MAX_VALUE_BYTES - 1);
            let input = format!("{prefix}{suffix}tail");
            assert_eq!(
                bulk_to_json(input.as_bytes()),
                JsonValue::String(format!("{prefix}… (truncated)"))
            );
        }
    }

    #[test]
    fn tokenizes_quoted_redis_cli_arguments() {
        assert_eq!(
            tokenize_redis_cli(r#"SET greeting "hello world""#).expect("tokens"),
            ["SET", "greeting", "hello world"]
        );
        assert_eq!(
            tokenize_redis_cli("HSET user:1 name 'Ada Lovelace'").expect("tokens"),
            ["HSET", "user:1", "name", "Ada Lovelace"]
        );
        assert!(tokenize_redis_cli(r#"SET broken "unterminated"#).is_err());
    }

    #[test]
    fn connection_state_commands_are_rejected() {
        let argv = |input: &str| tokenize_redis_cli(input).expect("tokens");
        assert!(connection_state_command_error("select", &argv("SELECT 2")).is_some());
        assert!(connection_state_command_error("hello", &argv("HELLO 3")).is_some());
        assert!(connection_state_command_error("hello", &argv("HELLO")).is_none());
        assert!(connection_state_command_error("client", &argv("CLIENT REPLY OFF")).is_some());
        assert!(connection_state_command_error("client", &argv("CLIENT LIST")).is_none());
        assert!(connection_state_command_error("get", &argv("GET key")).is_none());
    }

    #[test]
    fn read_only_command_detection_covers_common_writes() {
        assert!(is_read_command(&["GET".into(), "greeting".into()]));
        assert!(is_read_command(&["HGETALL".into(), "user:1".into()]));
        assert!(is_read_command(&[
            "CONFIG".into(),
            "GET".into(),
            "databases".into()
        ]));
        assert!(!is_read_command(&[
            "SET".into(),
            "greeting".into(),
            "hi".into()
        ]));
        assert!(!is_read_command(&["DEL".into(), "greeting".into()]));
        assert!(!is_read_command(&["FLUSHALL".into()]));
        assert!(!is_read_command(&[
            "CONFIG".into(),
            "SET".into(),
            "slowlog-log-slower-than".into()
        ]));
        assert!(is_unsupported_command(
            "subscribe",
            &["SUBSCRIBE".into(), "news".into()]
        ));
        assert!(is_unsupported_command(
            "xread",
            &[
                "XREAD".into(),
                "BLOCK".into(),
                "0".into(),
                "STREAMS".into(),
                "s".into(),
                "0".into()
            ]
        ));
    }

    #[test]
    fn hgetall_results_become_field_value_rows() {
        let response = value_to_query_response(
            &["HGETALL".into(), "user:1".into()],
            Value::Array(vec![
                Value::SimpleString("name".into()),
                Value::SimpleString("Ada".into()),
                Value::SimpleString("role".into()),
                Value::SimpleString("engineer".into()),
            ]),
            10,
            3,
        );
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
    fn scan_patterns_escape_globs_and_only_narrow_supported_operators() {
        let pattern = |operator, value: &str| {
            scan_pattern(&[FilterCondition {
                column: "key".into(),
                operator,
                value: Some(value.into()),
            }])
        };
        assert_eq!(
            pattern(FilterOperator::Equals, r"a*b?[c]\d"),
            r"a\*b\?\[c\]\\d"
        );
        assert_eq!(pattern(FilterOperator::Contains, "u:1*"), r"*[uU]:1\**");
        assert_eq!(pattern(FilterOperator::StartsWith, "Ab"), "[aA][bB]*");
        assert_eq!(pattern(FilterOperator::EndsWith, "é?"), r"*é\?");
        for operator in [
            FilterOperator::NotEquals,
            FilterOperator::In,
            FilterOperator::NotIn,
            FilterOperator::GreaterThan,
            FilterOperator::LessThanOrEqual,
            FilterOperator::IsNull,
        ] {
            assert_eq!(pattern(operator, "user"), "*");
        }
        assert_eq!(scan_pattern(&[]), "*");
    }

    struct TestRedis {
        child: std::process::Child,
    }

    impl Drop for TestRedis {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn unused_port() -> Option<u16> {
        std::net::TcpListener::bind("127.0.0.1:0")
            .ok()?
            .local_addr()
            .ok()
            .map(|addr| addr.port())
    }

    async fn start_test_redis() -> Option<(TestRedis, RedisSession)> {
        let port = unused_port()?;
        let child = Command::new("redis-server")
            .args([
                "--port",
                &port.to_string(),
                "--bind",
                "127.0.0.1",
                "--save",
                "",
                "--appendonly",
                "no",
                "--protected-mode",
                "no",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let server = TestRedis { child };
        let profile = test_profile(port, false);
        for _ in 0..80 {
            if let Ok(session) = RedisSession::connect(profile.clone(), None).await {
                return Some((server, session));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }

    fn test_profile(port: u16, read_only: bool) -> ConnectionProfile {
        let now = Utc::now();
        ConnectionProfile {
            id: Uuid::new_v4(),
            name: "test-redis".into(),
            color: None,
            engine: DatabaseEngine::Redis,
            host: "127.0.0.1".into(),
            port,
            username: String::new(),
            default_database: "0".into(),
            tls_mode: TlsMode::Disabled,
            ca_cert_path: None,
            ssh: None,
            read_only,
            open_on_startup: true,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn string_edits_keep_the_key_expiry() {
        let Some((_server, session)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        session
            .run_query("SET token old EX 3600", None)
            .await
            .expect("set string");
        let edit = |value: &str| RowMutation {
            original: vec![JsonValue::from("token"), JsonValue::from("old")],
            changes: vec![JsonValue::from("token"), JsonValue::from(value)],
            primary_key: vec![JsonValue::from("token")],
            xmin: None,
            deleted: false,
        };
        let saved = session
            .apply_mutations(&MutationBatch {
                profile_id: session.profile().id,
                database: session.profile().default_database.clone(),
                schema: "string".into(),
                table: "token".into(),
                mutations: vec![edit("new")],
            })
            .await
            .expect("edit string");
        assert_eq!((saved.applied, saved.conflicts.len()), (1, 0));
        let value = session.run_query("GET token", None).await.expect("get");
        assert_eq!(value.rows[0][0], JsonValue::from("new"));
        let ttl = session.run_query("TTL token", None).await.expect("ttl");
        let ttl = ttl.rows[0][0].as_i64().expect("ttl seconds");
        assert!(ttl > 3500, "the expiry survives the edit, got {ttl}");
    }

    #[tokio::test]
    async fn redis_session_browses_keys_runs_commands_and_applies_edits() {
        let Some((_server, session)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        session
            .run_query(r#"SET greeting "hello dbm""#, None)
            .await
            .expect("set string");
        session
            .run_query("HSET user:1 name Ada role engineer", None)
            .await
            .expect("set hash");
        session
            .run_query("LPUSH tasks ship", None)
            .await
            .expect("set list");

        let databases = session.list_databases().await.expect("databases");
        assert!(databases.iter().any(|database| database.name == "0"));

        let tree = session.schema_tree().await.expect("tree");
        assert!(
            tree.iter()
                .any(|node| node.schema.as_deref() == Some("keys"))
        );
        assert!(tree.iter().any(|node| {
            node.schema.as_deref() == Some("string")
                && node
                    .children
                    .iter()
                    .any(|child| child.table.as_deref() == Some("greeting"))
        }));
        assert!(tree.iter().any(|node| {
            node.schema.as_deref() == Some("hash")
                && node
                    .children
                    .iter()
                    .any(|child| child.table.as_deref() == Some("user:1"))
        }));

        let keys = session
            .table_page(&TablePageRequest {
                profile_id: session.profile().id,
                schema: "keys".into(),
                table: "all".into(),
                offset: 0,
                limit: 50,
                filters: vec![FilterCondition {
                    column: "key".into(),
                    operator: FilterOperator::Contains,
                    value: Some("user".into()),
                }],
                order_by: Some(OrderSpec {
                    column: "key".into(),
                    descending: false,
                }),
                include_total: Some(true),
            })
            .await
            .expect("keys page");
        assert_eq!(keys.rows.len(), 1);
        assert_eq!(keys.rows[0][0], JsonValue::String("user:1".into()));
        assert_eq!(keys.rows[0][1], JsonValue::String("hash".into()));

        let hash = session
            .table_page(&TablePageRequest {
                profile_id: session.profile().id,
                schema: "hash".into(),
                table: "user:1".into(),
                offset: 0,
                limit: 50,
                filters: Vec::new(),
                order_by: None,
                include_total: Some(true),
            })
            .await
            .expect("hash page");
        assert!(
            hash.rows
                .iter()
                .any(|row| row[0] == JsonValue::String("name".into())
                    && row[1] == JsonValue::String("Ada".into()))
        );

        let ping = session.run_query("PING", None).await.expect("ping");
        assert_eq!(ping.rows[0][0], JsonValue::String("PONG".into()));

        let mutated = session
            .apply_mutations(&MutationBatch {
                profile_id: session.profile().id,
                database: session.profile().default_database.clone(),
                schema: "hash".into(),
                table: "user:1".into(),
                mutations: vec![RowMutation {
                    original: vec![
                        JsonValue::String("name".into()),
                        JsonValue::String("Ada".into()),
                    ],
                    changes: vec![
                        JsonValue::String("name".into()),
                        JsonValue::String("Grace".into()),
                    ],
                    primary_key: vec![JsonValue::String("name".into())],
                    xmin: None,
                    deleted: false,
                }],
            })
            .await
            .expect("mutate hash");
        assert_eq!(mutated.applied, 1);

        let after = session
            .run_query("HGET user:1 name", None)
            .await
            .expect("hget");
        assert_eq!(after.rows[0][0], JsonValue::String("Grace".into()));

        let deleted = session
            .apply_mutations(&MutationBatch {
                profile_id: session.profile().id,
                database: session.profile().default_database.clone(),
                schema: "keys".into(),
                table: "all".into(),
                mutations: vec![RowMutation {
                    original: vec![
                        JsonValue::String("greeting".into()),
                        JsonValue::String("string".into()),
                        JsonValue::Number((-1).into()),
                    ],
                    changes: vec![
                        JsonValue::String("greeting".into()),
                        JsonValue::String("string".into()),
                        JsonValue::Number((-1).into()),
                    ],
                    primary_key: vec![JsonValue::String("greeting".into())],
                    xmin: None,
                    deleted: true,
                }],
            })
            .await
            .expect("delete key");
        assert_eq!(deleted.applied, 1);
        let missing = session
            .run_query("EXISTS greeting", None)
            .await
            .expect("exists");
        assert_eq!(missing.rows[0][0], JsonValue::Number(0.into()));
    }

    #[tokio::test]
    async fn list_pages_fetch_only_the_requested_range_and_keep_absolute_indices() {
        let Some((_server, session)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        session
            .run_query("RPUSH tasks z a y b x c w d v e u f", None)
            .await
            .unwrap();
        session
            .run_query("CONFIG SET slowlog-log-slower-than 0", None)
            .await
            .unwrap();
        let mut request = TablePageRequest {
            profile_id: session.profile().id,
            schema: "list".into(),
            table: "tasks".into(),
            offset: 2,
            limit: 3,
            filters: vec![],
            order_by: None,
            include_total: Some(true),
        };
        let row = |index, value: &str| vec![JsonValue::from(index), JsonValue::from(value)];
        let page = session.table_page(&request).await.unwrap();
        assert_eq!(page.rows, vec![row(2, "y"), row(3, "b"), row(4, "x")]);
        assert_eq!(page.total_rows, Some(12));
        assert!(page.has_more);
        // Assert the actual server request, not just the already-capped output:
        // fetching the whole list and slicing locally must fail this test.
        let log: Vec<Value> = redis_rs::cmd("SLOWLOG")
            .arg("GET")
            .arg(10)
            .query_async(&mut *session.conn().await)
            .await
            .unwrap();
        let ranges: Vec<Vec<String>> = log
            .into_iter()
            .filter_map(|entry| {
                let Value::Array(parts) = entry else {
                    return None;
                };
                let Value::Array(args) = parts.get(3)? else {
                    return None;
                };
                let args: Vec<_> = args.iter().cloned().map(value_as_string).collect();
                args.first()
                    .is_some_and(|cmd| cmd.eq_ignore_ascii_case("lrange"))
                    .then_some(args)
            })
            .collect();
        assert_eq!(ranges, vec![vec!["LRANGE", "tasks", "2", "5"]]);

        request.order_by = Some(OrderSpec {
            column: "index".into(),
            descending: false,
        });
        request.offset = 9;
        request.include_total = Some(false);
        let page = session.table_page(&request).await.unwrap();
        assert_eq!(page.rows, vec![row(9, "e"), row(10, "u"), row(11, "f")]);
        assert_eq!(page.total_rows, None);
        assert!(!page.has_more, "an exact final page is not truncated");
        request.offset = 11;
        let page = session.table_page(&request).await.unwrap();
        assert_eq!(page.rows, vec![row(11, "f")]);
        assert!(!page.has_more);
        request.offset = 12;
        let page = session.table_page(&request).await.unwrap();
        assert!(page.rows.is_empty());
        assert!(!page.has_more);

        // Value sorting, descending index order, and filters still apply to the
        // complete list before paging, not just the requested slice.
        request.offset = 2;
        request.order_by = Some(OrderSpec {
            column: "value".into(),
            descending: false,
        });
        let page = session.table_page(&request).await.unwrap();
        assert_eq!(page.rows, vec![row(5, "c"), row(7, "d"), row(9, "e")]);
        request.order_by = Some(OrderSpec {
            column: "index".into(),
            descending: true,
        });
        let page = session.table_page(&request).await.unwrap();
        assert_eq!(page.rows, vec![row(9, "e"), row(8, "v"), row(7, "d")]);
        request.order_by = None;
        request.filters = vec![FilterCondition {
            column: "value".into(),
            operator: FilterOperator::LessThan,
            value: Some("g".into()),
        }];
        request.include_total = Some(true);
        let page = session.table_page(&request).await.unwrap();
        assert_eq!(page.rows, vec![row(5, "c"), row(7, "d"), row(9, "e")]);
        assert_eq!(page.total_rows, Some(6));
        assert!(page.has_more);
    }

    #[tokio::test]
    async fn list_batches_keep_original_indices_when_deleting_and_updating() {
        let Some((_server, session)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        session
            .run_query("RPUSH tasks A B C D", None)
            .await
            .unwrap();
        let mutation = |index: i64, original: &str, changed: &str, deleted| RowMutation {
            original: vec![JsonValue::from(index), JsonValue::from(original)],
            changes: vec![JsonValue::from(index), JsonValue::from(changed)],
            primary_key: vec![JsonValue::from(index)],
            xmin: None,
            deleted,
        };
        let result = session
            .apply_mutations(&MutationBatch {
                profile_id: session.profile().id,
                database: session.profile().default_database.clone(),
                schema: "list".into(),
                table: "tasks".into(),
                mutations: vec![mutation(0, "A", "A", true), mutation(2, "C", "C", true)],
            })
            .await
            .unwrap();
        assert_eq!(result.applied, 2);
        assert!(result.conflicts.is_empty());
        assert_eq!(
            session
                .run_query("LRANGE tasks 0 -1", None)
                .await
                .unwrap()
                .rows,
            vec![vec![JsonValue::from("B")], vec![JsonValue::from("D")]]
        );

        session.run_query("DEL tasks", None).await.unwrap();
        session
            .run_query("RPUSH tasks A B C D E F G H I J K L", None)
            .await
            .unwrap();
        let result = session
            .apply_mutations(&MutationBatch {
                profile_id: session.profile().id,
                database: session.profile().default_database.clone(),
                schema: "list".into(),
                table: "tasks".into(),
                mutations: vec![
                    mutation(0, "A", "A", true),
                    mutation(2, "C", "C", true),
                    mutation(10, "K", "edited", false),
                ],
            })
            .await
            .unwrap();
        assert_eq!(result.applied, 3);
        assert!(result.conflicts.is_empty());
        let expected: Vec<_> = ["B", "D", "E", "F", "G", "H", "I", "J", "edited", "L"]
            .into_iter()
            .map(|value| vec![JsonValue::from(value)])
            .collect();
        assert_eq!(
            session
                .run_query("LRANGE tasks 0 -1", None)
                .await
                .unwrap()
                .rows,
            expected
        );
    }

    #[tokio::test]
    async fn key_index_pages_reach_every_key_past_five_thousand() {
        let Some((_server, session)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        let expected: Vec<_> = (0..5_100).map(|i| format!("item:{i:04}")).collect();
        {
            let mut conn = session.conn().await;
            let mut pipe = redis_rs::pipe();
            for key in &expected {
                pipe.cmd("SET").arg(key).arg("value").ignore();
            }
            pipe.query_async::<()>(&mut *conn).await.unwrap();
        }
        let mut request = TablePageRequest {
            profile_id: session.profile().id,
            schema: "keys".into(),
            table: "all".into(),
            offset: 0,
            limit: 1_000,
            filters: Vec::new(),
            order_by: None,
            include_total: Some(true),
        };
        let mut found = Vec::new();
        for page_index in 0..6 {
            request.offset = page_index * 1_000;
            let page = session.table_page(&request).await.unwrap();
            assert_eq!(page.total_rows, Some(5_100));
            assert_eq!(page.has_more, page_index < 5);
            if page_index == 0 {
                let stats = session.run_query("INFO commandstats", None).await.unwrap();
                let stats = stats.rows[0][0].as_str().unwrap();
                for command in ["type", "ttl"] {
                    assert!(
                        stats.lines().any(|line| {
                            line.starts_with(&format!("cmdstat_{command}:calls=1000,"))
                        }),
                        "only visible keys need {command}: {stats}"
                    );
                }
            }
            found.extend(
                page.rows
                    .into_iter()
                    .map(|row| row[0].as_str().unwrap().to_owned()),
            );
        }
        assert_eq!(found, expected, "default key order is stable and complete");

        request.order_by = Some(OrderSpec {
            column: "key".into(),
            descending: true,
        });
        request.offset = 5_000;
        request.include_total = Some(false);
        let page = session.table_page(&request).await.unwrap();
        assert_eq!(page.total_rows, None);
        assert!(!page.has_more);
        let found: Vec<_> = page
            .rows
            .iter()
            .map(|row| row[0].as_str().unwrap())
            .collect();
        let reverse_tail: Vec<_> = expected[..100].iter().rev().map(String::as_str).collect();
        assert_eq!(found, reverse_tail);

        request.order_by = None;
        request.filters = vec![FilterCondition {
            column: "ttl".into(),
            operator: FilterOperator::Equals,
            value: Some("-1".into()),
        }];
        request.include_total = Some(true);
        let page = session.table_page(&request).await.unwrap();
        assert_eq!(page.total_rows, Some(5_100));
        assert!(!page.has_more);
        let found: Vec<_> = page
            .rows
            .iter()
            .map(|row| row[0].as_str().unwrap())
            .collect();
        assert_eq!(
            found,
            expected[5_000..]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn queued_mutations_cannot_write_to_a_different_database() {
        use crate::session::DbSession;

        let Some((_server, original)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        original
            .run_query("SET greeting old-db", None)
            .await
            .unwrap();
        let mut batch = MutationBatch {
            profile_id: original.profile().id,
            database: "0".into(),
            schema: "string".into(),
            table: "greeting".into(),
            mutations: vec![RowMutation {
                original: vec![JsonValue::from("greeting"), JsonValue::from("old-db")],
                changes: vec![JsonValue::from("greeting"), JsonValue::from("edited")],
                primary_key: vec![JsonValue::from("greeting")],
                xmin: None,
                deleted: false,
            }],
        };
        let mut profile = original.profile().clone();
        profile.default_database = "1".into();
        let switched = DbSession::connect(profile, None).await.unwrap();
        switched
            .run_query("SET greeting new-db", None)
            .await
            .unwrap();
        let error = switched.apply_mutations(&batch).await.unwrap_err();
        assert!(error.to_string().contains("active database changed"));
        assert_eq!(
            switched.run_query("GET greeting", None).await.unwrap().rows,
            vec![vec![JsonValue::from("new-db")]]
        );
        assert_eq!(
            original.run_query("GET greeting", None).await.unwrap().rows,
            vec![vec![JsonValue::from("old-db")]]
        );

        batch.database = "1".into();
        batch.mutations[0].original[1] = JsonValue::from("new-db");
        assert_eq!(switched.apply_mutations(&batch).await.unwrap().applied, 1);
        assert_eq!(
            switched.run_query("GET greeting", None).await.unwrap().rows,
            vec![vec![JsonValue::from("edited")]]
        );
    }

    #[tokio::test]
    async fn database_selection_survives_restart_but_failed_switch_does_not() {
        use crate::models::SaveProfileInput;
        use crate::state::AppState;

        let Some((_server, writable)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        let path = std::env::temp_dir().join(format!("dbm-selection-{}.sqlite3", Uuid::new_v4()));
        let state = AppState::mobile(&path).unwrap();
        let input = SaveProfileInput {
            id: None,
            name: "Remember selection".into(),
            color: None,
            engine: DatabaseEngine::Redis,
            host: "127.0.0.1".into(),
            port: writable.profile().port,
            username: String::new(),
            default_database: "0".into(),
            tls_mode: TlsMode::Disabled,
            ca_cert_path: None,
            ssh: None,
            read_only: false,
            open_on_startup: false,
            password: None,
        };
        let profile = state.save_profile(input.clone()).unwrap();
        let other = state.save_profile(input).unwrap();
        let id = profile.id;
        let original = state.connect(profile).await.unwrap();
        original
            .run_query("SET location original", None)
            .await
            .unwrap();
        let selected = state.connect_database(id, " 3 ").await.unwrap();
        selected
            .run_query("SET location selected", None)
            .await
            .unwrap();
        assert_eq!(state.profile(id).unwrap().default_database, "3");
        assert!(!state.profile(id).unwrap().open_on_startup);
        assert_eq!(state.profile(other.id).unwrap().default_database, "0");
        assert!(state.connect_database(id, "9999").await.is_err());
        assert!(state.connect_database(id, " ").await.is_err());
        assert_eq!(state.profile(id).unwrap().default_database, "3");
        assert_eq!(
            state.session(id).await.unwrap().profile().default_database,
            "3"
        );
        state.disconnect(id).await;
        drop(state);

        let restarted = AppState::mobile(&path).unwrap();
        let session = restarted
            .connect(restarted.profile(id).unwrap())
            .await
            .unwrap();
        assert_eq!(
            session.run_query("GET location", None).await.unwrap().rows,
            vec![vec![JsonValue::from("selected")]]
        );
        let session = restarted
            .connect(restarted.profile(other.id).unwrap())
            .await
            .unwrap();
        assert_eq!(
            session.run_query("GET location", None).await.unwrap().rows,
            vec![vec![JsonValue::from("original")]]
        );
        restarted.disconnect(id).await;
        restarted.disconnect(other.id).await;
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn shared_workbench_records_success_and_failure_in_active_database() {
        use crate::keyring_store::CredentialStore;
        use crate::models::{QueryRequest, SaveProfileInput};
        use crate::session::DbSession;
        use crate::state::AppState;
        use crate::storage::LocalStore;
        use std::collections::HashMap;
        use std::sync::Arc;

        let Some((_server, writable)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        let path = std::env::temp_dir().join(format!("dbm-workbench-{}.sqlite3", Uuid::new_v4()));
        let store = LocalStore::from_path(&path).unwrap();
        let mut profile = store
            .save_profile(&SaveProfileInput {
                id: None,
                name: "Native test".into(),
                color: None,
                engine: DatabaseEngine::Redis,
                host: "127.0.0.1".into(),
                port: writable.profile().port,
                username: String::new(),
                default_database: "0".into(),
                tls_mode: TlsMode::Disabled,
                ca_cert_path: None,
                ssh: None,
                read_only: true,
                open_on_startup: true,
                password: None,
            })
            .unwrap();
        profile.default_database = "3".into();
        let id = profile.id;
        let session = DbSession::connect(profile, None).await.unwrap();
        let state = AppState {
            store,
            credentials: CredentialStore::default(),
            sessions: Mutex::new(HashMap::from([(id, Arc::new(session))])),
        };
        let response = state
            .run_query(QueryRequest {
                profile_id: id,
                sql: "PING".into(),
                max_rows: Some(10),
            })
            .await
            .unwrap();
        assert_eq!(response.rows, vec![vec![JsonValue::String("PONG".into())]]);
        let error = state
            .run_query(QueryRequest {
                profile_id: id,
                sql: "SET blocked 1".into(),
                max_rows: Some(10),
            })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("read-only"));
        assert!(state.store.list_history(id, "0", 100).unwrap().is_empty());
        let history = state.store.list_history(id, "3", 100).unwrap();
        assert_eq!(history.len(), 2);
        assert!(
            history
                .iter()
                .any(|entry| entry.sql == "PING" && entry.success)
        );
        assert!(
            history
                .iter()
                .any(|entry| entry.sql == "SET blocked 1" && !entry.success)
        );
        state.disconnect(id).await;
        assert!(
            state
                .run_query(QueryRequest {
                    profile_id: id,
                    sql: "PING".into(),
                    max_rows: None,
                })
                .await
                .is_err()
        );
        assert_eq!(state.store.list_history(id, "3", 100).unwrap().len(), 2);
        std::fs::remove_file(path).unwrap();
    }

    fn page_request(session: &RedisSession, schema: &str, table: &str) -> TablePageRequest {
        TablePageRequest {
            profile_id: session.profile().id,
            schema: schema.into(),
            table: table.into(),
            offset: 0,
            limit: 100,
            filters: Vec::new(),
            order_by: None,
            include_total: Some(true),
        }
    }

    #[tokio::test]
    async fn binary_keys_and_values_do_not_break_views() {
        let Some((_server, session)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        {
            let mut conn = session.conn().await;
            redis_rs::pipe()
                .cmd("SET")
                .arg(b"bin:\xff".as_slice())
                .arg("x")
                .ignore()
                .cmd("SET")
                .arg("plain")
                .arg(b"\x00\xff".as_slice())
                .ignore()
                .cmd("HSET")
                .arg("h")
                .arg(b"\xfe".as_slice())
                .arg(b"\xfd".as_slice())
                .arg("name")
                .arg("Ada")
                .ignore()
                .cmd("RPUSH")
                .arg("l")
                .arg(b"\xff".as_slice())
                .arg("ok")
                .ignore()
                .cmd("SADD")
                .arg("s")
                .arg(b"\xff".as_slice())
                .arg("ok")
                .ignore()
                .cmd("ZADD")
                .arg("z")
                .arg(1)
                .arg(b"\xff".as_slice())
                .arg(2)
                .arg("ok")
                .ignore()
                .query_async::<()>(&mut *conn)
                .await
                .unwrap();
        }
        let tree = session.schema_tree().await.unwrap();
        let names: Vec<_> = tree
            .iter()
            .flat_map(|node| node.children.iter())
            .filter_map(|child| child.table.as_deref())
            .collect();
        for key in ["plain", "h", "l", "s", "z"] {
            assert!(names.contains(&key), "{key} missing from {names:?}");
        }
        assert!(!names.iter().any(|name| name.starts_with("bin:")));

        let keys = session
            .table_page(&page_request(&session, "keys", "all"))
            .await
            .unwrap();
        let keys: Vec<_> = keys.rows.iter().map(|row| row[0].clone()).collect();
        assert_eq!(keys, ["h", "l", "plain", "s", "z"].map(JsonValue::from));

        let page = |schema: &'static str, table: &'static str| {
            let request = page_request(&session, schema, table);
            let session = &session;
            async move { session.table_page(&request).await.unwrap().rows }
        };
        assert_eq!(
            page("string", "plain").await,
            vec![vec![JsonValue::from("plain"), JsonValue::from("\\x00ff")]]
        );
        assert_eq!(
            page("hash", "h").await.len(),
            2,
            "binary hash fields are shown"
        );
        assert!(
            page("hash", "h")
                .await
                .contains(&vec![JsonValue::from("\\xfe"), JsonValue::from("\\xfd")])
        );
        assert_eq!(
            page("list", "l").await,
            vec![
                vec![JsonValue::from(0), JsonValue::from("\\xff")],
                vec![JsonValue::from(1), JsonValue::from("ok")]
            ]
        );
        let mut sorted = page_request(&session, "list", "l");
        sorted.order_by = Some(OrderSpec {
            column: "value".into(),
            descending: true,
        });
        assert_eq!(
            session.table_page(&sorted).await.unwrap().rows[1],
            vec![JsonValue::from(0), JsonValue::from("\\xff")]
        );
        assert_eq!(page("set", "s").await.len(), 2);
        assert!(
            page("zset", "z")
                .await
                .contains(&vec![JsonValue::from("\\xff"), JsonValue::from(1.0)])
        );
    }

    #[tokio::test]
    async fn schema_tree_fills_every_folder_from_one_scan_pass() {
        let Some((_server, session)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        {
            let mut conn = session.conn().await;
            let mut pipe = redis_rs::pipe();
            for index in 0..600 {
                pipe.cmd("SET").arg(format!("s:{index:03}")).arg(1).ignore();
            }
            for index in 0..300 {
                pipe.cmd("RPUSH")
                    .arg(format!("l:{index:03}"))
                    .arg(1)
                    .ignore();
            }
            for index in 0..3 {
                pipe.cmd("HSET")
                    .arg(format!("h:{index}"))
                    .arg("f")
                    .arg(1)
                    .ignore();
            }
            pipe.cmd("CONFIG").arg("RESETSTAT").ignore();
            pipe.query_async::<()>(&mut *conn).await.unwrap();
        }
        let tree = session.schema_tree().await.unwrap();
        let folder = |schema: &str| {
            tree.iter()
                .find(|node| node.schema.as_deref() == Some(schema))
                .map(|node| node.children.len())
        };
        assert_eq!(folder("string"), Some(TREE_KEYS_PER_TYPE));
        assert_eq!(folder("list"), Some(TREE_KEYS_PER_TYPE));
        assert_eq!(folder("hash"), Some(3));
        assert_eq!(folder("set"), None);
        let hashes = tree
            .iter()
            .find(|node| node.schema.as_deref() == Some("hash"))
            .unwrap();
        let names: Vec<_> = hashes
            .children
            .iter()
            .map(|child| child.name.as_str())
            .collect();
        assert_eq!(names, ["h:0", "h:1", "h:2"]);

        let stats = session.run_query("INFO commandstats", None).await.unwrap();
        let stats = stats.rows[0][0].as_str().unwrap().to_owned();
        let calls = |command: &str| {
            stats
                .lines()
                .find_map(|line| line.strip_prefix(&format!("cmdstat_{command}:calls=")))
                .and_then(|rest| rest.split(',').next()?.parse::<u32>().ok())
                .unwrap_or(0)
        };
        // 903 keys fit in about one SCAN COUNT 1000 page; one scan per type
        // used to need at least six.
        assert!(calls("scan") <= 3, "{stats}");
        assert!(calls("type") <= 1_000, "{stats}");
    }

    #[tokio::test]
    async fn only_unsupported_scan_type_errors_fall_back() {
        let Some((_server, session)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        let mut conn = session.conn().await;
        // Redis 5 answers SCAN … TYPE with the same reply as this bad option.
        let syntax = redis_rs::cmd("SCAN")
            .arg(0)
            .arg("BOGUS")
            .arg(1)
            .query_async::<Value>(&mut *conn)
            .await
            .unwrap_err();
        assert!(is_scan_type_unsupported(&syntax), "{syntax:?}");
        let wrong_type = redis_rs::cmd("SET")
            .arg("list")
            .arg(1)
            .query_async::<Value>(&mut *conn)
            .await
            .and(
                redis_rs::cmd("LPUSH")
                    .arg("list")
                    .arg(1)
                    .query_async::<Value>(&mut *conn)
                    .await,
            )
            .unwrap_err();
        assert!(!is_scan_type_unsupported(&wrong_type), "{wrong_type:?}");
        let io = redis_rs::RedisError::from(std::io::Error::other("connection reset"));
        assert!(!is_scan_type_unsupported(&io));
    }

    #[tokio::test]
    async fn key_filters_match_literally_and_support_every_operator() {
        let Some((_server, session)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        session
            .run_query("MSET user[1] a user1 b User:2 c admin d", None)
            .await
            .unwrap();
        let keys = |operator, value: &str| {
            let mut request = page_request(&session, "keys", "all");
            request.filters = vec![FilterCondition {
                column: "key".into(),
                operator,
                value: Some(value.into()),
            }];
            let session = &session;
            async move {
                session
                    .table_page(&request)
                    .await
                    .unwrap()
                    .rows
                    .into_iter()
                    .map(|row| row[0].as_str().unwrap().to_owned())
                    .collect::<Vec<_>>()
            }
        };
        assert_eq!(keys(FilterOperator::Equals, "user[1]").await, ["user[1]"]);
        assert_eq!(
            keys(FilterOperator::Equals, "user?").await,
            Vec::<String>::new()
        );
        assert_eq!(
            keys(FilterOperator::Contains, "USER").await,
            ["User:2", "user1", "user[1]"]
        );
        assert_eq!(keys(FilterOperator::StartsWith, "user[").await, ["user[1]"]);
        assert_eq!(
            keys(FilterOperator::NotEquals, "user1").await,
            ["User:2", "admin", "user[1]"]
        );
        assert_eq!(
            keys(FilterOperator::In, "user1, admin").await,
            ["admin", "user1"]
        );
        assert_eq!(
            keys(FilterOperator::NotIn, "user1, admin").await,
            ["User:2", "user[1]"]
        );
        assert_eq!(
            keys(FilterOperator::GreaterThan, "user").await,
            ["user1", "user[1]"]
        );
        assert_eq!(
            keys(FilterOperator::LessThan, "user").await,
            ["User:2", "admin"]
        );
    }

    fn row_mutation(
        original: Vec<JsonValue>,
        changes: Vec<JsonValue>,
        deleted: bool,
    ) -> RowMutation {
        RowMutation {
            primary_key: vec![original[0].clone()],
            original,
            changes,
            xmin: None,
            deleted,
        }
    }

    fn batch(
        session: &RedisSession,
        schema: &str,
        table: &str,
        mutations: Vec<RowMutation>,
    ) -> MutationBatch {
        MutationBatch {
            profile_id: session.profile().id,
            database: session.profile().default_database.clone(),
            schema: schema.into(),
            table: table.into(),
            mutations,
        }
    }

    #[tokio::test]
    async fn key_edits_report_missing_rows_as_conflicts_without_creating_them() {
        let Some((_server, session)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        session.run_query("HSET h a 1 b 2 c 3", None).await.unwrap();
        session.run_query("SADD s a b", None).await.unwrap();
        session.run_query("ZADD z 1 a 2 b", None).await.unwrap();
        session.run_query("SET greeting hi", None).await.unwrap();
        session.run_query("CONFIG RESETSTAT", None).await.unwrap();
        let pair = |a: &str, b: &str| vec![JsonValue::from(a), JsonValue::from(b)];

        let result = session
            .apply_mutations(&batch(
                &session,
                "hash",
                "h",
                vec![
                    row_mutation(pair("a", "1"), pair("a", "1"), true),
                    row_mutation(pair("gone", "1"), pair("gone", "1"), true),
                    row_mutation(pair("b", "2"), pair("b", "two"), false),
                    row_mutation(pair("missing", "1"), pair("missing", "new"), false),
                ],
            ))
            .await
            .unwrap();
        assert_eq!(result.applied, 2);
        assert_eq!(
            result.conflicts,
            vec![
                vec![JsonValue::from("gone")],
                vec![JsonValue::from("missing")]
            ]
        );
        let hash = session.run_query("HGETALL h", None).await.unwrap().rows;
        assert_eq!(hash, vec![pair("b", "two"), pair("c", "3")]);

        let result = session
            .apply_mutations(&batch(
                &session,
                "set",
                "s",
                vec![
                    row_mutation(vec!["a".into()], vec!["a".into()], true),
                    row_mutation(vec!["x".into()], vec!["x".into()], true),
                    row_mutation(vec!["b".into()], vec!["b".into()], false),
                ],
            ))
            .await
            .unwrap();
        assert_eq!(result.applied, 2);
        assert_eq!(result.conflicts, vec![vec![JsonValue::from("x")]]);

        let score =
            |member: &str, score: f64| vec![JsonValue::from(member), JsonValue::from(score)];
        let error = session
            .apply_mutations(&batch(
                &session,
                "zset",
                "z",
                vec![
                    row_mutation(score("a", 1.0), score("a", 1.0), true),
                    row_mutation(
                        score("b", 2.0),
                        vec![JsonValue::from("b"), JsonValue::from("lots")],
                        false,
                    ),
                ],
            ))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not a number"), "{error}");
        assert_eq!(
            session.run_query("ZCARD z", None).await.unwrap().rows,
            vec![vec![JsonValue::from(2)]],
            "an invalid score rejects the batch before any write"
        );
        let result = session
            .apply_mutations(&batch(
                &session,
                "zset",
                "z",
                vec![
                    row_mutation(score("a", 1.0), score("a", 1.0), true),
                    row_mutation(score("b", 2.0), score("b", 5.0), false),
                    row_mutation(score("x", 1.0), score("x", 9.0), false),
                ],
            ))
            .await
            .unwrap();
        assert_eq!(result.applied, 2);
        assert_eq!(result.conflicts, vec![vec![JsonValue::from("x")]]);
        assert_eq!(
            session
                .run_query("ZRANGE z 0 -1 WITHSCORES", None)
                .await
                .unwrap()
                .rows,
            vec![pair("b", "5")]
        );

        // Each batch costs one round trip, plus one existence check for hash
        // updates; nothing checks rows one at a time.
        let stats = session.run_query("INFO commandstats", None).await.unwrap();
        let stats = stats.rows[0][0].as_str().unwrap().to_owned();
        assert!(!stats.contains("cmdstat_exists:"), "{stats}");
        for expected in [
            "cmdstat_hexists:calls=2,",
            "cmdstat_sismember:calls=1,",
            "cmdstat_zscore:calls=2,",
        ] {
            assert!(stats.contains(expected), "{expected}: {stats}");
        }

        // The key disappears after the page loaded: SET XX must not recreate it.
        let edit = batch(
            &session,
            "string",
            "greeting",
            vec![row_mutation(
                pair("greeting", "hi"),
                pair("greeting", "hello"),
                false,
            )],
        );
        assert_eq!(session.apply_mutations(&edit).await.unwrap().applied, 1);
        {
            let mut conn = session.conn().await;
            let result = apply_string_mutations(&mut conn, "deleted", &[&edit.mutations[0]])
                .await
                .unwrap();
            assert_eq!(result, vec![false]);
        }
        assert_eq!(
            session
                .run_query("EXISTS deleted", None)
                .await
                .unwrap()
                .rows,
            vec![vec![JsonValue::from(0)]]
        );
    }

    #[tokio::test]
    async fn key_index_ttl_edits_never_delete_keys() {
        let Some((_server, session)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        session.run_query("MSET a 1 b 1 c 1", None).await.unwrap();
        session.run_query("EXPIRE c 100", None).await.unwrap();
        let row =
            |key: &str, ttl: JsonValue| vec![JsonValue::from(key), JsonValue::from("string"), ttl];
        let ttl_edit = |key: &str, ttl: JsonValue| {
            row_mutation(row(key, JsonValue::from(-1)), row(key, ttl), false)
        };
        let error = session
            .apply_mutations(&batch(
                &session,
                "keys",
                "all",
                vec![
                    row_mutation(
                        row("b", JsonValue::from(-1)),
                        row("b", JsonValue::from(-1)),
                        true,
                    ),
                    ttl_edit("a", JsonValue::from(0)),
                ],
            ))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("TTL 0"), "{error}");
        assert_eq!(
            session.run_query("EXISTS a b", None).await.unwrap().rows,
            vec![vec![JsonValue::from(2)]],
            "a rejected batch writes nothing"
        );

        let result = session
            .apply_mutations(&batch(
                &session,
                "keys",
                "all",
                vec![
                    ttl_edit("a", JsonValue::from(60)),
                    row_mutation(
                        row("c", JsonValue::from(100)),
                        row("c", JsonValue::from(-1)),
                        false,
                    ),
                    ttl_edit("missing", JsonValue::from(60)),
                    row_mutation(
                        row("gone", JsonValue::from(-1)),
                        row("gone", JsonValue::from(-5)),
                        false,
                    ),
                    row_mutation(
                        row("b", JsonValue::from(-1)),
                        row("b", JsonValue::from(-1)),
                        true,
                    ),
                ],
            ))
            .await
            .unwrap();
        assert_eq!(result.applied, 3);
        assert_eq!(
            result.conflicts,
            vec![
                vec![JsonValue::from("missing")],
                vec![JsonValue::from("gone")]
            ]
        );
        let ttl = |key: &'static str| {
            let session = &session;
            async move {
                session
                    .run_query(&format!("TTL {key}"), None)
                    .await
                    .unwrap()
                    .rows[0][0]
                    .as_i64()
                    .unwrap()
            }
        };
        assert!((1..=60).contains(&ttl("a").await));
        assert_eq!(ttl("c").await, -1);
        assert_eq!(ttl("b").await, -2);
        assert_eq!(ttl("missing").await, -2, "EXPIRE never creates keys");
        assert_eq!(parse_ttl(Some(&JsonValue::from(""))).unwrap(), None);
        assert_eq!(parse_ttl(None).unwrap(), None);
        assert!(parse_ttl(Some(&JsonValue::from("soon"))).is_err());
    }

    #[tokio::test]
    async fn previews_of_binary_or_truncated_values_cannot_be_saved() {
        let Some((_server, session)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        let long = "a".repeat(MAX_VALUE_BYTES + 10);
        {
            let mut conn = session.conn().await;
            redis_rs::pipe()
                .cmd("SET")
                .arg("bin")
                .arg(b"\xff\x00".as_slice())
                .ignore()
                .cmd("SET")
                .arg("long")
                .arg(&long)
                .ignore()
                .cmd("SET")
                .arg("literal")
                .arg(r"\xzz")
                .ignore()
                .cmd("HSET")
                .arg("h")
                .arg(b"\xfe".as_slice())
                .arg("v")
                .arg("ok")
                .arg(b"\xff".as_slice())
                .ignore()
                .cmd("RPUSH")
                .arg("l")
                .arg(b"\xff".as_slice())
                .ignore()
                .cmd("SADD")
                .arg("s")
                .arg(b"\xff".as_slice())
                .ignore()
                .query_async::<()>(&mut *conn)
                .await
                .unwrap();
        }
        let shown = |schema: &'static str, table: &'static str| {
            let request = page_request(&session, schema, table);
            let session = &session;
            async move { session.table_page(&request).await.unwrap().rows }
        };
        let refused = |schema: &'static str, table: &'static str, mutation: RowMutation| {
            let batch = batch(&session, schema, table, vec![mutation]);
            let session = &session;
            async move {
                let error = session.apply_mutations(&batch).await.unwrap_err();
                assert!(error.to_string().contains("hex"), "{error}");
            }
        };
        let edited = |row: &[JsonValue], value: &str| {
            row_mutation(
                row.to_vec(),
                vec![row[0].clone(), JsonValue::from(value)],
                false,
            )
        };

        let bin = shown("string", "bin").await.remove(0);
        refused("string", "bin", edited(&bin, "x")).await;
        let long_row = shown("string", "long").await.remove(0);
        assert!(long_row[1].as_str().unwrap().ends_with(TRUNCATION_MARKER));
        refused("string", "long", edited(&long_row, "x")).await;
        assert_eq!(
            session.run_query("STRLEN long", None).await.unwrap().rows,
            vec![vec![JsonValue::from(long.len())]]
        );
        let hash = shown("hash", "h").await;
        let binary_field = hash.iter().find(|row| row[0] == r"\xfe").unwrap();
        let binary_value = hash.iter().find(|row| row[0] == "ok").unwrap();
        refused("hash", "h", edited(binary_field, "x")).await;
        refused(
            "hash",
            "h",
            row_mutation(binary_field.clone(), binary_field.clone(), true),
        )
        .await;
        refused("hash", "h", edited(binary_value, "x")).await;
        let list = shown("list", "l").await.remove(0);
        refused("list", "l", edited(&list, "x")).await;
        let set = shown("set", "s").await.remove(0);
        refused("set", "s", row_mutation(set.clone(), set, true)).await;

        // Deleting a row whose value is only a preview is safe, and text that
        // is not a hex preview stays editable.
        for (schema, table, mutation) in [
            (
                "hash",
                "h",
                row_mutation(binary_value.clone(), binary_value.clone(), true),
            ),
            ("list", "l", row_mutation(list.clone(), list.clone(), true)),
            (
                "string",
                "bin",
                row_mutation(bin.clone(), bin.clone(), true),
            ),
        ] {
            let result = session
                .apply_mutations(&batch(&session, schema, table, vec![mutation]))
                .await
                .unwrap();
            assert_eq!(result.applied, 1, "{schema}");
        }
        let literal = shown("string", "literal").await.remove(0);
        let result = session
            .apply_mutations(&batch(
                &session,
                "string",
                "literal",
                vec![edited(&literal, "ok")],
            ))
            .await
            .unwrap();
        assert_eq!(result.applied, 1);
    }

    /// Starts a TLS-only redis-server with a throwaway CA, or `None` when
    /// openssl or a TLS-capable redis-server is unavailable.
    async fn start_tls_redis(
        directory: &std::path::Path,
        password: &str,
    ) -> Option<(TestRedis, u16, String)> {
        let path = |name: &str| directory.join(name).to_string_lossy().into_owned();
        std::fs::write(
            directory.join("server.ext"),
            "subjectAltName=IP:127.0.0.1\nextendedKeyUsage=serverAuth\nkeyUsage=digitalSignature,keyEncipherment\n",
        )
        .ok()?;
        let openssl = |args: &[&str]| {
            Command::new("openssl")
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        };
        let ready = openssl(&[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "2",
            "-subj",
            "/CN=Anybase Test CA",
            "-keyout",
            &path("ca.key"),
            "-out",
            &path("ca.crt"),
        ]) && openssl(&[
            "req",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-subj",
            "/CN=127.0.0.1",
            "-keyout",
            &path("server.key"),
            "-out",
            &path("server.csr"),
        ]) && openssl(&[
            "x509",
            "-req",
            "-days",
            "2",
            "-in",
            &path("server.csr"),
            "-CA",
            &path("ca.crt"),
            "-CAkey",
            &path("ca.key"),
            "-CAcreateserial",
            "-extfile",
            &path("server.ext"),
            "-out",
            &path("server.crt"),
        ]);
        if !ready {
            return None;
        }
        let port = unused_port()?;
        let child = Command::new("redis-server")
            .args([
                "--port",
                "0",
                "--tls-port",
                &port.to_string(),
                "--bind",
                "127.0.0.1",
                "--tls-cert-file",
                &path("server.crt"),
                "--tls-key-file",
                &path("server.key"),
                "--tls-ca-cert-file",
                &path("ca.crt"),
                "--tls-auth-clients",
                "no",
                "--requirepass",
                password,
                "--save",
                "",
                "--appendonly",
                "no",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let server = TestRedis { child };
        let mut profile = test_profile(port, false);
        profile.tls_mode = TlsMode::Required;
        profile.ca_cert_path = Some(path("ca.crt"));
        for _ in 0..80 {
            if RedisSession::connect(profile.clone(), Some(password.into()))
                .await
                .is_ok()
            {
                return Some((server, port, path("ca.crt")));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }

    #[tokio::test]
    async fn tls_profiles_use_their_ca_and_report_auth_errors() {
        let directory = tempfile::tempdir().unwrap();
        let Some((_server, port, ca)) = start_tls_redis(directory.path(), "secret").await else {
            eprintln!(
                "skipping live Redis TLS test because openssl or TLS redis-server is unavailable"
            );
            return;
        };
        let profile = |mode: TlsMode, ca: Option<&str>| {
            let mut profile = test_profile(port, false);
            profile.tls_mode = mode;
            profile.ca_cert_path = ca.map(ToOwned::to_owned);
            profile
        };
        let session =
            RedisSession::connect(profile(TlsMode::Required, Some(&ca)), Some("secret".into()))
                .await
                .expect("the profile CA verifies the server");
        assert_eq!(
            session.run_query("PING", None).await.unwrap().rows,
            vec![vec![JsonValue::from("PONG")]]
        );
        let error = RedisSession::connect(profile(TlsMode::Required, None), Some("secret".into()))
            .await
            .err()
            .expect("an unknown CA is rejected");
        assert!(error.to_string().contains("TLS"), "{error}");

        // A wrong password over working TLS is reported as such, not hidden
        // behind a plaintext retry against the TLS-only port.
        for ca in [Some(ca.as_str()), None] {
            let error =
                RedisSession::connect(profile(TlsMode::Preferred, ca), Some("wrong".into()))
                    .await
                    .err()
                    .expect("wrong password");
            let message = error.to_string();
            assert!(
                message.contains("Password authentication failed"),
                "{message}"
            );
            assert!(!message.contains("plaintext"), "{message}");
        }

        let error = RedisSession::connect(
            profile(
                TlsMode::Required,
                Some(&directory.path().join("missing.pem").to_string_lossy()),
            ),
            Some("secret".into()),
        )
        .await
        .err()
        .expect("missing CA file");
        assert!(
            error.to_string().contains("could not read CA certificate"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn preferred_tls_falls_back_and_passwords_keep_their_spaces() {
        let Some((_server, session)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        let port = session.profile().port;
        session
            .run_query(r#"CONFIG SET requirepass " spaced pass ""#, None)
            .await
            .unwrap();
        let mut profile = test_profile(port, false);
        profile.tls_mode = TlsMode::Preferred;
        let fallback = RedisSession::connect(profile.clone(), Some(" spaced pass ".into()))
            .await
            .expect("plaintext fallback after a failed TLS handshake");
        assert_eq!(
            fallback.run_query("PING", None).await.unwrap().rows,
            vec![vec![JsonValue::from("PONG")]]
        );
        profile.tls_mode = TlsMode::Disabled;
        assert!(
            RedisSession::connect(profile, Some("spaced pass".into()))
                .await
                .is_err(),
            "the trimmed password is a different password"
        );
    }

    #[tokio::test]
    async fn redis_read_only_profiles_block_writes() {
        let Some((_server, writable)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        let port = writable.profile().port;
        drop(writable);
        let session = RedisSession::connect(test_profile(port, true), None)
            .await
            .expect("connect read-only");
        let error = session
            .run_query("SET blocked 1", None)
            .await
            .expect_err("write blocked");
        assert!(error.to_string().contains("read-only"));
    }
}
