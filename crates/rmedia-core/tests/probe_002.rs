//! T016 (002): probe fields the upscaler needs — rotation, SAR, field order, colour tags —
//! read from real files.

mod common;

use common::*;
use rmedia_core::probe::{self, MediaInfo};
use rmedia_testkit::{Fixture, fixture, require_engine};
use tokio_util::sync::CancellationToken;

async fn probe_of(kind: Fixture) -> MediaInfo {
    let (tools, _) = engine();
    probe::probe(&tools, &fixture(kind), &CancellationToken::new()).await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn rotation_is_applied_to_the_displayed_size() {
    require_engine!("libx264");
    let m = probe_of(Fixture::Rotated90).await;
    let v = m.first_video().unwrap();
    assert_eq!((v.width, v.height), (Some(320), Some(240)), "coded size");
    assert_eq!(v.rotation, Some(90));
    assert_eq!(v.display_size(), Some((240, 320)));
}

#[tokio::test(flavor = "multi_thread")]
async fn anamorphic_sar_widens_the_displayed_size() {
    require_engine!("libx264");
    let m = probe_of(Fixture::Anamorphic).await;
    let v = m.first_video().unwrap();
    assert_eq!(v.sar, Some((64, 45)));
    assert_eq!(v.display_size(), Some((1024, 576)));
    assert!(!v.is_interlaced());
}

#[tokio::test(flavor = "multi_thread")]
async fn interlaced_and_hdr_sources_are_recognised() {
    require_engine!("libx264", "libx265");
    let i = probe_of(Fixture::Interlaced).await;
    assert!(i.first_video().unwrap().is_interlaced());
    let h = probe_of(Fixture::HdrTagged).await;
    let v = h.first_video().unwrap();
    assert!(v.is_hdr());
    assert_eq!(v.color_transfer.as_deref(), Some("smpte2084"));
    assert_eq!(v.color_primaries.as_deref(), Some("bt2020"));
    assert_eq!(v.codec_tag.as_deref(), Some("hvc1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn tagged_and_untagged_sdr() {
    require_engine!("libx264");
    let c = probe_of(Fixture::CleanPortrait).await;
    let v = c.first_video().unwrap();
    assert_eq!(v.color_space.as_deref(), Some("bt709"));
    assert_eq!(v.color_range.as_deref(), Some("tv"));
    assert!(!v.is_hdr());
    let g = probe_of(Fixture::Gop50Mp4).await;
    let v = g.first_video().unwrap();
    assert_eq!((v.color_space.as_deref(), v.rotation, v.sar), (None, None, Some((1, 1))));
    assert_eq!(v.display_size(), Some((320, 240)));
}
