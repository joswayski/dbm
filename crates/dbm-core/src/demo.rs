//! Deterministic in-memory fixture for the native clients' `--demo` mode.
//!
//! It never touches local profiles, credentials, the network, or disk. Text
//! filters and ordering are applied so the table controls can be exercised,
//! but it does not validate SQL semantics, and saving is always refused.

use chrono::{Duration, TimeZone, Utc};
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
    /// The same four connections as the desktop app's `?demo` preview
    /// (`browserDemo.ts`), so native screenshots compare one to one.
    pub fn new() -> Self {
        let created = Utc.with_ymd_and_hms(2026, 1, 12, 9, 0, 0).unwrap();
        let profile = |id,
                       name: &str,
                       color: &str,
                       engine,
                       host: &str,
                       port,
                       username: &str,
                       database: &str| {
            ConnectionProfile {
                id: Uuid::from_u128(id),
                name: name.into(),
                color: Some(color.into()),
                engine,
                host: host.into(),
                port,
                username: username.into(),
                default_database: database.into(),
                tls_mode: TlsMode::Required,
                ca_cert_path: None,
                ssh: None,
                read_only: false,
                created_at: created,
                updated_at: created,
            }
        };
        let postgres = DatabaseEngine::Postgres;
        Self {
            profiles: vec![
                profile(
                    1,
                    "Production",
                    "#ff9f43",
                    postgres,
                    "db.acme.internal",
                    5432,
                    "app_admin",
                    "acme",
                ),
                profile(
                    2,
                    "Staging",
                    "#3dd6c6",
                    postgres,
                    "staging-db.acme.internal",
                    5432,
                    "app_admin",
                    "acme",
                ),
                profile(
                    3,
                    "Analytics",
                    "#b48cff",
                    DatabaseEngine::Mysql,
                    "warehouse.acme.internal",
                    3306,
                    "analyst",
                    "warehouse",
                ),
                profile(
                    4,
                    "Session cache",
                    "#ff6b8a",
                    DatabaseEngine::Redis,
                    "cache.acme.internal",
                    6379,
                    "",
                    "0",
                ),
            ],
            history: demo_history(),
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
        Ok(vec![DatabaseRef {
            name: self.profile(id)?.default_database.clone(),
            is_template: false,
            is_connectable: true,
        }])
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
        match self.engine(id) {
            DatabaseEngine::Redis => vec![
                folder("Keys", "keys", vec![leaf("table", "keys", "all")]),
                folder("Strings", "string", vec![leaf("key", "string", "greeting")]),
                folder("Hashes", "hash", vec![leaf("key", "hash", "user:1")]),
            ],
            DatabaseEngine::Mysql => {
                let database = self
                    .profile(id)
                    .map_or_else(|_| "warehouse".to_owned(), |p| p.default_database.clone());
                vec![folder(
                    &database,
                    &database,
                    vec![
                        leaf("table", &database, "users"),
                        leaf("table", &database, "orders"),
                    ],
                )]
            }
            DatabaseEngine::Postgres => vec![folder(
                "public",
                "public",
                TABLES
                    .iter()
                    .map(|name| leaf("table", "public", name))
                    .chain(VIEWS.iter().map(|name| leaf("view", "public", name)))
                    .collect(),
            )],
        }
    }

    /// Any SELECT answers with the preview's revenue report; other reads get
    /// a one-cell placeholder and writes report no affected rows.
    pub fn run_query(&mut self, request: &QueryRequest) -> QueryResponse {
        let column = |name: &str, data_type: &str| QueryColumn {
            name: name.into(),
            data_type: data_type.into(),
        };
        let sql = request.sql.trim_start().to_lowercase();
        let first = sql.split_whitespace().next().unwrap_or_default().to_owned();
        let redis = self.engine(request.profile_id) == DatabaseEngine::Redis;
        let (columns, rows, affected, duration) = if !redis && contains_word(&sql, "select") {
            let (columns, rows) = revenue_report();
            (columns, rows, None, 41)
        } else if [
            "select", "show", "with", "values", "ping", "get", "hgetall", "scan", "keys", "info",
        ]
        .contains(&first.as_str())
        {
            let ping = sql.trim() == "ping";
            (
                vec![column(if ping { "value" } else { "result" }, "text")],
                vec![vec![json!(if ping { "PONG" } else { "DBM demo" })]],
                None,
                2,
            )
        } else {
            (vec![], vec![], Some(0), 2)
        };
        let response = QueryResponse {
            row_count: rows.len(),
            columns,
            rows,
            affected_rows: affected,
            duration_ms: duration,
            truncated: false,
            notices: vec![],
        };
        let database = self
            .profile(request.profile_id)
            .map(|p| p.default_database.clone())
            .unwrap_or_default();
        self.history.insert(
            0,
            QueryHistoryEntry {
                id: Uuid::new_v4(),
                profile_id: request.profile_id,
                database,
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

// ---- The desktop preview's dataset (`browserDemo.ts`), row for row. ----

const TABLES: [&str; 7] = [
    "accounts", "api_keys", "invoices", "orders", "products", "sessions", "users",
];
const VIEWS: [&str; 2] = ["active_subscriptions", "monthly_revenue"];

const FIRST: [&str; 40] = [
    "Maya", "Liam", "Priya", "Tomás", "Ada", "Jonah", "Sofia", "Kenji", "Elena", "Rahim", "Hana",
    "Marco", "Grace", "Owen", "Amara", "Felix", "Inês", "Noah", "Zara", "Mateo", "Leila", "Oscar",
    "Yuki", "Daniel", "Chloe", "Arjun", "Freya", "Samuel", "Nadia", "Lucas", "Isla", "Tariq",
    "Emma", "Diego", "Mei", "Theo", "Aisha", "Hugo", "Clara", "Ravi",
];
const LAST: [&str; 40] = [
    "Okafor",
    "Brennan",
    "Raman",
    "Vidal",
    "Nguyen",
    "Holloway",
    "Marchetti",
    "Mori",
    "Vasquez",
    "Chowdhury",
    "Kim",
    "Bellini",
    "Liu",
    "Price",
    "Osei",
    "Wagner",
    "Costa",
    "Fischer",
    "Ahmed",
    "Rossi",
    "Haddad",
    "Lindqvist",
    "Tanaka",
    "Moreau",
    "Dubois",
    "Mehta",
    "Larsen",
    "Adeyemi",
    "Petrova",
    "Silva",
    "Walsh",
    "Rahman",
    "Novak",
    "Herrera",
    "Zhang",
    "Becker",
    "Bello",
    "Laurent",
    "Jensen",
    "Iyer",
];
const DOMAINS: [&str; 17] = [
    "northwind.dev",
    "fieldstone.io",
    "kestrel.app",
    "corvid.co",
    "lumen.so",
    "brightline.com",
    "meridianlabs.io",
    "tidewater.jp",
    "quarry.dev",
    "pinecrest.org",
    "solstice.ai",
    "harbor.app",
    "ember.io",
    "granite.dev",
    "bluefin.co",
    "nordpeak.de",
    "lagoa.pt",
];
const PLANS: [&str; 18] = [
    "pro",
    "team",
    "pro",
    "free",
    "pro",
    "team",
    "pro",
    "free",
    "enterprise",
    "pro",
    "free",
    "team",
    "pro",
    "team",
    "free",
    "pro",
    "team",
    "enterprise",
];
const REGIONS: [&str; 4] = ["us-east-1", "eu-west-1", "ap-northeast-1", "us-west-2"];
const STATUSES: [&str; 8] = [
    "paid", "paid", "open", "paid", "void", "paid", "open", "paid",
];
const PRODUCTS: [(&str, &str, i64); 8] = [
    ("SKU-TEAM", "Team plan (monthly)", 24),
    ("SKU-PRO", "Pro plan (monthly)", 49),
    ("SKU-ENT", "Enterprise plan (monthly)", 199),
    ("SKU-SEAT", "Additional seat", 12),
    ("SKU-SSO", "SSO add-on", 99),
    ("SKU-AUDIT", "Audit log retention", 39),
    ("SKU-SUP", "Priority support", 149),
    ("SKU-API", "API overage (1M calls)", 20),
];

/// `timestamp()` in the preview: days and minutes after 2024-03-14 UTC.
fn timestamp(days: i64, minutes: i64) -> Value {
    let base = Utc.with_ymd_and_hms(2024, 3, 14, 0, 0, 0).unwrap();
    let at = base + Duration::days(days) + Duration::minutes(minutes);
    json!(at.format("%Y-%m-%d %H:%M:%S+00").to_string())
}

/// A JSON number that prints like JavaScript (`49`, not `49.0`).
fn num(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        json!(value as i64)
    } else {
        json!(value)
    }
}

/// The preview strips accents with NFD; these names only use a few.
fn ascii(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'è' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

fn seats_for(plan: &str, index: i64) -> i64 {
    match plan {
        "free" => 1,
        "team" => 3 + index % 5,
        "enterprise" => 48 + (index % 4) * 24,
        _ => 7 + index % 9,
    }
}

fn user_rows() -> Vec<Vec<Value>> {
    FIRST
        .iter()
        .enumerate()
        .map(|(i, first)| {
            let index = i as i64;
            let last = LAST[i];
            let plan = PLANS[i % PLANS.len()];
            let domain = DOMAINS[i % DOMAINS.len()];
            let local = match i % 3 {
                0 => format!("{first}.{last}"),
                1 => format!("{}.{last}", first.chars().next().unwrap_or_default()),
                _ => (*first).to_owned(),
            };
            let email = ascii(&format!("{local}@{domain}")).to_lowercase();
            let inactive = matches!(i, 7 | 10 | 14);
            vec![
                json!(1001 + index),
                if i == 7 { json!("old.account@example.com") } else { json!(email) },
                if i == 7 { Value::Null } else { json!(format!("{first} {last}")) },
                json!(plan),
                json!(seats_for(plan, index)),
                timestamp(index * 5, 540 + index * 37),
                json!(!inactive),
                json!({ "sso": plan == "enterprise" || plan == "team", "region": REGIONS[i % REGIONS.len()] }),
            ]
        })
        .collect()
}

fn spec(columns: &[(&str, &str, bool, Option<&str>)]) -> Vec<TableColumn> {
    columns
        .iter()
        .enumerate()
        .map(|(i, (name, data_type, nullable, default))| TableColumn {
            name: (*name).into(),
            data_type: (*data_type).into(),
            nullable: *nullable,
            default_value: default.map(Into::into),
            ordinal: i as i32 + 1,
        })
        .collect()
}

/// Columns, primary key, and rows (with `xmin` when keyed) for a table.
fn fixture_table(schema: &str, table: &str) -> (Vec<TableColumn>, Vec<String>, Vec<Vec<Value>>) {
    let key = |name: &str| vec![name.to_owned()];
    let text = "timestamp with time zone";
    let (columns, primary_key, rows) = match (schema, table) {
        ("keys", _) => (
            spec(&[
                ("key", "string", false, None),
                ("type", "string", false, None),
                ("ttl", "integer", false, None),
            ]),
            key("key"),
            vec![
                vec![json!("greeting"), json!("string"), json!(-1)],
                vec![json!("user:1"), json!("hash"), json!(-1)],
            ],
        ),
        ("string", key_name) => (
            spec(&[
                ("key", "string", false, None),
                ("value", "string", true, None),
            ]),
            key("key"),
            vec![vec![
                json!(key_name),
                json!(if key_name == "greeting" { "hello" } else { "" }),
            ]],
        ),
        ("hash", _) => (
            spec(&[
                ("field", "string", false, None),
                ("value", "string", true, None),
            ]),
            key("field"),
            vec![
                vec![json!("name"), json!("Ada")],
                vec![json!("role"), json!("engineer")],
            ],
        ),
        // Kept for unit tests; not listed in the schema tree.
        (_, "customers") => {
            let rows = (1..=500)
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
                .collect();
            return (
                vec![
                    column("id", "bigint", false, 1),
                    column("email", "text", false, 2),
                    column("active", "boolean", false, 3),
                    column("note", "text", true, 4),
                ],
                key("id"),
                rows,
            );
        }
        (_, "invoices") => (
            spec(&[
                ("number", "text", false, None),
                ("user_id", "bigint", false, None),
                ("status", "text", false, Some("'open'")),
                ("amount", "numeric", false, None),
                ("currency", "text", false, Some("'USD'")),
                ("issued_at", text, false, Some("now()")),
                ("paid_at", text, true, None),
            ]),
            key("number"),
            (0..36)
                .map(|i: i64| {
                    let status = STATUSES[i as usize % STATUSES.len()];
                    vec![
                        json!(format!("INV-{}", 24_081 + i)),
                        json!(1001 + (i * 7) % 40),
                        json!(status),
                        num(((i * 3779) % 90_000 + 4_900) as f64 / 100.0),
                        json!("USD"),
                        timestamp(120 + i * 3, 600 + i * 11),
                        if status == "paid" {
                            timestamp(122 + i * 3, 700 + i * 13)
                        } else {
                            Value::Null
                        },
                    ]
                })
                .collect(),
        ),
        (_, "orders") => (
            spec(&[
                ("id", "bigint", false, None),
                ("user_id", "bigint", false, None),
                ("status", "text", false, Some("'pending'")),
                ("quantity", "integer", false, None),
                ("total", "numeric", false, None),
                ("placed_at", text, false, Some("now()")),
            ]),
            key("id"),
            (0..40)
                .map(|i: i64| {
                    let statuses = [
                        "fulfilled",
                        "fulfilled",
                        "processing",
                        "fulfilled",
                        "refunded",
                        "pending",
                    ];
                    vec![
                        json!(58_210 + i),
                        json!(1001 + (i * 5) % 40),
                        json!(statuses[i as usize % 6]),
                        json!(1 + i % 4),
                        num(((i * 2311) % 40_000 + 1_200) as f64 / 100.0),
                        timestamp(200 + i, 400 + i * 17),
                    ]
                })
                .collect(),
        ),
        (_, "products") => (
            spec(&[
                ("sku", "text", false, None),
                ("name", "text", false, None),
                ("price", "numeric", false, None),
                ("active", "boolean", false, Some("true")),
            ]),
            key("sku"),
            PRODUCTS
                .iter()
                .enumerate()
                .map(|(i, (sku, name, price))| {
                    vec![json!(sku), json!(name), json!(price), json!(i != 7)]
                })
                .collect(),
        ),
        (_, "accounts") => (
            spec(&[
                ("id", "bigint", false, None),
                ("name", "text", false, None),
                ("owner_id", "bigint", false, None),
                ("created_at", text, false, None),
            ]),
            key("id"),
            DOMAINS
                .iter()
                .enumerate()
                .map(|(i, domain)| {
                    let stem = domain.split('.').next().unwrap_or_default();
                    let mut name = stem.to_owned();
                    if let Some(first) = name.get_mut(0..1) {
                        first.make_ascii_uppercase();
                    }
                    vec![
                        json!(501 + i),
                        json!(name),
                        json!(1001 + i),
                        timestamp(i as i64 * 9, 480),
                    ]
                })
                .collect(),
        ),
        (_, "api_keys") => (
            spec(&[
                ("id", "uuid", false, None),
                ("account_id", "bigint", false, None),
                ("label", "text", false, None),
                ("last_used_at", text, true, None),
            ]),
            key("id"),
            (0..12)
                .map(|i: i64| {
                    let tail = (700_000_000_000 + i * 7919).to_string();
                    let labels = ["CI deploys", "Billing sync", "Data export", "Mobile app"];
                    vec![
                        json!(format!("8f3c{i:02}a2-41d7-4c0e-9b1a-{}", &tail[..12])),
                        json!(501 + i % 17),
                        json!(labels[i as usize % 4]),
                        if i % 5 == 0 {
                            Value::Null
                        } else {
                            timestamp(500 + i, 300 + i * 41)
                        },
                    ]
                })
                .collect(),
        ),
        (_, "sessions") => (
            spec(&[
                ("id", "uuid", false, None),
                ("user_id", "bigint", false, None),
                ("ip", "inet", true, None),
                ("expires_at", text, false, None),
            ]),
            key("id"),
            (0..24)
                .map(|i: i64| {
                    let tail = (100_000_000_000 + i * 104_729).to_string();
                    vec![
                        json!(format!("c0ffee{i:02}-5e55-4a1d-8b00-{}", &tail[..12])),
                        json!(1001 + (i * 3) % 40),
                        json!(format!("10.24.{}.{}", i % 7, 40 + i * 3)),
                        timestamp(560 + i % 3, 60 * (i % 24)),
                    ]
                })
                .collect(),
        ),
        (_, "active_subscriptions") => (
            spec(&[
                ("user_id", "bigint", false, None),
                ("plan", "plan_tier", false, None),
                ("seats", "integer", false, None),
            ]),
            vec![],
            user_rows()
                .into_iter()
                .filter(|row| row[3] != json!("free") && row[6] == json!(true))
                .map(|row| vec![row[0].clone(), row[3].clone(), row[4].clone()])
                .collect(),
        ),
        (_, "monthly_revenue") => (
            spec(&[
                ("month", "date", false, None),
                ("mrr", "numeric", false, None),
                ("new_accounts", "integer", false, None),
            ]),
            vec![],
            (0..9)
                .map(|i: i64| {
                    vec![
                        json!(format!("2026-{:02}-01", i + 1)),
                        json!(48_200 + i * 3_450 + (i % 3) * 910),
                        json!(18 + (i * 7) % 13),
                    ]
                })
                .collect(),
        ),
        _ => (
            spec(&[
                ("id", "bigint", false, Some("nextval('users_id_seq')")),
                ("email", "text", false, None),
                ("full_name", "text", true, None),
                ("plan", "plan_tier", false, Some("'free'")),
                ("seats", "integer", false, Some("1")),
                ("created_at", text, false, Some("now()")),
                ("is_active", "boolean", false, Some("true")),
                ("metadata", "jsonb", true, None),
            ]),
            key("id"),
            user_rows(),
        ),
    };
    let rows = if primary_key.is_empty() {
        rows
    } else {
        rows.into_iter()
            .enumerate()
            .map(|(i, mut row)| {
                row.push(json!(format!("{}", 9_100 + i)));
                row
            })
            .collect()
    };
    (columns, primary_key, rows)
}

/// `demoQueryResponse()`: a revenue report over the first 12 named users.
fn revenue_report() -> (Vec<QueryColumn>, Vec<Vec<Value>>) {
    let column = |name: &str, data_type: &str| QueryColumn {
        name: name.into(),
        data_type: data_type.into(),
    };
    let rows = user_rows()
        .into_iter()
        .filter(|row| !row[2].is_null())
        .take(12)
        .enumerate()
        .map(|(i, row)| {
            let index = i as f64;
            let paid = 18_420.5 - index * 1_163.25 - (i % 3) as f64 * 87.4;
            vec![
                row[2].clone(),
                row[3].clone(),
                row[4].clone(),
                json!(14 - i as i64),
                num((paid * 100.0).round() / 100.0),
                json!(format!("2026-09-{:02}", 24 - i)),
            ]
        })
        .collect();
    (
        vec![
            column("full_name", "text"),
            column("plan", "plan_tier"),
            column("seats", "int4"),
            column("invoices", "int8"),
            column("paid_total", "numeric"),
            column("last_paid", "date"),
        ],
        rows,
    )
}

/// `demoHistory()`: Production's recent statements, newest first.
fn demo_history() -> Vec<QueryHistoryEntry> {
    let statements = [
        (
            "SELECT count(*) FROM sessions WHERE expires_at < now();",
            true,
        ),
        ("UPDATE users SET plan = 'team' WHERE id = 1012;", true),
        (
            "SELECT * FROM invoices WHERE status = 'open' ORDER BY issued_at;",
            true,
        ),
        ("SELECT plan, count(*) FROM users GROUP BY plan;", true),
        ("SELECT * FROM user_events LIMIT 50;", false),
        (
            "EXPLAIN ANALYZE SELECT * FROM orders WHERE user_id = 1003;",
            true,
        ),
    ];
    let start = Utc.with_ymd_and_hms(2026, 9, 25, 9, 40, 0).unwrap();
    statements
        .iter()
        .enumerate()
        .map(|(i, (sql, success))| QueryHistoryEntry {
            id: Uuid::from_u128(0x100 + i as u128),
            profile_id: Uuid::from_u128(1),
            database: "acme".into(),
            sql: (*sql).into(),
            executed_at: start - Duration::minutes(i as i64 * 7),
            duration_ms: 12 + i as u128 * 9,
            success: *success,
        })
        .collect()
}

fn contains_word(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|w| w == word)
}

/// A page of the fixture table named by `request`.
pub fn table_page(request: &TablePageRequest) -> TablePage {
    let (columns, primary_key, mut rows) = fixture_table(&request.schema, &request.table);
    let has_xmin = !primary_key.is_empty();
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
            primary_key,
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
        assert!(page.metadata.has_xmin);
        let store = DemoStore::new();
        assert_eq!(store.schema_tree(Uuid::from_u128(4))[0].name, "Keys");
        assert!(store.apply_mutations().is_err());
    }

    fn named(table: &str) -> TablePageRequest {
        TablePageRequest {
            table: table.into(),
            ..request("public", 0, 200)
        }
    }

    #[test]
    fn matches_the_desktop_preview_dataset() {
        let store = DemoStore::new();
        let names: Vec<_> = store.profiles().iter().map(|p| p.name.clone()).collect();
        assert_eq!(
            names,
            ["Production", "Staging", "Analytics", "Session cache"]
        );
        let users = table_page(&named("users"));
        assert_eq!(users.total_rows, Some(40));
        assert_eq!(users.metadata.columns.len(), 8);
        // Row 1002 as in the README screenshots, and Tomás's accent stripped.
        assert_eq!(users.rows[1][1], json!("l.brennan@fieldstone.io"));
        assert_eq!(users.rows[1][2], json!("Liam Brennan"));
        assert_eq!(users.rows[3][1], json!("tomas.vidal@corvid.co"));
        assert_eq!(users.rows[7][2], Value::Null);
        assert_eq!(users.rows[0][5], json!("2024-03-14 09:00:00+00"));
        assert_eq!(users.rows[0].last(), Some(&json!("9100")));
        assert_eq!(table_page(&named("invoices")).rows[0][3], json!(49));
        let view = table_page(&named("monthly_revenue"));
        assert!(view.metadata.primary_key.is_empty());
        assert!(!view.columns.contains(&"__dbm_xmin".to_owned()));
        let tree = store.schema_tree(Uuid::from_u128(1));
        assert_eq!(tree[0].children.len(), 9);
        assert_eq!(store.history(Uuid::from_u128(1), 10).len(), 6);
    }

    #[test]
    fn selects_return_the_preview_revenue_report() {
        let mut store = DemoStore::new();
        let response = store.run_query(&QueryRequest {
            profile_id: Uuid::from_u128(1),
            sql: "SELECT now();".into(),
            max_rows: None,
        });
        assert_eq!(response.rows.len(), 12);
        assert_eq!(response.rows[0][0], json!("Maya Okafor"));
        assert_eq!(response.rows[0][4], json!(18420.5));
        assert_eq!(response.duration_ms, 41);
    }
}
