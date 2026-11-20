//! The plan's data (data-model.md): jobs and their commands, issues and notices, the smart-cut summaries, and the free space a plan needs.

use super::*;

/// What this machine can do: the engine's encoders and filters (from `doctor::caps`), the model
/// files found, and the cached GPU capability record (refactor A4).
#[derive(Debug, Clone, Default)]
pub struct Caps {
    pub encoders: BTreeSet<String>,
    pub filters: BTreeSet<String>,
    /// Output formats the engine can write (`ffmpeg -muxers`; GIF export needs `gif`/`mp4`).
    pub muxers: BTreeSet<String>,
    /// The optional GIF engines gifsicle and gifski (feature 003).
    pub gif_engines: crate::gif::engines::Engines,
    pub models: crate::models::ModelSet,
    pub capability: Option<crate::upscale::capability::CapabilityRecord>,
    /// The face detector smart reframe uses (`None`: no model, smart falls back to blur).
    pub face: Option<crate::upscale::reframe::detect::DetectorSpec>,
    /// Where reframe analyses are cached (`None`: no cache).
    pub reframe_cache: Option<PathBuf>,
    /// The AI tier's worker and models (`None`: not installed; the AI tier cannot run).
    pub ai: Option<crate::upscale::ai::AiEngine>,
    /// The video targets a fit can name (feature 005): the built-ins plus the user's files.
    pub video_targets: crate::fit::target::VideoTargetRegistry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Issue {
    pub severity: Severity,
    pub code: IssueCode,
    pub origin: Origin,
    pub message: String,
    /// Structured facts about the issue (`SMART_UNAVAILABLE`: `reason`, feature 004).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub detail: Option<crate::error::ErrorDetail>,
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

/// The work a job performs.
#[derive(Debug, Clone, PartialEq)]
pub enum JobSpec {
    Clip(Box<ClipSpec>),
    Upscale(Box<crate::upscale::UpscaleSpec>),
    /// A GIF or Reddit MP4 (feature 003).
    Gif(Box<crate::gif::GifSpec>),
}

/// What a job occupies while it runs. Every job takes one of the `--jobs` slots; GPU jobs also
/// take the single GPU slot, so two GPU jobs never share the device (research R9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Cpu,
    Gpu,
}

/// A warning or note with a code (feature 003). Every notice's message is also in
/// `PlannedJob.warnings`, so 001/002 consumers keep reading plain strings (research R16).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Notice {
    pub code: IssueCode,
    pub message: String,
    /// Structured facts about the notice (`SMART_FALLBACK`: `reason`, feature 004).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub detail: Option<crate::error::ErrorDetail>,
}

/// The program a [`JobCommand`] runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Program {
    Ffmpeg,
    /// The AI tier's worker (`rmedia-sr`).
    AiWorker,
    Gifsicle,
    Gifski,
}

/// One process of a job: what runs, with which arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobCommand {
    pub program: Program,
    pub args: Vec<OsString>,
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
    /// The warnings that have a code, with it (empty for clip and upscale jobs).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<Notice>>", optional))]
    pub notices: Vec<Notice>,
    /// What smart mode will do (feature 004): present when the clip asked for smart mode and it
    /// applies (not with an upscale or a GIF).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub smart: Option<SmartSummary>,
    /// The planned fit (feature 005): present when the job fits a video target.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub fit: Option<crate::fit::plan::FitSummary>,
    #[serde(skip)]
    pub spec: JobSpec,
}

/// What a smart job will do, for the dry run (data-model.md › Planned job additions).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SmartSummary {
    pub requested: bool,
    /// What will run: `smart`, or `accurate` on a fallback.
    pub mode: Mode,
    /// `--no-smart-fallback`: a reason is an error instead.
    pub strict: bool,
    pub head: Option<crate::smart::Part>,
    /// Absent on a fallback.
    pub copy: Option<crate::smart::Part>,
    pub tail: Option<crate::smart::Part>,
    pub copied_share: Option<f64>,
    /// A reason known before encoding (`None` also when the keyframes could not be read).
    pub fallback: Option<crate::smart::FallbackReason>,
}

impl SmartSummary {
    pub(super) fn of(cut: &ClipSpec, strict: bool) -> Self {
        let p = cut.smart.as_ref();
        SmartSummary {
            requested: true,
            mode: cut.mode,
            strict,
            head: p.and_then(|p| p.head),
            copy: p.map(|p| p.copy),
            tail: p.and_then(|p| p.tail),
            copied_share: p.map(|p| p.copied_share),
            fallback: cut.smart_fallback,
        }
    }
}

/// The plan's smart jobs, counted (present when any job asked for smart mode).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SmartTotals {
    /// Jobs that will be smart-cut.
    pub smart: usize,
    /// Jobs that will run in accurate mode instead, by reason.
    pub fallback: std::collections::BTreeMap<crate::smart::FallbackReason, usize>,
    /// Jobs that upscale or make a GIF, so smart does not apply.
    pub not_applied: usize,
    /// Jobs cut in accurate mode because their keyframes could not be read (no reason code).
    /// Absent when zero.
    #[serde(skip_serializing_if = "is_zero")]
    #[cfg_attr(feature = "ts", ts(as = "Option<usize>", optional))]
    pub unreadable: usize,
}

pub(super) fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl SmartTotals {
    pub fn of(jobs: &[PlannedJob]) -> Option<Self> {
        let mut t = SmartTotals::default();
        let mut any = false;
        for j in jobs {
            if j.notices.iter().any(|n| n.code == IssueCode::SmartNotApplied) {
                any = true;
                t.not_applied += 1;
            }
            let Some(s) = &j.smart else { continue };
            any = true;
            match (s.mode, s.fallback) {
                (Mode::Smart, _) => t.smart += 1,
                (_, Some(r)) => *t.fallback.entry(r).or_default() += 1,
                (_, None) => t.unreadable += 1,
            }
        }
        any.then_some(t)
    }
}

impl PlannedJob {
    /// The exact ffmpeg arguments this job will run (for a gifski job, the ffmpeg half of its
    /// pipe).
    pub fn args(&self) -> Vec<OsString> {
        match &self.spec {
            JobSpec::Clip(c) => match (&c.smart, &self.scratch) {
                (Some(p), Some(dir)) => {
                    let rest = crate::smart::args::rest_spec(c).map(|_| dir.join(crate::exec::SMART_REST));
                    crate::smart::args::join_args(c, p, rest.as_deref(), &self.temp)
                }
                _ => ffargs::clip_args(c, &self.temp),
            },
            JobSpec::Upscale(u) => ffargs::upscale_args(u, &self.temp),
            JobSpec::Gif(g) if g.gif.engine == crate::gif::plan::EngineUsed::Gifski => {
                crate::gif::graph::y4m_args(g, &g.gif.start, crate::gif::graph::PROGRESS)
            }
            JobSpec::Gif(g) if g.gif.format == crate::gif::Format::Mp4 => {
                crate::gif::graph::mp4_args(g, &g.gif.start, crate::gif::graph::ENCODED_MP4)
            }
            JobSpec::Gif(g) => {
                let s = &g.gif.start;
                crate::gif::graph::encode_args(
                    g,
                    s,
                    &crate::gif::graph::palette_name(g, s),
                    crate::gif::graph::ENCODED,
                )
            }
        }
    }

    /// The AI tier's three argvs (`None` for other jobs). `args()` is the last of them.
    pub fn pipeline(&self) -> Option<ffargs::AiArgs> {
        match &self.spec {
            JobSpec::Upscale(u) if u.up.ai.is_some() => Some(ffargs::ai_args(u, &self.temp)),
            _ => None,
        }
    }

    /// Every process this job runs, in order (a pipe's stages in pipe order). For clip and
    /// upscale jobs the last one writes the output, and its arguments are [`PlannedJob::args`].
    pub fn commands(&self) -> Vec<JobCommand> {
        let ffmpeg = |args| JobCommand { program: Program::Ffmpeg, args };
        // A smart clip: head, copy, tail, the other streams, then the join (exec/smart.rs).
        if let (JobSpec::Clip(c), Some(dir)) = (&self.spec, &self.scratch)
            && let Some(p) = &c.smart
        {
            use crate::smart::args::{COPY, HEAD, TAIL, copy_args, part_args, rest_spec};
            let mut cmds: Vec<JobCommand> =
                p.head.map(|h| ffmpeg(part_args(c, p, h, &dir.join(HEAD)))).into_iter().collect();
            cmds.push(ffmpeg(copy_args(c, p, &dir.join(COPY))));
            cmds.extend(p.tail.map(|t| ffmpeg(part_args(c, p, t, &dir.join(TAIL)))));
            cmds.extend(
                rest_spec(c).map(|r| ffmpeg(ffargs::clip_args(&r, &dir.join(crate::exec::SMART_REST)))),
            );
            cmds.push(ffmpeg(self.args()));
            return cmds;
        }
        if let JobSpec::Gif(g) = &self.spec {
            use crate::gif::graph::{
                ENCODED, OPTIMISED, gifsicle_args, gifski_args, palette_args, palette_name,
            };
            let s = &g.gif.start;
            if let (crate::gif::plan::EngineUsed::Gifski, Some(p)) = (g.gif.engine, s.gifski) {
                return vec![
                    ffmpeg(self.args()),
                    JobCommand {
                        program: Program::Gifski,
                        args: gifski_args(s.rate, p, g.gif.loop_count, ENCODED),
                    },
                ];
            }
            if g.gif.format == crate::gif::Format::Mp4 {
                return vec![ffmpeg(self.args())];
            }
            let palette = palette_name(g, s);
            let mut cmds = vec![ffmpeg(palette_args(g, s, &palette)), ffmpeg(self.args())];
            if g.gif.optimiser.is_some() {
                cmds.push(JobCommand {
                    program: Program::Gifsicle,
                    args: gifsicle_args(s.lossy, ENCODED, OPTIMISED),
                });
            }
            return cmds;
        }
        match self.pipeline() {
            Some(p) => vec![
                ffmpeg(p.decode),
                JobCommand { program: Program::AiWorker, args: p.worker },
                ffmpeg(p.encode),
            ],
            None => vec![ffmpeg(self.args())],
        }
    }

    /// Media seconds this job produces (for overall progress weighting).
    pub fn media_len(&self) -> Ts {
        self.requested.end.saturating_sub(self.requested.start)
    }

    /// The scarce resource this job needs besides a job slot.
    pub fn resource(&self) -> Resource {
        match &self.spec {
            JobSpec::Clip(_) | JobSpec::Gif(_) => Resource::Cpu,
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
            JobSpec::Gif(_) => "gif",
        }
    }

    /// Clip jobs: the cutting mode.
    pub fn mode(&self) -> Option<Mode> {
        match &self.spec {
            JobSpec::Clip(c) => Some(c.mode),
            JobSpec::Upscale(_) | JobSpec::Gif(_) => None,
        }
    }

    /// Fast clip jobs: the span the copy is expected to produce.
    pub fn predicted(&self) -> Option<Span> {
        match &self.spec {
            JobSpec::Clip(c) => c.predicted,
            JobSpec::Upscale(_) | JobSpec::Gif(_) => None,
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
    /// The smart jobs, counted (feature 004).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub smart: Option<SmartTotals>,
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

/// The bytes each output folder needs for the upscale and GIF jobs that would run (clips are
/// not estimated).
pub fn space_needs(jobs: &[PlannedJob]) -> std::collections::BTreeMap<PathBuf, u64> {
    let mut needs = std::collections::BTreeMap::new();
    for job in jobs {
        let bytes = match &job.spec {
            JobSpec::Upscale(u) => u.up.estimate.bytes,
            JobSpec::Gif(g) => crate::gif::plan::rough_bytes(&g.gif),
            // A smart clip holds its parts and the joined file at once (item 27). Other clips are
            // left out: a re-encode's size is not predictable from the source.
            JobSpec::Clip(c) => match (&c.fit, &c.smart) {
                // A fit's size is known: at most its prediction (feature 005). A retry replaces
                // the temp file, so one copy is needed at a time.
                (Some(f), _) => f.predicted_bytes,
                (None, Some(s)) if s.bytes > 0 => s.bytes.saturating_mul(2),
                _ => continue,
            },
        };
        if let Some(dir) = job.output.parent()
            && job.disposition == Disposition::Run
        {
            *needs.entry(dir.to_path_buf()).or_default() += bytes;
        }
    }
    needs
}
