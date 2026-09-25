use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{self, Receiver, Sender};

use dbm_core::models::{
    ConnectionProfile, DatabaseEngine, DatabaseRef, FilterCondition, FilterOperator, MutationBatch,
    MutationResult, OrderSpec, QueryColumn, QueryHistoryEntry, QueryRequest, QueryResponse,
    RowMutation, SaveProfileInput, SchemaNode, TableColumn, TableMetadata, TablePage,
    TablePageRequest, TlsMode, WorkspaceInfo,
};
use dbm_core::session::DbSession;
use dbm_core::state::AppState;
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};
use serde_json::{Value, json};
use uuid::Uuid;

const PAGE_SIZE: u32 = 200;
const MAX_QUERY_ROWS: u32 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct RequestId(u64);

enum Command {
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
}

enum Payload {
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
}

struct Completion {
    id: RequestId,
    generation: u64,
    result: Result<Payload, String>,
}

struct Work {
    id: RequestId,
    generation: u64,
    command: Command,
}

struct Worker {
    tx: Sender<Work>,
    rx: Receiver<Completion>,
}

impl Worker {
    fn start(context: egui::Context, demo: bool) -> Self {
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
                            generation: work.generation,
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
            Self::Demo(demo) => demo.execute(command),
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
        Command::Table(request) => {
            let id = request.profile_id;
            Payload::Table(
                state
                    .with_session_retry(id, |s| {
                        let request = request.clone();
                        async move { s.table_page(&request).await }
                    })
                    .await
                    .map_err(string_error)?,
            )
        }
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
    };
    Ok(result)
}

fn transient_profile(input: &SaveProfileInput) -> dbm_core::error::AppResult<ConnectionProfile> {
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

#[derive(Clone)]
enum TabKind {
    Query,
    Table { schema: String, table: String },
}

#[derive(Clone)]
struct Tab {
    id: u64,
    profile_id: Uuid,
    title: String,
    kind: TabKind,
    sql: String,
    last_executed: Option<String>,
}

struct PendingRow {
    original: Vec<Value>,
    changes: Vec<Value>,
    primary_key: Vec<Value>,
    xmin: Option<String>,
    deleted: bool,
}

#[derive(Default)]
struct TableState {
    page: Option<TablePage>,
    page_index: u32,
    filter_column: String,
    filter_value: String,
    order: Option<OrderSpec>,
    selected: Option<usize>,
    pending: BTreeMap<usize, PendingRow>,
}

#[derive(Clone)]
struct ProfileForm {
    id: Option<Uuid>,
    name: String,
    color: String,
    engine: DatabaseEngine,
    host: String,
    port: u16,
    username: String,
    database: String,
    tls: TlsMode,
    ca_cert_path: Option<String>,
    ssh: Option<dbm_core::models::SshConfig>,
    password: String,
    read_only: bool,
}

impl ProfileForm {
    fn fresh() -> Self {
        Self {
            id: None,
            name: "Local PostgreSQL".into(),
            color: "#4c9aff".into(),
            engine: DatabaseEngine::Postgres,
            host: "localhost".into(),
            port: 5432,
            username: "postgres".into(),
            database: "postgres".into(),
            tls: TlsMode::Preferred,
            ca_cert_path: None,
            ssh: None,
            password: String::new(),
            read_only: false,
        }
    }
    fn from_profile(p: &ConnectionProfile) -> Self {
        Self {
            id: Some(p.id),
            name: p.name.clone(),
            color: p.color.clone().unwrap_or_else(|| "#4c9aff".into()),
            engine: p.engine,
            host: p.host.clone(),
            port: p.port,
            username: p.username.clone(),
            database: p.default_database.clone(),
            tls: p.tls_mode.clone(),
            ca_cert_path: p.ca_cert_path.clone(),
            ssh: p.ssh.clone(),
            password: String::new(),
            read_only: p.read_only,
        }
    }
    fn input(&self) -> SaveProfileInput {
        SaveProfileInput {
            id: self.id,
            name: self.name.clone(),
            color: Some(self.color.clone()),
            engine: self.engine,
            host: self.host.clone(),
            port: self.port,
            username: self.username.clone(),
            default_database: self.database.clone(),
            tls_mode: self.tls.clone(),
            ca_cert_path: self.ca_cert_path.clone(),
            ssh: self.ssh.clone(),
            read_only: self.read_only,
            password: if self.password.is_empty() && self.id.is_some() {
                None
            } else {
                Some(self.password.clone())
            },
        }
    }
}

pub struct Workbench {
    worker: Worker,
    demo: bool,
    profiles: Vec<ConnectionProfile>,
    workspaces: HashMap<Uuid, WorkspaceInfo>,
    schemas: HashMap<Uuid, Vec<SchemaNode>>,
    tabs: Vec<Tab>,
    active_tab: Option<u64>,
    active_profile: Option<Uuid>,
    query_results: HashMap<u64, QueryResponse>,
    histories: HashMap<(Uuid, String), Vec<QueryHistoryEntry>>,
    tables: HashMap<u64, TableState>,
    next_id: u64,
    generation: u64,
    pending_requests: HashMap<RequestId, (u64, &'static str, Option<u64>)>,
    error: Option<String>,
    notice: Option<String>,
    profile_form: Option<ProfileForm>,
    confirm_delete: Option<Uuid>,
    confirm_save: Option<u64>,
}

impl Workbench {
    pub fn new(cc: &eframe::CreationContext<'_>, demo: bool) -> Self {
        Self::with_context(cc.egui_ctx.clone(), demo)
    }

    fn with_context(context: egui::Context, demo: bool) -> Self {
        configure_style(&context);
        let worker = Worker::start(context, demo);
        let mut app = Self {
            worker,
            demo,
            profiles: vec![],
            workspaces: HashMap::new(),
            schemas: HashMap::new(),
            tabs: vec![],
            active_tab: None,
            active_profile: None,
            query_results: HashMap::new(),
            histories: HashMap::new(),
            tables: HashMap::new(),
            next_id: 1,
            generation: 1,
            pending_requests: HashMap::new(),
            error: None,
            notice: None,
            profile_form: None,
            confirm_delete: None,
            confirm_save: None,
        };
        app.send(Command::LoadProfiles, "Loading profiles");
        app
    }

    fn send(&mut self, command: Command, label: &'static str) {
        self.send_to(command, label, self.active_tab);
    }

    fn send_to(&mut self, command: Command, label: &'static str, tab: Option<u64>) {
        // A write cannot be double-submitted. UI navigation is still allowed;
        // completion routing uses the initiating tab, never the selected tab.
        if !self.pending_requests.is_empty() {
            return;
        }
        let id = RequestId(self.next_id);
        self.next_id += 1;
        self.pending_requests
            .insert(id, (self.generation, label, tab));
        if self
            .worker
            .tx
            .send(Work {
                id,
                generation: self.generation,
                command,
            })
            .is_err()
        {
            self.pending_requests.remove(&id);
            self.error = Some("Database worker stopped.".into());
        }
    }

    fn drain(&mut self) {
        while let Ok(done) = self.worker.rx.try_recv() {
            let Some((expected, _, tab)) = self.pending_requests.remove(&done.id) else {
                continue;
            };
            if !accept_completion(expected, self.generation, done.generation) {
                continue;
            }
            match done.result {
                Ok(payload) => self.apply(payload, tab),
                Err(error) => self.error = Some(error),
            }
        }
    }

    fn apply(&mut self, payload: Payload, target: Option<u64>) {
        match payload {
            Payload::Profiles(p) => self.profiles = p,
            Payload::Profile(p) => {
                self.close_profile(p.id);
                if let Some(old) = self.profiles.iter_mut().find(|x| x.id == p.id) {
                    *old = p;
                } else {
                    self.profiles.push(p);
                }
                self.profiles.sort_by_key(|p| p.name.to_lowercase());
                self.profile_form = None;
                self.notice =
                    Some("Profile saved. Password is stored in the OS credential store.".into());
            }
            Payload::Tested => self.notice = Some("Connection test succeeded.".into()),
            Payload::Deleted(id) => {
                self.profiles.retain(|p| p.id != id);
                self.close_profile(id);
                self.confirm_delete = None;
            }
            Payload::Workspace(w) => {
                let id = w.profile.id;
                if self
                    .workspaces
                    .get(&id)
                    .is_some_and(|old| old.profile.default_database != w.profile.default_database)
                {
                    let removed: Vec<_> = self
                        .tabs
                        .iter()
                        .filter(|tab| tab.profile_id == id)
                        .map(|tab| tab.id)
                        .collect();
                    for tab in removed {
                        self.query_results.remove(&tab);
                        self.tables.remove(&tab);
                    }
                    self.tabs
                        .retain(|tab| tab.profile_id != id || matches!(tab.kind, TabKind::Query));
                }
                self.workspaces.insert(id, w);
                self.active_profile = Some(id);
                self.ensure_query_tab(id);
                self.send(Command::Schema(id), "Loading schema");
            }
            Payload::Disconnected(id) => self.close_profile(id),
            Payload::Schema(id, tree) => {
                self.schemas.insert(id, tree);
            }
            Payload::Query(result) => {
                if let Some(tab) = target.filter(|id| self.tabs.iter().any(|tab| tab.id == *id)) {
                    self.query_results.insert(tab, result);
                }
                if let Some(id) = self
                    .tabs
                    .iter()
                    .find(|tab| Some(tab.id) == target)
                    .map(|tab| tab.profile_id)
                {
                    if let Some(w) = self.workspaces.get(&id) {
                        self.send(
                            Command::History(id, w.profile.default_database.clone()),
                            "Loading history",
                        );
                    }
                }
            }
            Payload::History(id, database, entries) => {
                self.histories.insert((id, database), entries);
            }
            Payload::Table(page) => {
                if let Some(tab) = target.filter(|id| self.tabs.iter().any(|tab| tab.id == *id)) {
                    let state = self.tables.entry(tab).or_default();
                    state.selected = None;
                    state.page = Some(page);
                }
            }
            Payload::Mutation(result) => {
                self.notice = Some(if result.conflicts.is_empty() {
                    format!("{} change(s) saved.", result.applied)
                } else {
                    format!("Saved with {} row conflict(s).", result.conflicts.len())
                });
                if let Some(tab) = target {
                    if let Some(state) = self.tables.get_mut(&tab) {
                        state
                            .pending
                            .retain(|_, row| result.conflicts.contains(&row.primary_key));
                    }
                    if result.conflicts.is_empty() {
                        self.load_table(tab);
                    }
                }
                self.confirm_save = None;
            }
        }
    }

    fn ensure_query_tab(&mut self, profile: Uuid) {
        if let Some(tab) = self.tabs.iter().rev().find(|t| t.profile_id == profile) {
            self.active_tab = Some(tab.id);
            return;
        }
        self.open_query(profile);
    }
    fn open_query(&mut self, profile: Uuid) {
        let id = self.next_id;
        self.next_id += 1;
        let redis = self
            .profiles
            .iter()
            .find(|p| p.id == profile)
            .is_some_and(|p| p.engine == DatabaseEngine::Redis);
        self.tabs.push(Tab {
            id,
            profile_id: profile,
            title: format!(
                "Query {}",
                self.tabs
                    .iter()
                    .filter(|t| t.profile_id == profile && matches!(t.kind, TabKind::Query))
                    .count()
                    + 1
            ),
            kind: TabKind::Query,
            sql: if redis { "PING" } else { "SELECT now();" }.into(),
            last_executed: None,
        });
        self.active_tab = Some(id);
        self.active_profile = Some(profile);
    }
    fn open_table(&mut self, profile: Uuid, schema: String, table: String) {
        if let Some(existing) = self.tabs.iter().find(|t| t.profile_id == profile && matches!(&t.kind, TabKind::Table { schema: s, table: t } if s == &schema && t == &table)) { self.active_tab = Some(existing.id); return; }
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(Tab {
            id,
            profile_id: profile,
            title: format!("{schema}.{table}"),
            kind: TabKind::Table { schema, table },
            sql: String::new(),
            last_executed: None,
        });
        self.tables.insert(id, TableState::default());
        self.active_tab = Some(id);
        self.active_profile = Some(profile);
        self.load_table(id);
    }
    fn close_profile(&mut self, id: Uuid) {
        let removed: Vec<_> = self
            .tabs
            .iter()
            .filter(|tab| tab.profile_id == id)
            .map(|tab| tab.id)
            .collect();
        for tab in removed {
            self.query_results.remove(&tab);
            self.tables.remove(&tab);
        }
        self.workspaces.remove(&id);
        self.schemas.remove(&id);
        self.tabs.retain(|t| t.profile_id != id);
        if self.active_profile == Some(id) {
            self.active_profile = None;
            self.active_tab = None;
        }
        self.generation += 1;
    }

    fn load_table(&mut self, tab_id: u64) {
        if self
            .tables
            .get(&tab_id)
            .is_some_and(|state| !state.pending.is_empty())
        {
            self.error = Some("Save or discard staged changes before reloading this table.".into());
            return;
        }
        let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else {
            return;
        };
        let TabKind::Table { schema, table } = &tab.kind else {
            return;
        };
        let (offset, filters, order_by) = {
            let state = self.tables.entry(tab_id).or_default();
            let filters = if state.filter_column.is_empty() || state.filter_value.is_empty() {
                vec![]
            } else {
                vec![FilterCondition {
                    column: state.filter_column.clone(),
                    operator: FilterOperator::Contains,
                    value: Some(state.filter_value.clone()),
                }]
            };
            (state.page_index * PAGE_SIZE, filters, state.order.clone())
        };
        self.send_to(
            Command::Table(TablePageRequest {
                profile_id: tab.profile_id,
                schema: schema.clone(),
                table: table.clone(),
                offset,
                limit: PAGE_SIZE,
                filters,
                order_by,
                include_total: Some(true),
            }),
            "Loading table",
            Some(tab_id),
        );
    }
}

impl eframe::App for Workbench {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain();
        if ctx.input(|input| input.viewport().close_requested())
            && (self.tables.values().any(|table| !table.pending.is_empty())
                || !self.pending_requests.is_empty())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.notice = Some(
                "Save or discard staged changes and wait for the active operation before closing."
                    .into(),
            );
        }
        egui::TopBottomPanel::top("top")
            .exact_height(if self.demo { 58.0 } else { 34.0 })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("DBM");
                    if self.demo {
                        ui.colored_label(
                            Color32::from_rgb(240, 177, 76),
                            "DEMO FIXTURE · NOT A LIVE CONNECTION",
                        );
                    }
                    if !self.pending_requests.is_empty() {
                        ui.spinner();
                        ui.label(
                            self.pending_requests
                                .values()
                                .next()
                                .map_or("Working…", |(_, l, _)| *l),
                        );
                    }
                });
                if self.demo {
                    ui.label(
                        RichText::new(
                            "Isolated deterministic data; profiles and edits are not persisted.",
                        )
                        .small()
                        .color(Color32::from_rgb(160, 160, 166)),
                    );
                }
            });
        self.sidebar(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            self.tabs_ui(ui);
            ui.separator();
            self.content_ui(ui);
        });
        self.dialogs(ctx);
        if let Some(error) = self.error.clone() {
            egui::TopBottomPanel::bottom("error").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(Color32::from_rgb(255, 138, 128), error);
                    if ui.button("Dismiss").clicked() {
                        self.error = None;
                    }
                });
            });
        } else if let Some(notice) = self.notice.clone() {
            egui::TopBottomPanel::bottom("notice").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(Color32::from_rgb(90, 211, 148), notice);
                    if ui.button("Dismiss").clicked() {
                        self.notice = None;
                    }
                });
            });
        }
    }
}

impl Workbench {
    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .default_width(260.0)
            .min_width(220.0)
            .show(ctx, |ui| {
                if !self.pending_requests.is_empty() {
                    ui.disable();
                }
                ui.horizontal(|ui| {
                    ui.strong("Connections");
                    if ui.button("+ New").clicked() {
                        self.profile_form = Some(ProfileForm::fresh());
                    }
                });
                ui.separator();
                for profile in self.profiles.clone() {
                    let connected = self.workspaces.contains_key(&profile.id);
                    let color = parse_color(profile.color.as_deref().unwrap_or("#4c9aff"));
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 3.5, color);
                        if ui
                            .selectable_label(
                                self.active_profile == Some(profile.id),
                                &profile.name,
                            )
                            .clicked()
                        {
                            if connected {
                                self.active_profile = Some(profile.id);
                                self.ensure_query_tab(profile.id);
                            } else {
                                self.send(Command::Connect(profile.id), "Connecting");
                            }
                        }
                        ui.menu_button("...", |ui| {
                            if ui.button("Edit").clicked() {
                                if self.profile_is_clean(profile.id) {
                                    self.profile_form = Some(ProfileForm::from_profile(&profile));
                                }
                                ui.close();
                            }
                            if connected && ui.button("Disconnect").clicked() {
                                if self.profile_is_clean(profile.id) {
                                    self.send(Command::Disconnect(profile.id), "Disconnecting");
                                }
                                ui.close();
                            }
                            if ui.button("Delete…").clicked() {
                                if self.profile_is_clean(profile.id) {
                                    self.confirm_delete = Some(profile.id);
                                }
                                ui.close();
                            }
                        });
                    });
                    if let Some(workspace) = self.workspaces.get(&profile.id).cloned() {
                        let mut database = workspace.profile.default_database.clone();
                        egui::ComboBox::from_id_salt(("db", profile.id))
                            .selected_text(&database)
                            .width(190.0)
                            .show_ui(ui, |ui| {
                                for db in workspace.databases.iter().filter(|d| d.is_connectable) {
                                    ui.selectable_value(&mut database, db.name.clone(), &db.name);
                                }
                            });
                        if database != workspace.profile.default_database
                            && self.profile_is_clean(profile.id)
                        {
                            self.send(
                                Command::SwitchDatabase(profile.id, database),
                                "Switching database",
                            );
                        }
                        ui.horizontal(|ui| {
                            if ui.small_button("+ Query").clicked() {
                                self.open_query(profile.id);
                            }
                            if ui.small_button("↻ Schema").clicked() {
                                self.send(Command::Schema(profile.id), "Refreshing schema");
                            }
                        });
                        if let Some(nodes) = self.schemas.get(&profile.id).cloned() {
                            for node in nodes {
                                egui::CollapsingHeader::new(node.name)
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        for child in node.children {
                                            if ui.selectable_label(false, &child.name).clicked() {
                                                if let (Some(s), Some(t)) =
                                                    (child.schema, child.table)
                                                {
                                                    self.open_table(profile.id, s, t);
                                                }
                                            }
                                        }
                                    });
                            }
                        }
                    }
                    ui.add_space(8.0);
                }
            });
    }

    fn tabs_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let tabs = self.tabs.clone();
            for tab in tabs {
                let color = self
                    .profiles
                    .iter()
                    .find(|p| p.id == tab.profile_id)
                    .and_then(|p| p.color.as_deref())
                    .map_or(Color32::from_rgb(76, 154, 255), parse_color);
                let selected = self.active_tab == Some(tab.id);
                if ui
                    .selectable_label(
                        selected,
                        RichText::new(&tab.title).color(if selected {
                            color
                        } else {
                            Color32::from_rgb(199, 199, 204)
                        }),
                    )
                    .clicked()
                {
                    self.active_tab = Some(tab.id);
                    self.active_profile = Some(tab.profile_id);
                }
                if selected && ui.small_button("×").clicked() {
                    if self
                        .tables
                        .get(&tab.id)
                        .is_some_and(|state| !state.pending.is_empty())
                    {
                        self.error =
                            Some("Save or discard staged changes before closing this tab.".into());
                    } else if self.pending_requests.is_empty() {
                        self.tabs.retain(|t| t.id != tab.id);
                        self.tables.remove(&tab.id);
                        self.query_results.remove(&tab.id);
                        self.active_tab = self.tabs.last().map(|t| t.id);
                        self.active_profile = self.tabs.last().map(|t| t.profile_id);
                    }
                }
            }
        });
    }

    fn content_ui(&mut self, ui: &mut egui::Ui) {
        if !self.pending_requests.is_empty() {
            ui.disable();
        }
        let Some(id) = self.active_tab else {
            ui.centered_and_justified(|ui| {
                ui.label("Select a saved connection to connect and open a query.");
            });
            return;
        };
        let Some(tab) = self.tabs.iter().find(|t| t.id == id).cloned() else {
            return;
        };
        match tab.kind {
            TabKind::Query => self.query_ui(ui, id, tab.profile_id),
            TabKind::Table { .. } => self.table_ui(ui, id, tab.profile_id),
        }
    }

    fn query_ui(&mut self, ui: &mut egui::Ui, tab_id: u64, profile_id: Uuid) {
        ui.horizontal(|ui| {
            if ui.button(RichText::new("▶ Run").strong()).clicked() {
                let sql = self
                    .tabs
                    .iter()
                    .find(|t| t.id == tab_id)
                    .map(|t| t.sql.clone())
                    .unwrap_or_default();
                if !sql.trim().is_empty() {
                    self.query_results.remove(&tab_id);
                    self.tabs
                        .iter_mut()
                        .find(|tab| tab.id == tab_id)
                        .unwrap()
                        .last_executed = Some(sql.clone());
                    self.send(
                        Command::Query(QueryRequest {
                            profile_id,
                            sql,
                            max_rows: Some(MAX_QUERY_ROWS),
                        }),
                        "Running query",
                    );
                }
            }
            if ui
                .add_enabled(
                    self.tabs
                        .iter()
                        .find(|tab| tab.id == tab_id)
                        .is_some_and(|tab| tab.last_executed.is_some()),
                    egui::Button::new("↻ Refresh result"),
                )
                .clicked()
            {
                let sql = self
                    .tabs
                    .iter()
                    .find(|t| t.id == tab_id)
                    .and_then(|t| t.last_executed.clone())
                    .unwrap_or_default();
                self.send(
                    Command::Query(QueryRequest {
                        profile_id,
                        sql,
                        max_rows: Some(MAX_QUERY_ROWS),
                    }),
                    "Refreshing query",
                );
            }
            ui.label("Ctrl+Enter · results capped at 10,000 rows");
        });
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            let response = ui.add_sized(
                [ui.available_width(), 180.0],
                egui::TextEdit::multiline(&mut tab.sql)
                    .font(egui::TextStyle::Monospace)
                    .code_editor(),
            );
            if response.has_focus()
                && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter))
            {
                let sql = tab.sql.clone();
                tab.last_executed = Some(sql.clone());
                self.query_results.remove(&tab_id);
                self.send(
                    Command::Query(QueryRequest {
                        profile_id,
                        sql,
                        max_rows: Some(MAX_QUERY_ROWS),
                    }),
                    "Running query",
                );
            }
        }
        ui.separator();
        if let Some(result) = self.query_results.get(&tab_id) {
            ui.label(format!(
                "{} row(s) · {} ms{}",
                result.row_count,
                result.duration_ms,
                if result.truncated {
                    " · truncated"
                } else {
                    ""
                }
            ));
            result_grid(ui, &result.columns, &result.rows);
        }
        ui.collapsing("Query history", |ui| {
            let database = self
                .workspaces
                .get(&profile_id)
                .map(|workspace| workspace.profile.default_database.clone())
                .unwrap_or_default();
            if ui.button("Reload history").clicked() {
                self.send(
                    Command::History(profile_id, database.clone()),
                    "Loading history",
                );
            }
            for entry in self
                .histories
                .get(&(profile_id, database))
                .into_iter()
                .flatten()
            {
                if ui
                    .selectable_label(
                        false,
                        format!(
                            "{}  {} ms  {}",
                            if entry.success { "✓" } else { "×" },
                            entry.duration_ms,
                            one_line(&entry.sql)
                        ),
                    )
                    .clicked()
                {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.sql.clone_from(&entry.sql);
                    }
                }
            }
        });
    }

    fn table_ui(&mut self, ui: &mut egui::Ui, tab_id: u64, profile_id: Uuid) {
        let read_only = self
            .profiles
            .iter()
            .find(|p| p.id == profile_id)
            .is_none_or(|p| p.read_only);
        let mut reload = false;
        let mut save = false;
        {
            let state = self.tables.entry(tab_id).or_default();
            ui.horizontal_wrapped(|ui| {
                ui.label("Filter");
                ui.add(
                    egui::TextEdit::singleline(&mut state.filter_column)
                        .desired_width(100.0)
                        .hint_text("Column"),
                );
                ui.label("contains");
                ui.add(
                    egui::TextEdit::singleline(&mut state.filter_value)
                        .desired_width(150.0)
                        .hint_text("Value"),
                );
                if ui.button("Apply").clicked() && state.pending.is_empty() {
                    state.page_index = 0;
                    reload = true;
                }
                if ui.button("↻ Refresh").clicked() {
                    if state.pending.is_empty() {
                        reload = true;
                    } else {
                        self.error =
                            Some("Save or discard pending row changes before refreshing.".into());
                    }
                }
                if !state.pending.is_empty() {
                    ui.colored_label(
                        Color32::from_rgb(240, 177, 76),
                        format!("{} pending", state.pending.len()),
                    );
                    if ui.button("Discard").clicked() {
                        state.pending.clear();
                    }
                    if ui
                        .add_enabled(!read_only, egui::Button::new("Save changes"))
                        .clicked()
                    {
                        save = true;
                    }
                }
                if read_only {
                    ui.label("Read-only");
                }
            });
        }
        if reload {
            self.load_table(tab_id);
            reload = false;
        }
        if save {
            self.confirm_save = Some(tab_id);
        }
        let Some(page) = self.tables.get(&tab_id).and_then(|s| s.page.clone()) else {
            ui.spinner();
            return;
        };
        let selected = self.tables.get(&tab_id).and_then(|s| s.selected);
        let pending = self.tables.get(&tab_id).map_or(0, |s| s.pending.len());
        let height = if selected.is_some() {
            ui.available_height() - 260.0
        } else {
            ui.available_height() - 85.0
        };
        let mut clicked_row = None;
        TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .min_scrolled_height(100.0)
            .max_scroll_height(height.max(100.0))
            .columns(
                Column::initial(160.0).resizable(true),
                page.metadata.columns.len(),
            )
            .header(34.0, |mut header| {
                for column in &page.metadata.columns {
                    header.col(|ui| {
                        if ui
                            .button(format!(
                                "{}  {}",
                                if page.metadata.primary_key.contains(&column.name) {
                                    "PK"
                                } else {
                                    ""
                                },
                                column.name
                            ))
                            .on_hover_text(&column.data_type)
                            .clicked()
                        {
                            let state = self.tables.get_mut(&tab_id).unwrap();
                            if !state.pending.is_empty() {
                                return;
                            }
                            state.order = Some(OrderSpec {
                                column: column.name.clone(),
                                descending: state
                                    .order
                                    .as_ref()
                                    .is_some_and(|o| o.column == column.name && !o.descending),
                            });
                            reload = true;
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(32.0, page.rows.len(), |mut row| {
                    let index = row.index();
                    let values = self
                        .tables
                        .get(&tab_id)
                        .and_then(|s| s.pending.get(&index))
                        .map_or(&page.rows[index], |p| &p.changes);
                    for value in values.iter().take(page.metadata.columns.len()) {
                        row.col(|ui| {
                            if let Some(pending) = self
                                .tables
                                .get(&tab_id)
                                .and_then(|state| state.pending.get(&index))
                            {
                                ui.visuals_mut().override_text_color = Some(if pending.deleted {
                                    Color32::from_rgb(255, 138, 128)
                                } else {
                                    Color32::from_rgb(240, 177, 76)
                                });
                            }
                            if ui
                                .selectable_label(selected == Some(index), value_text(value))
                                .clicked()
                            {
                                clicked_row = Some(index);
                            }
                        });
                    }
                });
            });
        if reload {
            self.load_table(tab_id);
            reload = false;
        }
        if let Some(index) = clicked_row {
            self.tables.get_mut(&tab_id).unwrap().selected = Some(index);
        }
        if let Some(index) = self.tables.get(&tab_id).and_then(|s| s.selected) {
            if index < page.rows.len() {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.strong("Row inspector");
                    if ui
                        .add_enabled(
                            !read_only && !page.metadata.primary_key.is_empty(),
                            egui::Button::new("Stage delete"),
                        )
                        .clicked()
                    {
                        let row = page.rows[index].clone();
                        let p = pending_row(&page, row);
                        self.tables
                            .get_mut(&tab_id)
                            .unwrap()
                            .pending
                            .entry(index)
                            .or_insert(p)
                            .deleted = true;
                    }
                });
                egui::ScrollArea::vertical()
                    .id_salt(("inspector", tab_id))
                    .max_height(140.0)
                    .show(ui, |ui| {
                        for (column_index, column) in page.metadata.columns.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("{}  {}", column.name, column.data_type));
                                let original = &page.rows[index][column_index];
                                let mut value = self
                                    .tables
                                    .get(&tab_id)
                                    .and_then(|s| s.pending.get(&index))
                                    .map_or_else(
                                        || value_edit(original),
                                        |p| value_edit(&p.changes[column_index]),
                                    );
                                let editable = !read_only
                                    && !page.metadata.primary_key.is_empty()
                                    && !page.metadata.primary_key.contains(&column.name);
                                if ui
                                    .add_enabled(
                                        editable,
                                        egui::TextEdit::singleline(&mut value).desired_width(400.0),
                                    )
                                    .changed()
                                    && editable
                                {
                                    let parsed = parse_edit(&value, original);
                                    let state = self.tables.get_mut(&tab_id).unwrap();
                                    let pending = state.pending.entry(index).or_insert_with(|| {
                                        pending_row(&page, page.rows[index].clone())
                                    });
                                    pending.changes[column_index] = parsed;
                                    if pending.changes == pending.original && !pending.deleted {
                                        state.pending.remove(&index);
                                    }
                                }
                            });
                        }
                    });
            }
        }
        ui.horizontal(|ui| {
            let state = self.tables.get_mut(&tab_id).unwrap();
            ui.label(format!(
                "Rows {}–{} of {}",
                page.offset + 1,
                page.offset + page.rows.len() as u32,
                page.total_rows
                    .map_or_else(|| "?".into(), |n| n.to_string())
            ));
            if ui
                .add_enabled(
                    state.page_index > 0 && pending == 0,
                    egui::Button::new("‹ Previous"),
                )
                .clicked()
            {
                state.page_index -= 1;
                reload = true;
            }
            if ui
                .add_enabled(page.has_more && pending == 0, egui::Button::new("Next ›"))
                .clicked()
            {
                state.page_index += 1;
                reload = true;
            }
        });
        if reload {
            self.load_table(tab_id);
        }
    }

    fn profile_is_clean(&mut self, profile: Uuid) -> bool {
        let dirty = self
            .tabs
            .iter()
            .filter(|tab| tab.profile_id == profile)
            .any(|tab| {
                self.tables
                    .get(&tab.id)
                    .is_some_and(|state| !state.pending.is_empty())
            });
        if dirty {
            self.error = Some("Save or discard this connection's staged edits first.".into());
        }
        !dirty
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(mut form) = self.profile_form.take() {
            let mut open = true;
            egui::Window::new(if form.id.is_some() {
                "Edit connection"
            } else {
                "New connection"
            })
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                if !self.pending_requests.is_empty() {
                    ui.disable();
                }
                egui::ComboBox::from_label("Engine")
                    .selected_text(format!("{:?}", form.engine))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut form.engine,
                            DatabaseEngine::Postgres,
                            "PostgreSQL",
                        );
                        ui.selectable_value(&mut form.engine, DatabaseEngine::Mysql, "MySQL");
                        ui.selectable_value(&mut form.engine, DatabaseEngine::Redis, "Redis");
                    });
                field(ui, "Name", &mut form.name);
                field(ui, "Color", &mut form.color);
                field(ui, "Host", &mut form.host);
                ui.horizontal(|ui| {
                    ui.label("Port");
                    ui.add(egui::DragValue::new(&mut form.port).range(1..=65535));
                });
                field(ui, "Username", &mut form.username);
                field(ui, "Database", &mut form.database);
                egui::ComboBox::from_label("TLS")
                    .selected_text(format!("{:?}", form.tls))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut form.tls, TlsMode::Disabled, "Disabled");
                        ui.selectable_value(&mut form.tls, TlsMode::Preferred, "Preferred");
                        ui.selectable_value(&mut form.tls, TlsMode::Required, "Required");
                    });
                let mut ca = form.ca_cert_path.clone().unwrap_or_default();
                field(ui, "CA certificate path", &mut ca);
                form.ca_cert_path = (!ca.is_empty()).then_some(ca);
                ui.horizontal(|ui| {
                    ui.label("Password");
                    ui.add(egui::TextEdit::singleline(&mut form.password).password(true));
                });
                ui.checkbox(&mut form.read_only, "Read-only safety");
                ui.label("Leave password blank when editing to keep the saved credential.");
                ui.horizontal(|ui| {
                    if ui.button("Test connection").clicked() {
                        self.send(Command::TestProfile(form.input()), "Testing connection");
                    }
                    if ui.button("Save").clicked()
                        && form.id.is_none_or(|id| self.profile_is_clean(id))
                    {
                        self.send(Command::SaveProfile(form.input()), "Saving profile");
                    }
                });
            });
            if open {
                self.profile_form = Some(form);
            }
        }
        if let Some(id) = self.confirm_delete {
            egui::Window::new("Delete connection?").collapsible(false).show(ctx, |ui| {
                if !self.pending_requests.is_empty() { ui.disable(); }
                ui.label("Remove this profile, local history and saved credential? The database is not deleted.");
                if ui.button("Cancel").clicked() { self.confirm_delete = None; }
                if ui.button("Delete").clicked() && self.profile_is_clean(id) { self.send(Command::DeleteProfile(id), "Deleting profile"); }
            });
        }
        if let Some(tab_id) = self.confirm_save {
            egui::Window::new("Apply staged changes?")
                .collapsible(false)
                .show(ctx, |ui| {
                    if !self.pending_requests.is_empty() {
                        ui.disable();
                    }
                    let count = self.tables.get(&tab_id).map_or(0, |s| s.pending.len());
                    ui.label(format!("Apply {count} staged row change(s)?"));
                    if ui.button("Cancel").clicked() {
                        self.confirm_save = None;
                    }
                    if ui.button("Apply changes").clicked() {
                        if let Some(batch) = self.mutation_batch(tab_id) {
                            self.send_to(Command::Mutate(batch), "Saving changes", Some(tab_id));
                        }
                    }
                });
        }
    }

    fn mutation_batch(&self, tab_id: u64) -> Option<MutationBatch> {
        let tab = self.tabs.iter().find(|t| t.id == tab_id)?;
        let TabKind::Table { schema, table } = &tab.kind else {
            return None;
        };
        let state = self.tables.get(&tab_id)?;
        Some(MutationBatch {
            profile_id: tab.profile_id,
            schema: schema.clone(),
            table: table.clone(),
            mutations: state
                .pending
                .values()
                .map(|p| RowMutation {
                    original: p.original.clone(),
                    changes: p.changes.clone(),
                    primary_key: p.primary_key.clone(),
                    xmin: p.xmin.clone(),
                    deleted: p.deleted,
                })
                .collect(),
        })
    }
}

fn pending_row(page: &TablePage, row: Vec<Value>) -> PendingRow {
    let count = page.metadata.columns.len();
    let original = row[..count].to_vec();
    let primary_key = page
        .metadata
        .primary_key
        .iter()
        .filter_map(|key| {
            page.metadata
                .columns
                .iter()
                .position(|c| &c.name == key)
                .map(|i| original[i].clone())
        })
        .collect();
    PendingRow {
        changes: original.clone(),
        original,
        primary_key,
        xmin: row.get(count).and_then(Value::as_str).map(str::to_owned),
        deleted: false,
    }
}
fn value_text(v: &Value) -> String {
    match v {
        Value::Null => "NULL".into(),
        Value::String(s) => s.clone(),
        _ => v.to_string(),
    }
}
fn value_edit(v: &Value) -> String {
    value_text(v)
}
fn parse_edit(s: &str, original: &Value) -> Value {
    if original.is_null() && s == "NULL" {
        Value::Null
    } else {
        match original {
            Value::Bool(_) => s
                .parse()
                .map_or_else(|_| Value::String(s.into()), Value::Bool),
            Value::Number(_) => serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.into())),
            _ => Value::String(s.into()),
        }
    }
}
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(value);
    });
}
fn parse_color(hex: &str) -> Color32 {
    let h = hex.trim_start_matches('#');
    if h.len() == 6 {
        if let Ok(v) = u32::from_str_radix(h, 16) {
            return Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
        }
    }
    Color32::from_rgb(76, 154, 255)
}
fn result_grid(ui: &mut egui::Ui, columns: &[QueryColumn], rows: &[Vec<Value>]) {
    let height = (ui.available_height() - 60.0).max(100.0);
    TableBuilder::new(ui)
        .striped(true)
        .max_scroll_height(height)
        .columns(Column::initial(160.0).resizable(true), columns.len())
        .header(34.0, |mut h| {
            for c in columns {
                h.col(|ui| {
                    ui.strong(&c.name);
                });
            }
        })
        .body(|body| {
            body.rows(32.0, rows.len(), |mut row| {
                let i = row.index();
                for value in &rows[i] {
                    row.col(|ui| {
                        ui.monospace(value_text(value));
                    });
                }
            })
        });
}
fn accept_completion(
    expected_generation: u64,
    current_generation: u64,
    completion_generation: u64,
) -> bool {
    expected_generation == completion_generation && completion_generation == current_generation
}

fn configure_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Geist".into(),
        egui::FontData::from_static(include_bytes!("../assets/geist.ttf")).into(),
    );
    fonts.font_data.insert(
        "Geist Mono".into(),
        egui::FontData::from_static(include_bytes!("../assets/geist-mono.ttf")).into(),
    );
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "Geist".into());
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .insert(0, "Geist Mono".into());
    ctx.set_fonts(fonts);
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(Color32::from_rgb(232, 232, 234));
    visuals.panel_fill = Color32::from_rgb(28, 28, 30);
    visuals.window_fill = Color32::from_rgb(38, 38, 41);
    visuals.extreme_bg_color = Color32::from_rgb(22, 22, 24);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(42, 42, 45);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(50, 50, 53);
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(76, 154, 255, 70);
    visuals.selection.stroke = Stroke::new(1.0, Color32::from_rgb(76, 154, 255));
    ctx.set_visuals(visuals);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 4.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(13.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, egui::FontId::monospace(12.0));
    ctx.set_style(style);
}

struct DemoBackend {
    profiles: Vec<ConnectionProfile>,
}
impl DemoBackend {
    fn new() -> Self {
        let now = chrono::Utc::now();
        Self {
            profiles: vec![ConnectionProfile {
                id: Uuid::from_u128(1),
                name: "Acme Analytics".into(),
                color: Some("#3dd6c6".into()),
                engine: DatabaseEngine::Postgres,
                host: "demo.invalid".into(),
                port: 5432,
                username: "demo".into(),
                default_database: "analytics".into(),
                tls_mode: TlsMode::Required,
                ca_cert_path: None,
                ssh: None,
                read_only: false,
                created_at: now,
                updated_at: now,
            }],
        }
    }
    fn execute(&mut self, command: Command) -> Result<Payload, String> {
        Ok(match command {
            Command::LoadProfiles => Payload::Profiles(self.profiles.clone()),
            Command::SaveProfile(input) => {
                let profile = transient_profile(&input).map_err(string_error)?;
                if let Some(old) = self.profiles.iter_mut().find(|x| x.id == profile.id) {
                    *old = profile.clone()
                } else {
                    self.profiles.push(profile.clone())
                };
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
                Payload::Workspace(WorkspaceInfo {
                    profile,
                    databases: vec![
                        DatabaseRef {
                            name: "analytics".into(),
                            is_template: false,
                            is_connectable: true,
                        },
                        DatabaseRef {
                            name: "warehouse".into(),
                            is_template: false,
                            is_connectable: true,
                        },
                    ],
                })
            }
            Command::Disconnect(id) => Payload::Disconnected(id),
            Command::Schema(id) => Payload::Schema(
                id,
                vec![SchemaNode {
                    name: "public".into(),
                    kind: "schema".into(),
                    schema: Some("public".into()),
                    table: None,
                    children: vec![
                        SchemaNode {
                            name: "customers".into(),
                            kind: "table".into(),
                            schema: Some("public".into()),
                            table: Some("customers".into()),
                            children: vec![],
                        },
                        SchemaNode {
                            name: "orders".into(),
                            kind: "table".into(),
                            schema: Some("public".into()),
                            table: Some("orders".into()),
                            children: vec![],
                        },
                    ],
                }],
            ),
            Command::Query(_) => Payload::Query(QueryResponse {
                columns: vec![
                    QueryColumn {
                        name: "customer".into(),
                        data_type: "text".into(),
                    },
                    QueryColumn {
                        name: "revenue".into(),
                        data_type: "numeric".into(),
                    },
                ],
                rows: vec![
                    vec![json!("Ada Lovelace"), json!(12450.25)],
                    vec![json!("Grace Hopper"), json!(9810.0)],
                ],
                row_count: 2,
                affected_rows: None,
                duration_ms: 7,
                truncated: false,
                notices: vec![],
            }),
            Command::History(id, database) => Payload::History(id, database, vec![]),
            Command::Table(req) => Payload::Table(demo_page(req)),
            Command::Mutate(_) => {
                return Err(
                    "Demo fixture: saving is disabled. Staged edits have been retained.".into(),
                );
            }
        })
    }
}
fn demo_page(req: TablePageRequest) -> TablePage {
    let columns = vec![
        TableColumn {
            name: "id".into(),
            data_type: "bigint".into(),
            nullable: false,
            default_value: None,
            ordinal: 1,
        },
        TableColumn {
            name: "email".into(),
            data_type: "text".into(),
            nullable: false,
            default_value: None,
            ordinal: 2,
        },
        TableColumn {
            name: "active".into(),
            data_type: "boolean".into(),
            nullable: false,
            default_value: None,
            ordinal: 3,
        },
    ];
    let all: Vec<Vec<Value>> = (1..=500)
        .map(|i| {
            vec![
                json!(i),
                json!(format!("person{i}@example.com")),
                json!(i % 3 != 0),
                json!(format!("xmin-{i}")),
            ]
        })
        .collect();
    let rows = all
        .into_iter()
        .skip(req.offset as usize)
        .take(req.limit as usize)
        .collect();
    TablePage {
        metadata: TableMetadata {
            schema: req.schema,
            table: req.table,
            columns: columns.clone(),
            primary_key: vec!["id".into()],
            has_xmin: true,
        },
        columns: vec![
            "id".into(),
            "email".into(),
            "active".into(),
            "__dbm_xmin".into(),
        ],
        rows,
        total_rows: Some(500),
        offset: req.offset,
        limit: req.limit,
        has_more: req.offset + req.limit < 500,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renderer_has_native_backend() {
        assert!(!wgpu::Instance::enabled_backend_features().is_empty());
    }

    #[test]
    fn replies_follow_initiating_tab_and_dirty_profiles_are_guarded() {
        let mut app = Workbench::with_context(egui::Context::default(), true);
        app.pending_requests.clear();
        let id = Uuid::from_u128(1);
        app.open_query(id);
        let first = app.active_tab.unwrap();
        app.open_query(id);
        let second = app.active_tab.unwrap();
        let result = QueryResponse {
            columns: vec![],
            rows: vec![vec![json!(37)]],
            row_count: 1,
            affected_rows: None,
            duration_ms: 0,
            truncated: false,
            notices: vec![],
        };
        app.apply(Payload::Query(result), Some(first));
        assert_eq!(app.query_results[&first].rows, vec![vec![json!(37)]]);
        assert!(!app.query_results.contains_key(&second));
        let page = demo_page(TablePageRequest {
            profile_id: id,
            schema: "public".into(),
            table: "customers".into(),
            offset: 0,
            limit: 1,
            filters: vec![],
            order_by: None,
            include_total: None,
        });
        let mut table = TableState::default();
        table
            .pending
            .insert(0, pending_row(&page, page.rows[0].clone()));
        app.tables.insert(first, table);
        assert!(!app.profile_is_clean(id));
        assert!(app.profile_is_clean(Uuid::from_u128(2)));
        assert_eq!(app.tables[&first].pending.len(), 1);
        app.load_table(first);
        assert!(app.pending_requests.is_empty());
    }

    #[test]
    fn literal_null_text_and_tls_profile_settings_survive_edits() {
        assert_eq!(parse_edit("NULL", &json!("name")), json!("NULL"));
        let mut profile = DemoBackend::new().profiles.remove(0);
        profile.ca_cert_path = Some("/tmp/test-ca.pem".into());
        let input = ProfileForm::from_profile(&profile).input();
        assert_eq!(input.tls_mode, TlsMode::Required);
        assert_eq!(input.ca_cert_path, profile.ca_cert_path);
        assert!(input.password.is_none());
    }
    #[test]
    fn stale_completion_is_rejected() {
        assert!(accept_completion(4, 4, 4));
        assert!(!accept_completion(3, 4, 3));
        assert!(!accept_completion(4, 4, 3));
    }
    #[test]
    fn pending_rows_preserve_pk_and_xmin() {
        let page = demo_page(TablePageRequest {
            profile_id: Uuid::nil(),
            schema: "public".into(),
            table: "t".into(),
            offset: 0,
            limit: 1,
            filters: vec![],
            order_by: None,
            include_total: Some(true),
        });
        let p = pending_row(&page, page.rows[0].clone());
        assert_eq!(p.primary_key, vec![json!(1)]);
        assert_eq!(p.xmin.as_deref(), Some("xmin-1"));
    }
    #[test]
    fn page_offsets_are_stable() {
        let p = demo_page(TablePageRequest {
            profile_id: Uuid::nil(),
            schema: "s".into(),
            table: "t".into(),
            offset: 200,
            limit: 200,
            filters: vec![],
            order_by: None,
            include_total: Some(true),
        });
        assert_eq!(p.rows[0][0], json!(201));
        assert!(p.has_more);
    }
}
