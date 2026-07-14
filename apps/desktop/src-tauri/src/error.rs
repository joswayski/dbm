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

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

impl From<tokio_postgres::Error> for AppError {
    fn from(error: tokio_postgres::Error) -> Self {
        Self::Database(error.to_string())
    }
}

impl From<keyring::Error> for AppError {
    fn from(error: keyring::Error) -> Self {
        Self::Credential(error.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
