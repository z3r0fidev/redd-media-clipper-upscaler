//! T022 (003): probe fields GIF export needs — the stream's frame rates, frame count, time base
//! and alpha plane — read from real files.

mod common;

use common::*;
use rmedia_core::probe::{self, MediaInfo};
use rmedia_testkit::{Fixture, fixture, require_engine};
use tokio_util::sync::CancellationToken;

async fn probe_of(path: &std::path::Path) -> MediaInfo {
    let (tools, _) = engine();
    probe::probe(&tools, path, &CancellationToken::new()).await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn rates_frame_count_and_time_base_are_read() {
    require_engine!("libx264");
    let m = probe_of(&fixture(Fixture::Land6)).await;
    let v = m.first_video().unwrap();
    assert_eq!(v.r_frame_rate, Some((30, 1)));
    assert_eq!(v.frame_rate, Some((30, 1)));
    assert_eq!(v.nb_frames, Some(180));
    assert!(v.time_base.is_some_and(|(n, d)| n == 1 && d > 0), "{:?}", v.time_base);
    assert!(!v.has_alpha);
    assert!(!v.is_vfr());
}

#[tokio::test(flavor = "multi_thread")]
async fn alpha_planes_are_detected() {
    require_engine!("ffv1");
    let m = probe_of(&fixture(Fixture::Alpha)).await;
    let v = m.first_video().unwrap();
    assert_eq!(v.pix_fmt.as_deref(), Some("yuva420p"));
    assert!(v.has_alpha);
}

#[tokio::test(flavor = "multi_thread")]
async fn variable_frame_rate_sources_are_flagged() {
    require_engine!("libx264");
    let m = probe_of(&fixture(Fixture::VfrMp4)).await;
    assert!(m.first_video().unwrap().is_vfr(), "{:?}", m.first_video());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_gif_reports_centisecond_time_base() {
    require_engine!("palettegen");
    let dir = tempfile::tempdir().unwrap();
    let gif = dir.path().join("t.gif");
    let st = std::process::Command::new(rmedia_testkit::ffmpeg())
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=64x48:r=15:d=1",
            "-f",
            "gif",
        ])
        .arg(&gif)
        .status()
        .unwrap();
    assert!(st.success());
    let m = probe_of(&gif).await;
    let v = m.first_video().unwrap();
    assert_eq!(v.time_base, Some((1, 100)));
    assert_eq!(v.nb_frames, Some(15), "ffprobe counts a GIF's frames (research R3)");
}
