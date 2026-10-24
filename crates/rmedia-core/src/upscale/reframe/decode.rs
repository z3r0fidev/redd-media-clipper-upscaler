//! Analysis decode: one ffmpeg process producing low-resolution frames and cut scores
//! (research R6).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use super::detect::{Detector, FrameView};
use super::{SAMPLE_FPS, SampleResult, cuts};
use crate::error::{Error, Result};
use crate::exec::process;
use crate::tools::Tools;

/// Long side of the analysis frames (research/09 §4: YuNet at 576 px is about 30 ms a frame).
pub const ANALYSIS_LONG_SIDE: u32 = 576;

/// Analysis frame size for a displayed source size: long side 576, aspect kept, even.
pub fn analysis_size(display: (u32, u32)) -> (u32, u32) {
    let even = |v: f64| ((v / 2.0).round().max(1.0) as u32) * 2;
    let (w, h) = (f64::from(display.0.max(1)), f64::from(display.1.max(1)));
    let long = f64::from(ANALYSIS_LONG_SIDE);
    if w >= h { (ANALYSIS_LONG_SIDE, even(long * h / w)) } else { (even(long * w / h), ANALYSIS_LONG_SIDE) }
}

/// `size` rounded up to multiples of 32 (YuNet's largest stride); the extra is zero padding.
pub fn padded(size: (u32, u32)) -> (u32, u32) {
    (size.0.div_ceil(32) * 32, size.1.div_ceil(32) * 32)
}

/// A private working directory for one analysis (holds `cuts.txt`), removed on drop.
#[derive(Debug)]
pub struct WorkDir(PathBuf);

impl WorkDir {
    /// Creates `<base>/tmp/<unique>` (or under the system temp folder without a cache).
    pub fn new(base: Option<&Path>) -> Result<WorkDir> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let root = base.map_or_else(std::env::temp_dir, |b| b.join("tmp"));
        let mut b = std::fs::DirBuilder::new();
        b.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        b.create(&root)?;
        sweep_stale(&root);
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = root.join(format!(
            "rmedia-analysis-{}-{nanos}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        crate::exec::commit::create_scratch(&dir)?;
        Ok(WorkDir(dir))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

/// Removes analysis work folders left by killed runs (older than a day).
fn sweep_stale(root: &Path) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let day = std::time::Duration::from_secs(24 * 3600);
    for e in rd.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > day);
        if old && e.file_name().to_string_lossy().starts_with("rmedia-analysis-") {
            crate::exec::commit::remove_with_retry(&e.path());
        }
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        crate::exec::commit::remove_with_retry(&self.0);
    }
}

/// Runs the analysis decode: `input` (the clip's input arguments) through `graph`
/// (`graph::analysis_graph`), detecting on every 3 fps frame with up to four threads while
/// ffmpeg keeps decoding. Returns the samples in order and the scdet scores. `progress` gets the
/// number of samples done.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    tools: &Tools,
    input: Vec<OsString>,
    graph: &str,
    size: (u32, u32),
    detector: Arc<dyn Detector>,
    work: &Path,
    cancel: &CancellationToken,
    mut progress: impl FnMut(usize) + Send,
) -> Result<(Vec<SampleResult>, Vec<(f64, f64)>)> {
    let mut args: Vec<OsString> =
        ["-hide_banner", "-nostdin", "-loglevel", "error"].iter().map(OsString::from).collect();
    args.extend(input);
    args.extend(
        ["-filter_complex", graph, "-map", "[raw]", "-an", "-sn", "-f", "rawvideo", "pipe:1"]
            .iter()
            .map(OsString::from),
    );
    let frame_len = size.0 as usize * size.1 as usize * 3;
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(4);
    let mut samples: Vec<SampleResult> = Vec::new();
    let consume = |mut stdout: tokio::process::ChildStdout| {
        let samples = &mut samples;
        let progress = &mut progress;
        async move {
            type Batch = std::io::Result<Vec<(Vec<super::detect::FaceBox>, [u32; 48])>>;
            // Detection of one batch runs while the next is read, so ffmpeg keeps decoding.
            let mut pending: Option<tokio::task::JoinHandle<Batch>> = None;
            let mut collect = |done: Vec<(Vec<super::detect::FaceBox>, [u32; 48])>,
                               samples: &mut Vec<SampleResult>| {
                let first = samples.len();
                for (i, (faces, hist)) in done.into_iter().enumerate() {
                    samples.push(SampleResult { t: (first + i) as f64 / SAMPLE_FPS, faces, hist });
                }
                progress(samples.len());
            };
            let mut eof = false;
            while !eof {
                let mut batch = Vec::with_capacity(workers * 2);
                while batch.len() < workers * 2 {
                    let mut frame = vec![0u8; frame_len];
                    match stdout.read_exact(&mut frame).await {
                        Ok(_) => batch.push(frame),
                        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                            eof = true;
                            break;
                        }
                        Err(e) => return Err(e),
                    }
                }
                if let Some(job) = pending.take() {
                    collect(job.await.map_err(std::io::Error::other)??, samples);
                }
                if batch.is_empty() {
                    break;
                }
                let det = detector.clone();
                pending = Some(tokio::task::spawn_blocking(move || {
                    detect_batch(det.as_ref(), batch, size, workers)
                }));
            }
            if let Some(job) = pending.take() {
                collect(job.await.map_err(std::io::Error::other)??, samples);
            }
            Ok(())
        }
    };
    let out = process::run_stream(&tools.ffmpeg.path, &args, Some(work), cancel, consume).await?;
    if !out.status.success() {
        return Err(Error::engine_failed("the reframe analysis decode failed", out.stderr_tail));
    }
    let scores =
        std::fs::read_to_string(work.join("cuts.txt")).map(|t| cuts::parse_scdet(&t)).unwrap_or_default();
    Ok((samples, scores))
}

/// Histogram and detections for a batch of frames, on up to `workers` threads, in order.
fn detect_batch(
    det: &dyn Detector,
    frames: Vec<Vec<u8>>,
    size: (u32, u32),
    workers: usize,
) -> std::io::Result<Vec<(Vec<super::detect::FaceBox>, [u32; 48])>> {
    let one = |bgr: &Vec<u8>| -> std::io::Result<(Vec<super::detect::FaceBox>, [u32; 48])> {
        let faces = det
            .detect(&FrameView { bgr, width: size.0, height: size.1 })
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok((faces, cuts::histogram(bgr)))
    };
    let chunk = frames.len().div_ceil(workers.max(1)).max(1);
    std::thread::scope(|s| {
        let handles: Vec<_> =
            frames.chunks(chunk).map(|c| s.spawn(move || c.iter().map(one).collect::<Vec<_>>())).collect();
        handles.into_iter().flat_map(|h| h.join().expect("detector thread panicked")).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(analysis_size((1920, 1080)), (576, 324));
        assert_eq!(analysis_size((1280, 720)), (576, 324));
        assert_eq!(analysis_size((1080, 1920)), (324, 576));
        assert_eq!(analysis_size((1024, 576)), (576, 324));
        assert_eq!(analysis_size((320, 240)), (576, 432), "small sources are scaled up for detection");
        assert_eq!(padded((576, 324)), (576, 352));
        assert_eq!(padded((324, 576)), (352, 576));
    }
}
