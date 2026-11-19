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
use crate::time::Ts;
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

impl GifSpec {
    /// This job over the first `len` seconds of its range (the budget search's trim, FR-011).
    pub fn trimmed(&self, len: f64) -> GifSpec {
        let mut t = self.clone();
        let d = Ts::from_secs_f64(len);
        t.cut.requested.end = t.cut.requested.start + d;
        match &mut t.cut.trim {
            Some(trim) => trim.end = Some(trim.start + d),
            None => t.cut.duration = Some(d),
        }
        if let Some((start, end)) = &mut t.frame.graph.trim {
            *end = Some(*start + d);
        }
        if let Some((_, tolerance)) = t.cut.verify_video {
            t.cut.verify_video = Some((d, tolerance));
        }
        t.gif.len = len;
        t.gif.frames = crate::upscale::reframe::frame_count(len, t.gif.start.fps()) as u64;
        t
    }
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
    /// Smart framing's analysis (002's reframe), when it applies.
    pub smart: Option<SmartPlan>,
}

/// What smart framing needs: the analysis setup, and the analysis once it has run.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartPlan {
    pub detector: crate::upscale::reframe::detect::DetectorSpec,
    pub cache: Option<PathBuf>,
    pub analysis_size: (u32, u32),
    /// Where the filter timeline starts on the source's (whole videos keep their timestamps).
    pub cmd_offset: f64,
    pub motion: crate::upscale::Motion,
    /// Set by the dry run or the executor's `analyze` stage.
    pub analysis: Option<std::sync::Arc<crate::upscale::reframe::ReframeAnalysis>>,
}

impl GifSpec {
    /// The smart-framing analysis input of this job (refactor B2): the GIF's own range, graph
    /// and frame rate.
    pub fn reframe_input(&self) -> Option<crate::upscale::reframe::ReframeInput<'_>> {
        let sp = self.frame.smart.as_ref()?;
        Some(crate::upscale::reframe::ReframeInput {
            cut: &self.cut,
            graph: &self.frame.graph,
            detector: Some(&sp.detector),
            cache_dir: sp.cache.as_deref(),
            analysis_size: sp.analysis_size,
            fps_out: self.gif.start.fps(),
            display: self.frame.display,
            crop: self.frame.graph.geometry.crop,
            motion: sp.motion,
        })
    }

    /// Applies a finished analysis: the crop follows its path (`reframe.cmd`), or the whole range
    /// is the blurred background when nothing fits. Returns what to tell the user when there was
    /// no subject (FR-014).
    pub fn with_analysis(
        &mut self,
        a: std::sync::Arc<crate::upscale::reframe::ReframeAnalysis>,
    ) -> Option<&'static str> {
        let whole = a.fallback_ranges.first().is_some_and(|r| {
            r.start <= a.clip_start
                && r.end.as_secs_f64() >= a.clip_start.as_secs_f64() + a.len_secs() - 0.5 / a.fps
        });
        if whole {
            // The blurred background throughout; the analysis stays (`--crop-path`, as in 002).
            let kept = self.frame.smart.take();
            self.blur();
            self.frame.smart = kept.map(|sp| SmartPlan { analysis: Some(a), ..sp });
            return Some("smart framing found no subject that fits the shape; using the blurred background");
        }
        let none = a.detections == 0;
        let x0 = a.path.first().map_or(0, |x| (*x).max(0) as u32);
        self.frame.graph.track =
            Some(crate::upscale::graph::Track { x0, fallback: !a.fallback_ranges.is_empty() });
        if let Some(sp) = &mut self.frame.smart {
            sp.analysis = Some(a);
        }
        none.then_some("smart framing found no subject; the crop stays centred")
    }

    /// Smart framing could not run: the blurred background instead.
    pub fn blur(&mut self) {
        let size = self.frame.graph.geometry.target;
        self.frame.framing_applied = Reframe::Blur;
        self.frame.graph.reframe = Reframe::Blur;
        self.frame.graph.geometry = geometry::plan(self.frame.display, size, Reframe::Blur);
        self.frame.graph.track = None;
        self.frame.smart = None;
    }
}

impl FramePlan {
    /// The graph for an output of `size` (the budget search lowers the width).
    pub fn graph_at(&self, size: (u32, u32)) -> GraphInput {
        let mut g = self.graph.clone();
        let mut geo = geometry::plan(self.display, size, self.framing_applied);
        // The same source window at every width: a smart-framing path was found for the start
        // size's crop, and the rounded heights of lower widths would move its edges.
        if self.framing_applied == Reframe::Smart
            && let (Some(start), Some(_)) = (self.graph.geometry.crop, geo.crop)
        {
            geo.crop = Some(start);
        }
        g.geometry = geo;
        g
    }

    /// The frame chain for `s` (see [`graph::frame_chain`]).
    pub fn chain(&self, s: &Settings) -> String {
        graph::frame_chain(
            &self.graph_at((s.width, s.height)),
            &GifChain { rate: &s.rate_text(), sharpen: self.sharpen, mp4: s.crf.is_some() },
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
    /// The Reddit MP4's x264 CRF (MP4 jobs only).
    pub crf: Option<u8>,
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

    /// The `settings` object of the JSON contract (`gifski` only for gifski jobs: additive).
    pub fn to_json(&self) -> Value {
        let t = self.timing();
        let mut v = json!({
            "width": self.width,
            "height": self.height,
            "fps": (self.fps() * 1000.0).round() / 1000.0,
            "timing": { "exact": t.exact, "avg_cs": (f64::from(t.avg_cs) * 100.0).round() / 100.0 },
            "colours": self.colours,
            "dither": self.dither,
            "lossy": self.lossy,
        });
        if let Some(p) = self.gifski {
            v["gifski"] = json!(p);
        }
        if let Some(crf) = self.crf {
            v["crf"] = json!(crf);
        }
        v
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
    /// The Reddit MP4's frame rate for this range (its own output, or a GIF's MP4 hint).
    pub mp4_rate: (u32, u32),
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
    cmd_offset: f64,
) -> Result<Planned, Problem> {
    let Some(target) = targets.get(&o.target).cloned() else {
        return Err((
            IssueCode::TargetUnknown,
            format!("unknown GIF target {:?} (known: {})", o.target, targets.ids().join(", ")),
        ));
    };
    let mp4 = o.format == Format::Mp4;
    let (needs, what) = if mp4 {
        (&crate::doctor::MP4_NEEDS[..], "the Reddit MP4")
    } else {
        (&crate::doctor::GIF_NEEDS[..], "GIF export")
    };
    let missing = caps.missing(needs);
    if !missing.is_empty() {
        return Err((
            IssueCode::EngineMissing,
            format!("the ffmpeg build lacks {} needed for {what}", missing.join(", ")),
        ));
    }
    if let Some(w) = o.width.filter(|w| !(64..=1920).contains(w)) {
        return Err(invalid(format!("width {w} is out of range (64 to 1920)")));
    }
    let fps = o.fps.unwrap_or(target.fps);
    timing::validate_for(fps, mp4).map_err(invalid)?;
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
        notices.push(Notice { code, message, detail: None });
    };

    // --- engine -------------------------------------------------------------------------------
    let engines = caps.gif_engines.clone().refusing(refused);
    let wants_gifski = !mp4
        && match o.engine {
            EngineChoice::Gifski => true,
            EngineChoice::Builtin => false,
            EngineChoice::Auto => target.engine == TargetEngine::Best,
        };
    let (engine, engine_reason, gifski) = match (wants_gifski, engines.gifski.program()) {
        (false, _) if mp4 => (EngineUsed::Libx264, "Reddit MP4: libx264".to_owned(), None),
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
    if engine != EngineUsed::Builtin {
        let name = if mp4 { "the Reddit MP4" } else { "gifski" };
        for (set, flag) in [
            (o.colours.is_some(), "--colors"),
            (o.dither.is_some(), "--dither"),
            (o.lossy.is_some(), "--lossy"),
        ] {
            if set {
                notes.push(format!("{flag} is ignored by {name}"));
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
    let wanted = o.width.unwrap_or_else(|| target.width_for(o.shape.ratio().unwrap_or(display)));
    let framing = if o.shape == Shape::Source { Reframe::Center } else { o.framing };
    // The widest output that needs no enlarging: the crop window for centre/smart, the whole
    // frame for blur (it is the foreground).
    let crop_w =
        if dw / dh > ratio_w / ratio_h { (dh * ratio_w / ratio_h).floor() as u32 } else { display.0 };
    let limit = if framing == Reframe::Blur { display.0 } else { crop_w };
    let mut width = wanted.min(limit).max(16);
    if mp4 {
        width &= !1; // 4:2:0 needs even sizes
    }
    if wanted > limit {
        notes.push(if o.width.is_some() {
            format!("kept source width {limit} (a GIF is never enlarged)")
        } else {
            format!("source narrower than {wanted} px: kept {limit}")
        });
    }
    let mut height = ((f64::from(width) * ratio_h / ratio_w).round() as u32).max(2);
    if mp4 {
        height = (height / 2 * 2).max(2);
    }
    // Smart framing follows the subject with 002's analysis (FR-013, US4): it needs a crop that
    // moves sideways, the face model and the filters the moving crop uses.
    let mut framing_applied = framing;
    let mut smart = None;
    let overflow = geometry::plan(display, (width, height), framing).overflow;
    if framing == Reframe::Smart && overflow != Overflow::None {
        let lacking: Vec<&str> =
            crate::plan::SMART_FILTERS.iter().copied().filter(|f| !caps.filters.contains(*f)).collect();
        match (&caps.face, overflow) {
            (None, _) => warnings.push("face model not found; using the blurred background".into()),
            (Some(_), Overflow::Horizontal) if !lacking.is_empty() => warnings.push(format!(
                "the ffmpeg build lacks {} needed for smart framing; using the blurred background",
                lacking.join(", ")
            )),
            (Some(d), Overflow::Horizontal) => {
                smart = Some(SmartPlan {
                    detector: d.clone(),
                    cache: caps.reframe_cache.clone(),
                    analysis_size: crate::upscale::reframe::decode::analysis_size(display),
                    cmd_offset,
                    motion: crate::upscale::Motion::default(),
                    analysis: None,
                })
            }
            (Some(_), _) => warnings
                .push("smart framing follows subjects sideways only; using the blurred background".into()),
        }
        if smart.is_none() {
            framing_applied = Reframe::Blur;
        }
    }

    // --- timing ---------------------------------------------------------------------------------
    let source_rate =
        if v.is_vfr() { v.r_frame_rate.or(v.frame_rate) } else { v.frame_rate.or(v.r_frame_rate) };
    // The MP4 keeps the source's rate up to the target's `mp4.max_fps` (no centisecond limit, R6).
    let mp4_rate = timing::cap(o.fps.unwrap_or(target.mp4.max_fps), source_rate);
    let rate = if mp4 { mp4_rate } else { timing::cap(fps, source_rate) };
    if let Some(n) = timing::approximate_note(rate).filter(|_| !mp4) {
        notice(&mut warnings, IssueCode::TimingApproximate, n);
    }
    let len = cut.requested.end.saturating_sub(cut.requested.start).as_secs_f64();
    let frames = crate::upscale::reframe::frame_count(len, f64::from(rate.0) / f64::from(rate.1)) as u64;
    if frames < 2 {
        return Err(invalid(format!(
            "an animation needs at least 2 frames: the range is {len:.3} s at {} fps",
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
        crf: mp4.then_some(target.mp4.crf),
    };
    warnings.extend(notes.iter().cloned());
    let frame = FramePlan {
        display,
        shape: o.shape,
        framing: o.framing,
        framing_applied,
        graph: gi,
        sharpen: target.sharpen,
        smart,
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
        mp4_rate,
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

#[cfg(test)]
mod tests {
    //! Review item 17: the planner's checks and choices, without an engine (synthetic probe
    //! output and capabilities; the end-to-end runs are in `tests/us*_gif.rs`).
    use super::*;
    use crate::event::Span;
    use crate::gif::BudgetSpec;
    use crate::plan::streams::{Container, StreamPlan};
    use crate::probe::parse_probe_json;
    use crate::request::Mode;
    use std::path::Path;

    /// A `w`×`h` source at `fps` (progressive, SDR).
    fn media(w: u32, h: u32, fps: u32) -> MediaInfo {
        let json = json!({
            "streams": [{"index":0,"codec_type":"video","codec_name":"h264","pix_fmt":"yuv420p",
                "width":w,"height":h,"field_order":"progressive",
                "r_frame_rate":format!("{fps}/1"),"avg_frame_rate":format!("{fps}/1")}],
            "format": {"format_name":"mov,mp4,m4a,3gp,3g2,mj2","duration":"600.0"},
        });
        parse_probe_json(Path::new("in.mp4"), 1, &json.to_string()).unwrap()
    }

    /// The accurate-mode cut of `start..end` seconds.
    fn cut(start: f64, end: f64) -> ClipSpec {
        let (s, e) = (Ts::from_secs_f64(start), Ts::from_secs_f64(end));
        ClipSpec {
            input: "in.mp4".into(),
            input_seek: Some(s),
            output_seek: None,
            duration: Some(e - s),
            trim: None,
            verify_video: None,
            mode: Mode::Accurate,
            requested: Span { start: s, end: e },
            predicted: None,
            container: Container::Mp4,
            encode: None,
            streams: StreamPlan { maps: Vec::new(), subtitle_codec: None, warnings: Vec::new() },
            smart: None,
            smart_fallback: None,
        }
    }

    /// An engine with everything GIF export and the Reddit MP4 need; no gifsicle or gifski.
    fn caps() -> Caps {
        let set = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect();
        Caps {
            filters: set(&["palettegen", "paletteuse"]),
            muxers: set(&["gif", "mp4"]),
            encoders: set(&["libx264"]),
            ..Caps::default()
        }
    }

    fn plan(o: &GifOptions, c: &Caps, m: &MediaInfo, range: (f64, f64)) -> Result<Planned, Problem> {
        plan_gif(o, &GifTargetRegistry::builtin(), c, m, &cut(range.0, range.1), &[], 0.0)
    }

    fn ok(o: &GifOptions) -> Planned {
        plan(o, &caps(), &media(1920, 1080, 30), (10.0, 14.0)).unwrap()
    }

    fn err(o: &GifOptions) -> Problem {
        plan(o, &caps(), &media(1920, 1080, 30), (10.0, 14.0)).unwrap_err()
    }

    #[test]
    fn an_unknown_target_names_the_known_ones() {
        let (code, msg) = err(&GifOptions { target: "nope".into(), ..GifOptions::default() });
        assert_eq!(code, IssueCode::TargetUnknown);
        assert!(msg.contains("reddit-post") && msg.contains("reddit-comment"), "{msg}");
    }

    #[test]
    fn a_missing_engine_feature_is_named() {
        let mut c = caps();
        c.filters.remove("palettegen");
        let (code, msg) = plan(&GifOptions::default(), &c, &media(1920, 1080, 30), (0.0, 4.0)).unwrap_err();
        assert_eq!(
            (code, msg.as_str()),
            (IssueCode::EngineMissing, "the ffmpeg build lacks palettegen needed for GIF export")
        );
        let mut c = caps();
        c.encoders.clear();
        let mp4 = GifOptions { format: Format::Mp4, ..GifOptions::default() };
        let (code, msg) = plan(&mp4, &c, &media(1920, 1080, 30), (0.0, 4.0)).unwrap_err();
        assert_eq!(code, IssueCode::EngineMissing);
        assert!(msg.contains("libx264") && msg.contains("the Reddit MP4"), "{msg}");
    }

    #[test]
    fn out_of_range_options_are_refused() {
        let d = GifOptions::default;
        for (o, want) in [
            (GifOptions { width: Some(63), ..d() }, "width 63"),
            (GifOptions { width: Some(1921), ..d() }, "width 1921"),
            (GifOptions { colours: Some(1), ..d() }, "colours 1"),
            (GifOptions { colours: Some(257), ..d() }, "colours 257"),
            (GifOptions { lossy: Some(201), ..d() }, "lossy 201"),
            (GifOptions { dither: Some(Dither::Bayer(6)), ..d() }, "bayer"),
            (GifOptions { budget: Some(BudgetSpec { soft: Some(9), hard: Some(8) }), ..d() }, "soft ≤ hard"),
        ] {
            let (code, msg) = err(&o);
            assert_eq!(code, IssueCode::InvalidInput, "{msg}");
            assert!(msg.contains(want), "{msg} lacks {want}");
        }
    }

    #[test]
    fn a_budget_below_the_static_floor_needs_trimming_allowed() {
        let tiny = Some(BudgetSpec { soft: Some(1000), hard: Some(MIN_BUDGET - 1) });
        let (code, _) = err(&GifOptions { budget: tiny, ..GifOptions::default() });
        assert_eq!(code, IssueCode::BudgetUnreachable);
        let p = ok(&GifOptions { budget: tiny, allow_trim: true, ..GifOptions::default() });
        assert_eq!(p.gif.budget.map(|b| b.hard), Some(MIN_BUDGET - 1));
    }

    #[test]
    fn the_target_sets_the_start_and_the_budget() {
        let p = ok(&GifOptions::default());
        let g = &p.gif;
        assert_eq!((g.start.width, g.start.height, g.start.rate), (720, 405, (15, 1)));
        assert_eq!(g.budget.map(|b| (b.soft, b.hard)), Some((8_000_000, 20_000_000)));
        assert_eq!((g.engine, g.frames, g.len), (EngineUsed::Builtin, 60, 4.0));
        assert_eq!(g.estimate.from, EstimateFrom::Pending);
        assert_eq!(g.start.lossy, 0, "no gifsicle, so no lossy pass");
        assert_eq!(rough_bytes(g), 20_000_000, "the hard budget before any sample");
        let none = ok(&GifOptions { budget: None, ..GifOptions::default() });
        assert_eq!(rough_bytes(&none.gif), 720 * 405 * 60 / 4, "a quarter byte per pixel per frame");
    }

    /// FR-004: a GIF is never larger than the picture it comes from.
    #[test]
    fn a_narrow_source_is_never_enlarged() {
        let m = media(480, 270, 30);
        let p = plan(&GifOptions::default(), &caps(), &m, (0.0, 4.0)).unwrap();
        assert_eq!((p.gif.start.width, p.gif.start.height), (480, 270));
        assert_eq!(p.gif.notes, ["source narrower than 720 px: kept 480"]);
        let asked = GifOptions { width: Some(1280), ..GifOptions::default() };
        let p = plan(&asked, &caps(), &m, (0.0, 4.0)).unwrap();
        assert_eq!(p.gif.notes, ["kept source width 480 (a GIF is never enlarged)"]);
        assert!(p.warnings.contains(&p.gif.notes[0]), "notes are warnings too");
    }

    #[test]
    fn the_reddit_mp4_has_even_sizes_and_keeps_the_source_rate() {
        let o = GifOptions { format: Format::Mp4, width: Some(301), ..GifOptions::default() };
        let p = ok(&o);
        let s = &p.gif.start;
        assert_eq!((s.width % 2, s.height % 2), (0, 0), "{}x{}", s.width, s.height);
        assert_eq!((s.width, s.rate, s.crf), (300, (30, 1), Some(23)));
        assert_eq!(p.gif.engine, EngineUsed::Libx264);
    }

    #[test]
    fn the_frame_rate_never_exceeds_the_source() {
        let p = plan(&GifOptions::default(), &caps(), &media(1920, 1080, 10), (0.0, 4.0)).unwrap();
        assert_eq!((p.gif.start.rate, p.gif.frames), ((10, 1), 40));
    }

    #[test]
    fn a_missing_gifski_falls_back_unless_strict() {
        let o = GifOptions { engine: EngineChoice::Gifski, ..GifOptions::default() };
        let p = ok(&o);
        assert_eq!(p.gif.engine, EngineUsed::Builtin);
        let fallback =
            p.notices.iter().find(|n| n.code == IssueCode::EngineFallback).expect("a fallback notice");
        assert!(fallback.message.starts_with("gifski is not available"), "{}", fallback.message);
        let (code, msg) = err(&GifOptions { strict_engine: true, ..o });
        assert_eq!(code, IssueCode::EngineMissing);
        assert!(msg.ends_with("--strict-engine is set"), "{msg}");
    }

    #[test]
    fn lossy_without_gifsicle_is_ignored_unless_strict() {
        let o = GifOptions { lossy: Some(40), ..GifOptions::default() };
        let p = ok(&o);
        assert_eq!(p.gif.start.lossy, 0);
        assert!(p.warnings.iter().any(|w| w.starts_with("--lossy needs gifsicle")), "{:?}", p.warnings);
        let (code, _) = err(&GifOptions { strict_engine: true, ..o });
        assert_eq!(code, IssueCode::EngineMissing);
    }

    #[test]
    fn gif_only_flags_are_noted_as_ignored_by_the_mp4() {
        let o = GifOptions {
            format: Format::Mp4,
            colours: Some(64),
            dither: Some(Dither::None),
            ..GifOptions::default()
        };
        assert_eq!(
            ok(&o).gif.notes,
            ["--colors is ignored by the Reddit MP4", "--dither is ignored by the Reddit MP4"]
        );
    }

    #[test]
    fn a_range_too_short_or_too_long_is_reported() {
        let (code, msg) =
            plan(&GifOptions::default(), &caps(), &media(1920, 1080, 30), (0.0, 0.05)).unwrap_err();
        assert_eq!(code, IssueCode::InvalidInput);
        assert!(msg.starts_with("an animation needs at least 2 frames"), "{msg}");
        let long = plan(&GifOptions::default(), &caps(), &media(1920, 1080, 30), (0.0, 20.0)).unwrap();
        assert!(long.notices.iter().any(|n| n.code == IssueCode::LongRange), "{:?}", long.notices);
    }

    #[test]
    fn trimming_shortens_the_range_and_the_frame_count() {
        let mut spec = GifSpec {
            cut: cut(10.0, 14.0),
            frame: ok(&GifOptions::default()).frame,
            gif: ok(&GifOptions::default()).gif,
        };
        spec.cut.verify_video = Some((Ts::from_secs_f64(4.0), Ts::from_micros(40_000)));
        let t = spec.trimmed(2.0);
        assert_eq!(t.cut.requested.end, Ts::from_secs_f64(12.0));
        assert_eq!(t.cut.duration, Some(Ts::from_secs_f64(2.0)));
        assert_eq!(t.cut.verify_video.map(|v| v.0), Some(Ts::from_secs_f64(2.0)));
        assert_eq!((t.gif.len, t.gif.frames), (2.0, 30));
    }
}
