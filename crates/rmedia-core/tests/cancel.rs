//! SC-007: cancellation stops within 2 s, leaves no partial files and no child processes.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::{Event, EventSink, JobOutcome};
use rmedia_core::exec::{self, RunConfig};
use rmedia_core::gif::EngineChoice;
use rmedia_core::request::Mode;
use rmedia_testkit::{Fixture, fixture, require_ffmpeg};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::sync::mpsc;

/// The orphan checks look at every engine process this test binary started, so the tests here
/// run one at a time.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
        .filter(|n| n.contains("ffmpeg") || n.contains("ffprobe") || n.contains("gifski"))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_run_is_fast_and_clean() {
    let _serial = SERIAL.lock().await;
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
    let _serial = SERIAL.lock().await;
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

/// T028 (002 US1): cancelling an upscale stops within 2 s and leaves no `.part`, no scratch
/// directory and no engine process (SC-006).
#[tokio::test(flavor = "multi_thread")]
async fn cancelling_an_upscale_is_fast_and_clean() {
    let _serial = SERIAL.lock().await;
    rmedia_testkit::require_engine!("libx265", "zscale", "cas", "gblur");
    let src = fixture(Fixture::Hd60Mp4);
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let mut o = opts(out.path(), Mode::Fast);
    o.upscale = Some(Default::default());
    let plan = make_plan(&request(&src, vec![clip(0.0, EndSpec::EndOfMedia)], o), &tools, &caps).await;
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    let scratch = plan.jobs[0].scratch.clone().expect("upscale jobs have a scratch dir");

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
    assert!(scratch.is_dir());
    let t0 = Instant::now();
    handle.cancel();
    let summary = handle.wait().await;
    let elapsed = t0.elapsed();
    assert!(elapsed < Duration::from_secs(2), "cancel took {elapsed:?}");
    assert_eq!(summary.exit_code, 130);
    assert!(matches!(summary.jobs[0].outcome, JobOutcome::Cancelled));
    assert!(partials(out.path()).is_empty(), "partial files left: {:?}", partials(out.path()));
    assert!(!scratch.exists(), "scratch dir left behind");
    assert!(child_engines().is_empty(), "orphaned engine processes: {:?}", child_engines());
}

/// T034, T038 (003 US1): cancelling a GIF job during `stage` stops within 2 s and leaves no
/// `.part`, no scratch folder and no engine process (with `engine`, the gifski pipe's two).
async fn cancel_gif_during(stage: &str, engine_choice: EngineChoice) {
    let _serial = SERIAL.lock().await;
    rmedia_testkit::require_engine!("palettegen", "paletteuse");
    let src = fixture(Fixture::Busy10);
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let mut o = opts(out.path(), Mode::Accurate);
    o.gif = Some(rmedia_core::gif::GifOptions {
        budget: None,
        width: Some(1280),
        fps: Some(25),
        engine: engine_choice,
        ..Default::default()
    });
    let plan = make_plan(&request(&src, vec![clip(0.0, EndSpec::EndOfMedia)], o), &tools, &caps).await;
    assert!(!plan.has_errors(), "{:?}", plan.issues);
    let scratch = plan.jobs[0].scratch.clone().expect("GIF jobs have a scratch folder");

    let (tx, mut rx) = mpsc::unbounded_channel();
    let cfg = RunConfig { jobs: 1, state_dir: Some(out.path().join("state")) };
    let handle = exec::start(plan, tools, cfg, Arc::new(Notify(tx)));
    loop {
        match tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
            Ok(Some(Event::JobStage { stage: s, .. })) if s == stage => break,
            Ok(Some(_)) => continue,
            other => panic!("no {stage} stage: {other:?}"),
        }
    }
    // Let the stage's process start.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!child_engines().is_empty(), "ffmpeg should be running in the {stage} stage");
    let t0 = Instant::now();
    handle.cancel();
    let summary = handle.wait().await;
    let elapsed = t0.elapsed();
    assert!(elapsed < Duration::from_secs(2), "cancel took {elapsed:?}");
    assert_eq!(summary.exit_code, 130);
    assert!(matches!(summary.jobs[0].outcome, JobOutcome::Cancelled), "{:?}", summary.jobs[0].outcome);
    assert!(partials(out.path()).is_empty(), "partial files left: {:?}", partials(out.path()));
    assert!(!scratch.exists(), "scratch folder left behind");
    assert!(child_engines().is_empty(), "orphaned engine processes: {:?}", child_engines());
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_gif_in_the_palette_stage_is_fast_and_clean() {
    cancel_gif_during("palette", EngineChoice::Builtin).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_gif_in_the_encode_stage_is_fast_and_clean() {
    cancel_gif_during("encode", EngineChoice::Builtin).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_the_gifski_pipe_is_fast_and_clean() {
    rmedia_testkit::require_tool!(gifski);
    cancel_gif_during("encode", EngineChoice::Gifski).await;
}
