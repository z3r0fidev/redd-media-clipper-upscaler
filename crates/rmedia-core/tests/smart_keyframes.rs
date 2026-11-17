//! Feature 004: the keyframe index on real fixtures (research R1, R2; task T011).

mod common;

use common::*;
use rmedia_core::event::Span;
use rmedia_core::probe;
use rmedia_core::smart::keyframes;
use rmedia_testkit::{Fixture, fixture};
use tokio_util::sync::CancellationToken;

async fn index(kind: Fixture, window: Option<Span>) -> rmedia_core::smart::KeyframeIndex {
    let (tools, _) = engine();
    let cancel = CancellationToken::new();
    let media = probe::probe(&tools, &fixture(kind), &cancel).await.expect("probe");
    keyframes(&tools, &media, window, &cancel).await.expect("index")
}

#[tokio::test(flavor = "multi_thread")]
async fn eligible_sources_have_clean_keyframes_every_two_seconds() {
    if !rmedia_testkit::require_engine(&["libx264"]) {
        return;
    }
    for kind in [Fixture::SmartMp4, Fixture::SmartMov, Fixture::SmartMkv, Fixture::SmartTs] {
        let idx = index(kind, None).await;
        let at: Vec<f64> = idx.keyframes.iter().map(|k| k.at.as_secs_f64()).collect();
        assert_eq!(at.len(), 15, "{kind:?}: {at:?}");
        for (i, t) in at.iter().enumerate() {
            assert!((t - 2.0 * i as f64).abs() < 0.04, "{kind:?}: keyframe {i} at {t} (user time)");
        }
        assert!(idx.keyframes.iter().all(|k| k.clean), "{kind:?}");
        assert_eq!(idx.frame_duration, Some(secs(0.04)), "{kind:?}");
        assert!(!idx.truncated && idx.window.is_none() && idx.packets.is_empty(), "{kind:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn open_gop_keyframes_are_unclean() {
    if !rmedia_testkit::require_engine(&["libx264"]) {
        return;
    }
    let idx = index(Fixture::OpenGop, None).await;
    let clean: Vec<bool> = idx.keyframes.iter().map(|k| k.clean).collect();
    assert_eq!(clean.len(), 5, "{clean:?}");
    assert!(clean[0], "the first GOP has nothing before it");
    // x264 puts leading pictures after some keyframes only (a GOP without them is closed in
    // fact): the index must agree with the packets, read independently, keyframe by keyframe.
    let packets = rmedia_testkit::video_packets(&fixture(Fixture::OpenGop));
    let expected: Vec<bool> = packets
        .iter()
        .enumerate()
        .filter(|(_, p)| p.key)
        .map(|(i, k)| packets[i + 1..].iter().take_while(|p| !p.key).all(|p| p.pts > k.pts))
        .collect();
    assert_eq!(clean, expected);
    assert!(clean.iter().any(|c| !c), "the fixture has open GOPs: {clean:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_window_keeps_its_packets_for_counting_frames() {
    if !rmedia_testkit::require_engine(&["libx264"]) {
        return;
    }
    let w = Span { start: secs(5.0), end: secs(13.0) };
    for kind in [Fixture::SmartMp4, Fixture::SmartTs] {
        let idx = index(kind, Some(w)).await;
        assert_eq!(idx.window, Some(w));
        // The read starts early enough to hold a keyframe at or before the window's start.
        let first = idx.keyframes.first().expect("keyframes").at;
        assert!(first <= secs(4.0), "{kind:?}: {first}");
        assert_eq!(idx.frames_between(secs(6.0), secs(12.0)), 150, "{kind:?}");
        for k in [4.0, 6.0, 8.0, 10.0, 12.0] {
            assert!(idx.keyframes.iter().any(|e| e.at == secs(k)), "{kind:?}: no keyframe at {k}");
        }
    }
}

/// A long file is read in chunks; keyframes near chunk boundaries are neither lost nor doubled,
/// even in MPEG-TS, where seeks land imprecisely.
#[tokio::test(flavor = "multi_thread")]
async fn chunked_reads_find_every_keyframe_once() {
    if !rmedia_testkit::require_engine(&["libx264"]) {
        return;
    }
    let (tools, _) = engine();
    let cancel = CancellationToken::new();
    for kind in [Fixture::SmartTs, Fixture::SmartMkv, Fixture::OpenGop] {
        let media = probe::probe(&tools, &fixture(kind), &cancel).await.unwrap();
        let whole = keyframes(&tools, &media, None, &cancel).await.unwrap();
        let chunked =
            rmedia_core::smart::keyframes::keyframes_in_chunks(&tools, &media, None, secs(7.0), &cancel)
                .await
                .unwrap();
        assert_eq!(chunked.keyframes, whole.keyframes, "{kind:?}");
        assert_eq!(chunked.frame_duration, whole.frame_duration, "{kind:?}");
        // T059 review: a file without a duration (some ts and mkv) is read to its end, not just
        // its first chunk.
        let mut no_duration = media.clone();
        no_duration.duration = None;
        let chunked = rmedia_core::smart::keyframes::keyframes_in_chunks(
            &tools,
            &no_duration,
            None,
            secs(7.0),
            &cancel,
        )
        .await
        .unwrap();
        assert_eq!(chunked.keyframes, whole.keyframes, "{kind:?} without a duration");
        assert!(!chunked.truncated);
    }
}
