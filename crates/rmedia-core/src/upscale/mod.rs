//! Upscaling jobs (feature 002): options, planning and the per-job upscale plan.

pub mod capability;
pub mod encode;
pub mod estimate;
pub mod geometry;
pub mod graph;
pub mod reframe;
pub mod tier;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::plan::ffargs::ClipSpec;
use crate::preset::{DEFAULT_PRESET, Preset};
use encode::ColourTags;
use estimate::Estimate;
use geometry::Geometry;
use graph::GraphInput;
use tier::{Tier, TierDecision};

/// An upscale job: the source range to read (an accurate-mode clip plan, so seeking, streams,
/// naming and collisions are shared with clipping) and how to upscale it.
#[derive(Debug, Clone, PartialEq)]
pub struct UpscaleSpec {
    pub cut: ClipSpec,
    pub up: UpscalePlan,
}

/// Smart-reframe analysis state of a job (US2 fills `Pending`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisState {
    /// The shape already matches, or the reframe is `center`/`blur`.
    NotNeeded,
    /// Analysis runs as the job's first stage (or was skipped with `--no-analyze`).
    Pending,
}

/// Everything resolved for one upscale job; self-contained (the preset is copied in).
#[derive(Debug, Clone, PartialEq)]
pub struct UpscalePlan {
    pub preset: Preset,
    pub target: (u32, u32),
    pub geometry: Geometry,
    /// The reframe the user asked for.
    pub reframe: Reframe,
    /// The reframe actually applied (smart falls back to blur until US2, or for lack of a model).
    pub reframe_applied: Reframe,
    pub motion: Motion,
    pub analysis: AnalysisState,
    pub tier: TierDecision,
    pub hdr: HdrMode,
    pub colour_in: ColourTags,
    /// Video filtergraph inputs (`graph::build`).
    pub graph: GraphInput,
    /// Encoder arguments (`encode::video_args`), fixed at plan time.
    pub video_args: Vec<String>,
    /// Every mapped audio stream is AAC and the preset allows copying it.
    pub audio_copy: bool,
    pub fps_out: f64,
    /// Video length the output must have (the source's video stream, or the clip span).
    pub expected_video: Option<crate::time::Ts>,
    pub estimate: Estimate,
    pub limit_warnings: Vec<String>,
}

impl UpscalePlan {
    /// The dry-run `upscale` block (contracts/json-output.md), shared by the CLI and desktop.
    pub fn to_json(&self) -> Value {
        let status = match self.analysis {
            AnalysisState::NotNeeded => "not_needed",
            AnalysisState::Pending => "pending",
        };
        json!({
            "preset": self.preset.id,
            "size": [self.target.0, self.target.1],
            "reframe": self.reframe,
            "reframe_applied": self.reframe_applied,
            "motion": self.motion,
            "tier": self.tier.tier,
            "tier_reason": self.tier.reason,
            "suggest": self.tier.suggest,
            "hdr": self.hdr,
            "no_upscale_needed": self.geometry.no_upscale_needed,
            "estimate": { "bytes": self.estimate.bytes, "secs": (self.estimate.secs * 10.0).round() / 10.0 },
            "limit_warnings": self.limit_warnings,
            "reframe_analysis": { "status": status },
        })
    }
}

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

impl PartialUpscale {
    /// Layers this over `base` (the resolved options below it). Any layer that is present turns
    /// upscaling on, filling unset fields from `base` or the defaults; `enabled: false` turns
    /// it off.
    pub fn apply(&self, base: Option<&UpscaleOptions>) -> Option<UpscaleOptions> {
        if self.enabled == Some(false) {
            return None;
        }
        let b = base.cloned().unwrap_or_default();
        Some(UpscaleOptions {
            preset: self.preset.clone().unwrap_or(b.preset),
            reframe: self.reframe.unwrap_or(b.reframe),
            tier: self.tier.unwrap_or(b.tier),
            hdr: self.hdr.unwrap_or(b.hdr),
            motion: self.motion.unwrap_or(b.motion),
            crf: self.crf.or(b.crf),
        })
    }

    /// The layer that turns `base` into `opts` (`None` = nothing to override), so that applying
    /// it to `base` gives `opts` back. Only changed fields are written (FR-022).
    pub fn diff(opts: Option<&UpscaleOptions>, base: Option<&UpscaleOptions>) -> Option<PartialUpscale> {
        if opts == base {
            return None;
        }
        let Some(o) = opts else {
            return Some(PartialUpscale { enabled: Some(false), ..PartialUpscale::default() });
        };
        let b = base.cloned().unwrap_or_default();
        Some(PartialUpscale {
            enabled: None,
            preset: (o.preset != b.preset).then(|| o.preset.clone()),
            reframe: (o.reframe != b.reframe).then_some(o.reframe),
            tier: (o.tier != b.tier).then_some(o.tier),
            hdr: (o.hdr != b.hdr).then_some(o.hdr),
            motion: (o.motion != b.motion).then_some(o.motion),
            // A crf set below and cleared here cannot be expressed; keep the base's value.
            crf: (o.crf != b.crf).then_some(o.crf).flatten(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn up(json: &str) -> PartialUpscale {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn presence_enables_and_enabled_false_disables() {
        assert_eq!(
            up("{}").apply(None),
            Some(UpscaleOptions::default()),
            "an empty layer turns upscaling on"
        );
        assert_eq!(up(r#"{"enabled":false}"#).apply(Some(&UpscaleOptions::default())), None);
        let reddit = up(r#"{"preset":"reddit-916"}"#).apply(None).unwrap();
        assert_eq!((reddit.preset.as_str(), reddit.reframe), ("reddit-916", Reframe::Smart));
        let blur_cpu = up(r#"{"reframe":"blur","tier":"cpu"}"#).apply(Some(&reddit)).unwrap();
        assert_eq!(
            (blur_cpu.preset.as_str(), blur_cpu.reframe, blur_cpu.tier),
            ("reddit-916", Reframe::Blur, TierChoice::Cpu)
        );
        assert!(serde_json::from_str::<PartialUpscale>(r#"{"scale":2}"#).is_err());
        assert!(serde_json::from_str::<PartialUpscale>(r#"{"tier":"quantum"}"#).is_err());
    }

    #[test]
    fn diff_inverts_apply() {
        let d = UpscaleOptions::default();
        let custom = UpscaleOptions { preset: "duo".into(), crf: Some(20), ..d.clone() };
        let cases = [
            (None, None),
            (Some(d.clone()), None),
            (None, Some(d.clone())),
            (Some(custom.clone()), Some(d.clone())),
            (Some(d.clone()), Some(UpscaleOptions { preset: "duo".into(), ..d.clone() })),
        ];
        for (opts, base) in cases {
            let p = PartialUpscale::diff(opts.as_ref(), base.as_ref());
            let back = match &p {
                None => base.clone(),
                Some(p) => p.apply(base.as_ref()),
            };
            assert_eq!(back, opts, "diff {p:?}");
        }
        assert_eq!(PartialUpscale::diff(Some(&d), None), Some(PartialUpscale::default()));
        assert_eq!(
            PartialUpscale::diff(Some(&custom), Some(&d)).unwrap(),
            up(r#"{"preset":"duo","crf":20}"#)
        );
    }
}
