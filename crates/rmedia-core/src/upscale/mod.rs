//! Upscaling jobs (feature 002): options, planning and the per-job upscale plan.

pub mod capability;
pub mod encode;
pub mod estimate;
pub mod geometry;
pub mod graph;
pub mod reframe;
pub mod tier;

use serde::{Deserialize, Serialize};

use crate::preset::DEFAULT_PRESET;
use tier::Tier;

/// How a source whose shape differs from the preset fills the frame (FR-012).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Reframe {
    /// Follow the subject (faces), falling back to `Blur` where none fits (FR-013/014).
    #[default]
    Smart,
    Center,
    /// The whole frame over a blurred, darkened copy that fills the screen.
    Blur,
}

/// Which quality tier to use: chosen per machine (`auto`) or forced (FR-008).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum TierChoice {
    #[default]
    Auto,
    Cpu,
    Gpu,
    Ai,
}

impl TierChoice {
    /// The forced tier, or `None` for `auto`.
    pub fn forced(self) -> Option<Tier> {
        match self {
            TierChoice::Auto => None,
            TierChoice::Cpu => Some(Tier::Cpu),
            TierChoice::Gpu => Some(Tier::Gpu),
            TierChoice::Ai => Some(Tier::Ai),
        }
    }
}

/// HDR sources: tone-map to SDR (default) or keep HDR on presets that can carry it (research R4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum HdrMode {
    #[default]
    Sdr,
    Keep,
}

/// Smart-reframe camera speed profile (research R6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Motion {
    Calm,
    #[default]
    Normal,
    Fast,
}

/// Fully resolved upscale options for one job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpscaleOptions {
    pub preset: String,
    pub reframe: Reframe,
    pub tier: TierChoice,
    pub hdr: HdrMode,
    pub motion: Motion,
    /// Overrides the preset's quality (CRF).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub crf: Option<u8>,
}

impl Default for UpscaleOptions {
    fn default() -> Self {
        UpscaleOptions {
            preset: DEFAULT_PRESET.to_owned(),
            reframe: Reframe::default(),
            tier: TierChoice::default(),
            hdr: HdrMode::default(),
            motion: Motion::default(),
            crf: None,
        }
    }
}

/// One layer of upscale overrides (manifest defaults, a video, a clip, the CLI, settings).
/// `enabled: false` turns upscaling off at that layer; setting any other field turns it on.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PartialUpscale {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub reframe: Option<Reframe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub tier: Option<TierChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub hdr: Option<HdrMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub motion: Option<Motion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub crf: Option<u8>,
}
