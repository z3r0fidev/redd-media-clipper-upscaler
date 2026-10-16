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
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    match spec {
        DetectorSpec::Yunet(path) => {
            // One parsed model per path for the whole process: every job shares it and its
            // optimised plans.
            static LOADED: OnceLock<Mutex<HashMap<PathBuf, Arc<YunetDetector>>>> = OnceLock::new();
            let mut loaded = LOADED.get_or_init(Default::default).lock().expect("detector registry");
            if let Some(d) = loaded.get(path) {
                return Ok(d.clone());
            }
            let d = Arc::new(YunetDetector::open(path)?);
            loaded.insert(path.clone(), d.clone());
            Ok(d)
        }
        #[cfg(any(test, feature = "test-support"))]
        DetectorSpec::Marker => Ok(Arc::new(MarkerDetector)),
    }
}

/// YuNet score threshold and NMS overlap (research R6).
pub const SCORE_THRESHOLD: f32 = 0.6;
pub const NMS_IOU: f32 = 0.3;

/// Decodes YuNet's 12 outputs (cls, obj, bbox, kps × strides 8/16/32, in that order) for an
/// input of `input` size (multiples of 32). Boxes centred outside `valid` (the unpadded frame)
/// are dropped. Deterministic NMS: by score, then anchor order.
pub fn decode(outputs: &[&[f32]], input: (u32, u32), valid: (u32, u32)) -> Vec<FaceBox> {
    let mut boxes = Vec::new();
    for (si, stride) in [8u32, 16, 32].into_iter().enumerate() {
        let (cols, rows) = (input.0 / stride, input.1 / stride);
        let (Some(cls), Some(obj), Some(bbox)) = (outputs.get(si), outputs.get(3 + si), outputs.get(6 + si))
        else {
            return Vec::new();
        };
        let n = (cols * rows) as usize;
        if cls.len() < n || obj.len() < n || bbox.len() < n * 4 {
            return Vec::new();
        }
        let s = stride as f32;
        for idx in 0..n {
            let score = (cls[idx].clamp(0.0, 1.0) * obj[idx].clamp(0.0, 1.0)).sqrt();
            if score < SCORE_THRESHOLD {
                continue;
            }
            let (row, col) = ((idx as u32 / cols) as f32, (idx as u32 % cols) as f32);
            let b = &bbox[idx * 4..idx * 4 + 4];
            let (cx, cy) = ((col + b[0]) * s, (row + b[1]) * s);
            let (w, h) = (b[2].exp() * s, b[3].exp() * s);
            if !(w.is_finite() && h.is_finite()) || cx >= valid.0 as f32 || cy >= valid.1 as f32 {
                continue;
            }
            boxes.push(FaceBox { x: cx - w / 2.0, y: cy - h / 2.0, w, h, score });
        }
    }
    nms(boxes, NMS_IOU)
}

/// Non-maximum suppression: best score first (stable, so ties keep anchor order).
fn nms(mut boxes: Vec<FaceBox>, iou: f32) -> Vec<FaceBox> {
    boxes.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<FaceBox> = Vec::new();
    for b in boxes {
        if kept.iter().all(|k| overlap(k, &b) <= iou) {
            kept.push(b);
        }
    }
    kept
}

fn overlap(a: &FaceBox, b: &FaceBox) -> f32 {
    let w = ((a.x + a.w).min(b.x + b.w) - a.x.max(b.x)).max(0.0);
    let h = ((a.y + a.h).min(b.y + b.h) - a.y.max(b.y)).max(0.0);
    let inter = w * h;
    let union = a.w * a.h + b.w * b.h - inter;
    if union > 0.0 { inter / union } else { 0.0 }
}

/// Largest model file read (YuNet is about 230 kB).
const MAX_MODEL_BYTES: u64 = 50 * 1024 * 1024;

/// YuNet 2026may under tract: dynamic input size, one optimised plan per frame size.
pub struct YunetDetector {
    id: String,
    model: tract_onnx::prelude::InferenceModel,
    plans: std::sync::Mutex<std::collections::HashMap<(u32, u32), YunetPlan>>,
}

/// An optimised, runnable plan (reference-counted by tract).
type YunetPlan = Arc<tract_onnx::prelude::TypedRunnableModel>;

impl std::fmt::Debug for YunetDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YunetDetector").field("id", &self.id).finish_non_exhaustive()
    }
}

fn model_error(e: impl std::fmt::Display) -> crate::Error {
    crate::Error::EngineFailed { message: format!("face model: {e}"), stderr_tail: String::new() }
}

impl YunetDetector {
    /// Loads the model, hashing exactly the bytes that are parsed.
    pub fn open(path: &std::path::Path) -> Result<YunetDetector> {
        use sha2::{Digest, Sha256};
        use tract_onnx::prelude::Framework;
        let meta = std::fs::metadata(path).map_err(model_error)?;
        if !meta.is_file() || meta.len() > MAX_MODEL_BYTES {
            return Err(model_error(format!("{} is not a model file", path.display())));
        }
        let bytes = std::fs::read(path).map_err(model_error)?;
        let sha = format!("{:x}", Sha256::digest(&bytes));
        let model =
            tract_onnx::onnx().model_for_read(&mut std::io::Cursor::new(bytes)).map_err(model_error)?;
        Ok(YunetDetector { id: format!("yunet-2026may:{sha}"), model, plans: Default::default() })
    }

    fn plan(&self, size: (u32, u32)) -> Result<YunetPlan> {
        use tract_onnx::prelude::*;
        let mut plans = self.plans.lock().expect("plan cache");
        if let Some(p) = plans.get(&size) {
            return Ok(p.clone());
        }
        let fact = f32::fact([1, 3, size.1 as usize, size.0 as usize]);
        let plan = self
            .model
            .clone()
            .with_input_fact(0, fact.into())
            .and_then(|m| m.into_optimized())
            .and_then(|m| m.into_runnable())
            .map_err(model_error)?;
        plans.insert(size, plan.clone());
        Ok(plan)
    }
}

impl Detector for YunetDetector {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn detect(&self, f: &FrameView<'_>) -> Result<Vec<FaceBox>> {
        use tract_onnx::prelude::*;
        let (pw, ph) = super::decode::padded((f.width, f.height));
        let plan = self.plan((pw, ph))?;
        let (w, h) = (f.width as usize, f.height as usize);
        // BGR, 0–255, zero padding to the right and bottom (as OpenCV's FaceDetectorYN feeds it).
        let input: Tensor =
            tract_ndarray::Array4::<f32>::from_shape_fn((1, 3, ph as usize, pw as usize), |(_, c, y, x)| {
                if x < w && y < h { f32::from(f.bgr[(y * w + x) * 3 + c]) } else { 0.0 }
            })
            .into();
        let outputs = plan.run(tvec!(input.into())).map_err(model_error)?;
        let views: Vec<&[f32]> = outputs
            .iter()
            .map(|t| {
                t.to_plain_array_view::<f32>()
                    .map_err(model_error)?
                    .to_slice()
                    .ok_or_else(|| model_error("output is not contiguous"))
            })
            .collect::<Result<_>>()?;
        Ok(decode(&views, (pw, ph), (f.width, f.height)))
    }
}

/// Finds pure-magenta blocks (synthetic test markers), as 4-connected components.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug)]
pub struct MarkerDetector;

#[cfg(any(test, feature = "test-support"))]
impl Detector for MarkerDetector {
    fn id(&self) -> String {
        "marker:1".into()
    }

    fn detect(&self, f: &FrameView<'_>) -> Result<Vec<FaceBox>> {
        let (w, h) = (f.width as usize, f.height as usize);
        let is_marker = |i: usize| {
            let p = &f.bgr[i * 3..i * 3 + 3];
            p[0] > 180 && p[1] < 90 && p[2] > 180
        };
        let mut seen = vec![false; w * h];
        let mut found = Vec::new();
        for start in 0..w * h {
            if seen[start] || !is_marker(start) {
                continue;
            }
            seen[start] = true;
            let (mut stack, mut area) = (vec![start], 0usize);
            let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
            while let Some(i) = stack.pop() {
                let (x, y) = (i % w, i / w);
                area += 1;
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                let mut visit = |j: usize| {
                    if !seen[j] && is_marker(j) {
                        seen[j] = true;
                        stack.push(j);
                    }
                };
                if x > 0 {
                    visit(i - 1);
                }
                if x + 1 < w {
                    visit(i + 1);
                }
                if y > 0 {
                    visit(i - w);
                }
                if y + 1 < h {
                    visit(i + w);
                }
            }
            if area >= 16 {
                found.push(FaceBox {
                    x: x0 as f32,
                    y: y0 as f32,
                    w: (x1 - x0 + 1) as f32,
                    h: (y1 - y0 + 1) as f32,
                    score: 1.0,
                });
            }
        }
        Ok(found)
    }
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
    fn yunet_loads_and_finds_nothing_in_a_blank_frame() {
        let settings = crate::settings::Settings::default();
        let Some(path) =
            crate::models::locate_in(crate::models::YUNET, &crate::models::search_dirs(&settings))
        else {
            eprintln!("skipping: YuNet not fetched (cargo xtask fetch-models)");
            return;
        };
        let d = load(&DetectorSpec::Yunet(path.clone())).unwrap();
        assert!(d.id().starts_with("yunet-2026may:"), "{}", d.id());
        let frame = vec![128u8; 576 * 324 * 3];
        let faces = d.detect(&FrameView { bgr: &frame, width: 576, height: 324 }).unwrap();
        assert!(faces.is_empty(), "{faces:?}");
        assert!(Arc::ptr_eq(&d, &load(&DetectorSpec::Yunet(path)).unwrap()), "one model per process");
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
