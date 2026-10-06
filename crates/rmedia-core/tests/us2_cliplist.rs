//! US2 acceptance scenarios: many clips from one video via a clip list.

mod common;

use std::path::Path;

use common::*;
use rmedia_core::event::JobOutcome;
use rmedia_core::exec::{self, RunConfig};
use rmedia_core::input::cliplist;
use rmedia_core::request::{Mode, OnExists, Options};
use rmedia_testkit::{Fixture, duration, fixture, require_ffmpeg};

fn list_request(src: &Path, text: &str, defaults: Options) -> rmedia_core::request::BatchRequest {
    let clips = cliplist::parse(Path::new("clips.txt"), text).expect("valid list");
    request(src, clips, defaults)
}

#[tokio::test(flavor = "multi_thread")]
async fn three_clips_named_or_numbered() {
    require_ffmpeg!();
    let src = fixture(Fixture::Gop50Mp4);
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let text = "# highlights\n0:02 0:04 intro\n\n0:10 +3\n00:00:20,500 0:25 the end\n";
    let req = list_request(&src, text, opts(out.path(), Mode::Accurate));
    let plan = make_plan(&req, &tools, &caps).await;
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    let (summary, _) = run_plan(plan, tools, out.path()).await;
    assert_eq!((summary.succeeded, summary.exit_code), (3, 0), "{summary:?}");
    let names: Vec<String> =
        summary.jobs.iter().map(|j| j.output.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(
        names,
        ["gop50_001_intro.mp4", "gop50_002_00-00-10.000_00-00-13.000.mp4", "gop50_003_the end.mp4"]
    );
    for (j, want) in summary.jobs.iter().zip([2.0, 3.0, 4.5]) {
        assert!((duration(&j.output) - want).abs() < 0.06, "{} {}", j.output.display(), duration(&j.output));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn intra_plan_collisions() {
    require_ffmpeg!();
    let src = fixture(Fixture::Gop50Mp4);
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let mut o = opts(out.path(), Mode::Fast);
    o.name_template = "{name}.{ext}".into();
    let text = "0:01 0:02 same\n0:03 0:04 same\n";
    let plan = make_plan(&list_request(&src, text, o.clone()), &tools, &caps).await;
    assert!(plan.issues.iter().any(|i| i.message.contains("two clips would write")), "{:?}", plan.issues);

    o.on_exists = OnExists::Rename;
    let plan = make_plan(&list_request(&src, text, o), &tools, &caps).await;
    assert!(plan.issues.is_empty());
    let outs: Vec<_> = plan.jobs.iter().map(|j| j.output.file_name().unwrap().to_owned()).collect();
    assert_eq!(outs, ["same.mp4", "same_1.mp4"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn one_failure_does_not_stop_the_others() {
    require_ffmpeg!();
    let src = fixture(Fixture::Gop50Mp4);
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    // Clip "b" writes into a sub-folder whose name is already taken by a file → fails at run time.
    std::fs::write(out.path().join("b"), b"not a folder").unwrap();
    let mut o = opts(out.path(), Mode::Fast);
    o.name_template = "{name}/{index}.{ext}".into();
    let req = list_request(&src, "0:01 0:03 a\n0:05 0:07 b\n0:09 0:11 c\n", o);
    let plan = make_plan(&req, &tools, &caps).await;
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    let cfg = RunConfig { jobs: 3, state_dir: Some(out.path().join("state")) };
    let summary =
        exec::start(plan, tools, cfg, std::sync::Arc::new(rmedia_core::event::NullSink)).wait().await;
    assert_eq!((summary.succeeded, summary.failed, summary.exit_code), (2, 1, 1), "{summary:?}");
    match &summary.jobs[1].outcome {
        JobOutcome::Failed { error } => assert_eq!(error.code, "OUTPUT_UNWRITABLE"),
        other => panic!("{other:?}"),
    }
    assert!(partials(out.path()).is_empty());
}
