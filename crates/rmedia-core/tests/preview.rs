//! T059: preview frames, proxies and thumbnails.

mod common;

use common::*;
use rmedia_core::preview::{PreviewCache, ProxyCodec, ensure_proxy, frame_at, thumbnails, time_offset};
use rmedia_core::probe::probe;
use rmedia_testkit::{Fixture, duration, fixture, require_ffmpeg};
use tokio_util::sync::CancellationToken;

#[tokio::test(flavor = "multi_thread")]
async fn frames_proxies_and_thumbnails() {
    require_ffmpeg!();
    let (tools, _) = engine();
    let c = CancellationToken::new();
    let media = probe(&tools, &fixture(Fixture::MultiMkv), &c).await.unwrap();

    let jpeg = frame_at(&tools, &media, secs(3.0), 120, &c).await.unwrap();
    assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "JPEG magic");

    let dir = tempfile::tempdir().unwrap();
    let cache = PreviewCache { dir: dir.path().to_path_buf(), max_bytes: u64::MAX };
    let seen = std::sync::Mutex::new(Vec::new());
    let report = |f: f64| seen.lock().unwrap().push(f);
    let proxy = ensure_proxy(&tools, &media, &cache, ProxyCodec::H264, &report, &c).await.unwrap();
    assert!(proxy.extension().unwrap() == "mp4");
    let src_len = media.duration.unwrap().as_secs_f64();
    assert!((duration(&proxy) - src_len).abs() < 0.2, "proxy {} vs source {src_len}", duration(&proxy));
    assert_eq!(*seen.lock().unwrap().last().unwrap(), 1.0);
    let t0 = std::time::Instant::now();
    let again = ensure_proxy(&tools, &media, &cache, ProxyCodec::H264, &|_| {}, &c).await.unwrap();
    assert_eq!(again, proxy);
    assert!(t0.elapsed().as_millis() < 200, "second call is a cache hit");
    let vp8 = ensure_proxy(&tools, &media, &cache, ProxyCodec::Vp8, &|_| {}, &c).await.unwrap();
    assert!(vp8.extension().unwrap() == "webm");

    let thumbs = thumbnails(&tools, &media, 5, &cache, &c).await.unwrap();
    assert_eq!(thumbs.len(), 5);
    assert!(thumbs.iter().all(|p| p.is_file()));

    // Eviction: a tiny cap removes everything except what was just produced.
    let small = PreviewCache { dir: dir.path().to_path_buf(), max_bytes: 1 };
    small.evict(Some(&proxy));
    assert!(proxy.is_file());
    assert!(!vp8.is_file());
}

#[tokio::test(flavor = "multi_thread")]
async fn time_offset_is_first_video_frame() {
    require_ffmpeg!();
    let (tools, _) = engine();
    let c = CancellationToken::new();
    let ts = probe(&tools, &fixture(Fixture::OffsetTs), &c).await.unwrap();
    let off = time_offset(&ts).as_secs_f64();
    assert!(off > 0.0 && off < 0.1, "MPEG-TS audio leads video: offset {off}");
    let mp4 = probe(&tools, &fixture(Fixture::Gop50Mp4), &c).await.unwrap();
    assert_eq!(time_offset(&mp4).as_micros(), 0);
}
