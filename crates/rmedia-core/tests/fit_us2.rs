//! Feature 005, US2: length and resolution limits (tasks T023–T025). Too-long ranges and
//! unreachable budgets are refused before any encode, or trimmed; sizes step down to the floor,
//! and the picture is never enlarged.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::error::IssueCode;
use rmedia_core::event::{JobMeasure, JobOutcome, Span};
use rmedia_core::fit::target::VideoTargetRegistry;
use rmedia_core::fit::{FitOptions, FitOutcome};
use rmedia_core::plan::{Caps, JobSpec, Plan};
use rmedia_core::request::Mode;
use rmedia_testkit::{Fixture, decode_errors, fixture, probe_json};

fn engine_ok() -> bool {
    rmedia_testkit::require_engine(&["libx264", "aac"])
}

/// A user target folder in `dir` with `short` (10 s) and `slow` (24 fps) next to the built-ins.
fn user_targets(dir: &Path) -> VideoTargetRegistry {
    let folder = dir.join("video-targets");
    std::fs::create_dir_all(&folder).unwrap();
    for (id, max_duration, max_fps) in [("short", 10, 60), ("slow", 900, 24)] {
        let json = serde_json::json!({
            "schema": 1, "id": id, "version": 1, "name": id, "description": "a test target",
            "verified": false, "max_bytes": 50_000_000, "max_duration": max_duration,
            "long_side": 1920, "floor_short_side": 480, "max_fps": max_fps,
            "quality": {"crf": 20, "max_bpp": 0.12, "min_bpp": 0.03},
            "audio": {"kbps": 128, "floor_kbps": 64}
        });
        std::fs::write(folder.join(format!("{id}.json")), json.to_string()).unwrap();
    }
    let reg = VideoTargetRegistry::load(Some(dir));
    assert!(reg.problems().is_empty(), "{:?}", reg.problems());
    reg
}

fn caps_with_targets(dir: &Path) -> (rmedia_core::tools::Tools, Caps) {
    let (tools, mut caps) = engine();
    caps.video_targets = user_targets(dir);
    (tools, caps)
}

fn target(id: &str) -> FitOptions {
    FitOptions { target: id.into(), ..FitOptions::default() }
}

async fn plan_fit(src: &Path, start: f64, end: f64, fit: FitOptions, out: &Path) -> Plan {
    let (tools, caps) = caps_with_targets(out);
    let o = rmedia_core::request::Options { fit: Some(fit), ..opts(out, Mode::Fast) };
    make_plan(&request(src, vec![clip(start, EndSpec::At(secs(end)))], o), &tools, &caps).await
}

/// The planned fit of the plan's only job.
fn planned(p: &Plan) -> &rmedia_core::fit::plan::FitPlan {
    assert!(!p.has_errors(), "{:?}", p.issues);
    match &p.jobs[0].spec {
        JobSpec::Clip(c) => c.fit.as_ref().expect("a fit"),
        other => panic!("{other:?}"),
    }
}

fn codes(p: &Plan) -> Vec<IssueCode> {
    p.jobs.iter().flat_map(|j| j.notices.iter().map(|n| n.code)).collect()
}

fn error(p: &Plan, code: IssueCode) -> &rmedia_core::plan::Issue {
    p.issues.iter().find(|i| i.code == code).unwrap_or_else(|| panic!("no {code:?} in {:?}", p.issues))
}

/// Plans and runs one fit; returns the output, its range and its fit block.
async fn run(src: &Path, start: f64, end: f64, fit: FitOptions, out: &Path) -> (PathBuf, Span, FitOutcome) {
    let p = plan_fit(src, start, end, fit, out).await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    let tools = engine().0;
    let (summary, _) = run_plan(p, tools, &out.join("state")).await;
    match &summary.jobs[0].outcome {
        JobOutcome::Succeeded { output, measure: JobMeasure::Clip { actual, fit: Some(f), .. } } => {
            (output.clone(), *actual, f.clone())
        }
        other => panic!("{other:?}"),
    }
}

/// The first video stream's probe entry.
fn video(p: &Path) -> serde_json::Value {
    let j = probe_json(p);
    j["streams"].as_array().unwrap().iter().find(|s| s["codec_type"] == "video").unwrap().clone()
}

fn size_of(p: &Path) -> (u64, u64) {
    let v = video(p);
    (v["width"].as_u64().unwrap(), v["height"].as_u64().unwrap())
}

fn video_secs(p: &Path) -> f64 {
    video(p)["duration"].as_str().and_then(|s| s.parse().ok()).expect("a video duration")
}

/// R4: the smallest cap that fits `len` seconds at the floor size (854x480 at 30 fps), with
/// the audio at its floor.
fn min_cap(len: f64) -> u64 {
    let need = 0.03 * 854.0 * 480.0 * 30.0 / 1000.0 + 64.0;
    (need * 1000.0 / 8.0 * len / 0.98).ceil() as u64
}

/// T023: refusals and trims, decided before any encode.
#[tokio::test(flavor = "multi_thread")]
async fn too_long_and_unreachable_ranges_are_refused_or_trimmed() {
    if !engine_ok() {
        return;
    }
    let src = fixture(Fixture::FitBusy);
    let out = tempfile::tempdir().unwrap();

    // 25 s against a 10 s limit.
    let p = plan_fit(&src, 2.0, 27.0, target("short"), out.path()).await;
    let e = error(&p, IssueCode::RangeTooLong);
    let d = e.detail.as_ref().expect("detail");
    assert_eq!((d.len, d.max_length), (Some(25.0), Some(10.0)), "{e:?}");
    assert!(p.jobs.is_empty());

    // The same with trimming allowed: the first 10 s.
    let p = plan_fit(&src, 2.0, 27.0, FitOptions { allow_trim: true, ..target("short") }, out.path()).await;
    let f = planned(&p);
    assert_eq!((f.len, f.trimmed_from), (10.0, Some(25.0)));
    assert_eq!(p.jobs[0].requested, Span { start: secs(2.0), end: secs(12.0) });
    assert!(codes(&p).contains(&IssueCode::FitTrimmed), "{:?}", p.jobs[0].notices);

    // A cap no size can meet: refused with the smallest cap that would fit.
    let tiny = FitOptions { max_size: Some(200_000), ..FitOptions::default() };
    let p = plan_fit(&src, 2.0, 27.0, tiny.clone(), out.path()).await;
    let e = error(&p, IssueCode::BudgetUnreachable);
    let got = e.detail.as_ref().and_then(|d| d.min_cap).expect("min_cap");
    assert!(got.abs_diff(min_cap(25.0)) <= 2, "{got} vs {}", min_cap(25.0));
    assert!(min_cap(25.0) > 200_000);

    // With trimming allowed: as long a start as the cap holds, at the floor.
    let p = plan_fit(&src, 2.0, 27.0, FitOptions { allow_trim: true, ..tiny }, out.path()).await;
    let f = planned(&p);
    let fits = 200_000.0 * 0.98 / (min_cap(1.0) as f64 * 0.98);
    assert!((f.len - fits).abs() < 0.1, "{} vs {fits}", f.len);
    assert_eq!((f.size, f.audio_kbps), ((854, 480), Some(64)));
    assert!(codes(&p).contains(&IssueCode::FitTrimmed) && codes(&p).contains(&IssueCode::FitLowered));

    // A tight cap: one step down the ladder, with a notice.
    let p = plan_fit(
        &src,
        2.0,
        12.0,
        FitOptions { max_size: Some(2_000_000), ..FitOptions::default() },
        out.path(),
    )
    .await;
    assert_eq!(planned(&p).size, (1440, 810));
    let n = p.jobs[0].notices.iter().find(|n| n.code == IssueCode::FitLowered).expect("FIT_LOWERED");
    assert!(n.message.contains("1920") && n.message.contains("1440"), "{}", n.message);
}

/// T024: a trimmed run is the range's first part; sizes are lowered when the budget needs it,
/// fitted to the target's long side, and never enlarged.
#[tokio::test(flavor = "multi_thread")]
async fn trims_and_sizes_are_what_was_planned() {
    if !engine_ok() {
        return;
    }
    let out = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::FitBusy);
    let frame = 1.0 / 30.0;

    let trim = FitOptions { allow_trim: true, ..target("short") };
    let (file, actual, f) = run(&src, 2.0, 27.0, trim, out.path()).await;
    assert!(f.trimmed, "{f:?}");
    assert!((actual.start.as_secs_f64() - 2.0).abs() <= frame, "{actual:?}");
    assert!((video_secs(&file) - 10.0).abs() <= frame + 1e-3, "{}", video_secs(&file));
    assert!(f.bytes <= 50_000_000);

    let (file, _, f) =
        run(&src, 2.0, 12.0, FitOptions { max_size: Some(2_000_000), ..FitOptions::default() }, out.path())
            .await;
    assert_eq!(size_of(&file), (1440, 810));
    assert_eq!(f.size, (1440, 810));
    assert!(f.bytes <= 2_000_000, "{f:?}");

    // Never enlarged: a 640x360 source stays 640x360.
    let (file, _, _) = run(&fixture(Fixture::Narrow), 0.0, 3.0, FitOptions::default(), out.path()).await;
    assert_eq!(size_of(&file), (640, 360));

    // A 4K source is fitted to 1920 on the long side.
    let (file, _, _) = run(&fixture(Fixture::FitUhd), 0.0, 2.0, FitOptions::default(), out.path()).await;
    assert_eq!(size_of(&file), (1920, 1080));
    assert!(decode_errors(&file).is_empty());
}

/// T025: HDR, variable and high frame rates, surround audio, and audio-only sources.
#[tokio::test(flavor = "multi_thread")]
async fn awkward_sources_give_a_plain_reddit_mp4_or_are_refused() {
    if !engine_ok() {
        return;
    }
    let out = tempfile::tempdir().unwrap();

    // HDR10 → SDR BT.709, 8-bit.
    let (file, _, _) = run(&fixture(Fixture::HdrTagged), 0.0, 2.0, FitOptions::default(), out.path()).await;
    let v = video(&file);
    assert_eq!(v["pix_fmt"], "yuv420p", "{v}");
    assert_eq!((v["color_transfer"].as_str(), v["color_primaries"].as_str()), (Some("bt709"), Some("bt709")));

    // A variable frame rate source, capped at the target's 24 fps; the length is kept.
    let (file, _, f) = run(&fixture(Fixture::VfrMp4), 1.0, 9.0, target("slow"), out.path()).await;
    let v = video(&file);
    assert_eq!(v["avg_frame_rate"], "24/1", "{v}");
    assert_eq!(f.fps, 24.0);
    assert!((video_secs(&file) - 8.0).abs() <= 1.0 / 24.0 + 1e-3, "{}", video_secs(&file));

    // 120 fps → the built-in target's 60.
    let (file, _, _) = run(&fixture(Fixture::HighFps120), 0.0, 2.0, FitOptions::default(), out.path()).await;
    assert_eq!(video(&file)["avg_frame_rate"], "60/1");

    // 5.1 → stereo, with a notice.
    let src = fixture(Fixture::FitSurround);
    let p = plan_fit(&src, 0.0, 4.0, FitOptions::default(), out.path()).await;
    assert!(
        p.jobs[0].warnings.iter().any(|w| w.contains("mixed down to stereo")),
        "{:?}",
        p.jobs[0].warnings
    );
    let (file, _, _) = run(&src, 0.0, 4.0, FitOptions::default(), out.path()).await;
    let j = probe_json(&file);
    let a = j["streams"].as_array().unwrap().iter().find(|s| s["codec_type"] == "audio").unwrap();
    assert_eq!(a["channels"], 2);

    // Audio only: a video target needs video. The probe refuses it first, as for every clip
    // (`PROBE_FAILED`, "has no video stream").
    let p = plan_fit(&fixture(Fixture::CoverArtM4a), 0.0, 2.0, FitOptions::default(), out.path()).await;
    assert!(error(&p, IssueCode::ProbeFailed).message.contains("no video stream"));
}
