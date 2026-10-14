//! Turning a `BatchRequest` into concrete, validated jobs (no files are written here).

pub mod ffargs;
pub mod naming;
pub mod streams;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::error::Error;
use crate::event::{JobId, Span};
use crate::exec::commit::temp_path;
use crate::probe::{self, MediaInfo};
use crate::request::{BatchRequest, ClipRequest, Mode, OnExists, Origin, SourceRequest};
use crate::time::{EndSpec, Ts};
use crate::tools::Tools;
use ffargs::ClipSpec;
use streams::Container;

/// How far before the target an approximate (coarse) input seek lands.
const COARSE_SEEK_MARGIN: Ts = Ts::from_micros(10_000_000);

/// Encoders available in the ffmpeg build (from `doctor::caps`).
#[derive(Debug, Clone, Default)]
pub struct Caps {
    pub encoders: BTreeSet<String>,
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
    pub code: &'static str,
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

/// The work a job performs. Features 002/003 add `Upscale` and `Gif` variants.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum JobSpec {
    Clip(ClipSpec),
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
        }
    }

    /// Media seconds this job produces (for overall progress weighting).
    pub fn media_len(&self) -> Ts {
        self.requested.end.saturating_sub(self.requested.start)
    }

    /// The job kind as it appears in JSON (`"clip"`).
    pub fn kind(&self) -> &'static str {
        match &self.spec {
            JobSpec::Clip(_) => "clip",
        }
    }

    /// Clip jobs: the cutting mode.
    pub fn mode(&self) -> Option<Mode> {
        match &self.spec {
            JobSpec::Clip(c) => Some(c.mode),
        }
    }

    /// Fast clip jobs: the span the copy is expected to produce.
    pub fn predicted(&self) -> Option<Span> {
        match &self.spec {
            JobSpec::Clip(c) => c.predicted,
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
        errors.peek().is_some() && errors.all(|i| i.code == "ENGINE_MISSING")
    }
}

/// Validates every request and resolves outputs. Reports all problems at once (FR-003).
pub async fn plan(req: &BatchRequest, tools: &Tools, caps: &Caps, cancel: &CancellationToken) -> Plan {
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
    };
    for s in &req.sources {
        if let Ok(c) = std::fs::canonicalize(&s.path) {
            planner.sources.insert(path_key(&c));
        }
    }
    for source in &req.sources {
        planner.plan_source(req, source).await;
    }
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
}

impl Planner<'_> {
    fn error(&mut self, origin: &Origin, code: &'static str, message: impl Into<String>) {
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
                "INVALID_INPUT",
                format!("{} is a playlist/stream description, not a video file", source.path.display()),
            );
        }
        let media = match self.probe(&source.path).await {
            Ok(m) => m,
            Err(msg) => {
                let code = if source.path.exists() { "PROBE_FAILED" } else { "SOURCE_MISSING" };
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

        // --- times ---------------------------------------------------------------------------
        let duration = media.duration;
        if let Some(d) = duration
            && clip.start >= d
        {
            return self.error(
                origin,
                "INVALID_INPUT",
                format!("start ({}) is at or beyond the end of the video ({})", clip.start, d),
            );
        }
        let mut end = match clip.end {
            EndSpec::At(t) => Some(t),
            EndSpec::Duration(d) if d == Ts::ZERO => {
                return self.error(origin, "INVALID_INPUT", "duration must be greater than zero");
            }
            EndSpec::Duration(d) => Some(clip.start + d),
            EndSpec::EndOfMedia => None,
        };
        if let Some(e) = end
            && e <= clip.start
        {
            return self.error(
                origin,
                "INVALID_INPUT",
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
                    "INVALID_INPUT",
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
        let (mut container, mut out_ext) = match Container::from_ext(&ext) {
            Some(c) => (c, ext.clone()),
            None if opts.container.is_some() => {
                return self.error(origin, "INVALID_INPUT", format!("unsupported output container .{ext}"));
            }
            None => {
                warnings.push(format!("unknown container .{ext}; writing .mkv"));
                (Container::Mkv, "mkv".to_owned())
            }
        };
        let mut encode = None;
        if opts.mode == Mode::Accurate {
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
                        "ENGINE_MISSING",
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

        let mut stream_plan = match streams::select_streams(media, &opts.streams, container, opts.mode) {
            Ok(p) => p,
            Err(msg) => return self.error(origin, "INVALID_INPUT", msg),
        };
        warnings.extend(stream_plan.warnings.iter().cloned());

        // --- output path ---------------------------------------------------------------------
        let source_dir = source.path.parent().map(Path::to_path_buf).unwrap_or_default();
        let out_dir = match &opts.output_dir {
            Some(d) if d.is_absolute() => d.clone(),
            Some(d) => std::path::absolute(d).unwrap_or_else(|_| d.clone()),
            None => source_dir.join("clips"),
        };
        let stem = source.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let rel = match naming::render(
            &opts.name_template,
            &naming::NameVars {
                source_stem: &stem,
                index,
                name: clip.name.as_deref(),
                start: clip.start,
                end: requested_end,
                ext: &out_ext,
            },
        ) {
            Ok(p) => p,
            Err(msg) => return self.error(origin, "INVALID_INPUT", msg),
        };
        let mut output = out_dir.join(rel);
        if self.is_source(&output) {
            return self.error(
                origin,
                "INVALID_INPUT",
                format!("output {} would overwrite a source video", output.display()),
            );
        }
        if self.claimed.contains(&path_key(&output)) {
            if opts.on_exists == OnExists::Rename {
                output = self.free_name(&output);
            } else {
                return self.error(
                    origin,
                    "INVALID_INPUT",
                    format!(
                        "two clips would write {}; use a name template with {{index}} or --on-exists rename",
                        output.display()
                    ),
                );
            }
        }
        let mut disposition = Disposition::Run;
        if output.exists() {
            match opts.on_exists {
                OnExists::Skip => disposition = Disposition::Skip { reason: "output already exists".into() },
                OnExists::Overwrite => warnings.push("existing output will be replaced".into()),
                OnExists::Rename => output = self.free_name(&output),
            }
        }
        self.claimed.insert(path_key(&output));

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
        } else if opts.mode == Mode::Accurate && source_precise {
            // Index-based demuxer seek + decode is frame exact.
            (Some(clip.start), None, clip.start, None)
        } else if opts.mode == Mode::Accurate {
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
                        "INVALID_INPUT",
                        format!("no keyframe found before {}; use --mode accurate for this clip", clip.start),
                    );
                }
                Err(Error::Cancelled) => return,
                Err(e) => return self.error(origin, e.code(), e.to_string()),
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
        self.jobs.push(PlannedJob {
            id,
            source: source.path.clone(),
            output,
            temp,
            on_exists: opts.on_exists,
            disposition,
            requested,
            warnings,
            spec: JobSpec::Clip(ClipSpec {
                input: source.path.clone(),
                input_seek,
                output_seek,
                duration: if trim.is_some() { None } else { end.map(|e| e - cut) },
                trim,
                verify_video: self.expected_video(media, opts.mode, clip.start, end, frame),
                mode: opts.mode,
                requested,
                predicted,
                container,
                encode,
                streams: stream_plan,
            }),
        });
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
