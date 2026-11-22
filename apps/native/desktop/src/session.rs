//! Open tabs kept across an update's restart, as Captures keeps open
//! captures: the app writes them just before the new build replaces it, and
//! the restarted app reopens them once, then deletes the file. Query text,
//! table paging, filters, sort, and staged row edits come back; results are
//! fetched again. The file stays on this machine, beside the app's settings.

use std::path::Path;

use dbm_core::models::{FilterCondition, OrderSpec, RowMutation, TablePage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// The file's name inside each app's data directory.
pub const FILE_NAME: &str = "update-session.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub tabs: Vec<Tab>,
    /// Index into `tabs` of the tab that was showing.
    #[serde(default)]
    pub active: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    pub profile_id: Uuid,
    pub title: String,
    /// A table tab's schema and table; `None` for a query tab.
    #[serde(default)]
    pub table: Option<(String, String)>,
    /// A query tab's editor text.
    #[serde(default)]
    pub sql: String,
    /// A query tab's last statement, re-run on restore when it showed the
    /// editable table viewer (a plain `SELECT * FROM table`).
    #[serde(default)]
    pub embedded_sql: Option<String>,
    #[serde(default)]
    pub collapsed: bool,
    /// The table page a table tab or table viewer showed.
    #[serde(default)]
    pub page: Option<Page>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub index: u32,
    pub limit: u32,
    #[serde(default)]
    pub filters: Vec<FilterCondition>,
    #[serde(default)]
    pub order: Option<OrderSpec>,
    /// Staged edits and deletes, with the original values and `xmin` they
    /// were staged against, so saving still detects rows changed meanwhile.
    #[serde(default)]
    pub pending: Vec<RowMutation>,
}

/// Writes `session` for the restarted app, readable only by this user.
pub fn save(path: &Path, session: &Session) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let staged = path.with_extension("json.new");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&staged)?;
    std::io::Write::write_all(&mut file, &serde_json::to_vec(session)?)?;
    file.sync_all()?;
    std::fs::rename(&staged, path)
}

/// The saved session, read once: the file is removed whether or not it parses.
pub fn take(path: &Path) -> Option<Session> {
    let bytes = std::fs::read(path).ok()?;
    let _ = std::fs::remove_file(path);
    serde_json::from_slice(&bytes).ok()
}

/// Removes a session that will not be used, after a failed restart.
pub fn discard(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Puts restored edits back on the rows of a freshly loaded page, matched by
/// primary key. Returns `(row index, edit)` pairs and how many edits found no
/// row on this page (deleted, or moved off it by another writer).
pub fn match_rows(
    page: &TablePage,
    pending: Vec<RowMutation>,
) -> (Vec<(usize, RowMutation)>, usize) {
    let columns = &page.metadata.columns;
    let key_columns: Vec<usize> = page
        .metadata
        .primary_key
        .iter()
        .filter_map(|key| columns.iter().position(|c| &c.name == key))
        .collect();
    let key = |row: &[Value]| -> Vec<Value> {
        key_columns
            .iter()
            .filter_map(|&i| row.get(i).cloned())
            .collect()
    };
    let mut matched = Vec::new();
    let mut missing = 0;
    for edit in pending {
        let row = (!key_columns.is_empty() && edit.primary_key.len() == key_columns.len())
            .then(|| {
                page.rows
                    .iter()
                    .position(|row| key(row) == edit.primary_key)
            })
            .flatten();
        match row {
            Some(index) if !matched.iter().any(|(i, _)| *i == index) => matched.push((index, edit)),
            _ => missing += 1,
        }
    }
    (matched, missing)
}

/// The notice a table shows when some restored edits found no row.
pub fn missing_rows_message(missing: usize) -> String {
    if missing == 1 {
        "1 staged edit couldn’t be restored: its row isn’t on this page anymore.".into()
    } else {
        format!(
            "{missing} staged edits couldn’t be restored: their rows aren’t on this page anymore."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn page() -> TablePage {
        serde_json::from_value(json!({
            "metadata": {
                "schema": "public", "table": "users",
                "columns": [
                    {"name": "name", "dataType": "text", "nullable": true, "defaultValue": null, "ordinal": 1},
                    {"name": "id", "dataType": "integer", "nullable": false, "defaultValue": null, "ordinal": 2}
                ],
                "primaryKey": ["id"],
                "hasXmin": true
            },
            "columns": ["name", "id"],
            "rows": [["ana", 7, "101"], ["bo", 3, "102"]],
            "offset": 0, "limit": 100, "totalRows": 2, "hasMore": false
        }))
        .unwrap()
    }

    fn edit(id: i64) -> RowMutation {
        RowMutation {
            original: vec![json!("x"), json!(id)],
            changes: vec![json!("y"), json!(id)],
            primary_key: vec![json!(id)],
            xmin: Some("100".into()),
            deleted: false,
        }
    }

    #[test]
    fn edits_find_their_rows_by_primary_key_not_position() {
        // The key is the second column; id 3 is the second row.
        let (matched, missing) = match_rows(&page(), vec![edit(3), edit(99), edit(7)]);
        let rows: Vec<usize> = matched.iter().map(|(row, _)| *row).collect();
        assert_eq!(rows, [1, 0]);
        assert_eq!(missing, 1);
        // The staged xmin is kept, so a row changed meanwhile still conflicts.
        assert_eq!(matched[0].1.xmin.as_deref(), Some("100"));
    }

    #[test]
    fn a_session_is_read_once() {
        let path = std::env::temp_dir().join(format!("dbm-session-{}.json", Uuid::new_v4()));
        let session = Session {
            tabs: vec![Tab {
                profile_id: Uuid::new_v4(),
                title: "Query 1".into(),
                table: None,
                sql: "select 1;".into(),
                embedded_sql: None,
                collapsed: false,
                page: Some(Page {
                    index: 2,
                    limit: 100,
                    filters: vec![],
                    order: None,
                    pending: vec![edit(3)],
                }),
            }],
            active: Some(0),
        };
        save(&path, &session).unwrap();
        let restored = take(&path).expect("the saved session");
        assert_eq!(
            serde_json::to_value(&restored).unwrap(),
            serde_json::to_value(&session).unwrap()
        );
        assert!(take(&path).is_none());
    }
}
