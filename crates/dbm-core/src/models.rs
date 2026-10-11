use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseEngine {
    #[default]
    Postgres,
    Mysql,
    Redis,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TlsMode {
    Disabled,
    #[default]
    Preferred,
    Required,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub private_key_path: Option<String>,
    pub use_agent: bool,
    pub password_auth: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfile {
    pub id: Uuid,
    pub name: String,
    pub color: Option<String>,
    #[serde(default)]
    pub engine: DatabaseEngine,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub default_database: String,
    pub tls_mode: TlsMode,
    pub ca_cert_path: Option<String>,
    pub ssh: Option<SshConfig>,
    pub read_only: bool,
    #[serde(default = "default_open_on_startup")]
    pub open_on_startup: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ConnectionProfile {
    pub fn validate(&self) -> AppResult<()> {
        if self.name.is_empty() || self.host.is_empty() {
            return Err(AppError::InvalidInput(
                "name, host, and username are required".into(),
            ));
        }
        if self.engine != DatabaseEngine::Redis && self.username.is_empty() {
            return Err(AppError::InvalidInput(
                "name, host, and username are required".into(),
            ));
        }
        if self.engine == DatabaseEngine::Redis {
            parse_redis_database(&self.default_database)?;
        } else if self.default_database.is_empty() {
            return Err(AppError::InvalidInput("database is required".into()));
        }
        if self.port == 0 {
            return Err(AppError::InvalidInput(
                "port must be between 1 and 65535".into(),
            ));
        }
        Ok(())
    }
}

/// Largest integer a JavaScript number represents exactly (`Number.MAX_SAFE_INTEGER`).
const MAX_SAFE_JSON_INTEGER: u64 = (1 << 53) - 1;

/// Serializes an integer for the UI, falling back to a string when a JavaScript
/// number would round it (for example, 64-bit snowflake IDs).
pub fn json_integer(value: i64) -> Value {
    if value.unsigned_abs() <= MAX_SAFE_JSON_INTEGER {
        Value::from(value)
    } else {
        Value::String(value.to_string())
    }
}

pub fn json_unsigned(value: u64) -> Value {
    if value <= MAX_SAFE_JSON_INTEGER {
        Value::from(value)
    } else {
        Value::String(value.to_string())
    }
}

/// Escapes `LIKE` wildcards so filters match the user's text literally. Pair
/// the pattern with `ESCAPE '!'`, which behaves the same in PostgreSQL and MySQL.
pub fn escape_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '!' | '%' | '_') {
            escaped.push('!');
        }
        escaped.push(character);
    }
    escaped
}

/// The lowercased words of each statement in a PostgreSQL or MySQL script,
/// for the read-only profile guard. Comments are dropped (MySQL `/*! ... */`
/// executable comments are kept), quoted strings and identifiers become a
/// placeholder, and `;` separates statements. A string literal that names a
/// `*read_only` setting keeps its text so `set_config('...read_only', ...)` is
/// still recognized. This is a guard rail, not a parser: where dialects are
/// ambiguous, such as PostgreSQL dollar quotes, text stays visible, which can
/// only make the guard stricter.
pub(crate) fn sql_statement_words(engine: DatabaseEngine, sql: &str) -> Vec<Vec<String>> {
    let mysql = engine == DatabaseEngine::Mysql;
    let chars = sql.chars().collect::<Vec<_>>();
    let mut statements = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut index = 0;
    while index < chars.len() {
        let character = chars[index];
        let next = chars.get(index + 1).copied();
        if character.is_alphanumeric() || character == '_' || (character == '$' && !word.is_empty())
        {
            word.extend(character.to_lowercase());
            index += 1;
            continue;
        }
        let escape_string = !mysql && character == '\'' && word == "e";
        if escape_string {
            word.clear();
        }
        if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        match character {
            ';' => {
                if !words.is_empty() {
                    statements.push(std::mem::take(&mut words));
                }
                index += 1;
            }
            '-' if next == Some('-')
                && (!mysql
                    || chars
                        .get(index + 2)
                        .is_none_or(|after| after.is_whitespace() || after.is_control())) =>
            {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
            }
            '#' if mysql => {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
            }
            '/' if next == Some('*') => {
                let executable = mysql
                    && (chars.get(index + 2) == Some(&'!')
                        || (chars.get(index + 2) == Some(&'M')
                            && chars.get(index + 3) == Some(&'!')));
                if executable {
                    // MySQL and MariaDB run the text of `/*!50000 ... */`. Skip
                    // the marker and version; `*/` is then plain punctuation.
                    index += if chars[index + 2] == 'M' { 4 } else { 3 };
                    while chars.get(index).is_some_and(char::is_ascii_digit) {
                        index += 1;
                    }
                    continue;
                }
                // PostgreSQL block comments nest; MySQL's do not.
                let mut depth = 0usize;
                while index < chars.len() {
                    if chars[index] == '/' && chars.get(index + 1) == Some(&'*') {
                        depth += 1;
                        index += 2;
                        if mysql {
                            depth = 1;
                        }
                    } else if chars[index] == '*' && chars.get(index + 1) == Some(&'/') {
                        depth -= 1;
                        index += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        index += 1;
                    }
                }
            }
            '\'' | '"' | '`' => {
                let quote = character;
                let backslash_escapes = escape_string || (mysql && quote != '`');
                let mut text = String::new();
                index += 1;
                while index < chars.len() {
                    let current = chars[index];
                    if backslash_escapes && current == '\\' {
                        text.extend(chars.get(index + 1));
                        index += 2;
                    } else if current == quote {
                        if chars.get(index + 1) == Some(&quote) {
                            text.push(quote);
                            index += 2;
                        } else {
                            index += 1;
                            break;
                        }
                    } else {
                        text.push(current);
                        index += 1;
                    }
                }
                let text = text.trim().to_lowercase();
                let is_string = quote == '\'' || (mysql && quote == '"');
                words.push(if is_string && text.ends_with("read_only") {
                    text
                } else {
                    quote.to_string()
                });
            }
            '$' if !mysql && next.is_none_or(|next| !next.is_ascii_digit()) => {
                // A PostgreSQL dollar-quote delimiter such as `$$` or `$body$`.
                // Its body stays visible so `DO` blocks are checked too.
                let mut end = index + 1;
                while chars
                    .get(end)
                    .is_some_and(|tag| tag.is_alphanumeric() || *tag == '_')
                {
                    end += 1;
                }
                index = if chars.get(end) == Some(&'$') {
                    end + 1
                } else {
                    index + 1
                };
            }
            _ => index += 1,
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    if !words.is_empty() {
        statements.push(words);
    }
    statements
}

/// Whether a statement would undo the server-side read-only mode of a
/// read-only profile's session, letting later statements write:
/// `BEGIN READ WRITE`, `SET SESSION CHARACTERISTICS AS TRANSACTION READ WRITE`,
/// `SET transaction_read_only = 0`, `RESET ALL`, `DISCARD ALL`, or
/// `set_config('default_transaction_read_only', ...)`.
pub(crate) fn reenables_writes(words: &[String]) -> bool {
    let pair = |first: &str, second: &str| {
        words
            .windows(2)
            .any(|pair| pair[0] == first && pair[1] == second)
    };
    let changes_setting = words
        .iter()
        .any(|word| matches!(word.as_str(), "set" | "reset" | "set_config"));
    pair("read", "write")
        || pair("reset", "all")
        || pair("discard", "all")
        || (changes_setting && words.iter().any(|word| word.ends_with("read_only")))
}

pub fn parse_redis_database(value: &str) -> AppResult<i64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput(
            "Redis database index is required".into(),
        ));
    }
    let db = trimmed.parse::<i64>().map_err(|_| {
        AppError::InvalidInput("Redis database must be a non-negative integer".into())
    })?;
    if db < 0 {
        return Err(AppError::InvalidInput(
            "Redis database must be a non-negative integer".into(),
        ));
    }
    Ok(db)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProfileInput {
    pub id: Option<Uuid>,
    pub name: String,
    pub color: Option<String>,
    #[serde(default)]
    pub engine: DatabaseEngine,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub default_database: String,
    #[serde(default)]
    pub tls_mode: TlsMode,
    pub ca_cert_path: Option<String>,
    pub ssh: Option<SshConfig>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default = "default_open_on_startup")]
    pub open_on_startup: bool,
    pub password: Option<String>,
}

fn default_open_on_startup() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub profile: ConnectionProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseRef {
    pub name: String,
    pub is_template: bool,
    pub is_connectable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub profile: ConnectionProfile,
    pub databases: Vec<DatabaseRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaNode {
    pub name: String,
    pub kind: String,
    pub schema: Option<String>,
    pub table: Option<String>,
    pub children: Vec<SchemaNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub ordinal: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableMetadata {
    pub schema: String,
    pub table: String,
    pub columns: Vec<TableColumn>,
    pub primary_key: Vec<String>,
    pub has_xmin: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterOperator {
    Equals,
    NotEquals,
    Contains,
    StartsWith,
    EndsWith,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    In,
    NotIn,
    IsNull,
    IsNotNull,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterCondition {
    pub column: String,
    pub operator: FilterOperator,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderSpec {
    pub column: String,
    pub descending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TablePageRequest {
    pub profile_id: Uuid,
    pub schema: String,
    pub table: String,
    pub offset: u32,
    pub limit: u32,
    pub filters: Vec<FilterCondition>,
    pub order_by: Option<OrderSpec>,
    pub include_total: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TablePage {
    pub metadata: TableMetadata,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub total_rows: Option<u64>,
    pub offset: u32,
    pub limit: u32,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowMutation {
    pub original: Vec<Value>,
    pub changes: Vec<Value>,
    pub primary_key: Vec<Value>,
    pub xmin: Option<String>,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationBatch {
    pub profile_id: Uuid,
    /// The database the table was opened on, captured before queueing a write.
    pub database: String,
    pub schema: String,
    pub table: String,
    pub mutations: Vec<RowMutation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationResult {
    pub applied: usize,
    pub conflicts: Vec<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRequest {
    pub profile_id: Uuid,
    pub sql: String,
    pub max_rows: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryColumn {
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponse {
    pub columns: Vec<QueryColumn>,
    pub rows: Vec<Vec<Value>>,
    pub row_count: usize,
    pub affected_rows: Option<u64>,
    pub duration_ms: u128,
    pub truncated: bool,
    pub notices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryHistoryEntry {
    pub id: Uuid,
    pub profile_id: Uuid,
    pub database: String,
    pub sql: String,
    pub executed_at: DateTime<Utc>,
    pub duration_ms: u128,
    pub success: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reenables(engine: DatabaseEngine, sql: &str) -> bool {
        sql_statement_words(engine, sql)
            .iter()
            .any(|words| reenables_writes(words))
    }

    #[test]
    fn statement_words_skip_comments_and_literals() {
        assert_eq!(
            sql_statement_words(
                DatabaseEngine::Postgres,
                "/* a /* nested */ note */ SELECT 'x;y' AS \"read write\"; -- tail\n DELETE FROM t"
            ),
            vec![vec!["select", "'", "as", "\""], vec!["delete", "from", "t"],]
        );
        assert_eq!(
            sql_statement_words(DatabaseEngine::Mysql, "# note\nSELECT 1--1, 'it\\'s'"),
            vec![vec!["select", "1", "1", "'"]]
        );
        assert_eq!(
            sql_statement_words(DatabaseEngine::Postgres, "SELECT E'\\'; DELETE'"),
            vec![vec!["select", "'"]]
        );
    }

    #[test]
    fn read_only_guard_catches_statements_that_reenable_writes() {
        use DatabaseEngine::{Mysql, Postgres};
        for sql in [
            "BEGIN READ WRITE",
            "begin   isolation level serializable,\n read\twrite",
            "START TRANSACTION READ WRITE",
            "SET SESSION CHARACTERISTICS AS TRANSACTION READ WRITE",
            "SET TRANSACTION READ /* sneaky */ WRITE",
            "SET default_transaction_read_only = off",
            "set session default_transaction_read_only to false",
            "SET LOCAL transaction_read_only = off",
            "RESET default_transaction_read_only",
            "RESET ALL",
            "DISCARD ALL",
            "SELECT 1; RESET ALL",
            "SELECT set_config('default_transaction_read_only', 'off', false)",
            "DO $$ BEGIN SET SESSION CHARACTERISTICS AS TRANSACTION READ WRITE; END $$",
            "/* note */ discard all",
        ] {
            assert!(reenables(Postgres, sql), "{sql}");
        }
        for sql in [
            "SET SESSION TRANSACTION READ WRITE",
            "SET transaction_read_only = 0",
            "SET @@session.transaction_read_only = OFF",
            "SET SESSION tx_read_only = 0",
            "START TRANSACTION READ WRITE",
            "/*!40101 SET SESSION TRANSACTION READ WRITE */",
            "SELECT 1; SET transaction_read_only=0",
        ] {
            assert!(reenables(Mysql, sql), "{sql}");
        }
        for (engine, sql) in [
            (Postgres, "SELECT * FROM users"),
            (Postgres, "SHOW default_transaction_read_only"),
            (Postgres, "SELECT current_setting('transaction_read_only')"),
            (
                Postgres,
                "SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY",
            ),
            (Postgres, "BEGIN READ ONLY"),
            (Postgres, "SELECT 'read write' -- read write\n"),
            (Postgres, "SET search_path = app"),
            (Mysql, "SELECT @@transaction_read_only"),
            (Mysql, "SET @x = 1"),
            (Mysql, "START TRANSACTION READ ONLY"),
            (Mysql, "SELECT 1 # read write"),
        ] {
            assert!(!reenables(engine, sql), "{sql}");
        }
    }

    #[test]
    fn large_integers_stay_exact_for_javascript() {
        assert_eq!(json_integer(42), Value::from(42));
        assert_eq!(
            json_integer(-9_007_199_254_740_991),
            Value::from(-9_007_199_254_740_991_i64)
        );
        assert_eq!(
            json_integer(1_234_567_890_123_456_789),
            Value::String("1234567890123456789".into())
        );
        assert_eq!(
            json_unsigned(u64::MAX),
            Value::String("18446744073709551615".into())
        );
    }

    #[test]
    fn like_wildcards_are_escaped() {
        assert_eq!(escape_like("100%_off!"), "100!%!_off!!");
        assert_eq!(escape_like("plain"), "plain");
    }
}
