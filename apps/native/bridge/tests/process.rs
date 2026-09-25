// XDG isolates the real process's storage from the developer's profiles. This
// test intentionally does not override macOS or Windows storage resolution.
#![cfg(target_os = "linux")]

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

#[test]
fn serial_pipe_protocol_recovers_from_bad_input_and_exits_on_eof() {
    let directory = std::env::temp_dir().join(format!("dbm-native-{}", uuid::Uuid::new_v4()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_dbm-native-bridge"))
        .env("XDG_DATA_HOME", &directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(input, "not json: secret that must not be echoed").unwrap();
    for request in [
        json!({"command": "listProfiles"}),
        json!({"command": "query", "request": {
            "profileId": "12345678-1234-4234-8234-123456789abc",
            "sql": "SELECT 'café'\nAS label", "maxRows": 37
        }}),
        json!({"command": "disconnect", "profile_id": "12345678-1234-4234-8234-123456789abc"}),
        json!({"command": "listProfiles"}),
    ] {
        writeln!(input, "{request}").unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        replies,
        vec![
            json!({"ok": false, "error": "invalid native request"}),
            json!({"ok": true, "value": []}),
            json!({"ok": false, "error": "workspace is not connected"}),
            json!({"ok": true, "value": null}),
            json!({"ok": true, "value": []}),
        ]
    );
    std::fs::remove_dir_all(directory).unwrap();
}
