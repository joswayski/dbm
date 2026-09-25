//! Private, serial stdio transport for native hosts; never opens a listening port.
use std::io::{self, BufRead, Read, Write};

use dbm_core::{models::QueryRequest, state::AppState};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "camelCase", deny_unknown_fields)]
enum Request {
    ListProfiles {},
    Connect { profile_id: Uuid },
    Disconnect { profile_id: Uuid },
    Query { request: QueryRequest },
}

async fn dispatch(state: &AppState, request: Request) -> Result<Value, String> {
    match request {
        Request::ListProfiles {} => {
            serde_json::to_value(state.profile_summaries().map_err(message)?)
        }
        Request::Connect { profile_id } => {
            let profile = state.profile(profile_id).map_err(message)?;
            state.connect(profile.clone()).await.map_err(message)?;
            serde_json::to_value(profile)
        }
        Request::Disconnect { profile_id } => {
            state.disconnect(profile_id).await;
            Ok(Value::Null)
        }
        Request::Query { mut request } => {
            request.max_rows = Some(request.max_rows.unwrap_or(10_000).clamp(1, 10_000));
            serde_json::to_value(state.run_query(request).await.map_err(message)?)
        }
    }
    .map_err(message)
}

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Small fixed worker pool, independent of machine CPU count. The blocking
    // pipe reader stays on this thread, not a Tokio executor or the AppKit thread.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let state = AppState::new()?;
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    loop {
        let mut line = String::new();
        // A bad host cannot force an unbounded request allocation.
        if (&mut input).take(1_048_577).read_line(&mut line)? == 0 {
            break;
        }
        if line.len() > 1_048_576 || !line.ends_with('\n') {
            return Err("request exceeds limit or is incomplete".into());
        }
        let result = match serde_json::from_str::<Request>(&line) {
            Ok(request) => runtime.block_on(dispatch(&state, request)),
            // Do not echo malformed SQL or profile data into diagnostics.
            Err(_) => Err("invalid native request".into()),
        };
        let reply = match result {
            Ok(value) => json!({"ok": true, "value": value}),
            Err(error) => json!({"ok": false, "error": error}),
        };
        serde_json::to_writer(&mut output, &reply)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    // Process exit releases all sessions, including on host EOF.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_rejects_unknown_commands_fields_and_invalid_ids() {
        for input in [
            r#"{"command":"saveProfile"}"#,
            r#"{"command":"listProfiles","password":"secret"}"#,
            r#"{"command":"connect","profile_id":"not-a-uuid"}"#,
        ] {
            assert!(serde_json::from_str::<Request>(input).is_err());
        }
    }

    #[test]
    fn query_preserves_multiline_unicode_and_profile_identity() {
        let input = json!({"command": "query", "request": {
            "profileId": "12345678-1234-4234-8234-123456789abc",
            "sql": "SELECT 'café'\nAS label", "maxRows": 37
        }});
        let Request::Query { request } = serde_json::from_value(input).unwrap() else {
            panic!("expected query");
        };
        assert_eq!(
            request.profile_id.to_string(),
            "12345678-1234-4234-8234-123456789abc"
        );
        assert_eq!(request.sql, "SELECT 'café'\nAS label");
        assert_eq!(request.max_rows, Some(37));
    }
}
