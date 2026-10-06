//! Events streamed from the core to the CLI renderer and the GUI (data-model.md › Event).

use std::path::PathBuf;

use serde::Serialize;

use crate::error::ErrorInfo;
use crate::time::Ts;

pub type JobId = u32;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Event {
    RunStarted {
        run_id: String,
        total_jobs: usize,
        total_media: Ts,
    },
    JobStarted {
        job_id: JobId,
    },
    JobProgress {
        job_id: JobId,
        fraction: f64,
        out_time: Ts,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        speed: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        eta_ms: Option<u64>,
    },
    JobWarning {
        job_id: JobId,
        message: String,
    },
    JobFinished {
        job_id: JobId,
        outcome: JobOutcome,
    },
    RunProgress {
        done: usize,
        total: usize,
        fraction: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        eta_ms: Option<u64>,
    },
    RunFinished {
        summary: RunSummary,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Span {
    pub start: Ts,
    pub end: Ts,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum JobOutcome {
    Succeeded {
        #[serde(serialize_with = "crate::pathser::path")]
        output: PathBuf,
        actual: Span,
        /// The produced span differs from the requested one by more than 50 ms.
        adjusted: bool,
    },
    Skipped {
        reason: String,
    },
    Failed {
        error: ErrorInfo,
    },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct JobResult {
    pub id: JobId,
    #[serde(serialize_with = "crate::pathser::path")]
    pub source: PathBuf,
    #[serde(serialize_with = "crate::pathser::path")]
    pub output: PathBuf,
    pub requested: Span,
    #[serde(flatten)]
    pub outcome: JobOutcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct RunSummary {
    pub succeeded: usize,
    pub skipped: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub exit_code: i32,
    pub jobs: Vec<JobResult>,
}

/// Receives events; the CLI renders them, the GUI forwards them over a Tauri channel.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: Event);
}

/// Discards events.
#[derive(Debug, Default)]
pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _: Event) {}
}

impl<F: Fn(Event) + Send + Sync> EventSink for F {
    fn emit(&self, event: Event) {
        self(event)
    }
}
