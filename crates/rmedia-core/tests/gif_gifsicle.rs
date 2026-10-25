//! T046 (003 US2): the gifsicle post-pass (research R2, FR-024): `-O3` and `--lossy` shrink the
//! GIF, the loop block survives, only generated names reach its argv, and without gifsicle the
//! job skips the pass (`ENGINE_FALLBACK` only when `--lossy` was asked for).

mod common;

use std::path::{Path, PathBuf};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::error::IssueCode;
use rmedia_core::event::{JobMeasure, JobOutcome};
use rmedia_core::gif::engines::Engines;
use rmedia_core::gif::{GifOptions, LoopCount};
use rmedia_core::plan::{JobSpec, Plan, Program};
use rmedia_core::request::Options;
use rmedia_testkit::{Fixture, fixture, gif_loop, require_engine, require_tool};

fn opts(out: &Path, edit: impl FnOnce(&mut GifOptions)) -> Options {
    let mut g = GifOptions { budget: None, width: Some(480), ..GifOptions::default() };
    edit(&mut g);
    Options { gif: Some(g), output_dir: Some(out.to_path_buf()), ..Options::default() }
}

async fn planned(src: &Path, o: Options, without_gifsicle: bool) -> Plan {
    let (tools, mut caps) = engine();
    if without_gifsicle {
        caps.gif_engines = Engines::default();
    }
    let p = make_plan(&request(src, vec![clip(0.0, EndSpec::EndOfMedia)], o), &tools, &caps).await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    p
}

async fn encode(p: Plan, out: &Path) -> (PathBuf, JobMeasure) {
    let (tools, _) = engine();
    let summary = run_plan(p, tools, out).await.0;
    match &summary.jobs[0].outcome {
        JobOutcome::Succeeded { output, measure } => (output.clone(), measure.clone()),
        other => panic!("{other:?}"),
    }
}

fn bytes(m: &JobMeasure) -> u64 {
    let JobMeasure::Gif { bytes, .. } = m else { panic!("{m:?}") };
    *bytes
}

#[tokio::test(flavor = "multi_thread")]
async fn o3_and_lossy_shrink_the_gif_and_keep_its_loop() {
    require_engine!("palettegen", "paletteuse");
    require_tool!(gifsicle);
    let src = fixture(Fixture::Land6);
    let d = tempfile::tempdir().unwrap();
    let plain =
        encode(planned(&src, opts(&d.path().join("a"), |g| g.optimise = false), false).await, d.path()).await;
    let o3 =
        encode(planned(&src, opts(&d.path().join("b"), |g| g.lossy = Some(0)), false).await, d.path()).await;
    let lossy =
        encode(planned(&src, opts(&d.path().join("c"), |g| g.lossy = Some(80)), false).await, d.path()).await;
    let (p, o, l) = (bytes(&plain.1), bytes(&o3.1), bytes(&lossy.1));
    eprintln!("gifsicle on Land6 480 px: plain {p}, -O3 {o}, --lossy=80 {l}");
    assert!(o < p, "-O3 shrinks: {o} vs {p}");
    assert!(l < o, "--lossy shrinks more: {l} vs {o}");
    let JobMeasure::Gif { optimiser, lossy: level, .. } = &lossy.1 else { unreachable!() };
    assert_eq!((*optimiser, *level), (true, Some(80)));
    assert_eq!(gif_loop(&lossy.0), Some(0), "the loop block survives -O3");

    let thrice = |g: &mut GifOptions| g.loop_count = LoopCount::Plays(3);
    let (out, _) = encode(planned(&src, opts(&d.path().join("d"), thrice), false).await, d.path()).await;
    assert_eq!(gif_loop(&out), Some(2), "3 plays = repeat 2, kept by gifsicle");
}

#[tokio::test(flavor = "multi_thread")]
async fn only_generated_names_reach_gifsicle() {
    require_engine!("palettegen", "paletteuse");
    require_tool!(gifsicle);
    let d = tempfile::tempdir().unwrap();
    let p = planned(&fixture(Fixture::Land6), opts(d.path(), |_| {}), false).await;
    let cmds = p.jobs[0].commands();
    let g = cmds.iter().find(|c| c.program == Program::Gifsicle).expect("a gifsicle pass");
    let args: Vec<String> = g.args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let end = args.iter().position(|a| a == "--").expect("`--` before the input");
    assert_eq!(args[end + 1..], ["enc.gif"], "{args:?}");
    let o = args.iter().position(|a| a == "-o").unwrap();
    assert_eq!(args[o + 1], "opt.gif");
    assert!(args.iter().all(|a| !a.contains('/') && !a.contains('\\')), "no paths: {args:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn without_gifsicle_the_pass_is_skipped() {
    require_engine!("palettegen", "paletteuse");
    let src = fixture(Fixture::Land6);
    let d = tempfile::tempdir().unwrap();
    let p = planned(&src, opts(d.path(), |_| {}), true).await;
    let JobSpec::Gif(g) = &p.jobs[0].spec else { panic!("not a GIF job") };
    assert!(g.gif.optimiser.is_none());
    assert!(p.jobs[0].commands().iter().all(|c| c.program != Program::Gifsicle));
    let codes: Vec<IssueCode> = p.jobs[0].notices.iter().map(|n| n.code).collect();
    assert!(!codes.contains(&IssueCode::EngineFallback), "the target's lossy is not a request: {codes:?}");
    let (_, m) = encode(p, d.path()).await;
    let JobMeasure::Gif { optimiser, lossy, .. } = m else { unreachable!() };
    assert_eq!((optimiser, lossy), (false, None));

    let asked = planned(&src, opts(&d.path().join("b"), |g| g.lossy = Some(40)), true).await;
    let codes: Vec<IssueCode> = asked.jobs[0].notices.iter().map(|n| n.code).collect();
    assert!(codes.contains(&IssueCode::EngineFallback), "--lossy without gifsicle: {codes:?}");
}
