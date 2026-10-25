//! The bounded, deterministic size-budget search (research R5). The search itself arrives with
//! US2; its record type is part of the outcome contract already.

use serde::Serialize;

use super::plan::Lowered;

/// What a step of the search was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifStepKind"))]
pub enum StepKind {
    /// A sample encode (counts as an encode).
    Sample,
    /// A gifsicle re-run on a sample (not an encode).
    Post,
    /// A full encode.
    Full,
}

/// One step of the search.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifSearchStep"))]
pub struct SearchStep {
    pub n: u32,
    pub kind: StepKind,
    pub bytes: u64,
    /// The predicted size of the whole GIF at this step's settings.
    pub estimate: u64,
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub settings: serde_json::Value,
}

/// Which limit the result is under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifFit"))]
pub enum Fit {
    Soft,
    Hard,
    None,
}

/// The search's record (data-model.md › SearchRecord).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifSearchRecord"))]
pub struct SearchRecord {
    pub encodes: u8,
    pub fit: Fit,
    pub lowered: Vec<Lowered>,
    pub steps: Vec<SearchStep>,
}
