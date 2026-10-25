//! GIF job executor (data-model.md › Stages and weights): palette → encode → optimise →
//! verify for the built-in engine. Every tool runs in the job's scratch folder with generated,
//! relative file names; the result is moved to the job's temp file, verified, then committed by
//! the scheduler.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::JobCtx;
use super::clip::classify_failure;
use super::process;
use super::progress::ProgressParser;
use crate::error::{Error, Result};
use crate::event::JobMeasure;
use crate::gif::graph::{ENCODED, OPTIMISED, encode_args, gifsicle_args, palette_args, palette_name};
use crate::gif::plan::{EngineUsed, GifSpec, Settings};
use crate::gif::verify::{GifExpect, check_gif, gif_facts};
use crate::time::Ts;

/// Without a budget, an encode that passes this size is stopped (a runaway output).
const MAX_OUTPUT: u64 = 4 << 30;

pub(super) async fn execute(spec: &GifSpec, ctx: &mut JobCtx<'_>) -> Result<JobMeasure> {
    let span = spec.cut.requested;
    let total = span.end.saturating_sub(span.start);
    let scratch = ctx
        .scratch_dir()
        .ok_or_else(|| Error::InvalidInput("a GIF job needs its scratch folder".into()))?
        .to_path_buf();
    let s = spec.gif.start.clone();
    let optimise = spec.gif.optimiser.is_some();
    let stages: Vec<(&str, f64)> = if optimise {
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

    next(ctx);
    let palette = palette_name(spec, &s);
    if !scratch.join(&palette).is_file() {
        let out = ctx.run_ffmpeg_in(Some(&scratch), &palette_args(spec, &s, &palette), total).await?;
        if !out.status.success() {
            return Err(classify_failure(&out.stderr_tail, ctx.output));
        }
    }

    next(ctx);
    let limit = spec.gif.budget.map_or(MAX_OUTPUT, |b| b.hard.saturating_mul(2));
    encode_watched(ctx, &scratch, &encode_args(spec, &s, &palette, ENCODED), total, limit).await?;

    let mut result = ENCODED;
    if let (true, Some(gifsicle)) = (optimise, spec.gif.optimiser.as_deref()) {
        next(ctx);
        run_gifsicle(ctx, gifsicle, &scratch, s.lossy).await?;
        result = OPTIMISED;
    }
    move_into(&scratch.join(result), ctx.temp)?;

    next(ctx);
    let facts = gif_facts(ctx.tools, ctx.temp, ctx.cancel).await?;
    let expect = GifExpect {
        width: s.width,
        height: s.height,
        frames: spec.gif.frames,
        loop_count: spec.gif.loop_count,
        max_bytes: spec.gif.budget.map(|b| b.hard),
    };
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
    let over = Arc::new(AtomicBool::new(false));
    let watch = {
        let (stop, over, file) = (stop.clone(), over.clone(), scratch.join(ENCODED));
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if std::fs::metadata(&file).is_ok_and(|m| m.len() > limit) {
                    over.store(true, Ordering::SeqCst);
                    stop.cancel();
                    return;
                }
            }
        })
    };
    let mut parser = ProgressParser::default();
    let ffmpeg = ctx.tools.ffmpeg.path.clone();
    let r = process::run_lines_in(&ffmpeg, args, Some(scratch), &stop, |line| {
        if let Some(block) = parser.feed(line) {
            ctx.progress(block, total);
        }
    })
    .await;
    watch.abort();
    if over.load(Ordering::SeqCst) {
        return Err(Error::engine_failed(
            format!("the GIF grew past {limit} bytes and was stopped"),
            String::new(),
        ));
    }
    let out = r?;
    if out.status.success() { Ok(()) } else { Err(classify_failure(&out.stderr_tail, ctx.output)) }
}

/// The gifsicle post-pass (`-O3`, lossy when set) with a cleared environment.
async fn run_gifsicle(ctx: &mut JobCtx<'_>, gifsicle: &Path, scratch: &Path, lossy: u16) -> Result<()> {
    let env = crate::gif::engines::engine_env();
    let args = gifsicle_args(lossy, ENCODED, OPTIMISED);
    let out = process::run_lines_env(gifsicle, &args, Some(scratch), Some(&env), ctx.cancel, |_| {}).await?;
    if out.status.success() {
        return Ok(());
    }
    let tail: String = out.stderr_tail.chars().filter(|c| !c.is_control() || *c == '\n').collect();
    let last = tail.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no message").trim().to_owned();
    Err(Error::EngineFailed {
        tool: "gifsicle".into(),
        message: format!("{last} ({})", super::pipeline::describe(Some(out.status))),
        stderr_tail: tail,
    })
}

/// Moves the finished GIF from the scratch folder to the temp file (a copy when they are on
/// different file systems).
fn move_into(from: &Path, temp: &Path) -> Result<()> {
    let unwritable =
        |e: std::io::Error| Error::OutputUnwritable { path: temp.to_path_buf(), message: e.to_string() };
    if std::fs::rename(from, temp).is_err() {
        std::fs::copy(from, temp).map_err(unwritable)?;
        let _ = std::fs::remove_file(from);
    }
    Ok(())
}
