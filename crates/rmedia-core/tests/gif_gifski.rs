//! T038 (003 US1): the gifski engine (research R7, R14, FR-023, SC-010). Encodes need gifski
//! (`require_tool!(gifski)`); the fallback tests run without it. Cancelling the pipe is tested
//! in `cancel.rs`, which owns the child-process checks.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::error::IssueCode;
use rmedia_core::event::{JobMeasure, JobOutcome};
use rmedia_core::gif::engines::Engines;
use rmedia_core::gif::plan::EngineUsed;
use rmedia_core::gif::{EngineChoice, GifOptions, LoopCount};
use rmedia_core::plan::{JobSpec, Plan, Program};
use rmedia_core::request::Options;
use rmedia_testkit::{Fixture, fixture, require_engine, require_tool};

fn gif(o: GifOptions, out: &Path) -> Options {
    Options { gif: Some(o), output_dir: Some(out.to_path_buf()), ..Options::default() }
}

fn max_quality() -> GifOptions {
    GifOptions { target: "max-quality".into(), ..GifOptions::default() }
}

async fn planned(src: &Path, o: GifOptions, out: &Path, without_gifski: bool) -> Plan {
    let (tools, mut caps) = engine();
    if without_gifski {
        caps.gif_engines = Engines::default();
    }
    make_plan(&request(src, vec![clip(0.0, EndSpec::EndOfMedia)], gif(o, out)), &tools, &caps).await
}

fn engine_of(p: &Plan) -> EngineUsed {
    let JobSpec::Gif(g) = &p.jobs[0].spec else { panic!("not a GIF job") };
    g.gif.engine
}

async fn encode(src: &Path, o: GifOptions, out: &Path) -> (PathBuf, JobMeasure) {
    let p = planned(src, o, out, false).await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    let (tools, _) = engine();
    let (summary, _) = run_plan(p, tools, out).await;
    match &summary.jobs[0].outcome {
        JobOutcome::Succeeded { output, measure, .. } => (output.clone(), measure.clone()),
        other => panic!("GIF failed: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn max_quality_encodes_through_the_gifski_pipe() {
    require_engine!("palettegen");
    require_tool!(gifski);
    let d = tempfile::tempdir().unwrap();
    let o = GifOptions { width: Some(320), ..max_quality() };
    let p = planned(&fixture(Fixture::Land6), o.clone(), d.path(), false).await;
    assert_eq!(engine_of(&p), EngineUsed::Gifski);
    let programs: Vec<Program> = p.jobs[0].commands().iter().map(|c| c.program).collect();
    assert_eq!(programs, [Program::Ffmpeg, Program::Gifski], "ffmpeg piped into gifski");

    let (output, measure) = encode(&fixture(Fixture::Land6), o, d.path()).await;
    let JobMeasure::Gif { engine, width, colours, optimiser, deterministic, .. } = measure else {
        panic!("{measure:?}")
    };
    assert_eq!((engine, width, colours, optimiser), (EngineUsed::Gifski, 320, None, false));
    assert!(deterministic, "gifski passed the Phase 0 determinism check (research R14)");
    assert_eq!(rmedia_testkit::gif_loop(&output), Some(0), "loops forever");
    let delays = rmedia_testkit::gif_delays(&output);
    assert!(delays.iter().all(|&d| d >= 2), "{delays:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_gifski_gif_that_plays_once_has_no_loop_block() {
    require_engine!("palettegen");
    require_tool!(gifski);
    let d = tempfile::tempdir().unwrap();
    let o = GifOptions { width: Some(160), loop_count: LoopCount::Plays(1), ..max_quality() };
    let (output, _) = encode(&fixture(Fixture::Land6), o, d.path()).await;
    // gifski's own `--repeat -1` writes repeat 0 (forever): the executor removes the block.
    assert_eq!(rmedia_testkit::gif_loop(&output), None);
}

/// SC-007 for gifski: the same request gives the same bytes (research R14, T013).
#[tokio::test(flavor = "multi_thread")]
async fn a_gifski_gif_is_byte_reproducible() {
    require_engine!("palettegen");
    require_tool!(gifski);
    let d = tempfile::tempdir().unwrap();
    let o = GifOptions { width: Some(320), ..max_quality() };
    let (a, _) = encode(&fixture(Fixture::Busy10), o.clone(), &d.path().join("a")).await;
    let (b, _) = encode(&fixture(Fixture::Busy10), o, &d.path().join("b")).await;
    assert!(std::fs::read(&a).unwrap() == std::fs::read(&b).unwrap(), "two gifski runs differ");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_gifski_falls_back_to_the_built_in_engine() {
    require_engine!("palettegen");
    let d = tempfile::tempdir().unwrap();
    let p = planned(&fixture(Fixture::Land6), max_quality(), d.path(), true).await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    assert_eq!(engine_of(&p), EngineUsed::Builtin);
    let codes: Vec<IssueCode> = p.jobs[0].notices.iter().map(|n| n.code).collect();
    assert_eq!(codes, [IssueCode::EngineFallback], "{:?}", p.jobs[0].notices);

    let strict = GifOptions { strict_engine: true, ..max_quality() };
    let p = planned(&fixture(Fixture::Land6), strict, d.path(), true).await;
    assert!(p.issues.iter().any(|i| i.code == IssueCode::EngineMissing), "{:?}", p.issues);
}

#[tokio::test(flavor = "multi_thread")]
async fn built_in_settings_get_a_note_with_gifski() {
    require_engine!("palettegen");
    require_tool!(gifski);
    let d = tempfile::tempdir().unwrap();
    let o = GifOptions { colours: Some(64), ..max_quality() };
    let p = planned(&fixture(Fixture::Land6), o, d.path(), false).await;
    assert!(
        p.jobs[0].warnings.iter().any(|w| w == "--colors is ignored by gifski"),
        "{:?}",
        p.jobs[0].warnings
    );
    assert!(p.jobs[0].notices.is_empty(), "a note, not a coded warning: {:?}", p.jobs[0].notices);
}

/// SC-010: gifski scores above the built-in engine at the same width and frame rate.
#[tokio::test(flavor = "multi_thread")]
async fn gifski_beats_the_built_in_engine_on_vmaf() {
    require_engine!("palettegen", "paletteuse", "libvmaf");
    require_tool!(gifski);
    for f in [Fixture::Land6, Fixture::Busy10] {
        let d = tempfile::tempdir().unwrap();
        let src = fixture(f);
        let base = GifOptions { width: Some(480), fps: Some(20), budget: None, ..max_quality() };
        let builtin = GifOptions { engine: EngineChoice::Builtin, ..base.clone() };
        let (a, _) = encode(&src, base, &d.path().join("gifski")).await;
        let (b, _) = encode(&src, builtin, &d.path().join("builtin")).await;
        let (va, vb) = (rmedia_testkit::vmaf_gif(&a, &src, 20), rmedia_testkit::vmaf_gif(&b, &src, 20));
        let (sa, sb) = (std::fs::metadata(&a).unwrap().len(), std::fs::metadata(&b).unwrap().len());
        eprintln!("SC-010 {f:?}: gifski VMAF {va:.2} ({sa} bytes), built-in {vb:.2} ({sb} bytes)");
        assert!(va > vb, "{f:?}: gifski {va:.2} is not above the built-in engine's {vb:.2}");
    }
}
