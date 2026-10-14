//! US1 acceptance scenarios against real, generated media (Constitution III).

mod common;

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::{JobMeasure, JobOutcome};
use rmedia_core::plan::Disposition;
use rmedia_core::request::{AudioSel, Mode, OnExists, Options};
use rmedia_testkit::{Fixture, duration, first_frame_psnr, fixture, require_ffmpeg, sha256, stream_types};

fn succeeded(o: &JobOutcome) -> (&std::path::Path, f64, f64, bool) {
    match o {
        JobOutcome::Succeeded { output, measure: JobMeasure::Clip { actual, adjusted } } => {
            (output, actual.start.as_secs_f64(), actual.end.as_secs_f64(), *adjusted)
        }
        other => panic!("job did not succeed: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn accurate_clip_is_frame_exact_and_source_untouched() {
    require_ffmpeg!();
    let src = fixture(Fixture::Gop50Mp4);
    let before = sha256(&src);
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let req = request(&src, vec![clip(10.5, EndSpec::At(secs(20.0)))], opts(out.path(), Mode::Accurate));
    let plan = make_plan(&req, &tools, &caps).await;
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    let (summary, _) = run_plan(plan, tools, out.path()).await;
    assert_eq!(summary.succeeded, 1, "{summary:?}");
    let (output, start, end, adjusted) = succeeded(&summary.jobs[0].outcome);

    assert!((duration(output) - 9.5).abs() <= 0.05, "duration {}", duration(output));
    assert!((start - 10.5).abs() < 1e-6 && (end - 20.0).abs() <= 0.05 && !adjusted);
    // The first frame matches source frame 10.5 better than its neighbours (±1 frame @ 25fps).
    let exact = first_frame_psnr(output, &src, 10.5);
    assert!(exact > first_frame_psnr(output, &src, 10.46), "not frame exact");
    assert!(exact > first_frame_psnr(output, &src, 10.54), "not frame exact");
    assert_eq!(stream_types(output), ["video", "audio"]);
    assert_eq!(sha256(&src), before, "source must never change");
    assert!(partials(out.path()).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn fast_clip_snaps_to_keyframe_and_reports_it() {
    require_ffmpeg!();
    let src = fixture(Fixture::Gop50Mp4);
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let req = request(&src, vec![clip(10.5, EndSpec::At(secs(20.0)))], opts(out.path(), Mode::Fast));
    let plan = make_plan(&req, &tools, &caps).await;
    let predicted = plan.jobs[0].predicted().expect("fast mode predicts the snap");
    assert_eq!(predicted.start, secs(10.0));
    let (summary, events) = run_plan(plan, tools, out.path()).await;
    let (output, start, end, adjusted) = succeeded(&summary.jobs[0].outcome);
    assert_eq!(start, 10.0);
    assert!(adjusted, "a 0.5 s snap must be reported");
    assert!((end - 20.0).abs() <= 0.25, "end {end}");
    // Stream copy: the first frame is the source keyframe at 10.0, bit-identical.
    assert!(first_frame_psnr(output, &src, 10.0).is_infinite());
    assert!(events.iter().any(|e| matches!(e, rmedia_core::event::Event::RunFinished { .. })));
}

#[tokio::test(flavor = "multi_thread")]
async fn duration_form_and_end_clamping() {
    require_ffmpeg!();
    let src = fixture(Fixture::Gop50Mp4);
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let mut late = clip(50.0, EndSpec::At(secs(70.0)));
    late.name = Some("late".into());
    let req = request(
        &src,
        vec![clip(30.0, EndSpec::Duration(secs(15.0))), late],
        opts(out.path(), Mode::Accurate),
    );
    let plan = make_plan(&req, &tools, &caps).await;
    assert!(plan.jobs[1].warnings.iter().any(|w| w.contains("past the end")), "{:?}", plan.jobs[1].warnings);
    let (summary, _) = run_plan(plan, tools, out.path()).await;
    assert_eq!(summary.succeeded, 2, "{summary:?}");
    let (a, ..) = succeeded(&summary.jobs[0].outcome);
    let (b, ..) = succeeded(&summary.jobs[1].outcome);
    assert!((duration(a) - 15.0).abs() <= 0.05);
    assert!((duration(b) - 10.0).abs() <= 0.1);
    assert!(b.file_name().unwrap().to_string_lossy().ends_with("_002_late.mp4"));
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_requests_are_rejected_before_processing() {
    require_ffmpeg!();
    let src = fixture(Fixture::Gop50Mp4);
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let req = request(
        &src,
        vec![
            clip(20.0, EndSpec::At(secs(10.0))),
            clip(61.0, EndSpec::At(secs(62.0))),
            clip(5.0, EndSpec::Duration(secs(0.0))),
        ],
        opts(out.path(), Mode::Fast),
    );
    let plan = make_plan(&req, &tools, &caps).await;
    assert_eq!(plan.issues.len(), 3, "all problems reported at once: {:?}", plan.issues);
    assert!(plan.issues[0].message.contains("not after start"));
    assert!(plan.issues[1].message.contains("beyond the end"));
    assert!(plan.jobs.is_empty());
    assert!(std::fs::read_dir(out.path()).unwrap().next().is_none(), "planning writes nothing");

    let missing = request(
        &out.path().join("nope.mp4"),
        vec![clip(0.0, EndSpec::EndOfMedia)],
        opts(out.path(), Mode::Fast),
    );
    let plan = make_plan(&missing, &tools, &caps).await;
    assert_eq!(plan.issues[0].code, "SOURCE_MISSING");
}

#[tokio::test(flavor = "multi_thread")]
async fn collision_policies() {
    require_ffmpeg!();
    let src = fixture(Fixture::Gop50Mp4);
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let mk = |on_exists| {
        let mut o = opts(out.path(), Mode::Fast);
        o.on_exists = on_exists;
        request(&src, vec![clip(2.0, EndSpec::At(secs(4.0)))], o)
    };
    let (first, _) =
        run_plan(make_plan(&mk(OnExists::Skip), &tools, &caps).await, tools.clone(), out.path()).await;
    let (path, ..) = succeeded(&first.jobs[0].outcome);
    let original = sha256(path);

    let plan = make_plan(&mk(OnExists::Skip), &tools, &caps).await;
    assert!(matches!(plan.jobs[0].disposition, Disposition::Skip { .. }));
    let (s, _) = run_plan(plan, tools.clone(), out.path()).await;
    assert_eq!((s.skipped, s.exit_code), (1, 0));
    assert_eq!(sha256(path), original);

    let (s, _) =
        run_plan(make_plan(&mk(OnExists::Rename), &tools, &caps).await, tools.clone(), out.path()).await;
    let (renamed, ..) = succeeded(&s.jobs[0].outcome);
    assert!(renamed.to_string_lossy().ends_with("_1.mp4"), "{}", renamed.display());

    let (s, _) = run_plan(make_plan(&mk(OnExists::Overwrite), &tools, &caps).await, tools, out.path()).await;
    assert_eq!(s.succeeded, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn timestamps_offset_and_vfr_sources() {
    require_ffmpeg!();
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let ts = fixture(Fixture::OffsetTs);
    let vfr = fixture(Fixture::VfrMp4);
    let mut req = request(&ts, vec![clip(5.0, EndSpec::At(secs(8.0)))], opts(out.path(), Mode::Accurate));
    req.sources.extend(request(&vfr, vec![clip(6.0, EndSpec::At(secs(12.0)))], Options::default()).sources);
    let plan = make_plan(&req, &tools, &caps).await;
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    let (summary, _) = run_plan(plan, tools, out.path()).await;
    let (a, ..) = succeeded(&summary.jobs[0].outcome);
    let (b, ..) = succeeded(&summary.jobs[1].outcome);
    assert!((duration(a) - 3.0).abs() <= 0.1, "ts {}", duration(a));
    // MPEG-TS seeks approximately; the two-stage seek must still be frame exact.
    // User time 0 is the first video frame, so frames sit at n*0.04.
    let exact = first_frame_psnr(a, &ts, 4.999);
    assert!(exact > 40.0, "ts accurate first frame psnr {exact}");
    assert!(exact > first_frame_psnr(a, &ts, 5.039));
    assert!(exact > first_frame_psnr(a, &ts, 4.959));
    assert!((duration(b) - 6.0).abs() <= 0.15, "vfr {}", duration(b));
}

#[tokio::test(flavor = "multi_thread")]
async fn fast_mode_on_mpeg_ts_starts_exactly_on_the_keyframe() {
    require_ffmpeg!();
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let ts = fixture(Fixture::OffsetTs);
    let req = request(&ts, vec![clip(5.0, EndSpec::At(secs(8.0)))], opts(out.path(), Mode::Fast));
    let plan = make_plan(&req, &tools, &caps).await;
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    // Keyframes every 2 s from user time 0 → the one before 5.0 is 4.0.
    let k = plan.jobs[0].predicted().expect("keyframe found").start.as_secs_f64();
    assert!((k - 4.0).abs() < 1e-6, "keyframe {k}");
    let (summary, _) = run_plan(plan, tools, out.path()).await;
    let (o, start, _, adjusted) = succeeded(&summary.jobs[0].outcome);
    assert_eq!(start, k);
    assert!(adjusted);
    assert!(first_frame_psnr(o, &ts, k - 0.001).is_infinite(), "copy must start on the keyframe");
}

#[tokio::test(flavor = "multi_thread")]
async fn streams_and_containers() {
    require_ffmpeg!();
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let mkv = fixture(Fixture::MultiMkv);

    // mkv fast: everything copied.
    let req = request(&mkv, vec![clip(2.0, EndSpec::At(secs(12.0)))], opts(out.path(), Mode::Fast));
    let (s, _) = run_plan(make_plan(&req, &tools, &caps).await, tools.clone(), out.path()).await;
    let (o, ..) = succeeded(&s.jobs[0].outcome);
    assert_eq!(stream_types(o), ["video", "audio", "audio", "subtitle"]);

    // mkv → mp4 accurate, second audio track only: subtitle converted to mov_text.
    let mut o2 = opts(&out.path().join("mp4"), Mode::Accurate);
    o2.container = Some("mp4".into());
    o2.streams.audio = AudioSel::Tracks(vec![2]);
    let req = request(&mkv, vec![clip(2.0, EndSpec::At(secs(12.0)))], o2);
    let (s, _) = run_plan(make_plan(&req, &tools, &caps).await, tools.clone(), out.path()).await;
    let (o, ..) = succeeded(&s.jobs[0].outcome);
    assert_eq!(stream_types(o), ["video", "audio", "subtitle"]);
    let probe = rmedia_testkit::probe_json(o);
    assert_eq!(probe["streams"][2]["codec_name"], "mov_text");

    // A non-existent audio track is a validation error.
    let mut o3 = opts(out.path(), Mode::Fast);
    o3.streams.audio = AudioSel::Tracks(vec![3]);
    let plan = make_plan(&request(&mkv, vec![clip(0.0, EndSpec::EndOfMedia)], o3), &tools, &caps).await;
    assert!(plan.issues[0].message.contains("audio track 3 does not exist"));

    // webm accurate (VP9/Opus), video-only mp4, and accurate fallback from .avi to .mp4.
    let webm = fixture(Fixture::SmallWebm);
    let noaudio = fixture(Fixture::NoAudioMp4);
    let gop = fixture(Fixture::Gop50Mp4);
    let mut avi = opts(&out.path().join("avi"), Mode::Accurate);
    avi.container = Some("avi".into());
    let mut req =
        request(&webm, vec![clip(1.0, EndSpec::At(secs(4.0)))], opts(&out.path().join("w"), Mode::Accurate));
    req.sources
        .extend(request(&noaudio, vec![clip(1.0, EndSpec::At(secs(4.0)))], Options::default()).sources);
    req.sources.extend(request(&gop, vec![clip(1.0, EndSpec::At(secs(3.0)))], Options::default()).sources);
    req.sources[2].overrides.container = Some("avi".into());
    let plan = make_plan(&req, &tools, &caps).await;
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    assert!(plan.jobs[2].output.extension().unwrap() == "mp4");
    assert!(plan.jobs[2].warnings.iter().any(|w| w.contains("cannot write .avi")));
    let (s, _) = run_plan(plan, tools, out.path()).await;
    assert_eq!(s.succeeded, 3, "{s:?}");
    let (w, ..) = succeeded(&s.jobs[0].outcome);
    assert_eq!(rmedia_testkit::probe_json(w)["streams"][0]["codec_name"], "vp9");
    let (n, ..) = succeeded(&s.jobs[1].outcome);
    assert_eq!(stream_types(n), ["video"]);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn unwritable_output_fails_cleanly() {
    use std::os::unix::fs::PermissionsExt;
    require_ffmpeg!();
    if unsafe_is_root() {
        return; // root ignores permissions
    }
    let src = fixture(Fixture::Gop50Mp4);
    let (tools, caps) = engine();
    let base = tempfile::tempdir().unwrap();
    let ro = base.path().join("ro");
    std::fs::create_dir(&ro).unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    let req = request(&src, vec![clip(1.0, EndSpec::At(secs(3.0)))], opts(&ro, Mode::Fast));
    let (s, _) = run_plan(make_plan(&req, &tools, &caps).await, tools, base.path()).await;
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    match &s.jobs[0].outcome {
        JobOutcome::Failed { error } => assert_eq!(error.code, "OUTPUT_UNWRITABLE", "{error:?}"),
        other => panic!("expected failure, got {other:?}"),
    }
    assert_eq!(s.exit_code, 1);
    assert!(partials(&ro).is_empty());
}

#[cfg(unix)]
fn unsafe_is_root() -> bool {
    std::env::var("USER").is_ok_and(|u| u == "root")
}

/// Review H1: seek strategy must follow the detected container, not the extension.
#[tokio::test(flavor = "multi_thread")]
async fn mislabelled_ts_named_mp4_is_still_frame_exact() {
    require_ffmpeg!();
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::MislabelledTs);
    let req = request(&src, vec![clip(12.5, EndSpec::At(secs(15.0)))], opts(out.path(), Mode::Accurate));
    let plan = make_plan(&req, &tools, &caps).await;
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    let (summary, _) = run_plan(plan, tools, out.path()).await;
    let (o, ..) = succeeded(&summary.jobs[0].outcome);
    let video = rmedia_testkit::probe_json(o)["streams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["codec_type"] == "video")
        .and_then(|s| s["duration"].as_str().and_then(|d| d.parse::<f64>().ok()))
        .unwrap();
    assert!((video - 2.5).abs() < 0.1, "video lasts {video}");
    let exact = first_frame_psnr(o, &src, 12.499);
    assert!(exact > 40.0, "first frame psnr {exact}");
}

/// Review H3: odd dimensions are cropped to even ones instead of failing.
#[tokio::test(flavor = "multi_thread")]
async fn odd_dimensions_encode_in_accurate_mode() {
    require_ffmpeg!();
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::OddDimsMp4);
    let req = request(&src, vec![clip(1.0, EndSpec::At(secs(3.0)))], opts(out.path(), Mode::Accurate));
    let (summary, _) = run_plan(make_plan(&req, &tools, &caps).await, tools, out.path()).await;
    let (o, ..) = succeeded(&summary.jobs[0].outcome);
    let v = &rmedia_testkit::probe_json(o)["streams"][0];
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(320), Some(240)));
}

/// Fast mode on an approximately-seeking source far from the start (no coarse demuxer seek).
#[tokio::test(flavor = "multi_thread")]
async fn fast_mode_on_mislabelled_ts_late_start() {
    require_ffmpeg!();
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::MislabelledTs);
    let req = request(&src, vec![clip(21.3, EndSpec::At(secs(25.0)))], opts(out.path(), Mode::Fast));
    let plan = make_plan(&req, &tools, &caps).await;
    let k = plan.jobs[0].predicted().expect("keyframe").start.as_secs_f64();
    assert!((k - 20.0).abs() < 1e-6, "keyframe {k}");
    let (summary, _) = run_plan(plan, tools, out.path()).await;
    let (o, start, ..) = succeeded(&summary.jobs[0].outcome);
    assert_eq!(start, k);
    assert!(first_frame_psnr(o, &src, k - 0.001).is_infinite(), "copy must start on the keyframe");
}
