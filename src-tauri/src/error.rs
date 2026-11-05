use serde::Serialize;
use specta::Type;

/// Errors crossing the IPC boundary. Serialized as `{ kind, message }`.
#[derive(Debug, thiserror::Error, Serialize, Type)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum MarginError {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("could not read {0}")]
    Parse(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("no project is open")]
    NoProject,
    /// The file changed on disk since the editor loaded it.
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, MarginError>;

impl From<std::io::Error> for MarginError {
    fn from(e: std::io::Error) -> Self {
        MarginError::Io(e.to_string())
    }
}

impl From<serde_norway::Error> for MarginError {
    fn from(e: serde_norway::Error) -> Self {
        MarginError::Parse(e.to_string())
    }
}

impl From<serde_json::Error> for MarginError {
    fn from(e: serde_json::Error) -> Self {
        MarginError::Parse(e.to_string())
    }
}
