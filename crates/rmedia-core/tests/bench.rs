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
