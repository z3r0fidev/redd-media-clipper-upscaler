//! T047 (003 US2): the automatic size budget (research R5, FR-009–FR-012): the search fits a
//! busy clip into 3 MB and lists what it lowered, repeats itself byte for byte, refuses an
//! unreachable budget without writing anything, trims only when allowed, and warns between the
//! soft and hard limits. Cancelling during `search` is tested in `cancel.rs`.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::error::IssueCode;
use rmedia_core::event::{Event, JobMeasure, JobOutcome, RunSummary};
use rmedia_core::gif::budget::{SearchRecord, StepKind};
use rmedia_core::gif::plan::EstimateFrom;
use rmedia_core::gif::{BudgetSpec, GifOptions};
use rmedia_core::plan::{JobSpec, Plan};
use rmedia_core::request::Options;
use rmedia_testkit::{Fixture, fixture, require_engine, require_tool};
use tokio_util::sync::CancellationToken;

/// The ladder's order (FR-009): `lowered` lists a subsequence of it.
const LADDER: [&str; 7] = ["lossy", "dither", "colours", "lossy_quality", "crf", "fps", "width"];

fn budgeted(out: &Path, soft: u64, hard: Option<u64>, allow_trim: bool) -> Options {
    let gif = GifOptions {
        budget: Some(BudgetSpec { soft: Some(soft), hard }),
        allow_trim,
        ..GifOptions::default()
    };
    Options { gif: Some(gif), output_dir: Some(out.to_path_buf()), ..Options::default() }
}

async fn planned(src: &Path, opts: Options) -> Plan {
    let (tools, caps) = engine();
    let p = make_plan(&request(src, vec![clip(0.0, EndSpec::EndOfMedia)], opts), &tools, &caps).await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    p
}

async fn run(p: Plan, out: &Path) -> (RunSummary, Vec<Event>) {
    let (tools, _) = engine();
    run_plan(p, tools, out).await
}

fn succeeded(s: &RunSummary) -> (PathBuf, u64, SearchRecord, JobMeasure) {
    match &s.jobs[0].outcome {
        JobOutcome::Succeeded { output, measure } => {
            let JobMeasure::Gif { bytes, search, .. } = measure else { panic!("{measure:?}") };
            (output.clone(), *bytes, search.clone().expect("a search record"), measure.clone())
        }
        other => panic!("the GIF job did not succeed: {other:?}"),
    }
}

fn gifs(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = walk(dir).into_iter().filter(|n| n.ends_with(".gif")).collect();
    v.sort();
    v
}

fn walk(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p.to_string_lossy().into_owned());
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn busy10_fits_3_mb_and_lists_what_it_lowered() {
    require_engine!("palettegen", "paletteuse");
    require_tool!(gifsicle);
    let d = tempfile::tempdir().unwrap();
    let p = planned(&fixture(Fixture::Busy10), budgeted(d.path(), 3_000_000, None, false)).await;
    let JobSpec::Gif(g) = &p.jobs[0].spec else { panic!("not a GIF job") };
    let b = g.gif.budget.expect("a budget");
    assert_eq!((b.soft, b.hard), (3_000_000, 3_000_000), "--budget alone sets hard too (R5)");

    let (summary, _) = run(p, d.path()).await;
    let (output, bytes, record, _) = succeeded(&summary);
    assert!(bytes <= 3_000_000, "{bytes}");
    assert_eq!(std::fs::metadata(&output).unwrap().len(), bytes);
    assert!(record.encodes <= 10, "{record:?}");
    let lowered: Vec<&str> = record.lowered.iter().map(|l| l.setting.as_str()).collect();
    assert!(!lowered.is_empty(), "a busy clip needs lowering");
    let order: Vec<usize> = lowered.iter().map(|s| LADDER.iter().position(|l| l == s).unwrap()).collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "ladder order: {lowered:?}");
    let encodes = record.steps.iter().filter(|s| s.kind != StepKind::Post).count();
    assert_eq!(encodes, usize::from(record.encodes), "posts are not encodes");
    eprintln!("US2 Busy10 → 3 MB: {bytes} bytes, {} encodes, lowered {lowered:?}", record.encodes);
}

#[tokio::test(flavor = "multi_thread")]
async fn two_budgeted_runs_are_byte_identical() {
    require_engine!("palettegen", "paletteuse");
    require_tool!(gifsicle);
    let mut results = Vec::new();
    for _ in 0..2 {
        let d = tempfile::tempdir().unwrap();
        let p = planned(&fixture(Fixture::Busy10), budgeted(d.path(), 3_000_000, None, false)).await;
        let (output, _, record, _) = succeeded(&run(p, d.path()).await.0);
        results.push((std::fs::read(output).unwrap(), record.steps));
    }
    assert!(results[0].0 == results[1].0, "the GIFs differ");
    assert_eq!(results[0].1, results[1].1, "the search steps differ");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreachable_budget_writes_nothing_and_says_what_fits() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let p = planned(&fixture(Fixture::Busy10), budgeted(d.path(), 100_000, None, false)).await;
    let (summary, _) = run(p, d.path()).await;
    let JobOutcome::Failed { error } = &summary.jobs[0].outcome else {
        panic!("{:?}", summary.jobs[0].outcome)
    };
    assert_eq!(error.code, "BUDGET_UNREACHABLE", "{error:?}");
    let detail = error.detail.as_ref().expect("detail");
    let fits = detail.fits_secs.expect("fits_secs");
    assert!(fits > 0.0 && fits < 10.0, "{fits}");
    assert!(detail.best_bytes.is_some_and(|b| b > 100_000), "{detail:?}");
    assert_eq!(summary.exit_code, 1);
    assert_eq!(gifs(d.path()), Vec::<String>::new(), "nothing is written");
    assert!(partials(d.path()).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn allow_trim_keeps_the_part_that_fits_and_reports_it() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let p = planned(&fixture(Fixture::Busy10), budgeted(d.path(), 100_000, None, true)).await;
    let (summary, _) = run(p, d.path()).await;
    let (output, bytes, _, measure) = succeeded(&summary);
    let JobMeasure::Gif { trimmed, .. } = measure else { unreachable!() };
    let span = trimmed.expect("trimmed");
    let kept = span.end.saturating_sub(span.start).as_secs_f64();
    assert!(kept > 0.0 && kept < 10.0, "{span:?}");
    assert!(bytes <= 100_000, "{bytes}");
    let probed =
        rmedia_testkit::gif_delays(&output).iter().map(|&c| u64::from(c)).sum::<u64>() as f64 / 100.0;
    assert!((probed - kept).abs() < 0.25, "the GIF lasts {probed} s, the record says {kept} s");
}

#[tokio::test(flavor = "multi_thread")]
async fn between_soft_and_hard_the_gif_is_kept_with_a_warning() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let p = planned(&fixture(Fixture::Busy10), budgeted(d.path(), 100_000, Some(50_000_000), false)).await;
    let (summary, events) = run(p, d.path()).await;
    let (_, bytes, record, _) = succeeded(&summary);
    assert!(bytes > 100_000 && bytes <= 50_000_000, "{bytes}");
    assert_eq!(record.fit, rmedia_core::gif::budget::Fit::Hard);
    let warned =
        events.iter().any(|e| matches!(e, Event::JobWarning { code: Some(IssueCode::OverRecommended), .. }));
    assert!(warned, "OVER_RECOMMENDED");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_dry_run_estimate_is_the_searchs_prediction() {
    require_engine!("palettegen", "paletteuse");
    require_tool!(gifsicle);
    let d = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let mut p = planned(&fixture(Fixture::Busy10), budgeted(d.path(), 3_000_000, None, false)).await;
    let (tools, _) = engine();
    rmedia_core::gif::estimate::prepare_in(&mut p, &tools, &CancellationToken::new(), scratch.path())
        .await
        .unwrap();
    let JobSpec::Gif(g) = &p.jobs[0].spec else { panic!("not a GIF job") };
    let est = g.gif.estimate.clone();
    assert_eq!(est.from, EstimateFrom::Search, "{:?}", p.jobs[0].warnings);
    assert!(est.bytes.is_some_and(|b| b <= 3_000_000), "{est:?}");
    assert!(!est.lowered.is_empty(), "the lowered settings are listed: {est:?}");

    let (summary, _) = run(p, d.path()).await;
    let (_, _, record, _) = succeeded(&summary);
    assert_eq!(est.lowered, record.lowered, "the dry run predicts the real search");
}

/// T080 L5: the samples see only the flat windows, so the first full encode misses soft; the
/// executor's retry encodes once more under its own scratch names, keeps the smaller result
/// and leaves nothing behind.
#[tokio::test(flavor = "multi_thread")]
async fn a_full_encode_that_misses_soft_is_retried_once() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let mut opts = budgeted(d.path(), 200_000, Some(50_000_000), false);
    let gif = opts.gif.as_mut().unwrap();
    (gif.width, gif.fps, gif.optimise) = (Some(320), Some(10), false);
    let p = planned(&fixture(Fixture::HiddenBusy10), opts).await;
    let (summary, _) = run(p, d.path()).await;
    let (output, bytes, record, _) = succeeded(&summary);
    let fulls: Vec<_> = record.steps.iter().filter(|s| s.kind == StepKind::Full).collect();
    assert!(
        fulls[0].bytes > 200_000 && fulls[0].estimate <= 200_000,
        "the samples underestimated: {record:?}"
    );
    assert_ne!(fulls[0].settings, fulls[1].settings, "the retry is one step lower");
    assert_eq!(bytes, fulls[0].bytes.min(fulls[1].bytes), "the smaller full encode is kept");
    assert_eq!(std::fs::metadata(&output).unwrap().len(), bytes);
    assert!(record.encodes <= 10);
    assert!(partials(d.path()).is_empty(), "no scratch or .part left: {:?}", walk(d.path()));
}
