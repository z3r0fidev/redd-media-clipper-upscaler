//! Crop path and fallback ranges as an ffmpeg `sendcmd` file, plus keyframes (research R5).
//!
//! Commands are written at frame midpoints `(k − 0.5)/fps`, so a timestamp printed with six
//! decimals or a slightly irregular (VFR) frame time never shifts a command by one frame. A crop
//! command is written only where x changes.

use super::ReframeAnalysis;
use crate::time::Ts;

/// The `reframe.cmd` text. `offset` (seconds) is added to every time: the render's first video
/// frame time when it is not 0 (whole videos whose audio starts first).
pub fn to_sendcmd(a: &ReframeAnalysis, offset: f64) -> String {
    let n = a.path.len();
    let at = |k: usize| offset + ((k as f64 - 0.5) / a.fps).max(0.0);
    // (time, order, line): crop commands sort before toggles at the same time.
    let mut lines: Vec<(f64, u8, String)> = Vec::new();
    for k in 1..n {
        if a.path[k] != a.path[k - 1] {
            lines.push((at(k), 0, format!("crop@trk x {}", a.path[k])));
        }
    }
    let frame =
        |t: Ts| ((t.as_secs_f64() - a.clip_start.as_secs_f64()) * a.fps - 1e-9).ceil().max(0.0) as usize;
    for r in &a.fallback_ranges {
        let (on, off) = (frame(r.start), frame(r.end));
        if on < n {
            lines.push((at(on), 1, "[enter] overlay@fb enable 1".into()));
        }
        if off < n {
            lines.push((at(off), 1, "[enter] overlay@fb enable 0".into()));
        }
    }
    lines.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
    lines.iter().map(|(t, _, cmd)| format!("{t:.6} {cmd};\n")).collect()
}

/// `(source seconds, crop x)` sampled at `hz`, for JSON and the desktop overlay.
pub fn keyframes(a: &ReframeAnalysis, hz: f64) -> Vec<(f64, i32)> {
    let Some(last) = a.path.len().checked_sub(1) else { return Vec::new() };
    let ticks = (a.len_secs() * hz + 1e-9).floor() as usize;
    (0..=ticks)
        .map(|j| {
            let t = j as f64 / hz;
            let k = ((t * a.fps + 1e-9).floor() as usize).min(last);
            (a.clip_start.as_secs_f64() + t, a.path[k])
        })
        .collect()
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
