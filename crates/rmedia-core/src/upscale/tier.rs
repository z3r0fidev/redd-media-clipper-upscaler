//! Quality tiers and tier selection (research R9).

use serde::{Deserialize, Serialize};

use super::TierChoice;
use super::capability::CapabilityRecord;

/// The scaler a job runs with: classical CPU, classical GPU (Vulkan/libplacebo) or AI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Tier {
    Cpu,
    Gpu,
    Ai,
}

/// The tier a job will use, and why (shown in dry runs and reports).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TierDecision {
    pub tier: Tier,
    pub reason: String,
    /// A better tier the user could choose (AI for low-resolution sources; never chosen
    /// automatically, research R9).
    pub suggest: Option<Tier>,
    /// Forced by the user: never falls back (FR-008).
    pub forced: bool,
    /// The Vulkan device of the GPU and AI tiers.
    pub device: Option<u32>,
}

impl TierDecision {
    pub fn cpu(reason: impl Into<String>) -> TierDecision {
        TierDecision { tier: Tier::Cpu, reason: reason.into(), suggest: None, forced: false, device: None }
    }
}

/// Chooses the tier for a source of `display` size (research R9). `auto` takes the GPU when the
/// capability record says it works, it is not slower than the CPU and it has not failed since; it never takes AI by itself but
/// suggests it for sources under 720p when AI is available. A forced tier the machine cannot run
/// is an error (`TIER_UNAVAILABLE`); a forced tier is used even after a failure.
pub fn select(
    choice: TierChoice,
    record: Option<&CapabilityRecord>,
    display: (u32, u32),
) -> Result<TierDecision, String> {
    let ai_ok = record.is_some_and(|r| r.tiers.ai.available);
    let suggest = (ai_ok && choice != TierChoice::Ai && display.0.min(display.1) < 720)
        .then_some(super::tier::Tier::Ai);
    let gpu_name = |r: &CapabilityRecord| {
        r.tiers.gpu.device.and_then(|i| r.device(i)).map_or_else(|| "the GPU".to_owned(), |d| d.name.clone())
    };
    // SC-003: auto takes the GPU only when its self-test is at least as fast as the CPU's (the
    // encoder dominates both; on integrated GPUs the upload can cost more than the scaling saves).
    let slower = |r: &CapabilityRecord| match (r.tiers.gpu.fps, r.tiers.cpu.fps) {
        (Some(g), Some(c)) if g < c => Some((g, c)),
        _ => None,
    };
    let decision = match (choice, record) {
        (TierChoice::Auto, None) => TierDecision::cpu("auto: no GPU check has run; the classical CPU tier"),
        (TierChoice::Auto, Some(r)) if r.tiers.gpu.available => match r.is_demoted(Tier::Gpu) {
            Some(d) => {
                TierDecision::cpu(format!("auto: the GPU tier failed earlier ({}); the CPU tier", d.error))
            }
            None if slower(r).is_some() => {
                let (g, c) = slower(r).unwrap_or_default();
                TierDecision::cpu(format!(
                    "auto: {} measured slower than the CPU here ({g:.0} vs {c:.0} fps); the CPU tier",
                    gpu_name(r)
                ))
            }
            None => TierDecision {
                tier: Tier::Gpu,
                reason: format!("auto: {} passed the self-test", gpu_name(r)),
                suggest: None,
                forced: false,
                device: r.tiers.gpu.device,
            },
        },
        (TierChoice::Auto, Some(r)) => TierDecision::cpu(format!(
            "auto: {}; the classical CPU tier",
            r.tiers.gpu.reason.as_deref().unwrap_or("no usable GPU")
        )),
        (TierChoice::Cpu, _) => TierDecision { forced: true, ..TierDecision::cpu("forced: cpu") },
        (forced, record) => {
            let (tier, info) = match (forced, record) {
                (TierChoice::Gpu, Some(r)) => (Tier::Gpu, Some(&r.tiers.gpu)),
                (TierChoice::Ai, Some(r)) => (Tier::Ai, Some(&r.tiers.ai)),
                (TierChoice::Gpu, None) => (Tier::Gpu, None),
                _ => (Tier::Ai, None),
            };
            let name = format!("{tier:?}").to_lowercase();
            match info {
                Some(i) if i.available => TierDecision {
                    tier,
                    reason: format!("forced: {name}"),
                    suggest: None,
                    forced: true,
                    device: i.device,
                },
                Some(i) => {
                    return Err(format!(
                        "the {name} tier is not available on this machine: {}",
                        i.reason.as_deref().unwrap_or("not detected")
                    ));
                }
                None => {
                    return Err(format!(
                        "the {name} tier is not available on this machine: no GPU check has run"
                    ));
                }
            }
        }
    };
    Ok(TierDecision { suggest: decision.suggest.or(suggest), ..decision })
}

/// A forced AI job predicted to take more than this many times the clip's length gets a
/// warning with the estimate (research R13).
pub const AI_SLOW_FACTOR: f64 = 10.0;

/// The warning for a forced AI job of `clip_secs` predicted to take `predicted_secs`, if it is
/// that slow: "too slow: estimated 45 min for this clip".
pub fn ai_too_slow(predicted_secs: f64, clip_secs: f64) -> Option<String> {
    (clip_secs > 0.0 && predicted_secs > AI_SLOW_FACTOR * clip_secs).then(|| {
        let est = if predicted_secs < 90.0 {
            format!("{} s", predicted_secs.round())
        } else {
            format!("{} min", (predicted_secs / 60.0).round())
        };
        format!("too slow: estimated {est} for this clip on the AI tier (the GPU tier is much faster)")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upscale::capability::{Demotion, TierInfo};

    fn record(gpu: bool, ai: bool) -> CapabilityRecord {
        let mut r = CapabilityRecord::default();
        r.tiers.gpu = if gpu {
            TierInfo { available: true, device: Some(0), ..Default::default() }
        } else {
            TierInfo { reason: Some("no Vulkan device".into()), ..Default::default() }
        };
        r.tiers.ai.available = ai;
        r
    }

    #[test]
    fn selection_table() {
        let src = (480, 854);
        let gpu = record(true, false);
        let d = select(TierChoice::Auto, Some(&gpu), src).unwrap();
        assert_eq!((d.tier, d.device, d.forced), (Tier::Gpu, Some(0), false));
        assert!(d.reason.starts_with("auto: the GPU passed"), "{}", d.reason);
        let none = select(TierChoice::Auto, Some(&record(false, false)), src).unwrap();
        assert_eq!(none.tier, Tier::Cpu);
        assert!(none.reason.contains("no Vulkan device"), "{}", none.reason);
        assert_eq!(select(TierChoice::Auto, None, src).unwrap().tier, Tier::Cpu);
        let mut demoted = gpu.clone();
        demoted.demoted.push(Demotion { tier: Tier::Gpu, error: "vk lost".into(), at: "x".into() });
        let after = select(TierChoice::Auto, Some(&demoted), src).unwrap();
        assert_eq!(after.tier, Tier::Cpu, "SC-008: a failed GPU is not chosen again");
        assert!(after.reason.contains("vk lost"));
        let forced = select(TierChoice::Gpu, Some(&demoted), src).unwrap();
        assert_eq!((forced.tier, forced.forced), (Tier::Gpu, true), "forcing still uses it");
        assert!(
            select(TierChoice::Gpu, Some(&record(false, false)), src)
                .unwrap_err()
                .contains("no Vulkan device")
        );
        assert!(select(TierChoice::Gpu, None, src).is_err());
        assert!(select(TierChoice::Ai, Some(&gpu), src).unwrap_err().contains("no AI check has run"));
        let cpu = select(TierChoice::Cpu, None, src).unwrap();
        assert_eq!((cpu.tier, cpu.forced), (Tier::Cpu, true));
    }

    #[test]
    fn auto_skips_a_gpu_that_measured_slower_than_the_cpu() {
        let mut r = record(true, false);
        (r.tiers.gpu.fps, r.tiers.cpu.fps) = (Some(29.0), Some(35.0));
        let d = select(TierChoice::Auto, Some(&r), (480, 854)).unwrap();
        assert_eq!(d.tier, Tier::Cpu);
        assert!(d.reason.contains("measured slower than the CPU here (29 vs 35 fps)"), "{}", d.reason);
        assert_eq!(
            select(TierChoice::Gpu, Some(&r), (480, 854)).unwrap().tier,
            Tier::Gpu,
            "forcing still works"
        );
        (r.tiers.gpu.fps, r.tiers.cpu.fps) = (Some(35.0), Some(35.0));
        assert_eq!(select(TierChoice::Auto, Some(&r), (480, 854)).unwrap().tier, Tier::Gpu);
    }

    #[test]
    fn auto_never_takes_ai_but_suggests_it_below_720p() {
        let ai = record(true, true);
        let d = select(TierChoice::Auto, Some(&ai), (1280, 719)).unwrap();
        assert_eq!((d.tier, d.suggest), (Tier::Gpu, Some(Tier::Ai)));
        assert_eq!(select(TierChoice::Auto, Some(&ai), (1280, 720)).unwrap().suggest, None);
        assert_eq!(select(TierChoice::Auto, Some(&record(true, false)), (480, 854)).unwrap().suggest, None);
        assert_eq!(select(TierChoice::Ai, Some(&ai), (480, 854)).unwrap().suggest, None);
        // Even with no GPU tier and an AI tier that is available, auto stays on the CPU.
        let only_ai = record(false, true);
        let d = select(TierChoice::Auto, Some(&only_ai), (320, 240)).unwrap();
        assert_eq!((d.tier, d.suggest), (Tier::Cpu, Some(Tier::Ai)));
    }

    #[test]
    fn forced_ai_reports_why_it_is_unavailable_and_warns_when_too_slow() {
        let mut r = record(true, false);
        r.tiers.ai.reason = Some("too slow on Intel(R) Graphics (integrated): about 105 min".into());
        let e = select(TierChoice::Ai, Some(&r), (480, 854)).unwrap_err();
        assert!(e.contains("about 105 min"), "{e}");
        let d = select(TierChoice::Ai, Some(&record(true, true)), (480, 854)).unwrap();
        assert_eq!((d.tier, d.forced), (Tier::Ai, true));
        assert_eq!(ai_too_slow(100.0, 10.0), None, "10x the clip is the limit");
        assert_eq!(
            ai_too_slow(2700.0, 60.0).as_deref(),
            Some("too slow: estimated 45 min for this clip on the AI tier (the GPU tier is much faster)")
        );
        assert!(ai_too_slow(80.0, 5.0).unwrap().contains("estimated 80 s"));
        assert_eq!(ai_too_slow(80.0, 0.0), None);
    }
}
