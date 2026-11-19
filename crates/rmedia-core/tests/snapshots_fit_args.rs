//! Feature 005, T015: the exact argv of a fit's pass 1, pass 2 and quality pass, from committed
//! ffprobe JSON (text). Paths are normalized so the snapshots are platform independent.

use std::path::{Path, PathBuf};

use rmedia_core::Ts;
use rmedia_core::event::Span;
use rmedia_core::fit::FitOptions;
use rmedia_core::fit::plan::{FitPlan, Source, plan_fit};
use rmedia_core::fit::target::VideoTargetRegistry;
use rmedia_core::plan::ffargs::{ClipSpec, FitPass, display_args, fit_args};
use rmedia_core::plan::streams::{Container, select_streams};
use rmedia_core::probe::parse_probe_json;
use rmedia_core::request::{Mode, StreamSelection};

fn spec(max_size: u64, len: f64) -> ClipSpec {
    let json =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/probe/gop50.json"))
            .unwrap();
    let m = parse_probe_json(&PathBuf::from("IN.mp4"), 1, &json).unwrap();
    let streams = select_streams(&m, &StreamSelection::default(), Container::Mp4, Mode::Accurate).unwrap();
    let target = VideoTargetRegistry::builtin().get("reddit-video").unwrap().clone();
    let fit: FitPlan = plan_fit(
        &target,
        &FitOptions { max_size: Some(max_size), ..FitOptions::default() },
        Source { len, size: (320, 240), fps: (25, 1), has_audio: true },
    )
    .unwrap();
    ClipSpec {
        input: m.path.clone(),
        input_seek: Some(Ts::from_secs_f64(9.0)),
        output_seek: Some(Ts::from_secs_f64(1.0)),
        duration: Some(Ts::from_secs_f64(len)),
        trim: None,
        verify_video: None,
        mode: Mode::Accurate,
        requested: Span { start: Ts::from_secs_f64(10.0), end: Ts::from_secs_f64(10.0 + len) },
        predicted: None,
        container: Container::Mp4,
        encode: Container::Mp4.encode_profile(),
        streams,
        smart: None,
        smart_fallback: None,
        fit: Some(fit),
    }
}

#[test]
fn fit_budget_pass1() {
    insta::assert_json_snapshot!(display_args(&fit_args(
        &spec(600_000, 20.0),
        FitPass::First,
        Path::new("OUT.part")
    )));
}

#[test]
fn fit_budget_pass2() {
    let s = spec(600_000, 20.0);
    let kbps = s.fit.as_ref().unwrap().video_kbps;
    insta::assert_json_snapshot!(display_args(&fit_args(
        &s,
        FitPass::Second { video_kbps: kbps },
        Path::new("OUT.part")
    )));
}

#[test]
fn fit_quality_pass() {
    insta::assert_json_snapshot!(display_args(&fit_args(
        &spec(1 << 30, 20.0),
        FitPass::Quality,
        Path::new("OUT.part")
    )));
}
