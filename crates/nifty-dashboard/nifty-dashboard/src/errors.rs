use anyhow::anyhow;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::Serialize;
use std::{backtrace::Backtrace, io};
use thiserror::Error;

//
// CLI errors
//

#[derive(Debug, Error)]
pub enum CliError {
    /// I/O errors (file/FD issues etc.)
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Argument / parsing issues (usually clap output).
    /// The message is printed verbatim so clap's formatting is preserved.
    #[error("{0}")]
    InvalidArgs(String),

    /// Unsupported shell for completions.
    #[error("Unsupported shell: {0}")]
    UnsupportedShell(String),

    /// Runtime failures (server errors, ACME flows, etc).
    /// The inner string is printed as-is by main().
    #[error("{0}")]
    RuntimeError(String),

    /// Runtime failures (server errors, ACME flows, etc).
    #[error("{0:#}")]
    Anyhow(#[from] anyhow::Error),
}

//
// HTTP / application errors
//

#[derive(Debug, Serialize, JsonSchema, Error)]
#[error("{inner}")]
pub struct AppError {
    #[serde(skip)]
    pub status: StatusCode,

    #[serde(skip)]
    pub inner: anyhow::Error,

    #[serde(skip)]
    #[allow(dead_code)]
    pub backtrace: Option<Backtrace>,
}

impl AppError {
    fn capture_backtrace() -> Option<Backtrace> {
        // debug_assertions is enabled in dev builds, disabled in release
        if cfg!(debug_assertions) {
            Some(Backtrace::capture())
        } else {
            None
        }
    }

    /// default 500 error
    pub fn new(err: impl Into<anyhow::Error>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            inner: err.into(),
            backtrace: Self::capture_backtrace(),
        }
    }

    /// build an error with a specific HTTP status
    pub fn with_status(status: StatusCode, err: impl Into<anyhow::Error>) -> Self {
        Self {
            status,
            inner: err.into(),
            backtrace: Self::capture_backtrace(),
        }
    }

    pub fn unauthorized(message: &str) -> Self {
        Self::with_status(StatusCode::UNAUTHORIZED, anyhow!(message.to_owned()))
    }

    pub fn forbidden(message: &str) -> Self {
        Self::with_status(StatusCode::FORBIDDEN, anyhow!(message.to_owned()))
    }

    pub fn internal(message: &str) -> Self {
        Self::with_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            anyhow!(message.to_owned()),
        )
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::new(err)
    }
}

impl From<tower_sessions::session::Error> for AppError {
    fn from(err: tower_sessions::session::Error) -> Self {
        // treat it as an internal error; you can specialize this later if you want
        AppError::new(err)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        AppError::new(err)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        use axum::http::StatusCode;
        use tracing::{error, warn};

        if self.status.is_server_error() {
            // only real server bugs get the heavy logging
            error!("internal error (status={}): {:#}", self.status, self.inner);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
        } else {
            // client / auth errors: shorter log, no backtrace
            warn!("request failed (status={}): {}", self.status, self.inner);
            (self.status, self.inner.to_string()).into_response()
        }
    }
}

pub type AppResult<T> = std::result::Result<T, AppError>;

#[derive(Serialize, JsonSchema)]
pub struct ErrorBody {
    pub error: String,
}
