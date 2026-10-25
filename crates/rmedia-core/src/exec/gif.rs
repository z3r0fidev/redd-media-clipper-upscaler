//! GIF job executor (data-model.md › Stages and weights): palette → encode → optimise →
//! verify for the built-in engine, encode (the ffmpeg → gifski pipe) → verify for gifski. Every
//! tool runs in the job's scratch folder with generated, relative file names; the result is
//! moved to the job's temp file, verified, then committed by the scheduler.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::JobCtx;
use super::clip::classify_failure;
use super::pipeline::{ProgressFrom, Report, Role, StageSpec, blame_pair, run_stages};
use super::process;
use super::progress::{ProgressBlock, ProgressParser};
use crate::error::{Error, IssueCode, Result};
use crate::event::{JobMeasure, Span};
use crate::gif::Format;
use crate::gif::LoopCount;
use crate::gif::graph::{
    ENCODED, ENCODED_MP4, OPTIMISED, PROGRESS, encode_args, gifsicle_args, gifski_args, mp4_args,
    palette_args, palette_name, strip_loop_block, y4m_args,
};
use crate::gif::plan::{EngineUsed, GifSpec, Settings};
use crate::gif::verify::{GifExpect, Mp4Expect, check_gif, check_mp4, gif_facts, mp4_faststart};
use crate::gif::{budget, estimate};
use crate::time::Ts;
use crate::tools::Tools;
use crate::upscale::estimate::human_bytes;

/// Without a budget, an encode that passes this size is stopped (a runaway output).
pub(crate) const MAX_OUTPUT: u64 = 4 << 30;
/// The gifski pipe is stopped when ffmpeg reports no progress for this long. gifski holds only
/// a few frames, so it finishes soon after ffmpeg does.
const PIPE_STALL: Duration = Duration::from_secs(300);
/// The gifsicle post-pass is stopped after this long (it takes seconds on a Reddit-sized GIF).
const GIFSICLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

pub(super) async fn execute(spec: &GifSpec, ctx: &mut JobCtx<'_>) -> Result<JobMeasure> {
    let scratch = ctx
        .scratch_dir()
        .ok_or_else(|| Error::InvalidInput("a GIF job needs its scratch folder".into()))?
        .to_path_buf();
    let mut spec = spec.clone();
    let analyse = spec.frame.smart.as_ref().is_some_and(|sp| sp.analysis.is_none());
    let uses_gifski = spec.gif.gifski.is_some() && spec.gif.engine == EngineUsed::Gifski;
    let mp4 = spec.gif.format == Format::Mp4;
    let optimise = spec.gif.optimiser.is_some() && !uses_gifski && !mp4;
    let budget = spec.gif.budget;
    let limit = budget.map_or(MAX_OUTPUT, |b| b.hard.saturating_mul(2));
    // The MP4 and the gifski pipe are one encode stage each.
    let stages = Stages::new(uses_gifski || mp4, optimise, budget.is_some(), analyse);
    if analyse {
        stages.enter(ctx, "analyze");
        let total = spec.cut.requested.end.saturating_sub(spec.cut.requested.start);
        let (tools, cancel) = (ctx.tools, ctx.cancel);
        match estimate::analyze(&mut spec, tools, cancel, |f| ctx.stage_fraction(f, total)).await {
            Ok(Some(note)) => ctx.warn(note.into()),
            Ok(None) => {}
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => ctx.warn(format!("smart framing analysis failed ({e}); using the blurred background")),
        }
    }
    if let Some(a) = spec.frame.smart.as_ref().and_then(|sp| sp.analysis.as_ref().map(|a| (a, sp.cmd_offset)))
    {
        let path = scratch.join(estimate::REFRAME_CMD);
        std::fs::write(&path, estimate::job_cmds(a.0, a.1))
            .map_err(|e| Error::OutputUnwritable { path, message: e.to_string() })?;
    }
    let spec = &spec;
    let gifski = spec.gif.gifski.as_deref().filter(|_| uses_gifski);
    let mut enc = Encoder { scratch: &scratch, gifski, mp4, optimise, limit, stages: &stages, fulls: 0 };

    let (s, len, result, search, over) = match budget {
        Some(b) => {
            stages.enter(ctx, "search");
            let mut sampler =
                JobSampler { ctx: &mut *ctx, spec, enc: &mut enc, results: Vec::new(), samples: 0 };
            let found = budget::search(&estimate::search_input(spec, b), &mut sampler).await?;
            let result = *sampler
                .results
                .get(found.attempt)
                .ok_or_else(|| Error::InvalidInput("the search encoded nothing".into()))?;
            debug_assert!(found.record.encodes <= 10, "{:?}", found.record);
            let len = found.trimmed_to.unwrap_or(spec.gif.len);
            (found.chosen, len, result, Some(found.record), found.over_recommended.then_some(found.bytes))
        }
        None => {
            let result = enc.full(ctx, spec, &spec.gif.start).await?;
            (spec.gif.start.clone(), spec.gif.len, result, None, None)
        }
    };
    // "The MP4 would be smaller" (US3): one MP4 sample at the MP4's own settings over the same
    // range, unless the dry run already made it.
    let mut mp4_estimate = spec.gif.estimate.mp4_bytes;
    if !mp4 && spec.gif.target.mp4.hint && mp4_estimate.is_none() {
        let range = if len < spec.gif.len - 1e-6 { spec.trimmed(len) } else { spec.clone() };
        match estimate::mp4_hint(ctx.tools, &range, &scratch, ctx.cancel).await {
            Ok(b) => mp4_estimate = Some(b),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(_) => {} // optional
        }
    }
    move_into(&scratch.join(result), ctx.temp)?;

    stages.enter(ctx, "verify");
    let bytes = if mp4 {
        let info = crate::probe::probe(ctx.tools, ctx.temp, ctx.cancel)
            .await
            .map_err(|e| Error::VerifyFailed(e.to_string()))?;
        let faststart = mp4_faststart(ctx.temp).map_err(|e| Error::VerifyFailed(e.to_string()))?;
        let expect = Mp4Expect {
            width: s.width,
            height: s.height,
            duration: Ts::from_secs_f64(len),
            frame: Ts::from_secs_f64(1.0 / s.fps()),
            max_bytes: budget.map(|b| b.hard),
        };
        check_mp4(&info, faststart, &expect).map_err(Error::VerifyFailed)?;
        info.size
    } else {
        let facts = gif_facts(ctx.tools, ctx.temp, ctx.cancel).await?;
        let frames = crate::upscale::reframe::frame_count(len, s.fps()) as u64;
        let expect = GifExpect {
            width: s.width,
            height: s.height,
            frames,
            // gifsicle merges identical frames, and gifski rounds its own delays: both are
            // checked by their total display time.
            length_cs: (result.starts_with("opt") || gifski.is_some()).then(|| {
                crate::gif::timing::delays(s.rate, frames as usize).into_iter().map(u64::from).sum()
            }),
            loop_count: spec.gif.loop_count,
            max_bytes: budget.map(|b| b.hard),
        };
        check_gif(&facts, &expect).map_err(Error::VerifyFailed)?;
        facts.bytes
    };
    if let (Some(bytes), Some(b)) = (over, budget) {
        ctx.warn_code(
            IssueCode::OverRecommended,
            format!(
                "{} is over the recommended {} (within the {} limit)",
                human_bytes(bytes),
                human_bytes(b.soft),
                human_bytes(b.hard)
            ),
        );
    }
    let total = spec.cut.requested.end.saturating_sub(spec.cut.requested.start);
    ctx.stage_fraction(1.0, total);
    let start = spec.cut.requested.start;
    let trimmed = (len < spec.gif.len - 1e-6).then(|| Span { start, end: start + Ts::from_secs_f64(len) });
    Ok(measure(spec, &s, bytes, optimise, search, trimmed, mp4_estimate))
}

/// The stages of a GIF job and their weights (data-model.md › Stages and weights), renormalised
/// when some are absent.
struct Stages {
    list: Vec<(&'static str, f64)>,
}

impl Stages {
    fn new(gifski: bool, optimise: bool, search: bool, analyze: bool) -> Stages {
        let mut list: Vec<(&'static str, f64)> = match (gifski, search, optimise) {
            (true, false, _) => vec![("encode", 0.9), ("verify", 0.1)],
            (true, true, _) => vec![("search", 0.3), ("encode", 0.6), ("verify", 0.1)],
            (false, false, true) => {
                vec![("palette", 0.25), ("encode", 0.6), ("optimise", 0.1), ("verify", 0.05)]
            }
            (false, false, false) => vec![("palette", 0.25), ("encode", 0.65), ("verify", 0.1)],
            (false, true, true) => {
                vec![
                    ("search", 0.3),
                    ("palette", 0.1),
                    ("encode", 0.35),
                    ("optimise", 0.05),
                    ("verify", 0.05),
                ]
            }
            (false, true, false) => {
                vec![("search", 0.3), ("palette", 0.1), ("encode", 0.35), ("verify", 0.05)]
            }
        };
        if analyze {
            list.insert(0, ("analyze", 0.15));
        }
        let sum: f64 = list.iter().map(|s| s.1).sum();
        Stages { list: list.into_iter().map(|(n, w)| (n, w / sum)).collect() }
    }

    fn position(&self, name: &str) -> Option<(u32, f64)> {
        self.list.iter().position(|s| s.0 == name).map(|i| (i as u32 + 1, self.list[i].1))
    }

    fn enter(&self, ctx: &mut JobCtx<'_>, name: &str) {
        if let Some((index, weight)) = self.position(name) {
            ctx.stage(name, index, self.list.len() as u32, weight);
        }
    }

    /// A retried full encode: the bar continues inside the current stage.
    fn restart(&self, ctx: &mut JobCtx<'_>, name: &str) {
        if let Some((index, _)) = self.position(name) {
            ctx.restart_stage(name, index, self.list.len() as u32);
        }
    }
}

/// Full encodes of a GIF job in its scratch folder.
struct Encoder<'a> {
    scratch: &'a Path,
    gifski: Option<&'a Path>,
    /// The Reddit MP4: one x264 encode.
    mp4: bool,
    optimise: bool,
    limit: u64,
    stages: &'a Stages,
    fulls: u32,
}

impl Encoder<'_> {
    /// One full encode of `spec` at `s`: palette → encode → optimise, or the gifski pipe. Returns
    /// the result's name in the scratch folder. A second call (the search's retry) restarts the
    /// encode stage instead of moving on.
    async fn full(&mut self, ctx: &mut JobCtx<'_>, spec: &GifSpec, s: &Settings) -> Result<&'static str> {
        let retry = self.fulls > 0;
        // Each attempt has its own files, so a retry never overwrites the first result (the
        // search may keep it); every tool runs with `-n` or its own no-clobber rule.
        debug_assert!(self.fulls < 2, "the search encodes in full at most twice (R5)");
        let (encoded, optimised, encoded_mp4) = attempt_names(self.fulls);
        self.fulls += 1;
        for f in [encoded, optimised, encoded_mp4] {
            let _ = std::fs::remove_file(self.scratch.join(f));
        }
        let span = spec.cut.requested;
        let total = span.end.saturating_sub(span.start);
        let enter = |ctx: &mut JobCtx<'_>, name: &str| {
            if !retry {
                self.stages.enter(ctx, name);
            }
        };
        if retry {
            self.stages.restart(ctx, "encode");
        }
        if self.mp4 {
            enter(ctx, "encode");
            let args = mp4_args(spec, s, encoded_mp4);
            encode_watched(ctx, self.scratch, &args, total, self.limit, encoded_mp4).await?;
            return Ok(encoded_mp4);
        }
        if let Some(gifski) = self.gifski {
            enter(ctx, "encode");
            let (tools, cancel, output) = (ctx.tools, ctx.cancel.clone(), ctx.output.to_path_buf());
            let pipe = Gifski {
                program: gifski,
                dir: self.scratch,
                output: encoded,
                limit: self.limit,
                timeout: None,
            };
            let producer = y4m_args(spec, s, PROGRESS);
            pipe.run(tools, producer, s, spec.gif.loop_count, Some(PROGRESS), &output, &cancel, |b| {
                ctx.progress(b, total)
            })
            .await?;
        } else {
            enter(ctx, "palette");
            let palette = palette_name(spec, s);
            if !self.scratch.join(&palette).is_file() {
                let out =
                    ctx.run_ffmpeg_in(Some(self.scratch), &palette_args(spec, s, &palette), total).await?;
                if !out.status.success() {
                    return Err(classify_failure(&out.stderr_tail, ctx.output));
                }
            }
            enter(ctx, "encode");
            let args = encode_args(spec, s, &palette, encoded);
            encode_watched(ctx, self.scratch, &args, total, self.limit, encoded).await?;
        }
        if let (true, Some(gifsicle)) = (self.optimise, spec.gif.optimiser.as_deref()) {
            enter(ctx, "optimise");
            run_gifsicle(ctx, gifsicle, self.scratch, s.lossy, self.limit, (encoded, optimised)).await?;
            return Ok(optimised);
        }
        Ok(encoded)
    }
}

/// The scratch file names of full encode `n` (0: the first, 1: the search's retry): encoded,
/// optimised, MP4.
fn attempt_names(n: u32) -> (&'static str, &'static str, &'static str) {
    if n == 0 { (ENCODED, OPTIMISED, ENCODED_MP4) } else { ("enc2.gif", "opt2.gif", "enc2.mp4") }
}

/// The job's [`budget::Sampler`]: samples and post-passes in the scratch folder (their own file
/// names), full encodes through [`Encoder`] (on the trimmed range when the search trims).
struct JobSampler<'a, 'c, 'e> {
    ctx: &'a mut JobCtx<'c>,
    spec: &'a GifSpec,
    enc: &'a mut Encoder<'e>,
    /// Each full encode's result name, by attempt.
    results: Vec<&'static str>,
    samples: u32,
}

impl budget::Sampler for JobSampler<'_, '_, '_> {
    async fn sample(&mut self, s: &Settings) -> Result<budget::Estimate> {
        let e =
            estimate::sample_estimate(self.ctx.tools, self.spec, s, self.enc.scratch, self.ctx.cancel).await;
        // The search stage moves by samples (at most 7, R5).
        self.samples += 1;
        let total = self.spec.cut.requested.end.saturating_sub(self.spec.cut.requested.start);
        self.ctx.stage_fraction(f64::from(self.samples) / 7.0, total);
        e
    }

    async fn post(&mut self, s: &Settings) -> Result<budget::Estimate> {
        estimate::post_estimate(self.ctx.tools, self.spec, s, self.enc.scratch, self.ctx.cancel).await
    }

    async fn full(&mut self, s: &Settings, len: f64) -> Result<u64> {
        let spec = if len < self.spec.gif.len - 1e-6 { self.spec.trimmed(len) } else { self.spec.clone() };
        let name = self.enc.full(self.ctx, &spec, s).await?;
        self.results.push(name);
        std::fs::metadata(self.enc.scratch.join(name))
            .map(|m| m.len())
            .map_err(|e| Error::engine_failed(format!("the GIF was not written: {e}"), String::new()))
    }
}

fn measure(
    spec: &GifSpec,
    s: &Settings,
    bytes: u64,
    optimised: bool,
    search: Option<budget::SearchRecord>,
    trimmed: Option<Span>,
    mp4_estimate: Option<u64>,
) -> JobMeasure {
    let builtin = spec.gif.engine == EngineUsed::Builtin;
    JobMeasure::Gif {
        span: spec.cut.requested,
        format: spec.gif.format,
        bytes,
        width: s.width,
        height: s.height,
        fps: s.fps(),
        timing: s.timing(),
        colours: builtin.then_some(s.colours),
        dither: builtin.then_some(s.dither),
        lossy: (builtin && optimised).then_some(s.lossy),
        engine: spec.gif.engine,
        optimiser: optimised,
        loop_count: spec.gif.loop_count,
        deterministic: spec.gif.deterministic,
        trimmed,
        search,
        mp4_estimate,
    }
}

/// Runs the encode in `scratch`, reporting progress, and stops it when its `output` passes
/// `limit` bytes (research R7's size watchdog).
async fn encode_watched(
    ctx: &mut JobCtx<'_>,
    scratch: &Path,
    args: &[std::ffi::OsString],
    total: Ts,
    limit: u64,
    output: &str,
) -> Result<()> {
    let stop = ctx.cancel.child_token();
    let watch = SizeWatch::start(scratch.join(output), limit, &stop);
    let mut parser = ProgressParser::default();
    let ffmpeg = ctx.tools.ffmpeg.path.clone();
    let r = process::run_lines_in(&ffmpeg, args, Some(scratch), &stop, |line| {
        if let Some(block) = parser.feed(line) {
            ctx.progress(block, total);
        }
    })
    .await;
    watch.stop()?;
    let out = r?;
    if out.status.success() { Ok(()) } else { Err(classify_failure(&out.stderr_tail, ctx.output)) }
}

/// Research R7's size watchdog: cancels `stop` once `file` passes `limit` bytes. Dropping it
/// stops the watching task.
pub(crate) struct SizeWatch {
    task: tokio::task::JoinHandle<()>,
    over: Arc<AtomicBool>,
    limit: u64,
}

impl SizeWatch {
    pub(crate) fn start(file: PathBuf, limit: u64, stop: &CancellationToken) -> SizeWatch {
        let over = Arc::new(AtomicBool::new(false));
        let (flag, stop) = (over.clone(), stop.clone());
        let task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if std::fs::metadata(&file).is_ok_and(|m| m.len() > limit) {
                    flag.store(true, Ordering::SeqCst);
                    stop.cancel();
                    return;
                }
            }
        });
        SizeWatch { task, over, limit }
    }

    /// Stops watching; an error when the file grew past the limit (the run was cancelled for it).
    pub(crate) fn stop(self) -> Result<()> {
        self.task.abort();
        if self.over.load(Ordering::SeqCst) {
            return Err(Error::engine_failed(
                format!("the GIF grew past {} bytes and was stopped", self.limit),
                String::new(),
            ));
        }
        Ok(())
    }
}

impl Drop for SizeWatch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Cancels `stop` after `after`; `fired()` tells whether it did.
pub(crate) struct Deadline {
    task: tokio::task::JoinHandle<()>,
    fired: Arc<AtomicBool>,
}

impl Deadline {
    pub(crate) fn start(after: Duration, stop: &CancellationToken) -> Deadline {
        let fired = Arc::new(AtomicBool::new(false));
        let (flag, stop) = (fired.clone(), stop.clone());
        let task = tokio::spawn(async move {
            tokio::time::sleep(after).await;
            flag.store(true, Ordering::SeqCst);
            stop.cancel();
        });
        Deadline { task, fired }
    }

    pub(crate) fn fired(&self) -> bool {
        self.fired.load(Ordering::SeqCst)
    }
}

impl Drop for Deadline {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The ffmpeg → gifski pipe (research R7), writing `output` in `dir`.
pub(crate) struct Gifski<'a> {
    pub program: &'a Path,
    pub dir: &'a Path,
    pub output: &'a str,
    /// The size watchdog's limit.
    pub limit: u64,
    /// A wall-clock limit, for runs without a progress file (no stall can be seen there).
    pub timeout: Option<Duration>,
}

impl Gifski<'_> {
    /// Runs `producer` (an ffmpeg argv writing y4m to stdout, and its progress to `progress`
    /// when set) into gifski with the settings of `s`. A play-once GIF loses the loop block
    /// gifski writes anyway. `job_output` names the output in disk errors.
    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        &self,
        tools: &Tools,
        producer: Vec<OsString>,
        s: &Settings,
        loop_count: LoopCount,
        progress: Option<&str>,
        job_output: &Path,
        cancel: &CancellationToken,
        on_progress: impl FnMut(ProgressBlock),
    ) -> Result<()> {
        let params =
            s.gifski.ok_or_else(|| Error::InvalidInput("a gifski encode needs its parameters".into()))?;
        let file = self.dir.join(self.output);
        let _ = std::fs::remove_file(&file);
        let from = match progress {
            Some(p) => {
                let path = self.dir.join(p);
                let _ = std::fs::remove_file(&path);
                ProgressFrom::File(path)
            }
            None => ProgressFrom::ConsumerStdout,
        };
        let stages = [
            StageSpec {
                program: tools.ffmpeg.path.clone(),
                args: producer,
                role: Role::Producer,
                report: Report::Tail,
                env: None,
            },
            StageSpec {
                program: self.program.to_path_buf(),
                args: gifski_args(s.rate, params, loop_count, self.output),
                role: Role::Consumer,
                report: Report::CleanTail,
                env: Some(crate::gif::engines::engine_env()),
            },
        ];
        let stop = cancel.child_token();
        let watch = SizeWatch::start(file.clone(), self.limit, &stop);
        // Without a progress file nothing reports progress, so nothing can show a stall.
        let stall = progress.map(|_| PIPE_STALL);
        let deadline = self.timeout.map(|t| Deadline::start(t, &stop));
        let r = run_stages(&stages, Some(self.dir), &from, stall, &stop, on_progress).await;
        watch.stop()?;
        if let (Some(d), Some(t)) = (&deadline, self.timeout)
            && d.fired()
            && !cancel.is_cancelled()
        {
            return Err(Error::engine_failed(
                format!("the gifski pipe took longer than {} s and was stopped", t.as_secs()),
                String::new(),
            ));
        }
        let ends = r?;
        match blame_pair(&ends[0], &ends[1]) {
            Some(0) => return Err(classify_failure(&ends[0].tail, job_output)),
            Some(_) if !ends[1].failed() => {
                return Err(Error::EngineFailed {
                    tool: "gifski".into(),
                    message: "gifski stopped reading frames before the end".into(),
                    stderr_tail: ends[1].tail.clone(),
                });
            }
            Some(_) => return Err(tool_failed("gifski", ends[1].status, &ends[1].tail)),
            None => {}
        }
        if loop_count == LoopCount::Plays(1) {
            let unwritable = |e: std::io::Error| Error::OutputUnwritable {
                path: job_output.to_path_buf(),
                message: e.to_string(),
            };
            let path = file.clone();
            tokio::task::spawn_blocking(move || strip_loop_block(&path))
                .await
                .map_err(|e| unwritable(std::io::Error::other(e)))?
                .map_err(unwritable)?;
        }
        Ok(())
    }
}

/// A failed optional engine, named by `tool`, with its last stderr line (control characters
/// removed: these programs are less trusted than ffmpeg).
fn tool_failed(tool: &str, status: Option<ExitStatus>, stderr_tail: &str) -> Error {
    let tail: String =
        stderr_tail.chars().filter(|c| !crate::gif::target::unsafe_char(*c) || *c == '\n').collect();
    let last = tail.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no message").trim().to_owned();
    Error::EngineFailed {
        tool: tool.into(),
        message: format!("{last} ({})", super::pipeline::describe(status)),
        stderr_tail: tail,
    }
}

/// The gifsicle post-pass (`-O2`, lossy when set) with a cleared environment, the size watchdog
/// on its output and a wall-clock limit.
async fn run_gifsicle(
    ctx: &mut JobCtx<'_>,
    gifsicle: &Path,
    scratch: &Path,
    lossy: u16,
    limit: u64,
    (input, output): (&str, &str),
) -> Result<()> {
    let env = crate::gif::engines::engine_env();
    let args = gifsicle_args(lossy, input, output);
    let stop = ctx.cancel.child_token();
    let watch = SizeWatch::start(scratch.join(output), limit, &stop);
    let deadline = Deadline::start(GIFSICLE_TIMEOUT, &stop);
    let r = process::run_lines_env(gifsicle, &args, Some(scratch), Some(&env), &stop, |_| {}).await;
    watch.stop()?;
    if deadline.fired() && !ctx.cancel.is_cancelled() {
        return Err(Error::EngineFailed {
            tool: "gifsicle".into(),
            message: format!(
                "gifsicle took longer than {} min and was stopped",
                GIFSICLE_TIMEOUT.as_secs() / 60
            ),
            stderr_tail: String::new(),
        });
    }
    let out = r?;
    if out.status.success() {
        Ok(())
    } else {
        Err(tool_failed("gifsicle", Some(out.status), &out.stderr_tail))
    }
}

/// Moves the finished GIF from the scratch folder to the temp file (a copy when they are on
/// different file systems). The copy goes into a newly created file, never through whatever
/// already sits at `temp` (a planted link).
fn move_into(from: &Path, temp: &Path) -> Result<()> {
    let unwritable =
        |e: std::io::Error| Error::OutputUnwritable { path: temp.to_path_buf(), message: e.to_string() };
    if std::fs::rename(from, temp).is_err() {
        match std::fs::remove_file(temp) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(unwritable(e)),
            _ => {}
        }
        let mut out =
            std::fs::OpenOptions::new().write(true).create_new(true).open(temp).map_err(unwritable)?;
        let mut src = std::fs::File::open(from).map_err(unwritable)?;
        std::io::copy(&mut src, &mut out).map_err(unwritable)?;
        let _ = std::fs::remove_file(from);
    }
    Ok(())
}
