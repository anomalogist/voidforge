use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("command `{cmd}` failed: {message}")]
    Command {
        cmd: String,
        message: String,
        code: Option<i32>,
    },
    #[error("operation cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn invalid<T: Into<String>>(message: T) -> Error {
    Error::Invalid(message.into())
}

pub fn not_found<T: Into<String>>(message: T) -> Error {
    Error::NotFound(message.into())
}
