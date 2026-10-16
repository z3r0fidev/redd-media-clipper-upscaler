//! Crop path and fallback ranges as an ffmpeg `sendcmd` file, plus keyframes (research R5).
//!
//! Commands are written at frame midpoints `(k − 0.5)/fps`, so a timestamp printed with six
//! decimals or a slightly irregular (VFR) frame time never shifts a command by one frame. A crop
//! command is written only where x changes.

use super::ReframeAnalysis;

/// The `reframe.cmd` text. `offset` (seconds) is added to every time: the render's first video
/// frame time when it is not 0 (whole videos whose audio starts first).
pub fn to_sendcmd(a: &ReframeAnalysis, offset: f64) -> String {
    let _ = (a, offset);
    todo!()
}

/// `(source seconds, crop x)` sampled at `hz`, for JSON and the desktop overlay.
pub fn keyframes(a: &ReframeAnalysis, hz: f64) -> Vec<(f64, i32)> {
    let _ = (a, hz);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Span;
    use crate::time::Ts;
    use crate::upscale::reframe::{Shot, ShotKind};

    fn analysis(clip_start: f64, path: Vec<i32>, fallback: &[(f64, f64)]) -> ReframeAnalysis {
        let s = |v: f64| Ts::from_secs_f64(v);
        let len = path.len() as f64 / 30.0;
        ReframeAnalysis {
            key: "k".into(),
            algo_version: super::super::ALGO_VERSION,
            source_size: (1280, 720),
            crop: (332, 720),
            crop_y: 0,
            fps: 30.0,
            clip_start: s(clip_start),
            shots: vec![Shot {
                span: Span { start: s(clip_start), end: s(clip_start + len) },
                kind: ShotKind::Mixed,
            }],
            fallback_ranges: fallback
                .iter()
                .map(|(a, b)| Span { start: s(clip_start + a), end: s(clip_start + b) })
                .collect(),
            path,
            detections: 0,
            samples: 0,
        }
    }

    fn path() -> Vec<i32> {
        let mut p = vec![100, 100, 102, 104];
        p.extend(vec![104; 86]);
        p
    }

    #[test]
    fn golden_command_file() {
        let a = analysis(0.0, path(), &[(1.0, 2.0)]);
        insta::assert_snapshot!(to_sendcmd(&a, 0.0), @r"
        0.050000 crop@trk x 102;
        0.083333 crop@trk x 104;
        0.983333 [enter] overlay@fb enable 1;
        1.983333 [enter] overlay@fb enable 0;
        ");
    }

    #[test]
    fn times_are_clip_relative_plus_offset() {
        let a = analysis(5.0, path(), &[(1.0, 2.0)]);
        assert_eq!(to_sendcmd(&a, 0.0), to_sendcmd(&analysis(0.0, path(), &[(1.0, 2.0)]), 0.0));
        assert!(to_sendcmd(&a, 0.02).starts_with("0.070000 crop@trk x 102;"));
        // A range reaching the end of the clip never switches back.
        let tail = analysis(0.0, path(), &[(2.0, 3.0)]);
        assert!(!to_sendcmd(&tail, 0.0).contains("enable 0"));
        // A range from the very start switches on at t = 0.
        let head = analysis(0.0, path(), &[(0.0, 1.0)]);
        assert!(to_sendcmd(&head, 0.0).contains("0.000000 [enter] overlay@fb enable 1;"));
    }

    #[test]
    fn keyframes_at_ten_hertz_in_source_time() {
        let a = analysis(5.0, vec![100; 30], &[]); // 1 s
        let k = keyframes(&a, 10.0);
        assert_eq!(k.len(), 11);
        assert_eq!(k[0], (5.0, 100));
        assert!((k[10].0 - 6.0).abs() < 1e-9);
    }
}
