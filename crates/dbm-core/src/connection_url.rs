//! Connection URL import shared by the native profile editors, mirroring
//! `apps/desktop/ui/src/connectionUrl.ts`.

use percent_encoding::percent_decode_str;
use serde::Serialize;
use url::Url;

use crate::models::{DatabaseEngine, TlsMode};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedConnection {
    pub engine: DatabaseEngine,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub default_database: String,
    pub tls_mode: TlsMode,
    pub password: Option<String>,
    pub suggested_name: String,
}

/// Default port and database for an engine.
pub fn engine_defaults(engine: DatabaseEngine) -> (u16, &'static str) {
    match engine {
        DatabaseEngine::Postgres => (5432, "postgres"),
        DatabaseEngine::Mysql => (3306, "mysql"),
        DatabaseEngine::Redis => (6379, "0"),
    }
}

pub fn parse_connection_url(value: &str) -> Result<ImportedConnection, String> {
    let url = Url::parse(value.trim()).map_err(|_| "Enter a valid connection URL.".to_owned())?;
    let engine = match url.scheme() {
        "postgres" | "postgresql" => DatabaseEngine::Postgres,
        "mysql" | "mariadb" => DatabaseEngine::Mysql,
        "redis" | "rediss" | "valkey" | "valkeys" => DatabaseEngine::Redis,
        _ => {
            return Err("The connection URL must begin with postgres://, postgresql://, mysql://, mariadb://, redis://, rediss://, valkey://, or valkeys://.".into());
        }
    };
    let (default_port, default_database) = engine_defaults(engine);
    let host = url
        .host_str()
        .unwrap_or("")
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let username = decode(url.username(), "username")?;
    if host.is_empty() {
        return Err("The connection URL must include a host.".into());
    }
    if engine != DatabaseEngine::Redis && username.is_empty() {
        return Err("The connection URL must include a host and username.".into());
    }
    let port = match url.port() {
        Some(0) => return Err("The connection URL contains an invalid port.".into()),
        Some(port) => port,
        None => default_port,
    };
    let path = decode(url.path().trim_start_matches('/'), "database")?;
    let default_database = if path.is_empty() {
        default_database.to_owned()
    } else {
        path
    };
    let password = url
        .password()
        .filter(|p| !p.is_empty())
        .map(|p| decode(p, "password"))
        .transpose()?;
    Ok(ImportedConnection {
        suggested_name: format!("{default_database} @ {host}"),
        engine,
        host,
        port,
        username,
        default_database,
        tls_mode: tls_mode(&url),
        password,
    })
}

fn tls_mode(url: &Url) -> TlsMode {
    if matches!(url.scheme(), "rediss" | "valkeys") {
        return TlsMode::Required;
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.to_lowercase())
    };
    let ssl_mode = param("sslmode")
        .or_else(|| param("ssl-mode"))
        .or_else(|| param("sslMode"));
    match ssl_mode.as_deref() {
        Some("disable" | "disabled") => TlsMode::Disabled,
        Some(
            "require" | "required" | "verify_ca" | "verify-ca" | "verify_identity"
            | "verify-identity" | "verify-full",
        ) => TlsMode::Required,
        _ if param("ssl").as_deref() == Some("true") => TlsMode::Required,
        _ => TlsMode::Preferred,
    }
}

fn decode(value: &str, field: &str) -> Result<String, String> {
    percent_decode_str(value)
        .decode_utf8()
        .map(|decoded| decoded.into_owned())
        .map_err(|_| format!("The connection URL contains an invalid {field}."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_postgres_mysql_and_redis_urls() {
        let pg = parse_connection_url(
            " postgresql://me:p%40ss@db.example.com:6543/app?sslmode=require ",
        )
        .unwrap();
        assert_eq!(
            pg,
            ImportedConnection {
                engine: DatabaseEngine::Postgres,
                host: "db.example.com".into(),
                port: 6543,
                username: "me".into(),
                default_database: "app".into(),
                tls_mode: TlsMode::Required,
                password: Some("p@ss".into()),
                suggested_name: "app @ db.example.com".into(),
            }
        );
        let mysql = parse_connection_url("mariadb://root@[::1]/?ssl-mode=DISABLED").unwrap();
        assert_eq!(
            (mysql.engine, mysql.host.as_str(), mysql.port),
            (DatabaseEngine::Mysql, "::1", 3306)
        );
        assert_eq!(
            (mysql.default_database.as_str(), mysql.tls_mode),
            ("mysql", TlsMode::Disabled)
        );
        assert_eq!(mysql.password, None);
        let redis = parse_connection_url("rediss://cache.local/3").unwrap();
        assert_eq!(
            (redis.engine, redis.username.as_str()),
            (DatabaseEngine::Redis, "")
        );
        assert_eq!(
            (redis.default_database.as_str(), redis.tls_mode),
            ("3", TlsMode::Required)
        );
    }

    #[test]
    fn rejects_unsupported_or_incomplete_urls() {
        assert!(parse_connection_url("not a url").is_err());
        assert!(
            parse_connection_url("http://x/y")
                .unwrap_err()
                .contains("must begin with")
        );
        assert!(
            parse_connection_url("postgres://db.example.com/app")
                .unwrap_err()
                .contains("username")
        );
        assert!(
            parse_connection_url("postgres://me@db:0/app")
                .unwrap_err()
                .contains("port")
        );
    }
}
