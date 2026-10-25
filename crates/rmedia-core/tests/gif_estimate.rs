//! T036 (003 US1): the sample-based size estimate (research R4). The model's error against the
//! real output is recorded here for each fixture; the SC-004 bound is asserted in T053.

mod common;

use std::path::Path;

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::JobOutcome;
use rmedia_core::gif::GifOptions;
use rmedia_core::gif::plan::EstimateFrom;
use rmedia_core::plan::{JobSpec, Plan};
use rmedia_core::request::Options;
use rmedia_testkit::{Fixture, fixture, require_engine};
use tokio_util::sync::CancellationToken;

async fn planned(src: &Path, end: EndSpec, out: &Path) -> Plan {
    let (tools, caps) = engine();
    let opts = Options {
        gif: Some(GifOptions { budget: None, ..GifOptions::default() }),
        output_dir: Some(out.to_path_buf()),
        ..Options::default()
    };
    let p = make_plan(&request(src, vec![clip(0.0, end)], opts), &tools, &caps).await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    p
}

fn estimate(p: &Plan) -> (Option<u64>, EstimateFrom) {
    let JobSpec::Gif(g) = &p.jobs[0].spec else { panic!("not a GIF job") };
    (g.gif.estimate.bytes, g.gif.estimate.from)
}

/// Estimates, then encodes: (estimate, real bytes).
async fn estimate_and_real(src: &Path, end: EndSpec) -> (u64, u64) {
    let out = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let (tools, _) = engine();
    let mut p = planned(src, end, out.path()).await;
    assert_eq!(estimate(&p), (None, EstimateFrom::Pending), "the planner never samples");
    rmedia_core::gif::estimate::prepare_in(&mut p, &tools, &CancellationToken::new(), scratch.path())
        .await
        .unwrap();
    assert!(p.jobs[0].warnings.iter().all(|w| !w.contains("no size estimate")), "{:?}", p.jobs[0].warnings);
    let (Some(est), EstimateFrom::Sample) = estimate(&p) else { panic!("{:?}", estimate(&p)) };
    assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0, "the sample folder is removed");
    let summary = run_plan(p, tools, out.path()).await.0;
    let JobOutcome::Succeeded { output, .. } = &summary.jobs[0].outcome else {
        panic!("{:?}", summary.jobs[0].outcome)
    };
    (est, std::fs::metadata(output).unwrap().len())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_short_range_is_sampled_whole_and_exact() {
    require_engine!("palettegen", "paletteuse");
    let (est, real) = estimate_and_real(&fixture(Fixture::Land6), EndSpec::At(secs(3.0))).await;
    assert_eq!(est, real, "a range of 4 s or less is the output itself");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_model_is_recorded_against_the_real_size() {
    require_engine!("palettegen", "paletteuse");
    for f in [Fixture::Land6, Fixture::FlatUi, Fixture::Static6, Fixture::Busy10] {
        let (est, real) = estimate_and_real(&fixture(f), EndSpec::EndOfMedia).await;
        let error = (est as f64 - real as f64) / real as f64;
        eprintln!("estimate {f:?}: {est} bytes for {real} real ({:+.1}%)", error * 100.0);
        // A sanity bound only: SC-004 (90% within 25%) is measured over the US2 set (T053).
        assert!(error.abs() < 1.0, "{f:?}: {est} vs {real}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_the_sample_is_an_error_and_leaves_no_folder() {
    require_engine!("palettegen", "paletteuse");
    let out = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let (tools, _) = engine();
    let mut p = planned(&fixture(Fixture::Busy10), EndSpec::EndOfMedia, out.path()).await;
    let cancel = CancellationToken::new();
    cancel.cancel();
    let r = rmedia_core::gif::estimate::prepare_in(&mut p, &tools, &cancel, scratch.path()).await;
    assert!(matches!(r, Err(rmedia_core::Error::Cancelled)), "{r:?}");
    assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);
}
