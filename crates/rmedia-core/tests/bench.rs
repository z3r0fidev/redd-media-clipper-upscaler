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

/// SC-003: 10,000 files including 1,000 videos are scanned in under 60 s.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "benchmark"]
async fn bench_sc003_scan_10k_files_under_60s() {
    require_ffmpeg!();
    let src = fixture(Fixture::NoAudioMp4);
    let root = tempfile::tempdir().unwrap();
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
    assert!(elapsed < Duration::from_secs(60), "SC-003 violated: {elapsed:?}");
}

/// 002 SC-003: the CPU tier turns a 60 s 480p30 portrait source into the default preset in at
/// most twice its duration (centre crop, so no smart-reframe analysis is timed).
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
