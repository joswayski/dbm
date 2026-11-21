//! The database worker: a small Tokio runtime owns the shared core state (or
//! the isolated demo fixture) and answers UI commands. Commands for one
//! connection run in the order they were sent, because they share its
//! session; different connections run at the same time, so a slow query or
//! an unreachable host never holds up the others.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, Sender};

use dbm_core::demo::{self, DemoStore, profile_from_input};
use dbm_core::models::{
    ConnectionProfile, MutationBatch, MutationResult, QueryHistoryEntry, QueryRequest,
    QueryResponse, SaveProfileInput, SchemaNode, TablePage, TablePageRequest, WorkspaceInfo,
};
use dbm_core::session::DbSession;
use dbm_core::state::AppState;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RequestId(pub u64);

pub enum Command {
    LoadProfiles,
    SaveProfile(SaveProfileInput),
    TestProfile(SaveProfileInput),
    ConnectionUrl(SaveProfileInput, bool),
    DeleteProfile(Uuid),
    Connect(Uuid),
    SwitchDatabase(Uuid, String),
    Disconnect(Uuid),
    Schema(Uuid),
    Query(QueryRequest),
    History(Uuid, String),
    Table(TablePageRequest),
    Mutate(MutationBatch),
    /// Writes every filtered row as CSV to the chosen destination.
    ExportCsv(ExportRequest),
}

/// Commands in one lane run one at a time, in the order sent; lanes run
/// concurrently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lane {
    /// Work that belongs to no saved connection: listing profiles, and the
    /// connection editor's tests and new saves.
    App,
    /// Everything that uses or replaces one connection's session.
    Profile(Uuid),
}

impl Command {
    pub fn lane(&self) -> Lane {
        match self {
            Self::LoadProfiles | Self::TestProfile(_) | Self::ConnectionUrl(..) => Lane::App,
            // Saving an existing profile closes its session.
            Self::SaveProfile(input) => input.id.map_or(Lane::App, Lane::Profile),
            Self::DeleteProfile(id)
            | Self::Connect(id)
            | Self::SwitchDatabase(id, _)
            | Self::Disconnect(id)
            | Self::Schema(id)
            | Self::History(id, _) => Lane::Profile(*id),
            Self::Query(request) => Lane::Profile(request.profile_id),
            Self::Table(request) => Lane::Profile(request.profile_id),
            Self::Mutate(batch) => Lane::Profile(batch.profile_id),
            Self::ExportCsv(request) => Lane::Profile(request.page.profile_id),
        }
    }
}

pub struct ExportRequest {
    pub page: TablePageRequest,
    pub columns: Vec<String>,
    /// Chosen on the UI side before the request is sent, so the dialog never
    /// holds up the worker.
    pub path: PathBuf,
    /// Rows written so far, for the "Exporting N / total…" label.
    pub progress: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

pub enum Payload {
    Profiles(Vec<ConnectionProfile>),
    Profile(ConnectionProfile),
    Tested,
    ConnectionUrl(String),
    Deleted(Uuid),
    Workspace(WorkspaceInfo),
    Disconnected(Uuid),
    Schema(Uuid, Vec<SchemaNode>),
    Query(QueryResponse),
    History(Uuid, String, Vec<QueryHistoryEntry>),
    Table(TablePage),
    Mutation(MutationResult),
    Exported(PathBuf, u64),
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
    pub fn start(wake: crate::Wake, demo: bool) -> Self {
        Self::serve(wake, move || {
            let backend = Arc::new(if demo {
                Backend::Demo(tokio::sync::Mutex::new(DemoBackend::new()))
            } else {
                match AppState::new() {
                    Ok(state) => Backend::Live(state),
                    Err(error) => Backend::Unavailable(error.to_string()),
                }
            });
            move |command| {
                let backend = backend.clone();
                async move { backend.execute(command).await }
            }
        })
    }

    /// Answers commands with the executor `open` returns, each lane in order
    /// and the lanes concurrently. `open` runs on the worker thread, so
    /// opening the local store never delays the first frame.
    fn serve<Open, Execute, Answer>(wake: crate::Wake, open: Open) -> Self
    where
        Open: FnOnce() -> Execute + Send + 'static,
        Execute: Fn(Command) -> Answer + Send + Sync + 'static,
        Answer: Future<Output = Result<Payload, String>> + Send + 'static,
    {
        let (work_tx, work_rx) = mpsc::channel::<Work>();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("dbm-database-worker".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .expect("database runtime");
                let execute = Arc::new(open());
                let mut lanes = HashMap::new();
                while let Ok(work) = work_rx.recv() {
                    let lane = work.command.lane();
                    let queue = lanes.entry(lane).or_insert_with(|| {
                        let (queue, waiting) = tokio::sync::mpsc::unbounded_channel();
                        runtime.spawn(run_lane(
                            waiting,
                            execute.clone(),
                            done_tx.clone(),
                            wake.clone(),
                        ));
                        queue
                    });
                    // A lane only closes with the runtime.
                    let _ = queue.send(work);
                }
            })
            .expect("start database worker");
        Self {
            tx: work_tx,
            rx: done_rx,
        }
    }
}

/// Answers one lane's commands in order.
async fn run_lane<Execute, Answer>(
    mut waiting: tokio::sync::mpsc::UnboundedReceiver<Work>,
    execute: Arc<Execute>,
    done: Sender<Completion>,
    wake: crate::Wake,
) where
    Execute: Fn(Command) -> Answer,
    Answer: Future<Output = Result<Payload, String>> + Send + 'static,
{
    while let Some(Work { id, command }) = waiting.recv().await {
        // On its own task, so a panic fails this request instead of ending
        // the lane and leaving the UI waiting on every later one.
        let result = tokio::spawn(execute(command))
            .await
            .unwrap_or_else(|_| Err(WORKER_PANIC.into()));
        if done.send(Completion { id, result }).is_err() {
            break;
        }
        wake();
    }
}

const WORKER_PANIC: &str = "The request stopped unexpectedly. A submitted write may have completed; check before retrying.";

enum Backend {
    Live(AppState),
    Demo(tokio::sync::Mutex<DemoBackend>),
    Unavailable(String),
}

impl Backend {
    async fn execute(&self, command: Command) -> Result<Payload, String> {
        match self {
            Self::Live(state) => execute_live(state, command).await,
            Self::Demo(backend) => backend.lock().await.execute(command).await,
            Self::Unavailable(error) => Err(error.clone()),
        }
    }
}

async fn execute_live(state: &AppState, command: Command) -> Result<Payload, String> {
    let result = match command {
        Command::ConnectionUrl(input, include_password) => Payload::ConnectionUrl(
            state
                .connection_url(&input, include_password)
                .map_err(string_error)?,
        ),
        Command::LoadProfiles => Payload::Profiles(
            state
                .profile_summaries()
                .map_err(string_error)?
                .into_iter()
                .map(|s| s.profile)
                .collect(),
        ),
        Command::SaveProfile(input) => {
            let profile = state.save_profile(input).map_err(string_error)?;
            state.disconnect(profile.id).await;
            Payload::Profile(profile)
        }
        Command::TestProfile(input) => {
            let profile = profile_from_input(&input).map_err(string_error)?;
            let password = match input.password {
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
            let session = state
                .connect_database(id, &database)
                .await
                .map_err(string_error)?;
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
            let progress = |rows: u64| request.progress.store(rows, Ordering::Relaxed);
            let rows = dbm_core::export::export_table(
                state,
                &request.path,
                &request.columns,
                &request.page,
                &progress,
            )
            .await?;
            Payload::Exported(request.path, rows)
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

/// The outcome of saving a query result: the file and its row count, `None`
/// when the dialog was canceled, or the error that stopped the write.
pub type ResultExport = Result<Option<(PathBuf, u64)>, String>;

/// Pages the demo fixture into the chosen file with the shared exporter.
async fn export_demo_csv(request: ExportRequest) -> Result<(PathBuf, u64), String> {
    let path = request.path;
    let progress = request.progress;
    progress.store(0, Ordering::Relaxed);
    // Each page request starts at the number of rows already written.
    let tracked = |page: TablePageRequest| {
        progress.store(u64::from(page.offset), Ordering::Relaxed);
        async move { Ok::<_, String>(demo::table_page(&page)) }
    };
    let rows =
        dbm_core::export::export_csv(&path, &request.columns, &request.page, tracked).await?;
    Ok((path, rows))
}

fn string_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// `--demo` answers from the shared in-memory fixture: no local profiles,
/// network, or persistence.
struct DemoBackend {
    store: DemoStore,
}

impl DemoBackend {
    fn new() -> Self {
        Self {
            store: DemoStore::new(),
        }
    }

    async fn execute(&mut self, command: Command) -> Result<Payload, String> {
        let store = &mut self.store;
        Ok(match command {
            Command::ConnectionUrl(input, include_password) => {
                Payload::ConnectionUrl(dbm_core::connection_url::format_connection_url(
                    &input,
                    if include_password {
                        input.password.as_deref()
                    } else {
                        None
                    },
                )?)
            }
            Command::LoadProfiles => Payload::Profiles(store.profiles()),
            Command::SaveProfile(input) => {
                Payload::Profile(store.save_profile(&input).map_err(string_error)?)
            }
            Command::TestProfile(input) => {
                profile_from_input(&input).map_err(string_error)?;
                Payload::Tested
            }
            Command::DeleteProfile(id) => {
                store.delete_profile(id);
                Payload::Deleted(id)
            }
            Command::Connect(id) => {
                Payload::Workspace(store.workspace(id, None).map_err(string_error)?)
            }
            Command::SwitchDatabase(id, database) => {
                Payload::Workspace(store.workspace(id, Some(&database)).map_err(string_error)?)
            }
            Command::Disconnect(id) => Payload::Disconnected(id),
            Command::Schema(id) => Payload::Schema(id, store.schema_tree(id)),
            Command::Query(request) => Payload::Query(store.run_query(&request)),
            Command::History(id, database) => {
                Payload::History(id, database, store.history(id, 100))
            }
            Command::Table(request) => Payload::Table(demo::table_page(&request)),
            Command::Mutate(_) => return Err(store.apply_mutations().unwrap_err()),
            Command::ExportCsv(request) => {
                let (path, rows) = export_demo_csv(request).await?;
                Payload::Exported(path, rows)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;

    const WAIT: Duration = Duration::from_secs(10);

    #[test]
    fn connections_run_side_by_side_and_each_keeps_its_order() {
        let slow = Uuid::from_u128(1);
        let fast = Uuid::from_u128(2);
        // The slow connection's first request waits until the test lets it go.
        let (release, held) = tokio::sync::oneshot::channel::<()>();
        let held = Arc::new(Mutex::new(Some(held)));
        let started = Arc::new(Mutex::new(Vec::new()));
        let seen = started.clone();
        let worker = Worker::serve(Arc::new(|| {}), move || {
            move |command: Command| {
                let held = held.clone();
                let seen = seen.clone();
                async move {
                    match command {
                        Command::Schema(id) => {
                            seen.lock().unwrap().push("schema");
                            let held = (id == slow).then(|| held.lock().unwrap().take()).flatten();
                            if let Some(held) = held {
                                let _ = held.await;
                            }
                            Ok(Payload::Schema(id, Vec::new()))
                        }
                        Command::Disconnect(id) => {
                            seen.lock().unwrap().push("disconnect");
                            Ok(Payload::Disconnected(id))
                        }
                        Command::Connect(_) => panic!("fixture"),
                        _ => Err("unexpected".into()),
                    }
                }
            }
        });
        let send = |id, command| {
            worker
                .tx
                .send(Work {
                    id: RequestId(id),
                    command,
                })
                .unwrap()
        };
        send(1, Command::Schema(slow));
        send(2, Command::Disconnect(slow));
        send(3, Command::Schema(fast));
        // The other connection answers while the first is still busy.
        assert_eq!(worker.rx.recv_timeout(WAIT).unwrap().id, RequestId(3));
        assert!(worker.rx.try_recv().is_err());
        assert!(!started.lock().unwrap().contains(&"disconnect"));
        release.send(()).unwrap();
        assert_eq!(worker.rx.recv_timeout(WAIT).unwrap().id, RequestId(1));
        assert_eq!(worker.rx.recv_timeout(WAIT).unwrap().id, RequestId(2));
        // A request that panics reports an error and its connection carries on.
        send(4, Command::Connect(slow));
        send(5, Command::Schema(slow));
        let failed = worker.rx.recv_timeout(WAIT).unwrap();
        assert_eq!(failed.id, RequestId(4));
        assert!(failed.result.is_err());
        assert_eq!(worker.rx.recv_timeout(WAIT).unwrap().id, RequestId(5));
    }

    #[test]
    fn commands_wait_behind_the_connection_they_use() {
        let id = Uuid::from_u128(7);
        let input = |id| SaveProfileInput {
            id,
            name: "Local".into(),
            color: None,
            engine: dbm_core::models::DatabaseEngine::Postgres,
            host: "127.0.0.1".into(),
            port: 5432,
            username: "postgres".into(),
            default_database: "postgres".into(),
            tls_mode: dbm_core::models::TlsMode::Disabled,
            ca_cert_path: None,
            ssh: None,
            read_only: false,
            open_on_startup: false,
            password: None,
        };
        assert_eq!(Command::Connect(id).lane(), Lane::Profile(id));
        assert_eq!(Command::DeleteProfile(id).lane(), Lane::Profile(id));
        // Saving an existing profile closes its session; a new one has none.
        assert_eq!(
            Command::SaveProfile(input(Some(id))).lane(),
            Lane::Profile(id)
        );
        assert_eq!(Command::SaveProfile(input(None)).lane(), Lane::App);
        // A connection test never touches the open session, so it does not
        // wait behind that connection's running query.
        assert_eq!(Command::TestProfile(input(Some(id))).lane(), Lane::App);
        assert_eq!(Command::LoadProfiles.lane(), Lane::App);
    }
}
