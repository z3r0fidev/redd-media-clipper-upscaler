//! Crop path per shot: median, deadzone, speed and acceleration limits (research R6).
//!
//! Positions are crop-window centres in source pixels until [`finalize`] turns them into even,
//! clamped crop x values. Each shot is followed independently: at a cut the position and the
//! speed reset, so the crop may jump there and nowhere else (FR-013).

use crate::upscale::Motion;

/// Per-frame limits, in pixels per frame and pixels per frame².
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub speed: f64,
    pub accel: f64,
}

/// Speed and acceleration limits for a motion profile, scaled to the crop width and frame rate
/// (research R6: calm 0.35/0.7, normal 0.7/1.5, fast 1.4/3.0 crop widths per s / s²).
pub fn limits(motion: Motion, crop_w: f64, fps: f64) -> Limits {
    let (speed, accel) = match motion {
        Motion::Calm => (0.35, 0.7),
        Motion::Normal => (0.7, 1.5),
        Motion::Fast => (1.4, 3.0),
    };
    Limits { speed: speed * crop_w / fps, accel: accel * crop_w / (fps * fps) }
}

/// Centred median over 5 values; the window shrinks symmetrically at the ends.
pub fn median5(v: &[f64]) -> Vec<f64> {
    (0..v.len())
        .map(|i| {
            let half = 2.min(i).min(v.len() - 1 - i);
            let mut w: Vec<f64> = v[i - half..=i + half].to_vec();
            w.sort_by(f64::total_cmp);
            w[half]
        })
        .collect()
}

/// The target moves only when the value leaves ±`band` around the current target, and then
/// jumps to the value (ignores small jitter, FR-013).
pub fn deadzone(v: &[f64], band: f64) -> Vec<f64> {
    let mut target = match v.first() {
        Some(t) => *t,
        None => return Vec::new(),
    };
    v.iter()
        .map(|x| {
            if (x - target).abs() > band {
                target = *x;
            }
            target
        })
        .collect()
}

/// Linear interpolation of samples `(t, value)` onto frames `t = (first + i) / fps`,
/// `i in 0..count`; values before the first and after the last sample hold.
pub fn interpolate(samples: &[(f64, f64)], fps: f64, first: usize, count: usize) -> Vec<f64> {
    // One pass over frames and samples together (frames are in time order).
    let mut j = 0;
    (0..count)
        .map(|i| {
            let t = (first + i) as f64 / fps;
            while j < samples.len() && samples[j].0 <= t {
                j += 1;
            }
            match j {
                0 => samples.first().map_or(0.0, |s| s.1),
                j if j == samples.len() => samples[j - 1].1,
                j => {
                    let ((t0, v0), (t1, v1)) = (samples[j - 1], samples[j]);
                    v0 + (v1 - v0) * (t - t0) / (t1 - t0)
                }
            }
        })
        .collect()
}

/// Follows `targets` from `targets[0]` at rest, never faster than `speed` and never changing
/// speed by more than `accel` per frame, slowing in time to stop at the target (no overshoot).
pub fn follow(targets: &[f64], lim: Limits) -> Vec<f64> {
    let Some(&start) = targets.first() else { return Vec::new() };
    let (v, a) = (lim.speed, lim.accel);
    let (mut pos, mut step) = (start, 0.0f64);
    let mut out = Vec::with_capacity(targets.len());
    out.push(pos);
    for &target in &targets[1..] {
        let d = target - pos;
        // The fastest speed that can still stop on the target, decelerating by `a` per frame.
        let stoppable = -a / 2.0 + (a * a / 4.0 + 2.0 * a * d.abs()).sqrt();
        let wanted = d.signum() * v.min(stoppable);
        let mut next = wanted.clamp(step - a, step + a).clamp(-v, v);
        let overshoots = (next > 0.0 && next > d) || (next < 0.0 && next < d);
        if overshoots && (d - step).abs() <= a {
            next = d;
        }
        step = next;
        pos += step;
        out.push(pos);
    }
    out
}

/// Crop centres → crop x: centre − w/2, clamped to `[0, max_x]`, rounded to even.
pub fn finalize(centres: &[f64], crop_w: u32, max_x: u32) -> Vec<i32> {
    let half = f64::from(crop_w) / 2.0;
    centres
        .iter()
        .map(|c| {
            let x = (c - half).clamp(0.0, f64::from(max_x));
            let even = (x / 2.0).round() * 2.0;
            (even as i32).clamp(0, max_x as i32 - (max_x as i32 % 2))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CW: f64 = 332.0;
    const FPS: f64 = 30.0;

    #[test]
    fn profiles_scale_with_crop_width_and_fps() {
        let n = limits(Motion::Normal, CW, FPS);
        assert!((n.speed - 0.7 * CW / FPS).abs() < 1e-9 && (n.accel - 1.5 * CW / FPS / FPS).abs() < 1e-9);
        let (c, f) = (limits(Motion::Calm, CW, FPS), limits(Motion::Fast, CW, FPS));
        assert!(c.speed < n.speed && n.speed < f.speed);
        assert!((f.speed - 1.4 * CW / FPS).abs() < 1e-9 && (c.accel - 0.7 * CW / FPS / FPS).abs() < 1e-9);
    }

    #[test]
    fn median_removes_single_outliers() {
        assert_eq!(median5(&[0.0, 0.0, 100.0, 0.0, 0.0]), vec![0.0; 5]);
        assert_eq!(
            median5(&[1.0, 2.0, 3.0]),
            vec![1.0, 2.0, 3.0],
            "the window shrinks symmetrically at the ends"
        );
        assert_eq!(median5(&[]), Vec::<f64>::new());
    }

    #[test]
    fn deadzone_ignores_jitter_and_follows_moves() {
        let band = 0.06 * CW; // 19.92
        let v = [500.0, 510.0, 490.0, 519.0, 525.0, 530.0, 500.0];
        assert_eq!(deadzone(&v, band), vec![500.0, 500.0, 500.0, 500.0, 525.0, 525.0, 500.0]);
    }

    #[test]
    fn interpolation_is_linear_and_holds_at_the_ends() {
        let s = [(0.0, 0.0), (1.0, 30.0)];
        let f = interpolate(&s, FPS, 0, 31);
        assert_eq!(f.len(), 31);
        assert!((f[15] - 15.0).abs() < 1e-9 && f[30] == 30.0);
        let late = interpolate(&[(1.0, 10.0), (2.0, 20.0)], FPS, 0, 5);
        assert_eq!(late, vec![10.0; 5], "before the first sample: hold");
    }

    #[test]
    fn following_respects_speed_and_acceleration_and_stops_on_target() {
        for motion in [Motion::Calm, Motion::Normal, Motion::Fast] {
            let lim = limits(motion, CW, FPS);
            let mut targets = vec![200.0; 10];
            targets.extend(vec![900.0; 400]);
            let p = follow(&targets, lim);
            assert_eq!(p[0], 200.0, "starts at rest on the first target");
            for w in p.windows(3) {
                let (s1, s2) = (w[1] - w[0], w[2] - w[1]);
                assert!(s2.abs() <= lim.speed + 1e-9, "{motion:?} speed {s2}");
                assert!((s2 - s1).abs() <= lim.accel + 1e-9, "{motion:?} accel {}", s2 - s1);
            }
            assert!((p.last().unwrap() - 900.0).abs() < 1.0, "{motion:?} reaches the target");
            assert!(p.iter().all(|x| *x <= 901.0), "{motion:?} never overshoots by more than a pixel");
        }
    }

    #[test]
    fn finalize_clamps_and_rounds_to_even() {
        // max_x = 1280 − 332 = 948
        assert_eq!(finalize(&[100.0, 166.0, 500.3, 1200.0, 1281.0], 332, 948), vec![0, 0, 334, 948, 948]);
        assert!(finalize(&[777.7], 332, 948).iter().all(|x| x % 2 == 0));
    }
}
