//! The Windows/Linux workbench: Graphite layout, connection sidebar, query and
//! table tabs, and dialogs. Database work runs on the serialized worker in
//! `backend.rs`; this module only owns UI state.

use std::collections::{HashMap, HashSet, VecDeque};

use dbm_core::connection_url::{engine_defaults, parse_connection_url};
use dbm_core::models::{
    ConnectionProfile, DatabaseEngine, MutationBatch, QueryHistoryEntry, QueryRequest,
    QueryResponse, RowMutation, SaveProfileInput, SchemaNode, TablePageRequest, TlsMode,
    WorkspaceInfo,
};
use dbm_core::sql_text::{
    ExecutionKind, TokenKind, completions, describe_schema_refresh, execution_target,
    filter_schema_nodes, highlight, requires_confirmation, resolve_full_table_select,
    safe_file_name,
};
use eframe::egui::{
    self, Color32, CornerRadius, Frame, Margin, RichText, Sense, Stroke, TextFormat, Vec2,
    text::LayoutJob,
};
use uuid::Uuid;

use crate::backend::{Command, ExportRequest, Payload, RequestId, Work, Worker};
use crate::icons::{self, Icon};
use crate::messages::{self, Message};
use crate::table_view::{
    self, PENDING_REFRESH_ERROR, TableAction, TableContext, TableState, result_grid,
};
use crate::theme::{self, chip, eyebrow, mono, primary_button, section_label, ui_font};

const MAX_QUERY_ROWS: u32 = 10_000;
const SIDEBAR_COLLAPSED_KEY: &str = "dbm.sidebarCollapsed";
const LARGE_EXPORT_WARNING_ROWS: u64 = 100_000;
fn engine_label(engine: DatabaseEngine) -> &'static str {
    match engine {
        DatabaseEngine::Postgres => "PostgreSQL",
        DatabaseEngine::Mysql => "MySQL",
        DatabaseEngine::Redis => "Redis",
    }
}

fn engine_preset_name(engine: DatabaseEngine) -> &'static str {
    match engine {
        DatabaseEngine::Postgres => "Local PostgreSQL",
        DatabaseEngine::Mysql => "Local MySQL",
        DatabaseEngine::Redis => "Local Redis",
    }
}

fn engine_preset_user(engine: DatabaseEngine) -> &'static str {
    match engine {
        DatabaseEngine::Postgres => "postgres",
        DatabaseEngine::Mysql => "root",
        DatabaseEngine::Redis => "default",
    }
}

fn default_query_text(engine: DatabaseEngine) -> &'static str {
    if engine == DatabaseEngine::Redis {
        "PING"
    } else {
        "SELECT now();"
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum TabKind {
    Query,
    Table { schema: String, table: String },
}

#[derive(Clone, Debug)]
struct Tab {
    id: u64,
    profile_id: Uuid,
    title: String,
    kind: TabKind,
    sql: String,
    /// Editor selection as UTF-8 byte offsets; equal ends are a cursor.
    selection: (usize, usize),
    last_executed: Option<String>,
    /// A `SELECT * FROM table` result shown in the editable table viewer.
    embedded: Option<(String, String)>,
    /// Shrunk to a narrow pill in the tab strip until selected again.
    collapsed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FeedbackKind {
    Info,
    Success,
    Error,
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
    ca_cert_path: String,
    ssh: Option<dbm_core::models::SshConfig>,
    password: String,
    read_only: bool,
    url: String,
    feedback: Option<(FeedbackKind, String)>,
    /// Save & connect tests first, then saves once the test succeeds.
    saving: bool,
}

impl ProfileForm {
    fn fresh() -> Self {
        let engine = DatabaseEngine::Postgres;
        let (port, database) = engine_defaults(engine);
        Self {
            id: None,
            name: engine_preset_name(engine).into(),
            color: theme::DEFAULT_CONNECTION_COLOR.into(),
            engine,
            host: "localhost".into(),
            port,
            username: engine_preset_user(engine).into(),
            database: database.into(),
            tls: TlsMode::Preferred,
            ca_cert_path: String::new(),
            ssh: None,
            password: String::new(),
            read_only: false,
            url: String::new(),
            feedback: None,
            saving: false,
        }
    }

    fn from_profile(p: &ConnectionProfile) -> Self {
        Self {
            id: Some(p.id),
            name: p.name.clone(),
            color: p
                .color
                .clone()
                .unwrap_or_else(|| theme::DEFAULT_CONNECTION_COLOR.into()),
            engine: p.engine,
            host: p.host.clone(),
            port: p.port,
            username: p.username.clone(),
            database: p.default_database.clone(),
            tls: p.tls_mode.clone(),
            ca_cert_path: p.ca_cert_path.clone().unwrap_or_default(),
            ssh: p.ssh.clone(),
            password: String::new(),
            read_only: p.read_only,
            url: String::new(),
            feedback: None,
            saving: false,
        }
    }

    /// Switches engine and replaces fields that still hold the previous
    /// engine's defaults, as the desktop editor does.
    fn set_engine(&mut self, engine: DatabaseEngine) {
        let previous = self.engine;
        let (old_port, old_database) = engine_defaults(previous);
        let (port, database) = engine_defaults(engine);
        if self.name == engine_preset_name(previous) {
            self.name = engine_preset_name(engine).into();
        }
        if self.port == old_port {
            self.port = port;
        }
        if self.username == engine_preset_user(previous) {
            self.username = engine_preset_user(engine).into();
        }
        if self.database == old_database {
            self.database = database.into();
        }
        self.engine = engine;
    }

    fn import_url(&mut self) {
        match parse_connection_url(&self.url) {
            Ok(imported) => {
                let name_is_default =
                    self.name.trim().is_empty() || self.name == engine_preset_name(self.engine);
                if self.id.is_none() && name_is_default {
                    self.name = imported.suggested_name;
                }
                self.engine = imported.engine;
                self.host = imported.host;
                self.port = imported.port;
                self.username = imported.username;
                self.database = imported.default_database;
                self.tls = imported.tls_mode;
                if let Some(password) = imported.password {
                    self.password = password;
                }
                self.feedback = Some((
                    FeedbackKind::Info,
                    "Connection URL imported. Review the details, then save and connect.".into(),
                ));
            }
            Err(error) => self.feedback = Some((FeedbackKind::Error, error)),
        }
    }

    fn input(&self) -> SaveProfileInput {
        let ca = self.ca_cert_path.trim();
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
            ca_cert_path: (!ca.is_empty()).then(|| ca.to_owned()),
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

fn connection_target_changed(previous: &ConnectionProfile, next: &ConnectionProfile) -> bool {
    previous.engine != next.engine
        || previous.host != next.host
        || previous.port != next.port
        || previous.username != next.username
        || previous.default_database != next.default_database
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RequestKind {
    Query,
    Table,
    Mutation,
    Export,
    Form,
    Other,
}

#[derive(Clone, Debug)]
struct RequestMeta {
    label: &'static str,
    tab: Option<u64>,
    profile: Option<Uuid>,
    kind: RequestKind,
    /// Set when the profile was closed; the reply is then ignored.
    stale: bool,
}

struct Queued {
    command: Command,
    meta: RequestMeta,
}

struct ConfirmQuery {
    tab: u64,
    sql: String,
    refresh: bool,
}

/// Keyword completion open in a query editor.
struct Completion {
    tab: u64,
    /// Byte offset where the completed word starts.
    start: usize,
    items: Vec<String>,
    selected: usize,
}

/// Bottom-right toast, as the desktop app uses for schema refresh summaries.
struct Toast {
    message: Message,
    /// A green dot when something changed, the accent dot otherwise.
    success: bool,
}

pub struct Workbench {
    worker: Worker,
    demo: bool,
    profiles: Vec<ConnectionProfile>,
    workspaces: HashMap<Uuid, WorkspaceInfo>,
    schemas: HashMap<Uuid, Vec<SchemaNode>>,
    schema_filters: HashMap<Uuid, String>,
    collapsed_profiles: HashSet<Uuid>,
    tabs: Vec<Tab>,
    active_tab: Option<u64>,
    active_profile: Option<Uuid>,
    query_results: HashMap<u64, QueryResponse>,
    histories: HashMap<(Uuid, String), Vec<QueryHistoryEntry>>,
    tables: HashMap<u64, TableState>,
    next_id: u64,
    pending_requests: HashMap<RequestId, RequestMeta>,
    queued: VecDeque<Queued>,
    toast: Option<Toast>,
    /// App-level error strip under the top bar.
    banner: Option<Message>,
    /// Inline errors under each query tab's editor.
    query_errors: HashMap<u64, Message>,
    profile_form: Option<ProfileForm>,
    confirm_delete: Option<Uuid>,
    confirm_query: Option<ConfirmQuery>,
    confirm_export: Option<u64>,
    renaming: Option<(u64, String)>,
    sidebar_collapsed: bool,
    completion: Option<Completion>,
    now: f64,
}

impl Workbench {
    pub fn new(cc: &eframe::CreationContext<'_>, demo: bool) -> Self {
        let mut app = Self::with_context(cc.egui_ctx.clone(), demo);
        app.sidebar_collapsed = cc
            .storage
            .and_then(|storage| storage.get_string(SIDEBAR_COLLAPSED_KEY))
            .is_some_and(|value| value == "true");
        app
    }

    fn with_context(context: egui::Context, demo: bool) -> Self {
        theme::configure(&context);
        let worker = Worker::start(context, demo);
        let mut app = Self {
            worker,
            demo,
            profiles: vec![],
            workspaces: HashMap::new(),
            schemas: HashMap::new(),
            schema_filters: HashMap::new(),
            collapsed_profiles: HashSet::new(),
            tabs: vec![],
            active_tab: None,
            active_profile: None,
            query_results: HashMap::new(),
            histories: HashMap::new(),
            tables: HashMap::new(),
            next_id: 1,
            pending_requests: HashMap::new(),
            queued: VecDeque::new(),
            toast: None,
            banner: None,
            query_errors: HashMap::new(),
            profile_form: None,
            confirm_delete: None,
            confirm_query: None,
            confirm_export: None,
            renaming: None,
            sidebar_collapsed: false,
            completion: None,
            now: 0.0,
        };
        app.send(Command::LoadProfiles, "Loading connections", None);
        app
    }

    // ---- requests -------------------------------------------------------

    fn send(&mut self, command: Command, label: &'static str, profile: Option<Uuid>) {
        self.dispatch(command, label, None, profile, RequestKind::Other);
    }

    /// Sends now when the worker is idle, otherwise queues behind the current
    /// request. Replies are routed to the initiating tab, never the selected one.
    fn dispatch(
        &mut self,
        command: Command,
        label: &'static str,
        tab: Option<u64>,
        profile: Option<Uuid>,
        kind: RequestKind,
    ) {
        let meta = RequestMeta {
            label,
            tab,
            profile,
            kind,
            stale: false,
        };
        if self.pending_requests.is_empty() {
            self.submit(command, meta);
        } else {
            self.queued.push_back(Queued { command, meta });
        }
    }

    fn submit(&mut self, command: Command, meta: RequestMeta) {
        let id = RequestId(self.next_id);
        self.next_id += 1;
        self.pending_requests.insert(id, meta);
        if self.worker.tx.send(Work { id, command }).is_err() {
            self.pending_requests.remove(&id);
            self.show_error("Database worker stopped.");
        }
    }

    fn busy(&self, kind: RequestKind, tab: Option<u64>) -> bool {
        self.pending_requests
            .values()
            .chain(self.queued.iter().map(|q| &q.meta))
            .any(|meta| meta.kind == kind && (tab.is_none() || meta.tab == tab))
    }

    fn drain(&mut self) {
        while let Ok(done) = self.worker.rx.try_recv() {
            let Some(meta) = self.pending_requests.remove(&done.id) else {
                continue;
            };
            if meta.stale {
                continue;
            }
            match done.result {
                Ok(payload) => self.apply(payload, &meta),
                Err(error) => self.fail(error, &meta),
            }
        }
        while self.pending_requests.is_empty() {
            let Some(next) = self.queued.pop_front() else {
                break;
            };
            self.submit(next.command, next.meta);
        }
    }

    fn fail(&mut self, error: String, meta: &RequestMeta) {
        match meta.kind {
            RequestKind::Form => {
                if let Some(form) = &mut self.profile_form {
                    form.saving = false;
                    form.feedback = Some((FeedbackKind::Error, error));
                    return;
                }
            }
            RequestKind::Table | RequestKind::Mutation | RequestKind::Export => {
                if let Some(tab) = meta.tab.filter(|tab| self.tables.contains_key(tab)) {
                    if meta.kind == RequestKind::Table {
                        if let Some(state) = self.tables.get_mut(&tab) {
                            state.restore_loaded();
                        }
                    }
                    self.table_error(tab, error);
                    return;
                }
            }
            RequestKind::Query => {
                self.queue_history(meta.tab);
                if let Some(tab) = meta.tab {
                    self.query_errors
                        .insert(tab, Message::error(error, self.now));
                    return;
                }
            }
            RequestKind::Other => {}
        }
        self.show_error(error);
    }

    /// App-level errors go to the strip under the top bar.
    fn show_error(&mut self, message: impl Into<String>) {
        self.banner = Some(Message::error(message, self.now));
    }

    fn show_toast(&mut self, message: impl Into<String>, success: bool) {
        self.toast = Some(Toast {
            message: Message::notice(message, self.now),
            success,
        });
    }

    fn table_error(&mut self, tab: u64, message: impl Into<String>) {
        let now = self.now;
        match self.tables.get_mut(&tab) {
            Some(state) => state.message = Some(Message::error(message, now)),
            None => self.show_error(message),
        }
    }

    fn table_notice(&mut self, tab: u64, message: Message) {
        if let Some(state) = self.tables.get_mut(&tab) {
            state.message = Some(message);
        }
    }

    fn apply(&mut self, payload: Payload, meta: &RequestMeta) {
        let target = meta.tab;
        match payload {
            Payload::Profiles(profiles) => self.profiles = profiles,
            Payload::Tested => {
                let Some(form) = &mut self.profile_form else {
                    return;
                };
                if form.saving {
                    form.feedback = Some((
                        FeedbackKind::Success,
                        "Connection successful. Saving and connecting…".into(),
                    ));
                    let input = form.input();
                    self.dispatch(
                        Command::SaveProfile(input),
                        "Saving connection",
                        None,
                        None,
                        RequestKind::Form,
                    );
                } else {
                    form.feedback = Some((FeedbackKind::Success, "Connection successful.".into()));
                }
            }
            Payload::Profile(profile) => self.profile_saved(profile),
            Payload::Deleted(id) => {
                self.profiles.retain(|p| p.id != id);
                self.close_profile(id);
                self.confirm_delete = None;
                self.profile_form = None;
            }
            Payload::Workspace(workspace) => self.workspace_opened(workspace),
            Payload::Disconnected(id) => self.close_profile(id),
            Payload::Schema(id, tree) => {
                if self.workspaces.contains_key(&id) {
                    let previous = self.schemas.insert(id, tree).unwrap_or_default();
                    if meta.label == "Refreshing schema" {
                        let kind = if self.engine(id) == DatabaseEngine::Redis {
                            "Keyspace"
                        } else {
                            "Schema"
                        };
                        let (changed, message) =
                            describe_schema_refresh(&previous, &self.schemas[&id], kind);
                        self.show_toast(message, changed);
                    }
                }
            }
            Payload::Query(result) => self.query_finished(target, result),
            Payload::History(id, database, entries) => {
                self.histories.insert((id, database), entries);
            }
            Payload::Table(page) => {
                if let Some(state) = target.and_then(|tab| self.tables.get_mut(&tab)) {
                    if let Some(request) = state.requested.take() {
                        state.loaded(page, request);
                    }
                }
            }
            Payload::Mutation(result) => {
                let Some(tab) = target else { return };
                // As in the desktop app: clear staged rows and reload, so rows
                // another writer changed show their current values.
                if let Some(state) = self.tables.get_mut(&tab) {
                    state.pending.clear();
                    state.drafts.clear();
                }
                self.load_table(tab);
                if result.conflicts.is_empty() {
                    let noun = if result.applied == 1 {
                        "change"
                    } else {
                        "changes"
                    };
                    let notice = format!("{} {noun} saved.", result.applied);
                    self.table_notice(tab, Message::notice(notice, self.now));
                } else {
                    self.table_error(
                        tab,
                        format!(
                            "{} row conflict(s); the table was refreshed.",
                            result.conflicts.len()
                        ),
                    );
                }
            }
            Payload::Exported(Some((path, rows))) => {
                if let Some(tab) = target {
                    self.table_notice(tab, Message::exported(path, rows, self.now));
                }
            }
            Payload::Exported(None) => {
                if let Some(tab) = target {
                    self.table_notice(tab, Message::notice("Export canceled.", self.now));
                }
            }
        }
    }

    fn profile_saved(&mut self, profile: ConnectionProfile) {
        let previous = self.profiles.iter().find(|p| p.id == profile.id).cloned();
        let id = profile.id;
        // Saving reconnects the profile; table tabs belong to the server and
        // database they were opened on.
        let target_changed = previous
            .as_ref()
            .is_some_and(|previous| connection_target_changed(previous, &profile));
        if target_changed {
            self.close_table_tabs(id);
        }
        self.workspaces.remove(&id);
        self.schemas.remove(&id);
        self.mark_stale(id);
        if let Some(old) = self.profiles.iter_mut().find(|x| x.id == id) {
            *old = profile;
        } else {
            self.profiles.push(profile);
        }
        self.profiles.sort_by_key(|p| p.name.to_lowercase());
        self.profile_form = None;
        self.active_profile = Some(id);
        self.dispatch(
            Command::Connect(id),
            "Connecting",
            None,
            Some(id),
            RequestKind::Other,
        );
    }

    fn workspace_opened(&mut self, workspace: WorkspaceInfo) {
        let id = workspace.profile.id;
        if self
            .workspaces
            .get(&id)
            .is_some_and(|old| old.profile.default_database != workspace.profile.default_database)
        {
            // Table tabs were opened on the previous database.
            self.close_table_tabs(id);
        }
        self.workspaces.insert(id, workspace);
        self.collapsed_profiles.remove(&id);
        self.activate_profile(id);
        self.dispatch(
            Command::Schema(id),
            "Loading schema",
            None,
            Some(id),
            RequestKind::Other,
        );
    }

    fn query_finished(&mut self, target: Option<u64>, result: QueryResponse) {
        let Some(tab_id) = target else { return };
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
            return;
        };
        let tree = self
            .schemas
            .get(&tab.profile_id)
            .map_or(&[][..], Vec::as_slice);
        tab.embedded = tab
            .last_executed
            .as_deref()
            .and_then(|sql| resolve_full_table_select(sql, tree));
        let embedded = tab.embedded.is_some();
        self.query_results.insert(tab_id, result);
        self.tables.remove(&tab_id);
        if embedded {
            self.tables.insert(tab_id, TableState::default());
            self.load_table(tab_id);
        }
        self.queue_history(Some(tab_id));
    }

    fn queue_history(&mut self, tab: Option<u64>) {
        let Some(profile) = tab
            .and_then(|id| self.tabs.iter().find(|t| t.id == id))
            .map(|t| t.profile_id)
        else {
            return;
        };
        if let Some(workspace) = self.workspaces.get(&profile) {
            let database = workspace.profile.default_database.clone();
            self.dispatch(
                Command::History(profile, database),
                "Loading history",
                None,
                Some(profile),
                RequestKind::Other,
            );
        }
    }

    // ---- tabs and profiles ----------------------------------------------

    fn profile(&self, id: Uuid) -> Option<&ConnectionProfile> {
        self.workspaces
            .get(&id)
            .map(|w| &w.profile)
            .or_else(|| self.profiles.iter().find(|p| p.id == id))
    }

    fn engine(&self, id: Uuid) -> DatabaseEngine {
        self.profile(id)
            .map_or(DatabaseEngine::Postgres, |p| p.engine)
    }

    fn profile_color(&self, id: Uuid) -> Color32 {
        self.profiles
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| p.color.as_deref())
            .map_or(theme::ACCENT, theme::parse_color)
    }

    /// Keeps or restores the profile's workbench: its current tab, its latest
    /// tab, or a new query tab.
    fn activate_profile(&mut self, profile: Uuid) {
        self.active_profile = Some(profile);
        let current = self
            .active_tab
            .and_then(|id| self.tabs.iter().find(|t| t.id == id));
        if current.is_some_and(|t| t.profile_id == profile) {
            return;
        }
        if let Some(tab) = self.tabs.iter().rev().find(|t| t.profile_id == profile) {
            self.active_tab = Some(tab.id);
            return;
        }
        self.open_query(profile);
    }

    fn open_query(&mut self, profile: Uuid) {
        let id = self.next_id;
        self.next_id += 1;
        let titles: HashSet<&str> = self
            .tabs
            .iter()
            .filter(|t| t.profile_id == profile && t.kind == TabKind::Query)
            .map(|t| t.title.as_str())
            .collect();
        let number = (1..).find(|n| !titles.contains(format!("Query {n}").as_str()));
        let sql = default_query_text(self.engine(profile)).to_owned();
        self.tabs.push(Tab {
            id,
            profile_id: profile,
            title: format!("Query {}", number.unwrap_or(1)),
            kind: TabKind::Query,
            selection: (0, 0),
            sql,
            last_executed: None,
            embedded: None,
            collapsed: false,
        });
        self.active_tab = Some(id);
        self.active_profile = Some(profile);
    }

    fn open_table(&mut self, profile: Uuid, schema: String, table: String) {
        let kind = TabKind::Table { schema, table };
        if let Some(existing) = self
            .tabs
            .iter()
            .find(|t| t.profile_id == profile && t.kind == kind)
        {
            self.active_tab = Some(existing.id);
            self.active_profile = Some(profile);
            return;
        }
        let TabKind::Table { schema, table } = &kind else {
            return;
        };
        let title = format!("{schema}.{table}");
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(Tab {
            id,
            profile_id: profile,
            title,
            kind,
            sql: String::new(),
            selection: (0, 0),
            last_executed: None,
            embedded: None,
            collapsed: false,
        });
        self.tables.insert(id, TableState::default());
        self.active_tab = Some(id);
        self.active_profile = Some(profile);
        self.load_table(id);
    }

    fn tab_dirty(&self, tab: u64) -> bool {
        self.tables.get(&tab).is_some_and(TableState::dirty)
    }

    fn select_tab(&mut self, tab_id: u64) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            tab.collapsed = false;
            self.active_tab = Some(tab_id);
            self.active_profile = Some(tab.profile_id);
        }
    }

    /// Shrinks a tab and, if it was active, selects its neighbor, as the
    /// desktop app does.
    fn collapse_tab(&mut self, tab_id: u64) {
        let Some(index) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        self.tabs[index].collapsed = true;
        if self.active_tab == Some(tab_id) {
            let next = self
                .tabs
                .get(index + 1)
                .or_else(|| index.checked_sub(1).and_then(|i| self.tabs.get(i)))
                .map(|t| t.id);
            match next {
                Some(next) => self.select_tab(next),
                None => self.active_tab = None,
            }
        }
    }

    fn close_tab(&mut self, tab_id: u64) {
        if self.tab_dirty(tab_id) {
            self.show_error("Save or discard staged changes before closing this tab.");
            return;
        }
        let Some(index) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        self.tabs.remove(index);
        self.tables.remove(&tab_id);
        self.query_results.remove(&tab_id);
        self.query_errors.remove(&tab_id);
        for meta in self.pending_requests.values_mut() {
            if meta.tab == Some(tab_id) {
                meta.stale = true;
            }
        }
        self.queued.retain(|q| q.meta.tab != Some(tab_id));
        if self.active_tab == Some(tab_id) {
            // Select the tab to the right, else the one to the left.
            let next = self.tabs.get(index).or_else(|| self.tabs.last());
            self.active_tab = next.map(|t| t.id);
            self.active_profile = next.map(|t| t.profile_id).or(self.active_profile);
        }
    }

    fn close_table_tabs(&mut self, profile: Uuid) {
        let removed: Vec<u64> = self
            .tabs
            .iter()
            .filter(|t| t.profile_id == profile && matches!(t.kind, TabKind::Table { .. }))
            .map(|t| t.id)
            .collect();
        for tab in &removed {
            self.tables.remove(tab);
        }
        self.tabs.retain(|t| !removed.contains(&t.id));
        for tab in self.tabs.iter_mut().filter(|t| t.profile_id == profile) {
            if tab.embedded.take().is_some() {
                self.tables.remove(&tab.id);
            }
        }
        if self.active_tab.is_some_and(|id| removed.contains(&id)) {
            self.active_tab = None;
        }
    }

    fn mark_stale(&mut self, profile: Uuid) {
        for meta in self.pending_requests.values_mut() {
            if meta.profile == Some(profile) {
                meta.stale = true;
            }
        }
        self.queued.retain(|q| q.meta.profile != Some(profile));
    }

    fn close_profile(&mut self, id: Uuid) {
        let removed: Vec<_> = self
            .tabs
            .iter()
            .filter(|tab| tab.profile_id == id)
            .map(|tab| tab.id)
            .collect();
        for tab in removed {
            self.tables.remove(&tab);
            self.query_errors.remove(&tab);
            self.tables.remove(&tab);
        }
        self.workspaces.remove(&id);
        self.schemas.remove(&id);
        self.tabs.retain(|t| t.profile_id != id);
        self.mark_stale(id);
        if self.active_profile == Some(id) {
            self.active_profile = None;
        }
        if self
            .active_tab
            .is_some_and(|tab| !self.tabs.iter().any(|t| t.id == tab))
        {
            self.active_tab = None;
        }
    }

    fn profile_is_clean(&mut self, profile: Uuid) -> bool {
        let dirty = self
            .tabs
            .iter()
            .filter(|tab| tab.profile_id == profile)
            .any(|tab| self.tab_dirty(tab.id));
        if dirty {
            self.show_error("Save or discard this connection's staged edits first.");
        }
        !dirty
    }

    /// The table a tab edits: its own table, or the table its last
    /// `SELECT *` resolved to.
    fn table_target(&self, tab_id: u64) -> Option<(Uuid, String, String)> {
        let tab = self.tabs.iter().find(|t| t.id == tab_id)?;
        match &tab.kind {
            TabKind::Table { schema, table } => {
                Some((tab.profile_id, schema.clone(), table.clone()))
            }
            TabKind::Query => tab
                .embedded
                .clone()
                .map(|(schema, table)| (tab.profile_id, schema, table)),
        }
    }

    fn load_table(&mut self, tab_id: u64) {
        if self
            .tables
            .get(&tab_id)
            .is_some_and(|state| !state.pending.is_empty())
        {
            self.table_error(tab_id, PENDING_REFRESH_ERROR);
            return;
        }
        let Some((profile, schema, table)) = self.table_target(tab_id) else {
            return;
        };
        let state = self.tables.entry(tab_id).or_default();
        state.loading = true;
        // Paging, filtering, or sorting replaces a stale notice, as in the
        // desktop app; errors and export results stay until they expire.
        if state
            .message
            .as_ref()
            .is_some_and(|m| m.kind == messages::Kind::Notice && m.export.is_none())
        {
            state.message = None;
        }
        let request = state.request(profile, &schema, &table);
        state.requested = Some(request.clone());
        self.dispatch(
            Command::Table(request),
            "Loading rows",
            Some(tab_id),
            Some(profile),
            RequestKind::Table,
        );
    }

    fn run_query(&mut self, tab_id: u64, sql: String, refresh: bool, confirmed: bool) {
        let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else {
            return;
        };
        let profile = tab.profile_id;
        let sql = sql.trim().to_owned();
        if sql.is_empty() || self.busy(RequestKind::Query, Some(tab_id)) {
            return;
        }
        if self.tab_dirty(tab_id) {
            self.query_errors.insert(
                tab_id,
                Message::error(
                    "Save or discard the pending table changes before running another query.",
                    self.now,
                ),
            );
            return;
        }
        if !confirmed && requires_confirmation(self.engine(profile), &sql) {
            self.confirm_query = Some(ConfirmQuery {
                tab: tab_id,
                sql,
                refresh,
            });
            return;
        }
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            tab.last_executed = Some(sql.clone());
        }
        self.query_errors.remove(&tab_id);
        self.dispatch(
            Command::Query(QueryRequest {
                profile_id: profile,
                sql,
                max_rows: Some(MAX_QUERY_ROWS),
            }),
            if refresh {
                "Refreshing query"
            } else {
                "Running query"
            },
            Some(tab_id),
            Some(profile),
            RequestKind::Query,
        );
    }

    fn mutation_batch(&self, tab_id: u64) -> Option<MutationBatch> {
        let (profile_id, schema, table) = self.table_target(tab_id)?;
        let state = self.tables.get(&tab_id)?;
        Some(MutationBatch {
            profile_id,
            schema,
            table,
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

    /// Saves staged rows directly, as the desktop app's Save changes does.
    fn save_changes(&mut self, tab_id: u64) {
        if self.busy(RequestKind::Mutation, Some(tab_id)) {
            return;
        }
        if let Some(batch) = self
            .mutation_batch(tab_id)
            .filter(|b| !b.mutations.is_empty())
        {
            let profile = batch.profile_id;
            self.dispatch(
                Command::Mutate(batch),
                "Saving changes",
                Some(tab_id),
                Some(profile),
                RequestKind::Mutation,
            );
        }
    }

    fn start_export(&mut self, tab_id: u64) {
        let Some((profile, schema, table)) = self.table_target(tab_id) else {
            return;
        };
        let Some(state) = self.tables.get(&tab_id) else {
            return;
        };
        let (Some(page), Some(loaded)) = (&state.page, &state.loaded) else {
            return;
        };
        let request = ExportRequest {
            page: TablePageRequest {
                profile_id: profile,
                ..loaded.clone()
            },
            columns: page
                .metadata
                .columns
                .iter()
                .map(|c| c.name.clone())
                .collect(),
            file_name: format!("{}.{}.csv", safe_file_name(&schema), safe_file_name(&table)),
            progress: std::sync::Arc::default(),
        };
        if let Some(state) = self.tables.get_mut(&tab_id) {
            state.export_progress = Some(request.progress.clone());
            state.message = None;
        }
        self.dispatch(
            Command::ExportCsv(request),
            "Exporting CSV",
            Some(tab_id),
            Some(profile),
            RequestKind::Export,
        );
    }
}

impl eframe::App for Workbench {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(SIDEBAR_COLLAPSED_KEY, self.sidebar_collapsed.to_string());
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.now = ctx.input(|input| input.time);
        self.drain();
        if ctx.input(|input| input.viewport().close_requested()) {
            let dirty = self.tabs.iter().any(|tab| self.tab_dirty(tab.id));
            if dirty || !self.pending_requests.is_empty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.show_error(if dirty {
                    "Save or discard staged changes before closing DBM."
                } else {
                    "Wait for the active database operation to finish before closing DBM."
                });
            }
        }
        if !self.pending_requests.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        self.sidebar(ctx);
        self.top_bar(ctx);
        self.banner_ui(ctx);
        self.tab_strip(ctx);
        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::BG))
            .show(ctx, |ui| self.content_ui(ui));
        self.dialogs(ctx);
        self.toast_ui(ctx);
    }
}

// ---- chrome ---------------------------------------------------------------

impl Workbench {
    fn sidebar(&mut self, ctx: &egui::Context) {
        if self.sidebar_collapsed {
            egui::SidePanel::left("sidebar-collapsed")
                .exact_width(48.0)
                .resizable(false)
                .frame(
                    Frame::new()
                        .fill(theme::SIDEBAR)
                        .inner_margin(Margin::symmetric(8, 12))
                        .stroke(Stroke::new(1.0, theme::BORDER)),
                )
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        if icons::button(ui, Icon::Sidebar, None, "Expand sidebar").clicked() {
                            self.sidebar_collapsed = false;
                        }
                    });
                });
            return;
        }
        egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(260.0)
            .width_range(220.0..=480.0)
            .frame(
                Frame::new()
                    .fill(theme::SIDEBAR)
                    .inner_margin(Margin::symmetric(10, 10))
                    .stroke(Stroke::new(1.0, theme::BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(6), theme::ACCENT_STRONG);
                    icons::paint(
                        ui.painter(),
                        rect.shrink(4.0),
                        Icon::Database,
                        Color32::WHITE,
                    );
                    ui.label(
                        RichText::new("DBM")
                            .strong()
                            .size(14.0)
                            .color(theme::TEXT_STRONG),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::button(ui, Icon::Sidebar, None, "Collapse sidebar").clicked() {
                            self.sidebar_collapsed = true;
                        }
                    });
                });
                ui.add_space(10.0);
                egui::TopBottomPanel::bottom("sidebar-footer")
                    .frame(Frame::new().inner_margin(Margin::symmetric(0, 8)))
                    .show_inside(ui, |ui| {
                        let button = ui.add_sized(
                            [ui.available_width(), 30.0],
                            egui::Button::new("New connection"),
                        );
                        let icon_rect = egui::Rect::from_center_size(
                            button.rect.center() - Vec2::new(62.0, 0.0),
                            Vec2::splat(13.0),
                        );
                        icons::paint(ui.painter(), icon_rect, Icon::Plus, theme::SECONDARY);
                        if button.clicked() {
                            self.profile_form = Some(ProfileForm::fresh());
                        }
                    });
                ui.label(section_label("Connections"));
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.profiles.is_empty() {
                            ui.label(RichText::new("No saved connections.").color(theme::MUTED));
                        }
                        for profile in self.profiles.clone() {
                            self.connection_item(ui, &profile);
                        }
                    });
            });
    }

    fn connection_item(&mut self, ui: &mut egui::Ui, profile: &ConnectionProfile) {
        let id = profile.id;
        let connected = self.workspaces.contains_key(&id);
        let active = self.active_profile == Some(id);
        let expanded = active && connected && !self.collapsed_profiles.contains(&id);
        let color = self.profile_color(id);
        let connecting = self
            .pending_requests
            .values()
            .any(|m| m.profile == Some(id) && m.label == "Connecting");
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 44.0), Sense::click());
        let response = response.on_hover_text(if connected { "" } else { "Connect" });
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, &profile.name)
        });
        let painter = ui.painter();
        if active {
            painter.rect_filled(rect, 7, theme::CONTROL);
        } else if response.hovered() {
            painter.rect_filled(rect, 7, Color32::from_white_alpha(8));
        }
        let dot = egui::Pos2::new(rect.left() + 12.0, rect.top() + 15.0);
        if connected {
            painter.circle_filled(dot, 4.0, color);
        } else {
            painter.circle_stroke(dot, 3.5, Stroke::new(1.5, color));
        }
        let text_left = rect.left() + 26.0;
        let actions_width = if connected && active { 60.0 } else { 34.0 };
        let chip_width = if profile.read_only { 68.0 } else { 0.0 };
        let text_width = (rect.right() - actions_width - chip_width - text_left).max(40.0);
        let single_line = |text: String, font: egui::FontId, color: Color32| {
            let mut job = LayoutJob::simple_singleline(text, font, color);
            job.wrap = egui::text::TextWrapping::truncate_at_width(text_width);
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        };
        let name = single_line(profile.name.clone(), ui_font(13.0), theme::TEXT_STRONG);
        let target = if profile.username.is_empty() {
            profile.host.clone()
        } else {
            format!("{}@{}", profile.username, profile.host)
        };
        let subtitle = single_line(
            format!("{} · {target}", engine_label(profile.engine)),
            ui_font(11.0),
            theme::FAINT,
        );
        ui.painter().galley(
            egui::Pos2::new(text_left, rect.top() + 6.0),
            name,
            theme::TEXT_STRONG,
        );
        ui.painter().galley(
            egui::Pos2::new(text_left, rect.top() + 24.0),
            subtitle,
            theme::FAINT,
        );
        if response.clicked() {
            if connected {
                self.activate_profile(id);
            } else {
                self.active_profile = Some(id);
                self.send(Command::Connect(id), "Connecting", Some(id));
            }
        }
        let actions = egui::Rect::from_min_max(
            egui::Pos2::new(rect.right() - actions_width, rect.top()),
            egui::Pos2::new(rect.right() - 2.0, rect.top() + 30.0),
        );
        {
            let ui = &mut ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(actions)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            {
                ui.spacing_mut().item_spacing.x = 0.0;
                let more = icons::button(ui, Icon::More, None, "Connection actions");
                egui::Popup::menu(&more).show(|ui| {
                    if ui.button("Edit connection").clicked() {
                        self.profile_form = Some(ProfileForm::from_profile(profile));
                        ui.close();
                    }
                    if connected
                        && ui
                            .add(theme::danger_button("Disconnect"))
                            .on_hover_text("Close this connection and its tabs")
                            .clicked()
                    {
                        if self.profile_is_clean(id) {
                            self.send(Command::Disconnect(id), "Disconnecting", None);
                        }
                        ui.close();
                    }
                });
                if connecting {
                    ui.spinner();
                } else if connected && active {
                    let (icon, tip) = if expanded {
                        (Icon::ChevronUp, "Collapse connection")
                    } else {
                        (Icon::ChevronDown, "Expand connection")
                    };
                    if icons::button(ui, icon, None, tip).clicked()
                        && !self.collapsed_profiles.remove(&id)
                    {
                        self.collapsed_profiles.insert(id);
                    }
                }
            }
        }
        if profile.read_only {
            let chip_rect = egui::Rect::from_min_size(
                egui::Pos2::new(rect.right() - actions_width - 64.0, rect.top() + 4.0),
                Vec2::new(64.0, 20.0),
            );
            chip(
                &mut ui.new_child(egui::UiBuilder::new().max_rect(chip_rect)),
                "Read-only",
                theme::MUTED,
            );
        }
        if !expanded {
            return;
        }
        let Some(workspace) = self.workspaces.get(&id).cloned() else {
            return;
        };
        let redis = profile.engine == DatabaseEngine::Redis;
        ui.indent(("workspace", id), |ui| {
            ui.label(section_label(if redis {
                "Database index"
            } else {
                "Database"
            }));
            let mut database = workspace.profile.default_database.clone();
            egui::ComboBox::from_id_salt(("db", id))
                .selected_text(&database)
                .width(ui.available_width() - 4.0)
                .show_ui(ui, |ui| {
                    for db in workspace.databases.iter().filter(|d| d.is_connectable) {
                        ui.selectable_value(&mut database, db.name.clone(), &db.name);
                    }
                });
            if database != workspace.profile.default_database && self.profile_is_clean(id) {
                self.send(
                    Command::SwitchDatabase(id, database),
                    "Switching database",
                    Some(id),
                );
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(section_label(if redis { "Keyspace" } else { "Schema" }));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let refreshing = self.pending_requests.values().any(|m| {
                        m.profile == Some(id)
                            && (m.label == "Loading schema" || m.label == "Refreshing schema")
                    });
                    let label = if refreshing {
                        "Refreshing…"
                    } else {
                        "Refresh"
                    };
                    if icon_btn(
                        ui,
                        !refreshing,
                        Icon::Refresh,
                        Some(label),
                        "Reload the schema tree",
                    ) {
                        self.send(Command::Schema(id), "Refreshing schema", Some(id));
                    }
                });
            });
            let filter = self.schema_filters.entry(id).or_default();
            let response = ui.add(
                egui::TextEdit::singleline(filter)
                    .hint_text(if redis {
                        "Filter keys…"
                    } else {
                        "Filter tables…"
                    })
                    .desired_width(f32::INFINITY),
            );
            if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                filter.clear();
            }
            let query = filter.clone();
            let nodes =
                filter_schema_nodes(self.schemas.get(&id).map_or(&[][..], Vec::as_slice), &query);
            let selected = self
                .active_tab
                .and_then(|tab| self.table_target(tab))
                .filter(|(profile, _, _)| *profile == id)
                .map(|(_, schema, table)| (schema, table));
            let mut open = None;
            for node in &nodes {
                schema_branch(
                    ui,
                    node,
                    0,
                    !query.trim().is_empty(),
                    (selected.as_ref(), color),
                    &mut open,
                );
            }
            if !query.trim().is_empty() && nodes.is_empty() {
                ui.label(
                    RichText::new(format!("No matches for “{}”.", query.trim()))
                        .color(theme::MUTED),
                );
            }
            if !self.schemas.contains_key(&id) {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Loading…").color(theme::MUTED));
                });
            }
            if let Some((schema, table)) = open {
                self.open_table(id, schema, table);
            }
        });
        ui.add_space(6.0);
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        let profile = self
            .active_tab
            .and_then(|id| self.tabs.iter().find(|t| t.id == id))
            .map(|t| t.profile_id)
            .or(self.active_profile)
            .and_then(|id| self.profile(id).cloned());
        egui::TopBottomPanel::top("top-bar")
            .exact_height(40.0)
            .frame(
                Frame::new()
                    .fill(theme::CHROME)
                    .inner_margin(Margin::symmetric(14, 0))
                    .stroke(Stroke::new(1.0, theme::BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if let Some(profile) = &profile {
                        let color = self.profile_color(profile.id);
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                        ui.painter().circle_filled(rect.center(), 4.0, color);
                        ui.label(RichText::new(&profile.name).strong().color(theme::TEXT_STRONG));
                        let target = if profile.username.is_empty() {
                            format!("{}:{}/{}", profile.host, profile.port, profile.default_database)
                        } else {
                            format!(
                                "{}@{}:{}/{}",
                                profile.username, profile.host, profile.port, profile.default_database
                            )
                        };
                        ui.label(RichText::new(target).font(mono(11.0)).color(theme::FAINT));
                        if profile.read_only {
                            chip(ui, "Read-only", theme::MUTED);
                        }
                    } else {
                        ui.label(RichText::new("No active connection").color(theme::MUTED));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.demo {
                            chip(ui, "Demo fixture · not a live connection", theme::MODIFIED)
                                .on_hover_text("Isolated deterministic data; profiles and edits are not persisted.");
                        }
                        if let Some(meta) = self.pending_requests.values().next() {
                            ui.label(RichText::new(meta.label).color(theme::MUTED));
                            ui.spinner();
                        }
                    });
                });
            });
    }

    fn tab_strip(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("tab-strip")
            .exact_height(36.0)
            .frame(
                Frame::new()
                    .fill(theme::CHROME)
                    .inner_margin(Margin {
                        left: 6,
                        right: 6,
                        top: 4,
                        bottom: 0,
                    })
                    .stroke(Stroke::new(1.0, theme::BORDER)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::horizontal()
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 1.0;
                            for tab in self.tabs.clone() {
                                self.tab_button(ui, &tab);
                            }
                            if let Some(profile) = self
                                .active_profile
                                .filter(|p| self.workspaces.contains_key(p))
                            {
                                if icon_btn(ui, true, Icon::Plus, None, "New query") {
                                    self.open_query(profile);
                                }
                            }
                        });
                    });
            });
    }

    fn tab_button(&mut self, ui: &mut egui::Ui, tab: &Tab) {
        let active = self.active_tab == Some(tab.id);
        let color = self.profile_color(tab.profile_id);
        if tab.collapsed {
            let (rect, response) = ui.allocate_exact_size(Vec2::new(64.0, 31.0), Sense::click());
            let response = response.on_hover_text(format!("Expand {}", tab.title));
            if response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    CornerRadius {
                        nw: 7,
                        ne: 7,
                        sw: 0,
                        se: 0,
                    },
                    Color32::from_white_alpha(10),
                );
            }
            icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(
                    rect.center() - Vec2::new(8.0, 6.0),
                    Vec2::splat(12.0),
                ),
                Icon::Expand,
                theme::FAINT,
            );
            let mut job =
                LayoutJob::simple_singleline(tab.title.clone(), ui_font(9.5), theme::FAINT);
            job.wrap = egui::text::TextWrapping::truncate_at_width(44.0);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(
                egui::pos2(rect.left() + 5.0, rect.center().y + 2.0),
                galley,
                theme::FAINT,
            );
            let close = egui::Rect::from_center_size(
                rect.right_center() - Vec2::new(10.0, 0.0),
                Vec2::splat(16.0),
            );
            let close_response = ui
                .interact(close, response.id.with("close"), Sense::click())
                .on_hover_text(format!("Close {}", tab.title));
            icons::paint(
                ui.painter(),
                close.shrink(3.0),
                Icon::Close,
                if close_response.hovered() {
                    theme::TEXT
                } else {
                    theme::FAINT
                },
            );
            if close_response.clicked() {
                self.close_tab(tab.id);
            } else if response.clicked() {
                self.select_tab(tab.id);
            }
            return;
        }
        let frame = Frame::new()
            .fill(if active {
                theme::BG
            } else {
                Color32::TRANSPARENT
            })
            .stroke(if active {
                Stroke::new(1.0, theme::BORDER)
            } else {
                Stroke::NONE
            })
            .corner_radius(CornerRadius {
                nw: 7,
                ne: 7,
                sw: 0,
                se: 0,
            })
            .inner_margin(Margin {
                left: 10,
                right: 4,
                top: 0,
                bottom: 0,
            });
        let response = frame
            .show(ui, |ui| {
                ui.set_height(31.0);
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let glyph = if tab.kind == TabKind::Query {
                        Icon::Code
                    } else {
                        Icon::Table
                    };
                    icons::show(ui, glyph, 13.0, color.gamma_multiply(0.85));
                    if let Some((_, draft)) = self.renaming.as_mut().filter(|(id, _)| *id == tab.id)
                    {
                        let edit = ui.add(egui::TextEdit::singleline(draft).desired_width(120.0));
                        edit.request_focus();
                        if edit.lost_focus() {
                            let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
                            let title = draft.trim().to_owned();
                            if !escape && !title.is_empty() {
                                if let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab.id) {
                                    t.title = title;
                                }
                            }
                            self.renaming = None;
                        }
                    } else {
                        let title = ui.add(
                            egui::Label::new(RichText::new(&tab.title).size(12.5).color(
                                if active {
                                    theme::TEXT_STRONG
                                } else {
                                    theme::MUTED
                                },
                            ))
                            .truncate()
                            .sense(Sense::click()),
                        );
                        if title.clicked() {
                            self.select_tab(tab.id);
                        }
                        if title.double_clicked() && tab.kind == TabKind::Query {
                            self.renaming = Some((tab.id, tab.title.clone()));
                        }
                        if title.middle_clicked() {
                            self.close_tab(tab.id);
                        }
                    }
                    if tab.kind == TabKind::Query
                        && self.renaming.is_none()
                        && icon_btn(
                            ui,
                            true,
                            Icon::Pencil,
                            None,
                            &format!("Rename {}", tab.title),
                        )
                    {
                        self.renaming = Some((tab.id, tab.title.clone()));
                    }
                    if active
                        && icon_btn(
                            ui,
                            true,
                            Icon::Collapse,
                            None,
                            &format!("Collapse {}", tab.title),
                        )
                    {
                        self.collapse_tab(tab.id);
                    }
                    if icon_btn(ui, true, Icon::Close, None, &format!("Close {}", tab.title)) {
                        self.close_tab(tab.id);
                    }
                });
            })
            .response;
        if active {
            let rect = response.rect;
            ui.painter().rect_filled(
                egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), 2.0)),
                CornerRadius {
                    nw: 7,
                    ne: 7,
                    sw: 0,
                    se: 0,
                },
                color,
            );
        }
    }

    fn toast_ui(&mut self, ctx: &egui::Context) {
        let Some(toast) = &self.toast else { return };
        let Some(opacity) = toast.message.opacity(ctx, self.now) else {
            self.toast = None;
            return;
        };
        let dot = if toast.success {
            theme::SUCCESS
        } else {
            theme::ACCENT
        };
        if messages::toast(ctx, &toast.message.text, dot, opacity) {
            self.toast = None;
        }
    }

    fn banner_ui(&mut self, ctx: &egui::Context) {
        let Some(banner) = &self.banner else { return };
        let Some(opacity) = banner.opacity(ctx, self.now) else {
            self.banner = None;
            return;
        };
        let mut dismiss = false;
        egui::TopBottomPanel::top("error-banner")
            .frame(messages::banner_frame(opacity))
            .show_separator_line(false)
            .show(ctx, |ui| {
                dismiss = messages::banner(ui, banner, opacity) == messages::Response::Dismiss;
            });
        if dismiss {
            self.banner = None;
        }
    }
}

/// Opens an exported file with its default app, or reveals it in the file
/// manager (Windows selects it; Linux opens its folder).
pub(crate) fn open_path(path: &std::path::Path, reveal: bool) {
    #[cfg(target_os = "windows")]
    let result = if reveal {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()
    } else {
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .spawn()
    };
    #[cfg(target_os = "macos")]
    let result = if reveal {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
    } else {
        std::process::Command::new("open").arg(path).spawn()
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let result = std::process::Command::new("xdg-open")
        .arg(if reveal {
            path.parent().unwrap_or(path)
        } else {
            path
        })
        .spawn();
    drop(result);
}

fn schema_branch(
    ui: &mut egui::Ui,
    node: &SchemaNode,
    depth: usize,
    force_open: bool,
    (selected, color): (Option<&(String, String)>, Color32),
    open: &mut Option<(String, String)>,
) {
    let leaf = match (&node.schema, &node.table) {
        (Some(schema), Some(table)) => Some((schema.clone(), table.clone())),
        _ => None,
    };
    let indent = 4.0 + depth as f32 * 14.0;
    if let Some(target) = leaf {
        let glyph = match node.kind.as_str() {
            "key" => Icon::Key,
            "view" => Icon::View,
            _ => Icon::Table,
        };
        let is_selected = selected == Some(&target);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, is_selected, &node.name)
        });
        if is_selected {
            ui.painter().rect_filled(rect, 6, color.gamma_multiply(0.3));
        } else if response.hovered() {
            ui.painter()
                .rect_filled(rect, 6, Color32::from_white_alpha(8));
        }
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + indent + 20.0, rect.center().y),
            Vec2::splat(13.0),
        );
        let icon_color = if is_selected { color } else { theme::MUTED };
        icons::paint(ui.painter(), icon_rect, glyph, icon_color);
        let mut job = LayoutJob::simple_singleline(
            node.name.clone(),
            ui_font(13.0),
            if is_selected {
                theme::TEXT_STRONG
            } else {
                theme::SECONDARY
            },
        );
        job.wrap =
            egui::text::TextWrapping::truncate_at_width(rect.right() - icon_rect.right() - 12.0);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let text_pos = egui::pos2(
            icon_rect.right() + 8.0,
            rect.center().y - galley.size().y / 2.0,
        );
        ui.painter().galley(text_pos, galley, theme::SECONDARY);
        if response.clicked() {
            *open = Some(target);
        }
        return;
    }
    let id = ui.make_persistent_id(("schema-node", depth, &node.kind, &node.name));
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, depth < 1);
    if force_open {
        state.set_open(true);
    }
    let header = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space(indent);
        let caret = if state.is_open() {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };
        icons::show(ui, caret, 12.0, theme::FAINT);
        let text = RichText::new(&node.name).color(theme::SECONDARY);
        ui.add(egui::Button::new(text).frame(false)).clicked()
    });
    if header.inner {
        state.toggle(ui);
    }
    state.show_body_unindented(ui, |ui| {
        for child in &node.children {
            schema_branch(ui, child, depth + 1, force_open, (selected, color), open);
        }
    });
}

// ---- content --------------------------------------------------------------

impl Workbench {
    fn content_ui(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.active_tab else {
            self.welcome(ui);
            return;
        };
        let Some(tab) = self.tabs.iter().find(|t| t.id == id).cloned() else {
            self.welcome(ui);
            return;
        };
        match tab.kind {
            TabKind::Query => self.query_ui(ui, &tab),
            TabKind::Table { .. } => self.table_ui(ui, id, tab.profile_id, false),
        }
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        let profile = self.active_profile.and_then(|id| self.profile(id).cloned());
        let connected = profile
            .as_ref()
            .is_some_and(|p| self.workspaces.contains_key(&p.id));
        let title = profile
            .as_ref()
            .map_or("No connection selected", |p| p.name.as_str())
            .to_owned();
        let message = match (&profile, connected) {
            (Some(_), true) => {
                "Choose a table from the sidebar or open a new query with the plus button above."
            }
            (Some(_), false) => {
                "This connection is selected but not connected. Select it again to connect."
            }
            (None, _) if self.profiles.is_empty() => {
                "Create a connection from the sidebar to get started."
            }
            (None, _) => "Select a saved connection from the sidebar to browse its data.",
        };
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.18);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
            ui.painter().rect_filled(
                rect,
                CornerRadius::same(16),
                Color32::from_rgb(0x27, 0x27, 0x2a),
            );
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(16),
                Stroke::new(1.0, Color32::from_white_alpha(18)),
                egui::StrokeKind::Inside,
            );
            icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(rect.center(), Vec2::splat(28.0)),
                Icon::Database,
                theme::ACCENT_TEXT,
            );
            ui.add_space(22.0);
            ui.label(
                RichText::new(title)
                    .size(20.0)
                    .strong()
                    .color(theme::TEXT_STRONG),
            );
            ui.add_space(8.0);
            ui.add(egui::Label::new(RichText::new(message).size(13.0).color(theme::MUTED)).wrap());
        });
    }

    fn query_ui(&mut self, ui: &mut egui::Ui, tab: &Tab) {
        let tab_id = tab.id;
        let engine = self.engine(tab.profile_id);
        let redis = engine == DatabaseEngine::Redis;
        let running = self.busy(RequestKind::Query, Some(tab_id));
        let target = execution_target(engine, &tab.sql, tab.selection.0, tab.selection.1);
        let run_label = match target.as_ref().map(|t| t.kind) {
            Some(ExecutionKind::Selection) => "Run selection",
            _ if redis => "Run command",
            _ => "Run statement",
        };
        let embedded_dirty = self.tab_dirty(tab_id);
        let mut run: Option<(String, bool)> = None;

        egui::TopBottomPanel::top(egui::Id::new(("query-toolbar", tab_id)))
            .exact_height(theme::TOOLBAR_HEIGHT + 16.0)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(theme::BG)
                    .inner_margin(Margin::symmetric(14, 0)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.vertical(|ui| {
                        ui.add_space(2.0);
                        ui.label(eyebrow(if redis {
                            "REDIS WORKBENCH"
                        } else {
                            "SQL WORKBENCH"
                        }));
                        ui.label(
                            RichText::new(&tab.title)
                                .font(ui_font(15.0))
                                .strong()
                                .color(theme::TEXT_STRONG),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label =
                            if running && self.pending_label(tab_id) == Some("Running query") {
                                "Running…".to_owned()
                            } else {
                                format!("      {run_label}   Ctrl+Enter")
                            };
                        let run_button =
                            ui.add_enabled(!running && target.is_some(), primary_button(&label));
                        if !running {
                            let icon_rect = egui::Rect::from_center_size(
                                run_button.rect.left_center() + Vec2::new(16.0, 0.0),
                                Vec2::splat(11.0),
                            );
                            icons::paint(ui.painter(), icon_rect, Icon::Play, Color32::WHITE);
                        }
                        if run_button
                            .on_hover_text(if redis {
                                "Run the selected command (Command/Ctrl+Enter)"
                            } else {
                                "Run the selected SQL (Command/Ctrl+Enter)"
                            })
                            .clicked()
                        {
                            run = target.as_ref().map(|t| (t.sql.clone(), false));
                        }
                        let refresh_label =
                            if running && self.pending_label(tab_id) == Some("Refreshing query") {
                                "Refreshing…"
                            } else {
                                "Refresh"
                            };
                        if icon_btn(
                            ui,
                            !running && tab.last_executed.is_some() && !embedded_dirty,
                            Icon::Refresh,
                            Some(refresh_label),
                            if tab.last_executed.is_some() {
                                "Re-run the last executed statement for fresh results"
                            } else {
                                "Run a statement first to enable refresh"
                            },
                        ) {
                            run = tab.last_executed.clone().map(|sql| (sql, true));
                        }
                    });
                });
            });

        egui::TopBottomPanel::top(egui::Id::new(("query-editor", tab_id)))
            .resizable(true)
            .default_height(260.0)
            .height_range(140.0..=(ui.available_height() - 120.0).max(160.0))
            .frame(Frame::new().fill(theme::BG).inner_margin(Margin {
                left: 14,
                right: 14,
                top: 0,
                bottom: 12,
            }))
            .show_separator_line(false)
            .show_inside(ui, |ui| {
                let card = |fill| {
                    Frame::new()
                        .fill(fill)
                        .stroke(Stroke::new(1.0, theme::BORDER))
                        .corner_radius(CornerRadius::same(10))
                };
                egui::SidePanel::right(egui::Id::new(("query-history", tab_id)))
                    .resizable(true)
                    .default_width(250.0)
                    .width_range(200.0..=460.0)
                    .show_separator_line(false)
                    .frame(card(theme::CHROME).outer_margin(Margin {
                        left: 10,
                        right: 0,
                        top: 0,
                        bottom: 0,
                    }))
                    .show_inside(ui, |ui| {
                        if let Some(sql) = self.history_ui(ui, tab.profile_id) {
                            if let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                                t.selection = (sql.len(), sql.len());
                                t.sql = sql;
                            }
                        }
                    });
                egui::CentralPanel::default()
                    .frame(card(theme::BG).inner_margin(Margin::symmetric(8, 6)))
                    .show_inside(ui, |ui| {
                        egui::TopBottomPanel::bottom(egui::Id::new(("editor-hint", tab_id)))
                            .frame(Frame::new().inner_margin(Margin::symmetric(4, 5)))
                            .show_inside(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(if redis {
                                            "The outlined command or selection will run · Command/Ctrl+Enter · results capped at 10,000 rows"
                                        } else {
                                            "The outlined statement or selected SQL will run · Command/Ctrl+Enter · results capped at 10,000 rows"
                                        })
                                        .font(ui_font(11.0))
                                        .color(theme::FAINT),
                                    );
                                });
                            });
                        if let Some(sql) = self.sql_editor(ui, tab_id, engine) {
                            run = Some((sql, false));
                        }
                    });
            });

        if let Some((sql, refresh)) = run {
            self.run_query(tab_id, sql, refresh, false);
        }

        if let Some(error) = self.query_errors.get(&tab_id).cloned() {
            match error.opacity(ui.ctx(), self.now) {
                None => {
                    self.query_errors.remove(&tab_id);
                }
                Some(opacity) => {
                    egui::TopBottomPanel::top(egui::Id::new(("query-error", tab_id)))
                        .frame(Frame::new().fill(theme::BG).inner_margin(Margin {
                            left: 2,
                            right: 2,
                            top: 0,
                            bottom: 8,
                        }))
                        .show_separator_line(false)
                        .show_inside(ui, |ui| {
                            if messages::inline(ui, &error, opacity) == messages::Response::Dismiss
                            {
                                self.query_errors.remove(&tab_id);
                            }
                        });
                }
            }
        }

        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::BG))
            .show_inside(ui, |ui| self.query_result_ui(ui, tab_id, tab.profile_id));
    }

    fn pending_label(&self, tab: u64) -> Option<&'static str> {
        self.pending_requests
            .values()
            .chain(self.queued.iter().map(|q| &q.meta))
            .find(|m| m.tab == Some(tab))
            .map(|m| m.label)
    }

    /// The code editor. Returns SQL to run when Command/Ctrl+Enter is pressed.
    fn sql_editor(
        &mut self,
        ui: &mut egui::Ui,
        tab_id: u64,
        engine: DatabaseEngine,
    ) -> Option<String> {
        let editor_id = egui::Id::new(("sql-editor", tab_id));
        let focused = ui.memory(|m| m.has_focus(editor_id));
        let run_pressed =
            focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));
        let mut completion = self
            .completion
            .take()
            .filter(|c| c.tab == tab_id && focused);
        let tab = self.tabs.iter_mut().find(|t| t.id == tab_id)?;
        // Completion keys are handled before the editor sees them.
        let mut accept = None;
        if let Some(open) = &mut completion {
            ui.input_mut(|i| {
                let none = egui::Modifiers::NONE;
                if i.consume_key(none, egui::Key::ArrowDown) {
                    open.selected = (open.selected + 1) % open.items.len();
                }
                if i.consume_key(none, egui::Key::ArrowUp) {
                    open.selected = (open.selected + open.items.len() - 1) % open.items.len();
                }
                if i.consume_key(none, egui::Key::Enter) || i.consume_key(none, egui::Key::Tab) {
                    accept = Some(open.selected);
                }
                if i.consume_key(none, egui::Key::Escape) {
                    open.items.clear();
                }
            });
        }
        if let (Some(index), Some(open)) = (accept, &completion) {
            let word = open.items[index].clone();
            let end = tab.selection.1.min(tab.sql.len());
            tab.sql.replace_range(open.start..end, &word);
            let cursor = open.start + word.len();
            tab.selection = (cursor, cursor);
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), editor_id) {
                let ccursor = egui::text::CCursor::new(tab.sql[..cursor].chars().count());
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(ccursor)));
                state.store(ui.ctx(), editor_id);
            }
            completion = None;
        }
        completion = completion.filter(|c| !c.items.is_empty());
        // The statement Command/Ctrl+Enter would run, as a char range.
        let active = {
            let (from, to) = tab.selection;
            (from == to)
                .then(|| execution_target(engine, &tab.sql, from, to))
                .flatten()
                .map(|t| tab.sql[..t.from].chars().count()..tab.sql[..t.to].chars().count())
        };
        let mut layouter = move |ui: &egui::Ui, text: &dyn egui::TextBuffer, _wrap: f32| {
            let job = editor_layout(engine, text.as_str());
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        };
        let line_count = tab.sql.split('\n').count().max(1);
        let output = egui::ScrollArea::both()
            .id_salt(("sql-scroll", tab_id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let numbers = (1..=line_count)
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join("\n");
                    ui.add(
                        egui::Label::new(
                            RichText::new(numbers).font(mono(13.0)).color(theme::FAINT),
                        )
                        .halign(egui::Align::RIGHT)
                        .selectable(false),
                    );
                    let highlight = ui.painter().add(egui::Shape::Noop);
                    let output = egui::TextEdit::multiline(&mut tab.sql)
                        .id(editor_id)
                        .font(egui::TextStyle::Monospace)
                        .code_editor()
                        .frame(false)
                        .background_color(theme::BG)
                        .desired_width(f32::INFINITY)
                        .desired_rows(12)
                        .lock_focus(true)
                        .layouter(&mut layouter)
                        .show(ui);
                    if let Some(active) = &active {
                        let width = ui.clip_rect().x_range();
                        let mut shapes = Vec::new();
                        let mut start = 0;
                        for row in &output.galley.rows {
                            let end = start + row.char_count_including_newline();
                            if start < active.end && end > active.start {
                                let rect = row.rect().translate(output.galley_pos.to_vec2());
                                let rect = egui::Rect::from_x_y_ranges(width, rect.y_range());
                                shapes.push(egui::Shape::rect_filled(
                                    rect,
                                    0,
                                    Color32::from_rgba_unmultiplied(76, 154, 255, 16),
                                ));
                                shapes.push(egui::Shape::rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::Pos2::new(
                                            output.response.rect.left() - 5.0,
                                            rect.top(),
                                        ),
                                        Vec2::new(2.0, rect.height()),
                                    ),
                                    0,
                                    theme::ACCENT.gamma_multiply(0.7),
                                ));
                            }
                            start = end;
                        }
                        ui.painter().set(highlight, egui::Shape::Vec(shapes));
                    }
                    output
                })
                .inner
            })
            .inner;
        let previous_selection = tab.selection;
        if let Some(range) = output.cursor_range {
            let byte = |index: usize| {
                tab.sql
                    .char_indices()
                    .nth(index)
                    .map_or(tab.sql.len(), |(offset, _)| offset)
            };
            tab.selection = (byte(range.secondary.index), byte(range.primary.index));
        } else if tab.selection.0.max(tab.selection.1) > tab.sql.len() {
            tab.selection = (tab.sql.len(), tab.sql.len());
        }
        // Open or refine keyword completion after typing an identifier.
        let (from, to) = tab.selection;
        if output.response.changed() && from == to {
            let start = tab.sql[..to]
                .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .map_or(0, |i| i + 1);
            let prefix = &tab.sql[start..to];
            let typed_letter = prefix
                .chars()
                .last()
                .is_some_and(|c| c.is_ascii_alphabetic());
            let items = if prefix.len() >= 2 && typed_letter {
                completions(engine, prefix)
            } else {
                Vec::new()
            };
            completion = (!items.is_empty()).then_some(Completion {
                tab: tab_id,
                start,
                items,
                selected: 0,
            });
        } else if tab.selection != previous_selection && !output.response.changed() {
            completion = None;
        }
        if let Some(open) = &completion {
            let ccursor = egui::text::CCursor::new(tab.sql[..to].chars().count());
            let anchor = output
                .galley
                .pos_from_cursor(ccursor)
                .translate(output.galley_pos.to_vec2());
            let mut picked = None;
            egui::Area::new(editor_id.with("completion"))
                .order(egui::Order::Foreground)
                .fixed_pos(anchor.left_bottom() + Vec2::new(0.0, 2.0))
                .show(ui.ctx(), |ui| {
                    Frame::new()
                        .fill(theme::POPOVER)
                        .stroke(Stroke::new(1.0, theme::BORDER_STRONG))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::same(4))
                        .show(ui, |ui| {
                            ui.set_min_width(160.0);
                            for (index, item) in open.items.iter().take(12).enumerate() {
                                let selected = index == open.selected;
                                let text =
                                    RichText::new(item).font(mono(12.5)).color(if selected {
                                        Color32::WHITE
                                    } else {
                                        theme::TEXT
                                    });
                                let button = egui::Button::new(text)
                                    .fill(if selected {
                                        theme::ACCENT_STRONG
                                    } else {
                                        Color32::TRANSPARENT
                                    })
                                    .stroke(Stroke::NONE)
                                    .min_size(Vec2::new(ui.available_width(), 22.0));
                                if ui.add(button).clicked() {
                                    picked = Some(index);
                                }
                            }
                        });
                });
            if let Some(index) = picked {
                let word = open.items[index].clone();
                tab.sql.replace_range(open.start..to, &word);
                let cursor = open.start + word.len();
                tab.selection = (cursor, cursor);
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), editor_id) {
                    let ccursor = egui::text::CCursor::new(tab.sql[..cursor].chars().count());
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::one(ccursor)));
                    state.store(ui.ctx(), editor_id);
                }
                ui.memory_mut(|m| m.request_focus(editor_id));
                completion = None;
            }
        }
        self.completion = completion;
        let tab = self.tabs.iter().find(|t| t.id == tab_id)?;
        if run_pressed {
            let (from, to) = tab.selection;
            return execution_target(engine, &tab.sql, from, to).map(|t| t.sql);
        }
        None
    }

    /// History for the tab's profile and database. Returns SQL the user picked.
    fn history_ui(&mut self, ui: &mut egui::Ui, profile: Uuid) -> Option<String> {
        let database = self
            .workspaces
            .get(&profile)
            .map(|w| w.profile.default_database.clone())
            .unwrap_or_default();
        let key = (profile, database.clone());
        if !self.histories.contains_key(&key)
            && !self
                .pending_requests
                .values()
                .any(|m| m.label == "Loading history")
            && self.workspaces.contains_key(&profile)
        {
            self.histories.insert(key.clone(), Vec::new());
            self.send(
                Command::History(profile, database),
                "Loading history",
                Some(profile),
            );
        }
        let entries = self.histories.get(&key).cloned().unwrap_or_default();
        Frame::new()
            .inner_margin(Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("History").strong().color(theme::TEXT_STRONG));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(entries.len().to_string())
                                .font(ui_font(11.0))
                                .color(theme::FAINT),
                        );
                    });
                });
            });
        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.cursor().top(),
            Stroke::new(1.0, theme::BORDER),
        );
        if entries.is_empty() {
            Frame::new().inner_margin(Margin::same(12)).show(ui, |ui| {
                ui.label(RichText::new("Run a query to start history.").color(theme::MUTED));
            });
            return None;
        }
        let mut picked = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for entry in entries.iter().take(100) {
                    let time = entry
                        .executed_at
                        .with_timezone(&chrono::Local)
                        .format("%-I:%M:%S %p")
                        .to_string();
                    let width = ui.available_width();
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::new(width, 50.0), Sense::click());
                    let response = response
                        .on_hover_text(format!("{} ms\n\n{}", entry.duration_ms, entry.sql));
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &entry.sql)
                    });
                    if response.hovered() {
                        ui.painter()
                            .rect_filled(rect, 0, Color32::from_white_alpha(8));
                    }
                    let (glyph, color) = if entry.success {
                        (Icon::Check, theme::SUCCESS)
                    } else {
                        (Icon::Alert, theme::DANGER)
                    };
                    icons::paint(
                        ui.painter(),
                        egui::Rect::from_center_size(
                            rect.left_top() + Vec2::new(20.0, 17.0),
                            Vec2::splat(12.0),
                        ),
                        glyph,
                        color,
                    );
                    let text_left = rect.left() + 34.0;
                    let mut job = LayoutJob::simple_singleline(
                        one_line(&entry.sql),
                        mono(12.0),
                        theme::SECONDARY,
                    );
                    job.wrap = egui::text::TextWrapping::truncate_at_width(
                        rect.right() - text_left - 10.0,
                    );
                    let sql = ui.fonts_mut(|fonts| fonts.layout_job(job));
                    ui.painter().galley(
                        egui::Pos2::new(text_left, rect.top() + 9.0),
                        sql,
                        theme::SECONDARY,
                    );
                    ui.painter().text(
                        egui::Pos2::new(text_left, rect.top() + 29.0),
                        egui::Align2::LEFT_TOP,
                        time,
                        ui_font(11.0),
                        theme::FAINT,
                    );
                    if response.clicked() {
                        picked = Some(entry.sql.clone());
                    }
                }
            });
        picked
    }

    fn query_result_ui(&mut self, ui: &mut egui::Ui, tab_id: u64, profile: Uuid) {
        let Some(result) = self.query_results.get(&tab_id) else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Results will appear here.").color(theme::MUTED));
            });
            return;
        };
        let embedded = self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .and_then(|t| t.embedded.clone());
        let duration = result.duration_ms;
        if embedded.is_some() {
            Frame::new()
                .inner_margin(Margin::symmetric(14, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "Table viewer · query completed in {duration} ms"
                            ))
                            .color(theme::MUTED),
                        );
                        chip(ui, "Editable table", theme::ACCENT_TEXT);
                    });
                });
            self.table_ui(ui, tab_id, profile, true);
            return;
        }
        Frame::new()
            .inner_margin(Margin::symmetric(14, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let affected = result
                        .affected_rows
                        .map_or_else(String::new, |n| format!(" · {n} affected"));
                    ui.label(
                        RichText::new(format!("{} rows{affected} · {duration} ms", result.row_count))
                            .color(theme::MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !result.columns.is_empty() {
                            chip(ui, "Read-only result", theme::MUTED).on_hover_text(
                                "This query does not resolve to one complete table, so DBM cannot safely map edits back to rows.",
                            );
                        }
                        if result.truncated {
                            chip(ui, "truncated", theme::MODIFIED);
                        }
                    });
                });
                for notice in &result.notices {
                    ui.label(RichText::new(notice).font(ui_font(11.5)).color(theme::MUTED));
                }
            });
        if !result.columns.is_empty() {
            Frame::new()
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(CornerRadius::same(10))
                .outer_margin(Margin {
                    left: 14,
                    right: 14,
                    top: 0,
                    bottom: 12,
                })
                .show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    result_grid(ui, tab_id, &result.columns, &result.rows);
                });
        }
    }
}

/// An icon button that can be disabled. Returns whether it was clicked.
fn icon_btn(
    ui: &mut egui::Ui,
    enabled: bool,
    icon: Icon,
    label: Option<&str>,
    tooltip: &str,
) -> bool {
    ui.add_enabled_ui(enabled, |ui| icons::button(ui, icon, label, tooltip))
        .inner
        .clicked()
}

fn editor_layout(engine: DatabaseEngine, text: &str) -> LayoutJob {
    let font = mono(13.0);
    let mut colors = vec![theme::TEXT; text.len()];
    for token in highlight(engine, text) {
        let color = match token.kind {
            TokenKind::Keyword => Color32::from_rgb(0xc6, 0x9c, 0xff),
            TokenKind::String => Color32::from_rgb(0x9f, 0xd8, 0x8a),
            TokenKind::Number => Color32::from_rgb(0xf0, 0xb1, 0x4c),
            TokenKind::Comment => theme::FAINT,
            TokenKind::QuotedIdentifier => theme::ACCENT_TEXT,
        };
        for slot in &mut colors[token.from..token.to] {
            *slot = color;
        }
    }
    let mut job = LayoutJob::default();
    let mut start = 0;
    for (index, _) in text
        .char_indices()
        .skip(1)
        .chain(std::iter::once((text.len(), ' ')))
    {
        if index < text.len() && colors[index] == colors[start] {
            continue;
        }
        job.append(
            &text[start..index],
            0.0,
            TextFormat::simple(font.clone(), colors[start]),
        );
        start = index;
    }
    if text.is_empty() {
        job.append("", 0.0, TextFormat::simple(font, theme::TEXT));
    }
    job
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---- table view -----------------------------------------------------------

impl Workbench {
    fn table_ui(&mut self, ui: &mut egui::Ui, tab_id: u64, profile_id: Uuid, embedded: bool) {
        let Some((_, schema, table)) = self.table_target(tab_id) else {
            return;
        };
        let cx = TableContext {
            tab_id,
            schema: &schema,
            table: &table,
            embedded,
            read_only: self.profile(profile_id).is_none_or(|p| p.read_only),
            saving: self.busy(RequestKind::Mutation, Some(tab_id)),
            exporting: self.busy(RequestKind::Export, Some(tab_id)),
        };
        let Some(mut state) = self.tables.remove(&tab_id) else {
            return;
        };
        let actions = table_view::show(ui, &cx, &mut state);
        self.tables.insert(tab_id, state);
        for action in actions {
            match action {
                TableAction::Reload => self.load_table(tab_id),
                TableAction::Save => self.save_changes(tab_id),
                TableAction::Copy(text, which) => {
                    ui.ctx().copy_text(text);
                    let notice = Message::notice(format!("Copied {which} as CSV."), self.now);
                    self.table_notice(tab_id, notice);
                }
                TableAction::Export => {
                    let total = self
                        .tables
                        .get(&tab_id)
                        .and_then(TableState::total_rows)
                        .unwrap_or(0);
                    if total > LARGE_EXPORT_WARNING_ROWS {
                        self.confirm_export = Some(tab_id);
                    } else {
                        self.start_export(tab_id);
                    }
                }
                TableAction::Error(message) => self.table_error(tab_id, message),
            }
        }
    }
}

// ---- dialogs --------------------------------------------------------------

fn modal<R>(
    ctx: &egui::Context,
    title: &str,
    width: f32,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    // Dim the app behind the dialog and swallow clicks on it.
    egui::Area::new(egui::Id::new(("modal-backdrop", title)))
        .order(egui::Order::Middle)
        .fixed_pos(egui::Pos2::ZERO)
        .show(ctx, |ui| {
            let screen = ctx.content_rect();
            ui.allocate_rect(screen, Sense::click());
            ui.painter()
                .rect_filled(screen, 0, Color32::from_black_alpha(120));
        });
    egui::Window::new(title)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .fixed_size([width, 0.0])
        .frame(
            Frame::new()
                .fill(theme::POPOVER)
                .stroke(Stroke::new(1.0, theme::BORDER_STRONG))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(Margin::same(18))
                .shadow(egui::Shadow {
                    offset: [0, 16],
                    blur: 40,
                    spread: 0,
                    color: Color32::from_black_alpha(140),
                }),
        )
        .show(ctx, add)
        .and_then(|response| response.inner)
}

fn dialog_title(ui: &mut egui::Ui, eyebrow_text: Option<&str>, title: &str) {
    if let Some(text) = eyebrow_text {
        ui.label(eyebrow(text));
    }
    ui.label(
        RichText::new(title)
            .size(17.0)
            .strong()
            .color(theme::TEXT_STRONG),
    );
    ui.add_space(10.0);
}

fn form_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(ui_font(12.0)).color(theme::MUTED));
}

impl Workbench {
    fn dialogs(&mut self, ctx: &egui::Context) {
        self.profile_dialog(ctx);
        if let Some(id) = self.confirm_delete {
            let name = self
                .profiles
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            let busy = !self.pending_requests.is_empty();
            let choice = modal(ctx, "Delete connection", 420.0, |ui| {
                dialog_title(ui, None, &format!("Delete connection “{name}”?"));
                ui.label(RichText::new("Saved password and query history for this profile will be removed. The database itself is not changed.").color(theme::SECONDARY));
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(!busy, egui::Button::new(RichText::new("Delete").color(Color32::WHITE)).fill(Color32::from_rgb(0xb4, 0x3c, 0x36))).clicked() {
                            return Some(true);
                        }
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            return Some(false);
                        }
                        None
                    })
                    .inner
                })
                .inner
            })
            .flatten();
            match choice {
                Some(true) if self.profile_is_clean(id) => {
                    self.send(Command::DeleteProfile(id), "Deleting connection", None)
                }
                Some(false) => self.confirm_delete = None,
                _ => {}
            }
        }
        if let Some(confirm) = &self.confirm_query {
            let preview: String = confirm.sql.chars().take(400).collect();
            let choice = modal(ctx, "Run destructive query", 480.0, |ui| {
                dialog_title(ui, None, "Run this query?");
                ui.label(
                    RichText::new("This query may change or remove many rows.")
                        .color(theme::SECONDARY),
                );
                ui.add_space(8.0);
                Frame::new()
                    .fill(theme::BG)
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.label(RichText::new(preview).font(mono(12.0)).color(theme::TEXT));
                    });
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Run anyway").color(Color32::WHITE))
                                .fill(Color32::from_rgb(0xb4, 0x3c, 0x36)),
                        )
                        .clicked()
                    {
                        return Some(true);
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        return Some(false);
                    }
                    None
                })
                .inner
            })
            .flatten();
            if let Some(run) = choice {
                if let Some(confirm) = self.confirm_query.take() {
                    if run {
                        self.run_query(confirm.tab, confirm.sql, confirm.refresh, true);
                    }
                }
            }
        }
        if let Some(tab_id) = self.confirm_export {
            let total = self
                .tables
                .get(&tab_id)
                .and_then(|s| s.page.as_ref())
                .and_then(|p| p.total_rows)
                .unwrap_or(0);
            let choice = modal(ctx, "Large export", 420.0, |ui| {
                dialog_title(ui, None, "Export a large table?");
                ui.label(
                    RichText::new(format!(
                        "This export contains {total} rows and may take a while. Continue?"
                    ))
                    .color(theme::SECONDARY),
                );
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(primary_button("Export")).clicked() {
                        return Some(true);
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        return Some(false);
                    }
                    None
                })
                .inner
            })
            .flatten();
            if let Some(export) = choice {
                self.confirm_export = None;
                if export {
                    self.start_export(tab_id);
                }
            }
        }
    }

    fn profile_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.profile_form.take() else {
            return;
        };
        let busy = self.busy(RequestKind::Form, None);
        let mut close = false;
        let mut test = false;
        let mut save = false;
        let mut delete = false;
        modal(ctx, "Connection", 560.0, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    dialog_title(
                        ui,
                        Some(&engine_label(form.engine).to_uppercase()),
                        if form.id.is_some() {
                            "Edit connection"
                        } else {
                            "New connection"
                        },
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if icon_btn(ui, true, Icon::Close, None, "Close") {
                        close = true;
                    }
                });
            });
            ui.add_enabled_ui(!busy, |ui| {
                form_label(ui, "Database engine");
                ui.horizontal(|ui| {
                    Frame::new()
                        .fill(theme::CONTROL)
                        .corner_radius(CornerRadius::same(7))
                        .inner_margin(Margin::same(2))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                for engine in [
                                    DatabaseEngine::Postgres,
                                    DatabaseEngine::Mysql,
                                    DatabaseEngine::Redis,
                                ] {
                                    let selected = form.engine == engine;
                                    let button = egui::Button::new(
                                        RichText::new(engine_label(engine)).color(if selected {
                                            theme::TEXT_STRONG
                                        } else {
                                            theme::MUTED
                                        }),
                                    )
                                    .fill(if selected {
                                        theme::CONTROL_ACTIVE
                                    } else {
                                        Color32::TRANSPARENT
                                    })
                                    .stroke(Stroke::NONE)
                                    .min_size(Vec2::new(120.0, 26.0));
                                    if ui.add(button).clicked() {
                                        form.set_engine(engine);
                                    }
                                }
                            });
                        });
                });
                ui.add_space(8.0);
                form_label(ui, "Connection URL");
                ui.horizontal(|ui| {
                    let placeholder = match form.engine {
                        DatabaseEngine::Postgres => "postgresql://user:password@host:5432/database",
                        DatabaseEngine::Mysql => "mysql://user:password@host:3306/database",
                        DatabaseEngine::Redis => "redis://default:password@host:6379/0",
                    };
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut form.url)
                            .password(true)
                            .hint_text(placeholder)
                            .desired_width(400.0),
                    );
                    let enter =
                        response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (ui
                        .add_enabled(!form.url.trim().is_empty(), egui::Button::new("Import URL"))
                        .clicked()
                        || enter)
                        && !form.url.trim().is_empty()
                    {
                        form.import_url();
                    }
                });
                ui.add_space(8.0);
                form_label(ui, "Name");
                ui.add(
                    egui::TextEdit::singleline(&mut form.name)
                        .hint_text(engine_preset_name(form.engine))
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(8.0);
                form_label(ui, "Connection color");
                ui.horizontal(|ui| {
                    for color in theme::CONNECTION_COLORS {
                        let (rect, response) =
                            ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                        ui.painter()
                            .circle_filled(rect.center(), 9.0, theme::parse_color(color));
                        if form.color.eq_ignore_ascii_case(color) {
                            ui.painter().circle_stroke(
                                rect.center(),
                                11.0,
                                Stroke::new(2.0, theme::TEXT_STRONG),
                            );
                        }
                        if response
                            .on_hover_text(format!("Use connection color {color}"))
                            .clicked()
                        {
                            form.color = color.into();
                        }
                    }
                    ui.add_space(8.0);
                    let mut custom = theme::parse_color(&form.color);
                    if ui
                        .color_edit_button_srgba(&mut custom)
                        .on_hover_text("Choose a custom color")
                        .changed()
                    {
                        form.color =
                            format!("#{:02x}{:02x}{:02x}", custom.r(), custom.g(), custom.b());
                    }
                    ui.label(RichText::new("Custom").color(theme::MUTED));
                });
                ui.add_space(8.0);
                let redis = form.engine == DatabaseEngine::Redis;
                let password_hint = if form.id.is_some() {
                    "Leave blank to keep saved password"
                } else {
                    "Stored in OS credential store"
                };
                ui.columns(2, |cols| {
                    form_label(&mut cols[0], "Host");
                    cols[0].add(
                        egui::TextEdit::singleline(&mut form.host).desired_width(f32::INFINITY),
                    );
                    form_label(&mut cols[1], "Port");
                    cols[1].add(
                        egui::DragValue::new(&mut form.port)
                            .range(1..=65535)
                            .speed(0.0),
                    );
                });
                ui.add_space(6.0);
                ui.columns(2, |cols| {
                    form_label(
                        &mut cols[0],
                        if redis {
                            "Username (ACL, optional)"
                        } else {
                            "Username"
                        },
                    );
                    cols[0].add(
                        egui::TextEdit::singleline(&mut form.username)
                            .hint_text(if redis { "default" } else { "" })
                            .desired_width(f32::INFINITY),
                    );
                    form_label(
                        &mut cols[1],
                        if redis { "Database index" } else { "Database" },
                    );
                    cols[1].add(
                        egui::TextEdit::singleline(&mut form.database)
                            .hint_text(if redis { "0" } else { "" })
                            .desired_width(f32::INFINITY),
                    );
                });
                ui.add_space(6.0);
                ui.columns(2, |cols| {
                    form_label(&mut cols[0], "Password");
                    cols[0].add(
                        egui::TextEdit::singleline(&mut form.password)
                            .password(true)
                            .hint_text(password_hint)
                            .desired_width(f32::INFINITY),
                    );
                    form_label(&mut cols[1], "TLS");
                    let width = cols[1].available_width();
                    egui::ComboBox::from_id_salt("tls")
                        .selected_text(match form.tls {
                            TlsMode::Preferred => "Preferred",
                            TlsMode::Required => "Required",
                            TlsMode::Disabled => "Disabled",
                        })
                        .width(width)
                        .show_ui(&mut cols[1], |ui| {
                            ui.selectable_value(&mut form.tls, TlsMode::Preferred, "Preferred");
                            ui.selectable_value(&mut form.tls, TlsMode::Required, "Required");
                            ui.selectable_value(&mut form.tls, TlsMode::Disabled, "Disabled");
                        });
                });
                ui.add_space(8.0);
                form_label(ui, "CA certificate path (optional)");
                ui.add(
                    egui::TextEdit::singleline(&mut form.ca_cert_path)
                        .hint_text("/path/to/root-ca.pem")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(8.0);
                ui.checkbox(
                    &mut form.read_only,
                    "Read-only profile (blocks GUI edits and mutations)",
                );
            });
            if let Some((kind, message)) = &form.feedback {
                ui.add_space(8.0);
                let color = match kind {
                    FeedbackKind::Info => theme::ACCENT_TEXT,
                    FeedbackKind::Success => theme::SUCCESS,
                    FeedbackKind::Error => theme::DANGER,
                };
                ui.label(RichText::new(message).color(color));
            }
            ui.add_space(6.0);
            ui.label(RichText::new("Passwords are stored in your operating system credential manager and are never written to DBM's profile database.").font(ui_font(11.5)).color(theme::FAINT));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if form.id.is_some()
                    && ui
                        .add_enabled(!busy, theme::danger_button("Delete"))
                        .clicked()
                {
                    delete = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if form.saving {
                        "Saving…"
                    } else {
                        "Save & connect"
                    };
                    if ui.add_enabled(!busy, primary_button(label)).clicked() {
                        save = true;
                    }
                    let testing = busy && !form.saving;
                    if ui
                        .add_enabled(
                            !busy,
                            egui::Button::new(if testing {
                                "Testing…"
                            } else {
                                "Test connection"
                            }),
                        )
                        .clicked()
                    {
                        test = true;
                    }
                });
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
        if test {
            form.feedback = None;
            form.saving = false;
            self.dispatch(
                Command::TestProfile(form.input()),
                "Testing connection",
                None,
                None,
                RequestKind::Form,
            );
        }
        if save && form.id.is_none_or(|id| self.profile_is_clean(id)) {
            form.saving = true;
            form.feedback = Some((
                FeedbackKind::Info,
                "Testing connection before saving…".into(),
            ));
            self.dispatch(
                Command::TestProfile(form.input()),
                "Testing connection",
                None,
                None,
                RequestKind::Form,
            );
        }
        if delete {
            if let Some(id) = form.id {
                if self.profile_is_clean(id) {
                    self.confirm_delete = Some(id);
                }
            }
        }
        if close && !busy {
            return;
        }
        self.profile_form = Some(form);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::table_view::pending_row;
    use crate::table_view::tests::page;
    use dbm_core::demo::DemoStore;

    fn app() -> Workbench {
        let mut app = Workbench::with_context(egui::Context::default(), true);
        app.pending_requests.clear();
        app
    }

    fn meta(tab: Option<u64>, kind: RequestKind) -> RequestMeta {
        RequestMeta {
            label: "test",
            tab,
            profile: Some(Uuid::from_u128(1)),
            kind,
            stale: false,
        }
    }

    #[test]
    fn replies_follow_initiating_tab_and_dirty_profiles_are_guarded() {
        let mut app = app();
        let id = Uuid::from_u128(1);
        app.open_query(id);
        let first = app.active_tab.unwrap();
        app.open_query(id);
        let second = app.active_tab.unwrap();
        assert_eq!(app.tabs[1].title, "Query 2");
        let result = QueryResponse {
            columns: vec![],
            rows: vec![vec![json!(37)]],
            row_count: 1,
            affected_rows: None,
            duration_ms: 0,
            truncated: false,
            notices: vec![],
        };
        app.apply(
            Payload::Query(result),
            &meta(Some(first), RequestKind::Query),
        );
        assert_eq!(app.query_results[&first].rows, vec![vec![json!(37)]]);
        assert!(!app.query_results.contains_key(&second));
        let mut table = TableState::default();
        table.page = Some(page());
        let row = page().rows[0].clone();
        table.pending.insert(0, pending_row(&page(), &row));
        app.tables.insert(first, table);
        assert!(!app.profile_is_clean(id));
        assert!(app.profile_is_clean(Uuid::from_u128(2)));
        app.pending_requests.clear();
        app.queued.clear();
        app.load_table(first);
        assert!(app.pending_requests.is_empty() && app.queued.is_empty());
        app.close_tab(first);
        assert!(
            app.tabs.iter().any(|t| t.id == first),
            "dirty tabs stay open"
        );
    }

    #[test]
    fn closing_a_profile_ignores_its_late_replies_only() {
        let mut app = app();
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        app.pending_requests.insert(
            RequestId(900),
            RequestMeta {
                profile: Some(a),
                ..meta(None, RequestKind::Other)
            },
        );
        app.pending_requests.insert(
            RequestId(901),
            RequestMeta {
                profile: Some(b),
                ..meta(None, RequestKind::Other)
            },
        );
        app.close_profile(a);
        assert!(app.pending_requests[&RequestId(900)].stale);
        assert!(!app.pending_requests[&RequestId(901)].stale);
    }

    #[test]
    fn full_table_select_opens_embedded_table_viewer() {
        let mut app = app();
        let id = Uuid::from_u128(1);
        app.schemas.insert(
            id,
            vec![SchemaNode {
                name: "public".into(),
                kind: "schema".into(),
                schema: Some("public".into()),
                table: None,
                children: vec![SchemaNode {
                    name: "customers".into(),
                    kind: "table".into(),
                    schema: Some("public".into()),
                    table: Some("customers".into()),
                    children: vec![],
                }],
            }],
        );
        app.open_query(id);
        let tab = app.active_tab.unwrap();
        app.tabs[0].last_executed = Some("select * from customers;".into());
        let response = QueryResponse {
            columns: vec![],
            rows: vec![],
            row_count: 0,
            affected_rows: None,
            duration_ms: 1,
            truncated: false,
            notices: vec![],
        };
        app.apply(
            Payload::Query(response),
            &meta(Some(tab), RequestKind::Query),
        );
        assert_eq!(
            app.tabs[0].embedded,
            Some(("public".into(), "customers".into()))
        );
        assert!(app.tables[&tab].loading);
        assert_eq!(
            app.table_target(tab),
            Some((id, "public".into(), "customers".into()))
        );
    }

    #[test]
    fn engine_switch_replaces_only_default_fields() {
        let mut form = ProfileForm::fresh();
        form.host = "db.internal".into();
        form.set_engine(DatabaseEngine::Redis);
        assert_eq!(
            (form.name.as_str(), form.port, form.database.as_str()),
            ("Local Redis", 6379, "0")
        );
        assert_eq!(form.host, "db.internal");
        form.port = 7000;
        form.set_engine(DatabaseEngine::Mysql);
        assert_eq!((form.port, form.username.as_str()), (7000, "root"));
        form.url = "postgres://me:secret@pg.local/app".into();
        form.import_url();
        assert_eq!(
            (form.engine, form.password.as_str()),
            (DatabaseEngine::Postgres, "secret")
        );
    }

    #[test]
    fn literal_null_text_and_tls_profile_settings_survive_edits() {
        let mut profile = DemoStore::new().profiles().remove(0);
        profile.ca_cert_path = Some("/tmp/test-ca.pem".into());
        let input = ProfileForm::from_profile(&profile).input();
        assert_eq!(input.tls_mode, TlsMode::Required);
        assert_eq!(input.ca_cert_path, profile.ca_cert_path);
        assert!(input.password.is_none());
    }

    #[test]
    fn editor_layout_covers_every_byte() {
        let text = "SELECT 'é' FROM t;\nSELECT 2;";
        let job = editor_layout(DatabaseEngine::Postgres, text);
        assert_eq!(job.text, text);
        assert!(
            job.sections.len() > 3,
            "keywords, literals, and text are separate sections"
        );
    }

    #[test]
    fn renderer_has_native_backend() {
        assert!(!wgpu::Instance::enabled_backend_features().is_empty());
    }
}
