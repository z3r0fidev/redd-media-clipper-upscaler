//! Turning a `BatchRequest` into concrete, validated jobs (no files are written here).

pub mod ffargs;
pub mod naming;
pub mod streams;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::error::{Error, IssueCode};
use crate::event::{JobId, Span};
use crate::exec::commit::{scratch_path, temp_path};
use crate::preset::{Preset, PresetRegistry, VideoCodec};
use crate::probe::{self, MediaInfo};
use crate::request::{
    BatchRequest, ClipRequest, DEFAULT_NAME_TEMPLATE, Mode, OnExists, Origin, SourceRequest,
    UPSCALE_CLIP_NAME_TEMPLATE, UPSCALE_NAME_TEMPLATE,
};
use crate::time::{EndSpec, Ts};
use crate::tools::Tools;
use crate::upscale::encode::{self, ColourTags, EncodeParams};
use crate::upscale::geometry::{self, Overflow};
use crate::upscale::graph::{GraphInput, Track};
use crate::upscale::reframe;
use crate::upscale::tier::{Tier, TierDecision};
use crate::upscale::{
    AnalysisState, HdrMode, Reframe, TierChoice, UpscaleOptions, UpscalePlan, UpscaleSpec, estimate,
};
use ffargs::ClipSpec;
use streams::Container;

/// Filters smart reframe needs besides the common upscale ones (research R5, R6).
const SMART_FILTERS: [&str; 6] = ["sendcmd", "scdet", "metadata", "split", "overlay", "crop"];

/// How far before the target an approximate (coarse) input seek lands.
const COARSE_SEEK_MARGIN: Ts = Ts::from_micros(10_000_000);

/// What this machine can do: the engine's encoders and filters (from `doctor::caps`), the model
/// files found, and the cached GPU capability record (refactor A4).
#[derive(Debug, Clone, Default)]
pub struct Caps {
    pub encoders: BTreeSet<String>,
    pub filters: BTreeSet<String>,
    pub models: crate::models::ModelSet,
    pub capability: Option<crate::upscale::capability::CapabilityRecord>,
    /// The face detector smart reframe uses (`None`: no model, smart falls back to blur).
    pub face: Option<crate::upscale::reframe::detect::DetectorSpec>,
    /// Where reframe analyses are cached (`None`: no cache).
    pub reframe_cache: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Issue {
    pub severity: Severity,
    pub code: IssueCode,
    pub origin: Origin,
    pub message: String,
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.origin, self.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Disposition {
    Run,
    Skip { reason: String },
}

/// The work a job performs. Feature 003 adds a `Gif` variant.
#[derive(Debug, Clone, PartialEq)]
pub enum JobSpec {
    Clip(Box<ClipSpec>),
    Upscale(Box<crate::upscale::UpscaleSpec>),
}

/// What a job occupies while it runs. Every job takes one of the `--jobs` slots; GPU jobs also
/// take the single GPU slot, so two GPU jobs never share the device (research R9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Cpu,
    Gpu,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PlannedJob {
    pub id: JobId,
    #[serde(serialize_with = "crate::pathser::path")]
    pub source: PathBuf,
    #[serde(serialize_with = "crate::pathser::path")]
    pub output: PathBuf,
    #[serde(skip)]
    pub temp: PathBuf,
    /// Per-job scratch directory (created, journaled and removed by the scheduler), for jobs that
    /// need working files such as a `sendcmd` script. `None` for clips.
    #[serde(skip)]
    pub scratch: Option<PathBuf>,
    pub on_exists: OnExists,
    pub disposition: Disposition,
    /// The span of the source this job processes (user time base).
    pub requested: Span,
    pub warnings: Vec<String>,
    #[serde(skip)]
    pub spec: JobSpec,
}

impl PlannedJob {
    /// The exact ffmpeg arguments this job will run.
    pub fn args(&self) -> Vec<OsString> {
        match &self.spec {
            JobSpec::Clip(c) => ffargs::clip_args(c, &self.temp),
            JobSpec::Upscale(u) => ffargs::upscale_args(u, &self.temp),
        }
    }

    /// Media seconds this job produces (for overall progress weighting).
    pub fn media_len(&self) -> Ts {
        self.requested.end.saturating_sub(self.requested.start)
    }

    /// The scarce resource this job needs besides a job slot.
    pub fn resource(&self) -> Resource {
        match &self.spec {
            JobSpec::Clip(_) => Resource::Cpu,
            JobSpec::Upscale(u) => match u.up.tier.tier {
                crate::upscale::tier::Tier::Cpu => Resource::Cpu,
                _ => Resource::Gpu,
            },
        }
    }

    /// The job kind as it appears in JSON (`"clip"`).
    pub fn kind(&self) -> &'static str {
        match &self.spec {
            JobSpec::Clip(_) => "clip",
            JobSpec::Upscale(_) => "upscale",
        }
    }

    /// Clip jobs: the cutting mode.
    pub fn mode(&self) -> Option<Mode> {
        match &self.spec {
            JobSpec::Clip(c) => Some(c.mode),
            JobSpec::Upscale(_) => None,
        }
    }

    /// Fast clip jobs: the span the copy is expected to produce.
    pub fn predicted(&self) -> Option<Span> {
        match &self.spec {
            JobSpec::Clip(c) => c.predicted,
            JobSpec::Upscale(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Plan {
    pub run_id: String,
    pub jobs: Vec<PlannedJob>,
    pub issues: Vec<Issue>,
    /// Planning was interrupted; the plan is incomplete and must not be run.
    pub cancelled: bool,
}

impl Plan {
    pub fn has_errors(&self) -> bool {
        self.cancelled || self.issues.iter().any(|i| i.severity == Severity::Error)
    }

    /// True when the only errors are engine capability problems (exit code 4).
    pub fn engine_errors_only(&self) -> bool {
        let mut errors = self.issues.iter().filter(|i| i.severity == Severity::Error).peekable();
        errors.peek().is_some() && errors.all(|i| i.code == IssueCode::EngineMissing)
    }
}

/// Validates every request and resolves outputs. Reports all problems at once (FR-003).
/// Upscale presets come from the built-ins plus the user's preset folder.
pub async fn plan(req: &BatchRequest, tools: &Tools, caps: &Caps, cancel: &CancellationToken) -> Plan {
    let presets = PresetRegistry::load(crate::settings::config_dir().as_deref());
    plan_with(req, tools, caps, presets, cancel).await
}

/// [`plan`] with an explicit preset registry (tests use the built-ins only).
pub async fn plan_with(
    req: &BatchRequest,
    tools: &Tools,
    caps: &Caps,
    presets: PresetRegistry,
    cancel: &CancellationToken,
) -> Plan {
    let mut planner = Planner {
        tools,
        caps,
        cancel,
        run_id: new_run_id(),
        jobs: Vec::new(),
        issues: Vec::new(),
        claimed: HashSet::new(),
        sources: HashSet::new(),
        probes: HashMap::new(),
        presets,
    };
    for s in &req.sources {
        if let Ok(c) = std::fs::canonicalize(&s.path) {
            planner.sources.insert(path_key(&c));
        }
    }
    for source in &req.sources {
        planner.plan_source(req, source).await;
    }
    planner.check_free_space();
    Plan {
        run_id: planner.run_id,
        jobs: planner.jobs,
        issues: planner.issues,
        cancelled: cancel.is_cancelled(),
    }
}

struct Planner<'a> {
    tools: &'a Tools,
    caps: &'a Caps,
    cancel: &'a CancellationToken,
    run_id: String,
    jobs: Vec<PlannedJob>,
    issues: Vec<Issue>,
    /// Outputs already assigned in this plan (normalized keys).
    claimed: HashSet<std::ffi::OsString>,
    /// Canonical source paths (normalized keys); outputs may never equal one.
    sources: HashSet<std::ffi::OsString>,
    probes: HashMap<PathBuf, Result<MediaInfo, String>>,
    presets: PresetRegistry,
}

impl Planner<'_> {
    fn error(&mut self, origin: &Origin, code: IssueCode, message: impl Into<String>) {
        self.issues.push(Issue {
            severity: Severity::Error,
            code,
            origin: origin.clone(),
            message: message.into(),
        });
    }

    async fn probe(&mut self, path: &Path) -> Result<MediaInfo, String> {
        if let Some(r) = self.probes.get(path) {
            return r.clone();
        }
        let r = match probe::probe(self.tools, path, self.cancel).await {
            Ok(m) if m.is_video() => Ok(m),
            Ok(_) => Err(format!("{} has no video stream", path.display())),
            Err(Error::SourceMissing(p)) => Err(format!("source not found: {}", p.display())),
            Err(Error::Cancelled) => Err("cancelled".into()),
            Err(e) => Err(e.to_string()),
        };
        self.probes.insert(path.to_path_buf(), r.clone());
        r
    }

    async fn plan_source(&mut self, req: &BatchRequest, source: &SourceRequest) {
        let ext =
            source.path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if matches!(ext.as_str(), "m3u8" | "m3u" | "ffconcat" | "sdp") {
            return self.error(
                &source.origin,
                IssueCode::InvalidInput,
                format!("{} is a playlist/stream description, not a video file", source.path.display()),
            );
        }
        let media = match self.probe(&source.path).await {
            Ok(m) => m,
            Err(msg) => {
                let code =
                    if source.path.exists() { IssueCode::ProbeFailed } else { IssueCode::SourceMissing };
                self.error(&source.origin, code, msg);
                return;
            }
        };
        let base = source.overrides.apply(&req.defaults);
        for (i, clip) in source.clips.iter().enumerate() {
            if self.cancel.is_cancelled() {
                return;
            }
            let opts = clip.overrides.apply(&base);
            self.plan_clip(source, &media, i + 1, clip, &opts).await;
        }
    }

    async fn plan_clip(
        &mut self,
        source: &SourceRequest,
        media: &MediaInfo,
        index: usize,
        clip: &ClipRequest,
        opts: &crate::request::Options,
    ) {
        let origin = &clip.origin;
        let mut warnings = Vec::new();

        // --- upscaling (feature 002) ---------------------------------------------------------
        let up = match &opts.upscale {
            Some(u) => match self.check_upscale(origin, u, media) {
                Some(preset) => Some((u, preset)),
                None => return,
            },
            None => None,
        };
        let whole = clip.start == Ts::ZERO && clip.end == EndSpec::EndOfMedia;
        // Upscaling always re-encodes, so it cuts like accurate mode (and `mode` is ignored).
        let mode = if up.is_some() { Mode::Accurate } else { opts.mode };

        // --- times ---------------------------------------------------------------------------
        let duration = media.duration;
        if let Some(d) = duration
            && clip.start >= d
        {
            return self.error(
                origin,
                IssueCode::InvalidInput,
                format!("start ({}) is at or beyond the end of the video ({})", clip.start, d),
            );
        }
        let mut end = match clip.end {
            EndSpec::At(t) => Some(t),
            EndSpec::Duration(d) if d == Ts::ZERO => {
                return self.error(origin, IssueCode::InvalidInput, "duration must be greater than zero");
            }
            EndSpec::Duration(d) => Some(clip.start + d),
            EndSpec::EndOfMedia => None,
        };
        if let Some(e) = end
            && e <= clip.start
        {
            return self.error(
                origin,
                IssueCode::InvalidInput,
                format!("end ({e}) is not after start ({})", clip.start),
            );
        }
        if let (Some(e), Some(d)) = (end, duration)
            && e > d
        {
            warnings.push(format!("end {e} is past the end of the video; clip ends at {d}"));
            end = None;
        }
        let requested_end = match (end, duration) {
            (Some(e), _) => e,
            (None, Some(d)) => d,
            (None, None) => {
                return self.error(
                    origin,
                    IssueCode::InvalidInput,
                    "the video's duration is unknown; give an explicit end time",
                );
            }
        };

        // --- container & encoding ------------------------------------------------------------
        let ext = opts
            .container
            .clone()
            .map(|c| c.trim_start_matches('.').to_ascii_lowercase())
            .unwrap_or_else(|| media.container.clone());
        let ext = match (&up, &opts.container) {
            (None, _) => ext,
            (Some(_), Some(c)) if !c.trim_start_matches('.').eq_ignore_ascii_case("mp4") => {
                warnings.push(format!("upscaled outputs are always .mp4 (ignoring container .{ext})"));
                "mp4".to_owned()
            }
            (Some(_), _) => "mp4".to_owned(),
        };
        let (mut container, mut out_ext) = match Container::from_ext(&ext) {
            Some(c) => (c, ext.clone()),
            None if opts.container.is_some() => {
                return self.error(
                    origin,
                    IssueCode::InvalidInput,
                    format!("unsupported output container .{ext}"),
                );
            }
            None => {
                warnings.push(format!("unknown container .{ext}; writing .mkv"));
                (Container::Mkv, "mkv".to_owned())
            }
        };
        let mut encode = None;
        if mode == Mode::Accurate && up.is_none() {
            if container.encode_profile().is_none() {
                warnings.push(format!(
                    "accurate mode cannot write .{out_ext}; writing .mp4 instead (choose --container to override)"
                ));
                container = Container::Mp4;
                out_ext = "mp4".to_owned();
            }
            let profile = container.encode_profile().expect("checked above");
            for enc in [profile.video_encoder, profile.audio_encoder] {
                if !self.caps.encoders.contains(enc) {
                    return self.error(
                        origin,
                        IssueCode::EngineMissing,
                        format!("the ffmpeg build lacks the {enc} encoder needed for accurate mode"),
                    );
                }
            }
            encode = Some(profile);
            if let Some(fmt) = media.first_video().and_then(|v| v.pix_fmt.as_deref())
                && ["10", "12", "16"].iter().any(|d| fmt.contains(d))
            {
                warnings.push(format!(
                    "source is high bit depth ({fmt}); accurate mode writes 8-bit 4:2:0 (HDR is not preserved — use fast mode to keep it)"
                ));
            }
        }

        let mut stream_plan = match streams::select_streams(media, &opts.streams, container, mode) {
            Ok(p) => p,
            Err(msg) => return self.error(origin, IssueCode::InvalidInput, msg),
        };
        warnings.extend(stream_plan.warnings.iter().cloned());

        // --- output path ---------------------------------------------------------------------
        // Absolute paths: upscale jobs run ffmpeg inside their scratch directory.
        let input = std::path::absolute(&source.path).unwrap_or_else(|_| source.path.clone());
        let source_dir = input.parent().map(Path::to_path_buf).unwrap_or_default();
        let out_dir = match &opts.output_dir {
            Some(d) if d.is_absolute() => d.clone(),
            Some(d) => std::path::absolute(d).unwrap_or_else(|_| d.clone()),
            None => source_dir.join(if up.is_some() && whole { "upscaled" } else { "clips" }),
        };
        let template = match &up {
            Some(_) if opts.name_template == DEFAULT_NAME_TEMPLATE && whole => UPSCALE_NAME_TEMPLATE,
            Some(_) if opts.name_template == DEFAULT_NAME_TEMPLATE => UPSCALE_CLIP_NAME_TEMPLATE,
            _ => opts.name_template.as_str(),
        };
        let stem = source.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let rel = match naming::render(
            template,
            &naming::NameVars {
                source_stem: &stem,
                index,
                name: clip.name.as_deref(),
                start: clip.start,
                end: requested_end,
                ext: &out_ext,
                preset: up.as_ref().map(|(u, _)| u.preset.as_str()),
            },
        ) {
            Ok(p) => p,
            Err(msg) => return self.error(origin, IssueCode::InvalidInput, msg),
        };
        let Some((output, disposition)) = self.claim_output(
            origin,
            out_dir.join(rel),
            opts.on_exists,
            if up.is_some() { "jobs" } else { "clips" },
            &mut warnings,
        ) else {
            return;
        };

        // --- seek plan (research.md R3) -----------------------------------------------------
        let requested = Span { start: clip.start, end: requested_end };
        // Decide from the demuxer ffprobe actually used, never the extension (a mislabelled
        // MPEG-TS named .mp4 must still get the two-stage seek).
        let source_precise = media.has_indexed_seeking();
        let frame = media.frame_duration().unwrap_or(Ts::from_micros(40_000));
        // Only argument combinations whose timing does not depend on how ffmpeg relates input
        // `-ss` to the container start are used (research.md R3).
        let mut trim = None;
        let (input_seek, output_seek, cut, predicted) = if clip.start == Ts::ZERO {
            (None, None, Ts::ZERO, None)
        } else if mode == Mode::Accurate && source_precise {
            // Index-based demuxer seek + decode is frame exact.
            (Some(clip.start), None, clip.start, None)
        } else if mode == Mode::Accurate {
            // Approximate seek: land well before the start, keep absolute timestamps and cut with
            // trim/atrim filters at absolute times — exact wherever the seek landed.
            let zero = media.zero_point();
            trim = Some(ffargs::Trim { start: zero + clip.start, end: end.map(|e| zero + e) });
            if stream_plan.maps.iter().any(|m| m.kind == crate::probe::StreamKind::Subtitle) {
                stream_plan.maps.retain(|m| m.kind != crate::probe::StreamKind::Subtitle);
                stream_plan.subtitle_codec = None;
                warnings.push(
                    "subtitles cannot be cut frame-accurately from this kind of source; dropped".into(),
                );
            }
            (Some(clip.start.saturating_sub(COARSE_SEEK_MARGIN)), None, clip.start, None)
        } else {
            // Stream copy must start on a keyframe: find it exactly, then cut just before it
            // (ffmpeg drops leading non-keyframes, so the output starts precisely at it).
            let key = match probe::keyframe_at_or_before(self.tools, media, clip.start, self.cancel).await {
                Ok(Some(k)) => k,
                Ok(None) => {
                    return self.error(
                        origin,
                        IssueCode::InvalidInput,
                        format!("no keyframe found before {}; use --mode accurate for this clip", clip.start),
                    );
                }
                Err(Error::Cancelled) => return,
                Err(e) => return self.error(origin, e.issue_code(), e.to_string()),
            };
            let gap = key.previous.map_or(Ts::from_micros(i64::MAX / 4), |p| key.at - p);
            let lead = Ts::from_micros((frame.as_micros() * 4).min(gap.as_micros() / 2));
            let predicted = Some(Span { start: key.at, end: requested_end });
            if source_precise {
                // Demuxer seek to S lands exactly on K (≤ S) for indexed containers.
                (Some(clip.start), None, clip.start, predicted)
            } else {
                // No demuxer seek: copy packets from the start (cheap) and cut just before K;
                // leading non-keyframes are dropped, so the output starts exactly on K.
                let cut = key.at.saturating_sub(lead);
                (None, Some(cut), cut, predicted)
            }
        };

        // ffmpeg's `-ss` timeline starts at the container start; user time starts at the first
        // video frame. Shift every seek by the difference (non-zero e.g. when audio leads).
        let shift = media.zero_point().saturating_sub(media.start_time);
        let (input_seek, output_seek) = if trim.is_some() {
            (input_seek, output_seek)
        } else {
            (input_seek.map(|t| t + shift), output_seek.map(|t| t + shift))
        };

        let id = self.jobs.len() as JobId + 1;
        let temp = temp_path(&output, &self.run_id, id);
        let cut = ClipSpec {
            input,
            input_seek,
            output_seek,
            duration: if trim.is_some() { None } else { end.map(|e| e - cut) },
            trim,
            verify_video: self.expected_video(media, mode, clip.start, end, frame),
            mode,
            requested,
            predicted,
            container,
            encode,
            streams: stream_plan,
        };
        let (spec, scratch) = match up {
            None => (JobSpec::Clip(Box::new(cut)), None),
            Some((u, preset)) => {
                let plan = self.upscale_plan(origin, u, preset, media, &cut, whole, &mut warnings);
                let scratch = scratch_path(&output, &self.run_id, id);
                (JobSpec::Upscale(Box::new(UpscaleSpec { cut, up: plan })), Some(scratch))
            }
        };
        self.jobs.push(PlannedJob {
            id,
            source: source.path.clone(),
            output,
            temp,
            scratch,
            on_exists: opts.on_exists,
            disposition,
            requested,
            warnings,
            spec,
        });
    }

    /// Upscale checks that need no output path: the preset exists, HDR can be kept, the tier can
    /// run here, and the engine has what the job needs (FR-008, research R4). Reports an issue
    /// and returns `None` when the job cannot be planned.
    fn check_upscale(&mut self, origin: &Origin, u: &UpscaleOptions, media: &MediaInfo) -> Option<Preset> {
        let Some(preset) = self.presets.get(&u.preset).cloned() else {
            let known = self.presets.ids().join(", ");
            self.error(
                origin,
                IssueCode::PresetUnknown,
                format!("unknown preset {:?} (known: {known})", u.preset),
            );
            return None;
        };
        if u.hdr == HdrMode::Keep && !preset.hdr_capable {
            self.error(
                origin,
                IssueCode::HdrUnsupported,
                format!("preset {} cannot keep HDR; use --hdr sdr or an HDR-capable preset", preset.id),
            );
            return None;
        }
        if let Some(tier) = u.tier.forced().filter(|t| *t != Tier::Cpu) {
            let name = format!("{tier:?}").to_lowercase();
            self.error(
                origin,
                IssueCode::TierUnavailable,
                format!("the {name} tier is not available on this machine"),
            );
            return None;
        }
        let v = media.first_video()?;
        let encoder = match preset.video.codec {
            VideoCodec::Hevc => "libx265",
            VideoCodec::H264 => "libx264",
        };
        let mut filters = vec!["zscale", "cas", "hqdn3d", "aac"];
        if v.is_interlaced() {
            filters.push("bwdif");
        }
        if v.is_hdr() && u.hdr == HdrMode::Sdr {
            filters.push("tonemap");
        }
        if u.reframe != Reframe::Center {
            filters.push("gblur");
        }

        let missing: Vec<&str> = std::iter::once(encoder)
            .chain(filters)
            .filter(|f| !self.caps.encoders.contains(*f) && !self.caps.filters.contains(*f))
            .collect();
        if !missing.is_empty() {
            self.error(
                origin,
                IssueCode::EngineMissing,
                format!("the ffmpeg build lacks {} needed for upscaling", missing.join(", ")),
            );
            return None;
        }
        Some(preset)
    }

    /// Resolves geometry, colour, frame rate, encoder, estimate and warnings for an upscale job
    /// (research R2–R5, R12). `cut` is the job's source range.
    #[allow(clippy::too_many_arguments)]
    fn upscale_plan(
        &mut self,
        origin: &Origin,
        u: &UpscaleOptions,
        preset: Preset,
        media: &MediaInfo,
        cut: &ClipSpec,
        whole: bool,
        warnings: &mut Vec<String>,
    ) -> UpscalePlan {
        let v = media.first_video().expect("checked: the source has video");
        let target = (preset.width, preset.height);
        let display = v.display_size().unwrap_or(target);
        // Smart reframe pans horizontally (v1): it needs a face model and a source wider than
        // the target. Otherwise it degrades to a reframe that never cuts anything off.
        let overflow = geometry::plan(display, target, u.reframe).overflow;
        let smart_missing: Vec<&str> =
            SMART_FILTERS.iter().copied().filter(|f| !self.caps.filters.contains(*f)).collect();
        let effective = match (u.reframe, overflow, &self.caps.face) {
            (Reframe::Smart, Overflow::Horizontal, None) => {
                warnings.push("face model not found; using blurred background".into());
                Reframe::Blur
            }
            (Reframe::Smart, Overflow::Horizontal, Some(_)) if !smart_missing.is_empty() => {
                warnings.push(format!(
                    "the ffmpeg build lacks {} needed for smart reframe; using blurred background",
                    smart_missing.join(", ")
                ));
                Reframe::Blur
            }
            (Reframe::Smart, Overflow::Vertical, _) => {
                warnings.push("smart reframe pans horizontally only; using a centre crop".into());
                Reframe::Center
            }
            (r, ..) => r,
        };
        let geometry = geometry::plan(display, target, effective);
        let smart = effective == Reframe::Smart && geometry.overflow == Overflow::Horizontal;
        if geometry.no_upscale_needed {
            warnings.push(format!(
                "no upscaling needed: the source ({}x{}) is already at least {}x{}; it is only reframed and scaled down",
                display.0, display.1, target.0, target.1
            ));
        }
        if display.0.min(display.1) <= 240 {
            warnings.push(format!(
                "very small source ({}x{}): upscaling cannot invent detail",
                display.0, display.1
            ));
        }
        let colour_in = ColourTags {
            space: v.color_space.clone(),
            primaries: v.color_primaries.clone(),
            transfer: v.color_transfer.clone(),
            range: v.color_range.clone(),
        };
        let keep_hdr = u.hdr == HdrMode::Keep && v.is_hdr();
        let tonemap = u.hdr == HdrMode::Sdr && v.is_hdr();
        if keep_hdr {
            warnings.push(
                "HDR is kept without its mastering metadata (MaxCLL/mastering display are not carried)"
                    .into(),
            );
        }
        if u.hdr == HdrMode::Keep && !v.is_hdr() {
            warnings.push("the source is SDR; it stays SDR".into());
        }
        let src_fps = v.frame_rate.map(|(n, d)| f64::from(n) / f64::from(d));
        let fps_cap = src_fps.filter(|f| *f > f64::from(preset.max_fps) + 0.01).map(|_| preset.max_fps);
        let fps_out = match (fps_cap, src_fps) {
            (Some(c), _) => f64::from(c),
            (None, Some(f)) => f,
            (None, None) => 30.0,
        };
        let graph = GraphInput {
            trim: cut.trim.map(|t| (t.start, t.end)),
            deinterlace: v.is_interlaced(),
            square_pixels: v.sar.is_some_and(|(n, d)| n != d),
            tonemap,
            keep_hdr,
            colour_in: crate::upscale::graph::zscale_input(&colour_in, v.height.unwrap_or(0)),
            fps_cap,
            geometry,
            reframe: effective,
            // Until the analysis runs: centred, with the fallback branch available.
            track: smart.then(|| Track { x0: geometry.crop.map_or(0, |c| c.x), fallback: true }),
        };
        let audio: Vec<&crate::probe::StreamInfo> = cut
            .streams
            .maps
            .iter()
            .filter(|m| m.kind == crate::probe::StreamKind::Audio)
            .filter_map(|m| media.streams.iter().find(|s| s.index == m.index))
            .collect();
        // Copying is exact only for whole videos; a cut re-encodes so audio starts with the frame.
        let audio_copy = preset.audio.copy_if_aac && whole && audio.iter().all(|s| s.codec == "aac");
        let audio_kbps = (!audio.is_empty()).then_some(preset.audio.bitrate_k);
        let len = cut.requested.end.saturating_sub(cut.requested.start).as_secs_f64();
        let est =
            estimate::estimate(&preset, fps_out, len, audio_kbps, estimate::cpu_fps(target.0, target.1));
        let limit_warnings = estimate::limit_warnings(&preset, &est, len);
        for w in &limit_warnings {
            self.issues.push(Issue {
                severity: Severity::Warning,
                code: IssueCode::LimitExceeded,
                origin: origin.clone(),
                message: w.clone(),
            });
        }
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get() as u32).clamp(2, 8);
        let video_args = encode::video_args(&EncodeParams {
            preset: &preset,
            crf: u.crf,
            hdr_keep: keep_hdr.then_some(&colour_in),
            fps_out,
            threads,
            maxrate_kbps: estimate::maxrate_for_limit(&preset, &est, len, audio_kbps),
        });
        let expected_video =
            if whole { v.duration.or(media.duration) } else { Some(cut.requested.end - cut.requested.start) };
        let reason = match u.tier {
            TierChoice::Auto => "auto: the classical CPU tier",
            _ => "forced: cpu",
        };
        UpscalePlan {
            target,
            geometry,
            reframe: u.reframe,
            reframe_applied: effective,
            motion: u.motion,
            analysis: if smart { AnalysisState::Pending } else { AnalysisState::NotNeeded },
            detector: if smart { self.caps.face.clone() } else { None },
            analysis_size: reframe::decode::analysis_size(display),
            reframe_cache: self.caps.reframe_cache.clone(),
            // Whole videos keep their timestamps: the video may start after the audio.
            cmd_offset: if whole { (media.zero_point() - media.start_time).as_secs_f64() } else { 0.0 },
            tier: TierDecision { tier: Tier::Cpu, reason: reason.into(), suggest: None },
            hdr: u.hdr,
            colour_in,
            graph,
            video_args,
            audio_copy,
            fps_out,
            expected_video,
            estimate: est,
            limit_warnings,
            preset,
        }
    }

    /// Warns when an output folder lacks room for the upscale outputs planned into it (FR-021).
    fn check_free_space(&mut self) {
        let mut needs: std::collections::BTreeMap<PathBuf, u64> = std::collections::BTreeMap::new();
        for job in &self.jobs {
            if let (JobSpec::Upscale(u), Some(dir)) = (&job.spec, job.output.parent())
                && job.disposition == Disposition::Run
            {
                *needs.entry(dir.to_path_buf()).or_default() += u.up.estimate.bytes;
            }
        }
        for message in estimate::check_space(&needs, estimate::free_space) {
            self.issues.push(Issue {
                severity: Severity::Warning,
                code: IssueCode::DiskFull,
                origin: Origin::Cli,
                message,
            });
        }
    }

    /// For accurate clips with an explicit end inside the video stream: the video length the output
    /// must have, with a tolerance of 3 frames (plus 100 ms).
    fn expected_video(
        &self,
        media: &MediaInfo,
        mode: Mode,
        start: Ts,
        end: Option<Ts>,
        frame: Ts,
    ) -> Option<(Ts, Ts)> {
        let end = end?;
        let video_len = media.first_video()?.duration?;
        (mode == Mode::Accurate && end <= video_len)
            .then(|| (end - start, Ts::from_micros(frame.as_micros() * 3 + 100_000)))
    }

    /// Resolves collisions for `output` (source files, outputs claimed earlier in this plan, files
    /// on disk) under the `on_exists` policy and claims the final path. Reports an issue and
    /// returns `None` when the job cannot get an output. `what` names the jobs in messages.
    fn claim_output(
        &mut self,
        origin: &Origin,
        mut output: PathBuf,
        on_exists: OnExists,
        what: &str,
        warnings: &mut Vec<String>,
    ) -> Option<(PathBuf, Disposition)> {
        if self.is_source(&output) {
            self.error(
                origin,
                IssueCode::InvalidInput,
                format!("output {} would overwrite a source video", output.display()),
            );
            return None;
        }
        if self.claimed.contains(&path_key(&output)) {
            if on_exists == OnExists::Rename {
                output = self.free_name(&output);
            } else {
                self.error(
                    origin,
                    IssueCode::InvalidInput,
                    format!(
                        "two {what} would write {}; use a name template with {{index}} or --on-exists rename",
                        output.display()
                    ),
                );
                return None;
            }
        }
        let mut disposition = Disposition::Run;
        if output.exists() {
            match on_exists {
                OnExists::Skip => disposition = Disposition::Skip { reason: "output already exists".into() },
                OnExists::Overwrite => warnings.push("existing output will be replaced".into()),
                OnExists::Rename => output = self.free_name(&output),
            }
        }
        self.claimed.insert(path_key(&output));
        Some((output, disposition))
    }

    fn is_source(&self, output: &Path) -> bool {
        // Canonicalize the nearest existing ancestor (resolving symlinked folders), then re-append
        // the parts that do not exist yet.
        let mut existing = output.to_path_buf();
        let mut rest = Vec::new();
        while !existing.exists() {
            match (existing.file_name().map(ToOwned::to_owned), existing.parent()) {
                (Some(name), Some(parent)) => {
                    rest.push(name);
                    existing = parent.to_path_buf();
                }
                _ => break,
            }
        }
        let mut resolved = std::fs::canonicalize(&existing).unwrap_or(existing);
        for part in rest.iter().rev() {
            resolved.push(part);
        }
        self.sources.contains(&path_key(&resolved))
    }

    /// `name_1.ext`, `name_2.ext`, … not on disk and not claimed in this plan.
    fn free_name(&self, path: &Path) -> PathBuf {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        (1..)
            .map(|i| path.with_file_name(format!("{stem}_{i}{ext}")))
            .find(|p| !p.exists() && !self.claimed.contains(&path_key(p)))
            .expect("unbounded search")
    }
}

/// Normalized comparison key (case-insensitive on Windows).
use crate::discover::path_key;

/// 64-bit run id. `RandomState` is keyed from the OS RNG, so temp names are not predictable
/// (prevents pre-planting a symlink at a temp path in a shared output folder).
fn new_run_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0),
    );
    h.write_u32(std::process::id());
    format!("{:016x}", h.finish())
}
