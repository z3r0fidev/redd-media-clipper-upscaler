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

mod external;
mod limits;

pub(crate) use external::*;
pub(crate) use limits::*;

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
    // The encode and gifsicle's copy both stay in the scratch folder until the move.
    let limit = Cap::within_free_space(
        budget.map_or(MAX_OUTPUT, |b| b.hard.saturating_mul(2)),
        &scratch,
        if optimise { 2 } else { 1 },
    );
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
            let mut sampler = JobSampler {
                ctx: &mut *ctx,
                spec,
                enc: &mut enc,
                results: Vec::new(),
                samples: 0,
                estimate: None,
            };
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
    // Whether gifsicle's file is the one kept, not only whether it ran.
    let optimised = result.starts_with("opt");
    Ok(measure(spec, &s, bytes, optimised, search, trimmed, mp4_estimate))
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
    limit: Cap,
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
            // gifsicle can make a GIF larger (at lossy 0, by up to about 1%), and its output
            // never looks better than its input: then the plain GIF is kept.
            let size = |f: &str| std::fs::metadata(self.scratch.join(f)).map(|m| m.len());
            return Ok(match (size(optimised), size(encoded)) {
                (Ok(o), Ok(e)) if o >= e => encoded,
                _ => optimised,
            });
        }
        Ok(encoded)
    }
}

/// A warning when `dir`'s folder lacks room for `copies` files of about `bytes` (× the plan's
/// 1.2 margin).
fn space_warning(dir: &Path, bytes: u64, copies: u64) -> Option<String> {
    space_warning_with(dir, bytes, copies, crate::upscale::estimate::free_space)
}

fn space_warning_with(
    dir: &Path,
    bytes: u64,
    copies: u64,
    free: impl Fn(&Path) -> Option<u64>,
) -> Option<String> {
    let folder = dir.parent().unwrap_or(dir).to_path_buf();
    let needs = std::collections::BTreeMap::from([(folder, bytes.saturating_mul(copies))]);
    crate::upscale::estimate::check_space(&needs, free).pop().map(|m| format!("{m} (from the samples)"))
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
    /// The latest predicted size, for the free-space check before the first full encode.
    estimate: Option<u64>,
}

impl budget::Sampler for JobSampler<'_, '_, '_> {
    async fn sample(&mut self, s: &Settings) -> Result<budget::Estimate> {
        let e =
            estimate::sample_estimate(self.ctx.tools, self.spec, s, self.enc.scratch, self.ctx.cancel).await;
        // The search stage moves by samples (at most 7, R5).
        self.samples += 1;
        let total = self.spec.cut.requested.end.saturating_sub(self.spec.cut.requested.start);
        self.ctx.stage_fraction(f64::from(self.samples) / 7.0, total);
        self.estimate = e.as_ref().ok().map(|e| e.bytes).or(self.estimate);
        e
    }

    async fn post(&mut self, s: &Settings) -> Result<budget::Estimate> {
        let e =
            estimate::post_estimate(self.ctx.tools, self.spec, s, self.enc.scratch, self.ctx.cancel).await;
        self.estimate = e.as_ref().ok().map(|e| e.bytes).or(self.estimate);
        e
    }

    async fn full(&mut self, s: &Settings, len: f64) -> Result<u64> {
        // The plan checked the folder against the hard limit; the samples now give a closer
        // size. Scratch is in the output folder: the encode and gifsicle's copy both stay there.
        if self.enc.fulls == 0
            && let Some(bytes) = self.estimate
            && let Some(message) =
                space_warning(self.enc.scratch, bytes, if self.enc.optimise { 2 } else { 1 })
        {
            self.ctx.warn_code(IssueCode::DiskFull, message);
        }
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
    limit: Cap,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// T080 L6: the sampled size, not the hard limit, decides the run-time warning.
    #[test]
    fn the_samples_decide_the_free_space_warning() {
        let scratch = Path::new("/out/gifs/.rmedia-run-1");
        let free = |d: &Path| (d == Path::new("/out/gifs")).then_some(5_000_000);
        assert_eq!(space_warning_with(scratch, 2_000_000, 2, free), None, "4.8 MB of 5.0 MB");
        let m = space_warning_with(scratch, 2_200_000, 2, free).unwrap();
        assert!(m.starts_with("/out/gifs has 5.0 MB free") && m.ends_with("(from the samples)"), "{m}");
        assert_eq!(space_warning_with(scratch, u64::MAX, 2, |_| None), None, "unknown free space");
    }

    /// Security review L2: the watchdog never lets the job's files outgrow the free space.
    #[test]
    fn the_cap_is_the_free_space_left_for_each_copy() {
        const MB: u64 = 1 << 20;
        assert_eq!(Cap::within(MAX_OUTPUT, 2, Some(100 << 30)), Cap::size(MAX_OUTPUT), "plenty of room");
        assert_eq!(Cap::within(MAX_OUTPUT, 2, None), Cap::size(MAX_OUTPUT), "unknown free space");
        assert_eq!(
            Cap::within(MAX_OUTPUT, 2, Some(1064 * MB)),
            Cap { bytes: 500 * MB, from_space: true },
            "(1 GiB + 40 MiB − the 64 MiB reserve) / 2"
        );
        assert_eq!(Cap::within(40_000_000, 1, Some(1 << 30)), Cap::size(40_000_000), "a budget's cap fits");
        assert_eq!(Cap::within(MAX_OUTPUT, 1, Some(10 * MB)), Cap { bytes: 0, from_space: true });
    }

    /// Every stage list's weights add up to the whole bar, in the order the job runs them.
    #[test]
    fn the_stages_fill_the_bar_in_order() {
        for bits in 0..16u8 {
            let [gifski, optimise, search, analyze] = [0, 1, 2, 3].map(|b| bits & (1 << b) != 0);
            let st = Stages::new(gifski, optimise, search, analyze);
            let sum: f64 = st.list.iter().map(|s| s.1).sum();
            assert!((sum - 1.0).abs() < 1e-9, "{bits:04b}: {sum}");
            let names: Vec<&str> = st.list.iter().map(|s| s.0).collect();
            assert_eq!(names.last(), Some(&"verify"));
            assert_eq!(names.contains(&"analyze"), analyze);
            assert_eq!(names.contains(&"search"), search);
            assert_eq!(names.contains(&"palette"), !gifski, "{names:?}");
            assert_eq!(names.contains(&"optimise"), !gifski && optimise, "{names:?}");
            assert_eq!(st.position(names[0]).map(|p| p.0), Some(1));
            assert_eq!(st.position("nope"), None);
        }
    }

    #[test]
    fn a_retry_writes_other_files() {
        let (a, b) = (attempt_names(0), attempt_names(1));
        assert_eq!(a, (ENCODED, OPTIMISED, ENCODED_MP4));
        assert!(a.0 != b.0 && a.1 != b.1 && a.2 != b.2);
    }

    /// gifsicle and gifski are less trusted than ffmpeg: their last line is shown without
    /// control characters.
    #[test]
    fn an_engine_failure_shows_its_last_clean_line() {
        let e = tool_failed("gifski", None, "frame 1\n\x1b[31mbad\x07 input\n  \n");
        let Error::EngineFailed { tool, message, stderr_tail } = &e else { panic!("{e:?}") };
        assert_eq!(tool, "gifski");
        assert!(message.starts_with("[31mbad input ("), "{message}");
        assert!(!stderr_tail.contains('\x1b') && !stderr_tail.contains('\x07'));
        let Error::EngineFailed { message, .. } = tool_failed("gifsicle", None, "") else { unreachable!() };
        assert!(message.starts_with("no message ("), "{message}");
    }

    #[tokio::test(start_paused = true)]
    async fn the_deadline_cancels_only_when_it_passes() {
        let stop = CancellationToken::new();
        let d = Deadline::start(Duration::from_secs(60), &stop);
        tokio::time::sleep(Duration::from_secs(59)).await;
        assert!(!d.fired() && !stop.is_cancelled());
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(d.fired() && stop.is_cancelled());
        // Dropped early, it never fires.
        let stop = CancellationToken::new();
        drop(Deadline::start(Duration::from_secs(1), &stop));
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert!(!stop.is_cancelled());
    }

    /// Review item 17: the watchdog fires within one poll of the file passing the limit, so the
    /// overshoot is at most what the encoder writes in one `SIZE_POLL` (plus one write). Paused
    /// time makes the writer's pace and the polls exact.
    #[tokio::test(start_paused = true)]
    async fn the_overshoot_is_at_most_one_poll_of_writing() {
        const CHUNK: usize = 64 * 1024;
        const STEP: Duration = Duration::from_millis(10);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("enc.gif");
        std::fs::write(&file, b"").unwrap();
        let limit = 3 * 1024 * 1024 + 123;
        let stop = CancellationToken::new();
        let watch = SizeWatch::start(file.clone(), Cap::size(limit), &stop);
        let writer = {
            let (file, stop) = (file.clone(), stop.clone());
            tokio::spawn(async move {
                use std::io::Write;
                let mut f = std::fs::OpenOptions::new().append(true).open(&file).unwrap();
                while !stop.is_cancelled() {
                    f.write_all(&[7u8; CHUNK]).unwrap();
                    tokio::time::sleep(STEP).await;
                }
            })
        };
        tokio::time::timeout(Duration::from_secs(60), stop.cancelled()).await.expect("the watchdog fires");
        writer.await.unwrap();
        let len = std::fs::metadata(&file).unwrap().len();
        let per_poll = (SIZE_POLL.as_millis() / STEP.as_millis()) as u64 + 1;
        assert!(len > limit, "fired only after the limit: {len}");
        assert!(len - limit <= per_poll * CHUNK as u64, "overshoot {} > {} chunks", len - limit, per_poll);
        assert_eq!(watch.stop().unwrap_err().code(), "ENGINE_FAILED");
    }

    /// The limit is a maximum: a file of exactly `limit` bytes, or none yet, is not stopped.
    #[tokio::test(start_paused = true)]
    async fn a_file_at_the_limit_or_missing_is_not_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let (at, missing) = (dir.path().join("at.gif"), dir.path().join("missing.gif"));
        std::fs::write(&at, vec![0u8; 4096]).unwrap();
        let (s1, s2) = (CancellationToken::new(), CancellationToken::new());
        let w1 = SizeWatch::start(at, Cap::size(4096), &s1);
        let w2 = SizeWatch::start(missing, Cap::size(0), &s2);
        tokio::time::sleep(SIZE_POLL * 40).await;
        assert!(!s1.is_cancelled() && !s2.is_cancelled());
        assert!(w1.stop().is_ok() && w2.stop().is_ok());
    }

    #[tokio::test]
    async fn passing_a_free_space_cap_is_disk_full_for_the_output_folder() {
        let out = tempfile::tempdir().unwrap();
        let scratch = out.path().join(".rmedia-run-1");
        std::fs::create_dir(&scratch).unwrap();
        std::fs::write(scratch.join("enc.gif"), vec![0u8; 2048]).unwrap();
        for (from_space, want) in [(true, "DISK_FULL"), (false, "ENGINE_FAILED")] {
            let stop = CancellationToken::new();
            let watch = SizeWatch::start(scratch.join("enc.gif"), Cap { bytes: 1024, from_space }, &stop);
            tokio::time::timeout(Duration::from_secs(5), stop.cancelled()).await.expect("the watchdog fires");
            let e = watch.stop().unwrap_err();
            assert_eq!(e.code(), want);
            if from_space {
                assert!(matches!(&e, Error::DiskFull(p) if p == out.path()), "{e:?}");
            }
        }
    }
}
