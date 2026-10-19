//! T076 (002 US4): upscaling from manifests and clip + upscale in one pass, end to end.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::{Event, JobMeasure, JobOutcome, RunSummary};
use rmedia_core::input::manifest_json;
use rmedia_core::request::{Options, PartialOptions};
use rmedia_core::upscale::{Reframe, UpscaleOptions};
use rmedia_testkit::{Fixture, fixture, probe_json, require_engine};

fn copy(f: Fixture, dir: &Path, name: &str) -> PathBuf {
    let to = dir.join(name);
    std::fs::copy(fixture(f), &to).unwrap();
    to
}

fn video_size(file: &Path) -> (u64, u64) {
    let j = probe_json(file);
    let v = j["streams"].as_array().unwrap().iter().find(|s| s["codec_type"] == "video").unwrap().clone();
    (v["width"].as_u64().unwrap(), v["height"].as_u64().unwrap())
}

/// Temp files or scratch folders left anywhere under `dir`.
fn leftovers(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.file_name().unwrap().to_string_lossy().contains(".rmedia-") {
            found.push(p.clone());
        }
        if p.is_dir() {
            found.extend(leftovers(&p));
        }
    }
    found
}

/// Stages per job, in order.
fn stages(events: &[Event]) -> BTreeMap<String, Vec<String>> {
    let mut by_job: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in events {
        if let Event::JobStage { job_id, stage, .. } = e {
            by_job.entry(format!("{job_id:?}")).or_default().push(stage.clone());
        }
    }
    by_job
}

fn succeeded(s: &RunSummary) -> Vec<(PathBuf, JobMeasure)> {
    s.jobs
        .iter()
        .map(|j| match &j.outcome {
            JobOutcome::Succeeded { output, measure } => (output.clone(), measure.clone()),
            other => panic!("{} did not succeed: {other:?}", j.source.display()),
        })
        .collect()
}

/// The quickstart › US4 manifest: per-video overrides, a clip, whole videos without `clips`.
#[tokio::test(flavor = "multi_thread")]
async fn quickstart_manifest_gives_each_output_its_preset_size() {
    require_engine!("libx265", "zscale", "cas", "gblur");
    let d = tempfile::tempdir().unwrap();
    let low = copy(Fixture::DegradedPortrait, d.path(), "low.mp4");
    let land = copy(Fixture::MovingMarker, d.path(), "land.mp4");
    let slides = copy(Fixture::TinySource, d.path(), "slides.mp4");
    let text = r#"{ "version": 1, "defaults": { "upscale": {} },
      "videos": [ { "input": "low.mp4" },
                  { "input": "land.mp4", "options": { "upscale": { "preset": "reddit-916" } },
                    "clips": [ { "start": 1, "end": 5, "name": "pan" } ] },
                  { "input": "slides.mp4", "options": { "upscale": { "reframe": "blur", "tier": "cpu" } } } ] }"#;
    let manifest = d.path().join("m.json");
    let req = manifest_json::parse(&manifest, text, &Options::default()).unwrap();
    let (tools, caps) = engine();
    let p = make_plan(&req, &tools, &caps).await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    let state = tempfile::tempdir().unwrap();
    let (summary, events) = run_plan(p, tools, state.path()).await;
    assert_eq!((summary.succeeded, summary.failed, summary.exit_code), (3, 0, 0));
    let outs = succeeded(&summary);
    let want = [
        (d.path().join("upscaled/low_phone-fill.mp4"), (1080, 2340)),
        (d.path().join("clips/land_001_pan_reddit-916.mp4"), (1080, 1920)),
        (d.path().join("upscaled/slides_phone-fill.mp4"), (1080, 2340)),
    ];
    for ((out, _), (path, size)) in outs.iter().zip(&want) {
        assert_eq!(out, path);
        assert_eq!(video_size(out), *size, "{}", out.display());
    }
    // The clip is cut and upscaled by one render: no separate cut, no intermediate file.
    let JobMeasure::Upscale { span, .. } = &outs[1].1 else { panic!() };
    assert_eq!((span.start, span.end), (secs(1.0), secs(5.0)));
    for (job, s) in stages(&events) {
        assert_eq!(s.iter().filter(|s| *s == "render").count(), 1, "job {job}: {s:?}");
        assert!(s.iter().all(|s| ["analyze", "render", "verify"].contains(&s.as_str())), "job {job}: {s:?}");
    }
    assert!(leftovers(d.path()).is_empty(), "{:?}", leftovers(d.path()));
    let mut names: Vec<String> = std::fs::read_dir(d.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["clips", "land.mp4", "low.mp4", "slides.mp4", "upscaled"], "nothing else written");
    for src in [low, land, slides] {
        assert!(src.exists());
    }
}

/// A partial clip from an indexed (MP4) and a non-indexed (MPEG-TS, timestamps from 10 s)
/// source: exact span, starts at zero with audio and video aligned, chapters dropped.
#[tokio::test(flavor = "multi_thread")]
async fn partial_clips_are_exact_and_aligned() {
    require_engine!("libx265", "zscale", "cas");
    let d = tempfile::tempdir().unwrap();
    let opts = Options {
        upscale: Some(UpscaleOptions { reframe: Reframe::Center, ..UpscaleOptions::default() }),
        output_dir: Some(d.path().join("out")),
        ..Options::default()
    };
    for (f, name) in [(Fixture::ChaptersMp4, "chapters.mp4"), (Fixture::OffsetTs, "offset.ts")] {
        let src = copy(f, d.path(), name);
        let (tools, caps) = engine();
        let p =
            make_plan(&request(&src, vec![clip(2.0, EndSpec::At(secs(6.0)))], opts.clone()), &tools, &caps)
                .await;
        assert!(!p.has_errors(), "{name}: {:?}", p.issues);
        let (summary, events) = run_plan(p, tools, d.path()).await;
        let (out, _) = succeeded(&summary).remove(0);
        assert_eq!(stages(&events).values().next().unwrap().iter().filter(|s| *s == "render").count(), 1);

        let probe = std::process::Command::new(rmedia_testkit::ffprobe())
            .args(["-v", "error", "-show_chapters", "-show_format", "-show_streams", "-of", "json"])
            .arg(&out)
            .output()
            .unwrap();
        let j: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
        let num = |v: &serde_json::Value| v.as_str().and_then(|s| s.parse::<f64>().ok()).unwrap();
        let duration = num(&j["format"]["duration"]);
        assert!((duration - 4.0).abs() <= 0.05, "{name}: duration {duration}");
        let start = |kind: &str| {
            let s = j["streams"].as_array().unwrap().iter().find(|s| s["codec_type"] == kind).unwrap();
            num(&s["start_time"])
        };
        let (v, a) = (start("video"), start("audio"));
        assert!(v.abs() <= 0.04, "{name}: first video frame at {v}");
        assert!((a - v).abs() <= 0.04, "{name}: audio starts at {a}, video at {v}");
        assert_eq!(j["chapters"].as_array().map(Vec::len), Some(0), "{name}: chapters dropped");
    }
    assert!(leftovers(d.path()).is_empty(), "{:?}", leftovers(d.path()));
}

/// One video failing does not stop the others; the summary and exit code follow 001.
#[tokio::test(flavor = "multi_thread")]
async fn one_failing_video_leaves_the_others_to_complete() {
    require_engine!("libx265", "zscale", "cas");
    let d = tempfile::tempdir().unwrap();
    let good = copy(Fixture::TinySource, d.path(), "good.mp4");
    let gone = copy(Fixture::TinySource, d.path(), "gone.mp4");
    let mut req = request(
        &good,
        vec![clip(0.0, EndSpec::EndOfMedia)],
        Options {
            upscale: Some(UpscaleOptions { reframe: Reframe::Center, ..UpscaleOptions::default() }),
            ..Options::default()
        },
    );
    let mut second = req.sources[0].clone();
    second.path = gone.clone();
    second.overrides = PartialOptions::default();
    req.sources.push(second);
    let (tools, caps) = engine();
    let p = make_plan(&req, &tools, &caps).await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    std::fs::remove_file(&gone).unwrap();
    let (summary, _) = run_plan(p, tools, d.path()).await;
    assert_eq!((summary.succeeded, summary.failed, summary.exit_code), (1, 1, 1));
    let ok = summary.jobs.iter().find(|j| j.source == good).unwrap();
    assert!(matches!(ok.outcome, JobOutcome::Succeeded { .. }));
    assert_eq!(video_size(&d.path().join("upscaled/good_phone-fill.mp4")), (1080, 2340));
    assert!(!d.path().join("upscaled/gone_phone-fill.mp4").exists());
    assert!(leftovers(d.path()).is_empty(), "{:?}", leftovers(d.path()));
}
