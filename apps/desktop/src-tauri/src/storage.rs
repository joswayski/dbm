use std::path::PathBuf;

#[cfg(test)]
use std::path::Path;

use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{ConnectionProfile, QueryHistoryEntry, SaveProfileInput, SshConfig, TlsMode};

#[derive(Debug, Clone)]
pub struct LocalStore {
    path: PathBuf,
}

impl LocalStore {
    pub fn new() -> AppResult<Self> {
        let project_dirs = ProjectDirs::from("io", "github", "dbv").ok_or_else(|| {
            AppError::Storage("could not determine application data directory".into())
        })?;
        let directory = project_dirs.data_local_dir();
        std::fs::create_dir_all(directory).map_err(|error| AppError::Storage(error.to_string()))?;
        let store = Self {
            path: directory.join("dbv.sqlite3"),
        };
        store.migrate()?;
        Ok(store)
    }

    #[cfg(test)]
    pub fn from_path(path: impl AsRef<Path>) -> AppResult<Self> {
        let store = Self {
            path: path.as_ref().to_path_buf(),
        };
        store.migrate()?;
        Ok(store)
    }

    fn open(&self) -> AppResult<Connection> {
        Ok(Connection::open(&self.path)?)
    }

    fn migrate(&self) -> AppResult<()> {
        let connection = self.open()?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS profiles (
                 id TEXT PRIMARY KEY NOT NULL,
                 name TEXT NOT NULL,
                 color TEXT,
                 host TEXT NOT NULL,
                 port INTEGER NOT NULL,
                 username TEXT NOT NULL,
                 default_database TEXT NOT NULL,
                 tls_mode TEXT NOT NULL,
                 ca_cert_path TEXT,
                 ssh_json TEXT,
                 read_only INTEGER NOT NULL DEFAULT 0,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS query_history (
                 id TEXT PRIMARY KEY NOT NULL,
                 profile_id TEXT NOT NULL,
                 database_name TEXT NOT NULL,
                 sql TEXT NOT NULL,
                 executed_at TEXT NOT NULL,
                 duration_ms INTEGER NOT NULL,
                 success INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS query_history_profile_time
                 ON query_history(profile_id, executed_at DESC);",
        )?;
        Ok(())
    }

    pub fn list_profiles(&self) -> AppResult<Vec<ConnectionProfile>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT id, name, color, host, port, username, default_database,
                    tls_mode, ca_cert_path, ssh_json, read_only, created_at, updated_at
             FROM profiles ORDER BY name COLLATE NOCASE, id",
        )?;
        let rows = statement.query_map([], profile_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn get_profile(&self, id: Uuid) -> AppResult<Option<ConnectionProfile>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT id, name, color, host, port, username, default_database,
                    tls_mode, ca_cert_path, ssh_json, read_only, created_at, updated_at
             FROM profiles WHERE id = ?1",
        )?;
        statement
            .query_row([id.to_string()], profile_from_row)
            .optional()
            .map_err(AppError::from)
    }

    pub fn save_profile(&self, input: &SaveProfileInput) -> AppResult<ConnectionProfile> {
        let id = input.id.unwrap_or_else(Uuid::new_v4);
        let now = Utc::now();
        let created_at = self
            .get_profile(id)?
            .map_or(now, |profile| profile.created_at);
        let profile = ConnectionProfile {
            id,
            name: input.name.trim().to_owned(),
            color: input.color.clone(),
            host: input.host.trim().to_owned(),
            port: input.port,
            username: input.username.trim().to_owned(),
            default_database: input.default_database.trim().to_owned(),
            tls_mode: input.tls_mode.clone(),
            ca_cert_path: input.ca_cert_path.clone(),
            ssh: input.ssh.clone(),
            read_only: input.read_only,
            created_at,
            updated_at: now,
        };
        if profile.name.is_empty() || profile.host.is_empty() || profile.username.is_empty() {
            return Err(AppError::InvalidInput(
                "name, host, and username are required".into(),
            ));
        }
        if profile.default_database.is_empty() {
            return Err(AppError::InvalidInput("database is required".into()));
        }

        let ssh_json = profile
            .ssh
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| AppError::Storage(error.to_string()))?;
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO profiles (
                id, name, color, host, port, username, default_database, tls_mode,
                ca_cert_path, ssh_json, read_only, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                color = excluded.color,
                host = excluded.host,
                port = excluded.port,
                username = excluded.username,
                default_database = excluded.default_database,
                tls_mode = excluded.tls_mode,
                ca_cert_path = excluded.ca_cert_path,
                ssh_json = excluded.ssh_json,
                read_only = excluded.read_only,
                updated_at = excluded.updated_at",
            params![
                profile.id.to_string(),
                profile.name,
                profile.color,
                profile.host,
                i64::from(profile.port),
                profile.username,
                profile.default_database,
                tls_mode_to_string(&profile.tls_mode),
                profile.ca_cert_path,
                ssh_json,
                i64::from(u8::from(profile.read_only)),
                profile.created_at.to_rfc3339(),
                profile.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(profile)
    }

    pub fn delete_profile(&self, id: Uuid) -> AppResult<()> {
        let connection = self.open()?;
        connection.execute("DELETE FROM profiles WHERE id = ?1", [id.to_string()])?;
        connection.execute(
            "DELETE FROM query_history WHERE profile_id = ?1",
            [id.to_string()],
        )?;
        Ok(())
    }

    pub fn add_history(&self, entry: &QueryHistoryEntry) -> AppResult<()> {
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO query_history (
                id, profile_id, database_name, sql, executed_at, duration_ms, success
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.id.to_string(),
                entry.profile_id.to_string(),
                entry.database,
                entry.sql,
                entry.executed_at.to_rfc3339(),
                i64::try_from(entry.duration_ms).unwrap_or(i64::MAX),
                i64::from(u8::from(entry.success)),
            ],
        )?;
        connection.execute(
            "DELETE FROM query_history
             WHERE profile_id = ?1 AND id NOT IN (
                 SELECT id FROM query_history
                 WHERE profile_id = ?1 ORDER BY executed_at DESC LIMIT 500
             )",
            [entry.profile_id.to_string()],
        )?;
        Ok(())
    }

    pub fn list_history(&self, profile_id: Uuid, limit: u32) -> AppResult<Vec<QueryHistoryEntry>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT id, profile_id, database_name, sql, executed_at, duration_ms, success
             FROM query_history WHERE profile_id = ?1
             ORDER BY executed_at DESC LIMIT ?2",
        )?;
        let rows =
            statement.query_map(params![profile_id.to_string(), i64::from(limit)], |row| {
                let id = parse_uuid(row.get::<_, String>(0)?)?;
                let profile_id = parse_uuid(row.get::<_, String>(1)?)?;
                let executed_at = parse_datetime(row.get::<_, String>(4)?)?;
                let duration_ms = u128::try_from(row.get::<_, i64>(5)?).unwrap_or_default();
                Ok(QueryHistoryEntry {
                    id,
                    profile_id,
                    database: row.get(2)?,
                    sql: row.get(3)?,
                    executed_at,
                    duration_ms,
                    success: row.get::<_, i64>(6)? != 0,
                })
            })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }
}

fn profile_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConnectionProfile> {
    let tls_mode = match row.get::<_, String>(7)?.as_str() {
        "disabled" => TlsMode::Disabled,
        "required" => TlsMode::Required,
        _ => TlsMode::Preferred,
    };
    let ssh = row
        .get::<_, Option<String>>(9)?
        .map(|json| serde_json::from_str::<SshConfig>(&json))
        .transpose()
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                9,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    Ok(ConnectionProfile {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        name: row.get(1)?,
        color: row.get(2)?,
        host: row.get(3)?,
        port: u16::try_from(row.get::<_, i64>(4)?).unwrap_or(5432),
        username: row.get(5)?,
        default_database: row.get(6)?,
        tls_mode,
        ca_cert_path: row.get(8)?,
        ssh,
        read_only: row.get::<_, i64>(10)? != 0,
        created_at: parse_datetime(row.get::<_, String>(11)?)?,
        updated_at: parse_datetime(row.get::<_, String>(12)?)?,
    })
}

fn parse_uuid(value: String) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

fn parse_datetime(value: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&value)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
}

fn tls_mode_to_string(mode: &TlsMode) -> &'static str {
    match mode {
        TlsMode::Disabled => "disabled",
        TlsMode::Preferred => "preferred",
        TlsMode::Required => "required",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_round_trip() {
        let path = std::env::temp_dir().join(format!("dbv-test-{}.sqlite3", Uuid::new_v4()));
        let store = LocalStore::from_path(&path).expect("store");
        let input = SaveProfileInput {
            id: None,
            name: "Local".into(),
            color: Some("#22c55e".into()),
            host: "localhost".into(),
            port: 5432,
            username: "postgres".into(),
            default_database: "postgres".into(),
            tls_mode: TlsMode::Disabled,
            ca_cert_path: None,
            ssh: None,
            read_only: false,
            password: None,
        };
        let profile = store.save_profile(&input).expect("save");
        let profiles = store.list_profiles().expect("list");
        assert_eq!(profiles, vec![profile]);
        std::fs::remove_file(path).expect("remove temp db");
    }
}
