//! T010 (002, refactor A2): per-job scratch directories are created next to the temp output,
//! journaled, and removed on success, failure and cancel.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::{Event, EventSink, JobOutcome};
use rmedia_core::exec::commit::scratch_path;
use rmedia_core::exec::{self, RunConfig};
use rmedia_core::request::Mode;
use rmedia_testkit::{Fixture, fixture, require_ffmpeg};
use tokio::sync::mpsc;

fn dirs_named_d(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "d")).collect()
        })
        .unwrap_or_default()
}

#[test]
fn scratch_name_is_the_part_name_with_a_d_suffix() {
    let p = scratch_path(std::path::Path::new("/v/out/game_phone-fill.mp4"), "r1a2", 3);
    assert_eq!(p, std::path::Path::new("/v/out/.game_phone-fill.rmedia-r1a2-3.d"));
}

#[tokio::test(flavor = "multi_thread")]
async fn scratch_dir_exists_during_the_job_and_is_removed_after_success_and_failure() {
    require_ffmpeg!();
    let src = fixture(Fixture::Gop50Mp4);
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let req = request(&src, vec![clip(1.0, EndSpec::At(secs(3.0)))], opts(out.path(), Mode::Accurate));
    let mut plan = make_plan(&req, &tools, &caps).await;
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    let job = &mut plan.jobs[0];
    let scratch = scratch_path(&job.output, &plan.run_id, job.id);
    job.scratch = Some(scratch.clone());

    // A second job that fails: its source vanishes after planning.
    let gone = out.path().join("gone.mp4");
    std::fs::copy(fixture(Fixture::Gop50Mp4), &gone).unwrap();
    let req2 =
        request(&gone, vec![clip(1.0, EndSpec::At(secs(3.0)))], opts(&out.path().join("b"), Mode::Accurate));
    let mut plan2 = make_plan(&req2, &tools, &caps).await;
    let job2 = &mut plan2.jobs[0];
    job2.scratch = Some(scratch_path(&job2.output, &plan2.run_id, job2.id));
    std::fs::remove_file(&gone).unwrap();

    let (summary, events) = run_plan(plan, tools.clone(), &out.path().join("state")).await;
    assert!(matches!(summary.jobs[0].outcome, JobOutcome::Succeeded { .. }), "{:?}", summary.jobs[0]);
    assert!(events.iter().any(|e| matches!(e, Event::JobStarted { .. })));
    assert!(!scratch.exists(), "scratch dir left after success");

    let (summary2, _) = run_plan(plan2, tools, &out.path().join("state")).await;
    assert!(matches!(summary2.jobs[0].outcome, JobOutcome::Failed { .. }), "{:?}", summary2.jobs[0]);
    assert!(dirs_named_d(&out.path().join("b")).is_empty(), "scratch dir left after failure");
    assert!(partials(out.path()).is_empty());
}

struct Notify(mpsc::UnboundedSender<Event>);
impl EventSink for Notify {
    fn emit(&self, e: Event) {
        let _ = self.0.send(e);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn scratch_dir_is_removed_on_cancel() {
    require_ffmpeg!();
    let src = fixture(Fixture::Hd60Mp4);
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let req = request(&src, vec![clip(0.0, EndSpec::EndOfMedia)], opts(out.path(), Mode::Accurate));
    let mut plan = make_plan(&req, &tools, &caps).await;
    let job = &mut plan.jobs[0];
    let scratch = scratch_path(&job.output, &plan.run_id, job.id);
    job.scratch = Some(scratch.clone());

    let (tx, mut rx) = mpsc::unbounded_channel();
    let cfg = RunConfig { jobs: 1, state_dir: Some(out.path().join("state")) };
    let handle = exec::start(plan, tools, cfg, Arc::new(Notify(tx)));
    loop {
        match tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
            Ok(Some(Event::JobProgress { .. })) => break,
            Ok(Some(_)) => continue,
            other => panic!("no progress event: {other:?}"),
        }
    }
    assert!(scratch.is_dir(), "scratch dir must exist while the job runs");
    handle.cancel();
    let summary = handle.wait().await;
    assert!(matches!(summary.jobs[0].outcome, JobOutcome::Cancelled));
    assert!(!scratch.exists(), "scratch dir left after cancel");
}
