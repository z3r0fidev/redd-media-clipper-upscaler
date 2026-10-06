//! SC-007: cancellation stops within 2 s, leaves no partial files and no child processes.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::{Event, EventSink, JobOutcome};
use rmedia_core::exec::{self, RunConfig};
use rmedia_core::request::Mode;
use rmedia_testkit::{Fixture, fixture, require_ffmpeg};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::sync::mpsc;

struct Notify(mpsc::UnboundedSender<Event>);
impl EventSink for Notify {
    fn emit(&self, e: Event) {
        let _ = self.0.send(e);
    }
}

/// ffmpeg/ffprobe processes whose parent is this test process.
fn child_engines() -> Vec<String> {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let me = Pid::from_u32(std::process::id());
    sys.processes()
        .values()
        .filter(|p| p.parent() == Some(me))
        .map(|p| p.name().to_string_lossy().into_owned())
        .filter(|n| n.contains("ffmpeg") || n.contains("ffprobe"))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_run_is_fast_and_clean() {
    require_ffmpeg!();
    let src = fixture(Fixture::Hd60Mp4);
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    // Two long accurate encodes, one at a time, so one is running and one is queued.
    let req = request(
        &src,
        vec![clip(0.0, EndSpec::EndOfMedia), clip(1.0, EndSpec::EndOfMedia)],
        opts(out.path(), Mode::Accurate),
    );
    let plan = make_plan(&req, &tools, &caps).await;
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);

    let (tx, mut rx) = mpsc::unbounded_channel();
    let cfg = RunConfig { jobs: 1, state_dir: Some(out.path().join("state")) };
    let handle = exec::start(plan, tools, cfg, Arc::new(Notify(tx)));
    // Wait until the first job reports progress (ffmpeg is running and writing).
    loop {
        match tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
            Ok(Some(Event::JobProgress { .. })) => break,
            Ok(Some(_)) => continue,
            other => panic!("no progress event: {other:?}"),
        }
    }
    assert!(!child_engines().is_empty(), "ffmpeg should be running");

    let t0 = Instant::now();
    handle.cancel();
    let summary = handle.wait().await;
    let elapsed = t0.elapsed();

    assert!(elapsed < Duration::from_secs(2), "cancel took {elapsed:?}");
    assert_eq!(summary.exit_code, 130);
    assert!(summary.jobs.iter().all(|j| matches!(j.outcome, JobOutcome::Cancelled)), "{summary:?}");
    assert!(partials(out.path()).is_empty(), "partial files left: {:?}", partials(out.path()));
    assert!(child_engines().is_empty(), "orphaned engine processes: {:?}", child_engines());
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_one_job_keeps_the_others() {
    require_ffmpeg!();
    let src = fixture(Fixture::Hd60Mp4);
    let small = fixture(Fixture::Gop50Mp4);
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let mut req = request(&src, vec![clip(0.0, EndSpec::EndOfMedia)], opts(out.path(), Mode::Accurate));
    req.sources.extend(request(&small, vec![clip(1.0, EndSpec::At(secs(3.0)))], Default::default()).sources);
    let plan = make_plan(&req, &tools, &caps).await;
    let long_id = plan.jobs[0].id;

    let (tx, mut rx) = mpsc::unbounded_channel();
    let cfg = RunConfig { jobs: 2, state_dir: None };
    let handle = exec::start(plan, tools, cfg, Arc::new(Notify(tx)));
    while let Some(e) = rx.recv().await {
        if matches!(e, Event::JobProgress { job_id, .. } if job_id == long_id) {
            break;
        }
    }
    handle.cancel_job(long_id);
    let summary = handle.wait().await;
    assert!(matches!(summary.jobs[0].outcome, JobOutcome::Cancelled));
    assert!(matches!(summary.jobs[1].outcome, JobOutcome::Succeeded { .. }), "{summary:?}");
    assert_eq!(summary.exit_code, 1, "a cancelled job means the run did not fully succeed");
    assert!(partials(out.path()).is_empty());
}
