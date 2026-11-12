//! Everything the GTK app knows, without widgets: profiles, workspaces, tabs,
//! requests to the serialized worker and their replies, the connection form,
//! table paging and staged edits, and message surfaces. It follows the egui
//! app's `workbench.rs` and `table_view.rs` rule for rule; the widgets in the
//! other modules render it and turn input into the methods below.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use dbm_core::cell_values::parse_cell_input;
use dbm_core::connection_url::{engine_defaults, parse_connection_url};
use dbm_core::models::{
    ConnectionProfile, DatabaseEngine, FilterCondition, FilterOperator, MutationBatch, OrderSpec,
    QueryHistoryEntry, QueryRequest, QueryResponse, RowMutation, SaveProfileInput, SchemaNode,
    TablePage, TablePageRequest, TlsMode, WorkspaceInfo,
};
use dbm_core::sql_text::{
    csv_document, describe_schema_refresh, requires_confirmation, resolve_full_table_select,
    safe_file_name,
};
use dbm_desktop::updates;
use dbm_desktop::worker::{Command, ExportRequest, Payload, RequestId, Work, Worker};
use serde_json::Value;
use uuid::Uuid;

pub const MAX_QUERY_ROWS: u32 = 10_000;
pub const MAX_PREVIEW_ROWS: u32 = 200;
pub const LARGE_EXPORT_WARNING_ROWS: u64 = 100_000;
pub const PASSWORD_LOAD: &str = "Loading saved password";
pub const PENDING_EXPORT_ERROR: &str = "Save or discard pending row changes before exporting.";
pub const PENDING_REFRESH_ERROR: &str = "Save or discard pending row changes before refreshing.";
pub const ERROR_SECONDS: f64 = 10.0;
pub const NOTICE_SECONDS: f64 = 6.0;
/// Connection colors offered in the profile editor.
pub const CONNECTION_COLORS: [&str; 8] = [
    "#4c9aff", "#ff9f43", "#3dd6c6", "#b48cff", "#ff6b8a", "#7ed957", "#f0b14c", "#8e8e93",
];
pub const DEFAULT_CONNECTION_COLOR: &str = "#4c9aff";

pub const FILTER_OPERATORS: [(FilterOperator, &str); 13] = [
    (FilterOperator::Equals, "Equals"),
    (FilterOperator::NotEquals, "Does not equal"),
    (FilterOperator::Contains, "Contains"),
    (FilterOperator::StartsWith, "Starts with"),
    (FilterOperator::EndsWith, "Ends with"),
    (FilterOperator::GreaterThan, "Greater than"),
    (FilterOperator::GreaterThanOrEqual, "Greater than or equal"),
    (FilterOperator::LessThan, "Less than"),
    (FilterOperator::LessThanOrEqual, "Less than or equal"),
    (FilterOperator::In, "In list"),
    (FilterOperator::NotIn, "Not in list"),
    (FilterOperator::IsNull, "Is null"),
    (FilterOperator::IsNotNull, "Is not null"),
];

pub fn filter_needs_value(operator: FilterOperator) -> bool {
    !matches!(operator, FilterOperator::IsNull | FilterOperator::IsNotNull)
}

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

/// A count with thousands separators, like `toLocaleString()` in en-US.
pub fn count(value: impl Into<u64>) -> String {
    let digits = value.into().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// ---- messages ------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Error,
    Notice,
}

/// An error strip, inline message, or toast. Errors expire after 10 seconds
/// and notices after 6; an export result stays until dismissed.
#[derive(Clone, Debug)]
pub struct Message {
    pub text: String,
    pub kind: Kind,
    pub shown_at: f64,
    /// A finished export: the file and how many rows it holds.
    pub export: Option<(PathBuf, u64)>,
}

impl Message {
    pub fn error(text: impl Into<String>, now: f64) -> Self {
        Self {
            text: text.into(),
            kind: Kind::Error,
            shown_at: now,
            export: None,
        }
    }

    pub fn notice(text: impl Into<String>, now: f64) -> Self {
        Self {
            kind: Kind::Notice,
            ..Self::error(text, now)
        }
    }

    pub fn exported(path: PathBuf, rows: u64, now: f64) -> Self {
        Self {
            export: Some((path, rows)),
            ..Self::notice("", now)
        }
    }

    /// When it disappears, if ever.
    pub fn expires_at(&self) -> Option<f64> {
        match (self.export.is_some(), self.kind) {
            (true, _) => None,
            (false, Kind::Error) => Some(self.shown_at + ERROR_SECONDS),
            (false, Kind::Notice) => Some(self.shown_at + NOTICE_SECONDS),
        }
    }

    pub fn expired(&self, now: f64) -> bool {
        self.expires_at().is_some_and(|at| now >= at)
    }
}

/// Bottom-right toast, used for schema refresh summaries.
pub struct Toast {
    pub message: Message,
    /// A green dot when something changed, the accent dot otherwise.
    pub success: bool,
}

// ---- tabs and forms ------------------------------------------------------

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
    /// The editor's text as of its last change; the text view owns edits.
    pub sql: String,
    /// Editor selection as UTF-8 byte offsets; equal ends are a cursor.
    pub selection: (usize, usize),
    pub last_executed: Option<String>,
    /// A `SELECT * FROM table` result shown in the editable table viewer.
    pub embedded: Option<(String, String)>,
    /// Shrunk to a narrow pill in the tab strip until selected again.
    pub collapsed: bool,
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
    /// Bumped when the form changes other than by typing, so the dialog
    /// rewrites its fields.
    pub revision: u64,
}

impl ProfileForm {
    pub fn fresh() -> Self {
        let engine = DatabaseEngine::Postgres;
        let (port, database) = engine_defaults(engine);
        Self {
            id: None,
            name: engine_preset_name(engine).into(),
            color: DEFAULT_CONNECTION_COLOR.into(),
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
            revision: 0,
        }
    }

    pub fn from_profile(p: &ConnectionProfile) -> Self {
        Self {
            id: Some(p.id),
            name: p.name.clone(),
            color: p
                .color
                .clone()
                .unwrap_or_else(|| DEFAULT_CONNECTION_COLOR.into()),
            engine: p.engine,
            host: p.host.clone(),
            port: p.port,
            username: p.username.clone(),
            database: p.default_database.clone(),
            tls: p.tls_mode.clone(),
            ca_cert_path: p.ca_cert_path.clone().unwrap_or_default(),
            ssh: p.ssh.clone(),
            read_only: p.read_only,
            open_on_startup: p.open_on_startup,
            password_loaded: false,
            ..Self::fresh()
        }
    }

    /// Switches engine and replaces fields that still hold the previous
    /// engine's defaults, as the desktop editor does.
    pub fn set_engine(&mut self, engine: DatabaseEngine) {
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
        self.revision += 1;
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
        self.revision += 1;
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

// ---- table state ---------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct PendingRow {
    pub original: Vec<Value>,
    pub changes: Vec<Value>,
    pub primary_key: Vec<Value>,
    pub xmin: Option<String>,
    pub deleted: bool,
}

#[derive(Clone, Debug)]
pub struct FilterDraft {
    pub column: String,
    pub operator: FilterOperator,
    pub value: String,
}

impl FilterDraft {
    pub fn new(column: &str) -> Self {
        Self {
            column: column.to_owned(),
            operator: FilterOperator::Contains,
            value: String::new(),
        }
    }
}

pub struct TableState {
    pub page: Option<Arc<TablePage>>,
    /// The request that produced `page`, restored when a later load fails.
    pub loaded: Option<TablePageRequest>,
    /// The request in flight, recorded as `loaded` when its page arrives.
    pub requested: Option<TablePageRequest>,
    /// Loads sent and not yet answered; only the last belongs to `requested`.
    pub in_flight: u32,
    pub loading: bool,
    /// When the current load started, for the delayed "Refreshing…" overlay.
    pub loading_since: Option<f64>,
    /// Inline error, notice, or export result under the filters.
    pub message: Option<Message>,
    /// Rows written so far by a running export.
    pub export_progress: Option<Arc<AtomicU64>>,
    pub page_index: u32,
    pub limit: u32,
    pub filters: Vec<FilterDraft>,
    pub applied: Vec<FilterCondition>,
    pub order: Option<OrderSpec>,
    pub selected: BTreeSet<usize>,
    pub pending: BTreeMap<usize, PendingRow>,
    /// The cell being edited in the grid.
    pub editing: Option<(usize, usize)>,
    pub inspector_open: bool,
    /// Column indexes collapsed to a narrow strip.
    pub collapsed_columns: BTreeSet<usize>,
    /// Bumped whenever rows, staged edits, or the selection change, so the
    /// grid knows to redraw.
    pub version: u64,
}

impl Default for TableState {
    fn default() -> Self {
        Self {
            page: None,
            loaded: None,
            requested: None,
            in_flight: 0,
            loading: false,
            loading_since: None,
            message: None,
            export_progress: None,
            page_index: 0,
            limit: MAX_PREVIEW_ROWS,
            filters: vec![],
            applied: vec![],
            order: None,
            selected: BTreeSet::new(),
            pending: BTreeMap::new(),
            editing: None,
            inspector_open: true,
            collapsed_columns: BTreeSet::new(),
            version: 0,
        }
    }
}

impl TableState {
    pub fn request(&self, profile_id: Uuid, schema: &str, table: &str) -> TablePageRequest {
        TablePageRequest {
            profile_id,
            schema: schema.into(),
            table: table.into(),
            offset: self.page_index * self.limit,
            limit: self.limit,
            filters: self.applied.clone(),
            order_by: self.order.clone(),
            include_total: Some(true),
        }
    }

    /// A page arrived for `request`.
    pub fn loaded(&mut self, page: TablePage, request: TablePageRequest) {
        self.loading = false;
        self.page_index = page.offset / page.limit.max(1);
        if self.filters.is_empty()
            && let Some(first) = page.metadata.columns.first()
        {
            self.filters.push(FilterDraft::new(&first.name));
        }
        self.loaded = Some(request);
        self.selected.clear();
        self.editing = None;
        self.page = Some(Arc::new(page));
        self.version += 1;
    }

    /// A load failed: return the controls to the page still on screen.
    pub fn restore_loaded(&mut self) {
        self.loading = false;
        if let Some(request) = &self.loaded {
            self.page_index = request.offset / request.limit.max(1);
            self.limit = request.limit;
            self.applied.clone_from(&request.filters);
            self.order.clone_from(&request.order_by);
        } else {
            self.page_index = 0;
        }
    }

    pub fn effective_order(&self) -> Option<OrderSpec> {
        self.order.clone().or_else(|| {
            let page = self.page.as_ref()?;
            Some(OrderSpec {
                column: page.metadata.primary_key.first()?.clone(),
                descending: false,
            })
        })
    }

    pub fn dirty(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn total_rows(&self) -> Option<u64> {
        self.page.as_ref().and_then(|p| p.total_rows)
    }

    /// Rows shown on the page, with staged edits applied, as CSV.
    pub fn csv(&self, rows: impl Iterator<Item = usize>) -> String {
        let Some(page) = &self.page else {
            return String::new();
        };
        let columns: Vec<String> = page
            .metadata
            .columns
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let values: Vec<Vec<Value>> = rows
            .filter_map(|i| {
                let row = page.rows.get(i)?;
                Some(
                    self.pending
                        .get(&i)
                        .map_or_else(|| row.clone(), |p| p.changes.clone()),
                )
            })
            .collect();
        csv_document(&columns, &values)
    }

    pub fn copyable_rows(&self) -> Vec<usize> {
        let count = self.page.as_ref().map_or(0, |p| p.rows.len());
        (0..count)
            .filter(|i| !self.pending.get(i).is_some_and(|p| p.deleted))
            .collect()
    }

    /// The row's values with staged edits applied.
    pub fn values(&self, row: usize) -> Option<&[Value]> {
        let page = self.page.as_ref()?;
        self.pending.get(&row).map_or_else(
            || page.rows.get(row).map(Vec::as_slice),
            |p| Some(&p.changes),
        )
    }

    /// Whether filters, beyond one empty draft, are set.
    pub fn has_filters(&self) -> bool {
        !self.applied.is_empty()
            || self.filters.len() > 1
            || self
                .filters
                .iter()
                .any(|f| !filter_needs_value(f.operator) || !f.value.trim().is_empty())
    }

    /// Applies the filter drafts that are complete.
    pub fn apply_filters(&mut self) {
        self.applied = self
            .filters
            .iter()
            .filter(|f| !f.column.is_empty())
            .filter(|f| !filter_needs_value(f.operator) || !f.value.trim().is_empty())
            .map(|f| FilterCondition {
                column: f.column.clone(),
                operator: f.operator,
                value: filter_needs_value(f.operator).then(|| f.value.trim().to_owned()),
            })
            .collect();
        self.page_index = 0;
    }

    pub fn clear_filters(&mut self) {
        let first = self
            .page
            .as_ref()
            .and_then(|p| p.metadata.columns.first())
            .map(|c| FilterDraft::new(&c.name));
        self.filters = first.into_iter().collect();
        self.applied.clear();
        self.page_index = 0;
    }

    /// Discards a row's staged change; undoing a delete keeps the row's
    /// edits, as the desktop's `discardPendingRow` does.
    pub fn discard_row(&mut self, row: usize) {
        match self.pending.get_mut(&row) {
            Some(pending) if pending.deleted && pending.changes != pending.original => {
                pending.deleted = false;
            }
            _ => {
                self.pending.remove(&row);
            }
        }
        self.version += 1;
    }
}

pub fn pending_row(page: &TablePage, row: &[Value]) -> PendingRow {
    let count = page.metadata.columns.len();
    let original = row[..count.min(row.len())].to_vec();
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

pub fn stage_cell(state: &mut TableState, row: usize, column: usize, text: &str) {
    let Some(page) = &state.page else { return };
    let Some(values) = page.rows.get(row) else {
        return;
    };
    let pending = state
        .pending
        .entry(row)
        .or_insert_with(|| pending_row(page, values));
    pending.changes[column] = parse_cell_input(
        text,
        &page.metadata.columns[column],
        &pending.original[column],
    );
    if !pending.deleted && pending.changes == pending.original {
        state.pending.remove(&row);
    }
    state.version += 1;
}

/// Stages (or, when every row is already staged, undoes) deletion of `rows`.
pub fn toggle_delete(state: &mut TableState, rows: &[usize]) {
    let all_deleted = rows
        .iter()
        .all(|r| state.pending.get(r).is_some_and(|p| p.deleted));
    stage_delete(state, rows, !all_deleted);
}

pub fn stage_delete(state: &mut TableState, rows: &[usize], deleted: bool) {
    let Some(page) = &state.page else { return };
    for &row in rows {
        if deleted {
            if let Some(values) = page.rows.get(row) {
                state
                    .pending
                    .entry(row)
                    .or_insert_with(|| pending_row(page, values))
                    .deleted = true;
            }
        } else if let Some(pending) = state.pending.get_mut(&row) {
            pending.deleted = false;
            if pending.changes == pending.original {
                state.pending.remove(&row);
            }
        }
    }
    state.version += 1;
}

pub fn editable(read_only: bool, page: &TablePage) -> bool {
    !read_only && !page.metadata.primary_key.is_empty()
}

pub fn editable_column(read_only: bool, page: &TablePage, column: usize) -> bool {
    editable(read_only, page)
        && !page
            .metadata
            .primary_key
            .contains(&page.metadata.columns[column].name)
}

/// Values above this size are previewed rather than edited.
pub const LARGE_VALUE_BYTES: usize = 64 * 1024;

/// A human-readable size for a value too large to edit in place.
pub fn large_value_size(value: &Value) -> Option<String> {
    let len = match value {
        Value::String(text) => text.len(),
        Value::Null | Value::Bool(_) | Value::Number(_) => return None,
        other => serde_json::to_vec(other).map_or(0, |bytes| bytes.len()),
    };
    (len > LARGE_VALUE_BYTES).then(|| {
        if len >= 1024 * 1024 {
            format!("{:.1} MiB", len as f64 / (1024.0 * 1024.0))
        } else {
            format!("{} KiB", len / 1024)
        }
    })
}

/// The first `chars` characters of a value's display text, without copying
/// a long string or serializing a large JSON document in full.
pub fn display_prefix(value: &Value, chars: usize) -> String {
    match value {
        Value::String(text) => text.chars().take(chars).collect(),
        Value::Null => "NULL".into(),
        Value::Array(_) | Value::Object(_) => {
            let text = serde_json::to_string(value).unwrap_or_default();
            text.chars().take(chars).collect()
        }
        other => other.to_string(),
    }
}

// ---- requests ------------------------------------------------------------

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
    /// Set when the profile or tab was closed; the reply is then ignored.
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

/// Work the widgets do on the state's behalf after an update.
pub enum Effect {
    Copy(String),
    /// Ask where to export a table tab's rows.
    PickExport {
        tab: u64,
        file_name: String,
    },
    /// Ask where to save a query result, then write it.
    SaveResult {
        tab: u64,
        file_name: String,
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
    },
    /// Rewrite a query editor's text, for history picks.
    SetSql(u64),
}

pub struct State {
    pub worker: Worker,
    pub demo: bool,
    pub profiles: Vec<ConnectionProfile>,
    pub workspaces: HashMap<Uuid, WorkspaceInfo>,
    pub schemas: HashMap<Uuid, Vec<SchemaNode>>,
    pub schema_filters: HashMap<Uuid, String>,
    pub collapsed_profiles: HashSet<Uuid>,
    /// Schema tree folders the user closed (default open) or opened (default
    /// closed), by profile and path.
    pub toggled_nodes: HashSet<(Uuid, String)>,
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
    /// App-level error strip under the top bar.
    pub banner: Option<Message>,
    /// Inline errors and result-action notices under each query tab's editor.
    pub query_errors: HashMap<u64, Message>,
    pub profile_form: Option<ProfileForm>,
    pub confirm_delete: Option<Uuid>,
    pub confirm_query: Option<ConfirmQuery>,
    pub confirm_export: Option<u64>,
    /// Exports waiting for a destination, by tab.
    pub choosing_export: HashMap<u64, (TablePageRequest, Vec<String>)>,
    /// Query results being saved as CSV, by tab.
    pub saving_results: HashSet<u64>,
    pub updates: updates::Updates,
    /// An update awaiting the user's go-ahead.
    pub confirm_update: Option<dbm_update::Available>,
    pub renaming: Option<u64>,
    pub sidebar_collapsed: bool,
    pub now: f64,
    pub window_width: i32,
    pub effects: Vec<Effect>,
}

impl State {
    pub fn new(wake: dbm_desktop::Wake, demo: bool) -> Self {
        let worker = Worker::start(wake.clone(), demo);
        let updates = updates::Updates::new(wake);
        let mut state = Self {
            worker,
            demo,
            profiles: vec![],
            workspaces: HashMap::new(),
            schemas: HashMap::new(),
            schema_filters: HashMap::new(),
            collapsed_profiles: HashSet::new(),
            toggled_nodes: HashSet::new(),
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
            saving_results: HashSet::new(),
            updates,
            confirm_update: None,
            renaming: None,
            sidebar_collapsed: false,
            now: 0.0,
            window_width: 1280,
            effects: Vec::new(),
        };
        state.send(Command::LoadProfiles, "Loading connections", None);
        state
    }

    pub fn send(&mut self, command: Command, label: &'static str, profile: Option<Uuid>) {
        self.dispatch(command, label, None, profile, RequestKind::Other);
    }

    /// Sends now when the worker is idle, otherwise queues behind the current
    /// request. Replies go to the initiating tab, never the selected one.
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
        self.requests()
            .any(|meta| meta.kind == kind && (tab.is_none() || meta.tab == tab))
    }

    /// Sent and queued requests.
    pub fn requests(&self) -> impl Iterator<Item = &RequestMeta> {
        self.pending_requests
            .values()
            .chain(self.queued.iter().map(|q| &q.meta))
    }

    pub fn has_label(&self, profile: Uuid, labels: &[&str]) -> bool {
        self.requests()
            .any(|m| m.profile == Some(profile) && labels.contains(&m.label))
    }

    /// Applies finished work and sends the next queued request.
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
    }

    pub fn fail(&mut self, error: String, meta: &RequestMeta) {
        match meta.kind {
            RequestKind::Form => {
                if let Some(form) = &mut self.profile_form {
                    form.revision += 1;
                    if meta.label == PASSWORD_LOAD {
                        if form.id == meta.profile && !form.password_edited {
                            form.password_load_failed = true;
                            form.feedback = Some((FeedbackKind::Error, error));
                        }
                        return;
                    }
                    form.saving = false;
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
                            // A newer load replaces this page anyway.
                            return;
                        }
                        state.requested = None;
                        state.restore_loaded();
                    }
                    if meta.kind == RequestKind::Export
                        && let Some(state) = self.tables.get_mut(&tab)
                    {
                        // Progress keeps the window re-rendering while set.
                        state.export_progress = None;
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

    /// App-level errors go to the strip under the top bar.
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

    /// Drops expired messages; returns the next time one expires.
    pub fn expire_messages(&mut self) -> Option<f64> {
        let now = self.now;
        if self.banner.as_ref().is_some_and(|m| m.expired(now)) {
            self.banner = None;
        }
        if self.toast.as_ref().is_some_and(|t| t.message.expired(now)) {
            self.toast = None;
        }
        self.query_errors.retain(|_, m| !m.expired(now));
        for table in self.tables.values_mut() {
            if table.message.as_ref().is_some_and(|m| m.expired(now)) {
                table.message = None;
            }
        }
        self.banner
            .iter()
            .chain(self.toast.iter().map(|t| &t.message))
            .chain(self.query_errors.values())
            .chain(self.tables.values().filter_map(|t| t.message.as_ref()))
            .filter_map(Message::expires_at)
            .min_by(f64::total_cmp)
    }

    pub fn apply(&mut self, payload: Payload, meta: &RequestMeta) {
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
                    form.revision += 1;
                }
            }
            Payload::Profiles(profiles) => {
                self.profiles = profiles;
                if !self.demo {
                    let startup: Vec<_> = self
                        .profiles
                        .iter()
                        .filter(|profile| profile.open_on_startup)
                        .map(|profile| profile.id)
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
                form.revision += 1;
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
                        .any(|item| item.id == tab && item.embedded.is_some())
                });
                if let Some(state) = target.and_then(|tab| self.tables.get_mut(&tab)) {
                    state.in_flight = state.in_flight.saturating_sub(1);
                    // Only the newest load's page matches the controls.
                    let latest = state.in_flight == 0;
                    if let Some(request) = state.requested.take_if(|_| latest) {
                        state.loaded(page, request);
                        // The paged table now owns the displayed rows; keep the
                        // query's metadata for its completion label only.
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
                // Clear staged rows and reload, so rows another writer changed
                // show their current values.
                if let Some(state) = self.tables.get_mut(&tab) {
                    state.pending.clear();
                    state.editing = None;
                    state.version += 1;
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
                    if let Some(state) = self.tables.get_mut(&tab) {
                        state.export_progress = None;
                        state.message = Some(message);
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
        // Saving reconnects the profile; table tabs belong to the server and
        // database they were opened on.
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

    pub fn workspace_opened(&mut self, workspace: WorkspaceInfo) {
        let id = workspace.profile.id;
        if self
            .workspaces
            .get(&id)
            .is_some_and(|old| old.profile.default_database != workspace.profile.default_database)
        {
            // Table tabs were opened on the previous database.
            self.close_table_tabs(id);
        }
        if let Some(profile) = self.profiles.iter_mut().find(|profile| profile.id == id) {
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
            let table = self.new_table_state();
            self.tables.insert(tab_id, table);
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

    /// Requests the tab's history the first time it is shown.
    pub fn ensure_history(&mut self, profile: Uuid) {
        let Some(database) = self
            .workspaces
            .get(&profile)
            .map(|w| w.profile.default_database.clone())
        else {
            return;
        };
        let key = (profile, database.clone());
        if self.histories.contains_key(&key)
            || self.requests().any(|m| m.label == "Loading history")
        {
            return;
        }
        self.histories.insert(key, Vec::new());
        self.send(
            Command::History(profile, database),
            "Loading history",
            Some(profile),
        );
    }

    // ---- tabs and profiles ------------------------------------------

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

    pub fn profile_color(&self, id: Uuid) -> String {
        self.profiles
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| p.color.clone())
            .filter(|c| c.len() == 7 && c.starts_with('#'))
            .unwrap_or_else(|| DEFAULT_CONNECTION_COLOR.into())
    }

    pub fn tab(&self, id: u64) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    /// Keeps or restores the profile's workbench: its current tab, its latest
    /// tab, or a new query tab.
    pub fn activate_profile(&mut self, profile: Uuid) {
        self.active_profile = Some(profile);
        if self
            .active_tab
            .and_then(|id| self.tab(id))
            .is_some_and(|t| t.profile_id == profile)
        {
            return;
        }
        if let Some(tab) = self.tabs.iter().rev().find(|t| t.profile_id == profile) {
            self.active_tab = Some(tab.id);
            return;
        }
        self.open_query(profile, true);
    }

    /// A click on a sidebar connection: switch to it, or connect.
    pub fn select_connection(&mut self, id: Uuid) {
        if self.workspaces.contains_key(&id) {
            self.activate_profile(id);
        } else {
            self.active_profile = Some(id);
            if !self.has_label(id, &["Connecting", "Switching database"]) {
                self.send(Command::Connect(id), "Connecting", Some(id));
            }
        }
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
        let table = self.new_table_state();
        self.tables.insert(id, table);
        self.active_tab = Some(id);
        self.active_profile = Some(profile);
        self.load_table(id);
    }

    /// The inspector starts open only on windows at least 1280 px wide.
    pub fn new_table_state(&self) -> TableState {
        TableState {
            inspector_open: self.window_width >= 1280,
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

    /// Shrinks a tab and, if it was active, selects its neighbor.
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

    pub fn rename_tab(&mut self, tab_id: u64, title: &str) {
        let title = title.trim();
        if !title.is_empty()
            && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id)
        {
            tab.title = title.to_owned();
        }
        self.renaming = None;
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
        if self.renaming == Some(tab_id) {
            self.renaming = None;
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
            // Select the tab to the right, else the one to the left.
            let next = self.tabs.get(index).or_else(|| self.tabs.last());
            self.active_tab = next.map(|t| t.id);
            self.active_profile = next.map(|t| t.profile_id).or(self.active_profile);
        }
    }

    pub fn close_table_tabs(&mut self, profile: Uuid) {
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
        // Their loads would run against the new database and report an error
        // for a tab that is gone.
        for meta in self.pending_requests.values_mut() {
            if meta.tab.is_some_and(|tab| removed.contains(&tab)) {
                meta.stale = true;
            }
        }
        self.queued
            .retain(|q| q.meta.tab.is_none_or(|tab| !removed.contains(&tab)));
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

    pub fn mark_stale(&mut self, profile: Uuid) {
        for meta in self.pending_requests.values_mut() {
            if meta.profile == Some(profile) {
                meta.stale = true;
            }
        }
        self.queued.retain(|q| q.meta.profile != Some(profile));
        // Tables kept open would otherwise wait forever for dropped loads.
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

    pub fn edit_profile(&mut self, id: Uuid) {
        let Some(current) = self.profile(id) else {
            return;
        };
        let form = ProfileForm::from_profile(current);
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

    pub fn disconnect(&mut self, id: Uuid) {
        if self.profile_is_clean(id) {
            self.send(Command::Disconnect(id), "Disconnecting", None);
        }
    }

    pub fn switch_database(&mut self, id: Uuid, database: String) {
        let current = self
            .workspaces
            .get(&id)
            .map(|w| &w.profile.default_database);
        if current != Some(&database) && self.profile_is_clean(id) {
            self.send(
                Command::SwitchDatabase(id, database),
                "Switching database",
                Some(id),
            );
        }
    }

    pub fn refresh_schema(&mut self, id: Uuid) {
        self.send(Command::Schema(id), "Refreshing schema", Some(id));
    }

    /// Whether a modal dialog is open, which disables the app shortcuts.
    pub fn modal_open(&self) -> bool {
        self.profile_form.is_some()
            || self.confirm_delete.is_some()
            || self.confirm_query.is_some()
            || self.confirm_update.is_some()
            || self.updates.failure_open
            || !matches!(self.updates.whats_new, updates::WhatsNew::Closed)
            || self.confirm_export.is_some()
    }

    // ---- profile form -----------------------------------------------

    fn form_busy(&self) -> bool {
        self.requests()
            .any(|meta| meta.kind == RequestKind::Form && meta.label != PASSWORD_LOAD)
    }

    pub fn profile_form_busy(&self) -> bool {
        self.form_busy()
    }

    pub fn test_profile(&mut self) {
        let Some(form) = &mut self.profile_form else {
            return;
        };
        form.feedback = None;
        form.saving = false;
        form.revision += 1;
        let input = form.input();
        self.dispatch(
            Command::TestProfile(input),
            "Testing connection",
            None,
            None,
            RequestKind::Form,
        );
    }

    /// Save & connect: tests first, then saves once the test succeeds.
    pub fn save_profile(&mut self) {
        if self.form_busy() {
            return;
        }
        let Some(id) = self.profile_form.as_ref().map(|f| f.id) else {
            return;
        };
        if !id.is_none_or(|id| self.profile_is_clean(id)) {
            return;
        }
        let Some(form) = &mut self.profile_form else {
            return;
        };
        form.saving = true;
        form.feedback = Some((
            FeedbackKind::Info,
            "Testing connection before saving…".into(),
        ));
        form.revision += 1;
        let input = form.input();
        self.dispatch(
            Command::TestProfile(input),
            "Testing connection",
            None,
            None,
            RequestKind::Form,
        );
    }

    pub fn copy_profile_url(&mut self) {
        let Some(form) = &mut self.profile_form else {
            return;
        };
        form.saving = false;
        form.revision += 1;
        if form.password_loaded && !form.password_load_failed {
            match dbm_core::connection_url::format_connection_url(
                &form.input(),
                Some(&form.password),
            ) {
                Ok(url) => {
                    form.feedback = Some((FeedbackKind::Success, "URL copied.".into()));
                    self.effects.push(Effect::Copy(url));
                }
                Err(error) => form.feedback = Some((FeedbackKind::Error, error)),
            }
        } else {
            form.feedback = Some((
                FeedbackKind::Error,
                "Cannot copy the full URL because the saved password did not load.".into(),
            ));
        }
    }

    pub fn request_delete_profile(&mut self) {
        if let Some(id) = self.profile_form.as_ref().and_then(|f| f.id)
            && self.profile_is_clean(id)
        {
            self.confirm_delete = Some(id);
        }
    }

    pub fn delete_profile(&mut self) {
        if let Some(id) = self.confirm_delete
            && self.profile_is_clean(id)
        {
            self.send(Command::DeleteProfile(id), "Deleting connection", None);
        }
    }

    /// Closes the connection dialog unless it is testing or saving; a saved
    /// password still loading is dropped.
    pub fn close_profile_form(&mut self) {
        if self.form_busy() {
            return;
        }
        for meta in self.pending_requests.values_mut() {
            if meta.label == PASSWORD_LOAD {
                meta.stale = true;
            }
        }
        self.queued.retain(|q| q.meta.label != PASSWORD_LOAD);
        self.profile_form = None;
    }

    // ---- queries and tables -----------------------------------------

    /// The table a tab edits: its own table, or the table its last
    /// `SELECT *` resolved to.
    pub fn table_target(&self, tab_id: u64) -> Option<(Uuid, String, String)> {
        let tab = self.tab(tab_id)?;
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

    pub fn read_only(&self, profile: Uuid) -> bool {
        self.profile(profile).is_none_or(|p| p.read_only)
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
        let now = self.now;
        let state = self.tables.entry(tab_id).or_default();
        state.loading = true;
        state.loading_since = Some(now);
        // Paging, filtering, or sorting replaces a stale notice; errors and
        // export results stay until they expire.
        if state
            .message
            .as_ref()
            .is_some_and(|m| m.kind == Kind::Notice && m.export.is_none())
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

    pub fn pending_label(&self, tab: u64) -> Option<&'static str> {
        self.requests()
            .find(|m| m.tab == Some(tab))
            .map(|m| m.label)
    }

    pub fn run_query(&mut self, tab_id: u64, sql: String, refresh: bool, confirmed: bool) {
        let Some(tab) = self.tab(tab_id) else {
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

    pub fn confirm_query_choice(&mut self, run: bool) {
        if let Some(confirm) = self.confirm_query.take()
            && run
        {
            self.run_query(confirm.tab, confirm.sql, confirm.refresh, true);
        }
    }

    /// History picks replace the editor text.
    pub fn pick_history(&mut self, tab_id: u64, sql: String) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            tab.selection = (sql.len(), sql.len());
            tab.sql = sql;
            self.effects.push(Effect::SetSql(tab_id));
        }
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

    /// Saves staged rows, as the desktop app's Save changes does.
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

    pub fn discard_changes(&mut self, tab_id: u64) {
        if let Some(state) = self.tables.get_mut(&tab_id) {
            state.pending.clear();
            state.editing = None;
            state.version += 1;
        }
    }

    /// Export all: large tables ask first.
    pub fn request_export(&mut self, tab_id: u64) {
        let Some(state) = self.tables.get(&tab_id) else {
            return;
        };
        if !state.pending.is_empty() {
            self.table_error(tab_id, PENDING_EXPORT_ERROR);
            return;
        }
        if state.total_rows().unwrap_or(0) > LARGE_EXPORT_WARNING_ROWS {
            self.confirm_export = Some(tab_id);
        } else {
            self.start_export(tab_id);
        }
    }

    pub fn confirm_export_choice(&mut self, export: bool) {
        if let Some(tab) = self.confirm_export.take()
            && export
        {
            self.start_export(tab);
        }
    }

    /// Asks for a destination; `export_chosen` starts the export.
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
        self.choosing_export.insert(tab_id, (request, columns));
        self.effects.push(Effect::PickExport {
            tab: tab_id,
            file_name,
        });
    }

    pub fn export_chosen(&mut self, tab_id: u64, path: Option<PathBuf>) {
        let Some((page, columns)) = self.choosing_export.remove(&tab_id) else {
            return;
        };
        let Some(path) = path else {
            self.table_notice(tab_id, Message::notice("Export canceled.", self.now));
            return;
        };
        if !self.tables.contains_key(&tab_id) {
            return;
        }
        let profile = page.profile_id;
        let request = ExportRequest {
            page,
            columns,
            path,
            progress: Arc::default(),
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

    /// Whether the tab is exporting: choosing a file, writing a table, or
    /// saving a query result.
    pub fn exporting(&self, tab_id: u64) -> bool {
        self.busy(RequestKind::Export, Some(tab_id))
            || self.choosing_export.contains_key(&tab_id)
            || self.saving_results.contains(&tab_id)
    }

    pub fn copy_result(&mut self, tab_id: u64) {
        let Some(result) = self.query_results.get(&tab_id) else {
            return;
        };
        let columns: Vec<_> = result.columns.iter().map(|c| c.name.clone()).collect();
        let text = csv_document(&columns, &result.rows);
        let rows = result.rows.len() as u64;
        self.effects.push(Effect::Copy(text));
        self.query_errors.insert(
            tab_id,
            Message::notice(
                format!("Copied {} result rows as CSV.", count(rows)),
                self.now,
            ),
        );
    }

    pub fn save_result(&mut self, tab_id: u64) {
        let Some(result) = self.query_results.get(&tab_id) else {
            return;
        };
        let title = self.tab(tab_id).map_or("query", |t| t.title.as_str());
        let effect = Effect::SaveResult {
            tab: tab_id,
            file_name: format!("{}.csv", safe_file_name(title)),
            columns: result.columns.iter().map(|c| c.name.clone()).collect(),
            rows: result.rows.clone(),
        };
        self.query_errors.remove(&tab_id);
        self.saving_results.insert(tab_id);
        self.effects.push(effect);
    }

    pub fn result_saved(&mut self, tab_id: u64, outcome: dbm_desktop::worker::ResultExport) {
        self.saving_results.remove(&tab_id);
        if self.tab(tab_id).is_none() {
            return;
        }
        let message = match outcome {
            Ok(Some((path, rows))) => Message::exported(path, rows, self.now),
            Ok(None) => Message::notice("Export canceled.", self.now),
            Err(error) => Message::error(error, self.now),
        };
        self.query_errors.insert(tab_id, message);
    }

    pub fn copy_table_rows(&mut self, tab_id: u64, rows: Vec<usize>, which: &str) {
        let Some(state) = self.tables.get(&tab_id) else {
            return;
        };
        let rows: Vec<usize> = rows
            .into_iter()
            .filter(|r| !state.pending.get(r).is_some_and(|p| p.deleted))
            .collect();
        let label = format!(
            "{} {which} {}",
            rows.len(),
            if rows.len() == 1 { "row" } else { "rows" }
        );
        let text = state.csv(rows.into_iter());
        self.effects.push(Effect::Copy(text));
        self.table_notice(
            tab_id,
            Message::notice(format!("Copied {label} as CSV."), self.now),
        );
    }

    /// Sort, page, limit, filter changes: reload unless edits are staged.
    pub fn reload_table(&mut self, tab_id: u64) {
        if self.tab_dirty(tab_id) {
            self.table_error(tab_id, PENDING_REFRESH_ERROR);
        } else {
            self.load_table(tab_id);
        }
    }

    pub fn close_requested(&mut self) -> bool {
        let dirty = self.tabs.iter().any(|tab| self.tab_dirty(tab.id));
        // Reads can be abandoned; a save or export stopped halfway can't.
        let writing =
            self.busy(RequestKind::Mutation, None) || self.busy(RequestKind::Export, None);
        if dirty || writing {
            self.show_error(if dirty {
                "Save or discard staged changes before closing Anybase."
            } else {
                "Wait for the save or export to finish before closing Anybase."
            });
            return false;
        }
        true
    }
}

#[cfg(test)]
pub mod tests {
    use serde_json::json;

    use super::*;
    use dbm_core::demo::DemoStore;

    pub fn app() -> State {
        let mut app = State::new(Arc::new(|| {}), true);
        app.pending_requests.clear();
        app
    }

    pub fn page() -> TablePage {
        dbm_core::demo::table_page(&TablePageRequest {
            profile_id: Uuid::nil(),
            schema: "public".into(),
            table: "customers".into(),
            offset: 0,
            limit: 5,
            filters: vec![],
            order_by: None,
            include_total: Some(true),
        })
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

    fn response(rows: Vec<Vec<Value>>) -> QueryResponse {
        QueryResponse {
            columns: vec![],
            row_count: rows.len(),
            rows,
            affected_rows: None,
            duration_ms: 1,
            truncated: false,
            notices: vec![],
        }
    }

    #[test]
    fn startup_opens_only_enabled_profiles_and_demo_stays_manual() {
        let mut app = app();
        let mut profiles = DemoStore::new().profiles();
        profiles[1].open_on_startup = false;
        profiles[3].open_on_startup = false;
        let expected = [profiles[0].id, profiles[2].id];
        app.apply(
            Payload::Profiles(profiles.clone()),
            &meta(None, RequestKind::Other),
        );
        assert!(app.pending_requests.is_empty() && app.queued.is_empty());
        app.demo = false;
        app.apply(Payload::Profiles(profiles), &meta(None, RequestKind::Other));
        let pending: Vec<_> = app.pending_requests.values().map(|r| r.profile).collect();
        assert_eq!(pending, vec![Some(expected[0])]);
        assert_eq!(app.queued.len(), 1);
        assert!(matches!(app.queued[0].command, Command::Connect(id) if id == expected[1]));
        assert!(app.active_profile.is_none());
    }

    #[test]
    fn background_workspaces_keep_focus_and_remember_database_after_disconnect() {
        let mut app = app();
        let store = DemoStore::new();
        app.profiles = store.profiles();
        let a = app.profiles[0].id;
        let b = app.profiles[1].id;
        app.workspace_opened(store.workspace(a, None).unwrap());
        let active = app.active_tab;
        app.workspace_opened(store.workspace(b, Some("captures")).unwrap());
        assert_eq!(app.active_profile, Some(a));
        assert_eq!(app.active_tab, active);
        assert!(
            app.tabs
                .iter()
                .any(|tab| tab.profile_id == b && tab.kind == TabKind::Query)
        );
        app.close_profile(b);
        assert_eq!(app.profile(b).unwrap().default_database, "captures");
        app.workspace_opened(store.workspace(a, None).unwrap());
        assert_eq!(app.tabs.len(), 1, "reconnect restores the existing tab");
    }

    #[test]
    fn replies_follow_initiating_tab_and_dirty_profiles_are_guarded() {
        let mut app = app();
        let id = Uuid::from_u128(1);
        app.open_query(id, true);
        let first = app.active_tab.unwrap();
        app.open_query(id, true);
        let second = app.active_tab.unwrap();
        assert_eq!(app.tabs[1].title, "Query 2");
        app.apply(
            Payload::Query(response(vec![vec![json!(37)]])),
            &meta(Some(first), RequestKind::Query),
        );
        assert_eq!(app.query_results[&first].rows, vec![vec![json!(37)]]);
        assert!(!app.query_results.contains_key(&second));
        let mut table = TableState {
            page: Some(Arc::new(page())),
            ..TableState::default()
        };
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
        assert!(!app.close_requested(), "dirty tabs keep the window open");
    }

    #[test]
    fn only_the_newest_table_load_is_shown_and_dropped_loads_never_stick() {
        let mut app = app();
        let id = Uuid::from_u128(1);
        app.open_table(id, "public".into(), "accounts".into());
        let tab = app.active_tab.unwrap();
        app.tables.get_mut(&tab).unwrap().limit = 7;
        app.load_table(tab);
        assert_eq!(app.tables[&tab].in_flight, 2);
        app.apply(Payload::Table(page()), &meta(Some(tab), RequestKind::Table));
        let state = &app.tables[&tab];
        assert!(state.page.is_none(), "an older page is dropped");
        assert!(state.loading);
        app.apply(Payload::Table(page()), &meta(Some(tab), RequestKind::Table));
        let state = &app.tables[&tab];
        assert!(state.page.is_some() && !state.loading);
        assert_eq!(state.loaded.as_ref().map(|r| r.limit), Some(7));
        app.load_table(tab);
        app.mark_stale(id);
        let state = &app.tables[&tab];
        assert!(!state.loading && state.in_flight == 0 && state.requested.is_none());
    }

    #[test]
    fn closing_a_profile_ignores_its_late_replies_only() {
        let mut app = app();
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        app.open_query(a, true);
        let tab = app.active_tab.unwrap();
        app.query_results
            .insert(tab, response(vec![vec![json!("large result")]]));
        app.histories.insert((a, "postgres".into()), vec![]);
        app.schema_filters.insert(a, "customer".into());
        for (id, profile) in [(900, a), (901, b)] {
            app.pending_requests.insert(
                RequestId(id),
                RequestMeta {
                    profile: Some(profile),
                    ..meta(None, RequestKind::Other)
                },
            );
        }
        app.close_profile(a);
        assert!(app.pending_requests[&RequestId(900)].stale);
        assert!(!app.pending_requests[&RequestId(901)].stale);
        assert!(!app.query_results.contains_key(&tab));
        assert!(!app.histories.keys().any(|(profile, _)| *profile == a));
        assert!(!app.schema_filters.contains_key(&a));
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
        app.open_query(id, true);
        let tab = app.active_tab.unwrap();
        app.tabs[0].last_executed = Some("select * from customers;".into());
        app.apply(
            Payload::Query(response(vec![vec![json!("kept until the table loads")]])),
            &meta(Some(tab), RequestKind::Query),
        );
        assert_eq!(
            app.tabs[0].embedded,
            Some(("public".into(), "customers".into()))
        );
        assert!(app.tables[&tab].loading);
        assert_eq!(app.query_results[&tab].rows.len(), 1);
        app.apply(Payload::Table(page()), &meta(Some(tab), RequestKind::Table));
        assert!(app.query_results[&tab].rows.is_empty());
        assert_eq!(app.query_results[&tab].row_count, 1);
        app.close_table_tabs(id);
        assert!(!app.query_results.contains_key(&tab));
        assert!(app.tabs[0].embedded.is_none());
    }

    #[test]
    fn mutation_requests_capture_the_active_database_before_queueing() {
        let mut app = app();
        let mut profile = DemoStore::new().profiles().remove(0);
        let id = profile.id;
        profile.default_database = "first-db".into();
        app.workspaces.insert(
            id,
            WorkspaceInfo {
                profile,
                databases: vec![],
            },
        );
        app.open_table(id, "public".into(), "customers".into());
        let tab = app.active_tab.unwrap();
        let page = page();
        app.tables
            .get_mut(&tab)
            .unwrap()
            .pending
            .insert(0, pending_row(&page, &page.rows[0]));
        let batch = app.mutation_batch(tab).unwrap();
        app.workspaces
            .get_mut(&id)
            .unwrap()
            .profile
            .default_database = "second-db".into();
        assert_eq!(batch.database, "first-db");
        assert_eq!(
            (batch.schema.as_str(), batch.table.as_str()),
            ("public", "customers")
        );
        assert_eq!(batch.mutations.len(), 1);
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
    fn saved_password_loading_preserves_edits() {
        let mut app = app();
        let profile = DemoStore::new().profiles().remove(0);
        let request = RequestMeta {
            label: PASSWORD_LOAD,
            tab: None,
            profile: Some(profile.id),
            kind: RequestKind::Form,
            stale: false,
        };
        app.profile_form = Some(ProfileForm::from_profile(&profile));
        app.fail("Keychain locked".into(), &request);
        let form = app.profile_form.as_ref().unwrap();
        assert!(!form.password_loaded && form.password_load_failed);
        assert_eq!(form.input().password, None, "an unloaded password is kept");
        let loaded = "postgresql://user:saved%40secret@host:5432/db?sslmode=require";
        app.apply(Payload::ConnectionUrl(loaded.into()), &request);
        assert_eq!(app.profile_form.as_ref().unwrap().password, "saved@secret");
        let form = app.profile_form.as_mut().unwrap();
        form.password = "new password".into();
        form.password_edited = true;
        app.apply(Payload::ConnectionUrl(loaded.into()), &request);
        assert_eq!(app.profile_form.as_ref().unwrap().password, "new password");
    }

    #[test]
    fn closing_the_edit_dialog_drops_its_queued_password_load() {
        let mut app = app();
        let profile = DemoStore::new().profiles().remove(0);
        app.pending_requests.insert(
            RequestId(900),
            RequestMeta {
                label: "Running query",
                ..meta(None, RequestKind::Query)
            },
        );
        app.profiles = vec![profile.clone()];
        app.edit_profile(profile.id);
        assert_eq!(app.queued.len(), 1);
        app.close_profile_form();
        assert!(app.profile_form.is_none());
        assert!(app.queued.is_empty());
    }

    #[test]
    fn staging_uses_typed_parsing_and_drops_no_op_edits() {
        let mut state = TableState {
            page: Some(Arc::new(page())),
            ..TableState::default()
        };
        stage_cell(&mut state, 0, 2, "no");
        assert_eq!(state.pending[&0].changes[2], json!(false));
        stage_cell(&mut state, 0, 3, "");
        assert_eq!(
            state.pending[&0].changes[3],
            Value::Null,
            "nullable empty is NULL"
        );
        stage_cell(&mut state, 0, 2, "true");
        stage_cell(&mut state, 0, 3, "Customer since 2001");
        assert!(
            state.pending.is_empty(),
            "reverting every field unstages the row"
        );
        toggle_delete(&mut state, &[1, 2]);
        assert_eq!(state.pending.len(), 2);
        stage_delete(&mut state, &[1], false);
        assert_eq!(state.pending.keys().copied().collect::<Vec<_>>(), vec![2]);
        toggle_delete(&mut state, &[2]);
        assert!(state.pending.is_empty());
    }

    #[test]
    fn copy_skips_deleted_rows_and_uses_staged_values() {
        let mut state = TableState {
            page: Some(Arc::new(page())),
            ..TableState::default()
        };
        stage_cell(&mut state, 0, 1, "ada@example.com");
        stage_delete(&mut state, &[1], true);
        let csv = state.csv(state.copyable_rows().into_iter());
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "id,email,active,note");
        assert_eq!(lines[1], "1,ada@example.com,true,Customer since 2001");
        assert!(lines[2].starts_with("3,"), "row 2 is staged for deletion");
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn undoing_a_delete_keeps_the_rows_edits() {
        let mut state = TableState {
            page: Some(Arc::new(page())),
            ..TableState::default()
        };
        stage_cell(&mut state, 0, 1, "ada@example.com");
        stage_delete(&mut state, &[0], true);
        state.discard_row(0);
        assert!(!state.pending[&0].deleted);
        assert_eq!(state.pending[&0].changes[1], json!("ada@example.com"));
        state.discard_row(0);
        assert!(state.pending.is_empty());
    }

    #[test]
    fn messages_expire_on_their_own_schedules() {
        let mut app = app();
        app.now = 100.0;
        app.show_error("boom");
        app.show_toast("Schema refreshed", true);
        app.open_query(Uuid::from_u128(1), true);
        let tab = app.active_tab.unwrap();
        app.query_errors
            .insert(tab, Message::exported("/tmp/x.csv".into(), 2, 100.0));
        assert_eq!(app.expire_messages(), Some(106.0));
        app.now = 106.0;
        assert_eq!(app.expire_messages(), Some(110.0));
        assert!(app.toast.is_none() && app.banner.is_some());
        app.now = 110.0;
        assert_eq!(app.expire_messages(), None);
        assert!(app.banner.is_none());
        assert!(app.query_errors.contains_key(&tab), "export results stay");
    }

    #[test]
    fn counts_use_thousands_separators() {
        assert_eq!(count(0u32), "0");
        assert_eq!(count(1_000u32), "1,000");
        assert_eq!(count(1_234_567u64), "1,234,567");
    }
}
