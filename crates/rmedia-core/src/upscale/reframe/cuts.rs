//! Scene-cut detection from scdet scores and frame histograms (research R6).

/// scdet scores at or above this mark a cut.
pub const CUT_SCORE: f64 = 10.0;
/// Histogram distance between consecutive samples that marks a cut scdet missed.
pub const HIST_CUT: f64 = 0.5;

/// Parses `metadata=mode=print` output: `frame:N  pts:P  pts_time:T` followed by
/// `lavfi.scd.score=S` (either on the next line or after a tab). Returns `(T, S)` per frame.
pub fn parse_scdet(text: &str) -> Vec<(f64, f64)> {
    let value = |line: &str, key: &str| -> Option<f64> {
        let rest = &line[line.find(key)? + key.len()..];
        rest.split_whitespace().next()?.parse().ok()
    };
    let mut out = Vec::new();
    let mut time = None;
    for line in text.lines() {
        if let Some(t) = value(line, "pts_time:") {
            time = Some(t);
        }
        if let (Some(score), Some(t)) = (value(line, "lavfi.scd.score="), time) {
            out.push((t, score));
            time = None;
        }
    }
    out
}

/// 16 bins per channel of a packed bgr24 frame.
pub fn histogram(bgr: &[u8]) -> [u32; 48] {
    let mut h = [0u32; 48];
    for px in bgr.chunks_exact(3) {
        h[usize::from(px[0] >> 4)] += 1;
        h[16 + usize::from(px[1] >> 4)] += 1;
        h[32 + usize::from(px[2] >> 4)] += 1;
    }
    h
}

/// Normalised L1 distance between histograms, in [0, 1].
pub fn hist_distance(a: &[u32; 48], b: &[u32; 48]) -> f64 {
    let (diff, total) = a.iter().zip(b).fold((0u64, 0u64), |(d, t), (x, y)| {
        (d + u64::from(x.abs_diff(*y)), t + u64::from(*x) + u64::from(*y))
    });
    if total == 0 { 0.0 } else { diff as f64 / total as f64 }
}

/// Cut times: every scdet score ≥ [`CUT_SCORE`], plus a cut between consecutive samples whose
/// histograms differ by more than [`HIST_CUT`] when scdet found none there (placed at the
/// highest-scoring frame in between, or at the later sample). Sorted, without t = 0.
pub fn cuts(scores: &[(f64, f64)], samples: &[(f64, [u32; 48])]) -> Vec<f64> {
    let mut out: Vec<f64> =
        scores.iter().filter(|(t, s)| *s >= CUT_SCORE && *t > 0.0).map(|(t, _)| *t).collect();
    for w in samples.windows(2) {
        let ((t0, h0), (t1, h1)) = (&w[0], &w[1]);
        if hist_distance(h0, h1) <= HIST_CUT || out.iter().any(|c| c > t0 && c <= t1) {
            continue;
        }
        let best = scores
            .iter()
            .filter(|(t, _)| t > t0 && t <= t1)
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.total_cmp(&a.0)))
            .map_or(*t1, |(t, _)| *t);
        out.push(best);
    }
    out.sort_by(f64::total_cmp);
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    out
}

/// Shots `[start, end)` covering `[0, len)` split at `cuts` (short shots are kept).
pub fn shots(cuts: &[f64], len: f64) -> Vec<(f64, f64)> {
    let mut bounds = vec![0.0];
    bounds.extend(cuts.iter().copied().filter(|c| *c > 0.0 && *c < len));
    bounds.push(len);
    bounds.windows(2).map(|w| (w[0], w[1])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured from the bundled ffmpeg n9.0.2 on the MovingMarker fixture.
    const GOLDEN: &str = "frame:0    pts:0       pts_time:0\nlavfi.scd.score=0.000\nframe:1    pts:512     pts_time:0.0333333\nlavfi.scd.score=0.034\nframe:180  pts:92160   pts_time:6\tlavfi.scd.score=12.543\nframe:181  pts:92672   pts_time:6.03333\nlavfi.scd.score=0.000\n";

    #[test]
    fn parses_both_line_layouts() {
        let s = parse_scdet(GOLDEN);
        assert_eq!(s.len(), 4);
        assert_eq!(s[1], (0.0333333, 0.034));
        assert_eq!(s[2], (6.0, 12.543));
        assert!(parse_scdet("garbage\nlavfi.scd.score=1\n").is_empty());
    }

    fn flat(level: u8) -> [u32; 48] {
        histogram(&[level; 30 * 3])
    }

    #[test]
    fn histograms_and_distance() {
        let h = flat(0);
        assert_eq!(h[0] + h[16] + h[32], 90);
        assert_eq!(hist_distance(&h, &h), 0.0);
        assert_eq!(hist_distance(&flat(0), &flat(255)), 1.0);
    }

    #[test]
    fn scdet_cuts_plus_histogram_cuts_between_samples() {
        let scores = [(0.0, 0.0), (1.0, 3.0), (2.0, 12.5), (3.0, 0.0), (3.9, 8.0), (4.0, 0.1)];
        let a = flat(10);
        let b = flat(250);
        let samples = [(0.0, a), (1.0, a), (2.0, b), (3.0, b), (4.0, a), (5.0, a)];
        // 2.0 from scdet; between samples 3.0 and 4.0 the histogram jumps and scdet saw no cut:
        // the cut goes to the highest score in (3, 4], 3.9.
        assert_eq!(cuts(&scores, &samples), vec![2.0, 3.9]);
    }

    #[test]
    fn shots_split_at_cuts_and_keep_short_ones() {
        assert_eq!(shots(&[6.0], 10.0), vec![(0.0, 6.0), (6.0, 10.0)]);
        assert_eq!(shots(&[2.0, 2.5], 4.0), vec![(0.0, 2.0), (2.0, 2.5), (2.5, 4.0)]);
        assert_eq!(shots(&[], 3.0), vec![(0.0, 3.0)]);
    }
}
