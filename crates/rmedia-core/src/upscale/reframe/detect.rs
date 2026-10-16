//! Face detectors: the `Detector` trait, YuNet under tract, and a test marker detector
//! (research R6, R7).

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// A detection in frame pixels (top-left corner, size).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FaceBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub score: f32,
}

impl FaceBox {
    pub fn centre_x(&self) -> f32 {
        self.x + self.w / 2.0
    }
}

/// A packed bgr24 frame.
#[derive(Debug, Clone, Copy)]
pub struct FrameView<'a> {
    pub bgr: &'a [u8],
    pub width: u32,
    pub height: u32,
}

/// Finds faces (or test markers) in a frame. Implementations are shared across worker threads.
pub trait Detector: Send + Sync + std::fmt::Debug {
    /// Identity for the analysis cache key (model name and SHA-256).
    fn id(&self) -> String;
    fn detect(&self, frame: &FrameView<'_>) -> Result<Vec<FaceBox>>;
}

/// Which detector a job uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectorSpec {
    /// YuNet 2026may (`models::YUNET`) at this path.
    Yunet(PathBuf),
    /// Pure-magenta markers in synthetic test videos (research R7).
    #[cfg(any(test, feature = "test-support"))]
    Marker,
}

/// Loads (or reuses) the detector for `spec`.
pub fn load(spec: &DetectorSpec) -> Result<Arc<dyn Detector>> {
    let _ = spec;
    todo!()
}

/// YuNet score threshold and NMS overlap (research R6).
pub const SCORE_THRESHOLD: f32 = 0.6;
pub const NMS_IOU: f32 = 0.3;

/// Decodes YuNet's 12 outputs (cls, obj, bbox, kps × strides 8/16/32, in that order) for an
/// input of `input` size (multiples of 32). Boxes centred outside `valid` (the unpadded frame)
/// are dropped. Deterministic NMS: by score, then anchor order.
pub fn decode(outputs: &[&[f32]], input: (u32, u32), valid: (u32, u32)) -> Vec<FaceBox> {
    let _ = (outputs, input, valid);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic outputs for a 64x64 input: anchors per stride 64, 16, 4.
    fn outputs(set: &[(usize, usize, f32, f32, [f32; 4])]) -> Vec<Vec<f32>> {
        let n = [64usize, 16, 4];
        let mut cls: Vec<Vec<f32>> = n.iter().map(|k| vec![0.0; *k]).collect();
        let mut obj = cls.clone();
        let mut bbox: Vec<Vec<f32>> = n.iter().map(|k| vec![0.0; k * 4]).collect();
        let kps: Vec<Vec<f32>> = n.iter().map(|k| vec![0.0; k * 10]).collect();
        for (stride_i, anchor, c, o, b) in set {
            cls[*stride_i][*anchor] = *c;
            obj[*stride_i][*anchor] = *o;
            bbox[*stride_i][anchor * 4..anchor * 4 + 4].copy_from_slice(b);
        }
        cls.into_iter().chain(obj).chain(bbox).chain(kps).collect()
    }

    fn run(set: &[(usize, usize, f32, f32, [f32; 4])], valid: (u32, u32)) -> Vec<FaceBox> {
        let o = outputs(set);
        let refs: Vec<&[f32]> = o.iter().map(Vec::as_slice).collect();
        decode(&refs, (64, 64), valid)
    }

    #[test]
    fn decodes_boxes_with_threshold_and_padding() {
        let (ln2, ln4) = (2f32.ln(), 4f32.ln());
        // Stride 8, row 2, col 3 (anchor 19): centre (24, 16), 16x32.
        let kept = run(&[(0, 2 * 8 + 3, 0.8, 0.8, [0.0, 0.0, ln2, ln4])], (64, 64));
        assert_eq!(kept.len(), 1);
        let b = kept[0];
        assert!(
            (b.x - 16.0).abs() < 1e-4
                && b.y.abs() < 1e-4
                && (b.w - 16.0).abs() < 1e-4
                && (b.h - 32.0).abs() < 1e-4,
            "{b:?}"
        );
        assert!((b.score - 0.8).abs() < 1e-5, "score = sqrt(cls × obj)");
        assert!(run(&[(0, 19, 0.5, 0.5, [0.0; 4])], (64, 64)).is_empty(), "0.5 is below the threshold");
        // Stride 8, col 7 → centre x 56, outside a 48-wide valid frame (padding).
        assert!(run(&[(0, 7, 0.9, 0.9, [0.0, 0.0, ln2, ln2])], (48, 64)).is_empty());
    }

    #[test]
    fn nms_keeps_the_best_of_overlapping_boxes() {
        let (ln2, ln4) = (2f32.ln(), 4f32.ln());
        let set = [
            (0, 19, 0.7, 0.7, [0.0, 0.0, ln2, ln4]),
            (0, 20, 0.95, 0.95, [-0.5, 0.0, ln2, ln4]), // same face, 4 px to the left
            (2, 3, 0.9, 0.9, [0.0, 0.0, 0.0, 0.0]),     // stride 32, far away
        ];
        let boxes = run(&set, (64, 64));
        assert_eq!(boxes.len(), 2, "{boxes:?}");
        assert!((boxes[0].score - 0.95).abs() < 1e-5, "highest score first");
    }

    #[test]
    fn marker_detector_finds_magenta_blocks() {
        let (w, h) = (576usize, 324usize);
        let mut frame = vec![128u8; w * h * 3];
        for (x0, y0) in [(100usize, 60usize), (400, 200)] {
            for y in y0..y0 + 80 {
                for x in x0..x0 + 80 {
                    let i = (y * w + x) * 3;
                    frame[i..i + 3].copy_from_slice(&[255, 0, 255]); // bgr magenta
                }
            }
        }
        let d = load(&DetectorSpec::Marker).unwrap();
        let mut found = d.detect(&FrameView { bgr: &frame, width: w as u32, height: h as u32 }).unwrap();
        found.sort_by(|a, b| a.x.total_cmp(&b.x));
        assert_eq!(found.len(), 2);
        let b = found[0];
        assert!(
            (b.x - 100.0).abs() <= 2.0 && (b.y - 60.0).abs() <= 2.0 && (b.w - 80.0).abs() <= 2.0,
            "{b:?}"
        );
        assert!(d.id().starts_with("marker"));
    }
}
