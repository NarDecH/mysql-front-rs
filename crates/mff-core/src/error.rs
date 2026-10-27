//! Unified error type for the whole library.

use thiserror::Error;

/// Errors produced by `mff-core`.
#[derive(Debug, Error)]
pub enum MffError {
    /// Error returned by the MySQL server / protocol layer.
    #[error("MySQL error: {0}")]
    MySql(#[from] mysql::Error),

    /// Filesystem error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Error while (de)serialising CSV data.
    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    /// Error while (de)serialising JSON data.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// The registry file has an unexpected structure.
    #[error("invalid registry file: {0}")]
    Registry(String),

    /// A connection with this name already exists.
    #[error("a connection named '{0}' already exists")]
    DuplicateName(String),

    /// No connection matched the given id or name.
    #[error("connection '{0}' not found")]
    ConnectionNotFound(String),

    /// Invalid parameter supplied by the caller.
    #[error("{0}")]
    Invalid(String),

    /// Anything else.
    #[error("{0}")]
    Other(String),
}

/// Convenient result alias used across the library.
pub type Result<T> = std::result::Result<T, MffError>;
