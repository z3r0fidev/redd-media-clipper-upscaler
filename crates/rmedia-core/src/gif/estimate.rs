//! Sample encodes and the size model (research R4). The estimate shown before encoding comes
//! from one ffmpeg run over three 1 s windows of the range (or the whole range, when it is
//! short), optimised as the job would be, then scaled up from its first frame and the mean of
//! its delta frames. Samples never run in the planner: [`prepare`] runs them after planning, in
//! a temporary folder of its own.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use super::graph::{gifsicle_args, palette_settings};
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
/// Ranges up to this long are encoded whole: the sample is the output itself.
pub const WHOLE_UP_TO: f64 = 4.0;
/// Where the windows are centred, as fractions of the range.
const CENTRES: [f64; 3] = [0.1, 0.5, 0.9];

const SAMPLE: &str = "sample.gif";
const SAMPLE_OPT: &str = "sample-opt.gif";

/// The source times the sample windows start at, for a range of `len` seconds from `start`;
/// `None` when the range is short enough to encode whole.
pub fn windows(start: Ts, len: f64) -> Option<[Ts; 3]> {
    (len > WHOLE_UP_TO).then(|| {
        CENTRES.map(|c| start + Ts::from_secs_f64((c * len - WINDOW / 2.0).clamp(0.0, len - WINDOW)))
    })
}

/// The frame chain of `s` reading from `label`, without the job's own trim (a sample seeks its
/// windows itself).
fn chain_from(spec: &GifSpec, s: &Settings, label: &str, trim: bool) -> String {
    let mut g = spec.frame.graph_at((s.width, s.height));
    if !trim {
        g.trim = None;
    }
    let chain = frame_chain(&g, &GifChain { rate: &s.rate_text(), sharpen: spec.frame.sharpen });
    let rest = chain.strip_prefix("[0:v]").expect("a frame chain reads the first input's video");
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

/// The sample encode of `s` into `output` (relative): each window is its own input with a fast
/// seek, the windows are joined with `concat`, then framed, scaled and paletted as one clip.
/// With smart framing (US4), crop commands are written on the joined timeline: window `k`
/// starts at `k` seconds.
pub fn sample_args(spec: &GifSpec, s: &Settings, starts: &[Ts], output: &str) -> Vec<OsString> {
    let mut a = common_args();
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
    let graph = joined + &one_pass(&chain_from(spec, s, "[cat]", false), s);
    a.extend(os(&["-filter_complex", &graph]));
    a.extend(gif_tail(spec, output));
    a
}

/// The whole range of a short job, in one run: the same frames as the job's own encode.
pub fn whole_args(spec: &GifSpec, s: &Settings, output: &str) -> Vec<OsString> {
    let (source, range) = input_parts(&spec.cut);
    let mut a = common_args();
    a.extend(source);
    a.extend(range);
    a.extend(os(&["-filter_complex", &one_pass(&chain_from(spec, s, "[0:v]", true), s)]));
    a.extend(gif_tail(spec, output));
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
        return header + first;
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

/// Encodes the sample of `s` in `dir` and measures it (optimised by gifsicle when the job is).
/// Returns the predicted size of the job's GIF.
pub async fn sample(
    tools: &Tools,
    spec: &GifSpec,
    s: &Settings,
    dir: &Path,
    cancel: &CancellationToken,
) -> Result<u64> {
    for f in [SAMPLE, SAMPLE_OPT] {
        let _ = std::fs::remove_file(dir.join(f));
    }
    let span = spec.cut.requested;
    let starts = windows(span.start, spec.gif.len);
    let args = match &starts {
        Some(w) => sample_args(spec, s, w, SAMPLE),
        None => whole_args(spec, s, SAMPLE),
    };
    let out = process::run_lines_in(&tools.ffmpeg.path, &args, Some(dir), cancel, |_| {}).await?;
    if !out.status.success() {
        return Err(Error::engine_failed("the sample encode failed", out.stderr_tail));
    }
    let mut measured = SAMPLE;
    if let Some(gifsicle) = spec.gif.optimiser.as_deref() {
        let env = super::engines::engine_env();
        let a = gifsicle_args(s.lossy, SAMPLE, SAMPLE_OPT);
        let out = process::run_lines_env(gifsicle, &a, Some(dir), Some(&env), cancel, |_| {}).await?;
        if !out.status.success() {
            return Err(Error::EngineFailed {
                tool: "gifsicle".into(),
                message: "the sample optimisation failed".into(),
                stderr_tail: out.stderr_tail,
            });
        }
        measured = SAMPLE_OPT;
    }
    let size = frame_sizes(tools, &dir.join(measured), cancel).await?;
    Ok(match starts {
        Some(w) => model(&size, w.len(), spec.gif.frames),
        None => size.bytes,
    })
}

/// The estimate with no budget search: one sample at the starting settings.
pub async fn predict(
    tools: &Tools,
    spec: &GifSpec,
    dir: &Path,
    cancel: &CancellationToken,
) -> Result<Estimate> {
    let s = spec.gif.start.clone();
    let bytes = sample(tools, spec, &s, dir, cancel).await?;
    Ok(Estimate {
        bytes: Some(bytes),
        from: EstimateFrom::Sample,
        settings: s,
        lowered: Vec::new(),
        mp4_bytes: None,
    })
}

/// A private temporary folder, removed with everything in it when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(parent: &Path) -> std::io::Result<TempDir> {
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let path = parent.join(format!("rmedia-sample-{}-{nanos}", std::process::id()));
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        b.create(&path)?;
        Ok(TempDir(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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
        if g.gif.engine == EngineUsed::Gifski {
            // gifski samples need its pipe (T043).
            continue;
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
    fn short_ranges_are_encoded_whole() {
        assert!(windows(Ts::ZERO, 4.0).is_none());
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
    }
}
