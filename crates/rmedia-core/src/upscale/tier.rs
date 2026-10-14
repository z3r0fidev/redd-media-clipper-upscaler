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

/// The tier a job will use, and why (shown in dry runs and reports).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TierDecision {
    pub tier: Tier,
    pub reason: String,
    /// A better tier the user could choose (AI for low-resolution sources; never chosen
    /// automatically, research R9).
    pub suggest: Option<Tier>,
}
