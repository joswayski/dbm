//! In-process C ABI for native hosts.
//!
//! A session pointer is owned by its caller until [`dbm_bridge_session_free`].
//! Calls on one session are serialized internally and may come from different
//! threads, but freeing a session concurrently with a call is invalid. Response
//! pointers are UTF-8, NUL-terminated allocations owned by this library; each
//! non-null response must be passed exactly once to [`dbm_bridge_response_free`].

#[cfg(test)]
use std::ffi::CStr;
use std::ffi::{CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::Mutex;

use dbm_core::{
    models::{MutationBatch, QueryRequest, SaveProfileInput, TablePageRequest, WorkspaceInfo},
    state::AppState,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

const MAX_REQUEST_BYTES: usize = 1_048_576;

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
}

struct Inner {
    runtime: tokio::runtime::Runtime,
    state: AppState,
}

/// Opaque session type. Its contents are private to Rust.
pub struct DbmBridgeSession {
    inner: Mutex<Inner>,
}

async fn dispatch(state: &AppState, request: Request) -> Result<Value, String> {
    match request {
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
        let state = AppState::new().map_err(message)?;
        Ok::<_, String>(Box::new(DbmBridgeSession {
            inner: Mutex::new(Inner { runtime, state }),
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
        let Ok(inner) = session.inner.lock() else {
            return error_response("native bridge session is unavailable");
        };
        let result = inner.runtime.block_on(dispatch(&inner.state, request));
        match result {
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
