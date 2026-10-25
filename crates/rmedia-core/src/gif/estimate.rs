//! Sample encodes and the size model (research R4). The estimate shown before encoding comes
//! from one ffmpeg run over three 1 s windows of the range (or the whole range, when it is
//! short), optimised as the job would be (or through the gifski pipe), then scaled up from its first frame and the mean of
//! its delta frames. Samples never run in the planner: [`prepare`] runs them after planning, in
//! a temporary folder of its own.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use super::budget;
use super::graph::{gifsicle_args, mp4_codec_args, palette_settings, pipe_head, y4m_graph, y4m_tail};
use super::plan::{EngineUsed, Estimate, EstimateFrom, GifSpec, Settings};
use crate::error::{Error, Result};
use crate::exec::process;
use crate::plan::ffargs::{common_args, input_parts};
use crate::plan::{Disposition, JobSpec, Plan};
use crate::probe::{INPUT_PROTOCOLS, file_arg};
use crate::time::Ts;
use crate::tools::Tools;
use crate::upscale::graph::{GifChain, frame_chain};

/// The length of one sample window, in seconds.
pub const WINDOW: f64 = 1.0;
/// Ranges up to this long are encoded whole: the sample is the output itself. With three
/// windows of [`WINDOW`] the sample never covers more than 3 s (150 frames at 50 fps), the
/// bound in contracts/tauri-commands.md.
pub const WHOLE_UP_TO: f64 = 3.0;
/// A sample that grows past this, or runs longer than [`SAMPLE_TIMEOUT`], is stopped.
const SAMPLE_LIMIT: u64 = 512 << 20;
const SAMPLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
/// Where the windows are centred, as fractions of the range.
const CENTRES: [f64; 3] = [0.1, 0.5, 0.9];

const SAMPLE: &str = "sample.gif";
const SAMPLE_OPT: &str = "sample-opt.gif";
const SAMPLE_MP4: &str = "sample.mp4";
/// The job's `reframe.cmd`: the opening crop position, then the path's moves (a crop that never
/// moves would leave `sendcmd` with no command, which it refuses).
pub fn job_cmds(a: &crate::upscale::reframe::ReframeAnalysis, offset: f64) -> String {
    let x0 = a.path.first().copied().unwrap_or(0);
    format!("{offset:.6} crop@trk x {x0};\n{}", crate::upscale::reframe::sendcmd::to_sendcmd(a, offset))
}
/// The job's smart-framing commands (the frame chain reads them in its working folder).
pub const REFRAME_CMD: &str = "reframe.cmd";
const SAMPLE_CMD: &str = "sample.cmd";

/// The source times the sample windows start at, for a range of `len` seconds from `start`;
/// `None` when the range is short enough to encode whole.
pub fn windows(start: Ts, len: f64) -> Option<[Ts; 3]> {
    (len > WHOLE_UP_TO).then(|| {
        CENTRES.map(|c| start + Ts::from_secs_f64((c * len - WINDOW / 2.0).clamp(0.0, len - WINDOW)))
    })
}

/// The smart-framing commands of a sample in `dir`, when the job follows a subject: the job's own
/// `reframe.cmd` for a whole-range sample, or `sample.cmd` on the windows' joined timeline
/// (window `k` starts at `k` seconds).
fn write_sample_cmds(spec: &GifSpec, starts: Option<&[Ts; 3]>, dir: &Path) -> Result<()> {
    use crate::upscale::reframe::sendcmd::to_sendcmd;
    let Some(sp) = &spec.frame.smart else { return Ok(()) };
    let Some(a) = &sp.analysis else { return Ok(()) };
    let (name, text) = match starts {
        None => (REFRAME_CMD, job_cmds(a, sp.cmd_offset)),
        Some(w) => {
            let joined = joined_analysis(a, w, WINDOW);
            let first = joined.path.first().copied().unwrap_or(0);
            (SAMPLE_CMD, format!("0.000000 crop@trk x {first};\n{}", to_sendcmd(&joined, 0.0)))
        }
    };
    std::fs::write(dir.join(name), text)
        .map_err(|e| Error::OutputUnwritable { path: dir.join(name), message: e.to_string() })
}

/// The analysis of the sample windows `starts`, each `window` seconds long, laid end to end from
/// 0 (the sample's `concat` timeline): the crop path and the fallback ranges of each window.
pub fn joined_analysis(
    a: &crate::upscale::reframe::ReframeAnalysis,
    starts: &[Ts],
    window: f64,
) -> crate::upscale::reframe::ReframeAnalysis {
    let per = (window * a.fps).round().max(1.0) as usize;
    let last = a.path.len().saturating_sub(1);
    let (mut path, mut ranges) = (Vec::new(), Vec::new());
    for (k, s) in starts.iter().enumerate() {
        let rel = s.as_secs_f64() - a.clip_start.as_secs_f64();
        for j in 0..per {
            let at = ((rel + j as f64 / a.fps) * a.fps).round().max(0.0) as usize;
            path.push(a.path.get(at.min(last)).copied().unwrap_or(0));
        }
        let (from, to) = (s.as_secs_f64(), s.as_secs_f64() + window);
        for r in &a.fallback_ranges {
            let (on, off) = (r.start.as_secs_f64().max(from), r.end.as_secs_f64().min(to));
            if on < off {
                let base = k as f64 * window;
                ranges.push(crate::event::Span {
                    start: Ts::from_secs_f64(base + on - from),
                    end: Ts::from_secs_f64(base + off - from),
                });
            }
        }
    }
    crate::upscale::reframe::ReframeAnalysis {
        path,
        fallback_ranges: ranges,
        clip_start: Ts::ZERO,
        ..a.clone()
    }
}

/// The frame chain of `s` reading from `label`, without the job's own trim (a sample seeks its
/// windows itself).
fn chain_from(spec: &GifSpec, s: &Settings, label: &str, trim: bool) -> String {
    let mut g = spec.frame.graph_at((s.width, s.height));
    if !trim {
        g.trim = None;
    }
    let chain = frame_chain(
        &g,
        &GifChain { rate: &s.rate_text(), sharpen: spec.frame.sharpen, mp4: s.crf.is_some() },
    );
    let rest = chain.strip_prefix("[0:v]").expect("a frame chain reads the first input's video");
    // Windows have their own commands, on the joined timeline (see `write_sample_cmds`).
    let rest = if trim {
        rest.to_owned()
    } else {
        rest.replace(&format!("sendcmd=f={REFRAME_CMD}"), &format!("sendcmd=f={SAMPLE_CMD}"))
    };
    format!("{label}{rest}")
}

/// The palette and its use in one graph: `split`, `palettegen`, `paletteuse`. Equivalent to the
/// job's two passes over the same frames.
fn one_pass(chain: &str, s: &Settings) -> String {
    format!(
        "{chain},split[s0][s1];[s0]{}[p];[s1][p]paletteuse={}:diff_mode=rectangle[g]",
        palette_settings(s),
        s.dither.paletteuse()
    )
}

fn os(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

fn gif_tail(spec: &GifSpec, output: &str) -> Vec<OsString> {
    os(&[
        "-map",
        "[g]",
        "-an",
        "-sn",
        "-dn",
        "-map_metadata",
        "-1",
        "-f",
        "gif",
        "-fflags",
        "+bitexact",
        "-loop",
        &spec.gif.loop_count.repeat_arg().to_string(),
        output,
    ])
}

/// What a sample encode writes.
#[derive(Debug, Clone, Copy)]
pub enum SampleOut<'a> {
    /// The built-in engine's GIF, at this relative name.
    Gif(&'a str),
    /// y4m frames on stdout, for gifski.
    Y4m,
    /// The Reddit MP4, at this relative name.
    Mp4(&'a str),
}

impl SampleOut<'_> {
    fn head(self) -> Vec<OsString> {
        match self {
            SampleOut::Gif(_) | SampleOut::Mp4(_) => common_args(),
            SampleOut::Y4m => pipe_head(None),
        }
    }

    /// The rest of the graph after `chain`, and the output options.
    fn finish(self, spec: &GifSpec, s: &Settings, chain: &str) -> (String, Vec<OsString>) {
        match self {
            SampleOut::Gif(output) => (one_pass(chain, s), gif_tail(spec, output)),
            SampleOut::Y4m => (y4m_graph(chain), y4m_tail()),
            SampleOut::Mp4(output) => {
                let mut tail = os(&["-map", "[g]"]);
                tail.extend(mp4_codec_args(s.crf.unwrap_or(spec.gif.target.mp4.crf), output));
                (format!("{chain}[g]"), tail)
            }
        }
    }
}

/// The sample encode of `s`: each window is its own input with a fast seek, the windows are
/// joined with `concat`, then framed, scaled and paletted (or made y4m) as one clip. With smart
/// framing (US4), crop commands are written on the joined timeline: window `k` starts at `k`
/// seconds.
pub fn sample_args(spec: &GifSpec, s: &Settings, starts: &[Ts], out: SampleOut) -> Vec<OsString> {
    let mut a = out.head();
    for w in starts {
        a.extend(os(&[
            "-ss",
            &w.ffmpeg(),
            "-t",
            &format!("{WINDOW}"),
            "-protocol_whitelist",
            INPUT_PROTOCOLS,
        ]));
        a.push("-i".into());
        a.push(file_arg(&spec.cut.input));
    }
    let inputs: String = (0..starts.len()).map(|i| format!("[{i}:v]")).collect();
    let joined = format!("{inputs}concat=n={}:v=1:a=0[cat];", starts.len());
    let (rest, tail) = out.finish(spec, s, &chain_from(spec, s, "[cat]", false));
    a.extend(os(&["-filter_complex", &(joined + &rest)]));
    a.extend(tail);
    a
}

/// The whole range of a short job, in one run: the same frames as the job's own encode.
pub fn whole_args(spec: &GifSpec, s: &Settings, out: SampleOut) -> Vec<OsString> {
    let (source, range) = input_parts(&spec.cut);
    let mut a = out.head();
    a.extend(source);
    a.extend(range);
    let (graph, tail) = out.finish(spec, s, &chain_from(spec, s, "[0:v]", true));
    a.extend(os(&["-filter_complex", &graph]));
    a.extend(tail);
    a
}

/// A measured sample: the file size and each frame's (start in seconds, bytes).
#[derive(Debug, Clone, PartialEq)]
pub struct SampleSize {
    pub bytes: u64,
    pub frames: Vec<(f64, u64)>,
}

/// The size model: `header + first frame + mean delta × (frames − 1)`. A frame that starts a
/// window (at `k × WINDOW` s) is a key frame of the sample, not of the GIF, so it is left out
/// of the mean.
pub fn model(sample: &SampleSize, windows: usize, frames: u64) -> u64 {
    let Some(&(_, first)) = sample.frames.first() else { return sample.bytes };
    let in_frames: u64 = sample.frames.iter().map(|f| f.1).sum();
    let header = sample.bytes.saturating_sub(in_frames);
    // The first frame at or after each later window's start.
    let keys: Vec<usize> = (1..windows)
        .filter_map(|k| sample.frames.iter().position(|(t, _)| *t >= k as f64 * WINDOW - 0.005))
        .collect();
    let deltas: Vec<u64> = sample
        .frames
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(i, _)| !keys.contains(i))
        .map(|(_, f)| f.1)
        .collect();
    if deltas.is_empty() {
        // Every later frame starts a window (a very low frame rate): their mean is all there
        // is. A single frame is a static sample that gifsicle merged: the GIF is one frame too.
        let rest: Vec<u64> = sample.frames.iter().skip(1).map(|f| f.1).collect();
        if rest.is_empty() {
            return header + first;
        }
        let mean = rest.iter().sum::<u64>() as f64 / rest.len() as f64;
        return header + first + (mean * frames.saturating_sub(1) as f64).round() as u64;
    }
    let mean = deltas.iter().sum::<u64>() as f64 / deltas.len() as f64;
    header + first + (mean * frames.saturating_sub(1) as f64).round() as u64
}

/// Each frame's start time and size, from ffprobe.
async fn frame_sizes(tools: &Tools, path: &Path, cancel: &CancellationToken) -> Result<SampleSize> {
    let bytes = std::fs::metadata(path)
        .map_err(|e| Error::engine_failed(format!("the sample was not written: {e}"), String::new()))?
        .len();
    let args: Vec<OsString> = os(&[
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "packet=pts_time,size",
        "-of",
        "csv=p=0",
    ])
    .into_iter()
    .chain([file_arg(path)])
    .collect();
    let out = process::run_capture(&tools.ffprobe.path, &args, cancel).await?;
    if !out.status.success() {
        return Err(Error::engine_failed("cannot read the sample's frames", out.stderr_tail));
    }
    let frames = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut f = l.trim().trim_end_matches(',').split(',');
            Some((f.next()?.parse().ok()?, f.next()?.parse().ok()?))
        })
        .collect();
    Ok(SampleSize { bytes, frames })
}

/// Runs `run` (given its own stop token) under the sample's size watchdog on `file` and its
/// wall-clock limit.
async fn bounded<T, F>(
    file: std::path::PathBuf,
    cancel: &CancellationToken,
    run: impl FnOnce(CancellationToken) -> F,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    use crate::exec::gif::{Deadline, SizeWatch};
    let stop = cancel.child_token();
    let watch = SizeWatch::start(file, SAMPLE_LIMIT, &stop);
    let deadline = Deadline::start(SAMPLE_TIMEOUT, &stop);
    let r = run(stop.clone()).await;
    watch.stop()?;
    if deadline.fired() && !cancel.is_cancelled() {
        return Err(Error::engine_failed(
            format!("the sample took longer than {} s and was stopped", SAMPLE_TIMEOUT.as_secs()),
            String::new(),
        ));
    }
    r
}

/// The frame count of a GIF of `spec`'s range at `s`'s frame rate.
pub fn frames_at(spec: &GifSpec, s: &Settings) -> u64 {
    crate::upscale::reframe::frame_count(spec.gif.len, s.fps()) as u64
}

/// Measures the sample file `name` in `dir` and predicts the job's GIF at `s` from it.
async fn measure(
    tools: &Tools,
    spec: &GifSpec,
    s: &Settings,
    dir: &Path,
    name: &str,
    cancel: &CancellationToken,
) -> Result<budget::Estimate> {
    let size = frame_sizes(tools, &dir.join(name), cancel).await?;
    let in_frames: u64 = size.frames.iter().map(|f| f.1).sum();
    let fixed = size.bytes.saturating_sub(in_frames) + size.frames.first().map_or(0, |f| f.1);
    let bytes = match windows(spec.cut.requested.start, spec.gif.len) {
        Some(w) => model(&size, w.len(), frames_at(spec, s)),
        None => size.bytes,
    };
    Ok(budget::Estimate { sample_bytes: size.bytes, bytes, fixed })
}

/// Encodes the sample of `s` in `dir` and measures it (optimised by gifsicle at `s.lossy` when
/// the job is; through gifski when that is the engine): a budget search's sample step.
pub async fn sample_estimate(
    tools: &Tools,
    spec: &GifSpec,
    s: &Settings,
    dir: &Path,
    cancel: &CancellationToken,
) -> Result<budget::Estimate> {
    for f in [SAMPLE, SAMPLE_OPT, SAMPLE_MP4] {
        let _ = std::fs::remove_file(dir.join(f));
    }
    let starts = windows(spec.cut.requested.start, spec.gif.len);
    let gifski =
        spec.gif.gifski.as_deref().filter(|_| spec.gif.engine == EngineUsed::Gifski && s.crf.is_none());
    let out = match (gifski, s.crf) {
        (_, Some(_)) => SampleOut::Mp4(SAMPLE_MP4),
        (Some(_), None) => SampleOut::Y4m,
        (None, None) => SampleOut::Gif(SAMPLE),
    };
    write_sample_cmds(spec, starts.as_ref(), dir)?;
    let args = match &starts {
        Some(w) => sample_args(spec, s, w, out),
        None => whole_args(spec, s, out),
    };
    if let Some(program) = gifski {
        let pipe = crate::exec::gif::Gifski {
            program,
            dir,
            output: SAMPLE,
            limit: SAMPLE_LIMIT,
            timeout: Some(SAMPLE_TIMEOUT),
        };
        let job_output = dir.join(SAMPLE);
        pipe.run(tools, args, s, spec.gif.loop_count, None, &job_output, cancel, |_| {}).await?;
    } else {
        let written = if s.crf.is_some() { SAMPLE_MP4 } else { SAMPLE };
        let out = bounded(dir.join(written), cancel, |stop| async move {
            process::run_lines_in(&tools.ffmpeg.path, &args, Some(dir), &stop, |_| {}).await
        })
        .await?;
        if !out.status.success() {
            return Err(Error::engine_failed("the sample encode failed", out.stderr_tail));
        }
    }
    if s.crf.is_some() {
        return mp4_measure(spec, dir);
    }
    if spec.gif.optimiser.is_some() && s.gifski.is_none() {
        return post_estimate(tools, spec, s, dir, cancel).await;
    }
    measure(tools, spec, s, dir, SAMPLE, cancel).await
}

/// An MP4 sample's prediction: its bytes per second of video, over the job's length (no
/// GIF-style key frame; the container's few hundred bytes are in the rate).
fn mp4_measure(spec: &GifSpec, dir: &Path) -> Result<budget::Estimate> {
    let bytes = std::fs::metadata(dir.join(SAMPLE_MP4))
        .map_err(|e| Error::engine_failed(format!("the MP4 sample was not written: {e}"), String::new()))?
        .len();
    let sampled = match windows(spec.cut.requested.start, spec.gif.len) {
        Some(w) => w.len() as f64 * WINDOW,
        None => spec.gif.len,
    };
    let predicted = (bytes as f64 * spec.gif.len / sampled.max(f64::EPSILON)).round() as u64;
    Ok(budget::Estimate { sample_bytes: bytes, bytes: predicted, fixed: 0 })
}

/// The "MP4 would be smaller" hint of a GIF job (US3 scenario 2, R6): one MP4 sample of the same
/// windows, at the GIF's start size and the MP4's own rate and CRF (what `--format mp4` makes).
pub async fn mp4_hint(tools: &Tools, spec: &GifSpec, dir: &Path, cancel: &CancellationToken) -> Result<u64> {
    let mut s = spec.gif.start.clone();
    s.crf = Some(spec.gif.target.mp4.crf);
    s.rate = spec.gif.mp4_rate;
    s.width &= !1;
    s.height = (s.height / 2 * 2).max(2);
    s.gifski = None;
    Ok(sample_estimate(tools, spec, &s, dir, cancel).await?.bytes)
}

/// Re-runs gifsicle on the last sample at `s.lossy` and measures it: a budget search's
/// post-pass (not an encode).
pub async fn post_estimate(
    tools: &Tools,
    spec: &GifSpec,
    s: &Settings,
    dir: &Path,
    cancel: &CancellationToken,
) -> Result<budget::Estimate> {
    let Some(gifsicle) = spec.gif.optimiser.as_deref() else {
        return measure(tools, spec, s, dir, SAMPLE, cancel).await;
    };
    let _ = std::fs::remove_file(dir.join(SAMPLE_OPT));
    let env = super::engines::engine_env();
    let a = gifsicle_args(s.lossy, SAMPLE, SAMPLE_OPT);
    let out = bounded(dir.join(SAMPLE_OPT), cancel, |stop| async move {
        process::run_lines_env(gifsicle, &a, Some(dir), Some(&env), &stop, |_| {}).await
    })
    .await?;
    if !out.status.success() {
        return Err(Error::EngineFailed {
            tool: "gifsicle".into(),
            message: "the sample optimisation failed".into(),
            stderr_tail: out.stderr_tail,
        });
    }
    measure(tools, spec, s, dir, SAMPLE_OPT, cancel).await
}

/// [`sample_estimate`]'s predicted size of the job's GIF.
pub async fn sample(
    tools: &Tools,
    spec: &GifSpec,
    s: &Settings,
    dir: &Path,
    cancel: &CancellationToken,
) -> Result<u64> {
    Ok(sample_estimate(tools, spec, s, dir, cancel).await?.bytes)
}

/// The dry run's [`budget::Sampler`]: real samples and post-passes, and a "full encode" that is
/// the prediction made for those settings (scaled to a trimmed length).
struct Predicting<'a> {
    tools: &'a Tools,
    spec: &'a GifSpec,
    dir: &'a Path,
    cancel: &'a CancellationToken,
    seen: Vec<(Settings, budget::Estimate)>,
}

impl budget::Sampler for Predicting<'_> {
    async fn sample(&mut self, s: &Settings) -> Result<budget::Estimate> {
        let e = sample_estimate(self.tools, self.spec, s, self.dir, self.cancel).await?;
        self.seen.push((s.clone(), e));
        Ok(e)
    }

    async fn post(&mut self, s: &Settings) -> Result<budget::Estimate> {
        let e = post_estimate(self.tools, self.spec, s, self.dir, self.cancel).await?;
        self.seen.push((s.clone(), e));
        Ok(e)
    }

    async fn full(&mut self, s: &Settings, len: f64) -> Result<u64> {
        // The search's retry encodes settings it never sampled: the nearest prediction is the
        // last one (the retry is a step below it).
        let e = self
            .seen
            .iter()
            .rev()
            .find(|(seen, _)| seen == s)
            .or(self.seen.last())
            .map(|(_, e)| *e)
            .ok_or_else(|| Error::InvalidInput("a full encode before any sample".into()))?;
        let share = (len / self.spec.gif.len.max(f64::EPSILON)).min(1.0);
        Ok(e.fixed + (e.bytes.saturating_sub(e.fixed) as f64 * share).round() as u64)
    }
}

/// The search's input for `spec`: the target's floors (or the start, when it has none).
pub fn search_input(spec: &GifSpec, budget: super::target::Budget) -> budget::SearchInput {
    let start = spec.gif.start.clone();
    budget::SearchInput {
        floor_width: spec.gif.floor_width.unwrap_or(start.width).min(start.width),
        floor_fps: spec.gif.floor_fps.unwrap_or_else(|| start.fps().ceil() as u32),
        budget,
        optimiser: spec.gif.optimiser.is_some(),
        allow_trim: spec.gif.allow_trim,
        len: spec.gif.len,
        start,
    }
}

/// The estimate shown before encoding: with a budget, the predicted result of the search
/// (samples only, nothing encoded in full); otherwise one sample at the starting settings.
pub async fn predict(
    tools: &Tools,
    spec: &GifSpec,
    dir: &Path,
    cancel: &CancellationToken,
) -> Result<Estimate> {
    let mut est = if let Some(b) = spec.gif.budget {
        let mut sampler = Predicting { tools, spec, dir, cancel, seen: Vec::new() };
        let found = budget::search(&search_input(spec, b), &mut sampler).await?;
        Estimate {
            bytes: Some(found.bytes),
            from: EstimateFrom::Search,
            settings: found.chosen,
            lowered: found.record.lowered,
            mp4_bytes: None,
        }
    } else {
        let s = spec.gif.start.clone();
        let bytes = sample(tools, spec, &s, dir, cancel).await?;
        Estimate {
            bytes: Some(bytes),
            from: EstimateFrom::Sample,
            settings: s,
            lowered: Vec::new(),
            mp4_bytes: None,
        }
    };
    if spec.gif.format == super::Format::Gif && spec.gif.target.mp4.hint {
        match mp4_hint(tools, spec, dir, cancel).await {
            Ok(b) => est.mp4_bytes = Some(b),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(_) => {} // the hint is optional: a failed MP4 sample leaves it out
        }
    }
    Ok(est)
}

/// A private temporary folder, removed with everything in it when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(parent: &Path) -> std::io::Result<TempDir> {
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let path = parent.join(format!("rmedia-sample-{}-{nanos}", std::process::id()));
        crate::exec::commit::create_scratch(&path)?;
        Ok(TempDir(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs the smart-framing analysis of `g` when it is still pending (002's analysis and cache),
/// and applies it. A failed analysis switches the job to the blurred background and is
/// returned (cancelling too); `Ok(Some(note))` when there was no subject to follow.
pub async fn analyze(
    g: &mut GifSpec,
    tools: &Tools,
    cancel: &CancellationToken,
    progress: impl FnMut(f64) + Send,
) -> Result<Option<&'static str>> {
    let pending = g.frame.smart.as_ref().is_some_and(|sp| sp.analysis.is_none());
    if !pending {
        return Ok(None);
    }
    let input = g.reframe_input().expect("a smart plan");
    match crate::upscale::reframe::analyze_cached(tools, &input, cancel, progress).await {
        Ok((a, _)) => Ok(g.with_analysis(a)),
        Err(Error::Cancelled) => Err(Error::Cancelled),
        Err(e) => {
            g.blur();
            Err(e)
        }
    }
}

/// Runs the smart-framing analysis of every GIF job of `plan` that still needs one (for
/// `--crop-path`; the executor and the dry run do it themselves otherwise). A failed analysis
/// is a warning on its job; only cancelling is an error.
pub async fn analyze_plan(plan: &mut Plan, tools: &Tools, cancel: &CancellationToken) -> Result<()> {
    for job in &mut plan.jobs {
        let JobSpec::Gif(g) = &mut job.spec else { continue };
        match analyze(g, tools, cancel, |_| {}).await {
            Ok(Some(note)) => job.warnings.push(note.into()),
            Ok(None) => {}
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => job
                .warnings
                .push(format!("smart framing analysis failed ({e}); using the blurred background")),
        }
    }
    Ok(())
}

/// The dry run's prepare step: fills in the estimate of every GIF job that would run. A failed
/// sample leaves its estimate pending, with a warning; only cancelling is an error.
pub async fn prepare(plan: &mut Plan, tools: &Tools, cancel: &CancellationToken) -> Result<()> {
    prepare_in(plan, tools, cancel, &std::env::temp_dir()).await
}

/// [`prepare`], with the temporary folders made in `parent`.
pub async fn prepare_in(
    plan: &mut Plan,
    tools: &Tools,
    cancel: &CancellationToken,
    parent: &Path,
) -> Result<()> {
    for job in &mut plan.jobs {
        let JobSpec::Gif(g) = &mut job.spec else { continue };
        if job.disposition != Disposition::Run || g.gif.estimate.from != EstimateFrom::Pending {
            continue;
        }
        match analyze(g, tools, cancel, |_| {}).await {
            Ok(Some(note)) => job.warnings.push(note.into()),
            Ok(None) => {}
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => job
                .warnings
                .push(format!("smart framing analysis failed ({e}); using the blurred background")),
        }
        let dir = match TempDir::new(parent) {
            Ok(d) => d,
            Err(e) => {
                job.warnings.push(format!("no size estimate: cannot create a temporary folder ({e})"));
                continue;
            }
        };
        match predict(tools, g, &dir.0, cancel).await {
            Ok(e) => g.gif.estimate = e,
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => job.warnings.push(format!("no size estimate: {e}")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_sit_at_10_50_and_90_percent_of_the_range() {
        let w = windows(Ts::from_secs_f64(100.0), 20.0).unwrap();
        let secs: Vec<f64> = w.iter().map(|t| (t.as_secs_f64() * 1000.0).round() / 1000.0).collect();
        assert_eq!(secs, [101.5, 109.5, 117.5]);
        let short = windows(Ts::ZERO, 5.0).unwrap();
        assert_eq!(short[0], Ts::ZERO, "clamped to the range's start");
        assert_eq!(short[2].as_secs_f64(), 4.0, "clamped so the window ends with the range");
    }

    #[test]
    fn sample_windows_crop_like_the_full_encode_at_the_same_times() {
        use crate::event::Span;
        use crate::upscale::reframe::ReframeAnalysis;
        // 10 s at 10 fps from 5 s: the crop x is the frame number; blurred background 6.5–7.5 s.
        let a = ReframeAnalysis {
            key: String::new(),
            algo_version: 0,
            source_size: (1280, 720),
            crop: (576, 720),
            crop_y: 0,
            fps: 10.0,
            clip_start: Ts::from_secs_f64(5.0),
            shots: Vec::new(),
            fallback_ranges: vec![Span { start: Ts::from_secs_f64(6.5), end: Ts::from_secs_f64(7.5) }],
            path: (0..100).collect(),
            detections: 1,
            samples: 1,
        };
        let starts = [Ts::from_secs_f64(5.0), Ts::from_secs_f64(7.0), Ts::from_secs_f64(13.5)];
        let j = joined_analysis(&a, &starts, 1.0);
        assert_eq!(j.path.len(), 30);
        assert_eq!(j.clip_start, Ts::ZERO);
        for (k, s) in starts.iter().enumerate() {
            let first = ((s.as_secs_f64() - 5.0) * 10.0).round() as i32;
            let want: Vec<i32> = (first..first + 10).collect();
            assert_eq!(j.path[k * 10..k * 10 + 10], want[..], "window {k}");
        }
        // 6.5–7.5 s overlaps the second window (7.0–8.0 s) over 7.0–7.5: joined 1.0–1.5 s.
        assert_eq!(j.fallback_ranges, [Span { start: Ts::from_secs_f64(1.0), end: Ts::from_secs_f64(1.5) }]);
    }

    #[test]
    fn short_ranges_are_encoded_whole() {
        assert!(windows(Ts::ZERO, 3.0).is_none());
        assert!(windows(Ts::ZERO, 3.5).is_some(), "longer ranges are sampled in 3 × 1 s");
        assert!(windows(Ts::ZERO, 1.5).is_none());
    }

    #[test]
    fn the_model_charges_one_key_frame_and_scales_the_deltas() {
        // Three windows of 4 frames at 4 fps: key frames of 1000 bytes, deltas of 100.
        let mut frames = Vec::new();
        for k in 0..3 {
            for i in 0..4 {
                frames.push((k as f64 + i as f64 * 0.25, if i == 0 { 1000 } else { 100 }));
            }
        }
        let body: u64 = frames.iter().map(|f| f.1).sum();
        let sample = SampleSize { bytes: body + 800, frames };
        // 40 frames: header 800 + first 1000 + 39 × 100.
        assert_eq!(model(&sample, 3, 40), 800 + 1000 + 3900);
    }

    #[test]
    fn a_static_sample_is_header_plus_frames_of_the_first_size() {
        let sample = SampleSize { bytes: 1100, frames: vec![(0.0, 1000)] };
        assert_eq!(model(&sample, 3, 1), 1100);
        assert_eq!(model(&sample, 3, 90), 1100, "gifsicle merged a static sample to one frame");
    }

    #[test]
    fn one_frame_per_window_uses_the_later_frames() {
        // 1 fps: each 1 s window holds one frame, so every later frame starts a window.
        let sample =
            SampleSize { bytes: 100 + 1000 + 600 + 400, frames: vec![(0.0, 1000), (1.0, 600), (2.0, 400)] };
        assert_eq!(model(&sample, 3, 10), 100 + 1000 + 9 * 500);
    }
}
