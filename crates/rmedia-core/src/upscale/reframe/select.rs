//! Subject selection and fallback ranges per sample (FR-014, research R6).

use super::detect::FaceBox;

/// What a sample shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Subject {
    /// The centre x of the subject group, in analysis pixels.
    Centre(f64),
    NoFace,
    /// Faces that cannot share one portrait crop.
    NoFit,
}

/// Picks the subject: the largest face plus every face at least half its height. When their
/// union is wider than 0.9 crop widths, or one is more than 1.5 crop widths from the largest,
/// nothing fits.
pub fn select(faces: &[FaceBox], crop_w: f64) -> Subject {
    let _ = (faces, crop_w);
    todo!()
}

/// Centre per sample: gaps (no face / no fit) shorter than `max_gap` seconds hold the last
/// centre (or take the next one at the start of a shot); longer gaps stay `None`.
pub fn hold(samples: &[(f64, Subject)], max_gap: f64) -> Vec<Option<f64>> {
    let _ = (samples, max_gap);
    todo!()
}

/// Fallback ranges `[start, end)` within `shot`: runs of no-face/no-fit samples lasting at
/// least 1 s (each sample covers ± half the sampling interval), clipped to the shot and merged.
/// A shot without any face falls back whole.
pub fn fallback_runs(samples: &[(f64, Subject)], shot: (f64, f64), interval: f64) -> Vec<(f64, f64)> {
    let _ = (samples, shot, interval);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(cx: f32, h: f32) -> FaceBox {
        FaceBox { x: cx - h / 2.0, y: 100.0, w: h, h, score: 0.9 }
    }

    const CW: f64 = 332.0;

    #[test]
    fn selection_table() {
        assert_eq!(select(&[], CW), Subject::NoFace);
        assert_eq!(select(&[face(400.0, 80.0)], CW), Subject::Centre(400.0));
        // A small face far away is background: ignored.
        assert_eq!(select(&[face(400.0, 80.0), face(1100.0, 30.0)], CW), Subject::Centre(400.0));
        // Two people 150 px apart share the crop: the group's centre.
        assert_eq!(select(&[face(400.0, 80.0), face(550.0, 70.0)], CW), Subject::Centre(472.5));
        // 300 px apart: the union (380 px) is wider than 0.9 × 332.
        assert_eq!(select(&[face(400.0, 80.0), face(700.0, 80.0)], CW), Subject::NoFit);
        // FarMarkers: two equal markers 1000 px apart.
        assert_eq!(select(&[face(140.0, 80.0), face(1140.0, 80.0)], CW), Subject::NoFit);
    }

    fn samples(kinds: &[Subject]) -> Vec<(f64, Subject)> {
        kinds.iter().enumerate().map(|(i, s)| (i as f64 / 3.0, *s)).collect()
    }

    #[test]
    fn short_gaps_hold_long_gaps_fall_back() {
        use Subject::*;
        let s = samples(&[Centre(10.0), NoFace, Centre(20.0), NoFit, NoFit, NoFit, NoFit, Centre(30.0)]);
        let held = hold(&s, 1.0);
        assert_eq!(&held[..3], &[Some(10.0), Some(10.0), Some(20.0)], "0.33 s gap holds");
        assert!(held[3..7].iter().all(Option::is_none), "1.33 s gap does not");
        assert_eq!(held[7], Some(30.0));
        let leading = hold(&samples(&[NoFace, Centre(5.0)]), 1.0);
        assert_eq!(leading, vec![Some(5.0), Some(5.0)], "a short gap at the start takes the next centre");
        let runs = fallback_runs(&s, (0.0, 8.0 / 3.0), 1.0 / 3.0);
        assert_eq!(runs.len(), 1);
        let (a, b) = runs[0];
        assert!((a - 5.0 / 6.0).abs() < 1e-9 && (b - 13.0 / 6.0).abs() < 1e-9, "{runs:?}");
    }

    #[test]
    fn short_runs_are_not_fallbacks_and_faceless_shots_fall_back_whole() {
        use Subject::*;
        let s = samples(&[Centre(1.0), NoFace, NoFace, Centre(1.0)]);
        assert!(fallback_runs(&s, (0.0, 4.0 / 3.0), 1.0 / 3.0).is_empty(), "0.67 s is held");
        let none = samples(&[NoFace, NoFace]);
        assert_eq!(
            fallback_runs(&none, (0.0, 0.6), 1.0 / 3.0),
            vec![(0.0, 0.6)],
            "a faceless shot, however short"
        );
        let far = samples(&[NoFit; 18]);
        assert_eq!(fallback_runs(&far, (0.0, 6.0), 1.0 / 3.0), vec![(0.0, 6.0)]);
    }
}
