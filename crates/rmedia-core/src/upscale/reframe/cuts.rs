//! Scene-cut detection from scdet scores and frame histograms (research R6).

/// scdet scores at or above this mark a cut.
pub const CUT_SCORE: f64 = 10.0;
/// Histogram distance between consecutive samples that marks a cut scdet missed.
pub const HIST_CUT: f64 = 0.5;

/// Parses `metadata=mode=print` output: `frame:N  pts:P  pts_time:T` followed by
/// `lavfi.scd.score=S` (either on the next line or after a tab). Returns `(T, S)` per frame.
pub fn parse_scdet(text: &str) -> Vec<(f64, f64)> {
    let _ = text;
    todo!()
}

/// 16 bins per channel of a packed bgr24 frame.
pub fn histogram(bgr: &[u8]) -> [u32; 48] {
    let _ = bgr;
    todo!()
}

/// Normalised L1 distance between histograms, in [0, 1].
pub fn hist_distance(a: &[u32; 48], b: &[u32; 48]) -> f64 {
    let _ = (a, b);
    todo!()
}

/// Cut times: every scdet score ≥ [`CUT_SCORE`], plus a cut between consecutive samples whose
/// histograms differ by more than [`HIST_CUT`] when scdet found none there (placed at the
/// highest-scoring frame in between, or at the later sample). Sorted, without t = 0.
pub fn cuts(scores: &[(f64, f64)], samples: &[(f64, [u32; 48])]) -> Vec<f64> {
    let _ = (scores, samples);
    todo!()
}

/// Shots `[start, end)` covering `[0, len)` split at `cuts` (short shots are kept).
pub fn shots(cuts: &[f64], len: f64) -> Vec<(f64, f64)> {
    let _ = (cuts, len);
    todo!()
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
        histogram(&vec![level; 30 * 3])
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
