//! T054 (003 US3): the Reddit MP4 (research R6, FR-005): silent, fast-start H.264 at the source's
//! rate (up to `mp4.max_fps`), a CRF ladder before any width drop, ≥ 5× smaller than the GIF at
//! the same width and rate (SC-006), the "MP4 would be smaller" hint on Reddit-post GIFs, and
//! each format working without the other's engine features.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::error::IssueCode;
use rmedia_core::event::{JobMeasure, JobOutcome};
use rmedia_core::gif::{BudgetSpec, Format, GifOptions};
use rmedia_core::plan::{Caps, JobSpec, Plan};
use rmedia_core::request::Options;
use rmedia_testkit::{Fixture, fixture, require_engine};
use tokio_util::sync::CancellationToken;

fn mp4(edit: impl FnOnce(&mut GifOptions)) -> GifOptions {
    let mut g = GifOptions { format: Format::Mp4, budget: None, ..GifOptions::default() };
    edit(&mut g);
    g
}

async fn plan_with(src: &Path, g: GifOptions, out: &Path, caps: Caps) -> Plan {
    let (tools, _) = engine();
    let o = Options { gif: Some(g), output_dir: Some(out.to_path_buf()), ..Options::default() };
    make_plan(&request(src, vec![clip(0.0, EndSpec::EndOfMedia)], o), &tools, &caps).await
}

async fn planned(src: &Path, g: GifOptions, out: &Path) -> Plan {
    let p = plan_with(src, g, out, engine().1).await;
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
async fn land6_as_a_reddit_mp4_is_silent_fast_start_h264_at_the_source_rate() {
    require_engine!("libx264");
    let d = tempfile::tempdir().unwrap();
    let p = planned(&fixture(Fixture::Land6), mp4(|_| {}), d.path()).await;
    let (out, m) = encode(p, d.path()).await;
    assert_eq!(out.extension().and_then(|e| e.to_str()), Some("mp4"));
    let JobMeasure::Gif { format, width, height, fps, colours, .. } = m else { unreachable!() };
    assert_eq!((format, colours), (Format::Mp4, None));
    assert!(width % 2 == 0 && height % 2 == 0, "{width}x{height}");
    assert_eq!(fps, 30.0, "the source's 30 fps is kept (max_fps 60)");
    assert!(rmedia_core::gif::verify::mp4_faststart(&out).unwrap(), "moov before mdat");
    let (tools, _) = engine();
    let info = rmedia_core::probe::probe(&tools, &out, &CancellationToken::new()).await.unwrap();
    assert_eq!(info.streams.len(), 1, "video only: {:?}", info.streams);
    let v = info.first_video().unwrap();
    assert_eq!((v.codec.as_str(), v.codec_tag.as_deref()), ("h264", Some("avc1")));
    let tags = (
        v.color_space.as_deref(),
        v.color_primaries.as_deref(),
        v.color_transfer.as_deref(),
        v.color_range.as_deref(),
    );
    assert_eq!(tags, (Some("bt709"), Some("bt709"), Some("bt709"), Some("tv")), "BT.709 limited, tagged");
    let dur = v.duration.or(info.duration).unwrap().as_secs_f64();
    assert!((dur - 6.0).abs() <= 1.0 / 30.0 + 0.02, "{dur}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_mp4_budget_lowers_the_crf_before_the_width() {
    require_engine!("libx264");
    let d = tempfile::tempdir().unwrap();
    let g = mp4(|g| g.budget = Some(BudgetSpec { soft: Some(150_000), hard: None }));
    let p = planned(&fixture(Fixture::Busy10), g, d.path()).await;
    let (_, m) = encode(p, d.path()).await;
    let JobMeasure::Gif { search: Some(r), .. } = &m else { panic!("{m:?}") };
    assert!(bytes(&m) <= 150_000, "{}", bytes(&m));
    let lowered: Vec<&str> = r.lowered.iter().map(|l| l.setting.as_str()).collect();
    assert_eq!(lowered.first(), Some(&"crf"), "{lowered:?}");
    assert!(lowered.iter().all(|s| ["crf", "width"].contains(s)), "{lowered:?}");
    if lowered.contains(&"width") {
        let crf = r.lowered.iter().find(|l| l.setting == "crf").unwrap();
        assert_eq!(crf.to, 32, "the CRF ladder is spent before the width drops: {crf:?}");
    }
}

/// SC-006: at the same width and frame rate the MP4 is at least 5× smaller than the GIF.
#[tokio::test(flavor = "multi_thread")]
async fn sc006_the_mp4_is_five_times_smaller_than_the_gif() {
    require_engine!("libx264", "palettegen", "paletteuse");
    for f in [Fixture::Land6, Fixture::Busy10] {
        let d = tempfile::tempdir().unwrap();
        let src = fixture(f);
        let gif = GifOptions { budget: None, width: Some(640), fps: Some(15), ..GifOptions::default() };
        let (_, gm) = encode(planned(&src, gif, &d.path().join("g")).await, d.path()).await;
        let mp4o = mp4(|g| {
            g.width = Some(640);
            g.fps = Some(15);
        });
        let (_, mm) = encode(planned(&src, mp4o, &d.path().join("m")).await, d.path()).await;
        let (g, m) = (bytes(&gm), bytes(&mm));
        eprintln!("SC-006 {f:?}: GIF {g} bytes, MP4 {m} bytes ({:.1}×)", g as f64 / m as f64);
        assert!(g >= 5 * m, "SC-006 {f:?}: GIF {g} vs MP4 {m}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reddit_post_gif_says_the_mp4_would_be_smaller() {
    require_engine!("libx264", "palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::Land6);
    let mut p = planned(&src, GifOptions::default(), d.path()).await;
    let (tools, _) = engine();
    rmedia_core::gif::estimate::prepare_in(&mut p, &tools, &CancellationToken::new(), scratch.path())
        .await
        .unwrap();
    let JobSpec::Gif(g) = &p.jobs[0].spec else { panic!() };
    let hint = g.gif.estimate.mp4_bytes.expect("an MP4 estimate in the dry run");
    let gif_est = g.gif.estimate.bytes.unwrap();
    assert!(hint < gif_est, "{hint} vs {gif_est}");

    let (_, m) = encode(planned(&src, GifOptions::default(), &d.path().join("run")).await, d.path()).await;
    let JobMeasure::Gif { mp4_estimate, .. } = m else { unreachable!() };
    assert!(mp4_estimate.is_some_and(|b| b < bytes(&m)), "{mp4_estimate:?}");

    let comment = GifOptions { target: "reddit-comment".into(), ..GifOptions::default() };
    let (_, m) = encode(planned(&src, comment, &d.path().join("c")).await, d.path()).await;
    let JobMeasure::Gif { mp4_estimate, .. } = m else { unreachable!() };
    assert_eq!(mp4_estimate, None, "only targets with mp4.hint");
}

#[tokio::test(flavor = "multi_thread")]
async fn each_format_works_without_the_others_engine_features() {
    require_engine!("libx264", "palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::Land6);
    let (_, caps) = engine();
    let mut no_x264 = caps.clone();
    no_x264.encoders.remove("libx264");
    let p = plan_with(&src, mp4(|_| {}), d.path(), no_x264.clone()).await;
    assert!(p.issues.iter().any(|i| i.code == IssueCode::EngineMissing), "{:?}", p.issues);
    let p = plan_with(&src, GifOptions::default(), d.path(), no_x264).await;
    assert!(!p.has_errors(), "the GIF still works: {:?}", p.issues);

    let mut no_palette = caps;
    no_palette.filters.remove("palettegen");
    no_palette.filters.remove("paletteuse");
    let p = plan_with(&src, GifOptions::default(), d.path(), no_palette.clone()).await;
    assert!(p.issues.iter().any(|i| i.code == IssueCode::EngineMissing), "{:?}", p.issues);
    let p = plan_with(&src, mp4(|_| {}), d.path(), no_palette).await;
    assert!(!p.has_errors(), "the MP4 still works: {:?}", p.issues);
}
