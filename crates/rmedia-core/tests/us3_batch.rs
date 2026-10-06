//! US3 acceptance scenarios and SC-004: batch manifests.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::*;
use rmedia_core::event::{JobOutcome, NullSink};
use rmedia_core::exec::{self, RunConfig};
use rmedia_core::input::{manifest_csv, manifest_json};
use rmedia_core::request::Options;
use rmedia_testkit::{Fixture, duration, fixture, require_ffmpeg};

/// Three source videos next to the manifest (hard links: cheap and identical).
fn workspace(n: usize) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::Gop50Mp4);
    for i in 1..=n {
        std::fs::hard_link(&src, d.path().join(format!("v{i}.mp4")))
            .or_else(|_| std::fs::copy(&src, d.path().join(format!("v{i}.mp4"))).map(|_| ()))
            .unwrap();
    }
    d
}

#[tokio::test(flavor = "multi_thread")]
async fn three_videos_two_clips_each_with_relative_paths() {
    require_ffmpeg!();
    let d = workspace(3);
    let manifest = d.path().join("jobs.json");
    std::fs::write(
        &manifest,
        r#"{"version":1,"defaults":{"output_dir":"out","mode":"accurate"},"videos":[
            {"input":"v1.mp4","clips":[{"start":1,"end":2},{"start":"0:03","duration":1.5,"name":"b"}]},
            {"input":"v2.mp4","clips":[{"start":1,"end":2},{"start":5,"end":6}]},
            {"input":"v3.mp4","options":{"mode":"fast"},"clips":[{"start":1,"end":3},{"start":10,"end":"end"}]}]}"#,
    )
    .unwrap();
    // Run from a different working directory: paths resolve against the manifest.
    let req = manifest_json::load(&manifest, &Options::default()).unwrap();
    let (tools, caps) = engine();
    let plan = make_plan(&req, &tools, &caps).await;
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    let (summary, _) = run_plan(plan, tools, d.path()).await;
    assert_eq!((summary.succeeded, summary.failed, summary.exit_code), (6, 0, 0), "{summary:?}");
    assert!(summary.jobs.iter().all(|j| j.output.starts_with(d.path().join("out"))));
    assert!((duration(&summary.jobs[1].output) - 1.5).abs() < 0.06);
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_video_is_reported_before_anything_runs() {
    require_ffmpeg!();
    let d = workspace(1);
    let manifest = d.path().join("jobs.csv");
    std::fs::write(&manifest, "input,start,end\nv1.mp4,1,2\nnope.mp4,1,2\n").unwrap();
    let req = manifest_csv::load(&manifest, &Options::default()).unwrap();
    let (tools, caps) = engine();
    let plan = make_plan(&req, &tools, &caps).await;
    assert_eq!(plan.issues.len(), 1);
    assert_eq!(plan.issues[0].code, "SOURCE_MISSING");
    assert!(plan.issues[0].to_string().contains("jobs.csv:3"), "{}", plan.issues[0]);
}

#[tokio::test(flavor = "multi_thread")]
async fn equivalent_json_and_csv_produce_identical_plans() {
    require_ffmpeg!();
    let d = workspace(2);
    let json = d.path().join("m.json");
    let csv = d.path().join("m.csv");
    std::fs::write(
        &json,
        r#"{"version":1,"videos":[
            {"input":"v1.mp4","clips":[{"start":"0:01","end":"0:02","name":"x"},{"start":5,"duration":2},
                                       {"start":"0:10","end":"+3"},{"start":20,"duration":"+1.5"},{"start":"50","end":"end"}]},
            {"input":"v2.mp4","options":{"mode":"accurate","output_dir":"o"},"clips":[{"start":3,"end":"end"}]}]}"#,
    )
    .unwrap();
    std::fs::write(&csv, "input,start,end,duration,name,mode,output\nv1.mp4,0:01,0:02,,x,,\nv1.mp4,5,,2,,,\n./v1.mp4,0:10,+3,,,,\nv1.mp4,20,,+1.5,,,\nv1.mp4,50,end,,,,\nv2.mp4,3,end,,,accurate,o\n").unwrap();
    let (tools, caps) = engine();
    let a = make_plan(&manifest_json::load(&json, &Options::default()).unwrap(), &tools, &caps).await;
    let b = make_plan(&manifest_csv::load(&csv, &Options::default()).unwrap(), &tools, &caps).await;
    let key = |p: &rmedia_core::plan::Plan| {
        p.jobs
            .iter()
            .map(|j| (j.source.clone(), j.output.clone(), j.mode, j.requested, j.args().len()))
            .collect::<Vec<_>>()
    };
    assert!(a.issues.is_empty() && b.issues.is_empty());
    assert_eq!(key(&a), key(&b));
}

/// SC-004: 100 clips across 20 videos run unattended; one injected failure does not stop the
/// other 99.
#[tokio::test(flavor = "multi_thread")]
async fn sc004_hundred_clips_one_failure() {
    require_ffmpeg!();
    let d = workspace(20);
    std::fs::write(d.path().join("blocked"), b"a file where a folder is needed").unwrap();
    let mut rows = String::from("input,start,end,output\n");
    for v in 1..=20 {
        for c in 0..5 {
            let out = if v == 7 && c == 3 { "blocked/sub" } else { "out" };
            rows.push_str(&format!("v{v}.mp4,{},{},{out}\n", c * 10 + 1, c * 10 + 3));
        }
    }
    let manifest = d.path().join("big.csv");
    std::fs::write(&manifest, rows).unwrap();
    let req = manifest_csv::load(&manifest, &Options::default()).unwrap();
    let (tools, caps) = engine();
    let plan = make_plan(&req, &tools, &caps).await;
    assert_eq!((plan.jobs.len(), plan.issues.len()), (100, 0), "{:?}", plan.issues);
    let cfg = RunConfig { jobs: 4, state_dir: Some(d.path().join("state")) };
    let summary = exec::start(plan, tools, cfg, Arc::new(NullSink)).wait().await;
    assert_eq!((summary.succeeded, summary.failed, summary.exit_code), (99, 1, 1));
    let failed = summary.jobs.iter().find(|j| matches!(j.outcome, JobOutcome::Failed { .. })).unwrap();
    assert!(failed.output.starts_with(d.path().join("blocked")));
    assert!(partials(Path::new(d.path())).is_empty());
}
