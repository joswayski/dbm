//! The serialized database worker: one thread with a small Tokio runtime owns
//! the shared core state (or the isolated demo fixture) and answers UI
//! commands in order.

use std::future::Future;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use dbm_core::demo::{self, DemoStore, profile_from_input};
use dbm_core::models::{
    ConnectionProfile, MutationBatch, MutationResult, QueryHistoryEntry, QueryRequest,
    QueryResponse, SaveProfileInput, SchemaNode, TablePage, TablePageRequest, WorkspaceInfo,
};
use dbm_core::session::DbSession;
use dbm_core::state::AppState;
use eframe::egui;
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
    /// Rows written so far, for the "Exporting N / total…" label.
    pub progress: std::sync::Arc<std::sync::atomic::AtomicU64>,
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
            Self::Demo(backend) => backend.execute(command).await,
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
            let profile = profile_from_input(&input).map_err(string_error)?;
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
    mut load: F,
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
    let progress = request.progress;
    progress.store(0, std::sync::atomic::Ordering::Relaxed);
    // Each page request starts at the number of rows already written.
    let tracked = |page: TablePageRequest| {
        progress.store(u64::from(page.offset), std::sync::atomic::Ordering::Relaxed);
        load(page)
    };
    let rows =
        dbm_core::export::export_csv(&path, &request.columns, &request.page, tracked).await?;
    Ok(Some((path, rows)))
}

fn string_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// `--demo` answers from the shared in-memory fixture: no local profiles,
/// network, or persistence.
pub struct DemoBackend {
    store: DemoStore,
}

impl DemoBackend {
    pub fn new() -> Self {
        Self {
            store: DemoStore::new(),
        }
    }

    async fn execute(&mut self, command: Command) -> Result<Payload, String> {
        let store = &mut self.store;
        Ok(match command {
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
            Command::ExportCsv(request) => Payload::Exported(
                export_csv(request, |page| async move { Ok(demo::table_page(&page)) }).await?,
            ),
        })
    }
}
