//! Feature 005, US4: device shapes and batches (tasks T034, T036). A fit takes an upscale
//! preset's shape and rate (research R7); batches carry fits at every level.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::{JobMeasure, JobOutcome};
use rmedia_core::fit::plan::Mode as FitMode;
use rmedia_core::fit::{FitOptions, FitOutcome};
use rmedia_core::request::Options;
use rmedia_core::upscale::{Reframe, TierChoice, UpscaleOptions};
use rmedia_testkit::{Fixture, decode_errors, fixture, probe_json};

fn engine_ok() -> bool {
    rmedia_testkit::require_engine(&["libx264", "aac", "zscale"])
}

fn reddit_916(out: &Path, cap: u64) -> Options {
    let u = UpscaleOptions {
        preset: "reddit-916".into(),
        reframe: Reframe::Blur,
        tier: TierChoice::Cpu,
        ..UpscaleOptions::default()
    };
    Options {
        upscale: Some(u),
        fit: Some(FitOptions { max_size: Some(cap), ..FitOptions::default() }),
        output_dir: Some(out.to_path_buf()),
        ..Options::default()
    }
}

/// `(codec, profile, pix_fmt, width, height, colour transfer)` of the video and the audio codec.
fn shape(p: &Path) -> (String, String, String, u64, u64, String, String) {
    let j = probe_json(p);
    let streams = j["streams"].as_array().unwrap();
    let v = streams.iter().find(|s| s["codec_type"] == "video").unwrap();
    let a =
        streams.iter().find(|s| s["codec_type"] == "audio").map_or("", |a| a["codec_name"].as_str().unwrap());
    let s = |k: &str| v[k].as_str().unwrap_or("").to_owned();
    (
        s("codec_name"),
        s("profile"),
        s("pix_fmt"),
        v["width"].as_u64().unwrap(),
        v["height"].as_u64().unwrap(),
        s("color_transfer"),
        a.to_owned(),
    )
}

async fn run_upscale_fit(cap: u64, out: &Path) -> (PathBuf, FitOutcome) {
    let (tools, caps) = engine();
    let src = fixture(Fixture::FitBusy);
    let req = request(&src, vec![clip(2.0, EndSpec::At(secs(8.0)))], reddit_916(out, cap));
    let p = make_plan(&req, &tools, &caps).await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    let (summary, _) = run_plan(p, tools, &out.join("state")).await;
    match &summary.jobs[0].outcome {
        JobOutcome::Succeeded { output, measure: JobMeasure::Upscale { fit: Some(f), .. } } => {
            (output.clone(), f.clone())
        }
        other => panic!("{other:?}"),
    }
}

/// T034: `reddit-916` + `reddit-video` gives a 9:16 H.264 SDR MP4 with sound within the cap; a
/// tighter cap lowers the size below the preset's.
#[tokio::test(flavor = "multi_thread")]
async fn an_upscale_to_a_device_shape_fits_its_cap() {
    if !engine_ok() {
        return;
    }
    let out = tempfile::tempdir().unwrap();
    let cap = 3_000_000;
    let (file, f) = run_upscale_fit(cap, out.path()).await;
    let bytes = std::fs::metadata(&file).unwrap().len();
    assert_eq!(f.bytes, bytes);
    assert!(bytes <= cap && bytes as f64 >= 0.9 * cap as f64, "{bytes} of {cap}");
    assert_eq!((f.mode, f.size), (FitMode::Budget, (1080, 1920)), "{f:?}");
    let (codec, profile, pix, w, h, trc, audio) = shape(&file);
    assert_eq!(
        (codec.as_str(), profile.as_str(), pix.as_str(), w, h, trc.as_str(), audio.as_str()),
        ("h264", "High", "yuv420p", 1080, 1920, "bt709", "aac")
    );
    assert!(decode_errors(&file).is_empty());

    // A tighter cap: one step down the ladder, after the preset's graph.
    let out = tempfile::tempdir().unwrap();
    let cap = 1_200_000;
    let (file, f) = run_upscale_fit(cap, out.path()).await;
    assert_eq!(f.size, (810, 1440), "{f:?}");
    let (_, _, _, w, h, _, _) = shape(&file);
    assert_eq!((w, h), (810, 1440));
    assert!(std::fs::metadata(&file).unwrap().len() <= cap);
}

/// T034: `--hdr keep` cannot be fitted (a fit is SDR H.264).
#[tokio::test(flavor = "multi_thread")]
async fn hdr_keep_and_fit_are_refused_together() {
    if !engine_ok() {
        return;
    }
    let out = tempfile::tempdir().unwrap();
    let (tools, caps) = engine();
    let mut o = reddit_916(out.path(), 3_000_000);
    let u = o.upscale.as_mut().unwrap();
    // An HDR-capable preset (reddit-916 itself refuses to keep HDR).
    u.preset = "phone-fill".into();
    u.hdr = rmedia_core::upscale::HdrMode::Keep;
    let req = request(&fixture(Fixture::FitShort), vec![clip(0.0, EndSpec::At(secs(1.0)))], o);
    let p = make_plan(&req, &tools, &caps).await;
    let e = p
        .issues
        .iter()
        .find(|i| i.code == rmedia_core::error::IssueCode::InvalidInput)
        .unwrap_or_else(|| panic!("{:?}", p.issues));
    assert!(e.message.contains("--hdr keep"), "{}", e.message);
}

/// T036: a batch with the target at the top level, one override and one `enabled: false`; every
/// fitted output is within its own cap. Smart mode does not apply to fitted clips.
#[tokio::test(flavor = "multi_thread")]
async fn a_batch_fits_each_clip_to_its_own_cap() {
    use rmedia_core::error::IssueCode;
    use rmedia_core::plan::JobSpec;
    if !engine_ok() {
        return;
    }
    let out = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::FitBusy);
    let text = serde_json::json!({
        "version": 1,
        "defaults": { "mode": "smart", "output_dir": out.path(),
                      "fit": { "target": "reddit-video", "max_size": "3MB" } },
        "videos": [ { "input": src, "clips": [
            { "start": 0, "end": 3 },
            { "start": 3, "end": 6, "options": { "fit": { "max_size": 1_000_000 } } },
            { "start": 6, "end": 9, "options": { "fit": { "enabled": false } } },
            { "start": 9, "end": 12 } ] } ]
    })
    .to_string();
    let req =
        rmedia_core::input::manifest_json::parse(&out.path().join("m.json"), &text, &Options::default())
            .unwrap();
    let (tools, caps) = engine();
    let p = make_plan(&req, &tools, &caps).await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    for (i, j) in p.jobs.iter().enumerate() {
        let JobSpec::Clip(c) = &j.spec else { panic!() };
        let smart_noted = j.notices.iter().any(|n| n.code == IssueCode::SmartNotApplied);
        assert_eq!((c.fit.is_some(), smart_noted), (i != 2, i != 2), "job {i}: {:?}", j.notices);
        if c.fit.is_some() {
            assert_eq!(c.mode, rmedia_core::request::Mode::Accurate, "job {i}");
        }
    }
    let (summary, _) = run_plan(p, tools, &out.path().join("state")).await;
    let caps_of = [Some(3_000_000), Some(1_000_000), None, Some(3_000_000)];
    for (i, (job, cap)) in summary.jobs.iter().zip(caps_of).enumerate() {
        let JobOutcome::Succeeded { output, measure: JobMeasure::Clip { fit, .. } } = &job.outcome else {
            panic!("job {i}: {:?}", job.outcome)
        };
        let bytes = std::fs::metadata(output).unwrap().len();
        match (cap, fit) {
            (Some(cap), Some(f)) => assert!(f.cap == cap && bytes <= cap, "job {i}: {bytes} > {cap}"),
            (None, None) => {}
            other => panic!("job {i}: {other:?}"),
        }
    }
}
