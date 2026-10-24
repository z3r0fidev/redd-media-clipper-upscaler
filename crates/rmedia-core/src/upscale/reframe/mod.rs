//! Smart reframe analysis (research R6): cuts, detection, subject selection, crop path.

pub mod cache;
pub mod cuts;
pub mod decode;
pub mod detect;
pub mod path;
pub mod select;
pub mod sendcmd;

use serde::{Deserialize, Serialize};

use crate::event::Span;
use crate::time::Ts;
use crate::upscale::Motion;
use detect::FaceBox;

/// Bump when the analysis changes, so cached results are recomputed.
pub const ALGO_VERSION: u32 = 1;
/// Samples per second analysed.
pub const SAMPLE_FPS: f64 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ShotKind {
    /// The crop follows a subject for the whole shot.
    Tracked,
    /// No subject fits anywhere in the shot: blurred background throughout.
    Fallback,
    /// Partly tracked, partly fallback.
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Shot {
    pub span: Span,
    pub kind: ShotKind,
}

/// The result of analysing one job's source range. Times are source times; `path` has one
/// crop x per output frame of the clip (frame k at `k / fps` after the clip start).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ReframeAnalysis {
    pub key: String,
    pub algo_version: u32,
    /// Displayed source size the crop moves over.
    pub source_size: (u32, u32),
    /// Crop window size (source pixels).
    pub crop: (u32, u32),
    pub crop_y: u32,
    pub fps: f64,
    pub clip_start: Ts,
    pub shots: Vec<Shot>,
    /// Ranges shown over the blurred background (FR-014), ascending and disjoint.
    pub fallback_ranges: Vec<Span>,
    pub path: Vec<i32>,
    pub detections: u32,
    pub samples: u32,
}

impl ReframeAnalysis {
    /// Clip length covered by the path.
    pub fn len_secs(&self) -> f64 {
        self.path.len() as f64 / self.fps
    }

    /// Structural checks for cached entries: version, even x inside the frame, ranges inside
    /// the clip.
    pub fn is_valid(&self) -> bool {
        let max_x = i64::from(self.source_size.0) - i64::from(self.crop.0);
        let end = self.clip_start.as_secs_f64() + self.len_secs() + 1.0 / self.fps.max(1.0);
        self.algo_version == ALGO_VERSION
            && self.fps.is_finite()
            && self.fps >= 1.0
            && self.crop_y + self.crop.1 <= self.source_size.1
            && !self.path.is_empty()
            && max_x >= 0
            && self.crop.1 <= self.source_size.1
            && self.path.iter().all(|x| x % 2 == 0 && i64::from(*x) >= 0 && i64::from(*x) <= max_x)
            && self.fallback_ranges.windows(2).all(|w| w[0].end <= w[1].start)
            && self
                .fallback_ranges
                .iter()
                .all(|r| r.start >= self.clip_start && r.start < r.end && r.end.as_secs_f64() <= end + 1e-6)
    }
}

/// Frames in a clip of `len` seconds at `fps`.
pub fn frame_count(len: f64, fps: f64) -> usize {
    (len * fps - 1e-9).ceil().max(0.0) as usize
}

/// One analysed sample: its clip time, detections (analysis pixels) and histogram.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleResult {
    pub t: f64,
    pub faces: Vec<FaceBox>,
    pub hist: [u32; 48],
}

/// What [`assemble`] needs besides the samples.
#[derive(Debug, Clone, PartialEq)]
pub struct AssembleParams {
    pub analysis_size: (u32, u32),
    pub display: (u32, u32),
    pub crop: (u32, u32),
    pub crop_y: u32,
    pub fps: f64,
    pub len: f64,
    pub clip_start: Ts,
    pub motion: Motion,
}

/// Turns samples and scdet scores into shots, fallback ranges and the per-frame crop path.
pub fn assemble(
    samples: &[SampleResult],
    scores: &[(f64, f64)],
    p: &AssembleParams,
    key: String,
) -> ReframeAnalysis {
    let scale = f64::from(p.display.0) / f64::from(p.analysis_size.0.max(1));
    let crop_w = f64::from(p.crop.0);
    let interval = 1.0 / SAMPLE_FPS;
    let subjects: Vec<(f64, select::Subject)> = samples
        .iter()
        .map(|s| {
            let faces: Vec<FaceBox> = s
                .faces
                .iter()
                .map(|f| FaceBox {
                    x: (f64::from(f.x) * scale) as f32,
                    y: (f64::from(f.y) * scale) as f32,
                    w: (f64::from(f.w) * scale) as f32,
                    h: (f64::from(f.h) * scale) as f32,
                    score: f.score,
                })
                .collect();
            (s.t, select::select(&faces, crop_w))
        })
        .collect();
    let hists: Vec<(f64, [u32; 48])> = samples.iter().map(|s| (s.t, s.hist)).collect();
    let shot_bounds = cuts::shots(&cuts::cuts(scores, &hists), p.len);
    let n = frame_count(p.len, p.fps);
    let lim = path::limits(p.motion, crop_w, p.fps);
    let frame_of = |t: f64| ((t * p.fps - 1e-9).ceil().max(0.0) as usize).min(n);
    let at = |t: f64| p.clip_start + Ts::from_secs_f64(t);
    let mut centres = vec![f64::from(p.display.0) / 2.0; n];
    let mut shots = Vec::new();
    let mut fallback: Vec<(f64, f64)> = Vec::new();
    for &(a, b) in &shot_bounds {
        let in_shot: Vec<(f64, select::Subject)> =
            subjects.iter().filter(|(t, _)| *t >= a && *t < b).copied().collect();
        let runs = select::fallback_runs(&in_shot, (a, b), interval);
        let held = select::hold(&in_shot, 1.0);
        let (fa, fb) = (frame_of(a), frame_of(b));
        let known: Vec<f64> = {
            // Long gaps keep the last position (hidden behind the blurred background).
            let mut last = held.iter().flatten().next().copied();
            held.iter()
                .map(|c| {
                    if c.is_some() {
                        last = *c;
                    }
                    last.unwrap_or(f64::from(p.display.0) / 2.0)
                })
                .collect()
        };
        if known.is_empty() && fa > 0 && fb > fa {
            // A shot too short to be sampled (a flash frame, a rapid cut): hold the position.
            let last = centres[fa - 1];
            centres[fa..fb].fill(last);
        }
        if !known.is_empty() && fb > fa {
            let smooth = path::deadzone(&path::median5(&known), 0.06 * crop_w);
            let pairs: Vec<(f64, f64)> = in_shot.iter().map(|(t, _)| *t).zip(smooth).collect();
            let targets = path::interpolate(&pairs, p.fps, fa, fb - fa);
            centres[fa..fb].copy_from_slice(&path::follow(&targets, lim));
        }
        let kind = match runs.as_slice() {
            [] => ShotKind::Tracked,
            [(s, e)] if (s - a).abs() < 1e-9 && (e - b).abs() < 1e-9 => ShotKind::Fallback,
            _ => ShotKind::Mixed,
        };
        shots.push(Shot { span: Span { start: at(a), end: at(b) }, kind });
        for r in runs {
            match fallback.last_mut() {
                Some(last) if last.1 >= r.0 - 1e-9 => last.1 = last.1.max(r.1),
                _ => fallback.push(r),
            }
        }
    }
    ReframeAnalysis {
        key,
        algo_version: ALGO_VERSION,
        source_size: p.display,
        crop: p.crop,
        crop_y: p.crop_y,
        fps: p.fps,
        clip_start: p.clip_start,
        shots,
        fallback_ranges: fallback.into_iter().map(|(s, e)| Span { start: at(s), end: at(e) }).collect(),
        path: path::finalize(&centres, p.crop.0, p.display.0 - p.crop.0),
        detections: samples.iter().map(|s| s.faces.len() as u32).sum(),
        samples: samples.len() as u32,
    }
}

/// What the analysis reads from a job (refactor B2, research R16): upscale and GIF jobs each
/// build one, so both share the analysis and its cache.
#[derive(Debug, Clone, Copy)]
pub struct ReframeInput<'a> {
    /// The source range (`input_args`, the requested span, the input file).
    pub cut: &'a crate::plan::ffargs::ClipSpec,
    /// The pre-processing the analysis graph repeats (trim, deinterlace, tone-map).
    pub graph: &'a crate::upscale::graph::GraphInput,
    pub detector: Option<&'a detect::DetectorSpec>,
    pub cache_dir: Option<&'a std::path::Path>,
    pub analysis_size: (u32, u32),
    /// The output frame rate the crop path has one entry per frame of.
    pub fps_out: f64,
    /// The displayed source size and the centred crop window the path moves.
    pub display: (u32, u32),
    pub crop: Option<crate::upscale::geometry::Crop>,
    pub motion: Motion,
}

impl crate::upscale::UpscaleSpec {
    /// This job's analysis input.
    pub fn reframe_input(&self) -> ReframeInput<'_> {
        let up = &self.up;
        ReframeInput {
            cut: &self.cut,
            graph: &up.graph,
            detector: up.detector.as_ref(),
            cache_dir: up.reframe_cache.as_deref(),
            analysis_size: up.analysis_size,
            fps_out: up.fps_out,
            display: up.geometry.display,
            crop: up.geometry.crop,
            motion: up.motion,
        }
    }
}

/// What an analysis of one job is keyed and checked by.
struct Prepared {
    detector: std::sync::Arc<dyn detect::Detector>,
    key: String,
    cache: Option<cache::Cache>,
    graph: String,
    crop: crate::upscale::geometry::Crop,
    len: f64,
    frames: usize,
}

impl Prepared {
    async fn new(tools: &crate::tools::Tools, input: &ReframeInput<'_>) -> crate::Result<Prepared> {
        use crate::Error;
        let not_smart = || Error::InvalidInput("this job has no smart reframe to analyse".into());
        let spec_det = input.detector.cloned().ok_or_else(not_smart)?;
        // Reading and hashing the model is blocking work.
        let detector = tokio::task::spawn_blocking(move || detect::load(&spec_det))
            .await
            .map_err(|e| Error::engine_failed(format!("face model: {e}"), String::new()))??;
        let crop = input.crop.ok_or_else(not_smart)?;
        let span = input.cut.requested;
        let len = span.end.saturating_sub(span.start).as_secs_f64();
        let graph = crate::upscale::graph::analysis_graph(input.graph, input.analysis_size);
        let meta = std::fs::metadata(&input.cut.input)?;
        let mtime_ns = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos() as i128);
        let source = std::fs::canonicalize(&input.cut.input).unwrap_or_else(|_| input.cut.input.clone());
        let key = cache::key(&cache::KeyInputs {
            source: source.to_string_lossy().into_owned(),
            size: meta.len(),
            mtime_ns,
            span: (span.start.as_secs_f64(), span.end.as_secs_f64()),
            chain: graph.clone(),
            analysis_size: input.analysis_size,
            crop: (crop.w, crop.h),
            fps: input.fps_out,
            motion: input.motion,
            algo: ALGO_VERSION,
            detector: detector.id(),
            ffmpeg: tools.version.text().to_owned(),
        });
        Ok(Prepared {
            detector,
            key,
            cache: input.cache_dir.map(|d| cache::Cache::new(d.to_path_buf())),
            graph,
            crop,
            len,
            frames: frame_count(len, input.fps_out),
        })
    }

    fn cached(&self, input: &ReframeInput<'_>) -> Option<ReframeAnalysis> {
        let fits = |a: &ReframeAnalysis| {
            a.path.len() == self.frames
                && a.source_size == input.display
                && a.crop == (self.crop.w, self.crop.h)
                && a.crop_y == self.crop.y
        };
        self.cache.as_ref().and_then(|c| c.get(&self.key, fits))
    }
}

/// The cache key of a job's analysis (tests check it is stable across refactors).
pub async fn cache_key(tools: &crate::tools::Tools, input: &ReframeInput<'_>) -> crate::Result<String> {
    Ok(Prepared::new(tools, input).await?.key)
}

/// The cached analysis of a job, if there is one; never decodes (the desktop preview).
pub async fn lookup_cached(
    tools: &crate::tools::Tools,
    input: &ReframeInput<'_>,
) -> crate::Result<Option<std::sync::Arc<ReframeAnalysis>>> {
    let p = Prepared::new(tools, input).await?;
    Ok(p.cached(input).map(std::sync::Arc::new))
}

/// Analyses a job's source range, or takes the result from the cache (research R10).
/// `progress` receives the fraction of samples done.
pub async fn analyze_cached(
    tools: &crate::tools::Tools,
    input: &ReframeInput<'_>,
    cancel: &tokio_util::sync::CancellationToken,
    mut progress: impl FnMut(f64) + Send,
) -> crate::Result<(std::sync::Arc<ReframeAnalysis>, crate::upscale::CacheUse)> {
    use crate::upscale::CacheUse;
    let p = Prepared::new(tools, input).await?;
    if let Some(a) = p.cached(input) {
        progress(1.0);
        return Ok((std::sync::Arc::new(a), CacheUse::Hit));
    }
    let work = decode::WorkDir::new(input.cache_dir)?;
    let expected = (p.len * SAMPLE_FPS).ceil().max(1.0);
    let (samples, scores) = decode::run(
        tools,
        crate::plan::ffargs::input_args(input.cut),
        &p.graph,
        input.analysis_size,
        p.detector.clone(),
        work.path(),
        cancel,
        |n| progress((n as f64 / expected).min(1.0)),
    )
    .await?;
    let params = AssembleParams {
        analysis_size: input.analysis_size,
        display: input.display,
        crop: (p.crop.w, p.crop.h),
        crop_y: p.crop.y,
        fps: input.fps_out,
        len: p.len,
        clip_start: input.cut.requested.start,
        motion: input.motion,
    };
    let a = assemble(&samples, &scores, &params, p.key.clone());
    if let Some(c) = &p.cache
        && let Err(e) = c.put(&a)
    {
        tracing::warn!("could not cache the reframe analysis: {e}");
    }
    Ok((std::sync::Arc::new(a), CacheUse::Miss))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> AssembleParams {
        AssembleParams {
            analysis_size: (640, 360),
            display: (1280, 720),
            crop: (332, 720),
            crop_y: 0,
            fps: 30.0,
            len: 10.0,
            clip_start: Ts::ZERO,
            motion: Motion::Normal,
        }
    }

    /// A marker moving right at 75 analysis px/s (150 source px/s) for 6 s, then a cut and a
    /// still marker at x = 500 (1000 source px), like the MovingMarker fixture.
    fn samples() -> Vec<SampleResult> {
        (0..30)
            .map(|i| {
                let t = f64::from(i) / SAMPLE_FPS;
                let (cx, level) = if t < 6.0 { (70.0 + 75.0 * t, 60) } else { (520.0, 200) };
                let b = FaceBox { x: cx as f32 - 20.0, y: 160.0, w: 40.0, h: 40.0, score: 1.0 };
                SampleResult { t, faces: vec![b], hist: cuts::histogram(&vec![level; 300]) }
            })
            .collect()
    }

    #[test]
    fn moving_subject_with_a_cut() {
        let scores = vec![(6.0, 12.5)];
        let a = assemble(&samples(), &scores, &params(), "k".into());
        assert_eq!(a.path.len(), 300);
        assert_eq!(a.shots.len(), 2);
        assert!((a.shots[1].span.start.as_secs_f64() - 6.0).abs() < 1e-6);
        assert!(a.fallback_ranges.is_empty());
        assert!(a.is_valid());
        let lim = path::limits(Motion::Normal, 332.0, 30.0);
        for k in 1..300 {
            let step = f64::from(a.path[k] - a.path[k - 1]).abs();
            if k != 180 {
                assert!(step <= lim.speed + 2.0, "frame {k}: step {step}");
            }
        }
        // After the cut the crop sits on the still marker (centre 1040 → x 874).
        assert_eq!(a.path[299], 874);
        // While tracking, the marker centre stays inside the crop.
        for k in (0..180).step_by(15) {
            let marker = 2.0 * (70.0 + 75.0 * k as f64 / 30.0);
            let x = f64::from(a.path[k]);
            assert!(marker > x + 40.0 && marker < x + 332.0 - 40.0, "frame {k}: marker {marker} crop {x}");
        }
    }

    #[test]
    fn a_shot_too_short_to_sample_holds_the_position_without_falling_back() {
        let mut s = samples();
        // A false cut at 3.1 s and a real one at 6.0 s: the 3.1–3.2 s shot has no sample.
        s.retain(|x| x.t < 6.0);
        let mut p = params();
        p.len = 6.0;
        let a = assemble(&s, &[(3.1, 12.0), (3.2, 12.0)], &p, "k".into());
        assert_eq!(a.shots.len(), 3);
        assert!(a.fallback_ranges.is_empty(), "{:?}", a.fallback_ranges);
        let k = (3.1 * 30.0_f64).ceil() as usize;
        assert_eq!(a.path[k], a.path[k - 1], "no jump into the unsampled shot");
    }

    #[test]
    fn frame_counts_and_validity() {
        assert_eq!(frame_count(10.0, 30.0), 300);
        assert_eq!(frame_count(10.01, 30.0), 301);
        assert_eq!(frame_count(1.0, 29.97), 30);
        let mut a = assemble(&samples(), &[(6.0, 12.5)], &params(), "k".into());
        a.path[3] = 949;
        assert!(!a.is_valid(), "odd and out of range");
    }
}
