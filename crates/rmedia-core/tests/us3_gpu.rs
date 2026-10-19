//! T067 (002 US3): the GPU tier on a real Vulkan device. Each test skips with a message when the
//! machine has no usable hardware GPU (software renderers are rejected), so CI without a GPU
//! still passes.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::{Event, EventSink, JobMeasure, JobOutcome};
use rmedia_core::exec::{self, RunConfig};
use rmedia_core::plan::{Caps, JobSpec};
use rmedia_core::preset::{DEFAULT_PRESET, PresetRegistry};
use rmedia_core::request::Options;
use rmedia_core::upscale::capability::{CapabilityRecord, CapabilityStore, CheckMode, ProbeEnv, ensure};
use rmedia_core::upscale::tier::Tier;
use rmedia_core::upscale::{Reframe, TierChoice, UpscaleOptions};
use rmedia_testkit::{Fixture, bicubic_baseline, fixture, probe_json, require_engine, vmaf};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Runs the capability check into `state`; `None` (with a message) when there is no usable GPU.
async fn gpu_record(state: &Path) -> Option<CapabilityRecord> {
    let (tools, _) = engine();
    let env = ProbeEnv::new(PresetRegistry::builtin().get(DEFAULT_PRESET).cloned().unwrap());
    let store = CapabilityStore::in_state_dir(state);
    let r = ensure(&tools, &store, &env, CheckMode::Ensure, &CancellationToken::new())
        .await
        .expect("the check finishes");
    if r.tiers.gpu.available {
        Some(r)
    } else {
        eprintln!("skipping: no usable GPU ({:?})", r.tiers.gpu.reason);
        None
    }
}

fn caps_with(r: &CapabilityRecord) -> Caps {
    let (_, mut caps) = engine();
    caps.capability = Some(r.clone());
    caps
}

fn opts(out: &Path, tier: TierChoice, preset: &str) -> Options {
    let u =
        UpscaleOptions { tier, preset: preset.into(), reframe: Reframe::Center, ..UpscaleOptions::default() };
    Options { upscale: Some(u), output_dir: Some(out.to_path_buf()), ..Options::default() }
}

async fn upscale(
    src: &Path,
    out: &Path,
    caps: &Caps,
    tier: TierChoice,
    preset: &str,
) -> (PathBuf, JobMeasure, Duration) {
    let (tools, _) = engine();
    let p =
        make_plan(&request(src, vec![clip(0.0, EndSpec::EndOfMedia)], opts(out, tier, preset)), &tools, caps)
            .await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    let t0 = Instant::now();
    let (summary, _) = run_plan(p, tools, &out.join("state")).await;
    match &summary.jobs[0].outcome {
        JobOutcome::Succeeded { output, measure } => (output.clone(), measure.clone(), t0.elapsed()),
        other => panic!("upscale failed: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_gpu_tier_makes_a_valid_output_and_the_check_is_cached() {
    require_engine!("libx265", "libplacebo");
    let d = tempfile::tempdir().unwrap();
    let state = d.path().join("state");
    let Some(r) = gpu_record(&state).await else { return };

    // An unchanged machine reuses the record: no self-test, same timestamp.
    let t0 = Instant::now();
    let again = gpu_record(&state).await.unwrap();
    assert_eq!(again.checked_at, r.checked_at, "the stored record is reused");
    assert!(t0.elapsed() < Duration::from_secs(10), "cached check took {:?}", t0.elapsed());

    let caps = caps_with(&r);
    let (out, measure, _) =
        upscale(&fixture(Fixture::DegradedPortrait), d.path(), &caps, TierChoice::Gpu, DEFAULT_PRESET).await;
    assert!(
        matches!(measure, JobMeasure::Upscale { tier_used: Tier::Gpu, tier_fallback: false, .. }),
        "{measure:?}"
    );
    let v = &probe_json(&out)["streams"][0];
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(1080), Some(2340)));
    assert_eq!((v["codec_name"].as_str(), v["codec_tag_string"].as_str()), (Some("hevc"), Some("hvc1")));
    assert_eq!(
        (v["color_primaries"].as_str(), v["color_transfer"].as_str(), v["color_space"].as_str()),
        (Some("bt709"), Some("bt709"), Some("bt709"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_blurred_background_reframe_runs_on_the_gpu() {
    require_engine!("libx265", "libplacebo", "gblur");
    let d = tempfile::tempdir().unwrap();
    let Some(r) = gpu_record(&d.path().join("state")).await else { return };
    let (tools, _) = engine();
    let mut o = opts(d.path(), TierChoice::Gpu, DEFAULT_PRESET);
    o.upscale.as_mut().unwrap().reframe = Reframe::Blur;
    let p = make_plan(
        &request(&fixture(Fixture::Hd60Mp4), vec![clip(0.0, EndSpec::At(secs(2.0)))], o),
        &tools,
        &caps_with(&r),
    )
    .await;
    let JobSpec::Upscale(u) = &p.jobs[0].spec else { panic!() };
    assert_eq!(u.up.tier.tier, Tier::Gpu);
    let (summary, _) = run_plan(p, tools, &d.path().join("state")).await;
    let JobOutcome::Succeeded { output, .. } = &summary.jobs[0].outcome else {
        panic!("{:?}", summary.jobs[0].outcome)
    };
    let v = &probe_json(output)["streams"][0];
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(1080), Some(2340)));
}

struct Notify(mpsc::UnboundedSender<Event>);
impl EventSink for Notify {
    fn emit(&self, e: Event) {
        let _ = self.0.send(e);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_gpu_upscale_is_fast_and_clean() {
    require_engine!("libx265", "libplacebo");
    let d = tempfile::tempdir().unwrap();
    let Some(r) = gpu_record(&d.path().join("state")).await else { return };
    let (tools, _) = engine();
    let p = make_plan(
        &request(
            &fixture(Fixture::Hd60Mp4),
            vec![clip(0.0, EndSpec::EndOfMedia)],
            opts(d.path(), TierChoice::Gpu, DEFAULT_PRESET),
        ),
        &tools,
        &caps_with(&r),
    )
    .await;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let cfg = RunConfig { jobs: 1, state_dir: Some(d.path().join("state")) };
    let handle = exec::start(p, tools, cfg, Arc::new(Notify(tx)));
    loop {
        match tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
            Ok(Some(Event::JobProgress { .. })) => break,
            Ok(Some(_)) => continue,
            other => panic!("no progress event: {other:?}"),
        }
    }
    let t0 = Instant::now();
    handle.cancel();
    let summary = handle.wait().await;
    assert!(t0.elapsed() < Duration::from_secs(2), "cancel took {:?}", t0.elapsed());
    assert!(matches!(summary.jobs[0].outcome, JobOutcome::Cancelled), "{:?}", summary.jobs[0].outcome);
    assert!(partials(d.path()).is_empty(), "{:?}", partials(d.path()));
    let demoted = CapabilityStore::in_state_dir(&d.path().join("state")).load().unwrap().demoted;
    assert!(demoted.is_empty(), "a cancel is not a GPU failure");
}

/// SC-003 (run with `--ignored --nocapture`): the GPU tier's quality is within 0.5 VMAF of the
/// CPU tier's, and the automatic choice is never slower than the CPU tier.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "benchmark; run with --ignored"]
async fn gpu_is_not_slower_and_not_worse_than_cpu() {
    require_engine!("libx265", "libx264", "libplacebo", "libvmaf", "zscale", "cas");
    let d = tempfile::tempdir().unwrap();
    let Some(r) = gpu_record(&d.path().join("state")).await else { return };
    let caps = caps_with(&r);
    let low = fixture(Fixture::DegradedPortrait);
    let clean = fixture(Fixture::CleanPortrait);
    let (cpu_out, ..) = upscale(&low, &d.path().join("qc"), &caps, TierChoice::Cpu, "reddit-916").await;
    let (gpu_out, ..) = upscale(&low, &d.path().join("qg"), &caps, TierChoice::Gpu, "reddit-916").await;
    let bicubic = bicubic_baseline(&low, 1080, 1920, &d.path().join("bicubic.mp4"));
    let (c, g, b) = (vmaf(&cpu_out, &clean), vmaf(&gpu_out, &clean), vmaf(&bicubic, &clean));
    eprintln!("VMAF: CPU {c:.2}, GPU {g:.2}, bicubic {b:.2}");
    assert!(g >= c - 0.5, "GPU VMAF {g:.2} more than 0.5 under CPU {c:.2}");
    // The SC-001 source (60 s of 480p30): the low-resolution input this feature is for.
    let src = fixture(Fixture::Portrait480Long);
    let (_, _, cpu_t) = upscale(&src, &d.path().join("cpu"), &caps, TierChoice::Cpu, DEFAULT_PRESET).await;
    let (_, _, gpu_t) = upscale(&src, &d.path().join("gpu"), &caps, TierChoice::Gpu, DEFAULT_PRESET).await;
    let (gf, cf) = (r.tiers.gpu.fps.unwrap_or(0.0), r.tiers.cpu.fps.unwrap_or(0.0));
    eprintln!("SC-003: CPU {cpu_t:.1?}, GPU {gpu_t:.1?} (self-test CPU {cf:.0} fps, GPU {gf:.0} fps)");
    // Auto takes the GPU only when the self-test found it at least as fast; then it must be.
    if gf >= cf {
        assert!(gpu_t <= cpu_t.mul_f64(1.05), "GPU {gpu_t:?} vs CPU {cpu_t:?}");
    } else {
        let (tools, _) = engine();
        let p = make_plan(
            &request(
                &src,
                vec![clip(0.0, EndSpec::EndOfMedia)],
                opts(d.path(), TierChoice::Auto, DEFAULT_PRESET),
            ),
            &tools,
            &caps,
        )
        .await;
        let JobSpec::Upscale(u) = &p.jobs[0].spec else { panic!() };
        assert_eq!(u.up.tier.tier, Tier::Cpu, "a GPU slower than the CPU is not chosen automatically");
    }
}
