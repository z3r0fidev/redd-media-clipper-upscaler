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
        todo!()
    }
}

/// Frames in a clip of `len` seconds at `fps`.
pub fn frame_count(len: f64, fps: f64) -> usize {
    let _ = (len, fps);
    todo!()
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
    let _ = (samples, scores, p, key);
    todo!()
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
    fn frame_counts_and_validity() {
        assert_eq!(frame_count(10.0, 30.0), 300);
        assert_eq!(frame_count(10.01, 30.0), 301);
        assert_eq!(frame_count(1.0, 29.97), 30);
        let mut a = assemble(&samples(), &[(6.0, 12.5)], &params(), "k".into());
        a.path[3] = 949;
        assert!(!a.is_valid(), "odd and out of range");
    }
}
