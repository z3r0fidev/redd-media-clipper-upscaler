//! Feature 004, US2: risky sources fall back safely and say why (tasks T031–T034).

mod common;

use std::path::{Path, PathBuf};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::error::IssueCode;
use rmedia_core::event::{Event, JobMeasure, JobOutcome, SmartOutcome};
use rmedia_core::plan::{Caps, JobSpec, Plan};
use rmedia_core::request::{Mode, Options};
use rmedia_core::smart::FallbackReason;
use rmedia_testkit::{Fixture, fixture, frame_count};

fn engine_ok() -> bool {
    rmedia_testkit::require_engine(&["libx264", "aac"])
}

fn smart_opts(out: &Path, edit: impl FnOnce(&mut Options)) -> Options {
    let mut o = opts(out, Mode::Smart);
    edit(&mut o);
    o
}

async fn plan_with_caps(src: &Path, start: f64, end: f64, o: Options, caps: &Caps) -> Plan {
    let (tools, _) = engine();
    make_plan(&request(src, vec![clip(start, EndSpec::At(secs(end)))], o), &tools, caps).await
}

/// The reason in a job's `SMART_FALLBACK` notice, which is also one of its warnings.
fn fallback_notice(plan: &Plan) -> Option<FallbackReason> {
    let job = &plan.jobs[0];
    let n = job.notices.iter().find(|n| n.code == IssueCode::SmartFallback)?;
    assert!(job.warnings.contains(&n.message), "{:?}", job.warnings);
    assert!(
        n.message.starts_with("smart cut not possible: ")
            && n.message.ends_with("; cutting in accurate mode")
    );
    n.detail.as_ref()?.reason
}

/// Runs a one-job plan; returns the output, the smart outcome and the events.
async fn run_one(plan: Plan, out: &Path) -> (PathBuf, SmartOutcome, Vec<Event>) {
    let (tools, _) = engine();
    let (summary, events) = run_plan(plan, tools, &out.join("state")).await;
    match &summary.jobs[0].outcome {
        JobOutcome::Succeeded { output, measure: JobMeasure::Clip { smart: Some(s), .. } } => {
            (output.clone(), s.clone(), events)
        }
        other => panic!("{other:?}"),
    }
}

/// The `SMART_FALLBACK` warning events and their reasons.
fn warned(events: &[Event]) -> Vec<Option<FallbackReason>> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::JobWarning { code: Some(IssueCode::SmartFallback), detail, .. } => {
                Some(detail.as_ref().and_then(|d| d.reason))
            }
            _ => None,
        })
        .collect()
}

/// A private copy of a fixture, so a test hook keyed on its path touches this test only.
fn private_copy(kind: Fixture, dir: &Path) -> PathBuf {
    let src = fixture(kind);
    let copy = dir.join(format!("private-{}", src.file_name().unwrap().to_string_lossy()));
    std::fs::copy(&src, &copy).unwrap();
    copy
}

/// A source, the range cut from it, the output container, the engine's capabilities, the reason
/// expected and the frames expected (when the source has a constant 25 fps).
type Case<'a> = (Fixture, f64, f64, Option<&'a str>, &'a Caps, FallbackReason, Option<u64>);

/// T031 (SC-004): every reason known before encoding, each with its source. The job is cut in
/// accurate mode, frame-exact, and says why.
#[tokio::test(flavor = "multi_thread")]
async fn risky_sources_fall_back() {
    use FallbackReason as R;
    if !engine_ok() {
        return;
    }
    let (_, caps) = engine();
    let mut no_x264 = caps.clone();
    no_x264.encoders.remove("libx264");
    let hevc = rmedia_testkit::require_engine(&["libx265"]);
    let mut cases: Vec<Case> = vec![
        // Without libx264 accurate mode cannot write H.264 either, so this one is cut to VP9.
        (Fixture::SmartMp4, 3.32, 8.72, Some("webm"), &no_x264, R::EncoderMissing, Some(135)),
        (Fixture::SmartMp4, 3.32, 8.72, Some("webm"), &caps, R::Container, Some(135)),
        (Fixture::High10, 1.32, 5.0, None, &caps, R::Profile, Some(92)),
        (Fixture::Interlaced, 0.52, 4.0, None, &caps, R::Interlaced, Some(87)),
        (Fixture::VfrMp4, 2.0, 12.0, None, &caps, R::Vfr, None),
        (Fixture::Rotated90, 0.52, 2.5, Some("ts"), &caps, R::Rotation, None),
        (Fixture::SmartMp4, 2.2, 3.8, None, &caps, R::NoFullGop, Some(40)),
        (Fixture::OpenGop, 1.32, 6.72, None, &caps, R::OpenGop, Some(135)),
    ];
    if hevc {
        cases.push((Fixture::HevcMp4, 1.32, 5.0, None, &caps, R::Codec, Some(92)));
    }
    for (kind, start, end, container, caps, reason, frames) in cases {
        let out = tempfile::tempdir().unwrap();
        let src = fixture(kind);
        let o = smart_opts(out.path(), |o| o.container = container.map(str::to_owned));
        let plan = plan_with_caps(&src, start, end, o, caps).await;
        assert!(plan.issues.is_empty(), "{kind:?}: {:?}", plan.issues);
        assert_eq!(fallback_notice(&plan), Some(reason), "{kind:?}");
        let JobSpec::Clip(c) = &plan.jobs[0].spec else { panic!("{kind:?}: not a clip job") };
        assert_eq!((c.mode, c.smart.is_none()), (Mode::Accurate, true), "{kind:?}");
        // T041: the dry run's summary and totals say the same.
        let s = plan.jobs[0].smart.as_ref().expect("a smart summary");
        assert_eq!((s.mode, s.copy, s.copied_share, s.fallback), (Mode::Accurate, None, None, Some(reason)));
        let totals = plan.smart.as_ref().expect("smart totals");
        assert_eq!((totals.smart, totals.fallback.get(&reason), totals.not_applied), (0, Some(&1), 0));
        let (output, smart, events) = run_one(plan, out.path()).await;
        assert_eq!((smart.mode_used, smart.copied_frames, smart.fallback), (Mode::Accurate, 0, Some(reason)));
        assert_eq!(smart.copied_share, 0.0);
        assert_eq!(warned(&events), [Some(reason)], "{kind:?}: the notice is sent as a warning event");
        if let Some(n) = frames {
            assert_eq!(frame_count(&output), n, "{kind:?}");
            assert_eq!(u64::from(smart.reencoded_frames), n, "{kind:?}");
        }
    }
}

/// T032 (scenario 3): reasons found only after the parts are made. The joined file is never
/// kept; the clip is redone in accurate mode with a `SMART_FALLBACK` warning.
#[tokio::test(flavor = "multi_thread")]
async fn run_time_fallback() {
    if !engine_ok() {
        return;
    }
    let (_, caps) = engine();
    let dir = tempfile::tempdir().unwrap();
    let reversed = private_copy(Fixture::SmartMp4, dir.path());
    rmedia_core::smart::test_hooks::reverse_parts_of(&reversed);
    for (src, end, reason, frames) in [
        // The picture shrinks at 4 s, inside the copied GOPs; the tail is the smaller size.
        (rmedia_testkit::res_change(), 6.72, FallbackReason::ParamsChanged, 85),
        // The parts joined in the wrong order.
        (reversed, 20.72, FallbackReason::SeamCheck, 435),
    ] {
        let out = tempfile::tempdir().unwrap();
        let plan = plan_with_caps(&src, 3.32, end, smart_opts(out.path(), |_| {}), &caps).await;
        assert!(plan.issues.is_empty() && plan.jobs[0].notices.is_empty(), "{reason}: {:?}", plan.jobs[0]);
        let (output, smart, events) = run_one(plan, out.path()).await;
        assert_eq!((smart.mode_used, smart.copied_frames, smart.fallback), (Mode::Accurate, 0, Some(reason)));
        assert_eq!(u64::from(smart.reencoded_frames), frames, "{reason}");
        assert_eq!(frame_count(&output), frames, "{reason}");
        assert_eq!(warned(&events), [Some(reason)], "{reason}");
        assert!(partials(out.path()).is_empty(), "{reason}: {:?}", partials(out.path()));
    }
}

/// T033 (scenario 4, FR-009): with `smart_fallback = false` a reason is an error instead.
#[tokio::test(flavor = "multi_thread")]
async fn strict_mode() {
    if !engine_ok() {
        return;
    }
    let (tools, caps) = engine();
    let strict = |out: &Path| smart_opts(out, |o| o.smart_fallback = false);

    // Found while planning: a planning issue, and no job.
    let out = tempfile::tempdir().unwrap();
    let plan = plan_with_caps(&fixture(Fixture::OpenGop), 1.32, 6.72, strict(out.path()), &caps).await;
    assert!(plan.jobs.is_empty(), "{:?}", plan.jobs);
    let [issue] = plan.issues.as_slice() else { panic!("{:?}", plan.issues) };
    assert_eq!(issue.code, IssueCode::SmartUnavailable);
    assert_eq!(issue.detail.as_ref().and_then(|d| d.reason), Some(FallbackReason::OpenGop));
    assert_eq!(issue.message, format!("smart cut not possible: {}", FallbackReason::OpenGop.message()));

    // Found while running: the job fails and nothing is written.
    let src = private_copy(Fixture::SmartMp4, out.path());
    rmedia_core::smart::test_hooks::reverse_parts_of(&src);
    let clips = out.path().join("clips");
    let plan = plan_with_caps(&src, 3.32, 20.72, strict(&clips), &caps).await;
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    let (summary, events) = run_plan(plan, tools, &out.path().join("state")).await;
    let JobOutcome::Failed { error } = &summary.jobs[0].outcome else { panic!("{:?}", summary.jobs[0]) };
    assert_eq!(error.code, "SMART_UNAVAILABLE");
    assert_eq!(error.detail.as_ref().and_then(|d| d.reason), Some(FallbackReason::SeamCheck));
    assert!(warned(&events).is_empty());
    let left: Vec<_> =
        std::fs::read_dir(&clips).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
}

/// The plan's smart totals: (smart, fallback, not applied).
fn totals(plan: &Plan) -> (usize, usize, usize) {
    let t = plan.smart.as_ref().expect("smart totals");
    (t.smart, t.fallback.values().sum(), t.not_applied)
}

/// T034 (FR-013): an upscale or a GIF re-encodes anyway, so smart mode does not apply; the job
/// is planned exactly as in accurate mode, with a `SMART_NOT_APPLIED` notice.
#[tokio::test(flavor = "multi_thread")]
async fn not_applied_with_upscale_or_gif() {
    if !engine_ok() {
        return;
    }
    let (_, caps) = engine();
    let src = fixture(Fixture::SmartMp4);
    let out = tempfile::tempdir().unwrap();
    let upscale = |o: &mut Options| {
        o.upscale = Some(rmedia_core::upscale::UpscaleOptions {
            tier: rmedia_core::upscale::TierChoice::Cpu,
            ..Default::default()
        })
    };
    let gif = |o: &mut Options| o.gif = Some(rmedia_core::gif::GifOptions::default());
    for (what, edit) in [("upscale", &upscale as &dyn Fn(&mut Options)), ("gif", &gif)] {
        let plan_in = |mode: Mode| {
            let mut o = opts(out.path(), mode);
            edit(&mut o);
            plan_with_caps(&src, 3.32, 6.72, o, &caps)
        };
        let (smart, accurate) = (plan_in(Mode::Smart).await, plan_in(Mode::Accurate).await);
        assert!(smart.issues.is_empty(), "{what}: {:?}", smart.issues);
        let (s, a) = (&smart.jobs[0], &accurate.jobs[0]);
        assert!(s.smart.is_none(), "{what}: no smart summary");
        assert_eq!(totals(&smart), (0, 0, 1), "{what}");
        let codes: Vec<_> = s.notices.iter().map(|n| n.code).collect();
        assert!(codes.contains(&IssueCode::SmartNotApplied), "{what}: {codes:?}");
        assert!(!codes.contains(&IssueCode::SmartFallback), "{what}: {codes:?}");
        let message = "smart cut does not apply: the clip is re-encoded for upscaling/GIF";
        assert!(s.warnings.iter().any(|w| w == message), "{what}: {:?}", s.warnings);
        let others: Vec<_> = s.warnings.iter().filter(|w| *w != message).collect();
        assert_eq!(others, a.warnings.iter().collect::<Vec<_>>(), "{what}");
        let commands = |p: &Plan| format!("{:?}", p.jobs[0].commands()).replace(&p.run_id, "RUN");
        assert_eq!(commands(&smart), commands(&accurate), "{what}: the same commands as accurate mode");
    }
}
