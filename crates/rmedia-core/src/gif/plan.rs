//! Planning a GIF job (data-model.md › GifPlan): the target, the engine, the output size and
//! frame rate, the starting settings and the notes and notices the dry run shows. The planner
//! never encodes; the size estimate is filled in later by the prepare step (research R4).

use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Value, json};

use super::target::{
    Budget, Dither, GifTarget, GifTargetRegistry, GifskiParams, MAX_BUDGET, StatsMode, TargetEngine,
};
use super::timing::{self, Timing};
use super::{EngineChoice, Format, GifOptions, LoopCount, Shape};
use crate::error::IssueCode;
use crate::plan::ffargs::ClipSpec;
use crate::plan::{Caps, Notice};
use crate::probe::MediaInfo;
use crate::upscale::Reframe;
use crate::upscale::encode::ColourTags;
use crate::upscale::geometry::{self, Overflow};
use crate::upscale::graph::{self, GifChain, GraphInput};

/// The smallest budget a GIF can meet at all (the static floor, research R5).
pub const MIN_BUDGET: u64 = 64 * 1024;

/// A GIF job: the source range to read (an accurate-mode clip plan, so seeking, naming and
/// collisions are shared with clipping), the frames to make and how to encode them.
#[derive(Debug, Clone, PartialEq)]
pub struct GifSpec {
    pub cut: ClipSpec,
    pub frame: FramePlan,
    pub gif: GifPlan,
}

/// How the source's frames become GIF frames (002's pre-processing and framing, refactor B1).
#[derive(Debug, Clone, PartialEq)]
pub struct FramePlan {
    /// Displayed source size.
    pub display: (u32, u32),
    pub shape: Shape,
    /// The framing asked for, and the one applied.
    pub framing: Reframe,
    pub framing_applied: Reframe,
    /// The graph at the starting size (`geometry.target` is the GIF size).
    pub graph: GraphInput,
    /// `unsharp` amount after the resample (flat-ui).
    pub sharpen: Option<f32>,
}

impl FramePlan {
    /// The graph for an output of `size` (the budget search lowers the width).
    pub fn graph_at(&self, size: (u32, u32)) -> GraphInput {
        let mut g = self.graph.clone();
        g.geometry = geometry::plan(self.display, size, self.framing_applied);
        g
    }

    /// The frame chain for `s` (see [`graph::frame_chain`]).
    pub fn chain(&self, s: &Settings) -> String {
        graph::frame_chain(
            &self.graph_at((s.width, s.height)),
            &GifChain { rate: &s.rate_text(), sharpen: self.sharpen },
        )
    }
}

/// The engine a job encodes with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifEngineUsed"))]
pub enum EngineUsed {
    /// FFmpeg's two-pass palette pipeline.
    Builtin,
    Gifski,
    /// The Reddit MP4 (H.264).
    Libx264,
}

/// The encode settings of one attempt (the start, a search step, the chosen one).
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    /// Frames per second as (num, den).
    pub rate: (u32, u32),
    pub colours: u16,
    pub dither: Dither,
    pub stats: StatsMode,
    /// gifsicle `--lossy` (0: lossless optimisation only).
    pub lossy: u16,
    /// The best-quality engine's parameters (gifski jobs).
    pub gifski: Option<GifskiParams>,
}

impl Settings {
    pub fn fps(&self) -> f64 {
        f64::from(self.rate.0) / f64::from(self.rate.1)
    }

    pub fn rate_text(&self) -> String {
        format!("{}/{}", self.rate.0, self.rate.1)
    }

    pub fn timing(&self) -> Timing {
        timing::timing(self.rate)
    }

    /// The `settings` object of the JSON contract.
    pub fn to_json(&self) -> Value {
        let t = self.timing();
        json!({
            "width": self.width,
            "height": self.height,
            "fps": (self.fps() * 1000.0).round() / 1000.0,
            "timing": { "exact": t.exact, "avg_cs": (f64::from(t.avg_cs) * 100.0).round() / 100.0 },
            "colours": self.colours,
            "dither": self.dither,
            "lossy": self.lossy,
        })
    }
}

/// Where an estimate comes from (contracts/json-output.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EstimateFrom {
    /// The predicted result of the budget search.
    Search,
    /// One sample at the starting settings (no budget).
    Sample,
    /// Not sampled (planning, `--no-sample`).
    Pending,
}

/// One setting the search lowers, from → to.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifLowered"))]
pub struct Lowered {
    pub setting: String,
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub from: Value,
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub to: Value,
}

/// The size shown before encoding (FR-019).
#[derive(Debug, Clone, PartialEq)]
pub struct Estimate {
    pub bytes: Option<u64>,
    pub from: EstimateFrom,
    pub settings: Settings,
    pub lowered: Vec<Lowered>,
    /// The Reddit MP4 of the same frames, for the "MP4 would be smaller" hint (US3).
    pub mp4_bytes: Option<u64>,
}

/// Everything resolved for one GIF job; self-contained (the target is copied in).
#[derive(Debug, Clone, PartialEq)]
pub struct GifPlan {
    pub target: GifTarget,
    pub format: Format,
    pub engine: EngineUsed,
    pub engine_reason: String,
    /// gifski, when it is the engine.
    pub gifski: Option<PathBuf>,
    /// gifsicle, when it is found and optimising is on (built-in engine only).
    pub optimiser: Option<PathBuf>,
    pub optimiser_version: Option<String>,
    pub start: Settings,
    pub floor_width: Option<u32>,
    pub floor_fps: Option<u32>,
    pub budget: Option<Budget>,
    pub loop_count: LoopCount,
    pub allow_trim: bool,
    pub estimate: Estimate,
    pub notes: Vec<String>,
    /// Planned frame count (verify allows ±1).
    pub frames: u64,
    /// The range in seconds.
    pub len: f64,
    /// Identical inputs give identical bytes (research R14).
    pub deterministic: bool,
}

impl GifPlan {
    /// The dry-run `gif` block (contracts/json-output.md).
    pub fn to_json(&self) -> Value {
        let e = &self.estimate;
        json!({
            "target": self.target.id,
            "target_verified": self.target.verified,
            "format": self.format,
            "engine": self.engine,
            "engine_reason": self.engine_reason,
            "optimiser": self.optimiser_version.as_ref().map(|v| format!("gifsicle {v}")),
            "settings": self.start.to_json(),
            "floors": { "width": self.floor_width, "fps": self.floor_fps },
            "budget": self.budget,
            "loop": self.loop_count,
            "allow_trim": self.allow_trim,
            "estimate": {
                "bytes": e.bytes,
                "from": e.from,
                "lowered": e.lowered,
                "settings": e.settings.to_json(),
                "mp4_bytes": e.mp4_bytes,
            },
            "notes": self.notes,
        })
    }
}

/// What planning a GIF job adds to the plan.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    pub frame: FramePlan,
    pub gif: GifPlan,
    /// Plain warnings (every notice's message is among them, research R16).
    pub warnings: Vec<String>,
    pub notices: Vec<Notice>,
}

/// A planning problem: the issue code and message.
pub type Problem = (IssueCode, String);

fn invalid(msg: impl Into<String>) -> Problem {
    (IssueCode::InvalidInput, msg.into())
}

/// Plans a GIF job for `cut` of `media` with options `o`. `refused` are folders an engine found
/// on PATH may not come from (the job's input folder and scratch).
pub fn plan_gif(
    o: &GifOptions,
    targets: &GifTargetRegistry,
    caps: &Caps,
    media: &MediaInfo,
    cut: &ClipSpec,
    refused: &[PathBuf],
) -> Result<Planned, Problem> {
    let Some(target) = targets.get(&o.target).cloned() else {
        return Err((
            IssueCode::TargetUnknown,
            format!("unknown GIF target {:?} (known: {})", o.target, targets.ids().join(", ")),
        ));
    };
    if o.format == Format::Mp4 {
        return Err(invalid("the Reddit MP4 format (--format mp4) is not available yet"));
    }
    let missing = caps.missing(&crate::doctor::GIF_NEEDS);
    if !missing.is_empty() {
        return Err((
            IssueCode::EngineMissing,
            format!("the ffmpeg build lacks {} needed for GIF export", missing.join(", ")),
        ));
    }
    if let Some(w) = o.width.filter(|w| !(64..=1920).contains(w)) {
        return Err(invalid(format!("width {w} is out of range (64 to 1920)")));
    }
    let fps = o.fps.unwrap_or(target.fps);
    timing::validate(fps).map_err(invalid)?;
    if let Some(c) = o.colours.filter(|c| !(2..=256).contains(c)) {
        return Err(invalid(format!("colours {c} is out of range (2 to 256)")));
    }
    if let Some(l) = o.lossy.filter(|l| *l > 200) {
        return Err(invalid(format!("lossy {l} is out of range (0 to 200)")));
    }
    if matches!(o.dither, Some(Dither::Bayer(s)) if s > 5) {
        return Err(invalid("the bayer dither scale must be 0 to 5"));
    }
    let budget = o.budget.and_then(|spec| spec.resolve(target.budget));
    if let Some(b) = budget {
        if b.soft == 0 || b.soft > b.hard || b.hard > MAX_BUDGET {
            return Err(invalid(format!("the budget must have 1 ≤ soft ≤ hard ≤ {MAX_BUDGET} bytes")));
        }
        if b.hard < MIN_BUDGET && !o.allow_trim {
            return Err((
                IssueCode::BudgetUnreachable,
                format!(
                    "the size limit ({} bytes) is below the smallest possible GIF ({MIN_BUDGET} bytes); \
                     raise it, or allow trimming",
                    b.hard
                ),
            ));
        }
    }
    let v = media.first_video().ok_or_else(|| invalid("the source has no video stream"))?;
    let display = v.display_size().ok_or_else(|| invalid("the source's picture size is unknown"))?;
    let mut warnings = Vec::new();
    let mut notices = Vec::new();
    let mut notes = Vec::new();
    let mut notice = |warnings: &mut Vec<String>, code, message: String| {
        warnings.push(message.clone());
        notices.push(Notice { code, message });
    };

    // --- engine -------------------------------------------------------------------------------
    let engines = caps.gif_engines.clone().refusing(refused);
    let wants_gifski = match o.engine {
        EngineChoice::Gifski => true,
        EngineChoice::Builtin => false,
        EngineChoice::Auto => target.engine == TargetEngine::Best,
    };
    let (engine, engine_reason, gifski) = match (wants_gifski, engines.gifski.program()) {
        (false, _) => {
            let why = if o.engine == EngineChoice::Builtin {
                "built-in requested"
            } else {
                "target engine: builtin"
            };
            (EngineUsed::Builtin, why.to_owned(), None)
        }
        (true, Some(p)) => (EngineUsed::Gifski, "gifski (best quality)".to_owned(), Some(p.to_path_buf())),
        (true, None) => {
            let reason = engines.gifski.reason.clone().unwrap_or_else(|| "not found".into());
            if o.strict_engine {
                return Err((
                    IssueCode::EngineMissing,
                    format!("gifski is not available ({reason}) and --strict-engine is set"),
                ));
            }
            notice(
                &mut warnings,
                IssueCode::EngineFallback,
                format!("gifski is not available ({reason}); using the built-in engine"),
            );
            (EngineUsed::Builtin, "gifski requested but not found: built-in".to_owned(), None)
        }
    };
    let optimiser = (engine == EngineUsed::Builtin && o.optimise)
        .then(|| engines.gifsicle.program().map(|p| (p.to_path_buf(), engines.gifsicle.version.clone())))
        .flatten();
    if engine == EngineUsed::Gifski {
        for (set, flag) in [
            (o.colours.is_some(), "--colors"),
            (o.dither.is_some(), "--dither"),
            (o.lossy.is_some(), "--lossy"),
        ] {
            if set {
                notes.push(format!("{flag} is ignored by gifski"));
            }
        }
    }
    if o.lossy.is_some_and(|l| l > 0) && optimiser.is_none() && engine == EngineUsed::Builtin {
        let why = if o.optimise {
            engines.gifsicle.reason.clone().unwrap_or_else(|| "not found".into())
        } else {
            "--no-optimise".into()
        };
        if o.strict_engine {
            return Err((
                IssueCode::EngineMissing,
                format!("--lossy needs gifsicle ({why}) and --strict-engine is set"),
            ));
        }
        notice(&mut warnings, IssueCode::EngineFallback, format!("--lossy needs gifsicle ({why}); ignored"));
    }

    // --- size (never larger than the picture it comes from, FR-004) ----------------------------
    let (dw, dh) = (f64::from(display.0), f64::from(display.1));
    let (ratio_w, ratio_h) = o.shape.ratio().map_or((dw, dh), |(a, b)| (f64::from(a), f64::from(b)));
    let portrait = ratio_h > ratio_w;
    let wanted = o.width.unwrap_or(if portrait { target.width.portrait } else { target.width.landscape });
    let framing = if o.shape == Shape::Source { Reframe::Center } else { o.framing };
    // The widest output that needs no enlarging: the crop window for centre/smart, the whole
    // frame for blur (it is the foreground).
    let crop_w =
        if dw / dh > ratio_w / ratio_h { (dh * ratio_w / ratio_h).floor() as u32 } else { display.0 };
    let limit = if framing == Reframe::Blur { display.0 } else { crop_w };
    let width = wanted.min(limit).max(16);
    if wanted > limit {
        notes.push(if o.width.is_some() {
            format!("kept source width {limit} (a GIF is never enlarged)")
        } else {
            format!("source narrower than {wanted} px: kept {limit}")
        });
    }
    let height = ((f64::from(width) * ratio_h / ratio_w).round() as u32).max(2);
    let mut framing_applied = framing;
    if framing == Reframe::Smart
        && geometry::plan(display, (width, height), framing).overflow != Overflow::None
    {
        warnings.push("smart framing is not available for GIFs yet; using the blurred background".into());
        framing_applied = Reframe::Blur;
    }

    // --- timing ---------------------------------------------------------------------------------
    let source_rate =
        if v.is_vfr() { v.r_frame_rate.or(v.frame_rate) } else { v.frame_rate.or(v.r_frame_rate) };
    let rate = timing::cap(fps, source_rate);
    if let Some(n) = timing::approximate_note(rate) {
        notice(&mut warnings, IssueCode::TimingApproximate, n);
    }
    let len = cut.requested.end.saturating_sub(cut.requested.start).as_secs_f64();
    let frames = crate::upscale::reframe::frame_count(len, f64::from(rate.0) / f64::from(rate.1)) as u64;
    if frames < 2 {
        return Err(invalid(format!(
            "a GIF needs at least 2 frames: the range is {len:.3} s at {} fps",
            timing::fps_text(rate)
        )));
    }
    if let Some(max) = target.max_duration.filter(|m| len > m + 1e-6) {
        notice(
            &mut warnings,
            IssueCode::LongRange,
            format!(
                "the range ({len:.1} s) is longer than {} recommends ({max} s); the GIF may be large",
                target.name
            ),
        );
    }

    // --- the frame chain --------------------------------------------------------------------------
    let colour_in = ColourTags {
        space: v.color_space.clone(),
        primaries: v.color_primaries.clone(),
        transfer: v.color_transfer.clone(),
        range: v.color_range.clone(),
    };
    let tonemap = v.is_hdr();
    if tonemap {
        let lacking: Vec<&str> =
            ["zscale", "tonemap"].into_iter().filter(|f| !caps.filters.contains(*f)).collect();
        if !lacking.is_empty() {
            return Err((
                IssueCode::EngineMissing,
                format!("the ffmpeg build lacks {} needed to tone-map this HDR source", lacking.join(", ")),
            ));
        }
    }
    let gi = GraphInput {
        trim: cut.trim.map(|t| (t.start, t.end)),
        deinterlace: v.is_interlaced(),
        square_pixels: v.sar.is_some_and(|(n, d)| n != d),
        tonemap,
        keep_hdr: false,
        colour_in: graph::zscale_input(&colour_in, v.height.unwrap_or(0)),
        params_in: graph::setparams_input(&colour_in, v.height.unwrap_or(0)),
        fps_cap: None,
        geometry: geometry::plan(display, (width, height), framing_applied),
        reframe: framing_applied,
        track: None,
        gpu: None,
        flatten_alpha: v.has_alpha,
    };
    let start = Settings {
        width,
        height,
        rate,
        colours: o.colours.unwrap_or(target.colours),
        dither: o.dither.unwrap_or(target.dither),
        stats: target.stats_mode,
        lossy: if optimiser.is_some() { o.lossy.or(target.lossy).unwrap_or(0) } else { 0 },
        gifski: (engine == EngineUsed::Gifski).then(|| {
            target.gifski.unwrap_or(GifskiParams { quality: 90, motion_quality: None, lossy_quality: None })
        }),
    };
    warnings.extend(notes.iter().cloned());
    let frame = FramePlan {
        display,
        shape: o.shape,
        framing: o.framing,
        framing_applied,
        graph: gi,
        sharpen: target.sharpen,
    };
    let (optimiser, optimiser_version) = optimiser.map_or((None, None), |(p, v)| (Some(p), v));
    let gif = GifPlan {
        format: o.format,
        engine,
        engine_reason,
        gifski,
        optimiser,
        optimiser_version,
        estimate: Estimate {
            bytes: None,
            from: EstimateFrom::Pending,
            settings: start.clone(),
            lowered: Vec::new(),
            mp4_bytes: None,
        },
        start,
        floor_width: target.floor_width,
        floor_fps: target.floor_fps,
        budget,
        loop_count: o.loop_count,
        allow_trim: o.allow_trim,
        notes,
        frames,
        len,
        // Every engine passed the Phase 0 check (T013, research R14): gifski too.
        deterministic: true,
        target,
    };
    Ok(Planned { frame, gif, warnings, notices })
}

/// A rough size for the free-space check before any sample is encoded: the hard budget, or
/// a quarter byte per pixel per frame.
pub fn rough_bytes(g: &GifPlan) -> u64 {
    g.estimate
        .bytes
        .or(g.budget.map(|b| b.hard))
        .unwrap_or_else(|| u64::from(g.start.width) * u64::from(g.start.height) * g.frames / 4)
}
