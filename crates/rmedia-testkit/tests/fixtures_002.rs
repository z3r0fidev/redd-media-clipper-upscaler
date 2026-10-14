//! T014/T015: the 002 fixtures have the properties the upscale tests rely on.

use rmedia_testkit::{Fixture, display_size, fixture, probe_json, require_engine, vmaf};

fn video(kind: Fixture) -> serde_json::Value {
    let j = probe_json(&fixture(kind));
    j["streams"].as_array().unwrap().iter().find(|s| s["codec_type"] == "video").unwrap().clone()
}

#[test]
fn portrait_fixtures_share_content_and_differ_in_size() {
    require_engine!("libx264");
    assert_eq!(display_size(&fixture(Fixture::CleanPortrait)), (1080, 1920));
    assert_eq!(display_size(&fixture(Fixture::DegradedPortrait)), (480, 854));
    assert_eq!(display_size(&fixture(Fixture::DegradedPortrait360)), (360, 640));
    assert_eq!(display_size(&fixture(Fixture::Portrait480Long)), (480, 854));
    assert_eq!(video(Fixture::CleanPortrait)["color_space"], "bt709");
}

#[test]
fn geometry_fixtures_carry_rotation_sar_and_field_order() {
    require_engine!("libx264");
    assert_eq!(display_size(&fixture(Fixture::Rotated90)), (240, 320));
    assert_eq!(video(Fixture::Anamorphic)["sample_aspect_ratio"], "64:45");
    assert_eq!(display_size(&fixture(Fixture::Anamorphic)), (1024, 576));
    assert_eq!(video(Fixture::Interlaced)["field_order"], "tt");
    assert_eq!(video(Fixture::HighFps120)["avg_frame_rate"], "120/1");
}

#[test]
fn hdr_fixture_is_tagged_pq() {
    require_engine!("libx265");
    let v = video(Fixture::HdrTagged);
    assert_eq!(v["codec_name"], "hevc");
    assert_eq!(v["color_transfer"], "smpte2084");
    assert_eq!(v["color_primaries"], "bt2020");
    assert_eq!(v["pix_fmt"], "yuv420p10le");
}

#[test]
fn chapters_fixture_has_two_chapters_and_a_title() {
    require_engine!("libx264");
    let out = std::process::Command::new(rmedia_testkit::ffprobe())
        .args(["-v", "error", "-show_chapters", "-show_format", "-of", "json"])
        .arg(fixture(Fixture::ChaptersMp4))
        .output()
        .unwrap();
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(j["chapters"].as_array().unwrap().len(), 2);
    assert_eq!(j["format"]["tags"]["title"], "rmedia chapters fixture");
}

#[test]
fn marker_fixtures_have_the_expected_length() {
    require_engine!("libx264");
    assert!((rmedia_testkit::duration(&fixture(Fixture::MovingMarker)) - 10.0).abs() < 0.1);
    assert!((rmedia_testkit::duration(&fixture(Fixture::FarMarkers)) - 6.0).abs() < 0.1);
    assert_eq!(display_size(&fixture(Fixture::MovingMarker)), (1280, 720));
}

#[test]
fn vmaf_is_high_for_identical_and_lower_for_degraded() {
    require_engine!("libvmaf", "libx264");
    let clean = fixture(Fixture::CleanPortrait);
    let same = vmaf(&clean, &clean);
    assert!(same >= 99.0, "identical files score {same}");
    let degraded = vmaf(&fixture(Fixture::DegradedPortrait360), &clean);
    assert!(degraded < same - 5.0, "degraded scored {degraded}");
}
