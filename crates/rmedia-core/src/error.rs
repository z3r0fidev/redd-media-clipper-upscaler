//! Error type with stable, machine-readable codes (see contracts/json-output.md).

use std::path::PathBuf;

use serde::Serialize;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    InvalidInput(String),
    #[error("source not found: {}", .0.display())]
    SourceMissing(PathBuf),
    #[error("cannot read media {}: {message}", path.display())]
    ProbeFailed { path: PathBuf, message: String },
    #[error("output already exists: {}", .0.display())]
    OutputExists(PathBuf),
    #[error("cannot write output {}: {message}", path.display())]
    OutputUnwritable { path: PathBuf, message: String },
    #[error("not enough disk space writing {}", .0.display())]
    DiskFull(PathBuf),
    #[error("ffmpeg failed: {message}")]
    EngineFailed { message: String, stderr_tail: String },
    #[error("output verification failed: {0}")]
    VerifyFailed(String),
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    EngineMissing(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    /// Stable code used in JSON output and by the GUI.
    pub fn code(&self) -> &'static str {
        match self {
            Error::InvalidInput(_) => "INVALID_INPUT",
            Error::SourceMissing(_) => "SOURCE_MISSING",
            Error::ProbeFailed { .. } => "PROBE_FAILED",
            Error::OutputExists(_) => "OUTPUT_EXISTS",
            Error::OutputUnwritable { .. } => "OUTPUT_UNWRITABLE",
            Error::DiskFull(_) => "DISK_FULL",
            Error::EngineFailed { .. } => "ENGINE_FAILED",
            Error::VerifyFailed(_) => "VERIFY_FAILED",
            Error::Cancelled => "CANCELLED",
            Error::EngineMissing(_) => "ENGINE_MISSING",
            Error::Io(_) => "IO_ERROR",
        }
    }

    pub fn info(&self) -> ErrorInfo {
        ErrorInfo {
            code: self.code().to_owned(),
            message: self.to_string(),
            stderr_tail: match self {
                Error::EngineFailed { stderr_tail, .. } if !stderr_tail.is_empty() => {
                    Some(stderr_tail.clone())
                }
                _ => None,
            },
        }
    }
}

/// Serializable error summary for events and reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorInfo {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_tail: Option<String>,
}
