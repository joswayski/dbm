//! Deterministic in-memory fixture for the native clients' `--demo` mode.
//!
//! It never touches local profiles, credentials, the network, or disk. Text
//! filters and ordering are applied so the table controls can be exercised,
//! but it does not validate SQL semantics, and saving is always refused.

use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{
    ConnectionProfile, DatabaseEngine, DatabaseRef, FilterCondition, FilterOperator,
    MutationResult, OrderSpec, ProfileSummary, QueryColumn, QueryHistoryEntry, QueryRequest,
    QueryResponse, SaveProfileInput, SchemaNode, TableColumn, TableMetadata, TablePage,
    TablePageRequest, TlsMode, WorkspaceInfo,
};
use crate::sql_text::display_value;

pub const SAVE_REFUSED: &str = "Demo fixture: saving is disabled. Staged edits have been retained.";

pub struct DemoStore {
    profiles: Vec<ConnectionProfile>,
    history: Vec<QueryHistoryEntry>,
}

impl Default for DemoStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Builds a profile from editor input without storing it.
pub fn profile_from_input(input: &SaveProfileInput) -> AppResult<ConnectionProfile> {
    let now = Utc::now();
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

impl DemoStore {
    pub fn new() -> Self {
        let now = Utc::now();
        let profile =
            |id, name: &str, color: &str, engine, port, username: &str, database: &str| {
                ConnectionProfile {
                    id: Uuid::from_u128(id),
                    name: name.into(),
                    color: Some(color.into()),
                    engine,
                    host: "demo.invalid".into(),
                    port,
                    username: username.into(),
                    default_database: database.into(),
                    tls_mode: TlsMode::Required,
                    ca_cert_path: None,
                    ssh: None,
                    read_only: false,
                    created_at: now,
                    updated_at: now,
                }
            };
        Self {
            profiles: vec![
                profile(
                    1,
                    "Acme Analytics",
                    "#3dd6c6",
                    DatabaseEngine::Postgres,
                    5432,
                    "demo",
                    "analytics",
                ),
                profile(
                    2,
                    "Session Cache",
                    "#ff9f43",
                    DatabaseEngine::Redis,
                    6379,
                    "",
                    "0",
                ),
            ],
            history: Vec::new(),
        }
    }

    pub fn profiles(&self) -> Vec<ConnectionProfile> {
        self.profiles.clone()
    }

    pub fn profile_summaries(&self) -> Vec<ProfileSummary> {
        self.profiles
            .iter()
            .cloned()
            .map(|profile| ProfileSummary { profile })
            .collect()
    }

    fn profile(&self, id: Uuid) -> AppResult<&ConnectionProfile> {
        self.profiles
            .iter()
            .find(|p| p.id == id)
            .ok_or(AppError::ProfileNotFound)
    }

    fn engine(&self, id: Uuid) -> DatabaseEngine {
        self.profile(id)
            .map_or(DatabaseEngine::Postgres, |p| p.engine)
    }

    pub fn save_profile(&mut self, input: &SaveProfileInput) -> AppResult<ConnectionProfile> {
        let profile = profile_from_input(input)?;
        if let Some(old) = self.profiles.iter_mut().find(|x| x.id == profile.id) {
            *old = profile.clone();
        } else {
            self.profiles.push(profile.clone());
        }
        Ok(profile)
    }

    pub fn delete_profile(&mut self, id: Uuid) {
        self.profiles.retain(|p| p.id != id);
    }

    pub fn databases(&self, id: Uuid) -> AppResult<Vec<DatabaseRef>> {
        let names: &[&str] = if self.profile(id)?.engine == DatabaseEngine::Redis {
            &["0", "1", "2"]
        } else {
            &["analytics", "warehouse"]
        };
        Ok(names
            .iter()
            .map(|name| DatabaseRef {
                name: (*name).into(),
                is_template: false,
                is_connectable: true,
            })
            .collect())
    }

    pub fn workspace(&self, id: Uuid, database: Option<&str>) -> AppResult<WorkspaceInfo> {
        let mut profile = self.profile(id)?.clone();
        if let Some(database) = database {
            profile.default_database = database.into();
        }
        Ok(WorkspaceInfo {
            databases: self.databases(id)?,
            profile,
        })
    }

    pub fn schema_tree(&self, id: Uuid) -> Vec<SchemaNode> {
        let leaf = |kind: &str, schema: &str, name: &str| SchemaNode {
            name: name.into(),
            kind: kind.into(),
            schema: Some(schema.into()),
            table: Some(name.into()),
            children: vec![],
        };
        let folder = |name: &str, schema: &str, children| SchemaNode {
            name: name.into(),
            kind: "schema".into(),
            schema: Some(schema.into()),
            table: None,
            children,
        };
        if self.engine(id) == DatabaseEngine::Redis {
            return vec![
                folder("Keys", "keys", vec![leaf("table", "keys", "all")]),
                folder(
                    "Strings",
                    "string",
                    vec![
                        leaf("key", "string", "greeting"),
                        leaf("key", "string", "session:42"),
                    ],
                ),
                folder("Hashes", "hash", vec![leaf("key", "hash", "user:1")]),
            ];
        }
        vec![
            folder(
                "public",
                "public",
                vec![
                    leaf("table", "public", "customers"),
                    leaf("table", "public", "orders"),
                    leaf("view", "public", "revenue_by_month"),
                ],
            ),
            folder("audit", "audit", vec![leaf("table", "audit", "events")]),
        ]
    }

    pub fn run_query(&mut self, request: &QueryRequest) -> QueryResponse {
        let column = |name: &str, data_type: &str| QueryColumn {
            name: name.into(),
            data_type: data_type.into(),
        };
        let (columns, rows) = if self.engine(request.profile_id) == DatabaseEngine::Redis {
            (vec![column("result", "redis")], vec![vec![json!("PONG")]])
        } else {
            (
                vec![column("customer", "text"), column("revenue", "numeric")],
                vec![
                    vec![json!("Ada Lovelace"), json!("12450.25")],
                    vec![json!("Grace Hopper"), json!("9810.00")],
                    vec![json!("Katherine Johnson"), Value::Null],
                ],
            )
        };
        let response = QueryResponse {
            row_count: rows.len(),
            columns,
            rows,
            affected_rows: None,
            duration_ms: 7 + (request.sql.len() % 5) as u128,
            truncated: false,
            notices: vec![],
        };
        self.history.insert(
            0,
            QueryHistoryEntry {
                id: Uuid::new_v4(),
                profile_id: request.profile_id,
                database: String::new(),
                sql: request.sql.clone(),
                executed_at: Utc::now(),
                duration_ms: response.duration_ms,
                success: true,
            },
        );
        response
    }

    pub fn history(&self, id: Uuid, limit: usize) -> Vec<QueryHistoryEntry> {
        self.history
            .iter()
            .filter(|entry| entry.profile_id == id)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Always refused, so staged edits stay in place.
    pub fn apply_mutations(&self) -> Result<MutationResult, String> {
        Err(SAVE_REFUSED.into())
    }
}

fn column(name: &str, data_type: &str, nullable: bool, ordinal: i32) -> TableColumn {
    TableColumn {
        name: name.into(),
        data_type: data_type.into(),
        nullable,
        default_value: None,
        ordinal,
    }
}

/// A page of the fixture table named by `request`.
pub fn table_page(request: &TablePageRequest) -> TablePage {
    let (columns, primary_key, has_xmin, mut rows): (
        Vec<TableColumn>,
        &str,
        bool,
        Vec<Vec<Value>>,
    ) = match (request.schema.as_str(), request.table.as_str()) {
        ("keys", _) => (
            vec![
                column("key", "string", false, 1),
                column("type", "string", false, 2),
                column("ttl", "integer", false, 3),
            ],
            "key",
            false,
            vec![
                vec![json!("greeting"), json!("string"), json!(-1)],
                vec![json!("session:42"), json!("string"), json!(3600)],
                vec![json!("user:1"), json!("hash"), json!(-1)],
            ],
        ),
        ("hash", _) => (
            vec![
                column("field", "string", false, 1),
                column("value", "string", false, 2),
            ],
            "field",
            false,
            vec![
                vec![json!("name"), json!("Ada")],
                vec![json!("plan"), json!("pro")],
            ],
        ),
        ("string", key) => (
            vec![column("value", "string", false, 1)],
            "value",
            false,
            vec![vec![json!(if key == "greeting" {
                "hello"
            } else {
                "active"
            })]],
        ),
        _ => (
            vec![
                column("id", "bigint", false, 1),
                column("email", "text", false, 2),
                column("active", "boolean", false, 3),
                column("note", "text", true, 4),
            ],
            "id",
            true,
            (1..=500)
                .map(|i| {
                    vec![
                        json!(i),
                        json!(format!("person{i}@example.com")),
                        json!(i % 3 != 0),
                        if i % 4 == 0 {
                            Value::Null
                        } else {
                            json!(format!("Customer since {}", 2000 + i % 25))
                        },
                        json!(format!("xmin-{i}")),
                    ]
                })
                .collect(),
        ),
    };
    rows.retain(|row| {
        request
            .filters
            .iter()
            .all(|f| matches_filter(&columns, row, f))
    });
    if let Some(OrderSpec { column, descending }) = &request.order_by
        && let Some(index) = columns.iter().position(|c| &c.name == column)
    {
        rows.sort_by(|a, b| {
            let ordering = match (&a[index], &b[index]) {
                (Value::Number(x), Value::Number(y)) => x
                    .as_f64()
                    .partial_cmp(&y.as_f64())
                    .unwrap_or(std::cmp::Ordering::Equal),
                (x, y) => display_value(x).cmp(&display_value(y)),
            };
            if *descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }
    let total = rows.len() as u32;
    let mut names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();
    if has_xmin {
        names.push("__dbm_xmin".into());
    }
    TablePage {
        metadata: TableMetadata {
            schema: request.schema.clone(),
            table: request.table.clone(),
            columns,
            primary_key: vec![primary_key.into()],
            has_xmin,
        },
        columns: names,
        rows: rows
            .into_iter()
            .skip(request.offset as usize)
            .take(request.limit as usize)
            .collect(),
        total_rows: Some(u64::from(total)),
        offset: request.offset,
        limit: request.limit,
        has_more: request.offset + request.limit < total,
    }
}

fn matches_filter(columns: &[TableColumn], row: &[Value], filter: &FilterCondition) -> bool {
    let Some(index) = columns.iter().position(|c| c.name == filter.column) else {
        return true;
    };
    let cell = &row[index];
    let text = display_value(cell).to_lowercase();
    let value = filter.value.clone().unwrap_or_default().to_lowercase();
    let list = || {
        value
            .split(',')
            .map(|v| v.trim().to_owned())
            .collect::<Vec<_>>()
    };
    let compare = || {
        let (Ok(left), Ok(right)) = (text.parse::<f64>(), value.parse::<f64>()) else {
            return text.cmp(&value);
        };
        left.partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal)
    };
    match filter.operator {
        FilterOperator::Equals => text == value,
        FilterOperator::NotEquals => text != value,
        FilterOperator::Contains => text.contains(&value),
        FilterOperator::StartsWith => text.starts_with(&value),
        FilterOperator::EndsWith => text.ends_with(&value),
        FilterOperator::GreaterThan => compare().is_gt(),
        FilterOperator::GreaterThanOrEqual => compare().is_ge(),
        FilterOperator::LessThan => compare().is_lt(),
        FilterOperator::LessThanOrEqual => compare().is_le(),
        FilterOperator::In => list().contains(&text),
        FilterOperator::NotIn => !list().contains(&text),
        FilterOperator::IsNull => cell.is_null(),
        FilterOperator::IsNotNull => !cell.is_null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(schema: &str, offset: u32, limit: u32) -> TablePageRequest {
        TablePageRequest {
            profile_id: Uuid::nil(),
            schema: schema.into(),
            table: "customers".into(),
            offset,
            limit,
            filters: vec![],
            order_by: None,
            include_total: Some(true),
        }
    }

    #[test]
    fn page_offsets_are_stable() {
        let page = table_page(&request("public", 200, 200));
        assert_eq!(page.rows[0][0], json!(201));
        assert!(page.has_more);
        assert_eq!(page.columns.last().map(String::as_str), Some("__dbm_xmin"));
    }

    #[test]
    fn filters_and_order_apply() {
        let mut req = request("public", 0, 10);
        req.filters = vec![FilterCondition {
            column: "note".into(),
            operator: FilterOperator::IsNull,
            value: None,
        }];
        req.order_by = Some(OrderSpec {
            column: "id".into(),
            descending: true,
        });
        let page = table_page(&req);
        assert_eq!(page.total_rows, Some(125));
        assert_eq!(page.rows[0][0], json!(500));
    }

    #[test]
    fn redis_fixture_has_key_shaped_pages_and_refuses_saves() {
        let page = table_page(&request("hash", 0, 10));
        assert_eq!(page.metadata.primary_key, vec!["field".to_owned()]);
        assert!(!page.metadata.has_xmin);
        let store = DemoStore::new();
        assert_eq!(store.schema_tree(Uuid::from_u128(2))[0].name, "Keys");
        assert!(store.apply_mutations().is_err());
    }
}
