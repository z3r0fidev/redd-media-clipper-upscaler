//! Feature 004, US1: a frame-exact clip that keeps the original quality (tasks T015–T020).

mod common;

use std::path::{Path, PathBuf};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::{JobMeasure, JobOutcome, SmartOutcome, Span};
use rmedia_core::plan::{JobSpec, Plan};
use rmedia_core::request::Mode;
use rmedia_core::smart::SmartPlan;
use rmedia_testkit::{Fixture, decode_errors, fixture, frame_count, framemd5, stream_starts, video_packets};

fn engine_ok() -> bool {
    rmedia_testkit::require_engine(&["libx264", "aac"])
}

fn smart_plan(plan: &Plan) -> &SmartPlan {
    match &plan.jobs[0].spec {
        JobSpec::Clip(c) => {
            c.smart.as_ref().unwrap_or_else(|| panic!("not planned as smart: {:?}", plan.jobs[0].warnings))
        }
        _ => panic!("not a clip job"),
    }
}

async fn plan_smart(src: &Path, start: f64, end: EndSpec, out: &Path) -> Plan {
    let (tools, caps) = engine();
    let plan = make_plan(&request(src, vec![clip(start, end)], opts(out, Mode::Smart)), &tools, &caps).await;
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    plan
}

/// Plans and runs one smart clip; returns the output and the outcome's smart block.
async fn cut(src: &Path, start: f64, end: EndSpec, out: &Path) -> (PathBuf, Span, SmartOutcome) {
    let (tools, _) = engine();
    let plan = plan_smart(src, start, end, out).await;
    let (summary, _) = run_plan(plan, tools, &out.join("state")).await;
    match &summary.jobs[0].outcome {
        JobOutcome::Succeeded { output, measure: JobMeasure::Clip { actual, smart: Some(s), .. } } => {
            (output.clone(), *actual, s.clone())
        }
        other => panic!("{other:?}"),
    }
}

/// T015 (SC-001): mid-GOP ends in every eligible container, with and without B-frames and
/// audio: exactly the requested frames, starting on the requested frame.
#[tokio::test(flavor = "multi_thread")]
async fn frame_exact_ends() {
    if !engine_ok() {
        return;
    }
    for (kind, end, frames) in [
        (Fixture::SmartMp4, 20.72, 435),
        (Fixture::SmartMov, 20.72, 435),
        (Fixture::SmartMkv, 20.72, 435),
        (Fixture::SmartTs, 20.72, 435),
        (Fixture::SmartBaseline, 16.72, 335),
    ] {
        let src = fixture(kind);
        let out = tempfile::tempdir().unwrap();
        let (output, actual, smart) = cut(&src, 3.32, EndSpec::At(secs(end)), out.path()).await;
        assert_eq!(smart.mode_used, Mode::Smart, "{kind:?}: {smart:?}");
        assert_eq!(frame_count(&output), frames, "{kind:?}");
        assert_eq!((smart.copied_frames + smart.reencoded_frames) as u64, frames, "{kind:?}");
        assert!((actual.start.as_secs_f64() - 3.32).abs() < 0.001, "{kind:?}: {actual:?}");
        // The frame count above is the exact check; the reported end is the container's duration,
        // which the audio's last AAC frame can extend (Matroska with FFmpeg 6.1: +61 ms).
        assert!((actual.end.as_secs_f64() - end).abs() < 0.1, "{kind:?}: {actual:?}");
        let on = rmedia_testkit::first_frame_psnr(&output, &src, 3.32);
        let before = rmedia_testkit::first_frame_psnr(&output, &src, 3.28);
        let after = rmedia_testkit::first_frame_psnr(&output, &src, 3.36);
        assert!(on > 35.0 && on > before + 5.0 && on > after + 5.0, "{kind:?}: {before} {on} {after}");
    }
}

/// T059 review: a start or end a fraction of a millisecond after a frame time picks the same
/// frames as accurate mode (frames at or after the start, before the end), not the frame just
/// before it.
#[tokio::test(flavor = "multi_thread")]
async fn ends_just_after_a_frame_pick_the_same_frames_as_accurate() {
    if !engine_ok() {
        return;
    }
    let src = fixture(Fixture::SmartMp4);
    let out = tempfile::tempdir().unwrap();
    // (start, end) → (head frames, tail frames); 25 fps, keyframes every 2 s.
    for (start, end, head, tail) in [
        (3.32, 20.72, 17, 18),     // on frames: 3.32–3.96 and 20.00–20.68
        (3.3196, 20.7196, 17, 18), // just before: the same frames
        (3.3204, 20.7204, 16, 19), // just after: 3.36–3.96 and 20.00–20.72
    ] {
        let plan = plan_smart(&src, start, EndSpec::At(secs(end)), out.path()).await;
        let p = smart_plan(&plan);
        assert_eq!(
            (p.head.map(|h| h.frames), p.tail.map(|t| t.frames), p.copy.frames),
            (Some(head), Some(tail), 400),
            "{start}–{end}"
        );
    }
    let (output, _, smart) = cut(&src, 3.3204, EndSpec::At(secs(20.7204)), out.path()).await;
    assert_eq!(smart.mode_used, Mode::Smart);
    assert_eq!(frame_count(&output), 435);
    let on = rmedia_testkit::first_frame_psnr(&output, &src, 3.36);
    let before = rmedia_testkit::first_frame_psnr(&output, &src, 3.32);
    assert!(on > 35.0 && on > before + 5.0, "starts on 3.36, not 3.32: {before} {on}");
}

/// T016 (US1 scenario 2, edge cases): ends on keyframes skip the head or tail; the end of the
/// video needs no tail; a range ending after the last keyframe has a tail from it.
#[tokio::test(flavor = "multi_thread")]
async fn keyframe_aligned_ends_skip_parts() {
    if !engine_ok() {
        return;
    }
    let src = fixture(Fixture::SmartMp4);
    let out = tempfile::tempdir().unwrap();
    let parts = |p: &SmartPlan| (p.head.map(|h| h.frames), p.copy.frames, p.tail.map(|t| t.frames));
    for (start, end, want) in [
        (4.0, EndSpec::At(secs(20.0)), (None, 400, None)),
        (3.32, EndSpec::At(secs(20.0)), (Some(17), 400, None)),
        (4.0, EndSpec::At(secs(20.72)), (None, 400, Some(18))),
        (3.32, EndSpec::EndOfMedia, (Some(17), 650, None)),
        (3.32, EndSpec::At(secs(29.5)), (Some(17), 600, Some(38))),
    ] {
        let plan = plan_smart(&src, start, end, out.path()).await;
        assert_eq!(parts(smart_plan(&plan)), want, "{start} {end:?}");
    }
    // A pure copy is still frame-exact: every frame identical to the source.
    let (output, _, smart) = cut(&src, 4.0, EndSpec::At(secs(20.0)), out.path()).await;
    assert_eq!((smart.copied_frames, smart.reencoded_frames), (400, 0));
    assert_eq!(framemd5(&output, 0.0, 400), framemd5(&src, 4.0, 400));
}

/// T017 (SC-002, SC-006): long clips copy at least 80% of their frames, and every copied frame
/// is identical to the source.
#[tokio::test(flavor = "multi_thread")]
async fn long_clips_copy_most_frames_unchanged() {
    if !engine_ok() {
        return;
    }
    let src = fixture(Fixture::SmartMp4);
    let out = tempfile::tempdir().unwrap();
    let (output, _, smart) = cut(&src, 3.32, EndSpec::At(secs(26.72)), out.path()).await;
    assert_eq!((smart.copied_frames, smart.reencoded_frames), (550, 35));
    assert!(smart.copied_share >= 0.8, "{smart:?}");
    assert_eq!(framemd5(&output, 17.0 * 0.04, 550), framemd5(&src, 4.0, 550));
}

/// T018 (SC-003, scenario 3): no decode errors, an even frame step across the seams, and audio
/// that starts with the video and lasts as long.
#[tokio::test(flavor = "multi_thread")]
async fn seams_decode_cleanly() {
    if !engine_ok() {
        return;
    }
    for (kind, end) in [
        (Fixture::SmartMp4, 20.72),
        (Fixture::SmartMkv, 20.72),
        (Fixture::SmartTs, 20.72),
        (Fixture::OtherEncoder, 14.72),
    ] {
        let src = fixture(kind);
        let out = tempfile::tempdir().unwrap();
        let (output, _, smart) = cut(&src, 3.32, EndSpec::At(secs(end)), out.path()).await;
        assert_eq!(smart.mode_used, Mode::Smart, "{kind:?}: {smart:?}");
        assert_eq!(decode_errors(&output), Vec::<String>::new(), "{kind:?}");
        let mut pts: Vec<f64> = video_packets(&output).iter().filter_map(|p| p.pts).collect();
        pts.sort_by(f64::total_cmp);
        let steps: Vec<f64> = pts.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(steps.iter().all(|s| (s - 0.04).abs() < 0.002), "{kind:?}: uneven frame steps");
        let (v, a) = stream_starts(&output);
        assert!((v - a).abs() < 0.02, "{kind:?}: audio starts at {a}, video at {v}");
        let audio = rmedia_testkit::probe_json(&output)["streams"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["codec_type"] == "audio")
            .and_then(|s| s["duration"].as_str()?.parse::<f64>().ok());
        if let Some(d) = audio {
            assert!((d - (end - 3.32)).abs() < 0.05, "{kind:?}: audio lasts {d}");
        }
    }
}

/// T019 (SC-006): the re-encoded head scores at least as well as accurate mode's output for the
/// same frames (within 0.5 VMAF: both are the same encoder at the same settings).
#[tokio::test(flavor = "multi_thread")]
async fn reencoded_parts_match_accurate_quality() {
    if !rmedia_testkit::require_engine(&["libx264", "aac", "libvmaf"]) {
        return;
    }
    let src = fixture(Fixture::OtherEncoder);
    let out = tempfile::tempdir().unwrap();
    let (smart, _, _) = cut(&src, 3.32, EndSpec::At(secs(14.72)), out.path()).await;
    let (tools, caps) = engine();
    let acc_dir = out.path().join("accurate");
    let plan = make_plan(
        &request(&src, vec![clip(3.32, EndSpec::At(secs(4.0)))], opts(&acc_dir, Mode::Accurate)),
        &tools,
        &caps,
    )
    .await;
    let (summary, _) = run_plan(plan, tools, &out.path().join("state2")).await;
    let JobOutcome::Succeeded { output: accurate, .. } = &summary.jobs[0].outcome else {
        panic!("{summary:?}")
    };
    let head = |file: &Path, name: &str| -> PathBuf {
        let p = out.path().join(name);
        let st = std::process::Command::new(rmedia_testkit::ffmpeg())
            .args(["-v", "error", "-y", "-i"])
            .arg(file)
            .args(["-map", "0:v:0", "-frames:v", "17", "-c:v", "ffv1"])
            .arg(&p)
            .status()
            .unwrap();
        assert!(st.success());
        p
    };
    let reference = out.path().join("ref.mkv");
    let st = std::process::Command::new(rmedia_testkit::ffmpeg())
        .args(["-v", "error", "-y", "-ss", "3.32", "-i"])
        .arg(&src)
        .args(["-map", "0:v:0", "-frames:v", "17", "-c:v", "ffv1"])
        .arg(&reference)
        .status()
        .unwrap();
    assert!(st.success());
    let smart_score = rmedia_testkit::vmaf(&head(&smart, "smart-head.mkv"), &reference);
    let accurate_score = rmedia_testkit::vmaf(&head(accurate, "accurate-head.mkv"), &reference);
    assert!(smart_score >= accurate_score - 0.5, "smart {smart_score} vs accurate {accurate_score}");
}

/// T020 (FR-012, R10): the outcome reports the shares, and the same cut twice gives the same
/// bytes on one machine.
#[tokio::test(flavor = "multi_thread")]
async fn outcome_reports_shares_and_output_is_deterministic() {
    if !engine_ok() {
        return;
    }
    let src = fixture(Fixture::SmartMp4);
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (first, _, smart) = cut(&src, 3.32, EndSpec::At(secs(20.72)), a.path()).await;
    assert_eq!((smart.copied_frames, smart.reencoded_frames, smart.fallback), (400, 35, None));
    assert_eq!(smart.copied_share, 0.9195);
    let (second, _, _) = cut(&src, 3.32, EndSpec::At(secs(20.72)), b.path()).await;
    assert_eq!(rmedia_testkit::sha256(&first), rmedia_testkit::sha256(&second));
}
