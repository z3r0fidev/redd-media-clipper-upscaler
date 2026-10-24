//! T048 (002 US2): smart reframe end to end with the test marker detector (research R7):
//! the marker stays in frame (SC-004), the crop jumps only at the cut, far-apart subjects fall
//! back to the blurred background (FR-014), and a cached analysis renders identically.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::{JobMeasure, JobOutcome};
use rmedia_core::plan::{Caps, JobSpec, Plan};
use rmedia_core::request::Options;
use rmedia_core::upscale::reframe::detect::{self, DetectorSpec, FrameView};
use rmedia_core::upscale::reframe::{ReframeAnalysis, path};
use rmedia_core::upscale::{AnalysisState, CacheUse, Motion, Reframe, UpscaleOptions, prepare_plan};
use rmedia_testkit::{Fixture, fixture, require_engine, sha256};
use tokio_util::sync::CancellationToken;

fn caps(cache: &Path, face: Option<DetectorSpec>) -> Caps {
    let (_, mut caps) = engine();
    caps.face = face;
    caps.reframe_cache = Some(cache.to_path_buf());
    caps
}

fn opts(out: &Path, reframe: Reframe) -> Options {
    let u = UpscaleOptions { reframe, ..UpscaleOptions::default() };
    Options { upscale: Some(u), output_dir: Some(out.to_path_buf()), ..Options::default() }
}

async fn planned(kind: Fixture, out: &Path, caps: &Caps, reframe: Reframe) -> Plan {
    let (tools, _) = engine();
    let mut p = make_plan(
        &request(&fixture(kind), vec![clip(0.0, EndSpec::EndOfMedia)], opts(out, reframe)),
        &tools,
        caps,
    )
    .await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    prepare_plan(&mut p, &tools, &CancellationToken::new()).await.unwrap();
    p
}

fn analysis(p: &Plan) -> (ReframeAnalysis, CacheUse) {
    match &p.jobs[0].spec {
        JobSpec::Upscale(u) => match &u.up.analysis {
            AnalysisState::Ready(a, c) => ((**a).clone(), *c),
            other => panic!("not analysed: {other:?}"),
        },
        _ => panic!("not an upscale job"),
    }
}

async fn render(p: Plan, out: &Path) -> (PathBuf, JobMeasure) {
    let (tools, _) = engine();
    let (summary, _) = run_plan(p, tools, out).await;
    match &summary.jobs[0].outcome {
        JobOutcome::Succeeded { output, measure } => (output.clone(), measure.clone()),
        other => panic!("render failed: {other:?}"),
    }
}

/// Output frames at a quarter of 1080x2340, with the marker boxes found in each.
fn markers_per_frame(file: &Path) -> (Vec<Vec<detect::FaceBox>>, (u32, u32)) {
    let (w, h) = (270u32, 585u32);
    let o = Command::new(rmedia_testkit::ffmpeg())
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(file)
        .args(["-vf", &format!("scale={w}:{h}:flags=area"), "-f", "rawvideo", "-pix_fmt", "bgr24", "-"])
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
    (boxes, (w, h))
}

#[tokio::test(flavor = "multi_thread")]
async fn moving_subject_stays_in_frame_and_the_crop_jumps_only_at_the_cut() {
    require_engine!("libx265", "zscale", "cas", "sendcmd", "scdet");
    let d = tempfile::tempdir().unwrap();
    let caps = caps(&d.path().join("cache"), Some(DetectorSpec::Marker));
    let p = planned(Fixture::MovingMarker, &d.path().join("a"), &caps, Reframe::Smart).await;
    let (a, cache) = analysis(&p);
    assert_eq!(cache, CacheUse::Miss);
    assert_eq!(a.shots.len(), 2, "{:?}", a.shots);
    let cut = a.shots[1].span.start.as_secs_f64();
    assert!((cut - 6.0).abs() <= 1.0 / 30.0 + 1e-6, "cut at {cut}");
    assert!(a.fallback_ranges.is_empty(), "{:?}", a.fallback_ranges);
    let lim = path::limits(Motion::Normal, f64::from(a.crop.0), a.fps);
    let cut_frame = (cut * a.fps).round() as usize;
    for k in 1..a.path.len() {
        let step = f64::from(a.path[k] - a.path[k - 1]).abs();
        if k.abs_diff(cut_frame) > 1 {
            assert!(step <= lim.speed + 2.0, "frame {k}: step {step} over {}", lim.speed);
        }
    }

    let (out, measure) = render(p, &d.path().join("a")).await;
    assert!(matches!(&measure, JobMeasure::Upscale { fallback_ranges, .. } if fallback_ranges.is_empty()));
    let (frames, (w, _)) = markers_per_frame(&out);
    let inside: Vec<bool> =
        frames.iter().map(|b| b.len() == 1 && b[0].x > 0.0 && b[0].x + b[0].w < w as f32).collect();
    let share = inside.iter().filter(|i| **i).count() as f64 / inside.len() as f64;
    assert!(share >= 0.95, "SC-004: marker fully inside in {:.1}% of frames", share * 100.0);
    let longest_out = inside.split(|i| *i).map(<[bool]>::len).max().unwrap_or(0);
    eprintln!("SC-004: inside {:.1}% of {} frames, longest out {longest_out}", share * 100.0, inside.len());
    assert!(longest_out <= 15, "SC-004: marker out of frame for {longest_out} frames in a row");

    // A second run reuses the cached analysis and renders the same bytes.
    let again = planned(Fixture::MovingMarker, &d.path().join("b"), &caps, Reframe::Smart).await;
    assert_eq!(analysis(&again).1, CacheUse::Hit);
    let (out2, _) = render(again, &d.path().join("b")).await;
    assert_eq!(sha256(&out2), sha256(&out), "cold and warm cache render identically");
}

#[tokio::test(flavor = "multi_thread")]
async fn subjects_too_far_apart_fall_back_to_the_blurred_background() {
    require_engine!("libx265", "zscale", "cas", "sendcmd", "scdet", "gblur");
    let d = tempfile::tempdir().unwrap();
    let caps = caps(&d.path().join("cache"), Some(DetectorSpec::Marker));
    let p = planned(Fixture::FarMarkers, d.path(), &caps, Reframe::Smart).await;
    let (a, _) = analysis(&p);
    assert_eq!(a.fallback_ranges.len(), 1, "{:?}", a.fallback_ranges);
    let r = a.fallback_ranges[0];
    assert!(r.start.as_secs_f64() < 0.01 && r.end.as_secs_f64() > 5.9, "{r:?}");
    let (out, measure) = render(p, d.path()).await;
    assert!(matches!(&measure, JobMeasure::Upscale { fallback_ranges, .. } if fallback_ranges.len() == 1));
    let (frames, _) = markers_per_frame(&out);
    let mid = &frames[frames.len() / 2];
    assert_eq!(mid.len(), 2, "the whole frame (both markers) over the blurred background: {mid:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn other_reframes_and_a_missing_model() {
    require_engine!("libx265", "zscale", "cas", "gblur");
    let d = tempfile::tempdir().unwrap();
    for reframe in [Reframe::Center, Reframe::Blur] {
        let p =
            planned(Fixture::MovingMarker, d.path(), &caps(d.path(), Some(DetectorSpec::Marker)), reframe)
                .await;
        let argv = rmedia_core::plan::ffargs::display_args(&p.jobs[0].args()).join(" ");
        assert!(!argv.contains("sendcmd"), "{reframe:?} applies to the whole video");
    }
    let p = planned(Fixture::MovingMarker, d.path(), &caps(d.path(), None), Reframe::Smart).await;
    assert!(p.jobs[0].warnings.iter().any(|w| w == "face model not found; using blurred background"));
    let JobSpec::Upscale(u) = &p.jobs[0].spec else { panic!() };
    assert_eq!(u.up.reframe_applied, Reframe::Blur);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_clip_of_the_source_is_tracked_in_clip_time() {
    require_engine!("libx265", "zscale", "cas", "sendcmd", "scdet");
    let d = tempfile::tempdir().unwrap();
    let caps = caps(&d.path().join("cache"), Some(DetectorSpec::Marker));
    let (tools, _) = engine();
    // 2 s – 8 s: the cut (source 6 s) lands 4 s into the clip.
    let req = request(
        &fixture(Fixture::MovingMarker),
        vec![clip(2.0, EndSpec::At(secs(8.0)))],
        opts(d.path(), Reframe::Smart),
    );
    let mut p = make_plan(&req, &tools, &caps).await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    prepare_plan(&mut p, &tools, &CancellationToken::new()).await.unwrap();
    let (a, _) = analysis(&p);
    assert_eq!(a.path.len(), 180);
    assert_eq!(a.shots.len(), 2, "{:?}", a.shots);
    let cut = a.shots[1].span.start.as_secs_f64();
    assert!((cut - 6.0).abs() <= 1.0 / 30.0 + 1e-6, "cut reported in source time: {cut}");
    let (out, _) = render(p, d.path()).await;
    let (frames, (w, _)) = markers_per_frame(&out);
    let inside = frames.iter().filter(|b| b.len() == 1 && b[0].x > 0.0 && b[0].x + b[0].w < w as f32).count();
    assert!(inside as f64 >= 0.95 * frames.len() as f64, "{inside}/{} frames", frames.len());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_analysis_that_fails_while_rendering_falls_back_to_the_blurred_background() {
    require_engine!("libx265", "zscale", "cas", "gblur");
    let d = tempfile::tempdir().unwrap();
    // A model path that does not exist: planning accepts it, loading fails in the job.
    let caps = caps(&d.path().join("cache"), Some(DetectorSpec::Yunet(d.path().join("missing.onnx"))));
    let (tools, _) = engine();
    let p = make_plan(
        &request(
            &fixture(Fixture::MovingMarker),
            vec![clip(0.0, EndSpec::EndOfMedia)],
            opts(d.path(), Reframe::Smart),
        ),
        &tools,
        &caps,
    )
    .await;
    let (summary, events) = run_plan(p, tools, d.path()).await;
    assert!(matches!(summary.jobs[0].outcome, JobOutcome::Succeeded { .. }), "{:?}", summary.jobs[0].outcome);
    let warned = events.iter().any(|e| matches!(e, rmedia_core::event::Event::JobWarning { message, .. } if message.contains("using the blurred background")));
    assert!(warned, "the fallback is reported");
}

/// T018 (003, refactor B2): analysis through `ReframeInput` keys and reuses the cache exactly as
/// 002 did. The key is rebuilt here from the job's own fields the way 002 computed it.
#[tokio::test(flavor = "multi_thread")]
async fn reframe_input_keeps_the_002_cache_key_and_lookup() {
    use rmedia_core::upscale::reframe::{analyze_cached, cache, cache_key, lookup_cached};
    require_engine!("libx264", "zscale", "cas", "hqdn3d", "sendcmd", "scdet");
    let (tools, _) = engine();
    let dir = tempfile::tempdir().unwrap();
    let caps = caps(&dir.path().join("cache"), Some(DetectorSpec::Marker));
    let p = planned(Fixture::MovingMarker, dir.path(), &caps, Reframe::Smart).await;
    let (a, first) = analysis(&p);
    assert_eq!(first, CacheUse::Miss);
    let JobSpec::Upscale(u) = &p.jobs[0].spec else { panic!("not an upscale job") };
    let input = u.reframe_input();
    let key = cache_key(&tools, &input).await.unwrap();
    assert_eq!(key, a.key, "the analysis was stored under the input's key");

    let src = std::fs::canonicalize(&u.cut.input).unwrap();
    let meta = std::fs::metadata(&src).unwrap();
    let span = u.cut.requested;
    let crop = u.up.geometry.crop.unwrap();
    let by_002 = cache::key(&cache::KeyInputs {
        source: src.to_string_lossy().into_owned(),
        size: meta.len(),
        mtime_ns: meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as i128,
        span: (span.start.as_secs_f64(), span.end.as_secs_f64()),
        chain: rmedia_core::upscale::graph::analysis_graph(&u.up.graph, u.up.analysis_size),
        analysis_size: u.up.analysis_size,
        crop: (crop.w, crop.h),
        fps: u.up.fps_out,
        motion: u.up.motion,
        algo: rmedia_core::upscale::reframe::ALGO_VERSION,
        detector: detect::load(&DetectorSpec::Marker).unwrap().id(),
        ffmpeg: tools.version.text().to_owned(),
    });
    assert_eq!(key, by_002, "the key is derived from the same inputs as in 002");

    let cached = lookup_cached(&tools, &input).await.unwrap().expect("found in the cache");
    assert_eq!(*cached, a);
    let mut pending = u.clone();
    pending.up.analysis = AnalysisState::Pending;
    let (again, hit) =
        analyze_cached(&tools, &pending.reframe_input(), &CancellationToken::new(), |_| {}).await.unwrap();
    assert_eq!((hit, &*again), (CacheUse::Hit, &a));
}
