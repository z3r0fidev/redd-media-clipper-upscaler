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

/// T037 (003 US1, SC-005): the default `reddit-post` GIF of `Land6` scores at least as well as a
/// plain one-pass `ffmpeg -vf fps,scale` GIF of the same width and frame rate, on VMAF against
/// the source frames. The sizes are recorded: comparing at the same size needs the budget
/// search (US2), which then sets the budget to the plain GIF's size.
#[tokio::test(flavor = "multi_thread")]
async fn the_default_gif_beats_a_plain_one_pass_gif() {
    require_engine!("palettegen", "paletteuse", "libvmaf");
    let d = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::Land6);
    let opts = Options {
        gif: Some(rmedia_core::gif::GifOptions::default()),
        output_dir: Some(d.path().to_path_buf()),
        ..Options::default()
    };
    let (tools, caps) = engine();
    let p = make_plan(&request(&src, vec![clip(0.0, EndSpec::EndOfMedia)], opts), &tools, &caps).await;
    let (summary, _) = run_plan(p, tools, d.path()).await;
    let ours = match &summary.jobs[0].outcome {
        JobOutcome::Succeeded { output, .. } => output.clone(),
        other => panic!("GIF failed: {other:?}"),
    };
    let plain = d.path().join("plain.gif");
    rmedia_testkit::plain_gif(&src, 720, 15, &plain);
    let (a, b) = (rmedia_testkit::vmaf_gif(&ours, &src, 15), rmedia_testkit::vmaf_gif(&plain, &src, 15));
    let (sa, sb) = (std::fs::metadata(&ours).unwrap().len(), std::fs::metadata(&plain).unwrap().len());
    eprintln!("SC-005: reddit-post VMAF {a:.2} ({sa} bytes), plain one-pass {b:.2} ({sb} bytes)");
    assert!(a >= b, "the default GIF ({a:.2}) scores below a plain one-pass GIF ({b:.2})");
}
