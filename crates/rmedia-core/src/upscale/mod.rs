//! Upscaling jobs (feature 002): options, planning and the per-job upscale plan.

pub mod ai;
pub mod capability;
pub mod encode;
pub mod estimate;
pub mod geometry;
pub mod graph;
pub mod preview;
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

/// Smart-reframe analysis state of a job.
#[derive(Debug, Clone, PartialEq)]
pub enum AnalysisState {
    /// The shape already matches, or the reframe is `center`/`blur`.
    NotNeeded,
    /// Analysis runs as the job's first stage (or was skipped with `--no-analyze`).
    Pending,
    /// Analysed (in a dry run, for `--crop-path`, or by the executor).
    Ready(std::sync::Arc<reframe::ReframeAnalysis>, CacheUse),
}

/// Whether an analysis came from the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheUse {
    Hit,
    Miss,
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
    /// Smart reframe: the face detector to analyse with.
    pub detector: Option<reframe::detect::DetectorSpec>,
    /// Analysis frame size (long side 576).
    pub analysis_size: (u32, u32),
    /// Where analyses are cached.
    pub reframe_cache: Option<std::path::PathBuf>,
    /// Seconds added to `reframe.cmd` times: the render's first video frame time.
    pub cmd_offset: f64,
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
    /// `fps_out` as a rational (the AI tier's constant frame rate).
    pub frame_rate: (u32, u32),
    /// The AI tier's worker and models (`None` for the other tiers).
    pub ai: Option<ai::AiEngine>,
    /// Video length the output must have (the source's video stream, or the clip span).
    pub expected_video: Option<crate::time::Ts>,
    pub estimate: Estimate,
    pub limit_warnings: Vec<String>,
}

impl UpscalePlan {
    /// Applies a finished analysis: the graph starts the crop where the path starts and keeps
    /// the fallback branch only when some range needs it.
    pub fn with_analysis(
        mut self,
        a: std::sync::Arc<reframe::ReframeAnalysis>,
        cache: CacheUse,
    ) -> UpscalePlan {
        let x0 = a.path.first().map_or(0, |x| (*x).max(0) as u32);
        // Nothing to follow anywhere: render the plain blurred background (one branch instead
        // of the crop and the background both).
        let whole = a.fallback_ranges.first().is_some_and(|r| {
            r.start <= a.clip_start
                && r.end.as_secs_f64() >= a.clip_start.as_secs_f64() + a.len_secs() - 0.5 / a.fps
        });
        if whole {
            self = self.with_blur();
        } else {
            self.graph.track = Some(graph::Track { x0, fallback: !a.fallback_ranges.is_empty() });
        }
        self.analysis = AnalysisState::Ready(a, cache);
        self
    }

    /// Switches to the CPU tier (after an automatic GPU job failed).
    pub fn with_tier_cpu(mut self, reason: impl Into<String>) -> UpscalePlan {
        self.tier = tier::TierDecision::cpu(reason);
        self.graph.gpu = None;
        self
    }

    /// Falls back to the blurred background (the analysis could not run).
    pub fn with_blur(mut self) -> UpscalePlan {
        let g = geometry::plan(self.geometry.display, self.geometry.target, Reframe::Blur);
        self.geometry = g;
        self.graph.geometry = g;
        self.graph.reframe = Reframe::Blur;
        self.graph.track = None;
        self.reframe_applied = Reframe::Blur;
        self.analysis = AnalysisState::NotNeeded;
        self
    }

    /// The finished analysis, if any.
    pub fn analysis(&self) -> Option<&reframe::ReframeAnalysis> {
        match &self.analysis {
            AnalysisState::Ready(a, _) => Some(a),
            _ => None,
        }
    }

    /// The `reframe_analysis` block of the dry-run JSON.
    fn analysis_json(&self) -> Value {
        match &self.analysis {
            AnalysisState::NotNeeded => json!({ "status": "not_needed" }),
            AnalysisState::Pending => json!({ "status": "pending" }),
            AnalysisState::Ready(a, cache) => {
                let span = |s: &crate::event::Span| json!({ "start": s.start, "end": s.end });
                json!({
                    "status": "ready",
                    "cache": cache,
                    "crop": [a.crop.0, a.crop.1],
                    "axis": "x",
                    "fps": a.fps,
                    "shots": a.shots.iter().map(|s| json!({ "start": s.span.start, "end": s.span.end, "kind": s.kind })).collect::<Vec<_>>(),
                    "fallback_ranges": a.fallback_ranges.iter().map(span).collect::<Vec<_>>(),
                    "path": {
                        "hz": 10,
                        "keyframes": reframe::sendcmd::keyframes(a, 10.0)
                            .into_iter()
                            .map(|(t, x)| json!([(t * 1000.0).round() / 1000.0, x]))
                            .collect::<Vec<_>>(),
                    },
                })
            }
        }
    }

    /// The dry-run `upscale` block (contracts/json-output.md), shared by the CLI and desktop.
    pub fn to_json(&self) -> Value {
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
            "reframe_analysis": self.analysis_json(),
        })
    }

    /// The AI tier's frame rate for ffmpeg (`n/d`).
    pub fn rate(&self) -> String {
        format!("{}/{}", self.frame_rate.0, self.frame_rate.1)
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

/// Highest x264/x265 CRF a user may set.
pub const MAX_CRF: u8 = 51;

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
    /// `Some(None)` (JSON `null`) clears a crf set by a layer below, back to the preset's.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "some_or_null")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number | null"))]
    pub crf: Option<Option<u8>>,
}

/// Keeps an explicit `null` apart from a missing field (`#[serde(default)]` covers the latter).
fn some_or_null<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<u8>>, D::Error> {
    Option::<u8>::deserialize(d).map(Some)
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
            crf: self.crf.unwrap_or(b.crf),
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
            crf: (o.crf != b.crf).then_some(o.crf),
        })
    }
}

/// Runs the smart-reframe analysis of every pending job in `plan` (dry runs, `--crop-path`, the
/// desktop preview), so the plan shows the crop path and the render reuses it. A job whose
/// analysis fails falls back to the blurred background with a warning.
pub async fn prepare_plan(
    plan: &mut crate::plan::Plan,
    tools: &crate::tools::Tools,
    cancel: &tokio_util::sync::CancellationToken,
) -> crate::Result<()> {
    for job in &mut plan.jobs {
        let crate::plan::JobSpec::Upscale(u) = &mut job.spec else { continue };
        if u.up.analysis != AnalysisState::Pending {
            continue;
        }
        match reframe::analyze_cached(tools, &u.reframe_input(), cancel, |_| {}).await {
            Ok((a, cache)) => u.up = u.up.clone().with_analysis(a, cache),
            Err(crate::Error::Cancelled) => return Err(crate::Error::Cancelled),
            Err(e) => {
                job.warnings
                    .push(format!("smart reframe analysis failed ({e}); using the blurred background"));
                u.up = u.up.clone().with_blur();
            }
        }
    }
    Ok(())
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
            (Some(d.clone()), Some(custom.clone())),
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
        // A crf set below and cleared here is written as `null`.
        let cleared = PartialUpscale::diff(Some(&d), Some(&custom)).unwrap();
        assert_eq!(cleared.crf, Some(None));
        assert_eq!(serde_json::to_string(&cleared).unwrap(), r#"{"preset":"phone-fill","crf":null}"#);
        assert_eq!(up(r#"{"crf":null}"#).apply(Some(&custom)).unwrap().crf, None);
        assert_eq!(up("{}").apply(Some(&custom)).unwrap().crf, Some(20));
    }
}
