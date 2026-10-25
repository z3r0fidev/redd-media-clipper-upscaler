//! T064 (003 US5), the spec's independent test: a manifest with two videos and three clips, one
//! of them `reddit-comment`, gives three GIFs with the right budgets; a fourth clip that cannot
//! fit its budget fails alone; nothing is left behind.

mod common;

use std::path::Path;

use common::*;
use rmedia_core::event::JobOutcome;
use rmedia_core::input::manifest_json;
use rmedia_core::plan::JobSpec;
use rmedia_core::request::Options;
use rmedia_testkit::{Fixture, fixture, require_engine};

fn gifs(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .map(|p| if p.is_dir() { gifs(&p) } else { usize::from(p.extension().is_some_and(|x| x == "gif")) })
        .sum()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_manifest_of_two_videos_and_three_clips() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let (land, busy) = (fixture(Fixture::Land6), fixture(Fixture::Busy10));
    let text = format!(
        r#"{{ "version": 1, "defaults": {{ "gif": {{ "target": "reddit-post", "width": 320 }} }},
             "videos": [
               {{ "input": {land:?}, "clips": [
                   {{ "start": 0, "end": 2 }},
                   {{ "start": 2, "end": 4, "options": {{ "gif": {{ "target": "reddit-comment" }} }} }} ] }},
               {{ "input": {busy:?}, "clips": [
                   {{ "start": 0, "end": 2 }},
                   {{ "start": 2, "end": 8, "options": {{ "gif": {{ "width": 720, "budget": {{ "soft": 100000 }} }} }} }} ] }} ] }}"#
    );
    let defaults = Options { output_dir: Some(d.path().join("out")), ..Options::default() };
    let req = manifest_json::parse(&d.path().join("jobs.json"), &text, &defaults).unwrap();
    let (tools, caps) = engine();
    let p = make_plan(&req, &tools, &caps).await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    assert_eq!(p.jobs.len(), 4);
    let budgets: Vec<(String, u64, u64)> = p
        .jobs
        .iter()
        .map(|j| {
            let JobSpec::Gif(g) = &j.spec else { panic!("not a GIF job") };
            let b = g.gif.budget.unwrap();
            (g.gif.target.id.clone(), b.soft, b.hard)
        })
        .collect();
    assert_eq!(budgets[0], ("reddit-post".into(), 8_000_000, 20_000_000));
    assert_eq!(budgets[1], ("reddit-comment".into(), 3_000_000, 8_000_000));
    assert_eq!(budgets[3].1, 100_000, "the clip's own budget");

    let (summary, _) = run_plan(p, tools, d.path()).await;
    let ok = summary.jobs.iter().filter(|j| matches!(j.outcome, JobOutcome::Succeeded { .. })).count();
    assert_eq!(ok, 3, "{:?}", summary.jobs.iter().map(|j| &j.outcome).collect::<Vec<_>>());
    let JobOutcome::Failed { error } = &summary.jobs[3].outcome else {
        panic!("{:?}", summary.jobs[3].outcome)
    };
    assert_eq!(error.code, "BUDGET_UNREACHABLE", "one failure does not stop the others");
    assert_eq!(gifs(&d.path().join("out")), 3);
    assert!(partials(d.path()).is_empty(), "no leftovers");
}
