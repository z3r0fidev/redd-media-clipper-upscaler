//! T057 (003 US4): portrait and square GIFs through 002's reframe (FR-013, FR-014): the shapes
//! and their widths with centre and blur, smart framing that keeps a moving subject in frame,
//! one analysis for a whole budget search, the no-subject fallback, and no enlarging.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::{Event, JobMeasure, JobOutcome};
use rmedia_core::gif::{BudgetSpec, GifOptions, Shape};
use rmedia_core::plan::{Caps, Plan};
use rmedia_core::request::Options;
use rmedia_core::upscale::Reframe;
use rmedia_core::upscale::reframe::detect::{self, DetectorSpec, FrameView};
use rmedia_testkit::{Fixture, display_size, fixture, require_engine};

fn caps(cache: &Path) -> Caps {
    let (_, mut caps) = engine();
    caps.face = Some(DetectorSpec::Marker);
    caps.reframe_cache = Some(cache.to_path_buf());
    caps
}

fn gif(shape: Shape, framing: Reframe) -> GifOptions {
    GifOptions { shape, framing, budget: None, ..GifOptions::default() }
}

async fn planned(src: &Path, g: GifOptions, out: &Path, caps: &Caps) -> Plan {
    let (tools, _) = engine();
    let o = Options { gif: Some(g), output_dir: Some(out.to_path_buf()), ..Options::default() };
    let p = make_plan(&request(src, vec![clip(0.0, EndSpec::EndOfMedia)], o), &tools, caps).await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    p
}

async fn encode(p: Plan, out: &Path) -> (PathBuf, JobMeasure, Vec<Event>) {
    let (tools, _) = engine();
    let (summary, events) = run_plan(p, tools, out).await;
    match &summary.jobs[0].outcome {
        JobOutcome::Succeeded { output, measure } => (output.clone(), measure.clone(), events),
        other => panic!("{other:?}"),
    }
}

fn warnings(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::JobWarning { message, .. } => Some(message.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn shapes_with_centre_and_blur_have_the_portrait_widths() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let caps = caps(&d.path().join("cache"));
    let src = fixture(Fixture::Land6);
    // Centre crops are never enlarged (FR-004): a 9:16 crop of 1280x720 is 405 px wide. Blur
    // keeps the whole frame as its foreground, so it reaches the target's portrait width.
    for (shape, centre, blur) in [
        (Shape::Tall, (405, 720), (480, 853)),
        (Shape::Feed, (480, 600), (480, 600)),
        (Shape::Square, (720, 720), (720, 720)),
    ] {
        for (framing, want) in [(Reframe::Center, centre), (Reframe::Blur, blur)] {
            let out = d.path().join(format!("{shape:?}-{framing:?}"));
            let (file, _, _) = encode(planned(&src, gif(shape, framing), &out, &caps).await, &out).await;
            assert_eq!(display_size(&file), want, "{shape:?} {framing:?}");
        }
    }
}

/// Frames of a GIF with the marker boxes found in each.
fn markers(file: &Path) -> (Vec<Vec<detect::FaceBox>>, u32) {
    let (w, h) = display_size(file);
    let o = Command::new(rmedia_testkit::ffmpeg())
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(file)
        .args(["-f", "rawvideo", "-pix_fmt", "bgr24", "-"])
        .output()
        .unwrap();
    assert!(o.status.success());
    let det = detect::load(&DetectorSpec::Marker).unwrap();
    let frame = (w * h * 3) as usize;
    let boxes = o
        .stdout
        .chunks_exact(frame)
        .map(|f| det.detect(&FrameView { bgr: f, width: w, height: h }).unwrap())
        .collect();
    (boxes, w)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_smart_4_5_gif_keeps_the_moving_marker_in_frame() {
    require_engine!("palettegen", "paletteuse", "sendcmd", "scdet");
    let d = tempfile::tempdir().unwrap();
    let caps = caps(&d.path().join("cache"));
    let p = planned(&fixture(Fixture::MovingMarker), gif(Shape::Feed, Reframe::Smart), d.path(), &caps).await;
    let (file, _, events) = encode(p, d.path()).await;
    assert!(events.iter().any(|e| matches!(e, Event::JobStage { stage, .. } if stage == "analyze")));
    let (frames, w) = markers(&file);
    let inside = frames.iter().filter(|b| b.len() == 1 && b[0].x > 0.0 && b[0].x + b[0].w < w as f32).count();
    let share = inside as f64 / frames.len() as f64;
    eprintln!("US4: marker inside in {:.1}% of {} GIF frames", share * 100.0, frames.len());
    assert!(share >= 0.95, "marker inside in {:.1}% of frames", share * 100.0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_budget_search_reuses_one_analysis() {
    require_engine!("palettegen", "paletteuse", "sendcmd", "scdet");
    let d = tempfile::tempdir().unwrap();
    let cache = d.path().join("cache");
    let caps = caps(&cache);
    let mut g = gif(Shape::Feed, Reframe::Smart);
    g.budget = Some(BudgetSpec { soft: Some(70_000), hard: None });
    g.fps = Some(25);
    let p = planned(&fixture(Fixture::MovingMarker), g, &d.path().join("a"), &caps).await;
    let (_, m, events) = encode(p, &d.path().join("a")).await;
    let JobMeasure::Gif { search: Some(r), .. } = &m else { panic!("{m:?}") };
    let samples = r.steps.iter().filter(|s| s.kind == rmedia_core::gif::budget::StepKind::Sample).count();
    eprintln!("US4 search: {samples} samples, lowered {:?}", r.lowered);
    assert!(samples >= 2, "several samples share the analysis: {r:?}");
    let analyses =
        events.iter().filter(|e| matches!(e, Event::JobStage { stage, .. } if stage == "analyze")).count();
    assert_eq!(analyses, 1, "one analysis for every sample and encode");
    let entries = std::fs::read_dir(&cache).map_or(0, |r| r.count());
    assert!(entries >= 1, "the analysis is cached");
}

#[tokio::test(flavor = "multi_thread")]
async fn no_subject_keeps_the_centred_crop_and_says_so() {
    require_engine!("palettegen", "paletteuse", "sendcmd", "scdet");
    let d = tempfile::tempdir().unwrap();
    let caps = caps(&d.path().join("cache"));
    // FlatUi has no magenta, so the marker detector finds nothing (Land6's test pattern has some).
    let p = planned(&fixture(Fixture::FlatUi), gif(Shape::Tall, Reframe::Smart), d.path(), &caps).await;
    let (file, _, events) = encode(p, d.path()).await;
    assert_eq!(display_size(&file), (405, 720), "the centred crop");
    let w = warnings(&events);
    assert!(w.iter().any(|m| m.starts_with("smart framing found no subject")), "{w:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_large_portrait_input_is_only_scaled_down() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let caps = caps(&d.path().join("cache"));
    let src = fixture(Fixture::CleanPortrait); // 1080x1920, like a 002 output
    let (file, _, _) = encode(
        planned(&src, gif(Shape::Source, Reframe::Center), &d.path().join("a"), &caps).await,
        d.path(),
    )
    .await;
    assert_eq!(display_size(&file).0, 480, "the target's portrait width");
    let wide = GifOptions { width: Some(1600), ..gif(Shape::Source, Reframe::Center) };
    let p = planned(&src, wide, &d.path().join("b"), &caps).await;
    assert!(
        p.jobs[0].warnings.iter().any(|w| w.contains("kept source width 1080")),
        "{:?}",
        p.jobs[0].warnings
    );
}
