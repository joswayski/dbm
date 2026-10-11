use std::error::Error as StdError;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("profile not found")]
    ProfileNotFound,
    #[error("workspace is not connected")]
    NotConnected,
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("credential error: {0}")]
    Credential(String),
    #[error("database error: {0}")]
    Database(String),
    #[error("unsupported operation: {0}")]
    Unsupported(String),
}

impl AppError {
    pub fn database(error: &(dyn StdError + 'static)) -> Self {
        let mut messages: Vec<String> = Vec::new();
        let mut current = Some(error);
        while let Some(source) = current {
            let message = source.to_string();
            // Some drivers include their entire source in Display already.
            // Keep distinct context, but do not append the same cause again.
            if !message.is_empty()
                && !messages.iter().any(|existing| {
                    existing == &message || existing.ends_with(&format!(": {message}"))
                })
            {
                messages.push(message);
            }
            current = source.source();
        }
        Self::Database(messages.join(": "))
    }

    pub fn is_connection_lost(&self) -> bool {
        let Self::Database(message) = self else {
            return false;
        };
        // Server messages and OS error text can be localized. Typed checks at
        // conversion add the CONNECTION_LOST prefix, and SQLSTATE codes are
        // stable: class 08 (connection exception), 57P01/57P02 (server shut
        // down), 57P05 (idle session timeout), 25P03 (idle-in-transaction
        // timeout). The English phrases remain as a fallback.
        if ["08", "57P01", "57P02", "57P05", "25P03"]
            .iter()
            .any(|code| message.contains(&format!("(SQLSTATE {code}")))
        {
            return true;
        }
        let message = message.to_ascii_lowercase();
        [
            CONNECTION_LOST,
            "connection closed",
            "connection reset",
            "connection aborted",
            "connection broken",
            "broken pipe",
            "not connected",
            "connection is closed",
            "terminating connection",
        ]
        .iter()
        .any(|needle| message.contains(needle))
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

/// Prefix for errors that typed checks identified as a lost connection.
const CONNECTION_LOST: &str = "connection lost";

/// Whether an I/O error in the source chain means an established connection
/// went away, independent of the (possibly localized) OS message.
fn has_lost_connection_io_error(error: &(dyn StdError + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(source) = current {
        if let Some(io) = source.downcast_ref::<std::io::Error>() {
            return matches!(
                io.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::NotConnected
                    | std::io::ErrorKind::UnexpectedEof
            );
        }
        current = source.source();
    }
    false
}

impl AppError {
    /// A database error, marked as a lost connection when typed checks say so
    /// even though its message would not.
    fn database_with_connection_state(error: &(dyn StdError + 'static), lost: bool) -> Self {
        let converted = Self::database(error);
        match converted {
            Self::Database(message)
                if lost && !Self::Database(message.clone()).is_connection_lost() =>
            {
                Self::Database(format!("{CONNECTION_LOST}: {message}"))
            }
            other => other,
        }
    }
}

impl From<tokio_postgres::Error> for AppError {
    fn from(error: tokio_postgres::Error) -> Self {
        if let Some(database_error) = error.as_db_error() {
            let mut message = format!(
                "{} (SQLSTATE {})",
                database_error.message(),
                database_error.code().code()
            );
            if let Some(detail) = database_error.detail() {
                message.push_str(&format!("\nDetail: {detail}"));
            }
            if let Some(hint) = database_error.hint() {
                message.push_str(&format!("\nHint: {hint}"));
            }
            Self::Database(message)
        } else {
            let lost = error.is_closed() || has_lost_connection_io_error(&error);
            Self::database_with_connection_state(&error, lost)
        }
    }
}

impl From<mysql_async::Error> for AppError {
    fn from(error: mysql_async::Error) -> Self {
        if let mysql_async::Error::Server(server) = &error {
            return Self::Database(format!("{} (SQLSTATE {})", server.message, server.state));
        }
        let lost = matches!(
            error,
            mysql_async::Error::Driver(mysql_async::DriverError::ConnectionClosed)
        ) || has_lost_connection_io_error(&error);
        Self::database_with_connection_state(&error, lost)
    }
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
impl From<keyring::Error> for AppError {
    fn from(error: keyring::Error) -> Self {
        Self::Credential(error.to_string())
    }
}

impl From<redis_rs::RedisError> for AppError {
    fn from(error: redis_rs::RedisError) -> Self {
        Self::database(&error)
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::AppError;

    #[test]
    fn database_errors_do_not_repeat_causes_already_in_the_message() {
        let cause =
            "failed to lookup address information: nodename nor servname provided, or not known";
        let error = mysql_async::Error::from(std::io::Error::other(cause));
        assert_eq!(
            AppError::from(error).to_string(),
            format!("database error: Input/output error: Input/output error: {cause}")
        );

        #[derive(Debug, thiserror::Error)]
        #[error("error connecting to server")]
        struct ConnectError(#[source] std::io::Error);

        let error = ConnectError(std::io::Error::other("connection refused"));
        assert_eq!(
            AppError::database(&error).to_string(),
            "database error: error connecting to server: connection refused"
        );
    }

    #[test]
    fn identifies_lost_database_connections() {
        assert!(AppError::Database("connection closed".into()).is_connection_lost());
        assert!(AppError::Database("Broken pipe".into()).is_connection_lost());
        assert!(
            AppError::Database(
                "terminating connection due to administrator command (SQLSTATE 57P01)".into()
            )
            .is_connection_lost()
        );
        assert!(!AppError::Database("syntax error at or near SELECT".into()).is_connection_lost());
        assert!(!AppError::NotConnected.is_connection_lost());
        // Localized server messages are recognized by their SQLSTATE.
        assert!(
            AppError::Database(
                "Beende Verbindung aufgrund von Anweisung des Administrators (SQLSTATE 57P01)"
                    .into()
            )
            .is_connection_lost()
        );
        assert!(AppError::Database("Lost connection (SQLSTATE 08S01)".into()).is_connection_lost());
        assert!(
            !AppError::Database("Syntaxfehler bei SELECT (SQLSTATE 42601)".into())
                .is_connection_lost()
        );
    }

    #[test]
    fn typed_io_errors_mark_lost_connections_whatever_the_os_message() {
        let localized = std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "Eine vorhandene Verbindung wurde vom Remotehost geschlossen. (os error 10054)",
        );
        let error = AppError::from(mysql_async::Error::Io(mysql_async::IoError::Io(localized)));
        assert!(error.is_connection_lost(), "{error}");
        let refused = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let error = AppError::from(mysql_async::Error::Io(mysql_async::IoError::Io(refused)));
        assert!(!error.is_connection_lost(), "{error}");
        let error = AppError::from(mysql_async::Error::Driver(
            mysql_async::DriverError::ConnectionClosed,
        ));
        assert!(error.is_connection_lost(), "{error}");
    }
}
