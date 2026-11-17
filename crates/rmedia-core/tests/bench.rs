//! Success-criteria timing checks. Run with `cargo test --release -- --ignored bench`.

mod common;

use std::time::{Duration, Instant};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::request::Mode;
use rmedia_testkit::{Fixture, fixture, require_ffmpeg};

/// SC-001: a fast-mode clip from a 1080p source completes in under 5 s regardless of length.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "benchmark"]
async fn bench_sc001_fast_clip_1080p_under_5s() {
    require_ffmpeg!();
    let src = fixture(Fixture::Hd60Mp4);
    let (tools, caps) = engine();
    for (start, end) in [(1.0, 5.0), (5.0, 55.0)] {
        let out = tempfile::tempdir().unwrap();
        let t0 = Instant::now();
        let req = request(&src, vec![clip(start, EndSpec::At(secs(end)))], opts(out.path(), Mode::Fast));
        let plan = make_plan(&req, &tools, &caps).await;
        let (summary, _) = run_plan(plan, tools.clone(), out.path()).await;
        let elapsed = t0.elapsed();
        assert_eq!(summary.succeeded, 1);
        eprintln!("fast clip {start}-{end}s: {elapsed:?}");
        assert!(elapsed < Duration::from_secs(5), "SC-001 violated: {elapsed:?}");
    }
}

/// SC-003: 10,000 files including 1,000 videos are scanned in under 60 s. On the Windows CI
/// runners the time is reported but not asserted: their file system is slow and uneven for this
/// many small files (64–89 s there, while the same commits passed on the Linux runners).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "benchmark"]
async fn bench_sc003_scan_10k_files_under_60s() {
    require_ffmpeg!();
    let src = fixture(Fixture::NoAudioMp4);
    // Next to the fixture: hard links cannot cross volumes (the Windows runners keep the
    // workspace and the temp folder on different drives).
    let root = tempfile::tempdir_in(src.parent().unwrap()).unwrap();
    for i in 0..10_000 {
        let dir = root.path().join(format!("d{}", i % 50));
        std::fs::create_dir_all(&dir).unwrap();
        if i % 10 == 0 {
            std::fs::hard_link(&src, dir.join(format!("v{i}.mp4"))).unwrap();
        } else {
            std::fs::write(dir.join(format!("f{i}.txt")), b"x").unwrap();
        }
    }
    let (tools, _) = engine();
    let t0 = Instant::now();
    let r = rmedia_core::discover::scan(
        &tools,
        &[root.path().to_path_buf()],
        &Default::default(),
        &|_| {},
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let elapsed = t0.elapsed();
    eprintln!("scanned 10k files / {} videos in {elapsed:?}", r.videos.len());
    assert_eq!(r.videos.len(), 1000);
    if cfg!(windows) && std::env::var_os("GITHUB_ACTIONS").is_some() {
        eprintln!("Windows CI runner: SC-003 not asserted");
        return;
    }
    assert!(elapsed < Duration::from_secs(60), "SC-003 violated: {elapsed:?}");
}

/// 002 SC-003: the CPU tier turns a 60 s 480p30 portrait source into the default preset in at
/// most twice its duration (centre crop, so no smart-reframe analysis is timed). The bar is
/// defined for the reference machine (12 threads); on smaller machines, such as the 4-vCPU CI
/// runners, the time is reported but not asserted.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "benchmark"]
async fn bench_upscale_sc003_cpu_tier_within_2x_duration() {
    rmedia_testkit::require_engine!("libx265", "zscale", "cas");
    let src = fixture(Fixture::Portrait480Long);
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let mut o = opts(out.path(), Mode::Accurate);
    o.upscale = Some(rmedia_core::upscale::UpscaleOptions {
        reframe: rmedia_core::upscale::Reframe::Center,
        ..Default::default()
    });
    let plan = make_plan(&request(&src, vec![clip(0.0, EndSpec::EndOfMedia)], o), &tools, &caps).await;
    let t0 = Instant::now();
    let (summary, _) = run_plan(plan, tools, out.path()).await;
    let elapsed = t0.elapsed();
    assert_eq!(summary.succeeded, 1, "{summary:?}");
    eprintln!("CPU tier, 60 s 480p → 1080x2340: {elapsed:?}");
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    if threads < 12 {
        eprintln!("{threads} threads: below the reference machine, SC-003 not asserted");
        return;
    }
    assert!(elapsed < Duration::from_secs(120), "SC-003 violated: {elapsed:?}");
}

/// 002 SC-005: the smart-reframe analysis of a 60 s 1080p source takes at most half the time
/// of its centre-crop render. Needs the YuNet model.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "benchmark"]
async fn bench_upscale_sc005_smart_reframe_overhead() {
    rmedia_testkit::require_engine!("libx265", "zscale", "cas", "sendcmd", "scdet");
    let src = fixture(Fixture::Hd60Mp4);
    let (tools, mut caps) = engine();
    caps.face = Some(yunet().expect("run `cargo xtask fetch-models` first"));
    let out = tempfile::tempdir().unwrap();
    caps.reframe_cache = Some(out.path().join("cache"));
    let plan_with = |reframe| {
        let mut o = opts(out.path(), Mode::Accurate);
        o.upscale = Some(rmedia_core::upscale::UpscaleOptions { reframe, ..Default::default() });
        o.on_exists = rmedia_core::request::OnExists::Rename;
        request(&src, vec![clip(0.0, EndSpec::EndOfMedia)], o)
    };
    let centre = make_plan(&plan_with(rmedia_core::upscale::Reframe::Center), &tools, &caps).await;
    let t0 = Instant::now();
    let (summary, _) = run_plan(centre, tools.clone(), out.path()).await;
    let render = t0.elapsed();
    assert_eq!(summary.succeeded, 1, "{summary:?}");
    let mut smart = make_plan(&plan_with(rmedia_core::upscale::Reframe::Smart), &tools, &caps).await;
    let t0 = Instant::now();
    rmedia_core::upscale::prepare_plan(&mut smart, &tools, &tokio_util::sync::CancellationToken::new())
        .await
        .unwrap();
    let analysis = t0.elapsed();
    eprintln!("60 s 1080p: centre render {render:?}, smart analysis {analysis:?}");
    assert!(analysis.as_secs_f64() <= render.as_secs_f64() * 0.5, "SC-005 violated");
}

/// 003 SC-003: `Busy10` fits the default Reddit-post budget in at most 3× the clip's duration
/// (search included), asserted on the reference class of machine (≥ 12 threads).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "benchmark"]
async fn bench_gif_sc003_busy10_to_the_post_budget() {
    rmedia_testkit::require_engine!("palettegen", "paletteuse");
    let src = fixture(Fixture::Busy10);
    let (tools, caps) = engine();
    let out = tempfile::tempdir().unwrap();
    let o = rmedia_core::request::Options {
        gif: Some(rmedia_core::gif::GifOptions::default()),
        output_dir: Some(out.path().to_path_buf()),
        ..Default::default()
    };
    let plan = make_plan(&request(&src, vec![clip(0.0, EndSpec::EndOfMedia)], o), &tools, &caps).await;
    assert!(!plan.has_errors(), "{:?}", plan.issues);
    let t0 = Instant::now();
    let (summary, _) = run_plan(plan, tools, out.path()).await;
    let elapsed = t0.elapsed();
    assert_eq!(summary.succeeded, 1, "{:?}", summary.jobs[0].outcome);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    eprintln!("GIF SC-003 Busy10 → reddit-post: {elapsed:?} on {threads} threads (bar 30 s)");
    if threads >= 12 {
        assert!(elapsed <= Duration::from_secs(30), "SC-003 violated: {elapsed:?}");
    }
}

/// Feature 004, T023 (SC-005): a smart cut of a 60 s range of a 1080p H.264 source takes at most
/// one third of accurate mode's time (planning and verification included).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "benchmark"]
async fn bench_sc005_smart_vs_accurate() {
    rmedia_testkit::require_engine!("libx264", "aac");
    let src = fixture(Fixture::SmartHd60);
    let (tools, caps) = engine();
    let mut times = Vec::new();
    for mode in [Mode::Accurate, Mode::Smart] {
        let out = tempfile::tempdir().unwrap();
        let t0 = Instant::now();
        let req = request(&src, vec![clip(5.3, EndSpec::At(secs(65.7)))], opts(out.path(), mode));
        let plan = make_plan(&req, &tools, &caps).await;
        let (summary, _) = run_plan(plan, tools.clone(), out.path()).await;
        assert_eq!(summary.succeeded, 1, "{summary:?}");
        if mode == Mode::Smart {
            let s = match &summary.jobs[0].outcome {
                rmedia_core::event::JobOutcome::Succeeded {
                    measure: rmedia_core::event::JobMeasure::Clip { smart: Some(s), .. },
                    ..
                } => s.clone(),
                other => panic!("{other:?}"),
            };
            assert_eq!(s.mode_used, Mode::Smart, "{s:?}");
        }
        times.push(t0.elapsed());
    }
    let (accurate, smart) = (times[0], times[1]);
    eprintln!(
        "60 s of 1080p: accurate {accurate:?}, smart {smart:?} ({:.0}%)",
        100.0 * smart.as_secs_f64() / accurate.as_secs_f64()
    );
    assert!(smart.as_secs_f64() <= accurate.as_secs_f64() / 3.0, "SC-005 violated");
}

/// Feature 004, SC-007's marker half: the whole-file keyframe index of a 10-minute H.264 source
/// (15,000 frames, a keyframe every 2 s) is ready in at most 2 s. The index reads packets, never
/// pixels, so a small frame size times it as well as a large one. On the Windows CI runners the
/// time is reported but not asserted: process start-up there is slow and uneven (0.5–2.1 s
/// across runs, while the Linux runners stay near 0.4 s).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "benchmark"]
async fn bench_sc007_keyframes_of_a_10_minute_source() {
    rmedia_testkit::require_engine!("libx264");
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("ten.mp4");
    let st = std::process::Command::new(rmedia_testkit::ffmpeg())
        .args(["-v", "error", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=25:duration=600"])
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-x264-params",
            "keyint=50:min-keyint=50:scenecut=0",
        ])
        .args(["-pix_fmt", "yuv420p"])
        .arg(&src)
        .status()
        .unwrap();
    assert!(st.success());
    let (tools, _) = engine();
    let cancel = tokio_util::sync::CancellationToken::new();
    let t0 = Instant::now();
    let media = rmedia_core::probe::probe(&tools, &src, &cancel).await.unwrap();
    let index = rmedia_core::smart::keyframes(&tools, &media, None, &cancel).await.unwrap();
    let elapsed = t0.elapsed();
    eprintln!("10 min source: {} keyframes in {elapsed:?}", index.keyframes.len());
    assert_eq!((index.keyframes.len(), index.truncated), (300, false));
    if cfg!(windows) && std::env::var_os("GITHUB_ACTIONS").is_some() {
        eprintln!("Windows CI runner: SC-007 not asserted");
        return;
    }
    assert!(elapsed < Duration::from_secs(2), "SC-007 violated: {elapsed:?}");
}
