//! The desktop's before/after still (US5, contracts/tauri-commands.md › `upscale_preview`).
//!
//! One ffmpeg run, input-seeked to the requested time, gives two PNGs at the preset size:
//! "after" is the job's own processing at that frame; "before" has the same framing and colours
//! scaled with plain bicubic (the baseline the quality bar is measured against). Smart reframe
//! uses the cached crop path when there is one; the preview never starts an analysis.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use super::graph::GraphInput;
use super::reframe::{self, ReframeAnalysis};
use super::{AnalysisState, Reframe, UpscaleOptions, UpscaleSpec, graph};
use crate::error::{Error, Result};
use crate::exec::commit::remove_with_retry;
use crate::exec::process::run_capture;
use crate::plan::{self, Caps, JobSpec};
use crate::preset::PresetRegistry;
use crate::probe::{INPUT_PROTOCOLS, file_arg};
use crate::request::{BatchRequest, ClipRequest, OnExists, Options, Origin, PartialOptions, SourceRequest};
use crate::time::{EndSpec, Ts};
use crate::tools::Tools;

/// Stills kept in the preview folder (two per preview); older ones are removed.
const KEEP_FILES: usize = 48;
/// A temporary still this old belongs to a run that was killed.
const STALE_PART: std::time::Duration = std::time::Duration::from_secs(600);

/// What to preview: a source (or a range of it), a time in it and the upscale options.
#[derive(Debug, Clone)]
pub struct PreviewRequest {
    pub path: PathBuf,
    /// The clip range (start, end); `None` = the whole video.
    pub span: Option<(Ts, EndSpec)>,
    /// Source time (the user time base), inside the range.
    pub at: Ts,
    pub options: UpscaleOptions,
}

/// The two stills and how they were made.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewPair {
    pub before: PathBuf,
    pub after: PathBuf,
    pub size: (u32, u32),
    /// Not exactly what the render will show: smart reframe before its analysis (centred
    /// crop), or HDR kept (the still is shown as SDR).
    pub approximate: bool,
    /// The crop's x in displayed source pixels, when the still is cropped.
    pub crop_x: Option<u32>,
    /// The time lies in a range shown over the blurred background.
    pub in_fallback: bool,
    /// The AI tier: the "after" still is the classical CPU scaler standing in (running the
    /// worker for a still would not fit the 5 s budget on most GPUs); the render is sharper.
    pub classical: bool,
}

/// Plans one upscale job for `path` (the whole video, or `span`), with the planner's checks,
/// geometry and tier choice. Planning errors are returned as `InvalidInput`.
pub async fn single_job(
    tools: &Tools,
    caps: &Caps,
    presets: PresetRegistry,
    path: &Path,
    span: Option<(Ts, EndSpec)>,
    options: &UpscaleOptions,
    cancel: &CancellationToken,
) -> Result<UpscaleSpec> {
    Ok(planned_job(tools, caps, presets, path, span, options, cancel).await?.0)
}

/// [`single_job`] with the job's warnings (e.g. no face model, so smart reframe is blurred).
pub async fn planned_job(
    tools: &Tools,
    caps: &Caps,
    presets: PresetRegistry,
    path: &Path,
    span: Option<(Ts, EndSpec)>,
    options: &UpscaleOptions,
    cancel: &CancellationToken,
) -> Result<(UpscaleSpec, Vec<String>)> {
    let (start, end) = span.unwrap_or((Ts::ZERO, EndSpec::EndOfMedia));
    let req = BatchRequest {
        // Nothing is written: an existing output must not make the plan skip the job.
        defaults: Options {
            upscale: Some(options.clone()),
            on_exists: OnExists::Overwrite,
            ..Options::default()
        },
        sources: vec![SourceRequest {
            path: path.to_path_buf(),
            clips: vec![ClipRequest {
                start,
                end,
                name: None,
                overrides: PartialOptions::default(),
                origin: Origin::Gui,
            }],
            overrides: PartialOptions::default(),
            origin: Origin::Gui,
        }],
    };
    let mut p = plan::plan_with(&req, tools, caps, presets, cancel).await;
    let errors: Vec<String> =
        p.issues.iter().filter(|i| i.severity == plan::Severity::Error).map(|i| i.message.clone()).collect();
    if !errors.is_empty() {
        return Err(Error::InvalidInput(errors.join("; ")));
    }
    match p.jobs.pop() {
        Some(job) => match job.spec {
            JobSpec::Upscale(u) => Ok((*u, job.warnings)),
            _ => Err(Error::InvalidInput("nothing to upscale".into())),
        },
        None => Err(Error::InvalidInput("nothing to upscale".into())),
    }
}

/// The before/after stills of `req` in `out_dir` (created if needed; the newest stills are
/// kept). A request already rendered returns the same files without running ffmpeg. If the
/// GPU tier fails here, the stills are made once more on the CPU and flagged approximate (the
/// render would fall back the same way).
pub async fn preview_frame(
    tools: &Tools,
    caps: &Caps,
    presets: PresetRegistry,
    req: &PreviewRequest,
    out_dir: &Path,
    cancel: &CancellationToken,
) -> Result<PreviewPair> {
    let spec = single_job(tools, caps, presets, &req.path, req.span, &req.options, cancel).await?;
    let span = spec.cut.requested;
    if req.at < span.start || req.at >= span.end {
        return Err(Error::InvalidInput(format!(
            "{} is outside the range {}–{}",
            req.at, span.start, span.end
        )));
    }
    let up = &spec.up;
    let analysis = match &up.analysis {
        AnalysisState::Ready(a, _) => Some(a.clone()),
        AnalysisState::Pending => reframe::lookup_cached(tools, &spec).await.unwrap_or_else(|e| {
            tracing::warn!("reframe cache lookup failed: {e}");
            None
        }),
        AnalysisState::NotNeeded => None,
    };
    let smart = up.reframe_applied == Reframe::Smart;
    let (x, in_fallback) = match &analysis {
        Some(a) if smart => frame_crop(a, req.at),
        _ => (None, false),
    };
    let still = graph::still_input(&up.graph, x, in_fallback);
    let pair = PreviewPair {
        before: PathBuf::new(),
        after: PathBuf::new(),
        size: up.target,
        approximate: (smart && analysis.is_none()) || up.graph.keep_hdr,
        crop_x: (!in_fallback).then(|| still.geometry.crop.map(|c| c.x)).flatten(),
        in_fallback,
        classical: up.tier.tier == super::tier::Tier::Ai,
    };
    let source = crate::paths::canonicalize(&req.path)?;
    let still_req = Still { tools, source: &source, at: req.at, out_dir, cancel };
    match still_req.render(&still, pair.clone()).await {
        Err(Error::EngineFailed { message, .. }) if still.gpu.is_some() && !cancel.is_cancelled() => {
            tracing::warn!("GPU preview failed ({message}); using the CPU");
            let cpu = GraphInput { gpu: None, ..still };
            still_req.render(&cpu, PreviewPair { approximate: true, ..pair }).await
        }
        other => other,
    }
}

/// One still pair to render: the source, the time and where the files go.
struct Still<'a> {
    tools: &'a Tools,
    source: &'a Path,
    at: Ts,
    out_dir: &'a Path,
    cancel: &'a CancellationToken,
}

impl Still<'_> {
    /// Renders (or finds) the pair for this graph input; files are named by everything that
    /// changes their pixels.
    async fn render(&self, still: &GraphInput, mut pair: PreviewPair) -> Result<PreviewPair> {
        let filter = graph::preview_pair(still);
        let meta = std::fs::metadata(self.source)?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        let mut h = Sha256::new();
        for part in [
            self.source.to_string_lossy().as_bytes(),
            &meta.len().to_le_bytes(),
            &mtime.to_le_bytes(),
            &self.at.as_micros().to_le_bytes(),
            filter.as_bytes(),
            self.tools.version.text().as_bytes(),
        ] {
            h.update((part.len() as u64).to_le_bytes());
            h.update(part);
        }
        let key = crate::hex(&h.finalize()[..12]);
        std::fs::create_dir_all(self.out_dir)?;
        pair.before = self.out_dir.join(format!("{key}-before.png"));
        pair.after = self.out_dir.join(format!("{key}-after.png"));
        if pair.before.is_file() && pair.after.is_file() {
            // Recently shown stills are the last to be evicted.
            for f in [&pair.before, &pair.after] {
                let _ = std::fs::File::options()
                    .append(true)
                    .open(f)
                    .and_then(|file| file.set_modified(std::time::SystemTime::now()));
            }
            return Ok(pair);
        }

        let probe = crate::probe::probe(self.tools, self.source, self.cancel).await?;
        let seek = self.at + crate::preview::time_offset(&probe);
        let unique = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
        );
        let part = |p: &Path| {
            p.with_file_name(format!(".{}.{unique}.part", p.file_name().unwrap().to_string_lossy()))
        };
        let (before_part, after_part) = (part(&pair.before), part(&pair.after));
        let mut args: Vec<OsString> =
            ["-hide_banner", "-nostdin", "-n", "-loglevel", "error"].map(OsString::from).to_vec();
        if let Some(device) = still.gpu {
            args.extend(
                ["-init_hw_device", &format!("vulkan=vk:{device}"), "-filter_hw_device", "vk"]
                    .map(OsString::from),
            );
        }
        args.extend(["-protocol_whitelist", INPUT_PROTOCOLS, "-ss"].map(OsString::from));
        args.push(seek.ffmpeg().into());
        args.push("-i".into());
        args.push(file_arg(self.source));
        args.extend(["-filter_complex", filter.as_str()].map(OsString::from));
        for (label, file) in [("[after]", &after_part), ("[before]", &before_part)] {
            args.extend(
                ["-map", label, "-frames:v", "1", "-c:v", "png", "-f", "image2", "-update", "1"]
                    .map(OsString::from),
            );
            args.push(file_arg(file));
        }
        let out = run_capture(&self.tools.ffmpeg.path, &args, self.cancel).await;
        let written = out.and_then(|o| {
            let ok = o.status.success()
                && [&before_part, &after_part]
                    .iter()
                    .all(|f| std::fs::metadata(f).is_ok_and(|m| m.len() > 0));
            if ok {
                Ok(())
            } else {
                Err(Error::engine_failed(
                    format!("could not render the preview at {}", self.at),
                    o.stderr_tail,
                ))
            }
        });
        let committed = written.and_then(|()| {
            std::fs::rename(&before_part, &pair.before)?;
            std::fs::rename(&after_part, &pair.after)?;
            Ok(())
        });
        for f in [&before_part, &after_part] {
            if f.exists() {
                remove_with_retry(f);
            }
        }
        committed?;
        evict(self.out_dir, &[&pair.before, &pair.after]);
        Ok(pair)
    }
}

/// The crop x at source time `at` from an analysis, or that `at` is in a fallback range.
fn frame_crop(a: &Arc<ReframeAnalysis>, at: Ts) -> (Option<u32>, bool) {
    if a.fallback_ranges.iter().any(|r| r.start <= at && at < r.end) {
        return (None, true);
    }
    let k = ((at.saturating_sub(a.clip_start)).as_secs_f64() * a.fps).round() as usize;
    let x = a.path.get(k.min(a.path.len().saturating_sub(1))).map(|x| (*x).max(0) as u32);
    (x, false)
}

/// Removes the least recently used stills beyond [`KEEP_FILES`] (never `keep`), and temporary
/// files left by a run that was killed (older than [`STALE_PART`]).
fn evict(dir: &Path, keep: &[&PathBuf]) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let now = std::time::SystemTime::now();
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        let Some(modified) = e.metadata().ok().filter(|m| m.is_file()).and_then(|m| m.modified().ok()) else {
            continue;
        };
        match p.extension().and_then(|x| x.to_str()) {
            Some("png") => files.push((modified, p)),
            Some("part") if now.duration_since(modified).is_ok_and(|age| age > STALE_PART) => {
                remove_with_retry(&p);
            }
            _ => {}
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, p) in files.into_iter().skip(KEEP_FILES) {
        if !keep.contains(&&p) {
            remove_with_retry(&p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Span;

    fn analysis(fallback: Vec<Span>) -> Arc<ReframeAnalysis> {
        Arc::new(ReframeAnalysis {
            key: String::new(),
            algo_version: 1,
            source_size: (1280, 720),
            crop: (332, 720),
            crop_y: 0,
            fps: 10.0,
            clip_start: Ts::from_secs_f64(2.0),
            shots: Vec::new(),
            fallback_ranges: fallback,
            path: (0..30).map(|k| k * 10).collect(),
            detections: 0,
            samples: 0,
        })
    }

    #[test]
    fn eviction_keeps_the_newest_stills_and_sweeps_stale_parts() {
        let d = tempfile::tempdir().unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        let mut paths = Vec::new();
        for i in 0..KEEP_FILES + 3 {
            let p = d.path().join(format!("{i:03}.png"));
            std::fs::write(&p, b"x").unwrap();
            // 000 is the oldest.
            let t = old + std::time::Duration::from_secs(i as u64);
            std::fs::File::options().append(true).open(&p).unwrap().set_modified(t).unwrap();
            paths.push(p);
        }
        let stale = d.path().join(".a.png.1-2.part");
        let fresh = d.path().join(".b.png.1-3.part");
        for p in [&stale, &fresh] {
            std::fs::write(p, b"x").unwrap();
        }
        std::fs::File::options().append(true).open(&stale).unwrap().set_modified(old).unwrap();
        evict(d.path(), &[&paths[0]]);
        assert!(paths[0].exists(), "kept even though it is the oldest");
        assert!(!paths[1].exists() && !paths[2].exists(), "the oldest beyond the newest 48 go");
        assert!(paths[3..].iter().all(|p| p.exists()));
        assert!(!stale.exists() && fresh.exists(), "only stale temporary files are removed");
    }

    #[test]
    fn frame_crop_reads_the_path_at_clip_time() {
        let s = Ts::from_secs_f64;
        let a = analysis(vec![Span { start: s(4.0), end: s(4.5) }]);
        assert_eq!(frame_crop(&a, s(2.0)), (Some(0), false));
        assert_eq!(frame_crop(&a, s(3.04)), (Some(100), false), "frame 10 of the clip");
        assert_eq!(frame_crop(&a, s(4.2)), (None, true));
        assert_eq!(frame_crop(&a, s(99.0)), (Some(290), false), "clamped to the last frame");
        assert_eq!(frame_crop(&analysis(Vec::new()), s(1.0)), (Some(0), false));
    }
}
