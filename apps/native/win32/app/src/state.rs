//! The workbench's state and behavior, independent of drawing: the request
//! queue in front of the serialized database worker, replies routed to the
//! tab that asked, profiles, workspaces, tabs, queries, table loads, saves
//! and exports. Ported from the egui app's `workbench.rs` so both clients
//! follow the same rules (AGENTS.md "Product behavior to preserve").

use std::collections::{HashMap, HashSet, VecDeque};

use dbm_core::connection_url::{engine_defaults, parse_connection_url};
use dbm_core::models::{
    ConnectionProfile, DatabaseEngine, MutationBatch, QueryHistoryEntry, QueryRequest,
    QueryResponse, RowMutation, SaveProfileInput, SchemaNode, TablePageRequest, TlsMode,
    WorkspaceInfo,
};
use dbm_core::sql_text::{
    describe_schema_refresh, requires_confirmation, resolve_full_table_select, safe_file_name,
};
use dbm_desktop::updates;
use dbm_desktop::worker::{Command, ExportRequest, Payload, RequestId, Work, Worker};
use dbm_ui::Color32;
use dbm_ui::theme;
use uuid::Uuid;

use crate::messages::{self, Message};
use crate::table::{PENDING_REFRESH_ERROR, TableState};

pub const MAX_QUERY_ROWS: u32 = 10_000;
pub const LARGE_EXPORT_WARNING_ROWS: u64 = 100_000;
pub const PASSWORD_LOAD: &str = "Loading saved password";

pub fn engine_label(engine: DatabaseEngine) -> &'static str {
    match engine {
        DatabaseEngine::Postgres => "PostgreSQL",
        DatabaseEngine::Mysql => "MySQL",
        DatabaseEngine::Redis => "Redis",
    }
}

pub fn engine_preset_name(engine: DatabaseEngine) -> &'static str {
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
pub enum TabKind {
    Query,
    Table { schema: String, table: String },
}

#[derive(Clone, Debug)]
pub struct Tab {
    pub id: u64,
    pub profile_id: Uuid,
    pub title: String,
    pub kind: TabKind,
    pub sql: String,
    /// Editor selection as UTF-8 byte offsets (anchor, caret).
    pub selection: (usize, usize),
    pub last_executed: Option<String>,
    /// A `SELECT * FROM table` result shown in the editable table viewer.
    pub embedded: Option<(String, String)>,
    pub collapsed: bool,
}

impl Tab {
    /// A per-frame copy for drawing chrome, leaving out the editor text,
    /// which can be megabytes.
    pub fn without_sql(&self) -> Self {
        Self {
            id: self.id,
            profile_id: self.profile_id,
            title: self.title.clone(),
            kind: self.kind.clone(),
            sql: String::new(),
            selection: self.selection,
            last_executed: self.last_executed.clone(),
            embedded: self.embedded.clone(),
            collapsed: self.collapsed,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeedbackKind {
    Info,
    Success,
    Error,
}

#[derive(Clone)]
pub struct ProfileForm {
    pub id: Option<Uuid>,
    pub name: String,
    pub color: String,
    pub engine: DatabaseEngine,
    pub host: String,
    pub port: u16,
    pub port_text: String,
    pub username: String,
    pub database: String,
    pub tls: TlsMode,
    pub ca_cert_path: String,
    pub ssh: Option<dbm_core::models::SshConfig>,
    pub password: String,
    pub read_only: bool,
    pub open_on_startup: bool,
    pub url: String,
    pub importing_url: bool,
    pub show_url_password: bool,
    pub show_password: bool,
    pub password_loaded: bool,
    pub password_edited: bool,
    pub password_load_failed: bool,
    pub feedback: Option<(FeedbackKind, String)>,
    /// Save & connect tests first, then saves once the test succeeds.
    pub saving: bool,
}

impl ProfileForm {
    pub fn fresh() -> Self {
        let engine = DatabaseEngine::Postgres;
        let (port, database) = engine_defaults(engine);
        Self {
            id: None,
            name: engine_preset_name(engine).into(),
            color: theme::DEFAULT_CONNECTION_COLOR.into(),
            engine,
            host: "localhost".into(),
            port,
            port_text: port.to_string(),
            username: engine_preset_user(engine).into(),
            database: database.into(),
            tls: TlsMode::Preferred,
            ca_cert_path: String::new(),
            ssh: None,
            password: String::new(),
            read_only: false,
            open_on_startup: true,
            url: String::new(),
            importing_url: false,
            show_url_password: false,
            show_password: false,
            password_loaded: true,
            password_edited: false,
            password_load_failed: false,
            feedback: None,
            saving: false,
        }
    }

    pub fn from_profile(p: &ConnectionProfile) -> Self {
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
            port_text: p.port.to_string(),
            username: p.username.clone(),
            database: p.default_database.clone(),
            tls: p.tls_mode.clone(),
            ca_cert_path: p.ca_cert_path.clone().unwrap_or_default(),
            ssh: p.ssh.clone(),
            password: String::new(),
            read_only: p.read_only,
            open_on_startup: p.open_on_startup,
            url: String::new(),
            importing_url: false,
            show_url_password: false,
            show_password: false,
            password_loaded: false,
            password_edited: false,
            password_load_failed: false,
            feedback: None,
            saving: false,
        }
    }

    /// Switches engine and replaces fields still holding the previous
    /// engine's defaults.
    pub fn set_engine(&mut self, engine: DatabaseEngine) {
        let previous = self.engine;
        let (old_port, old_database) = engine_defaults(previous);
        let (port, database) = engine_defaults(engine);
        if self.name == engine_preset_name(previous) {
            self.name = engine_preset_name(engine).into();
        }
        if self.port == old_port {
            self.port = port;
            self.port_text = port.to_string();
        }
        if self.username == engine_preset_user(previous) {
            self.username = engine_preset_user(engine).into();
        }
        if self.database == old_database {
            self.database = database.into();
        }
        self.engine = engine;
    }

    pub fn import_url(&mut self) {
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
                self.port_text = imported.port.to_string();
                self.username = imported.username;
                self.database = imported.default_database;
                self.tls = imported.tls_mode;
                self.password = imported.password.unwrap_or_default();
                self.password_loaded = true;
                self.password_edited = true;
                self.password_load_failed = false;
                self.importing_url = false;
                self.url.clear();
                self.feedback = Some((
                    FeedbackKind::Info,
                    "Connection URL imported. Review the details, then save and connect.".into(),
                ));
            }
            Err(error) => self.feedback = Some((FeedbackKind::Error, error)),
        }
    }

    /// Keeps the numeric port in step with its text field.
    pub fn port_edited(&mut self) {
        self.port_text.retain(|c| c.is_ascii_digit());
        if let Ok(port) = self.port_text.parse::<u16>()
            && port > 0
        {
            self.port = port;
        }
    }

    pub fn input(&self) -> SaveProfileInput {
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
            open_on_startup: self.open_on_startup,
            password: if self.id.is_some() && !self.password_loaded {
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
pub enum RequestKind {
    Query,
    Table,
    Mutation,
    Export,
    Form,
    Other,
}

#[derive(Clone, Debug)]
pub struct RequestMeta {
    pub label: &'static str,
    pub tab: Option<u64>,
    pub profile: Option<Uuid>,
    pub kind: RequestKind,
    /// Set when the profile or tab closed; the reply is then ignored.
    pub stale: bool,
}

pub struct Queued {
    pub command: Command,
    pub meta: RequestMeta,
}

pub struct ConfirmQuery {
    pub tab: u64,
    pub sql: String,
    pub refresh: bool,
}

/// Keyword completion open in a query editor.
pub struct Completion {
    pub tab: u64,
    /// Byte offset where the completed word starts.
    pub start: usize,
    pub items: Vec<String>,
    pub selected: usize,
}

type ExportDestination = (u64, Option<std::path::PathBuf>);
type SavedResult = (u64, dbm_desktop::worker::ResultExport);

pub struct Toast {
    pub message: Message,
    /// A green dot when something changed, the accent dot otherwise.
    pub success: bool,
}

pub struct Workbench {
    pub wake: dbm_desktop::Wake,
    pub worker: Worker,
    pub demo: bool,
    pub profiles: Vec<ConnectionProfile>,
    pub workspaces: HashMap<Uuid, WorkspaceInfo>,
    pub schemas: HashMap<Uuid, Vec<SchemaNode>>,
    pub schema_filters: HashMap<Uuid, String>,
    pub collapsed_profiles: HashSet<Uuid>,
    /// Schema tree branches the person toggled away from their default.
    pub schema_toggled: HashSet<(Uuid, String)>,
    pub tabs: Vec<Tab>,
    pub active_tab: Option<u64>,
    pub active_profile: Option<Uuid>,
    pub query_results: HashMap<u64, QueryResponse>,
    pub histories: HashMap<(Uuid, String), Vec<QueryHistoryEntry>>,
    pub tables: HashMap<u64, TableState>,
    pub next_id: u64,
    pub pending_requests: HashMap<RequestId, RequestMeta>,
    pub queued: VecDeque<Queued>,
    pub toast: Option<Toast>,
    pub banner: Option<Message>,
    pub query_errors: HashMap<u64, Message>,
    pub profile_form: Option<ProfileForm>,
    pub confirm_delete: Option<Uuid>,
    pub confirm_query: Option<ConfirmQuery>,
    pub confirm_export: Option<u64>,
    pub choosing_export: HashMap<u64, (TablePageRequest, Vec<String>)>,
    export_paths: (
        std::sync::mpsc::Sender<ExportDestination>,
        std::sync::mpsc::Receiver<ExportDestination>,
    ),
    pub saving_results: HashSet<u64>,
    saved_results: (
        std::sync::mpsc::Sender<SavedResult>,
        std::sync::mpsc::Receiver<SavedResult>,
    ),
    pub updates: updates::Updates,
    pub confirm_update: Option<dbm_update::Available>,
    pub renaming: Option<(u64, String)>,
    pub sidebar_collapsed: bool,
    pub sidebar_width: f32,
    pub completion: Option<Completion>,
    /// Editor height per query tab, dragged at the editor/results boundary.
    pub editor_heights: HashMap<u64, f32>,
    pub history_widths: HashMap<u64, f32>,
    pub now: f64,
    pub window_width: f32,
}

impl Workbench {
    pub fn new(wake: dbm_desktop::Wake, demo: bool) -> Self {
        let worker = Worker::start(wake.clone(), demo);
        let updates = updates::Updates::new(wake.clone());
        let mut app = Self {
            wake,
            worker,
            demo,
            profiles: vec![],
            workspaces: HashMap::new(),
            schemas: HashMap::new(),
            schema_filters: HashMap::new(),
            collapsed_profiles: HashSet::new(),
            schema_toggled: HashSet::new(),
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
            choosing_export: HashMap::new(),
            export_paths: std::sync::mpsc::channel(),
            saving_results: HashSet::new(),
            saved_results: std::sync::mpsc::channel(),
            updates,
            confirm_update: None,
            renaming: None,
            sidebar_collapsed: false,
            sidebar_width: 260.0,
            completion: None,
            editor_heights: HashMap::new(),
            history_widths: HashMap::new(),
            now: 0.0,
            window_width: 1280.0,
        };
        app.send(Command::LoadProfiles, "Loading connections", None);
        app
    }

    // ---- requests -------------------------------------------------------

    pub fn send(&mut self, command: Command, label: &'static str, profile: Option<Uuid>) {
        self.dispatch(command, label, None, profile, RequestKind::Other);
    }

    /// Sends now when the worker is idle, otherwise queues. Replies are
    /// routed to the initiating tab, never the selected one.
    pub fn dispatch(
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

    pub fn busy(&self, kind: RequestKind, tab: Option<u64>) -> bool {
        self.metas()
            .any(|meta| meta.kind == kind && (tab.is_none() || meta.tab == tab))
    }

    /// Requests running and waiting.
    pub fn metas(&self) -> impl Iterator<Item = &RequestMeta> {
        self.pending_requests
            .values()
            .chain(self.queued.iter().map(|q| &q.meta))
    }

    pub fn drain(&mut self) {
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
        self.export_chosen();
        self.results_saved();
    }

    fn fail(&mut self, error: String, meta: &RequestMeta) {
        match meta.kind {
            RequestKind::Form => {
                if let Some(form) = &mut self.profile_form {
                    form.saving = false;
                    if meta.label == PASSWORD_LOAD {
                        if form.id == meta.profile && !form.password_edited {
                            form.password_load_failed = true;
                            form.feedback = Some((FeedbackKind::Error, error));
                        }
                        return;
                    }
                    form.feedback = Some((FeedbackKind::Error, error));
                    return;
                }
            }
            RequestKind::Table | RequestKind::Mutation | RequestKind::Export => {
                if let Some(tab) = meta.tab.filter(|tab| self.tables.contains_key(tab)) {
                    if meta.kind == RequestKind::Table
                        && let Some(state) = self.tables.get_mut(&tab)
                    {
                        state.in_flight = state.in_flight.saturating_sub(1);
                        if state.in_flight > 0 {
                            return;
                        }
                        state.requested = None;
                        state.restore_loaded();
                    }
                    self.table_error(tab, error);
                    return;
                }
                if meta.kind == RequestKind::Export
                    && let Some(tab) = meta.tab
                {
                    self.query_errors
                        .insert(tab, Message::error(error, self.now));
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

    pub fn show_error(&mut self, message: impl Into<String>) {
        self.banner = Some(Message::error(message, self.now));
    }

    pub fn show_toast(&mut self, message: impl Into<String>, success: bool) {
        self.toast = Some(Toast {
            message: Message::notice(message, self.now),
            success,
        });
    }

    pub fn table_error(&mut self, tab: u64, message: impl Into<String>) {
        let now = self.now;
        match self.tables.get_mut(&tab) {
            Some(state) => state.message = Some(Message::error(message, now)),
            None => self.show_error(message),
        }
    }

    pub fn table_notice(&mut self, tab: u64, message: Message) {
        if let Some(state) = self.tables.get_mut(&tab) {
            state.message = Some(message);
        }
    }

    fn apply(&mut self, payload: Payload, meta: &RequestMeta) {
        let target = meta.tab;
        match payload {
            Payload::ConnectionUrl(url) => {
                if let Some(form) = &mut self.profile_form
                    && meta.label == PASSWORD_LOAD
                    && form.id == meta.profile
                    && !form.password_edited
                {
                    match parse_connection_url(&url) {
                        Ok(imported) => {
                            form.password = imported.password.unwrap_or_default();
                            form.password_loaded = true;
                            form.password_load_failed = false;
                        }
                        Err(error) => {
                            form.password_load_failed = true;
                            form.feedback = Some((FeedbackKind::Error, error));
                        }
                    }
                }
            }
            Payload::Profiles(profiles) => {
                self.profiles = profiles;
                if !self.demo {
                    let startup: Vec<_> = self
                        .profiles
                        .iter()
                        .filter(|p| p.open_on_startup)
                        .map(|p| p.id)
                        .collect();
                    for id in startup {
                        self.send(Command::Connect(id), "Connecting", Some(id));
                    }
                }
            }
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
                let embedded = target.is_some_and(|tab| {
                    self.tabs
                        .iter()
                        .any(|t| t.id == tab && t.embedded.is_some())
                });
                if let Some(state) = target.and_then(|tab| self.tables.get_mut(&tab)) {
                    state.in_flight = state.in_flight.saturating_sub(1);
                    let latest = state.in_flight == 0;
                    if let Some(request) = state.requested.take_if(|_| latest) {
                        state.loaded(page, request);
                        // The paged table now owns the rows; keep the query's
                        // metadata for its status line only.
                        if embedded
                            && let Some(result) =
                                target.and_then(|tab| self.query_results.get_mut(&tab))
                        {
                            result.rows = Vec::new();
                        }
                    }
                }
            }
            Payload::Mutation(result) => {
                let Some(tab) = target else { return };
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
            Payload::Exported(path, rows) => {
                if let Some(tab) = target {
                    let message = Message::exported(path, rows, self.now);
                    if self.tables.contains_key(&tab) {
                        self.table_notice(tab, message);
                    } else {
                        self.query_errors.insert(tab, message);
                    }
                }
            }
        }
    }

    fn profile_saved(&mut self, profile: ConnectionProfile) {
        let previous = self.profiles.iter().find(|p| p.id == profile.id).cloned();
        let id = profile.id;
        if previous
            .as_ref()
            .is_some_and(|previous| connection_target_changed(previous, &profile))
        {
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
            self.close_table_tabs(id);
        }
        if let Some(profile) = self.profiles.iter_mut().find(|p| p.id == id) {
            *profile = workspace.profile.clone();
        }
        self.workspaces.insert(id, workspace);
        self.collapsed_profiles.remove(&id);
        if self.active_profile.is_none() || self.active_profile == Some(id) {
            self.activate_profile(id);
        } else if !self.tabs.iter().any(|tab| tab.profile_id == id) {
            self.open_query(id, false);
        }
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
            let state = self.new_table_state();
            self.tables.insert(tab_id, state);
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

    /// Asks for a profile's history the first time it is shown.
    pub fn ensure_history(&mut self, profile: Uuid) {
        let Some(database) = self
            .workspaces
            .get(&profile)
            .map(|w| w.profile.default_database.clone())
        else {
            return;
        };
        let key = (profile, database.clone());
        if !self.histories.contains_key(&key) && !self.metas().any(|m| m.label == "Loading history")
        {
            self.histories.insert(key, Vec::new());
            self.send(
                Command::History(profile, database),
                "Loading history",
                Some(profile),
            );
        }
    }

    // ---- tabs and profiles ----------------------------------------------

    pub fn profile(&self, id: Uuid) -> Option<&ConnectionProfile> {
        self.workspaces
            .get(&id)
            .map(|w| &w.profile)
            .or_else(|| self.profiles.iter().find(|p| p.id == id))
    }

    pub fn engine(&self, id: Uuid) -> DatabaseEngine {
        self.profile(id)
            .map_or(DatabaseEngine::Postgres, |p| p.engine)
    }

    pub fn profile_color(&self, id: Uuid) -> Color32 {
        self.profiles
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| p.color.as_deref())
            .map_or(theme::ACCENT, theme::parse_color)
    }

    /// Keeps or restores the profile's workbench: its current tab, its
    /// latest tab, or a new query tab.
    pub fn activate_profile(&mut self, profile: Uuid) {
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
        self.open_query(profile, true);
    }

    pub fn open_query(&mut self, profile: Uuid, activate: bool) {
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
        if activate {
            self.active_tab = Some(id);
            self.active_profile = Some(profile);
        }
    }

    pub fn open_table(&mut self, profile: Uuid, schema: String, table: String) {
        let kind = TabKind::Table { schema, table };
        if let Some(existing) = self
            .tabs
            .iter_mut()
            .find(|t| t.profile_id == profile && t.kind == kind)
        {
            existing.collapsed = false;
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
        let state = self.new_table_state();
        self.tables.insert(id, state);
        self.active_tab = Some(id);
        self.active_profile = Some(profile);
        self.load_table(id);
    }

    /// A fresh table view; the inspector starts open only on windows at
    /// least 1280 DIPs wide, as on the desktop.
    pub fn new_table_state(&self) -> TableState {
        TableState {
            inspector_open: self.window_width >= 1280.0,
            ..TableState::default()
        }
    }

    pub fn tab_dirty(&self, tab: u64) -> bool {
        self.tables.get(&tab).is_some_and(TableState::dirty)
    }

    pub fn select_tab(&mut self, tab_id: u64) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            tab.collapsed = false;
            self.active_tab = Some(tab_id);
            self.active_profile = Some(tab.profile_id);
        }
    }

    /// Shrinks a tab and, when it was active, selects its neighbour.
    pub fn collapse_tab(&mut self, tab_id: u64) {
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

    pub fn close_tab(&mut self, tab_id: u64) {
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
        self.choosing_export.remove(&tab_id);
        if self.renaming.as_ref().is_some_and(|(id, _)| *id == tab_id) {
            self.renaming = None;
        }
        if self.completion.as_ref().is_some_and(|c| c.tab == tab_id) {
            self.completion = None;
        }
        if self.confirm_export == Some(tab_id) {
            self.confirm_export = None;
        }
        if self.confirm_query.as_ref().is_some_and(|c| c.tab == tab_id) {
            self.confirm_query = None;
        }
        for meta in self.pending_requests.values_mut() {
            if meta.tab == Some(tab_id) {
                meta.stale = true;
            }
        }
        self.queued.retain(|q| q.meta.tab != Some(tab_id));
        if self.active_tab == Some(tab_id) {
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
                self.query_results.remove(&tab.id);
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
        for tab in self.tabs.iter().filter(|tab| tab.profile_id == profile) {
            if let Some(state) = self.tables.get_mut(&tab.id)
                && state.in_flight > 0
            {
                state.in_flight = 0;
                state.requested = None;
                state.restore_loaded();
            }
        }
    }

    pub fn close_profile(&mut self, id: Uuid) {
        let removed: Vec<_> = self
            .tabs
            .iter()
            .filter(|tab| tab.profile_id == id)
            .map(|tab| tab.id)
            .collect();
        for tab in removed {
            self.tables.remove(&tab);
            self.query_errors.remove(&tab);
            self.query_results.remove(&tab);
        }
        self.workspaces.remove(&id);
        self.schemas.remove(&id);
        self.schema_filters.remove(&id);
        self.histories.retain(|(profile, _), _| *profile != id);
        self.collapsed_profiles.remove(&id);
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

    pub fn profile_is_clean(&mut self, profile: Uuid) -> bool {
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

    /// Opens the editor for a saved profile and loads its password.
    pub fn edit_profile(&mut self, id: Uuid) {
        let Some(current) = self.profile(id).cloned() else {
            return;
        };
        let form = ProfileForm::from_profile(&current);
        let input = form.input();
        self.profile_form = Some(form);
        self.dispatch(
            Command::ConnectionUrl(input, true),
            PASSWORD_LOAD,
            None,
            Some(id),
            RequestKind::Form,
        );
    }

    /// Selecting a profile: restore its workbench, or connect.
    pub fn choose_profile(&mut self, id: Uuid) {
        if self.workspaces.contains_key(&id) {
            self.activate_profile(id);
        } else {
            self.active_profile = Some(id);
            if !self.connecting(id) {
                self.send(Command::Connect(id), "Connecting", Some(id));
            }
        }
    }

    pub fn connecting(&self, id: Uuid) -> bool {
        self.metas().any(|m| {
            m.profile == Some(id) && (m.label == "Connecting" || m.label == "Switching database")
        })
    }

    /// The table a tab edits: its own, or the one its last `SELECT *` named.
    pub fn table_target(&self, tab_id: u64) -> Option<(Uuid, String, String)> {
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

    pub fn load_table(&mut self, tab_id: u64) {
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
        // Paging, filtering or sorting replaces a stale notice.
        if state
            .message
            .as_ref()
            .is_some_and(|m| m.kind == messages::Kind::Notice && m.export.is_none())
        {
            state.message = None;
        }
        let request = state.request(profile, &schema, &table);
        state.requested = Some(request.clone());
        state.in_flight += 1;
        self.dispatch(
            Command::Table(request),
            "Loading rows",
            Some(tab_id),
            Some(profile),
            RequestKind::Table,
        );
    }

    pub fn run_query(&mut self, tab_id: u64, sql: String, refresh: bool, confirmed: bool) {
        let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else {
            return;
        };
        let profile = tab.profile_id;
        let sql = sql.trim().to_owned();
        if sql.is_empty() || self.busy(RequestKind::Query, Some(tab_id)) || self.exporting(tab_id) {
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

    pub fn mutation_batch(&self, tab_id: u64) -> Option<MutationBatch> {
        let (profile_id, schema, table) = self.table_target(tab_id)?;
        let state = self.tables.get(&tab_id)?;
        Some(MutationBatch {
            profile_id,
            database: self
                .workspaces
                .get(&profile_id)?
                .profile
                .default_database
                .clone(),
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

    pub fn save_changes(&mut self, tab_id: u64) {
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

    /// Export all: warns before very large tables, then asks for a file.
    pub fn request_export(&mut self, tab_id: u64) {
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

    /// Asks for a destination without blocking; `export_chosen` starts the
    /// export once a file is picked.
    pub fn start_export(&mut self, tab_id: u64) {
        if self.choosing_export.contains_key(&tab_id) {
            return;
        }
        let Some((profile, schema, table)) = self.table_target(tab_id) else {
            return;
        };
        let Some(state) = self.tables.get(&tab_id) else {
            return;
        };
        let (Some(page), Some(loaded)) = (&state.page, &state.loaded) else {
            return;
        };
        let request = TablePageRequest {
            profile_id: profile,
            ..loaded.clone()
        };
        let columns = page
            .metadata
            .columns
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let file_name = format!("{}.{}.csv", safe_file_name(&schema), safe_file_name(&table));
        let done = self.export_paths.0.clone();
        match crate::platform::pick_export_path(file_name, self.wake.clone(), move |path| {
            let _ = done.send((tab_id, path));
        }) {
            Ok(()) => {
                self.choosing_export.insert(tab_id, (request, columns));
            }
            Err(error) => {
                self.table_error(tab_id, format!("Couldn't open the save dialog: {error}"))
            }
        }
    }

    /// Saves the displayed rows of a query result as CSV.
    pub fn save_result(&mut self, tab_id: u64) {
        let Some(result) = self.query_results.get(&tab_id) else {
            return;
        };
        let title = self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .map_or("query", |t| t.title.as_str());
        let file_name = format!("{}.csv", safe_file_name(title));
        let columns = result.columns.iter().map(|c| c.name.clone()).collect();
        let rows = result.rows.clone();
        self.query_errors.remove(&tab_id);
        let done = self.saved_results.0.clone();
        match crate::platform::save_result_csv(
            file_name,
            columns,
            rows,
            self.wake.clone(),
            move |outcome| {
                let _ = done.send((tab_id, outcome));
            },
        ) {
            Ok(()) => {
                self.saving_results.insert(tab_id);
            }
            Err(error) => {
                let message = format!("Couldn't open the save dialog: {error}");
                self.query_errors
                    .insert(tab_id, Message::error(message, self.now));
            }
        }
    }

    pub fn exporting(&self, tab_id: u64) -> bool {
        self.busy(RequestKind::Export, Some(tab_id))
            || self.choosing_export.contains_key(&tab_id)
            || self.saving_results.contains(&tab_id)
    }

    fn results_saved(&mut self) {
        while let Ok((tab_id, outcome)) = self.saved_results.1.try_recv() {
            self.saving_results.remove(&tab_id);
            if !self.tabs.iter().any(|t| t.id == tab_id) {
                continue;
            }
            let message = match outcome {
                Ok(Some((path, rows))) => Message::exported(path, rows, self.now),
                Ok(None) => Message::notice("Export canceled.", self.now),
                Err(error) => Message::error(error, self.now),
            };
            self.query_errors.insert(tab_id, message);
        }
    }

    fn export_chosen(&mut self) {
        while let Ok((tab_id, path)) = self.export_paths.1.try_recv() {
            let Some((page, columns)) = self.choosing_export.remove(&tab_id) else {
                continue;
            };
            let Some(path) = path else {
                self.table_notice(tab_id, Message::notice("Export canceled.", self.now));
                continue;
            };
            if !self.tables.contains_key(&tab_id) {
                continue;
            }
            let profile = page.profile_id;
            let request = ExportRequest {
                page,
                columns,
                path,
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

    /// The label of the first request waiting for `tab`.
    pub fn pending_label(&self, tab: u64) -> Option<&'static str> {
        self.metas().find(|m| m.tab == Some(tab)).map(|m| m.label)
    }

    /// Why the window can't close yet, if anything.
    pub fn close_blocker(&self) -> Option<&'static str> {
        let dirty = self.tabs.iter().any(|tab| self.tab_dirty(tab.id));
        // Reads can be abandoned; a save or export stopped halfway can't.
        let writing =
            self.busy(RequestKind::Mutation, None) || self.busy(RequestKind::Export, None);
        if dirty {
            Some("Save or discard staged changes before closing Anybase.")
        } else if writing {
            Some("Wait for the save or export to finish before closing Anybase.")
        } else {
            None
        }
    }

    pub fn modal_open(&self) -> bool {
        self.profile_form.is_some()
            || self.confirm_delete.is_some()
            || self.confirm_query.is_some()
            || self.confirm_update.is_some()
            || self.updates.failure_open
            || !matches!(self.updates.whats_new, updates::WhatsNew::Closed)
            || self.confirm_export.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{pending_row, tests::page};
    use dbm_core::demo::DemoStore;

    fn app() -> Workbench {
        let mut app = Workbench::new(std::sync::Arc::new(|| {}), true);
        app.pending_requests.clear();
        app
    }

    fn meta(tab: Option<u64>, kind: RequestKind) -> RequestMeta {
        RequestMeta {
            label: "test",
            tab,
            profile: None,
            kind,
            stale: false,
        }
    }

    #[test]
    fn replies_follow_initiating_tab_and_dirty_profiles_are_guarded() {
        let mut app = app();
        let profile = Uuid::from_u128(1);
        app.open_query(profile, true);
        let first = app.active_tab.unwrap();
        app.open_query(profile, true);
        let second = app.active_tab.unwrap();
        app.fail("boom".into(), &meta(Some(first), RequestKind::Query));
        assert!(app.query_errors.contains_key(&first));
        assert!(!app.query_errors.contains_key(&second));
        let mut state = TableState::default();
        let p = page();
        state.pending.insert(0, pending_row(&p, &p.rows[0]));
        app.tables.insert(second, state);
        assert!(!app.profile_is_clean(profile));
        app.close_tab(second);
        assert!(
            app.tabs.iter().any(|t| t.id == second),
            "dirty tabs stay open"
        );
    }

    #[test]
    fn only_the_newest_table_load_is_shown_and_dropped_loads_never_stick() {
        let mut app = app();
        let profile = Uuid::from_u128(2);
        app.open_table(profile, "public".into(), "customers".into());
        let tab = app.active_tab.unwrap();
        app.load_table(tab);
        assert_eq!(app.tables[&tab].in_flight, 2);
        let older = page();
        app.apply(Payload::Table(older), &meta(Some(tab), RequestKind::Table));
        assert!(app.tables[&tab].loading, "the first reply is stale");
        app.fail("gone".into(), &meta(Some(tab), RequestKind::Table));
        assert!(!app.tables[&tab].loading && app.tables[&tab].requested.is_none());
    }

    #[test]
    fn closing_a_profile_closes_its_tabs_and_ignores_its_late_replies() {
        let mut app = app();
        let (a, b) = (Uuid::from_u128(3), Uuid::from_u128(4));
        app.open_query(a, true);
        app.open_query(b, true);
        app.dispatch(
            Command::Schema(a),
            "Loading schema",
            None,
            Some(a),
            RequestKind::Other,
        );
        app.close_profile(a);
        assert!(app.tabs.iter().all(|t| t.profile_id == b));
        assert!(
            app.pending_requests
                .values()
                .all(|m| m.profile != Some(a) || m.stale)
        );
    }

    #[test]
    fn full_table_select_opens_the_embedded_table_viewer() {
        let mut app = app();
        let mut store = DemoStore::new();
        let profile = store.profiles()[0].id;
        let tree = store.schema_tree(profile);
        let (schema, table) = tree
            .iter()
            .flat_map(|n| n.children.iter())
            .find_map(|n| n.schema.clone().zip(n.table.clone()))
            .unwrap();
        app.schemas.insert(profile, tree);
        app.open_query(profile, true);
        let tab = app.active_tab.unwrap();
        let sql = format!("SELECT * FROM {schema}.{table}");
        app.tabs[0].last_executed = Some(sql.clone());
        let result = store.run_query(&QueryRequest {
            profile_id: profile,
            sql,
            max_rows: None,
        });
        app.query_finished(Some(tab), result);
        assert_eq!(app.tabs[0].embedded, Some((schema, table)));
        assert!(app.tables.contains_key(&tab));
    }

    #[test]
    fn engine_switch_replaces_only_default_fields() {
        let mut form = ProfileForm::fresh();
        form.host = "db.internal".into();
        form.set_engine(DatabaseEngine::Redis);
        assert_eq!(form.name, "Local Redis");
        assert_eq!(form.username, "default");
        assert_eq!(form.host, "db.internal");
        assert_eq!(form.port_text, form.port.to_string());
        form.username = "admin".into();
        form.set_engine(DatabaseEngine::Mysql);
        assert_eq!(form.username, "admin");
    }

    #[test]
    fn saved_passwords_are_never_sent_until_loaded() {
        let p = DemoStore::new().profiles()[0].clone();
        let mut form = ProfileForm::from_profile(&p);
        assert_eq!(form.input().password, None);
        form.password_loaded = true;
        assert_eq!(form.input().password, Some(String::new()));
    }

    #[test]
    fn startup_opens_only_enabled_profiles_and_demo_stays_manual() {
        let mut app = app();
        let profiles = DemoStore::new().profiles();
        app.apply(Payload::Profiles(profiles), &meta(None, RequestKind::Other));
        assert!(app.pending_requests.is_empty() && app.queued.is_empty());
        app.demo = false;
        let mut profiles = DemoStore::new().profiles();
        for (i, p) in profiles.iter_mut().enumerate() {
            p.open_on_startup = i == 0;
        }
        app.apply(Payload::Profiles(profiles), &meta(None, RequestKind::Other));
        assert_eq!(app.metas().filter(|m| m.label == "Connecting").count(), 1);
    }
}
