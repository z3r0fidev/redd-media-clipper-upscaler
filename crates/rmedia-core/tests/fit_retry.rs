//! Feature 005, T013: the retry path, made deterministic with the `test-support` overshoot hook.
//! Its own test binary: the hook is process-wide.

mod common;

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::{Event, JobMeasure, JobOutcome};
use rmedia_core::fit::FitOptions;
use rmedia_core::request::Mode;
use rmedia_testkit::{Fixture, fixture};

/// The tests here share the hook, so they run one at a time.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn run(
    forced: u32,
    cap: Option<u64>,
) -> (rmedia_core::event::RunSummary, Vec<Event>, tempfile::TempDir, Option<std::path::PathBuf>) {
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let o = rmedia_core::request::Options {
        fit: Some(FitOptions { max_size: cap, ..FitOptions::default() }),
        ..opts(out.path(), Mode::Fast)
    };
    let src = fixture(Fixture::FitBusy);
    let plan = make_plan(&request(&src, vec![clip(3.0, EndSpec::At(secs(9.0)))], o), &tools, &caps).await;
    assert!(!plan.has_errors(), "{:?}", plan.issues);
    let scratch = plan.jobs[0].scratch.clone();
    rmedia_core::fit::test_hooks::force_overshoot(forced);
    let (summary, events) = run_plan(plan, tools, &out.path().join("state")).await;
    rmedia_core::fit::test_hooks::force_overshoot(0);
    (summary, events, out, scratch)
}

fn stages(events: &[Event], name: &str) -> usize {
    events.iter().filter(|e| matches!(e, Event::JobStage { stage, .. } if stage == name)).count()
}

/// One overshoot: pass 2 runs again lower; pass 1 runs once (its stats are reused, R2).
#[tokio::test(flavor = "multi_thread")]
async fn an_overshoot_is_encoded_again_lower_without_a_new_first_pass() {
    let _serial = SERIAL.lock().await;
    if !rmedia_testkit::require_engine(&["libx264", "aac"]) {
        return;
    }
    let (summary, events, _out, _) = run(1, Some(2_000_000)).await;
    let JobOutcome::Succeeded { measure: JobMeasure::Clip { fit: Some(f), .. }, .. } =
        &summary.jobs[0].outcome
    else {
        panic!("{:?}", summary.jobs[0].outcome)
    };
    assert_eq!(f.attempts.len(), 2, "{f:?}");
    assert!(f.attempts[1].video_kbps < f.attempts[0].video_kbps, "{f:?}");
    assert!(f.bytes <= f.cap);
    assert_eq!(stages(&events, "pass1"), 1, "pass 1 ran again");
}

/// Four overshoots (the first encode and three retries; 4 MB leaves room for three −11% steps
/// above the minimum bitrate): `FIT_FAILED`, and nothing is left.
#[tokio::test(flavor = "multi_thread")]
async fn still_over_after_three_retries_fails_and_leaves_nothing() {
    let _serial = SERIAL.lock().await;
    if !rmedia_testkit::require_engine(&["libx264", "aac"]) {
        return;
    }
    let (summary, _, out, scratch) = run(4, Some(4_000_000)).await;
    let JobOutcome::Failed { error } = &summary.jobs[0].outcome else {
        panic!("{:?}", summary.jobs[0].outcome)
    };
    assert_eq!(error.code, "FIT_FAILED", "{error:?}");
    let detail = error.detail.as_ref().unwrap();
    assert_eq!(detail.attempts.as_ref().map(Vec::len), Some(4));
    assert_eq!(summary.exit_code, 1);
    assert!(partials(out.path()).is_empty(), "{:?}", partials(out.path()));
    assert!(scratch.is_some_and(|s| !s.exists()), "scratch folder left behind");
    let mp4s: Vec<_> =
        walk(out.path()).into_iter().filter(|p| p.extension().is_some_and(|x| x == "mp4")).collect();
    assert!(mp4s.is_empty(), "no output may be committed: {mp4s:?}");
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() { v.extend(walk(&p)) } else { v.push(p) }
    }
    v
}

/// A retry that would fall well below the target's minimum bitrate (under 90% of it) fails at
/// once (1.6 MB over 6 s at 1080p plans about 5% above the 1,866 kb/s minimum; the third
/// attempt, two −11% steps down, would need less than 1,680 kb/s).
#[tokio::test(flavor = "multi_thread")]
async fn a_retry_below_the_minimum_bitrate_fails_early() {
    let _serial = SERIAL.lock().await;
    if !rmedia_testkit::require_engine(&["libx264", "aac"]) {
        return;
    }
    let (summary, _, out, _) = run(4, Some(1_600_000)).await;
    let JobOutcome::Failed { error } = &summary.jobs[0].outcome else {
        panic!("{:?}", summary.jobs[0].outcome)
    };
    assert_eq!(error.code, "FIT_FAILED", "{error:?}");
    let attempts = error.detail.as_ref().and_then(|d| d.attempts.as_ref()).map_or(0, Vec::len);
    assert!(attempts < 4, "{attempts} attempts");
    assert!(partials(out.path()).is_empty());
}

/// A quality-mode encode that overshoots falls back to budget mode within its `encode` stage:
/// no `pass2` stage it did not plan, and every stage index within its count.
#[tokio::test(flavor = "multi_thread")]
async fn a_quality_overshoot_retries_within_its_encode_stage() {
    let _serial = SERIAL.lock().await;
    if !rmedia_testkit::require_engine(&["libx264", "aac"]) {
        return;
    }
    let (summary, events, _out, _) = run(1, None).await;
    let JobOutcome::Succeeded { measure: JobMeasure::Clip { fit: Some(f), .. }, .. } =
        &summary.jobs[0].outcome
    else {
        panic!("{:?}", summary.jobs[0].outcome)
    };
    assert_eq!((f.mode, f.attempts.len()), (rmedia_core::fit::plan::Mode::Quality, 2), "{f:?}");
    let seen: Vec<(String, u32, u32)> = events
        .iter()
        .filter_map(|e| match e {
            Event::JobStage { stage, index, of, .. } => Some((stage.clone(), *index, *of)),
            _ => None,
        })
        .collect();
    let names: Vec<&str> = seen.iter().map(|s| s.0.as_str()).collect();
    assert_eq!(names, ["encode", "encode", "verify"], "{seen:?}");
    assert!(seen.iter().all(|(_, i, of)| i <= of), "{seen:?}");
}
