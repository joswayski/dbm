#![cfg(target_os = "linux")]

use std::ffi::{CStr, c_char};
use std::ptr;
use std::sync::Mutex;

use dbm_native_bridge::{
    dbm_bridge_response_free, dbm_bridge_session_call, dbm_bridge_session_create,
    dbm_bridge_session_free,
};
use serde_json::{Value, json};

static ENVIRONMENT: Mutex<()> = Mutex::new(());

unsafe fn take(pointer: *mut c_char) -> Value {
    assert!(!pointer.is_null());
    let value = unsafe { serde_json::from_slice(CStr::from_ptr(pointer).to_bytes()).unwrap() };
    unsafe { dbm_bridge_response_free(pointer) };
    value
}

#[test]
fn abi_lifecycle_unicode_nulls_and_independent_response_ownership() {
    let _environment = ENVIRONMENT.lock().unwrap();
    let directory = std::env::temp_dir().join(format!("dbm-native-abi-{}", uuid::Uuid::new_v4()));
    unsafe { std::env::set_var("XDG_DATA_HOME", &directory) };

    let mut init_error = ptr::null_mut();
    let session = unsafe { dbm_bridge_session_create(&raw mut init_error) };
    assert!(!session.is_null());
    assert!(init_error.is_null());

    let first = br#"{"command":"listProfiles"}"#;
    let second = br#"{"command":"query","request":{"profileId":"12345678-1234-4234-8234-123456789abc","sql":"SELECT 'caf\u00e9 \ud83d\ude80'\nAS label","maxRows":37}}"#;
    let first_reply = unsafe { dbm_bridge_session_call(session, first.as_ptr(), first.len()) };
    let second_reply = unsafe { dbm_bridge_session_call(session, second.as_ptr(), second.len()) };
    // Both allocations stay valid until individually freed.
    assert_eq!(
        unsafe { take(second_reply) },
        json!({"ok": false, "error": "workspace is not connected"})
    );
    assert_eq!(
        unsafe { take(first_reply) },
        json!({"ok": true, "value": []})
    );

    let null_reply = unsafe { dbm_bridge_session_call(session, ptr::null(), 0) };
    assert_eq!(
        unsafe { take(null_reply) }["error"],
        "native bridge request is null"
    );
    let invalid_utf8 = [0xff];
    let invalid_reply = unsafe { dbm_bridge_session_call(session, invalid_utf8.as_ptr(), 1) };
    assert_eq!(
        unsafe { take(invalid_reply) }["error"],
        "invalid native request"
    );

    let oversized = vec![b' '; 1_048_577];
    let reply = unsafe { dbm_bridge_session_call(session, oversized.as_ptr(), oversized.len()) };
    assert_eq!(
        unsafe { take(reply) }["error"],
        "native request exceeds 1 MiB limit"
    );
    let unknown = br#"{"command":"listProfiles","unexpected":"private"}"#;
    let reply = unsafe { dbm_bridge_session_call(session, unknown.as_ptr(), unknown.len()) };
    assert_eq!(unsafe { take(reply) }["error"], "invalid native request");

    unsafe { dbm_bridge_session_free(session) };
    std::fs::remove_dir_all(directory).unwrap();
}

fn call(session: *mut dbm_native_bridge::DbmBridgeSession, request: &Value) -> Value {
    let bytes = request.to_string().into_bytes();
    let reply = unsafe { dbm_bridge_session_call(session, bytes.as_ptr(), bytes.len()) };
    let reply = unsafe { take(reply) };
    assert_eq!(reply["ok"], true, "{reply}");
    reply["value"].clone()
}

#[test]
fn shared_editor_helpers_use_utf16_offsets() {
    let _environment = ENVIRONMENT.lock().unwrap();
    let directory = std::env::temp_dir().join(format!("dbm-native-abi-{}", uuid::Uuid::new_v4()));
    unsafe { std::env::set_var("XDG_DATA_HOME", &directory) };
    let session = unsafe { dbm_bridge_session_create(ptr::null_mut()) };
    assert!(!session.is_null());

    // "🚀" is two UTF-16 units, so the second statement starts at unit 13.
    let text = "SELECT '🚀';\nSELECT 2;";
    let target = call(
        session,
        &json!({"command": "executionTarget", "engine": "postgres", "text": text, "selection_from": 15, "selection_to": 15}),
    );
    assert_eq!(
        target,
        json!({"from": 13, "to": 22, "sql": "SELECT 2;", "kind": "statement"})
    );
    let selection = call(
        session,
        &json!({"command": "executionTarget", "engine": "redis", "text": "PING\nGET a", "selection_from": 5, "selection_to": 8}),
    );
    assert_eq!(selection["sql"], "GET");
    assert_eq!(selection["kind"], "selection");

    let tokens = call(
        session,
        &json!({"command": "highlight", "engine": "postgres", "text": "SELECT '🚀' AS x"}),
    );
    assert_eq!(
        tokens,
        json!([
            {"from": 0, "to": 6, "kind": "keyword"},
            {"from": 7, "to": 11, "kind": "string"},
            {"from": 12, "to": 14, "kind": "keyword"},
        ])
    );
    assert_eq!(
        call(
            session,
            &json!({"command": "requiresConfirmation", "engine": "postgres", "text": "DELETE FROM users"})
        ),
        true
    );
    let tree = json!([{"name": "public", "kind": "schema", "schema": "public", "table": null, "children": [
        {"name": "users", "kind": "table", "schema": "public", "table": "users", "children": []}
    ]}]);
    assert_eq!(
        call(
            session,
            &json!({"command": "resolveFullTableSelect", "text": "select * from users;", "tree": tree})
        ),
        json!({"schema": "public", "table": "users"})
    );
    let column = json!({"name": "active", "dataType": "boolean", "nullable": true, "defaultValue": null, "ordinal": 1});
    assert_eq!(
        call(
            session,
            &json!({"command": "parseCell", "text": "yes", "column": column, "original": false})
        ),
        true
    );
    assert_eq!(
        call(
            session,
            &json!({"command": "parseCell", "text": "", "column": column, "original": false})
        ),
        Value::Null
    );
    assert_eq!(
        call(session, &json!({"command": "editableText", "value": null})),
        ""
    );
    assert_eq!(
        call(
            session,
            &json!({"command": "csv", "columns": ["a", "b"], "rows": [[1, "x,y"], [null, true]]})
        ),
        "a,b\n1,\"x,y\"\nNULL,true"
    );
    let imported = call(
        session,
        &json!({"command": "parseConnectionUrl", "url": "rediss://cache.local:6380/2"}),
    );
    assert_eq!(imported["engine"], "redis");
    assert_eq!(imported["port"], 6380);
    assert_eq!(imported["defaultDatabase"], "2");
    assert_eq!(imported["tlsMode"], "required");
    let bad = br#"{"command":"parseConnectionUrl","url":"ftp://x"}"#;
    let reply = unsafe { take(dbm_bridge_session_call(session, bad.as_ptr(), bad.len())) };
    assert_eq!(reply["ok"], false);

    unsafe { dbm_bridge_session_free(session) };
    std::fs::remove_dir_all(directory).unwrap();
}
