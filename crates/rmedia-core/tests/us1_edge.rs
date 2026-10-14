//! T030 (002 US1): edge-case sources, each output checked with ffprobe — rotation, anamorphic
//! pixels, interlacing, HDR to SDR and kept HDR, variable and high frame rates, tiny sources.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::JobOutcome;
use rmedia_core::request::Options;
use rmedia_core::upscale::{HdrMode, Reframe, UpscaleOptions};
use rmedia_testkit::{Fixture, fixture, probe_json, require_engine};

async fn upscale(
    kind: Fixture,
    out: &Path,
    edit: impl FnOnce(&mut UpscaleOptions),
) -> (PathBuf, Vec<String>) {
    let mut u =
        UpscaleOptions { preset: "reddit-916".into(), reframe: Reframe::Center, ..UpscaleOptions::default() };
    edit(&mut u);
    let opts = Options { upscale: Some(u), output_dir: Some(out.to_path_buf()), ..Options::default() };
    let (tools, caps) = engine();
    let p =
        make_plan(&request(&fixture(kind), vec![clip(0.0, EndSpec::EndOfMedia)], opts), &tools, &caps).await;
    assert!(!p.has_errors(), "{kind:?}: {:?}", p.issues);
    let warnings = p.jobs[0].warnings.clone();
    let (summary, _) = run_plan(p, tools, out).await;
    match &summary.jobs[0].outcome {
        JobOutcome::Succeeded { output, .. } => (output.clone(), warnings),
        other => panic!("{kind:?} did not succeed: {other:?}"),
    }
}

fn video(file: &Path) -> serde_json::Value {
    probe_json(file)["streams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["codec_type"] == "video")
        .unwrap()
        .clone()
}

fn frames(file: &Path) -> u64 {
    let o = std::process::Command::new(rmedia_testkit::ffprobe())
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-count_packets",
            "-show_entries",
            "stream=nb_read_packets",
        ])
        .args(["-of", "csv=p=0"])
        .arg(file)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().parse().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn geometry_sources_come_out_upright_square_and_progressive() {
    require_engine!("libx264", "zscale", "cas", "bwdif");
    let d = tempfile::tempdir().unwrap();
    let (rot, _) = upscale(Fixture::Rotated90, &d.path().join("rot"), |_| {}).await;
    let v = video(&rot);
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(1080), Some(1920)));
    assert!(
        v["side_data_list"]
            .as_array()
            .is_none_or(|l| l.iter().all(|s| s["rotation"].as_f64().unwrap_or(0.0) == 0.0)),
        "no leftover rotation: {v}"
    );

    let (ana, _) = upscale(Fixture::Anamorphic, &d.path().join("ana"), |_| {}).await;
    let v = video(&ana);
    assert_eq!(
        (v["width"].as_u64(), v["height"].as_u64(), v["sample_aspect_ratio"].as_str()),
        (Some(1080), Some(1920), Some("1:1"))
    );

    let (int, _) = upscale(Fixture::Interlaced, &d.path().join("int"), |_| {}).await;
    assert_eq!(video(&int)["field_order"], "progressive");
}

#[tokio::test(flavor = "multi_thread")]
async fn hdr_is_tone_mapped_by_default_and_kept_on_request() {
    require_engine!("libx264", "libx265", "zscale", "cas", "tonemap");
    let d = tempfile::tempdir().unwrap();
    let (sdr, _) = upscale(Fixture::HdrTagged, &d.path().join("sdr"), |_| {}).await;
    let v = video(&sdr);
    assert_eq!(
        (v["color_transfer"].as_str(), v["color_primaries"].as_str(), v["pix_fmt"].as_str()),
        (Some("bt709"), Some("bt709"), Some("yuv420p"))
    );

    let (keep, _) = upscale(Fixture::HdrTagged, &d.path().join("keep"), |u| {
        u.preset = "phone-fill".into();
        u.hdr = HdrMode::Keep;
    })
    .await;
    let v = video(&keep);
    assert_eq!(
        (
            v["codec_name"].as_str(),
            v["pix_fmt"].as_str(),
            v["color_transfer"].as_str(),
            v["color_primaries"].as_str()
        ),
        (Some("hevc"), Some("yuv420p10le"), Some("smpte2084"), Some("bt2020"))
    );

    let (sdr_keep, warnings) = upscale(Fixture::TinySource, &d.path().join("sdrkeep"), |u| {
        u.preset = "phone-fill".into();
        u.hdr = HdrMode::Keep;
    })
    .await;
    let v = video(&sdr_keep);
    assert_eq!(
        (v["pix_fmt"].as_str(), v["color_transfer"].as_str()),
        (Some("yuv420p"), Some("bt709")),
        "SDR never becomes HDR"
    );
    assert!(warnings.iter().any(|w| w.contains("stays SDR")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("very small source")), "{warnings:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn frame_rates_are_kept_or_capped_at_60() {
    require_engine!("libx264", "zscale", "cas");
    let d = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::VfrMp4);
    let (vfr, _) = upscale(Fixture::VfrMp4, &d.path().join("vfr"), |_| {}).await;
    assert_eq!(
        frames(&vfr),
        frames(&src),
        "variable frame timing passes through: no frames added or dropped"
    );

    let (fast, _) = upscale(Fixture::HighFps120, &d.path().join("fast"), |_| {}).await;
    assert_eq!(video(&fast)["avg_frame_rate"], "60/1");
}
