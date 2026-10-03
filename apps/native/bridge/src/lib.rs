//! In-process C ABI for native hosts.
//!
//! A session pointer is owned by its caller until [`dbm_bridge_session_free`].
//! Calls on one session are serialized internally and may come from different
//! threads, but freeing a session concurrently with a call is invalid. Response
//! pointers are UTF-8, NUL-terminated allocations owned by this library; each
//! non-null response must be passed exactly once to [`dbm_bridge_response_free`].

use std::collections::HashMap;
#[cfg(test)]
use std::ffi::CStr;
use std::ffi::{CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::{LazyLock, Mutex};

use dbm_core::{
    cell_values::{editable_text, parse_cell_input},
    connection_url::parse_connection_url,
    demo::{self, DemoStore, profile_from_input},
    export::export_csv,
    models::{
        DatabaseEngine, MutationBatch, QueryRequest, SaveProfileInput, SchemaNode, TableColumn,
        TablePageRequest, TlsMode, WorkspaceInfo,
    },
    session::DbSession,
    sql_text::{
        byte_to_utf16, completions, csv_document, describe_schema_refresh, execution_target,
        highlight, inline_diff, requires_confirmation, resolve_full_table_select, utf16_to_byte,
    },
    state::AppState,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

const MAX_REQUEST_BYTES: usize = 1_048_576;

/// Rows written so far by each running export, keyed by destination path,
/// so a host can show progress through the session-free helper call while
/// the export holds its session.
static EXPORT_PROGRESS: LazyLock<Mutex<HashMap<String, u64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn set_export_progress(path: &str, rows: Option<u64>) {
    if let Ok(mut progress) = EXPORT_PROGRESS.lock() {
        match rows {
            Some(rows) => progress.insert(path.to_owned(), rows),
            None => progress.remove(path),
        };
    }
}

/// Runs the shared exporter while recording progress for `path`.
async fn export_with_progress<F, Fut, E>(
    path: &str,
    columns: &[String],
    request: &TablePageRequest,
    mut load: F,
) -> Result<Value, String>
where
    F: FnMut(TablePageRequest) -> Fut,
    Fut: std::future::Future<Output = Result<dbm_core::models::TablePage, E>>,
    E: std::fmt::Display,
{
    set_export_progress(path, Some(0));
    // Each page request starts at the number of rows already written.
    let result = export_csv(std::path::Path::new(path), columns, request, |page| {
        set_export_progress(path, Some(u64::from(page.offset)));
        load(page)
    })
    .await;
    set_export_progress(path, None);
    Ok(json!({ "rows": result? }))
}

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "camelCase", deny_unknown_fields)]
enum Request {
    ListProfiles {},
    SaveProfile {
        input: SaveProfileInput,
    },
    DeleteProfile {
        profile_id: Uuid,
    },
    /// Connects with the form's settings (or the saved password) and closes.
    TestProfile {
        input: SaveProfileInput,
    },
    ConnectionUrl {
        input: SaveProfileInput,
        include_password: bool,
    },
    Connect {
        profile_id: Uuid,
    },
    ConnectDatabase {
        profile_id: Uuid,
        database: String,
    },
    Disconnect {
        profile_id: Uuid,
    },
    ListDatabases {
        profile_id: Uuid,
    },
    LoadSchemaTree {
        profile_id: Uuid,
    },
    LoadTablePage {
        request: TablePageRequest,
    },
    Query {
        request: QueryRequest,
    },
    ListQueryHistory {
        profile_id: Uuid,
        database: String,
        limit: Option<u32>,
    },
    ApplyTableMutations {
        batch: MutationBatch,
    },
    /// Streams every row matching `request`'s filters and order to `path`.
    ExportCsv {
        request: TablePageRequest,
        columns: Vec<String>,
        path: String,
    },
    /// Rows written so far by the export to `path`, or null when none runs.
    ExportProgress {
        path: String,
    },
    // Editor and grid helpers shared with the other hosts. They touch no
    // database. Offsets are UTF-16 code units, as `NSString` uses.
    ExecutionTarget {
        engine: DatabaseEngine,
        text: String,
        selection_from: usize,
        selection_to: usize,
    },
    RequiresConfirmation {
        engine: DatabaseEngine,
        text: String,
    },
    ResolveFullTableSelect {
        text: String,
        tree: Vec<SchemaNode>,
    },
    Highlight {
        engine: DatabaseEngine,
        text: String,
    },
    ParseCell {
        text: String,
        column: TableColumn,
        original: Value,
    },
    EditableText {
        value: Value,
    },
    Csv {
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
    },
    ParseConnectionUrl {
        url: String,
    },
    FormatConnectionUrl {
        input: SaveProfileInput,
        show_password: bool,
    },
    InlineDiff {
        before: String,
        after: String,
    },
    Completions {
        engine: DatabaseEngine,
        prefix: String,
    },
    DescribeSchemaRefresh {
        previous: Vec<SchemaNode>,
        next: Vec<SchemaNode>,
        kind: String,
    },
    // Self-updates from the latest release. They block on the
    // network, so hosts call them off the main thread.
    /// This build's channel number, or null for development builds.
    #[cfg(feature = "desktop-updater")]
    UpdateCurrent {},
    /// A newer signed build for this platform, or null.
    #[cfg(feature = "desktop-updater")]
    UpdateCheck {},
    /// Downloads and verifies `update` into `directory`; returns the file path.
    #[cfg(feature = "desktop-updater")]
    UpdateDownload {
        update: dbm_update::Available,
        directory: String,
    },
}

enum Backend {
    Live(AppState),
    Mobile(AppState),
    Demo(DemoStore),
}

struct Inner {
    runtime: tokio::runtime::Runtime,
    backend: Backend,
}

/// Opaque session type. Its contents are private to Rust.
pub struct DbmBridgeSession {
    inner: Mutex<Inner>,
}

/// Keep mobile capabilities in the core boundary, not just hidden UI controls.
async fn dispatch_mobile(state: &AppState, request: Request) -> Result<Value, String> {
    match &request {
        Request::SaveProfile { input } | Request::TestProfile { input } => {
            if !input.read_only || input.tls_mode != TlsMode::Required || input.ssh.is_some() {
                return Err("mobile connections require read-only mode and verified TLS; SSH is not supported".into());
            }
        }
        Request::Connect { profile_id } | Request::ConnectDatabase { profile_id, .. } => {
            let profile = state.profile(*profile_id).map_err(message)?;
            if !profile.read_only || profile.tls_mode != TlsMode::Required {
                return Err("mobile connections require read-only mode and verified TLS".into());
            }
        }
        Request::Disconnect { profile_id } => {
            state.disconnect(*profile_id).await;
            state
                .credentials
                .delete_password(*profile_id)
                .map_err(message)?;
            return Ok(Value::Null);
        }
        Request::ListProfiles {}
        | Request::DeleteProfile { .. }
        | Request::ListDatabases { .. }
        | Request::LoadSchemaTree { .. }
        | Request::LoadTablePage { .. }
        | Request::Query { .. }
        | Request::ListQueryHistory { .. } => {}
        _ => return Err("operation is unavailable in the read-only mobile app".into()),
    }
    dispatch(state, request).await
}

async fn dispatch(state: &AppState, request: Request) -> Result<Value, String> {
    match request {
        Request::ConnectionUrl {
            input,
            include_password,
        } => Ok(Value::String(
            state
                .connection_url(&input, include_password)
                .map_err(message)?,
        )),
        Request::ListProfiles {} => {
            serde_json::to_value(state.profile_summaries().map_err(message)?)
        }
        Request::SaveProfile { input } => {
            let profile = state.save_profile(input.clone()).map_err(message)?;
            if let Some(password) = input.password.as_deref() {
                if password.is_empty() {
                    state
                        .credentials
                        .delete_password(profile.id)
                        .map_err(message)?;
                } else {
                    state
                        .credentials
                        .save_password(profile.id, password)
                        .map_err(message)?;
                }
            }
            state.disconnect(profile.id).await;
            serde_json::to_value(profile)
        }
        Request::TestProfile { input } => {
            let profile = profile_from_input(&input).map_err(message)?;
            let password = match input.password {
                Some(password) => Some(password),
                None => input
                    .id
                    .map(|id| state.credentials.get_password(id))
                    .transpose()
                    .map_err(message)?
                    .flatten(),
            };
            let session = DbSession::connect(profile, password)
                .await
                .map_err(message)?;
            session.close().await;
            Ok(Value::Null)
        }
        Request::DeleteProfile { profile_id } => {
            state.disconnect(profile_id).await;
            state
                .credentials
                .delete_password(profile_id)
                .map_err(message)?;
            state.store.delete_profile(profile_id).map_err(message)?;
            Ok(Value::Null)
        }
        Request::Connect { profile_id } => {
            let profile = state.profile(profile_id).map_err(message)?;
            let session = state.connect(profile.clone()).await.map_err(message)?;
            let databases = session.list_databases().await.map_err(message)?;
            serde_json::to_value(WorkspaceInfo { profile, databases })
        }
        Request::ConnectDatabase {
            profile_id,
            database,
        } => {
            let database = database.trim();
            if database.is_empty() {
                return Err("database is required".into());
            }
            let mut profile = state.profile(profile_id).map_err(message)?;
            profile.default_database = database.to_owned();
            let session = state.connect(profile.clone()).await.map_err(message)?;
            let databases = session.list_databases().await.map_err(message)?;
            serde_json::to_value(WorkspaceInfo { profile, databases })
        }
        Request::Disconnect { profile_id } => {
            state.disconnect(profile_id).await;
            Ok(Value::Null)
        }
        Request::ListDatabases { profile_id } => serde_json::to_value(
            state
                .with_session_retry(profile_id, |session| async move {
                    session.list_databases().await
                })
                .await
                .map_err(message)?,
        ),
        Request::LoadSchemaTree { profile_id } => serde_json::to_value(
            state
                .with_session_retry(
                    profile_id,
                    |session| async move { session.schema_tree().await },
                )
                .await
                .map_err(message)?,
        ),
        Request::LoadTablePage { request } => serde_json::to_value(
            state
                .with_session_retry(request.profile_id, |session| {
                    let request = &request;
                    async move { session.table_page(request).await }
                })
                .await
                .map_err(message)?,
        ),
        Request::Query { mut request } => {
            request.max_rows = Some(request.max_rows.unwrap_or(10_000).clamp(1, 10_000));
            serde_json::to_value(state.run_query(request).await.map_err(message)?)
        }
        Request::ListQueryHistory {
            profile_id,
            database,
            limit,
        } => serde_json::to_value(
            state
                .store
                .list_history(profile_id, &database, limit.unwrap_or(100).clamp(1, 500))
                .map_err(message)?,
        ),
        Request::ApplyTableMutations { batch } => serde_json::to_value(
            state
                .session(batch.profile_id)
                .await
                .map_err(message)?
                .apply_mutations(&batch)
                .await
                .map_err(message)?,
        ),
        Request::ExportCsv {
            request,
            columns,
            path,
        } => {
            return export_with_progress(&path, &columns, &request, |page| {
                state.with_session_retry(page.profile_id, move |session| {
                    let page = page.clone();
                    async move { session.table_page(&page).await }
                })
            })
            .await;
        }
        helper => return helper_value(helper),
    }
    .map_err(message)
}

/// Editor and grid helpers that touch no database, for live and demo sessions.
fn helper_value(request: Request) -> Result<Value, String> {
    match request {
        Request::ExecutionTarget {
            engine,
            text,
            selection_from,
            selection_to,
        } => {
            let from = utf16_to_byte(&text, selection_from);
            let to = utf16_to_byte(&text, selection_to);
            Ok(
                execution_target(engine, &text, from, to).map_or(Value::Null, |target| {
                    json!({
                        "from": byte_to_utf16(&text, target.from),
                        "to": byte_to_utf16(&text, target.to),
                        "sql": target.sql,
                        "kind": target.kind,
                    })
                }),
            )
        }
        Request::RequiresConfirmation { engine, text } => {
            Ok(Value::Bool(requires_confirmation(engine, &text)))
        }
        Request::ResolveFullTableSelect { text, tree } => {
            Ok(resolve_full_table_select(&text, &tree).map_or(
                Value::Null,
                |(schema, table)| json!({ "schema": schema, "table": table }),
            ))
        }
        Request::Highlight { engine, text } => Ok(Value::Array(
            highlight(engine, &text)
                .into_iter()
                .map(|token| {
                    json!({
                        "from": byte_to_utf16(&text, token.from),
                        "to": byte_to_utf16(&text, token.to),
                        "kind": token.kind,
                    })
                })
                .collect(),
        )),
        Request::ParseCell {
            text,
            column,
            original,
        } => Ok(parse_cell_input(&text, &column, &original)),
        Request::EditableText { value } => Ok(Value::String(editable_text(&value))),
        Request::Csv { columns, rows } => Ok(Value::String(csv_document(&columns, &rows))),
        Request::ParseConnectionUrl { url } => serde_json::to_value(parse_connection_url(&url)?),
        Request::FormatConnectionUrl {
            input,
            show_password,
        } => Ok(Value::String(
            dbm_core::connection_url::display_connection_url(&input, show_password)?,
        )),
        Request::ExportProgress { path } => Ok(EXPORT_PROGRESS
            .lock()
            .ok()
            .and_then(|progress| progress.get(&path).copied())
            .map_or(Value::Null, Value::from)),
        Request::Completions { engine, prefix } => {
            serde_json::to_value(completions(engine, &prefix))
        }
        Request::InlineDiff { before, after } => serde_json::to_value(inline_diff(&before, &after)),
        #[cfg(feature = "desktop-updater")]
        Request::UpdateCurrent {} => {
            Ok(dbm_update::current_build().map_or(Value::Null, Value::from))
        }
        #[cfg(feature = "desktop-updater")]
        Request::UpdateCheck {} => {
            let Some(current) = dbm_update::current_build() else {
                return Ok(Value::Null);
            };
            serde_json::to_value(dbm_update::check(current)?)
        }
        #[cfg(feature = "desktop-updater")]
        Request::UpdateDownload { update, directory } => {
            let path = dbm_update::download(&update, std::path::Path::new(&directory))?;
            Ok(Value::String(path.to_string_lossy().into_owned()))
        }
        Request::DescribeSchemaRefresh {
            previous,
            next,
            kind,
        } => {
            let (changed, message) = describe_schema_refresh(&previous, &next, &kind);
            Ok(json!({ "changed": changed, "message": message }))
        }
        _ => return Err("unsupported native request".into()),
    }
    .map_err(message)
}

/// Answers from the in-memory fixture. Exports and saves are refused.
fn dispatch_demo(store: &mut DemoStore, request: Request) -> Result<Value, String> {
    match request {
        Request::ConnectionUrl {
            input,
            include_password,
        } => Ok(Value::String(
            dbm_core::connection_url::format_connection_url(
                &input,
                if include_password {
                    input.password.as_deref()
                } else {
                    None
                },
            )?,
        )),
        Request::ListProfiles {} => serde_json::to_value(store.profile_summaries()),
        Request::SaveProfile { input } => {
            serde_json::to_value(store.save_profile(&input).map_err(message)?)
        }
        Request::DeleteProfile { profile_id } => {
            store.delete_profile(profile_id);
            Ok(Value::Null)
        }
        Request::TestProfile { input } => {
            profile_from_input(&input).map_err(message)?;
            Ok(Value::Null)
        }
        Request::Connect { profile_id } => {
            serde_json::to_value(store.workspace(profile_id, None).map_err(message)?)
        }
        Request::ConnectDatabase {
            profile_id,
            database,
        } => serde_json::to_value(
            store
                .workspace(profile_id, Some(&database))
                .map_err(message)?,
        ),
        Request::Disconnect { .. } => Ok(Value::Null),
        Request::ListDatabases { profile_id } => {
            serde_json::to_value(store.databases(profile_id).map_err(message)?)
        }
        Request::LoadSchemaTree { profile_id } => {
            serde_json::to_value(store.schema_tree(profile_id))
        }
        Request::LoadTablePage { request } => serde_json::to_value(demo::table_page(&request)),
        Request::Query { request } => serde_json::to_value(store.run_query(&request)),
        Request::ListQueryHistory {
            profile_id, limit, ..
        } => serde_json::to_value(store.history(profile_id, limit.unwrap_or(100) as usize)),
        Request::ApplyTableMutations { .. } => return Err(store.apply_mutations().unwrap_err()),
        Request::ExportCsv { .. } => unreachable!("demo exports run on the session runtime"),
        helper => return helper_value(helper),
    }
    .map_err(message)
}

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn response(value: Value) -> *mut c_char {
    // JSON escapes embedded NULs, so serialization always forms a C string.
    CString::new(value.to_string()).map_or(ptr::null_mut(), CString::into_raw)
}

fn owned_string(value: impl Into<Vec<u8>>) -> *mut c_char {
    CString::new(value).map_or(ptr::null_mut(), CString::into_raw)
}

fn error_response(error: impl std::fmt::Display) -> *mut c_char {
    response(json!({"ok": false, "error": error.to_string()}))
}

/// Creates a session. On failure returns null and writes an owned error response
/// to `error_out` when it is non-null. Free that response with
/// `dbm_bridge_response_free`.
///
/// # Safety
///
/// When non-null, `error_out` must point to writable pointer storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dbm_bridge_session_create(
    error_out: *mut *mut c_char,
) -> *mut DbmBridgeSession {
    unsafe {
        create_session(error_out, || {
            AppState::new().map(Backend::Live).map_err(message)
        })
    }
}

/// Creates a TLS-required, read-only session with host-managed metadata storage
/// and session-only passwords. `path` is an absolute UTF-8 SQLite file path.
/// Its parent must exist; the host controls sandbox protection and backup policy.
///
/// # Safety
///
/// `path` must address `length` readable bytes. A non-null `error_out` must point
/// to writable pointer storage. Errors and sessions have the usual ABI ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dbm_bridge_mobile_session_create(
    path: *const u8,
    length: usize,
    error_out: *mut *mut c_char,
) -> *mut DbmBridgeSession {
    unsafe {
        create_session(error_out, || {
            if path.is_null() || length == 0 || length > MAX_REQUEST_BYTES {
                return Err("invalid mobile storage path".into());
            }
            let bytes = std::slice::from_raw_parts(path, length);
            let path =
                std::str::from_utf8(bytes).map_err(|_| "mobile storage path must be UTF-8")?;
            let path = std::path::Path::new(path);
            if !path.is_absolute() {
                return Err("mobile storage path must be absolute".into());
            }
            AppState::mobile(path).map(Backend::Mobile).map_err(message)
        })
    }
}

unsafe fn create_session(
    error_out: *mut *mut c_char,
    create: impl FnOnce() -> Result<Backend, String>,
) -> *mut DbmBridgeSession {
    if !error_out.is_null() {
        // SAFETY: The ABI contract requires a writable pointer when non-null.
        unsafe { *error_out = ptr::null_mut() };
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(message)?;
        let backend = create()?;
        Ok::<_, String>(Box::new(DbmBridgeSession {
            inner: Mutex::new(Inner { runtime, backend }),
        }))
    }));
    match result {
        Ok(Ok(session)) => Box::into_raw(session),
        Ok(Err(error)) => {
            if !error_out.is_null() {
                unsafe { *error_out = owned_string(error) };
            }
            ptr::null_mut()
        }
        Err(_) => {
            if !error_out.is_null() {
                unsafe { *error_out = owned_string("native bridge initialization panicked") };
            }
            ptr::null_mut()
        }
    }
}

/// Creates a session backed by the in-memory demo fixture. It never reads
/// local profiles or credentials, touches the network, or writes to disk.
/// Free it with `dbm_bridge_session_free`. Returns null only if the runtime
/// cannot start.
#[unsafe(no_mangle)]
pub extern "C" fn dbm_bridge_demo_session_create() -> *mut DbmBridgeSession {
    catch_unwind(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;
        Some(Box::into_raw(Box::new(DbmBridgeSession {
            inner: Mutex::new(Inner {
                runtime,
                backend: Backend::Demo(DemoStore::new()),
            }),
        })))
    })
    .ok()
    .flatten()
    .unwrap_or(ptr::null_mut())
}

/// Executes one JSON request. `request` need not be NUL terminated; `length`
/// is its exact byte count. The returned owned response is never auto-replayed.
///
/// # Safety
///
/// `session` must be null or a live pointer returned by create. `request` must
/// address `length` readable bytes. The session may not be freed during a call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dbm_bridge_session_call(
    session: *mut DbmBridgeSession,
    request: *const u8,
    length: usize,
) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        if session.is_null() {
            return error_response("native bridge session is null");
        }
        if request.is_null() {
            return error_response("native bridge request is null");
        }
        if length > MAX_REQUEST_BYTES {
            return error_response("native request exceeds 1 MiB limit");
        }
        // SAFETY: The caller guarantees `length` readable bytes for non-null `request`.
        let bytes = unsafe { std::slice::from_raw_parts(request, length) };
        let Ok(request) = serde_json::from_slice::<Request>(bytes) else {
            return error_response("invalid native request");
        };
        // SAFETY: Null was rejected and the caller keeps the session alive for this call.
        let session = unsafe { &*session };
        let Ok(mut inner) = session.inner.lock() else {
            return error_response("native bridge session is unavailable");
        };
        let Inner { runtime, backend } = &mut *inner;
        let result = match backend {
            Backend::Live(state) => runtime.block_on(dispatch(state, request)),
            Backend::Mobile(state) => runtime.block_on(dispatch_mobile(state, request)),
            Backend::Demo(store) => match request {
                // Demo exports stream the fixture to the chosen file, as the
                // other hosts' demo mode does.
                Request::ExportCsv {
                    request,
                    columns,
                    path,
                } => runtime.block_on(export_with_progress(&path, &columns, &request, |page| {
                    std::future::ready(Ok::<_, String>(demo::table_page(&page)))
                })),
                request => dispatch_demo(store, request),
            },
        };
        match result {
            Ok(value) => response(json!({"ok": true, "value": value})),
            Err(error) => error_response(error),
        }
    }))
    .unwrap_or_else(|_| error_response("native bridge call panicked"))
}

/// Runs one editor or grid helper request (`executionTarget`, `highlight`,
/// `requiresConfirmation`, `resolveFullTableSelect`, `parseCell`,
/// `editableText`, `csv`, `parseConnectionUrl`) without a session. These
/// touch no database or storage, so the UI thread may call this directly
/// while a session call is running. Free the response as usual.
///
/// # Safety
///
/// `request` must address `length` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dbm_bridge_helper_call(request: *const u8, length: usize) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        if request.is_null() {
            return error_response("native bridge request is null");
        }
        if length > MAX_REQUEST_BYTES {
            return error_response("native request exceeds 1 MiB limit");
        }
        // SAFETY: The caller guarantees `length` readable bytes for non-null `request`.
        let bytes = unsafe { std::slice::from_raw_parts(request, length) };
        let Ok(request) = serde_json::from_slice::<Request>(bytes) else {
            return error_response("invalid native request");
        };
        match helper_value(request) {
            Ok(value) => response(json!({"ok": true, "value": value})),
            Err(error) => error_response(error),
        }
    }))
    .unwrap_or_else(|_| error_response("native bridge call panicked"))
}

/// Frees a session. Null is accepted. No call may be active concurrently.
///
/// # Safety
///
/// `session` must be null or a live, not-previously-freed pointer from create.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dbm_bridge_session_free(session: *mut DbmBridgeSession) {
    if !session.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: The pointer was returned by create and is freed exactly once.
            drop(unsafe { Box::from_raw(session) });
        }));
    }
}

/// Frees a response returned by this library. Null is accepted.
///
/// # Safety
///
/// `response` must be null or a not-previously-freed pointer returned by this library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dbm_bridge_response_free(response: *mut c_char) {
    if !response.is_null() {
        // SAFETY: The pointer came from CString::into_raw and is freed exactly once.
        drop(unsafe { CString::from_raw(response) });
    }
}

#[cfg(test)]
fn response_json(pointer: *mut c_char) -> Value {
    assert!(!pointer.is_null());
    let value = unsafe { serde_json::from_slice(CStr::from_ptr(pointer).to_bytes()).unwrap() };
    unsafe { dbm_bridge_response_free(pointer) };
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_profiles_persist_but_credentials_do_not_and_capabilities_fail_closed() {
        let directory = std::env::temp_dir().join(format!("dbm-mobile-{}-é", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("profiles.sqlite3");
        let bytes = path.to_str().unwrap().as_bytes();
        let mut error = ptr::null_mut();
        let session =
            unsafe { dbm_bridge_mobile_session_create(bytes.as_ptr(), bytes.len(), &mut error) };
        assert!(!session.is_null());
        assert!(error.is_null());
        let call = |request: Value| {
            let bytes = request.to_string().into_bytes();
            response_json(unsafe { dbm_bridge_session_call(session, bytes.as_ptr(), bytes.len()) })
        };
        let input = json!({"name":"Phone 🦀", "host":"db.invalid", "port":5432,
            "username":"reader", "defaultDatabase":"app", "readOnly":true,
            "tlsMode":"required", "password":"fixture-session-password"});
        for (field, value) in [("readOnly", json!(false)), ("tlsMode", json!("preferred"))] {
            let mut unsafe_input = input.clone();
            unsafe_input[field] = value;
            assert_eq!(
                call(json!({"command":"saveProfile", "input":unsafe_input}))["ok"],
                false
            );
            assert_eq!(
                call(json!({"command":"testProfile", "input":unsafe_input}))["ok"],
                false
            );
        }
        let saved = call(json!({"command":"saveProfile", "input":input}));
        assert_eq!(saved["ok"], true);
        let id: Uuid = serde_json::from_value(saved["value"]["id"].clone()).unwrap();
        {
            let inner = unsafe { &*session }.inner.lock().unwrap();
            let Backend::Mobile(state) = &inner.backend else {
                panic!("expected mobile backend")
            };
            assert_eq!(
                state.credentials.get_password(id).unwrap().as_deref(),
                Some("fixture-session-password")
            );
        }
        let summaries = call(json!({"command":"listProfiles"}));
        assert_eq!(summaries["value"][0]["profile"]["name"], "Phone 🦀");
        assert!(summaries["value"][0]["profile"].get("password").is_none());
        assert_eq!(
            call(
                json!({"command":"exportCsv", "path":directory.join("must-not-exist.csv"),
            "columns":["id"], "request":{"profileId":id,"schema":"public","table":"items",
            "offset":0,"limit":50,"filters":[],"orderBy":null,"includeTotal":false}})
            )["ok"],
            false
        );
        assert!(!directory.join("must-not-exist.csv").exists());
        assert_eq!(
            call(
                json!({"command":"applyTableMutations", "batch":{"profileId":id,
            "database":"app","schema":"public","table":"items","mutations":[]}})
            )["ok"],
            false
        );
        assert_eq!(
            call(json!({"command":"disconnect","profile_id":id}))["ok"],
            true
        );
        {
            let inner = unsafe { &*session }.inner.lock().unwrap();
            let Backend::Mobile(state) = &inner.backend else {
                panic!("expected mobile backend")
            };
            assert!(state.credentials.get_password(id).unwrap().is_none());
        }
        unsafe { dbm_bridge_session_free(session) };
        let reopened = AppState::mobile(&path).unwrap();
        assert_eq!(reopened.profile(id).unwrap().name, "Phone 🦀");
        assert!(reopened.credentials.get_password(id).unwrap().is_none());
        let disk = std::fs::read(&path).unwrap();
        assert!(
            !disk
                .windows(b"fixture-session-password".len())
                .any(|window| window == b"fixture-session-password")
        );
        drop(reopened);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn mobile_constructor_rejects_invalid_paths_without_desktop_fallback() {
        for path in [
            b"relative.sqlite3".as_slice(),
            b"\xff".as_slice(),
            b"".as_slice(),
        ] {
            let mut error = ptr::null_mut();
            let session =
                unsafe { dbm_bridge_mobile_session_create(path.as_ptr(), path.len(), &mut error) };
            assert!(session.is_null());
            assert!(!error.is_null());
            unsafe { dbm_bridge_response_free(error) };
        }
    }

    #[test]
    fn connection_url_helper_separates_masked_preview_from_full_copy() {
        for (show_password, expected) in [
            (
                false,
                "postgresql://me:*******@db.local:5432/app?sslmode=require",
            ),
            (
                true,
                "postgresql://me:p%40ss@db.local:5432/app?sslmode=require",
            ),
        ] {
            let request: Request = serde_json::from_value(json!({
                "command": "formatConnectionUrl", "show_password": show_password,
                "input": { "name": "Test", "host": "db.local", "port": 5432,
                    "username": "me", "defaultDatabase": "app", "password": "p@ss",
                    "tlsMode": "required" }
            }))
            .unwrap();
            assert_eq!(
                helper_value(request).unwrap(),
                Value::String(expected.into())
            );
        }
    }

    #[test]
    fn null_arguments_return_owned_errors_and_null_frees_are_safe() {
        let reply = unsafe { dbm_bridge_session_call(ptr::null_mut(), ptr::null(), 0) };
        assert_eq!(
            response_json(reply)["error"],
            "native bridge session is null"
        );
        unsafe {
            dbm_bridge_session_free(ptr::null_mut());
            dbm_bridge_response_free(ptr::null_mut());
        }
    }
}
