//! Budgeted video (feature 005): an MP4 with audio that fits a video target's size cap and
//! length limit (specs/005-budgeted-reddit-video). `target` holds the targets as data, `plan`
//! the pure planner; `exec::fit` encodes, measures and retries.

pub mod plan;
pub mod target;

use serde::Serialize;

/// One encode of a fit job: the video bitrate it used and the size it came out at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FitAttempt {
    pub video_kbps: u32,
    pub bytes: u64,
}
