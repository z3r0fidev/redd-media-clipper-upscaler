//! Quality tiers and tier selection (research R9).

use serde::Serialize;

/// The scaler a job runs with: classical CPU, classical GPU (Vulkan/libplacebo) or AI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Tier {
    Cpu,
    Gpu,
    Ai,
}
