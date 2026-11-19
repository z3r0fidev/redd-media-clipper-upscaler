//! Feature 005, US1: a clip with sound that Reddit accepts (tasks T011, T012, T014).

mod common;

use std::path::{Path, PathBuf};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::{JobMeasure, JobOutcome, Span};
use rmedia_core::fit::plan::Mode as FitMode;
use rmedia_core::fit::{FitOptions, FitOutcome};
use rmedia_core::request::Mode;
use rmedia_testkit::{Fixture, audio_start_offset, decode_errors, fixture, mp4_faststart, probe_json};

fn engine_ok() -> bool {
    rmedia_testkit::require_engine(&["libx264", "aac"])
}

/// Plans and runs one fit clip of `src` over `start..end`; returns the output and its fit block.
async fn fit(src: &Path, start: f64, end: f64, fit: FitOptions, out: &Path) -> (PathBuf, Span, FitOutcome) {
    let (tools, caps) = engine();
    let o = rmedia_core::request::Options { fit: Some(fit), ..opts(out, Mode::Fast) };
    let plan = make_plan(&request(src, vec![clip(start, EndSpec::At(secs(end)))], o), &tools, &caps).await;
    assert!(!plan.has_errors(), "{:?}", plan.issues);
    let (summary, _) = run_plan(plan, tools, &out.join("state")).await;
    match &summary.jobs[0].outcome {
        JobOutcome::Succeeded { output, measure: JobMeasure::Clip { actual, fit: Some(f), .. } } => {
            (output.clone(), *actual, f.clone())
        }
        other => panic!("{other:?}"),
    }
}

fn capped(bytes: u64) -> FitOptions {
    FitOptions { max_size: Some(bytes), ..FitOptions::default() }
}

/// `(codec_type, codec_name, profile, pix_fmt, channels, width, height)` of each stream.
fn streams(p: &Path) -> Vec<(String, String, String, String, u64, u64, u64)> {
    let j = probe_json(p);
    let s = |v: &serde_json::Value, k: &str| v[k].as_str().unwrap_or("").to_owned();
    let n = |v: &serde_json::Value, k: &str| v[k].as_u64().unwrap_or(0);
    j["streams"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            (
                s(v, "codec_type"),
                s(v, "codec_name"),
                s(v, "profile"),
                s(v, "pix_fmt"),
                n(v, "channels"),
                n(v, "width"),
                n(v, "height"),
            )
        })
        .collect()
}

/// T011 (SC-001–SC-003): a tight cap is met and mostly used; the output is a phone- and
/// web-ready MP4 of the exact range with its audio in sync.
#[tokio::test(flavor = "multi_thread")]
async fn a_tight_cap_is_met_with_sound_in_sync() {
    if !engine_ok() {
        return;
    }
    let src = fixture(Fixture::FitBusy);
    let out = tempfile::tempdir().unwrap();
    let cap = 8_000_000;
    let (file, actual, f) = fit(&src, 2.0, 27.0, capped(cap), out.path()).await;
    let bytes = std::fs::metadata(&file).unwrap().len();
    assert_eq!(f.bytes, bytes);
    assert!(bytes <= cap, "{bytes} > {cap}");
    assert!(bytes as f64 >= 0.9 * cap as f64, "only {bytes} of {cap} (SC-002)");
    assert_eq!((f.mode, f.attempts.len(), f.trimmed), (FitMode::Budget, 1, false), "{f:?}");
    assert_eq!(file.extension().unwrap(), "mp4");
    let st = streams(&file);
    assert_eq!(st.len(), 2, "{st:?}");
    assert_eq!(
        (st[0].0.as_str(), st[0].1.as_str(), st[0].2.as_str(), st[0].3.as_str()),
        ("video", "h264", "High", "yuv420p")
    );
    assert_eq!((st[1].0.as_str(), st[1].1.as_str(), st[1].2.as_str(), st[1].4), ("audio", "aac", "LC", 2));
    assert!(st[0].5.is_multiple_of(2) && st[0].6.is_multiple_of(2));
    assert!(mp4_faststart(&file), "moov must come before mdat (FR-016)");
    let len = (actual.end - actual.start).as_secs_f64();
    assert!((len - 25.0).abs() <= 1.0 / 30.0 + 1e-3, "length {len}");
    let offset = audio_start_offset(&file);
    assert!(offset.abs() <= 1024.0 / 48_000.0, "audio starts {offset} s off the video");
    assert!(decode_errors(&file).is_empty(), "{:?}", decode_errors(&file));
}

/// T012 (FR-007, FR-013): a generous cap keeps the target's quality instead of filling it, and
/// a source without audio gives the whole budget to the picture.
#[tokio::test(flavor = "multi_thread")]
async fn a_generous_cap_is_not_filled_and_no_audio_means_no_audio_track() {
    if !engine_ok() {
        return;
    }
    let out = tempfile::tempdir().unwrap();
    let (file, _, f) = fit(&fixture(Fixture::FitShort), 0.0, 2.0, FitOptions::default(), out.path()).await;
    assert_eq!((f.mode, f.attempts.len()), (FitMode::Quality, 1), "{f:?}");
    // The quality ceiling: 7,465 kb/s of video over 2 s plus its 2 s VBV buffer, and the audio.
    let ceiling = 7465 * 125 * 4 + 128 * 125 * 2;
    assert!(f.bytes <= ceiling, "{} > {ceiling}", f.bytes);
    assert!(f.bytes < f.cap / 100);
    assert_eq!(std::fs::metadata(&file).unwrap().len(), f.bytes);

    let out = tempfile::tempdir().unwrap();
    let (file, _, f) = fit(&fixture(Fixture::FitNoAudio), 2.0, 27.0, capped(8_000_000), out.path()).await;
    assert_eq!(f.audio_kbps, None);
    assert_eq!(f.video_kbps, 2508, "the whole budget goes to the video");
    assert!(streams(&file).iter().all(|s| s.0 == "video"), "{:?}", streams(&file));
    assert!(f.bytes <= 8_000_000 && f.bytes >= 7_200_000, "{}", f.bytes);
}

/// T014 (R12): the same fit twice gives the same bytes.
#[tokio::test(flavor = "multi_thread")]
async fn a_fit_is_deterministic() {
    if !engine_ok() {
        return;
    }
    let src = fixture(Fixture::FitBusy);
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (fa, _, _) = fit(&src, 3.0, 9.0, capped(2_000_000), a.path()).await;
    let (fb, _, _) = fit(&src, 3.0, 9.0, capped(2_000_000), b.path()).await;
    assert_eq!(rmedia_testkit::sha256(&fa), rmedia_testkit::sha256(&fb));
}
