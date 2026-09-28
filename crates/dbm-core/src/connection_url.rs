//! Connection URL import and export shared by the native profile editors.

use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use serde::Serialize;
use url::Url;

use crate::models::{DatabaseEngine, SaveProfileInput, TlsMode};

/// Formats editor settings without persisting or logging credentials. Passwords
/// are supplied explicitly by the caller, never implicitly taken from the form.
pub fn format_connection_url(
    input: &SaveProfileInput,
    password: Option<&str>,
) -> Result<String, String> {
    let profile = crate::demo::profile_from_input(input).map_err(|error| error.to_string())?;
    let scheme = match profile.engine {
        DatabaseEngine::Postgres => "postgresql",
        DatabaseEngine::Mysql => "mysql",
        DatabaseEngine::Redis if profile.tls_mode != TlsMode::Disabled => "rediss",
        DatabaseEngine::Redis => "redis",
    };
    let encode = |value: &str| utf8_percent_encode(value, NON_ALPHANUMERIC).to_string();
    let host = profile.host.trim_start_matches('[').trim_end_matches(']');
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let host = url::Host::parse(&host).map_err(|_| "Enter a valid connection host.".to_owned())?;
    let mut authority = encode(&profile.username);
    if let Some(password) = password.filter(|value| !value.is_empty()) {
        authority.push(':');
        authority.push_str(&encode(password));
    }
    if !authority.is_empty() {
        authority.push('@');
    }
    let mut url = Url::parse(&format!(
        "{scheme}://{authority}{host}:{}/{}",
        profile.port,
        encode(&profile.default_database)
    ))
    .map_err(|_| "Cannot create a URL from these connection settings.".to_owned())?;
    match profile.engine {
        DatabaseEngine::Postgres => {
            url.query_pairs_mut().append_pair(
                "sslmode",
                match profile.tls_mode {
                    TlsMode::Disabled => "disable",
                    TlsMode::Preferred => "prefer",
                    TlsMode::Required => "require",
                },
            );
            if let Some(ca) = profile.ca_cert_path.as_deref() {
                url.query_pairs_mut().append_pair("sslrootcert", ca);
            }
        }
        DatabaseEngine::Mysql => {
            url.query_pairs_mut().append_pair(
                "ssl-mode",
                match profile.tls_mode {
                    TlsMode::Disabled => "DISABLED",
                    TlsMode::Preferred => "PREFERRED",
                    TlsMode::Required => "REQUIRED",
                },
            );
            if let Some(ca) = profile.ca_cert_path.as_deref() {
                url.query_pairs_mut().append_pair("ssl-ca", ca);
            }
        }
        DatabaseEngine::Redis => {}
    }
    Ok(url.into())
}

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

    fn input(engine: DatabaseEngine) -> SaveProfileInput {
        SaveProfileInput {
            id: None,
            name: "Test".into(),
            color: None,
            engine,
            host: "::1".into(),
            port: 6543,
            username: "a@b".into(),
            default_database: "app/data ?#é".into(),
            tls_mode: TlsMode::Required,
            ca_cert_path: None,
            ssh: None,
            read_only: false,
            password: Some("must not be included implicitly".into()),
        }
    }

    #[test]
    fn exports_encoded_credentials_database_ipv6_and_tls() {
        let input = input(DatabaseEngine::Postgres);
        let url = format_connection_url(&input, Some("p@ss:/?#% é")).unwrap();
        assert_eq!(
            url,
            "postgresql://a%40b:p%40ss%3A%2F%3F%23%25%20%C3%A9@[::1]:6543/app%2Fdata%20%3F%23%C3%A9?sslmode=require"
        );
        let imported = parse_connection_url(&url).unwrap();
        assert_eq!(imported.password.as_deref(), Some("p@ss:/?#% é"));
        assert_eq!(imported.default_database, "app/data ?#é");
        assert_eq!(imported.host, "::1");
        assert_eq!(
            format_connection_url(&input, None).unwrap(),
            "postgresql://a%40b@[::1]:6543/app%2Fdata%20%3F%23%C3%A9?sslmode=require"
        );
    }

    #[test]
    fn exports_engine_tls_options_and_rejects_invalid_hosts() {
        let mut input = input(DatabaseEngine::Mysql);
        input.host = "db.example.com".into();
        input.username = "root".into();
        input.default_database = "app".into();
        for (mode, expected) in [
            (TlsMode::Disabled, "DISABLED"),
            (TlsMode::Preferred, "PREFERRED"),
            (TlsMode::Required, "REQUIRED"),
        ] {
            input.tls_mode = mode;
            assert_eq!(
                format_connection_url(&input, None).unwrap(),
                format!("mysql://root@db.example.com:6543/app?ssl-mode={expected}")
            );
        }
        input.engine = DatabaseEngine::Redis;
        input.username.clear();
        input.default_database = "3".into();
        assert_eq!(
            format_connection_url(&input, Some("secret")).unwrap(),
            "rediss://:secret@db.example.com:6543/3"
        );
        input.tls_mode = TlsMode::Disabled;
        assert_eq!(
            format_connection_url(&input, None).unwrap(),
            "redis://db.example.com:6543/3"
        );
        input.host = "db.example.com/other".into();
        assert!(format_connection_url(&input, None).is_err());
    }

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
