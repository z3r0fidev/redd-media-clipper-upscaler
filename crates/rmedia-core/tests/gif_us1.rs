//! T033 (003 US1): a GIF that fits a Reddit post, one test per acceptance scenario and edge
//! case: defaults (≤ 8 MB, 720 wide, 15 fps in 6/7 cs delays, looping), a narrower source, an
//! approximate and a rejected frame rate, a range within one frame, the loop table, alpha, HDR,
//! rotation, a variable frame rate, a too-short range, a long range and output collisions.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::error::IssueCode;
use rmedia_core::event::{JobMeasure, JobOutcome, RunSummary};
use rmedia_core::gif::{GifOptions, LoopCount};
use rmedia_core::plan::{JobSpec, Plan, Severity};
use rmedia_core::request::{OnExists, Options};
use rmedia_testkit::{Fixture, display_size, fixture, gif_delays, gif_loop, gif_opaque, require_engine};

fn gif_opts(out: &Path, edit: impl FnOnce(&mut GifOptions)) -> Options {
    let mut g = GifOptions::default();
    edit(&mut g);
    Options { gif: Some(g), output_dir: Some(out.to_path_buf()), ..Options::default() }
}

async fn plan_of(src: &Path, start: f64, end: EndSpec, opts: Options) -> Plan {
    let (tools, caps) = engine();
    make_plan(&request(src, vec![clip(start, end)], opts), &tools, &caps).await
}

async fn run(p: Plan, state: &Path) -> RunSummary {
    assert!(!p.has_errors(), "{:?}", p.issues);
    let (tools, _) = engine();
    run_plan(p, tools, state).await.0
}

/// The output and measure of a succeeded GIF job.
fn gif_result(s: &RunSummary) -> (PathBuf, JobMeasure) {
    match &s.jobs[0].outcome {
        JobOutcome::Succeeded { output, measure } => {
            assert!(matches!(measure, JobMeasure::Gif { .. }), "{measure:?}");
            (output.clone(), measure.clone())
        }
        other => panic!("the GIF job did not succeed: {other:?}"),
    }
}

fn notes(p: &Plan) -> String {
    p.jobs.iter().flat_map(|j| j.warnings.iter().cloned()).collect::<Vec<_>>().join(" | ")
}

#[tokio::test(flavor = "multi_thread")]
async fn land6_defaults_fit_a_reddit_post() {
    require_engine!("palettegen", "paletteuse");
    let out = tempfile::tempdir().unwrap();
    let p = plan_of(&fixture(Fixture::Land6), 0.0, EndSpec::EndOfMedia, gif_opts(out.path(), |_| {})).await;
    let job = &p.jobs[0];
    assert_eq!(job.kind(), "gif");
    assert_eq!(job.output.file_name().unwrap(), "land6_reddit-post.gif");
    assert!(job.notices.iter().any(|n| n.code == IssueCode::TimingApproximate), "{:?}", job.notices);
    let s = run(p, out.path()).await;
    let (output, measure) = gif_result(&s);
    let bytes = std::fs::metadata(&output).unwrap().len();
    assert!(bytes <= 8_000_000, "{bytes} bytes");
    assert_eq!(display_size(&output), (720, 405), "720 wide, the source's shape");
    let delays = gif_delays(&output);
    assert!(delays.iter().all(|d| *d == 6 || *d == 7), "{delays:?}");
    assert!((delays.len() as i64 - 90).abs() <= 1, "{} frames", delays.len());
    assert_eq!(gif_loop(&output), Some(0), "loops forever");
    let JobMeasure::Gif { bytes: b, width, height, fps, span, timing, loop_count, optimiser, .. } = measure
    else {
        unreachable!()
    };
    assert_eq!((b, width, height), (bytes, 720, 405));
    assert!((fps - 15.0).abs() < 1e-9 && !timing.exact, "{fps} {timing:?}");
    assert!((span.end.as_secs_f64() - 6.0).abs() < 0.05, "{span:?}");
    assert_eq!(loop_count, LoopCount::Forever);
    assert_eq!(optimiser, rmedia_testkit::gif_tool(rmedia_testkit::GifTool::Gifsicle).is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_narrower_source_keeps_its_width_with_a_note() {
    require_engine!("palettegen", "paletteuse");
    let out = tempfile::tempdir().unwrap();
    let p =
        plan_of(&fixture(Fixture::Narrow), 0.0, EndSpec::At(secs(1.0)), gif_opts(out.path(), |_| {})).await;
    assert!(notes(&p).contains("kept 640"), "{}", notes(&p));
    let (output, _) = gif_result(&run(p, out.path()).await);
    assert_eq!(display_size(&output), (640, 360));
    // An explicit width above the source is not an upscale either.
    let p = plan_of(
        &fixture(Fixture::Land6),
        0.0,
        EndSpec::At(secs(1.0)),
        gif_opts(out.path(), |g| g.width = Some(1600)),
    )
    .await;
    assert!(notes(&p).contains("kept source width 1280"), "{}", notes(&p));
    let JobSpec::Gif(g) = &p.jobs[0].spec else { panic!() };
    assert_eq!(g.gif.start.width, 1280);
}

#[tokio::test(flavor = "multi_thread")]
async fn thirty_fps_is_approximate_and_fifty_one_is_rejected() {
    require_engine!("palettegen", "paletteuse");
    let out = tempfile::tempdir().unwrap();
    let p = plan_of(
        &fixture(Fixture::Land6),
        0.0,
        EndSpec::At(secs(2.0)),
        gif_opts(out.path(), |g| g.fps = Some(30)),
    )
    .await;
    assert!(notes(&p).contains("30 fps is shown as 3–4 cs"), "{}", notes(&p));
    let (output, _) = gif_result(&run(p, out.path()).await);
    let delays = gif_delays(&output);
    assert!(delays.iter().all(|d| *d == 3 || *d == 4), "{delays:?}");
    let p = plan_of(
        &fixture(Fixture::Land6),
        0.0,
        EndSpec::At(secs(2.0)),
        gif_opts(out.path(), |g| g.fps = Some(51)),
    )
    .await;
    let e = p.issues.iter().find(|i| i.severity == Severity::Error).expect("rejected");
    assert_eq!(e.code, IssueCode::InvalidInput);
    assert!(e.message.contains("50 fps or less"), "{}", e.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_range_is_within_one_frame() {
    require_engine!("palettegen", "paletteuse");
    let out = tempfile::tempdir().unwrap();
    let p =
        plan_of(&fixture(Fixture::Land6), 1.0, EndSpec::At(secs(3.0)), gif_opts(out.path(), |_| {})).await;
    let (output, _) = gif_result(&run(p, out.path()).await);
    let delays = gif_delays(&output);
    assert!((delays.len() as i64 - 30).abs() <= 1, "{} frames", delays.len());
    let total: u32 = delays.iter().sum();
    assert!((193..=207).contains(&total), "{total} cs");
}

#[tokio::test(flavor = "multi_thread")]
async fn plays_follow_the_loop_table() {
    require_engine!("palettegen", "paletteuse");
    let out = tempfile::tempdir().unwrap();
    for (count, want, name) in [
        (LoopCount::Plays(1), None, "once"),
        (LoopCount::Plays(3), Some(2), "three"),
        (LoopCount::Forever, Some(0), "forever"),
    ] {
        let opts = Options {
            name_template: format!("{{source}}_{name}.{{ext}}"),
            ..gif_opts(out.path(), |g| g.loop_count = count)
        };
        let p = plan_of(&fixture(Fixture::Narrow), 0.0, EndSpec::At(secs(0.5)), opts).await;
        let (output, _) = gif_result(&run(p, out.path()).await);
        assert_eq!(gif_loop(&output), want, "{count:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn alpha_sources_give_an_opaque_gif() {
    require_engine!("palettegen", "paletteuse", "drawbox", "ffv1");
    let out = tempfile::tempdir().unwrap();
    let p =
        plan_of(&fixture(Fixture::Alpha), 0.0, EndSpec::At(secs(1.0)), gif_opts(out.path(), |_| {})).await;
    let JobSpec::Gif(g) = &p.jobs[0].spec else { panic!() };
    assert!(g.frame.graph.flatten_alpha);
    let (output, _) = gif_result(&run(p, out.path()).await);
    assert!(gif_opaque(&output));
}

#[tokio::test(flavor = "multi_thread")]
async fn hdr_sources_are_tone_mapped() {
    require_engine!("palettegen", "paletteuse", "zscale", "tonemap", "libx265");
    let out = tempfile::tempdir().unwrap();
    let p = plan_of(&fixture(Fixture::HdrTagged), 0.0, EndSpec::At(secs(1.0)), gif_opts(out.path(), |_| {}))
        .await;
    let JobSpec::Gif(g) = &p.jobs[0].spec else { panic!() };
    assert!(g.frame.graph.tonemap);
    let (output, _) = gif_result(&run(p, out.path()).await);
    assert_eq!(display_size(&output), (640, 360), "narrower than 720: kept");
}

#[tokio::test(flavor = "multi_thread")]
async fn rotated_sources_are_turned_before_scaling() {
    require_engine!("palettegen", "paletteuse");
    let out = tempfile::tempdir().unwrap();
    let p = plan_of(&fixture(Fixture::Rotated90), 0.0, EndSpec::At(secs(1.0)), gif_opts(out.path(), |_| {}))
        .await;
    let (output, _) = gif_result(&run(p, out.path()).await);
    assert_eq!(display_size(&output), (240, 320), "portrait, at the source's width");
}

#[tokio::test(flavor = "multi_thread")]
async fn variable_frame_rate_sources_keep_their_duration() {
    require_engine!("palettegen", "paletteuse");
    let out = tempfile::tempdir().unwrap();
    let p =
        plan_of(&fixture(Fixture::VfrMp4), 2.0, EndSpec::At(secs(8.0)), gif_opts(out.path(), |_| {})).await;
    let (output, _) = gif_result(&run(p, out.path()).await);
    let total: u32 = gif_delays(&output).iter().sum();
    assert!((593..=607).contains(&total), "lasts {total} cs, not 600");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_range_under_two_frames_is_rejected() {
    require_engine!("palettegen", "paletteuse");
    let out = tempfile::tempdir().unwrap();
    let p =
        plan_of(&fixture(Fixture::Land6), 1.0, EndSpec::At(secs(1.05)), gif_opts(out.path(), |_| {})).await;
    let e = p.issues.iter().find(|i| i.severity == Severity::Error).expect("rejected");
    assert_eq!(e.code, IssueCode::InvalidInput);
    assert!(e.message.contains("at least 2 frames"), "{}", e.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_long_range_warns_but_still_plans() {
    require_engine!("palettegen", "paletteuse");
    let out = tempfile::tempdir().unwrap();
    // reddit-comment recommends at most 6 s.
    let p = plan_of(
        &fixture(Fixture::Busy10),
        0.0,
        EndSpec::EndOfMedia,
        gif_opts(out.path(), |g| g.target = "reddit-comment".into()),
    )
    .await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    let job = &p.jobs[0];
    let n = job.notices.iter().find(|n| n.code == IssueCode::LongRange).expect("LONG_RANGE");
    assert!(job.warnings.contains(&n.message), "every notice is also a plain warning");
    assert!(n.message.contains("6 s"), "{}", n.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_target_is_an_input_error() {
    let out = tempfile::tempdir().unwrap();
    let p = plan_of(
        &fixture(Fixture::Narrow),
        0.0,
        EndSpec::At(secs(1.0)),
        gif_opts(out.path(), |g| g.target = "myspace".into()),
    )
    .await;
    let e = p.issues.iter().find(|i| i.severity == Severity::Error).expect("rejected");
    assert_eq!(e.code, IssueCode::TargetUnknown);
    assert!(e.message.contains("reddit-post"), "lists the known targets: {}", e.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_collision_follows_on_exists() {
    require_engine!("palettegen", "paletteuse");
    let out = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::Narrow);
    let first =
        run(plan_of(&src, 0.0, EndSpec::At(secs(0.5)), gif_opts(out.path(), |_| {})).await, out.path()).await;
    let (output, _) = gif_result(&first);
    let skip = plan_of(&src, 0.0, EndSpec::At(secs(0.5)), gif_opts(out.path(), |_| {})).await;
    assert_ne!(skip.jobs[0].disposition, rmedia_core::plan::Disposition::Run, "skipped by default");
    let opts = Options { on_exists: OnExists::Rename, ..gif_opts(out.path(), |_| {}) };
    let renamed = plan_of(&src, 0.0, EndSpec::At(secs(0.5)), opts).await;
    assert_ne!(renamed.jobs[0].output, output);
    assert!(
        renamed.jobs[0].output.to_string_lossy().ends_with("_reddit-post_1.gif"),
        "{:?}",
        renamed.jobs[0].output
    );
    assert!(partials(out.path()).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_without_room_for_the_gifs_gets_one_warning() {
    // C1: GIF jobs count towards the free-space check, per output folder.
    require_engine!("palettegen", "paletteuse");
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let src = fixture(Fixture::Land6);
    let mut jobs = plan_of(&src, 0.0, EndSpec::EndOfMedia, gif_opts(a.path(), |_| {})).await.jobs;
    jobs.extend(plan_of(&src, 0.0, EndSpec::EndOfMedia, gif_opts(b.path(), |_| {})).await.jobs);
    let needs = rmedia_core::plan::space_needs(&jobs);
    assert_eq!(needs.len(), 2, "{needs:?}");
    assert!(needs.values().all(|n| *n == 20_000_000), "the hard budget before any sample: {needs:?}");
    let short = a.path().to_path_buf();
    let w = rmedia_core::upscale::estimate::check_space(&needs, |d| {
        Some(if d == short { 1_000_000 } else { 1 << 40 })
    });
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].starts_with(&*short.to_string_lossy()), "{w:?}");
}
