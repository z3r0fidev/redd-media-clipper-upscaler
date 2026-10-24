//! Frame rates and GIF centisecond delays (research R3, FR-003).
//!
//! A GIF stores each frame's delay in whole centiseconds. FFmpeg's gif muxer rescales frame
//! timestamps to 1/100 s with round-to-nearest, so a rate whose frame period is not a whole
//! number of centiseconds alternates two delays (15 fps → 7, 6, 7, 7, 6, 7 …). Rates are never
//! changed to a different exact one behind the user's back; approximate ones are reported.

use serde::Serialize;

/// The highest GIF frame rate accepted: a 2 cs delay (many players slow anything shorter).
pub const MAX_FPS: u32 = 50;
/// The shortest delay a GIF may have (centiseconds).
pub const MIN_DELAY_CS: u32 = 2;

/// How a frame rate is shown in GIF delays.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifTiming"))]
pub struct Timing {
    /// Every delay is the same whole number of centiseconds.
    pub exact: bool,
    /// The average delay in centiseconds (100 / fps).
    pub avg_cs: f32,
}

/// Checks a requested GIF frame rate: 1 to [`MAX_FPS`].
pub fn validate(fps: u32) -> Result<(), String> {
    match fps {
        0 => Err("the frame rate must be at least 1 fps".into()),
        f if f > MAX_FPS => Err(format!(
            "{f} fps is too fast for a GIF: frame delays are whole centiseconds and players slow down \
             anything under {MIN_DELAY_CS} cs; use {MAX_FPS} fps or less (or --format mp4)"
        )),
        _ => Ok(()),
    }
}

/// The output rate: the target's, capped at the source's (`n/d`), so frames are never
/// duplicated. An unknown source rate keeps the target's.
pub fn cap(target_fps: u32, source: Option<(u32, u32)>) -> (u32, u32) {
    match source.filter(|(n, d)| *n > 0 && *d > 0) {
        Some((n, d)) if u64::from(n) < u64::from(target_fps) * u64::from(d) => reduce(n, d),
        _ => (target_fps, 1),
    }
}

fn reduce(n: u32, d: u32) -> (u32, u32) {
    let (mut a, mut b) = (n, d);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    (n / a, d / a)
}

/// The timing of `rate` (`(n, d)` frames per second).
pub fn timing(rate: (u32, u32)) -> Timing {
    let (n, d) = rate;
    let cs = 100 * u64::from(d);
    Timing { exact: cs % u64::from(n) == 0, avg_cs: (cs as f64 / f64::from(n)) as f32 }
}

/// The delays the gif muxer writes for `frames` frames at `rate`: the differences of the frame
/// times rounded to centiseconds (the last frame keeps the rounded frame period).
pub fn delays(rate: (u32, u32), frames: usize) -> Vec<u32> {
    let (n, d) = (u64::from(rate.0), u64::from(rate.1));
    // round(i * 100 * d / n), half away from zero, as av_rescale_q does.
    let at = |i: u64| (i * 200 * d + n) / (2 * n);
    let period = (200 * d + n) / (2 * n);
    (0..frames as u64)
        .map(|i| if i + 1 < frames as u64 { at(i + 1) - at(i) } else { period } as u32)
        .collect()
}

/// The fps as people write it: `15`, `29.97`.
pub fn fps_text(rate: (u32, u32)) -> String {
    let (n, d) = rate;
    if n % d == 0 {
        return (n / d).to_string();
    }
    let s = format!("{:.2}", f64::from(n) / f64::from(d));
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// The `TIMING_APPROXIMATE` notice for an inexact rate (`None` when exact).
pub fn approximate_note(rate: (u32, u32)) -> Option<String> {
    let t = timing(rate);
    if t.exact {
        return None;
    }
    let ds = delays(rate, 120);
    let (lo, hi) = (ds.iter().min().copied().unwrap_or(0), ds.iter().max().copied().unwrap_or(0));
    let fps = 100.0 / f64::from(t.avg_cs);
    Some(format!("{} fps is shown as {lo}–{hi} cs frame delays (≈ {fps:.1} fps)", fps_text(rate)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_rates_have_one_whole_delay() {
        for (fps, cs) in [(5, 20), (10, 10), (20, 5), (25, 4), (50, 2)] {
            let t = timing((fps, 1));
            assert!(t.exact, "{fps} fps");
            assert_eq!(t.avg_cs, cs as f32, "{fps} fps");
            assert!(delays((fps, 1), 40).iter().all(|d| *d == cs), "{fps} fps");
            assert_eq!(approximate_note((fps, 1)), None);
        }
    }

    #[test]
    fn approximate_rates_alternate_two_delays() {
        // (fps, avg cs, the two delays)
        for (rate, avg, lo, hi) in
            [((12, 1), 8.333, 8, 9), ((15, 1), 6.667, 6, 7), ((24, 1), 4.167, 4, 5), ((30, 1), 3.333, 3, 4)]
        {
            let t = timing(rate);
            assert!(!t.exact, "{rate:?}");
            assert!((f64::from(t.avg_cs) - avg).abs() < 0.001, "{rate:?}: {}", t.avg_cs);
            let ds = delays(rate, 60);
            assert!(ds.iter().all(|d| *d == lo || *d == hi), "{rate:?}: {ds:?}");
            assert!(ds.iter().all(|d| *d >= MIN_DELAY_CS));
        }
        assert_eq!(approximate_note((15, 1)).unwrap(), "15 fps is shown as 6–7 cs frame delays (≈ 15.0 fps)");
        assert_eq!(
            approximate_note((30000, 1001)).unwrap(),
            "29.97 fps is shown as 3–4 cs frame delays (≈ 30.0 fps)"
        );
    }

    #[test]
    fn fifteen_fps_matches_the_muxer_measurement() {
        // T005(b) and research R3: 60 frames at 15 fps are 20 delays of 6 cs and 40 of 7 cs,
        // starting 7, 6, 7.
        let ds = delays((15, 1), 60);
        assert_eq!(&ds[..6], &[7, 6, 7, 7, 6, 7]);
        assert_eq!(ds.iter().filter(|d| **d == 6).count(), 20);
        assert_eq!(ds.iter().filter(|d| **d == 7).count(), 40);
        assert_eq!(ds.iter().sum::<u32>(), 400, "4 s");
    }

    #[test]
    fn rates_above_fifty_are_rejected() {
        assert!(validate(50).is_ok());
        assert!(validate(1).is_ok());
        assert!(validate(51).unwrap_err().contains("50 fps or less"));
        assert!(validate(0).is_err());
    }

    #[test]
    fn the_rate_is_capped_at_the_source() {
        assert_eq!(cap(15, Some((30, 1))), (15, 1));
        assert_eq!(cap(30, Some((30000, 1001))), (30000, 1001), "never faster than the source");
        assert_eq!(cap(20, Some((24000, 1001))), (20, 1));
        assert_eq!(cap(20, Some((12, 1))), (12, 1));
        assert_eq!(cap(15, Some((30, 2))), (15, 1), "15/1, reduced");
        assert_eq!(cap(15, Some((10, 2))), (5, 1));
        assert_eq!(cap(15, None), (15, 1));
        assert_eq!(fps_text((30000, 1001)), "29.97");
        assert_eq!(fps_text((25, 2)), "12.5");
    }
}
