//! T066 (002 US3): tier choice from the capability record and the one-time automatic fallback.
//! A GPU decision pinned to a device that does not exist behaves like a GPU that vanished since
//! the check (dock, driver), so this runs on any machine.

mod common;

use std::path::Path;

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::{Event, JobMeasure, JobOutcome};
use rmedia_core::plan::{Caps, JobSpec};
use rmedia_core::preset::{DEFAULT_PRESET, PresetRegistry};
use rmedia_core::request::Options;
use rmedia_core::upscale::capability::{
    CapabilityRecord, CapabilityStore, CheckMode, DeviceKind, GpuDevice, ProbeEnv, TierInfo, ensure,
};
use rmedia_core::upscale::tier::Tier;
use rmedia_core::upscale::{Reframe, TierChoice, UpscaleOptions};
use rmedia_testkit::{Fixture, fixture, probe_json, require_engine};
use tokio_util::sync::CancellationToken;

/// A record that claims a working GPU at index 99, stored in `state` and put in the caps.
fn phantom_gpu(state: &Path) -> Caps {
    let mut r = CapabilityRecord { fingerprint: "ab".repeat(32), ..CapabilityRecord::default() };
    r.tiers.gpu = TierInfo { available: true, device: Some(99), fps: Some(40.0), ..TierInfo::default() };
    r.devices.push(GpuDevice {
        index: 99,
        name: "Undocked GPU".into(),
        kind: DeviceKind::Discrete,
        device_id: "0x1234".into(),
        usable: true,
        reason: None,
    });
    let store = CapabilityStore::in_state_dir(state);
    store.update(|_| r.clone()).unwrap();
    let (_, mut caps) = engine();
    caps.capability = Some(r);
    caps
}

fn opts(out: &Path, tier: TierChoice) -> Options {
    let u = UpscaleOptions { tier, reframe: Reframe::Center, ..UpscaleOptions::default() };
    Options { upscale: Some(u), output_dir: Some(out.to_path_buf()), ..Options::default() }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_automatic_gpu_failure_retries_on_cpu_once_and_is_remembered() {
    require_engine!("libx265", "zscale", "cas", "libplacebo");
    let d = tempfile::tempdir().unwrap();
    let state = d.path().join("state");
    let caps = phantom_gpu(&state);
    let (tools, _) = engine();
    let src = fixture(Fixture::DegradedPortrait);
    let p = make_plan(
        &request(&src, vec![clip(0.0, EndSpec::EndOfMedia)], opts(d.path(), TierChoice::Auto)),
        &tools,
        &caps,
    )
    .await;
    let JobSpec::Upscale(u) = &p.jobs[0].spec else { panic!() };
    assert_eq!((u.up.tier.tier, u.up.tier.device), (Tier::Gpu, Some(99)), "{}", u.up.tier.reason);
    let argv = rmedia_core::plan::ffargs::display_args(&p.jobs[0].args()).join(" ");
    assert!(
        argv.contains("-init_hw_device vulkan=vk:99 -filter_hw_device vk") && argv.contains("libplacebo="),
        "{argv}"
    );

    let (summary, events) = run_plan(p, tools.clone(), &state).await;
    let JobOutcome::Succeeded { output, measure } = &summary.jobs[0].outcome else {
        panic!("{:?}", summary.jobs[0].outcome)
    };
    assert!(
        matches!(measure, JobMeasure::Upscale { tier_used: Tier::Cpu, tier_fallback: true, .. }),
        "{measure:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::JobWarning { message, .. } if message.contains("retried on CPU")))
    );
    let fractions: Vec<f64> = events
        .iter()
        .filter_map(|e| if let Event::JobProgress { fraction, .. } = e { Some(*fraction) } else { None })
        .collect();
    assert!(
        fractions.windows(2).all(|w| w[1] >= w[0]),
        "progress never moves back on the retry: {fractions:?}"
    );
    let v = &probe_json(output)["streams"][0];
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(1080), Some(2340)));

    // SC-008: the failure is remembered; the next automatic choice is the CPU.
    let stored = CapabilityStore::in_state_dir(&state).load().unwrap();
    let demotion = stored.is_demoted(Tier::Gpu).expect("the GPU is demoted");
    assert!(!demotion.error.contains('/'), "no paths in the record: {}", demotion.error);
    let mut caps = caps;
    caps.capability = Some(stored);
    let next = make_plan(
        &request(&src, vec![clip(0.0, EndSpec::EndOfMedia)], opts(&d.path().join("b"), TierChoice::Auto)),
        &tools,
        &caps,
    )
    .await;
    let JobSpec::Upscale(u) = &next.jobs[0].spec else { panic!() };
    assert_eq!(u.up.tier.tier, Tier::Cpu);
    assert!(u.up.tier.reason.contains("failed earlier"), "{}", u.up.tier.reason);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forced_gpu_never_falls_back() {
    require_engine!("libx265", "zscale", "cas", "libplacebo");
    let d = tempfile::tempdir().unwrap();
    let state = d.path().join("state");
    let caps = phantom_gpu(&state);
    let (tools, _) = engine();
    let p = make_plan(
        &request(
            &fixture(Fixture::TinySource),
            vec![clip(0.0, EndSpec::EndOfMedia)],
            opts(d.path(), TierChoice::Gpu),
        ),
        &tools,
        &caps,
    )
    .await;
    let (summary, _) = run_plan(p, tools, &state).await;
    match &summary.jobs[0].outcome {
        JobOutcome::Failed { error } => assert_eq!(error.code, "ENGINE_FAILED"),
        other => panic!("a forced GPU must fail, not fall back: {other:?}"),
    }
    assert!(
        CapabilityStore::in_state_dir(&state).load().unwrap().demoted.is_empty(),
        "forced failures are not demotions"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tiers_the_machine_cannot_run_are_planning_errors() {
    require_engine!("libx265", "zscale", "cas");
    let d = tempfile::tempdir().unwrap();
    let (tools, mut caps) = engine();
    let mut r = CapabilityRecord::default();
    r.tiers.gpu.reason = Some("no Vulkan device".into());
    caps.capability = Some(r);
    let p = make_plan(
        &request(
            &fixture(Fixture::TinySource),
            vec![clip(0.0, EndSpec::EndOfMedia)],
            opts(d.path(), TierChoice::Gpu),
        ),
        &tools,
        &caps,
    )
    .await;
    assert_eq!(p.issues[0].code.as_str(), "TIER_UNAVAILABLE");
    assert!(p.issues[0].message.contains("no Vulkan device"), "{}", p.issues[0].message);
    let auto = make_plan(
        &request(
            &fixture(Fixture::TinySource),
            vec![clip(0.0, EndSpec::EndOfMedia)],
            opts(d.path(), TierChoice::Auto),
        ),
        &tools,
        &caps,
    )
    .await;
    let JobSpec::Upscale(u) = &auto.jobs[0].spec else { panic!() };
    assert_eq!(u.up.tier.tier, Tier::Cpu);
    assert!(u.up.tier.reason.contains("no Vulkan device"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_check_cut_short_is_never_stored() {
    rmedia_testkit::require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    let store = CapabilityStore::in_state_dir(d.path());
    let (tools, _) = engine();
    let env = || ProbeEnv::new(PresetRegistry::builtin().get(DEFAULT_PRESET).cloned().unwrap());
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(ensure(&tools, &store, &env(), CheckMode::Recheck, &cancelled).await.is_none());
    // A budget too small for the self-tests: whatever ran is used once, nothing is stored.
    let tiny = ProbeEnv { budget: std::time::Duration::from_millis(1), ..env() };
    if let Some(r) = ensure(&tools, &store, &tiny, CheckMode::Recheck, &CancellationToken::new()).await {
        assert!(r.tiers.cpu.fps.is_none(), "{r:?}");
    }
    assert!(!store.path().exists(), "an inconclusive check is not stored");
    // Cached mode never runs the self-tests.
    assert!(ensure(&tools, &store, &env(), CheckMode::Cached, &CancellationToken::new()).await.is_none());
    assert!(!store.path().exists());
}
