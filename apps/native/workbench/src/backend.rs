//! The serialized database worker: one thread with a small Tokio runtime owns
//! the shared core state (or the isolated demo fixture) and answers UI
//! commands in order.

use std::future::Future;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use dbm_core::models::{
    ConnectionProfile, DatabaseEngine, DatabaseRef, FilterCondition, FilterOperator, MutationBatch,
    MutationResult, OrderSpec, QueryColumn, QueryHistoryEntry, QueryRequest, QueryResponse,
    SaveProfileInput, SchemaNode, TableColumn, TableMetadata, TablePage, TablePageRequest, TlsMode,
    WorkspaceInfo,
};
use dbm_core::session::DbSession;
use dbm_core::sql_text::display_value;
use dbm_core::state::AppState;
use eframe::egui;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RequestId(pub u64);

pub enum Command {
    LoadProfiles,
    SaveProfile(SaveProfileInput),
    TestProfile(SaveProfileInput),
    DeleteProfile(Uuid),
    Connect(Uuid),
    SwitchDatabase(Uuid, String),
    Disconnect(Uuid),
    Schema(Uuid),
    Query(QueryRequest),
    History(Uuid, String),
    Table(TablePageRequest),
    Mutate(MutationBatch),
    /// Asks for a destination, then writes every filtered row as CSV.
    ExportCsv(ExportRequest),
}

pub struct ExportRequest {
    pub page: TablePageRequest,
    pub columns: Vec<String>,
    pub file_name: String,
}

pub enum Payload {
    Profiles(Vec<ConnectionProfile>),
    Profile(ConnectionProfile),
    Tested,
    Deleted(Uuid),
    Workspace(WorkspaceInfo),
    Disconnected(Uuid),
    Schema(Uuid, Vec<SchemaNode>),
    Query(QueryResponse),
    History(Uuid, String, Vec<QueryHistoryEntry>),
    Table(TablePage),
    Mutation(MutationResult),
    Exported(Option<(PathBuf, u64)>),
}

pub struct Completion {
    pub id: RequestId,
    pub result: Result<Payload, String>,
}

pub struct Work {
    pub id: RequestId,
    pub command: Command,
}

pub struct Worker {
    pub tx: Sender<Work>,
    pub rx: Receiver<Completion>,
}

impl Worker {
    pub fn start(context: egui::Context, demo: bool) -> Self {
        let (work_tx, work_rx) = mpsc::channel::<Work>();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("dbm-serialized-worker".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .expect("database runtime");
                let mut backend = if demo {
                    Backend::Demo(DemoBackend::new())
                } else {
                    match AppState::new() {
                        Ok(state) => Backend::Live(state),
                        Err(error) => Backend::Unavailable(error.to_string()),
                    }
                };
                while let Ok(work) = work_rx.recv() {
                    let result = runtime.block_on(backend.execute(work.command));
                    if done_tx
                        .send(Completion {
                            id: work.id,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                    context.request_repaint();
                }
            })
            .expect("start database worker");
        Self {
            tx: work_tx,
            rx: done_rx,
        }
    }
}

enum Backend {
    Live(AppState),
    Demo(DemoBackend),
    Unavailable(String),
}

impl Backend {
    async fn execute(&mut self, command: Command) -> Result<Payload, String> {
        match self {
            Self::Live(state) => execute_live(state, command).await,
            Self::Demo(demo) => demo.execute(command).await,
            Self::Unavailable(error) => Err(error.clone()),
        }
    }
}

async fn execute_live(state: &AppState, command: Command) -> Result<Payload, String> {
    let result = match command {
        Command::LoadProfiles => Payload::Profiles(
            state
                .profile_summaries()
                .map_err(string_error)?
                .into_iter()
                .map(|s| s.profile)
                .collect(),
        ),
        Command::SaveProfile(input) => {
            let password = input.password.clone();
            let profile = state.save_profile(input).map_err(string_error)?;
            if let Some(password) = password {
                if password.is_empty() {
                    state.credentials.delete_password(profile.id)
                } else {
                    state.credentials.save_password(profile.id, &password)
                }
                .map_err(string_error)?;
            }
            state.disconnect(profile.id).await;
            Payload::Profile(profile)
        }
        Command::TestProfile(input) => {
            let profile = transient_profile(&input).map_err(string_error)?;
            let password = match input.password.filter(|value| !value.is_empty()) {
                Some(password) => Some(password),
                None => input
                    .id
                    .map(|id| state.credentials.get_password(id))
                    .transpose()
                    .map_err(string_error)?
                    .flatten(),
            };
            let session = DbSession::connect(profile, password)
                .await
                .map_err(string_error)?;
            session.close().await;
            Payload::Tested
        }
        Command::DeleteProfile(id) => {
            state.disconnect(id).await;
            state
                .credentials
                .delete_password(id)
                .map_err(string_error)?;
            state.store.delete_profile(id).map_err(string_error)?;
            Payload::Deleted(id)
        }
        Command::Connect(id) => {
            let profile = state.profile(id).map_err(string_error)?;
            let session = state.connect(profile).await.map_err(string_error)?;
            let databases = session.list_databases().await.map_err(string_error)?;
            Payload::Workspace(WorkspaceInfo {
                profile: session.profile().clone(),
                databases,
            })
        }
        Command::SwitchDatabase(id, database) => {
            let mut profile = state.profile(id).map_err(string_error)?;
            profile.default_database = database;
            let session = state.connect(profile).await.map_err(string_error)?;
            let databases = session.list_databases().await.map_err(string_error)?;
            Payload::Workspace(WorkspaceInfo {
                profile: session.profile().clone(),
                databases,
            })
        }
        Command::Disconnect(id) => {
            state.disconnect(id).await;
            Payload::Disconnected(id)
        }
        Command::Schema(id) => Payload::Schema(
            id,
            state
                .with_session_retry(id, |s| async move { s.schema_tree().await })
                .await
                .map_err(string_error)?,
        ),
        Command::Query(request) => {
            Payload::Query(state.run_query(request).await.map_err(string_error)?)
        }
        Command::History(id, database) => Payload::History(
            id,
            database.clone(),
            state
                .store
                .list_history(id, &database, 100)
                .map_err(string_error)?,
        ),
        Command::Table(request) => Payload::Table(live_table_page(state, request).await?),
        Command::Mutate(batch) => {
            let profile = state.profile(batch.profile_id).map_err(string_error)?;
            if profile.read_only {
                return Err("This connection is read-only.".into());
            }
            Payload::Mutation(
                state
                    .session(batch.profile_id)
                    .await
                    .map_err(string_error)?
                    .apply_mutations(&batch)
                    .await
                    .map_err(string_error)?,
            )
        }
        Command::ExportCsv(request) => {
            Payload::Exported(export_csv(request, |page| live_table_page(state, page)).await?)
        }
    };
    Ok(result)
}

async fn live_table_page(state: &AppState, request: TablePageRequest) -> Result<TablePage, String> {
    state
        .with_session_retry(request.profile_id, |s| {
            let request = request.clone();
            async move { s.table_page(&request).await }
        })
        .await
        .map_err(string_error)
}

/// Asks for a destination, then streams every filtered row there with the
/// shared exporter. `None` means the dialog was canceled.
async fn export_csv<F, Fut>(
    request: ExportRequest,
    load: F,
) -> Result<Option<(PathBuf, u64)>, String>
where
    F: FnMut(TablePageRequest) -> Fut,
    Fut: Future<Output = Result<TablePage, String>>,
{
    let Some(path) = rfd::FileDialog::new()
        .set_file_name(&request.file_name)
        .add_filter("CSV", &["csv"])
        .save_file()
    else {
        return Ok(None);
    };
    let rows = dbm_core::export::export_csv(&path, &request.columns, &request.page, load).await?;
    Ok(Some((path, rows)))
}

pub fn transient_profile(
    input: &SaveProfileInput,
) -> dbm_core::error::AppResult<ConnectionProfile> {
    let now = chrono::Utc::now();
    let profile = ConnectionProfile {
        id: input.id.unwrap_or_else(Uuid::new_v4),
        name: input.name.trim().into(),
        color: input.color.clone(),
        engine: input.engine,
        host: input.host.trim().into(),
        port: input.port,
        username: input.username.trim().into(),
        default_database: input.default_database.trim().into(),
        tls_mode: input.tls_mode.clone(),
        ca_cert_path: input.ca_cert_path.clone(),
        ssh: input.ssh.clone(),
        read_only: input.read_only,
        created_at: now,
        updated_at: now,
    };
    profile.validate()?;
    Ok(profile)
}

fn string_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// Deterministic in-memory fixture for `--demo`: no local profiles, network,
/// or persistence. Filters on text columns and ordering are applied so the
/// table controls can be exercised, but this does not validate SQL semantics.
pub struct DemoBackend {
    pub profiles: Vec<ConnectionProfile>,
    history: Vec<QueryHistoryEntry>,
}

impl DemoBackend {
    pub fn new() -> Self {
        let now = chrono::Utc::now();
        let profile =
            |id, name: &str, color: &str, engine, port, database: &str| ConnectionProfile {
                id: Uuid::from_u128(id),
                name: name.into(),
                color: Some(color.into()),
                engine,
                host: "demo.invalid".into(),
                port,
                username: "demo".into(),
                default_database: database.into(),
                tls_mode: TlsMode::Required,
                ca_cert_path: None,
                ssh: None,
                read_only: false,
                created_at: now,
                updated_at: now,
            };
        Self {
            profiles: vec![
                profile(
                    1,
                    "Acme Analytics",
                    "#3dd6c6",
                    DatabaseEngine::Postgres,
                    5432,
                    "analytics",
                ),
                profile(
                    2,
                    "Session Cache",
                    "#ff9f43",
                    DatabaseEngine::Redis,
                    6379,
                    "0",
                ),
            ],
            history: Vec::new(),
        }
    }

    fn engine(&self, id: Uuid) -> DatabaseEngine {
        self.profiles
            .iter()
            .find(|p| p.id == id)
            .map_or(DatabaseEngine::Postgres, |p| p.engine)
    }

    async fn execute(&mut self, command: Command) -> Result<Payload, String> {
        Ok(match command {
            Command::LoadProfiles => Payload::Profiles(self.profiles.clone()),
            Command::SaveProfile(input) => {
                let profile = transient_profile(&input).map_err(string_error)?;
                if let Some(old) = self.profiles.iter_mut().find(|x| x.id == profile.id) {
                    *old = profile.clone();
                } else {
                    self.profiles.push(profile.clone());
                }
                Payload::Profile(profile)
            }
            Command::TestProfile(_) => Payload::Tested,
            Command::DeleteProfile(id) => {
                self.profiles.retain(|p| p.id != id);
                Payload::Deleted(id)
            }
            Command::Connect(id) | Command::SwitchDatabase(id, _) => {
                let mut profile = self
                    .profiles
                    .iter()
                    .find(|profile| profile.id == id)
                    .cloned()
                    .ok_or("profile not found")?;
                if let Command::SwitchDatabase(_, database) = command {
                    profile.default_database = database;
                }
                let names: &[&str] = if profile.engine == DatabaseEngine::Redis {
                    &["0", "1", "2"]
                } else {
                    &["analytics", "warehouse"]
                };
                Payload::Workspace(WorkspaceInfo {
                    profile,
                    databases: names
                        .iter()
                        .map(|name| DatabaseRef {
                            name: (*name).into(),
                            is_template: false,
                            is_connectable: true,
                        })
                        .collect(),
                })
            }
            Command::Disconnect(id) => Payload::Disconnected(id),
            Command::Schema(id) => Payload::Schema(id, demo_schema(self.engine(id))),
            Command::Query(request) => {
                let response = demo_query(self.engine(request.profile_id), &request.sql);
                self.history.insert(
                    0,
                    QueryHistoryEntry {
                        id: Uuid::new_v4(),
                        profile_id: request.profile_id,
                        database: String::new(),
                        sql: request.sql,
                        executed_at: chrono::Utc::now(),
                        duration_ms: response.duration_ms,
                        success: true,
                    },
                );
                Payload::Query(response)
            }
            Command::History(id, database) => Payload::History(
                id,
                database,
                self.history
                    .iter()
                    .filter(|entry| entry.profile_id == id)
                    .cloned()
                    .collect(),
            ),
            Command::Table(request) => Payload::Table(demo_page(&request)),
            Command::Mutate(_) => {
                return Err(
                    "Demo fixture: saving is disabled. Staged edits have been retained.".into(),
                );
            }
            Command::ExportCsv(request) => Payload::Exported(
                export_csv(request, |page| async move { Ok(demo_page(&page)) }).await?,
            ),
        })
    }
}

fn demo_schema(engine: DatabaseEngine) -> Vec<SchemaNode> {
    let leaf = |kind: &str, schema: &str, name: &str| SchemaNode {
        name: name.into(),
        kind: kind.into(),
        schema: Some(schema.into()),
        table: Some(name.into()),
        children: vec![],
    };
    let folder = |name: &str, schema: &str, children| SchemaNode {
        name: name.into(),
        kind: "schema".into(),
        schema: Some(schema.into()),
        table: None,
        children,
    };
    if engine == DatabaseEngine::Redis {
        return vec![
            folder("Keys", "keys", vec![leaf("table", "keys", "all")]),
            folder(
                "Strings",
                "string",
                vec![
                    leaf("key", "string", "greeting"),
                    leaf("key", "string", "session:42"),
                ],
            ),
            folder("Hashes", "hash", vec![leaf("key", "hash", "user:1")]),
        ];
    }
    vec![
        folder(
            "public",
            "public",
            vec![
                leaf("table", "public", "customers"),
                leaf("table", "public", "orders"),
                leaf("view", "public", "revenue_by_month"),
            ],
        ),
        folder("audit", "audit", vec![leaf("table", "audit", "events")]),
    ]
}

fn demo_query(engine: DatabaseEngine, sql: &str) -> QueryResponse {
    let column = |name: &str, data_type: &str| QueryColumn {
        name: name.into(),
        data_type: data_type.into(),
    };
    let (columns, rows) = if engine == DatabaseEngine::Redis {
        (vec![column("result", "redis")], vec![vec![json!("PONG")]])
    } else {
        (
            vec![column("customer", "text"), column("revenue", "numeric")],
            vec![
                vec![json!("Ada Lovelace"), json!("12450.25")],
                vec![json!("Grace Hopper"), json!("9810.00")],
                vec![json!("Katherine Johnson"), Value::Null],
            ],
        )
    };
    QueryResponse {
        row_count: rows.len(),
        columns,
        rows,
        affected_rows: None,
        duration_ms: 7 + (sql.len() % 5) as u128,
        truncated: false,
        notices: vec![],
    }
}

pub fn demo_page(req: &TablePageRequest) -> TablePage {
    let column = |name: &str, data_type: &str, nullable, ordinal| TableColumn {
        name: name.into(),
        data_type: data_type.into(),
        nullable,
        default_value: None,
        ordinal,
    };
    let columns = vec![
        column("id", "bigint", false, 1),
        column("email", "text", false, 2),
        column("active", "boolean", false, 3),
        column("note", "text", true, 4),
    ];
    let mut all: Vec<Vec<Value>> = (1..=500)
        .map(|i| {
            vec![
                json!(i),
                json!(format!("person{i}@example.com")),
                json!(i % 3 != 0),
                if i % 4 == 0 {
                    Value::Null
                } else {
                    json!(format!("Customer since {}", 2000 + i % 25))
                },
                json!(format!("xmin-{i}")),
            ]
        })
        .collect();
    all.retain(|row| {
        req.filters
            .iter()
            .all(|filter| demo_filter(&columns, row, filter))
    });
    if let Some(OrderSpec { column, descending }) = &req.order_by {
        if let Some(index) = columns.iter().position(|c| &c.name == column) {
            all.sort_by(|a, b| {
                let ordering = match (&a[index], &b[index]) {
                    (Value::Number(x), Value::Number(y)) => x
                        .as_f64()
                        .partial_cmp(&y.as_f64())
                        .unwrap_or(std::cmp::Ordering::Equal),
                    (x, y) => display_value(x).cmp(&display_value(y)),
                };
                if *descending {
                    ordering.reverse()
                } else {
                    ordering
                }
            });
        }
    }
    let total = all.len() as u32;
    let rows = all
        .into_iter()
        .skip(req.offset as usize)
        .take(req.limit as usize)
        .collect();
    TablePage {
        metadata: TableMetadata {
            schema: req.schema.clone(),
            table: req.table.clone(),
            columns: columns.clone(),
            primary_key: vec!["id".into()],
            has_xmin: true,
        },
        columns: vec![
            "id".into(),
            "email".into(),
            "active".into(),
            "note".into(),
            "__dbm_xmin".into(),
        ],
        rows,
        total_rows: Some(u64::from(total)),
        offset: req.offset,
        limit: req.limit,
        has_more: req.offset + req.limit < total,
    }
}

fn demo_filter(columns: &[TableColumn], row: &[Value], filter: &FilterCondition) -> bool {
    let Some(index) = columns.iter().position(|c| c.name == filter.column) else {
        return true;
    };
    let cell = &row[index];
    let text = display_value(cell).to_lowercase();
    let value = filter.value.clone().unwrap_or_default().to_lowercase();
    let list = || {
        value
            .split(',')
            .map(|v| v.trim().to_owned())
            .collect::<Vec<_>>()
    };
    let compare = || {
        let (Ok(left), Ok(right)) = (text.parse::<f64>(), value.parse::<f64>()) else {
            return text.cmp(&value);
        };
        left.partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal)
    };
    match filter.operator {
        FilterOperator::Equals => text == value,
        FilterOperator::NotEquals => text != value,
        FilterOperator::Contains => text.contains(&value),
        FilterOperator::StartsWith => text.starts_with(&value),
        FilterOperator::EndsWith => text.ends_with(&value),
        FilterOperator::GreaterThan => compare().is_gt(),
        FilterOperator::GreaterThanOrEqual => compare().is_ge(),
        FilterOperator::LessThan => compare().is_lt(),
        FilterOperator::LessThanOrEqual => compare().is_le(),
        FilterOperator::In => list().contains(&text),
        FilterOperator::NotIn => !list().contains(&text),
        FilterOperator::IsNull => cell.is_null(),
        FilterOperator::IsNotNull => !cell.is_null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(offset: u32, limit: u32) -> TablePageRequest {
        TablePageRequest {
            profile_id: Uuid::nil(),
            schema: "public".into(),
            table: "customers".into(),
            offset,
            limit,
            filters: vec![],
            order_by: None,
            include_total: Some(true),
        }
    }

    #[test]
    fn page_offsets_are_stable() {
        let page = demo_page(&request(200, 200));
        assert_eq!(page.rows[0][0], json!(201));
        assert!(page.has_more);
    }

    #[test]
    fn demo_filters_and_order_apply() {
        let mut req = request(0, 10);
        req.filters = vec![FilterCondition {
            column: "note".into(),
            operator: FilterOperator::IsNull,
            value: None,
        }];
        req.order_by = Some(OrderSpec {
            column: "id".into(),
            descending: true,
        });
        let page = demo_page(&req);
        assert_eq!(page.total_rows, Some(125));
        assert_eq!(page.rows[0][0], json!(500));
    }
}
