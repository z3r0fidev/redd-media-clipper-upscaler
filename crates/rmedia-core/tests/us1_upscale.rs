//! T027 (002 US1): end-to-end upscales on the CPU tier, verified with ffprobe (SC-001's
//! automated part), plus determinism (Principle II) and stream handling (FR-020).

mod common;

use std::path::{Path, PathBuf};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::{Event, JobMeasure, JobOutcome, RunSummary};
use rmedia_core::request::Options;
use rmedia_core::upscale::{Reframe, UpscaleOptions};
use rmedia_testkit::{Fixture, fixture, probe_json, require_engine, sha256};

fn up(out: &Path, edit: impl FnOnce(&mut UpscaleOptions)) -> Options {
    let mut u = UpscaleOptions::default();
    edit(&mut u);
    Options { upscale: Some(u), output_dir: Some(out.to_path_buf()), ..Options::default() }
}

async fn run(src: &Path, opts: Options, state: &Path) -> (RunSummary, Vec<Event>) {
    let (tools, caps) = engine();
    let p = make_plan(&request(src, vec![clip(0.0, EndSpec::EndOfMedia)], opts), &tools, &caps).await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    run_plan(p, tools, state).await
}

fn output(s: &RunSummary) -> (PathBuf, JobMeasure) {
    match &s.jobs[0].outcome {
        JobOutcome::Succeeded { output, measure } => (output.clone(), measure.clone()),
        other => panic!("job did not succeed: {other:?}"),
    }
}

fn stream(file: &Path, kind: &str) -> serde_json::Value {
    probe_json(file)["streams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["codec_type"] == kind)
        .cloned()
        .unwrap_or_default()
}

/// Leftover temp files or scratch directories anywhere under `dir`.
fn leftovers(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if name.contains(".rmedia-") {
            found.push(p.clone());
        }
        if p.is_dir() {
            found.extend(leftovers(&p));
        }
    }
    found
}

#[tokio::test(flavor = "multi_thread")]
async fn phone_fill_is_hvc1_bt709_and_byte_identical_on_a_second_run() {
    require_engine!("libx265", "zscale", "cas", "gblur");
    let d = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::DegradedPortrait);
    let before = sha256(&src);
    let (summary, events) = run(&src, up(&d.path().join("a"), |_| {}), d.path()).await;
    let (out, measure) = output(&summary);
    let v = stream(&out, "video");
    let got = (
        v["width"].as_u64(),
        v["height"].as_u64(),
        v["codec_name"].as_str(),
        v["codec_tag_string"].as_str(),
        v["color_space"].as_str(),
        v["color_primaries"].as_str(),
        v["color_transfer"].as_str(),
        v["color_range"].as_str(),
    );
    assert_eq!(
        got,
        (
            Some(1080),
            Some(2340),
            Some("hevc"),
            Some("hvc1"),
            Some("bt709"),
            Some("bt709"),
            Some("bt709"),
            Some("tv")
        )
    );
    assert_eq!(stream(&out, "audio")["codec_name"], "aac", "AAC audio is copied");
    assert_eq!(sha256(&src), before, "the source must never change");
    let JobMeasure::Upscale { tier_used, tier_fallback, no_upscale_needed, .. } = measure else { panic!() };
    assert_eq!((format!("{tier_used:?}"), tier_fallback, no_upscale_needed), ("Cpu".into(), false, false));
    let stages: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            Event::JobStage { stage, .. } => Some(stage.as_str()),
            _ => None,
        })
        .collect();
    // 9:16 into 9:19.5 overflows a little, so smart reframe analyses first.
    assert_eq!(stages, ["analyze", "render", "verify"]);
    assert!(leftovers(d.path()).is_empty(), "{:?}", leftovers(d.path()));

    let (again, _) = run(&src, up(&d.path().join("b"), |_| {}), d.path()).await;
    assert_eq!(sha256(&output(&again).0), sha256(&out), "same input and options → same bytes (Principle II)");
}

#[tokio::test(flavor = "multi_thread")]
async fn reddit_preset_is_h264_with_faststart_and_large_sources_only_scale_down() {
    require_engine!("libx264", "zscale", "cas");
    let d = tempfile::tempdir().unwrap();
    let opts = up(d.path(), |u| {
        u.preset = "reddit-916".into();
        u.reframe = Reframe::Center;
    });
    let (summary, _) = run(&fixture(Fixture::CleanPortrait), opts, d.path()).await;
    let (out, measure) = output(&summary);
    let v = stream(&out, "video");
    assert_eq!(
        (v["width"].as_u64(), v["height"].as_u64(), v["codec_name"].as_str()),
        (Some(1080), Some(1920), Some("h264"))
    );
    assert_eq!(v["profile"], "High");
    let bytes = std::fs::read(&out).unwrap();
    let pos = |tag: &[u8]| bytes.windows(4).position(|w| w == tag).unwrap();
    assert!(pos(b"moov") < pos(b"mdat"), "+faststart puts the index first");
    assert!(
        matches!(measure, JobMeasure::Upscale { no_upscale_needed: true, .. }),
        "1080x1920 source: scale only"
    );
    assert!(summary.jobs[0].requested.end.as_secs_f64() > 9.9);
}

#[tokio::test(flavor = "multi_thread")]
async fn subtitles_chapters_metadata_and_audio_conversion() {
    require_engine!("libx264", "zscale", "cas", "libopus");
    let d = tempfile::tempdir().unwrap();
    let reddit = |out: &Path| {
        up(out, |u| {
            u.preset = "reddit-916".into();
            u.reframe = Reframe::Center;
        })
    };
    let (subs, _) = run(&fixture(Fixture::MultiMkv), reddit(&d.path().join("subs")), d.path()).await;
    let out = output(&subs).0;
    assert_eq!(stream(&out, "subtitle")["codec_name"], "mov_text", "text subtitles carried as mov_text");

    let (ch, _) = run(&fixture(Fixture::ChaptersMp4), reddit(&d.path().join("ch")), d.path()).await;
    let out = output(&ch).0;
    let probe = std::process::Command::new(rmedia_testkit::ffprobe())
        .args(["-v", "error", "-show_chapters", "-show_format", "-of", "json"])
        .arg(&out)
        .output()
        .unwrap();
    let j: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
    assert_eq!(j["chapters"].as_array().map(Vec::len), Some(2), "chapters kept");
    assert_eq!(j["format"]["tags"]["title"], "rmedia chapters fixture", "metadata kept");

    let (opus, _) = run(&fixture(Fixture::SmallWebm), reddit(&d.path().join("opus")), d.path()).await;
    let a = stream(&output(&opus).0, "audio");
    assert_eq!(a["codec_name"], "aac", "Opus is re-encoded to AAC for mp4");
}
