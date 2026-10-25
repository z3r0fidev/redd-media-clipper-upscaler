//! Events streamed from the core to the CLI renderer and the GUI (data-model.md › Event).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{ErrorInfo, IssueCode};
use crate::time::Ts;
use crate::upscale::tier::Tier;

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
        /// The warning's code, for warnings that have one (feature 003, e.g. `ENGINE_FALLBACK`).
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        code: Option<IssueCode>,
    },
    /// A multi-stage job (e.g. upscale: analyze → render → verify) entered stage `index` of `of`.
    /// `job_progress.fraction` always covers the whole job, weighted across stages.
    JobStage {
        job_id: JobId,
        stage: String,
        index: u32,
        of: u32,
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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
        /// What the job produced; its keys are flattened next to `output` (`"measure"` names
        /// the kind), so clip outcomes keep 001's `actual` and `adjusted` keys.
        #[serde(flatten)]
        measure: JobMeasure,
    },
    Skipped {
        reason: String,
    },
    Failed {
        error: ErrorInfo,
    },
    Cancelled,
}

/// What a succeeded job measured on its output, per job kind.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "measure", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum JobMeasure {
    Clip {
        actual: Span,
        /// The produced span differs from the requested one by more than 50 ms.
        adjusted: bool,
    },
    Upscale {
        span: Span,
        tier_used: Tier,
        /// An automatic GPU job failed and was retried on the CPU tier (FR-008).
        tier_fallback: bool,
        /// Time ranges that fell back to the blurred background (FR-014).
        fallback_ranges: Vec<Span>,
        /// The source was already at or above the target size (FR-005).
        no_upscale_needed: bool,
    },
    /// A GIF or Reddit MP4 (feature 003, contracts/json-output.md › Outcome).
    Gif {
        span: Span,
        format: crate::gif::Format,
        bytes: u64,
        width: u32,
        height: u32,
        fps: f64,
        timing: crate::gif::timing::Timing,
        /// Palette size, dithering and gifsicle lossy level (built-in engine only).
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        colours: Option<u16>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        dither: Option<crate::gif::target::Dither>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        lossy: Option<u16>,
        engine: crate::gif::plan::EngineUsed,
        /// gifsicle optimised the output.
        optimiser: bool,
        #[serde(rename = "loop")]
        loop_count: crate::gif::LoopCount,
        /// The same input and options give the same bytes (research R14).
        deterministic: bool,
        /// The range was shortened to fit the budget (`--allow-trim`, FR-011).
        trimmed: Option<Span>,
        /// The budget search, when there was a budget (FR-012).
        search: Option<crate::gif::budget::SearchRecord>,
        /// The Reddit MP4 of the same frames, for the hint (US3).
        mp4_estimate: Option<u64>,
    },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{ErrorDetail, IssueCode};
    use serde_json::json;

    #[test]
    fn job_warnings_without_a_code_serialise_as_before() {
        let plain = Event::JobWarning { job_id: 3, message: "careful".into(), code: None };
        assert_eq!(
            serde_json::to_value(&plain).unwrap(),
            json!({"type": "job_warning", "job_id": 3, "message": "careful"})
        );
        let coded =
            Event::JobWarning { job_id: 3, message: "careful".into(), code: Some(IssueCode::EngineFallback) };
        assert_eq!(serde_json::to_value(&coded).unwrap()["code"], "ENGINE_FALLBACK");
    }

    #[test]
    fn error_info_without_detail_serialises_as_before() {
        let info = crate::Error::Cancelled.info();
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            json!({"code": "CANCELLED", "message": "cancelled"})
        );
        let budget = crate::Error::BudgetUnreachable {
            fits_secs: Some(4.2),
            best_bytes: Some(9_800_000),
            min_bytes: None,
        };
        let v = serde_json::to_value(budget.info()).unwrap();
        assert_eq!(v["code"], "BUDGET_UNREACHABLE");
        assert_eq!(v["detail"], json!({"fits_secs": 4.2, "best_bytes": 9_800_000}));
        let floor =
            crate::Error::BudgetUnreachable { fits_secs: None, best_bytes: None, min_bytes: Some(65_536) };
        assert_eq!(serde_json::to_value(floor.info()).unwrap()["detail"], json!({"min_bytes": 65_536}));
        assert_eq!(
            ErrorDetail::default(),
            ErrorDetail { fits_secs: None, best_bytes: None, min_bytes: None }
        );
    }

    #[test]
    fn engine_failures_name_their_tool_and_default_to_ffmpeg() {
        let e = crate::Error::engine_failed("bad input", "tail");
        assert_eq!(e.to_string(), "ffmpeg failed: bad input");
        assert_eq!(e.code(), "ENGINE_FAILED");
        let g = crate::Error::EngineFailed {
            tool: "gifsicle".into(),
            message: "exit 1".into(),
            stderr_tail: String::new(),
        };
        assert_eq!(g.to_string(), "gifsicle failed: exit 1");
        assert_eq!(g.code(), "ENGINE_FAILED");
    }
}
