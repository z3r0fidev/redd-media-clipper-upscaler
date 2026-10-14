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

/// Stable, machine-readable problem codes (contracts/json-output.md). They serialise as the
/// SCREAMING_SNAKE_CASE strings that 001 already emitted, plus the 002 additions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum IssueCode {
    InvalidInput,
    SourceMissing,
    ProbeFailed,
    OutputExists,
    OutputUnwritable,
    DiskFull,
    EngineFailed,
    VerifyFailed,
    Cancelled,
    EngineMissing,
    IoError,
    /// A forced quality tier this machine cannot run (002).
    TierUnavailable,
    /// An unknown device preset id (002).
    PresetUnknown,
    /// `hdr=keep` with a preset that cannot carry HDR (002).
    HdrUnsupported,
    /// Warning: a predicted output exceeds a preset's platform limit (002).
    LimitExceeded,
}

impl IssueCode {
    pub fn as_str(self) -> &'static str {
        match self {
            IssueCode::InvalidInput => "INVALID_INPUT",
            IssueCode::SourceMissing => "SOURCE_MISSING",
            IssueCode::ProbeFailed => "PROBE_FAILED",
            IssueCode::OutputExists => "OUTPUT_EXISTS",
            IssueCode::OutputUnwritable => "OUTPUT_UNWRITABLE",
            IssueCode::DiskFull => "DISK_FULL",
            IssueCode::EngineFailed => "ENGINE_FAILED",
            IssueCode::VerifyFailed => "VERIFY_FAILED",
            IssueCode::Cancelled => "CANCELLED",
            IssueCode::EngineMissing => "ENGINE_MISSING",
            IssueCode::IoError => "IO_ERROR",
            IssueCode::TierUnavailable => "TIER_UNAVAILABLE",
            IssueCode::PresetUnknown => "PRESET_UNKNOWN",
            IssueCode::HdrUnsupported => "HDR_UNSUPPORTED",
            IssueCode::LimitExceeded => "LIMIT_EXCEEDED",
        }
    }
}

impl std::fmt::Display for IssueCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<&str> for IssueCode {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl Error {
    /// The typed code for this error.
    pub fn issue_code(&self) -> IssueCode {
        match self {
            Error::InvalidInput(_) => IssueCode::InvalidInput,
            Error::SourceMissing(_) => IssueCode::SourceMissing,
            Error::ProbeFailed { .. } => IssueCode::ProbeFailed,
            Error::OutputExists(_) => IssueCode::OutputExists,
            Error::OutputUnwritable { .. } => IssueCode::OutputUnwritable,
            Error::DiskFull(_) => IssueCode::DiskFull,
            Error::EngineFailed { .. } => IssueCode::EngineFailed,
            Error::VerifyFailed(_) => IssueCode::VerifyFailed,
            Error::Cancelled => IssueCode::Cancelled,
            Error::EngineMissing(_) => IssueCode::EngineMissing,
            Error::Io(_) => IssueCode::IoError,
        }
    }

    /// Stable code used in JSON output and by the GUI.
    pub fn code(&self) -> &'static str {
        self.issue_code().as_str()
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ErrorInfo {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub stderr_tail: Option<String>,
}
