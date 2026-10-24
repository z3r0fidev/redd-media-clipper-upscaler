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
    /// An external tool failed. `tool` names it ("ffmpeg" unless said otherwise, see
    /// [`Error::engine_failed`]); the JSON code is `ENGINE_FAILED` for every tool.
    #[error("{tool} failed: {message}")]
    EngineFailed { tool: String, message: String, stderr_tail: String },
    #[error("output verification failed: {0}")]
    VerifyFailed(String),
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    EngineMissing(String),
    /// A size budget cannot be met (feature 003). At plan time only `min_bytes` is set (the
    /// static floor); after the search's floor sample, `fits_secs` (the longest range that fits)
    /// and `best_bytes` (the smallest size reached).
    #[error("{}", budget_message(*fits_secs, *best_bytes, *min_bytes))]
    BudgetUnreachable { fits_secs: Option<f64>, best_bytes: Option<u64>, min_bytes: Option<u64> },
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
    /// An unknown GIF target id (003).
    TargetUnknown,
    /// A GIF size budget cannot be reached at the target's floors (003).
    BudgetUnreachable,
    /// Warning: the result is over the soft budget and within the hard one (003).
    OverRecommended,
    /// Warning: an optional GIF engine that mattered is unavailable; another one was used (003).
    EngineFallback,
    /// Notice: the frame rate is not exact in GIF's centisecond delays (003).
    TimingApproximate,
    /// Warning: the range is longer than the GIF target's recommended duration (003).
    LongRange,
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
            IssueCode::TargetUnknown => "TARGET_UNKNOWN",
            IssueCode::BudgetUnreachable => "BUDGET_UNREACHABLE",
            IssueCode::OverRecommended => "OVER_RECOMMENDED",
            IssueCode::EngineFallback => "ENGINE_FALLBACK",
            IssueCode::TimingApproximate => "TIMING_APPROXIMATE",
            IssueCode::LongRange => "LONG_RANGE",
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
            Error::BudgetUnreachable { .. } => IssueCode::BudgetUnreachable,
            Error::Io(_) => IssueCode::IoError,
        }
    }

    /// An ffmpeg failure (the tool most errors come from).
    pub fn engine_failed(message: impl Into<String>, stderr_tail: impl Into<String>) -> Error {
        Error::EngineFailed {
            tool: "ffmpeg".into(),
            message: message.into(),
            stderr_tail: stderr_tail.into(),
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
            detail: match self {
                Error::BudgetUnreachable { fits_secs, best_bytes, min_bytes } => Some(ErrorDetail {
                    fits_secs: fits_secs.map(|s| (s * 10.0).round() / 10.0),
                    best_bytes: *best_bytes,
                    min_bytes: *min_bytes,
                }),
                _ => None,
            },
        }
    }
}

fn budget_message(fits_secs: Option<f64>, best_bytes: Option<u64>, min_bytes: Option<u64>) -> String {
    let human = crate::upscale::estimate::human_bytes;
    match (fits_secs, best_bytes, min_bytes) {
        (_, _, Some(min)) => format!("the size budget is below the smallest possible GIF ({})", human(min)),
        (Some(s), Some(best), _) => format!(
            "the size budget cannot be reached at the target's lowest settings (smallest {}); about {s:.1} s fits (use --allow-trim to trim)",
            human(best)
        ),
        (Some(s), None, _) => {
            format!("the size budget cannot be reached; about {s:.1} s fits (use --allow-trim to trim)")
        }
        (None, Some(best), _) => {
            format!(
                "the size budget cannot be reached at the target's lowest settings (smallest {})",
                human(best)
            )
        }
        (None, None, None) => "the size budget cannot be reached".into(),
    }
}

/// Serializable error summary for events and reports.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ErrorInfo {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub stderr_tail: Option<String>,
    /// Structured facts about the error (only `BUDGET_UNREACHABLE` has any, feature 003).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub detail: Option<ErrorDetail>,
}

/// `ErrorInfo.detail` (contracts/json-output.md › Outcome): absent fields are not serialised.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ErrorDetail {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub fits_secs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub best_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub min_bytes: Option<u64>,
}
