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
use crate::error::{Error, Result};
use crate::event::JobMeasure;
use crate::gif::LoopCount;
use crate::gif::graph::{
    ENCODED, OPTIMISED, PROGRESS, encode_args, gifsicle_args, gifski_args, palette_args, palette_name,
    strip_loop_block, y4m_args,
};
use crate::gif::plan::{EngineUsed, GifSpec, Settings};
use crate::gif::verify::{GifExpect, check_gif, gif_facts};
use crate::time::Ts;
use crate::tools::Tools;

/// Without a budget, an encode that passes this size is stopped (a runaway output).
pub(crate) const MAX_OUTPUT: u64 = 4 << 30;
/// The gifski pipe is stopped when ffmpeg reports no progress for this long. gifski holds only
/// a few frames, so it finishes soon after ffmpeg does.
const PIPE_STALL: Duration = Duration::from_secs(300);
/// The gifsicle post-pass is stopped after this long (it takes seconds on a Reddit-sized GIF).
const GIFSICLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

pub(super) async fn execute(spec: &GifSpec, ctx: &mut JobCtx<'_>) -> Result<JobMeasure> {
    let span = spec.cut.requested;
    let total = span.end.saturating_sub(span.start);
    let scratch = ctx
        .scratch_dir()
        .ok_or_else(|| Error::InvalidInput("a GIF job needs its scratch folder".into()))?
        .to_path_buf();
    let s = spec.gif.start.clone();
    let optimise = spec.gif.optimiser.is_some();
    let gifski = spec.gif.gifski.as_deref().filter(|_| spec.gif.engine == EngineUsed::Gifski);
    let stages: Vec<(&str, f64)> = if gifski.is_some() {
        vec![("encode", 0.9), ("verify", 0.1)]
    } else if optimise {
        vec![("palette", 0.25), ("encode", 0.6), ("optimise", 0.1), ("verify", 0.05)]
    } else {
        vec![("palette", 0.25), ("encode", 0.65), ("verify", 0.1)]
    };
    let of = stages.len() as u32;
    let mut index = 0u32;
    let mut next = |ctx: &mut JobCtx<'_>| {
        let (name, weight) = stages[index as usize];
        index += 1;
        ctx.stage(name, index, of, weight);
    };

    let limit = spec.gif.budget.map_or(MAX_OUTPUT, |b| b.hard.saturating_mul(2));
    if let Some(gifski) = gifski {
        next(ctx);
        let (tools, cancel, output) = (ctx.tools, ctx.cancel.clone(), ctx.output.to_path_buf());
        let pipe = Gifski { program: gifski, dir: &scratch, output: ENCODED, limit, timeout: None };
        let producer = y4m_args(spec, &s, PROGRESS);
        pipe.run(tools, producer, &s, spec.gif.loop_count, Some(PROGRESS), &output, &cancel, |b| {
            ctx.progress(b, total)
        })
        .await?;
    } else {
        next(ctx);
        let palette = palette_name(spec, &s);
        if !scratch.join(&palette).is_file() {
            let out = ctx.run_ffmpeg_in(Some(&scratch), &palette_args(spec, &s, &palette), total).await?;
            if !out.status.success() {
                return Err(classify_failure(&out.stderr_tail, ctx.output));
            }
        }

        next(ctx);
        encode_watched(ctx, &scratch, &encode_args(spec, &s, &palette, ENCODED), total, limit).await?;
    }

    let mut result = ENCODED;
    if let (true, Some(gifsicle)) = (optimise, spec.gif.optimiser.as_deref()) {
        next(ctx);
        run_gifsicle(ctx, gifsicle, &scratch, s.lossy, limit).await?;
        result = OPTIMISED;
    }
    move_into(&scratch.join(result), ctx.temp)?;

    next(ctx);
    let facts = gif_facts(ctx.tools, ctx.temp, ctx.cancel).await?;
    let expect = GifExpect {
        width: s.width,
        height: s.height,
        frames: spec.gif.frames,
        // gifsicle merges identical frames, and gifski rounds its own delays: both are checked
        // by their total display time.
        length_cs: (result == OPTIMISED || gifski.is_some()).then(|| {
            crate::gif::timing::delays(s.rate, spec.gif.frames as usize).into_iter().map(u64::from).sum()
        }),
        loop_count: spec.gif.loop_count,
        max_bytes: spec.gif.budget.map(|b| b.hard),
    };
    // Over the hard limit fails here until US2's budget search lowers the settings.
    check_gif(&facts, &expect).map_err(Error::VerifyFailed)?;
    ctx.stage_fraction(1.0, total);
    Ok(measure(spec, &s, facts.bytes, optimise))
}

fn measure(spec: &GifSpec, s: &Settings, bytes: u64, optimised: bool) -> JobMeasure {
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
        trimmed: None,
        search: None,
        mp4_estimate: spec.gif.estimate.mp4_bytes,
    }
}

/// Runs the encode in `scratch`, reporting progress, and stops it when `enc.gif` passes
/// `limit` bytes (research R7's size watchdog).
async fn encode_watched(
    ctx: &mut JobCtx<'_>,
    scratch: &Path,
    args: &[std::ffi::OsString],
    total: Ts,
    limit: u64,
) -> Result<()> {
    let stop = ctx.cancel.child_token();
    let watch = SizeWatch::start(scratch.join(ENCODED), limit, &stop);
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
    let tail: String = stderr_tail.chars().filter(|c| !c.is_control() || *c == '\n').collect();
    let last = tail.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no message").trim().to_owned();
    Error::EngineFailed {
        tool: tool.into(),
        message: format!("{last} ({})", super::pipeline::describe(status)),
        stderr_tail: tail,
    }
}

/// The gifsicle post-pass (`-O3`, lossy when set) with a cleared environment, the size watchdog
/// on its output and a wall-clock limit.
async fn run_gifsicle(
    ctx: &mut JobCtx<'_>,
    gifsicle: &Path,
    scratch: &Path,
    lossy: u16,
    limit: u64,
) -> Result<()> {
    let env = crate::gif::engines::engine_env();
    let args = gifsicle_args(lossy, ENCODED, OPTIMISED);
    let stop = ctx.cancel.child_token();
    let watch = SizeWatch::start(scratch.join(OPTIMISED), limit, &stop);
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
