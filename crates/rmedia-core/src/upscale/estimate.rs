//! Output size and time estimates, free-space check, platform limits (FR-021, research R12).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::preset::Preset;

/// CPU-tier encode speed at 1080x1920 on the reference laptop (x265 `-preset fast` ran about
/// 1.7x realtime at 30 fps; research/02 "Measured"). Used until the capability self-test (US3)
/// measures this machine.
const CPU_FPS_AT_1080X1920: f64 = 50.0;

/// Free space must exceed the estimates by this factor (they are rough).
const SPACE_MARGIN: f64 = 1.2;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Estimate {
    /// Predicted output size.
    pub bytes: u64,
    /// Predicted processing time.
    pub secs: f64,
}

/// The CPU tier's assumed encode speed (frames/s) at `w`x`h`.
pub fn cpu_fps(w: u32, h: u32) -> f64 {
    CPU_FPS_AT_1080X1920 * (1080.0 * 1920.0) / (f64::from(w) * f64::from(h))
}

/// The size the capability self-tests run at (the default preset).
const SELF_TEST_PIXELS: f64 = 1080.0 * 2340.0;

/// Encode speed (frames/s) of `tier` at `size`: the self-test measurement from the capability
/// record scaled by pixel count, or the reference-laptop CPU figure without a measurement.
pub fn encode_fps(
    tier: crate::upscale::tier::Tier,
    record: Option<&crate::upscale::capability::CapabilityRecord>,
    size: (u32, u32),
) -> f64 {
    use crate::upscale::tier::Tier;
    let measured = record.and_then(|r| match tier {
        Tier::Cpu => r.tiers.cpu.fps,
        Tier::Gpu => r.tiers.gpu.fps,
        Tier::Ai => r.tiers.ai.fps,
    });
    match measured {
        Some(fps) if fps > 0.0 => f64::from(fps) * SELF_TEST_PIXELS / (f64::from(size.0) * f64::from(size.1)),
        _ => cpu_fps(size.0, size.1),
    }
}

/// Size: `est_bpp × W × H × fps × duration / 8` plus audio. Time: frames / encode speed.
pub fn estimate(
    preset: &Preset,
    fps_out: f64,
    duration_s: f64,
    audio_kbps: Option<u32>,
    encode_fps: f64,
) -> Estimate {
    let pixels = f64::from(preset.width) * f64::from(preset.height);
    let video = f64::from(preset.est_bpp) * pixels * fps_out * duration_s / 8.0;
    let audio = f64::from(audio_kbps.unwrap_or(0)) * 1000.0 / 8.0 * duration_s;
    let secs = if encode_fps > 0.0 { duration_s * fps_out / encode_fps } else { 0.0 };
    Estimate { bytes: (video + audio).round() as u64, secs }
}

/// Warnings for predicted outputs over the preset's platform limits (FR-002).
pub fn limit_warnings(preset: &Preset, est: &Estimate, duration_s: f64) -> Vec<String> {
    let Some(limits) = &preset.limits else { return Vec::new() };
    let mut w = Vec::new();
    if let Some(max) = limits.max_bytes
        && est.bytes > max
    {
        w.push(format!(
            "predicted size {} exceeds the {} limit of {}; the video bitrate is capped to fit",
            human_bytes(est.bytes),
            preset.name,
            human_bytes(max)
        ));
    }
    if let Some(max) = limits.max_duration
        && duration_s > max.as_secs_f64()
    {
        w.push(format!(
            "duration {:.0} s exceeds the {} limit of {:.0} s; the platform may reject it",
            duration_s,
            preset.name,
            max.as_secs_f64()
        ));
    }
    w
}

/// Peak video bitrate (kb/s) that keeps the output under the preset's size limit, when the
/// estimate says it would not fit; 5 % headroom for the container.
pub fn maxrate_for_limit(
    preset: &Preset,
    est: &Estimate,
    duration_s: f64,
    audio_kbps: Option<u32>,
) -> Option<u32> {
    let max = preset.limits.as_ref()?.max_bytes?;
    if est.bytes <= max || duration_s <= 0.0 {
        return None;
    }
    let total_kbps = max as f64 * 8.0 / 1000.0 / duration_s * 0.95;
    let video = total_kbps - f64::from(audio_kbps.unwrap_or(0));
    Some(video.max(100.0) as u32)
}

/// Warnings for output folders whose free space is below the summed estimates × 1.2. `free`
/// returns the space available for a folder (`None` = unknown, no warning).
pub fn check_space(needs: &BTreeMap<PathBuf, u64>, free: impl Fn(&Path) -> Option<u64>) -> Vec<String> {
    needs
        .iter()
        .filter_map(|(dir, bytes)| {
            let want = (*bytes as f64 * SPACE_MARGIN) as u64;
            let have = free(dir)?;
            (have < want).then(|| {
                format!(
                    "{} has {} free but these outputs need about {}",
                    dir.display(),
                    human_bytes(have),
                    human_bytes(want)
                )
            })
        })
        .collect()
}

/// Free space for `dir`, measured at its nearest existing ancestor (outputs folders are created
/// at run time).
pub fn free_space(dir: &Path) -> Option<u64> {
    let existing = dir.ancestors().find(|a| a.is_dir())?;
    fs4::available_space(existing).ok()
}

pub fn human_bytes(b: u64) -> String {
    let b = b as f64;
    if b >= 1e9 {
        format!("{:.2} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else {
        format!("{:.0} kB", b / 1e3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::PresetRegistry;

    fn preset(id: &str) -> Preset {
        PresetRegistry::builtin().get(id).unwrap().clone()
    }

    #[test]
    fn size_and_time_follow_the_formula() {
        let p = preset("phone-fill");
        let e = estimate(&p, 30.0, 60.0, Some(192), 45.0);
        // 0.06 × 1080 × 2340 × 30 × 60 / 8 + 192 kb/s × 60 s
        let video: f64 = f64::from(p.est_bpp) * 1080.0 * 2340.0 * 30.0 * 60.0 / 8.0;
        assert_eq!(e.bytes, (video + 192_000.0 / 8.0 * 60.0).round() as u64);
        assert!((e.secs - 40.0).abs() < 1e-9, "1800 frames at 45 fps");
        assert!(cpu_fps(1080, 2340) < cpu_fps(1080, 1920));
    }

    #[test]
    fn reddit_limits_warn_and_cap_the_bitrate() {
        let p = preset("reddit-916");
        let small = estimate(&p, 30.0, 60.0, Some(192), 50.0);
        assert!(limit_warnings(&p, &small, 60.0).is_empty());
        assert_eq!(maxrate_for_limit(&p, &small, 60.0, Some(192)), None);
        // 20 minutes at 60 fps: over both limits.
        let long = estimate(&p, 60.0, 1200.0, Some(192), 50.0);
        let w = limit_warnings(&p, &long, 1200.0);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w[0].contains("exceeds the Reddit 9:16 limit of 1.07 GB"), "{w:?}");
        assert!(w[1].contains("duration 1200 s"), "{w:?}");
        let cap = maxrate_for_limit(&p, &long, 1200.0, Some(192)).unwrap();
        assert!((6_000..7_000).contains(&cap), "1 GiB over 20 min ≈ 6.8 Mb/s total, got {cap}");
        assert!(
            limit_warnings(&preset("phone-fill"), &long, 1200.0).is_empty(),
            "no limits on phone presets"
        );
    }

    #[test]
    fn free_space_shortfall_warns_per_folder() {
        let needs = BTreeMap::from([(PathBuf::from("/a"), 1_000_000), (PathBuf::from("/b"), 1_000_000)]);
        let w =
            check_space(&needs, |d| if d == Path::new("/a") { Some(1_100_000) } else { Some(10_000_000) });
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].starts_with("/a has 1.1 MB free"), "{w:?}");
        assert!(check_space(&needs, |_| None).is_empty(), "unknown free space is not a warning");
        assert!(
            free_space(Path::new("/definitely/not/here/yet")).is_some(),
            "measured at an existing ancestor"
        );
    }
}
