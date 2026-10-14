//! T029 (002 US1, SC-002): the CPU tier beats a plain bicubic resize on degraded 480p and 360p
//! portrait sources, measured against the clean original. The automated bar is +1.5 VMAF on
//! these synthetic fixtures (owner decision 2026-10-14): the restrained filters measured +2.8
//! at 480p, and passing +3 here would need sharpening that visibly rings. The +3 target is
//! checked on real footage by the owner (checklists/validation.md).
//!
//! Run with `reddit-916` and `--reframe center`: the sources are 9:16 like the preset, so the
//! output shows the same picture as the reference (no crop) and VMAF compares like with like.

mod common;

/// SC-002 CPU-tier margin on the synthetic fixtures (VMAF points over bicubic).
const MARGIN: f64 = 1.5;

use std::path::{Path, PathBuf};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::JobOutcome;
use rmedia_core::request::Options;
use rmedia_core::upscale::{Reframe, UpscaleOptions};
use rmedia_testkit::{Fixture, bicubic_baseline, fixture, require_engine, vmaf};

async fn cpu_tier(src: &Path, out: &Path) -> PathBuf {
    let u =
        UpscaleOptions { preset: "reddit-916".into(), reframe: Reframe::Center, ..UpscaleOptions::default() };
    let opts = Options { upscale: Some(u), output_dir: Some(out.to_path_buf()), ..Options::default() };
    let (tools, caps) = engine();
    let p = make_plan(&request(src, vec![clip(0.0, EndSpec::EndOfMedia)], opts), &tools, &caps).await;
    let (summary, _) = run_plan(p, tools, out).await;
    match &summary.jobs[0].outcome {
        JobOutcome::Succeeded { output, .. } => output.clone(),
        other => panic!("upscale failed: {other:?}"),
    }
}

async fn margin(degraded: Fixture) -> (f64, f64) {
    let d = tempfile::tempdir().unwrap();
    let src = fixture(degraded);
    let clean = fixture(Fixture::CleanPortrait);
    let ours = cpu_tier(&src, d.path()).await;
    let baseline = bicubic_baseline(&src, 1080, 1920, &d.path().join("bicubic.mp4"));
    let (a, b) = (vmaf(&ours, &clean), vmaf(&baseline, &clean));
    eprintln!("{degraded:?}: CPU tier VMAF {a:.2}, bicubic {b:.2}, margin {:.2}", a - b);
    (a, b)
}

#[tokio::test(flavor = "multi_thread")]
async fn cpu_tier_beats_bicubic_at_480p() {
    require_engine!("libx264", "zscale", "cas", "hqdn3d", "libvmaf");
    let (ours, bicubic) = margin(Fixture::DegradedPortrait).await;
    assert!(ours >= bicubic + MARGIN, "SC-002: {ours:.2} vs bicubic {bicubic:.2}");
}

#[tokio::test(flavor = "multi_thread")]
async fn cpu_tier_beats_bicubic_at_360p() {
    require_engine!("libx264", "zscale", "cas", "hqdn3d", "libvmaf");
    let (ours, bicubic) = margin(Fixture::DegradedPortrait360).await;
    assert!(ours >= bicubic + MARGIN, "SC-002: {ours:.2} vs bicubic {bicubic:.2}");
}
