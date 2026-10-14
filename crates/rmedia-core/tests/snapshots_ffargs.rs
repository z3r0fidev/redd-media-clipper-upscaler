//! T021: exact ffmpeg argv for each mode × container × stream layout, from committed ffprobe
//! JSON (text). Paths are normalized so snapshots are platform independent.

use std::path::{Path, PathBuf};

use rmedia_core::Ts;
use rmedia_core::event::Span;
use rmedia_core::plan::ffargs::{ClipSpec, Trim, clip_args, display_args};
use rmedia_core::plan::streams::{Container, select_streams};
use rmedia_core::probe::{MediaInfo, parse_probe_json};
use rmedia_core::request::{AudioSel, Mode, StreamSelection, SubSel};

fn media(name: &str, ext: &str) -> MediaInfo {
    let json = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/probe").join(format!("{name}.json")),
    )
    .unwrap();
    parse_probe_json(&PathBuf::from(format!("IN.{ext}")), 1, &json).unwrap()
}

type Seeks = (Option<f64>, Option<f64>, Option<f64>);

fn args(m: &MediaInfo, mode: Mode, container: Container, sel: StreamSelection, seeks: Seeks) -> Vec<String> {
    let streams = select_streams(m, &sel, container, mode).unwrap();
    let spec = ClipSpec {
        input: m.path.clone(),
        input_seek: seeks.0.map(Ts::from_secs_f64),
        output_seek: seeks.1.map(Ts::from_secs_f64),
        duration: seeks.2.map(Ts::from_secs_f64),
        trim: None,
        verify_video: None,
        mode,
        requested: Span { start: Ts::ZERO, end: Ts::ZERO },
        predicted: None,
        container,
        encode: if mode == Mode::Accurate { container.encode_profile() } else { None },
        streams,
    };
    let mut v = display_args(&clip_args(&spec, Path::new("OUT.part")));
    v.extend(spec.streams.warnings.iter().map(|w| format!("# warning: {w}")));
    v
}

#[test]
fn mp4_fast_two_stage_keyframe_cut() {
    let m = media("gop50", "mp4");
    insta::assert_json_snapshot!(args(
        &m,
        Mode::Fast,
        Container::Mp4,
        Default::default(),
        (None, Some(9.84), Some(10.16))
    ));
}

#[test]
fn mp4_accurate_single_seek() {
    let m = media("gop50", "mp4");
    insta::assert_json_snapshot!(args(
        &m,
        Mode::Accurate,
        Container::Mp4,
        Default::default(),
        (Some(10.5), None, Some(9.5))
    ));
}

#[test]
fn ts_accurate_copyts_trim() {
    let m = media("offset_ts", "ts");
    let streams = select_streams(&m, &Default::default(), Container::MpegTs, Mode::Accurate).unwrap();
    let spec = ClipSpec {
        input: m.path.clone(),
        input_seek: Some(Ts::from_secs_f64(2.5)),
        output_seek: None,
        duration: None,
        trim: Some(Trim { start: Ts::from_secs_f64(13.98), end: Some(Ts::from_secs_f64(16.48)) }),
        verify_video: None,
        mode: Mode::Accurate,
        requested: Span { start: Ts::ZERO, end: Ts::ZERO },
        predicted: None,
        container: Container::MpegTs,
        encode: Container::MpegTs.encode_profile(),
        streams,
    };
    insta::assert_json_snapshot!(display_args(&clip_args(&spec, Path::new("OUT.part"))));
}

#[test]
fn mkv_fast_copies_everything() {
    let m = media("multi_mkv", "mkv");
    insta::assert_json_snapshot!(args(
        &m,
        Mode::Fast,
        Container::Mkv,
        Default::default(),
        (None, Some(1.84), None)
    ));
}

#[test]
fn mkv_to_mp4_accurate_second_audio_converts_subs() {
    let m = media("multi_mkv", "mkv");
    let sel = StreamSelection { audio: AudioSel::Tracks(vec![2]), subtitles: SubSel::All };
    insta::assert_json_snapshot!(args(
        &m,
        Mode::Accurate,
        Container::Mp4,
        sel,
        (Some(2.0), None, Some(10.0))
    ));
}

#[test]
fn mkv_to_ts_drops_subtitles_with_warning() {
    let m = media("multi_mkv", "mkv");
    let sel = StreamSelection { audio: AudioSel::None, subtitles: SubSel::All };
    insta::assert_json_snapshot!(args(&m, Mode::Accurate, Container::MpegTs, sel, (Some(2.0), None, None)));
}

#[test]
fn webm_accurate_vp9_opus() {
    let m = media("small_webm", "webm");
    insta::assert_json_snapshot!(args(
        &m,
        Mode::Accurate,
        Container::Webm,
        Default::default(),
        (Some(1.0), None, Some(3.0))
    ));
}

#[test]
fn video_only_accurate_has_no_audio_codec() {
    let m = media("noaudio", "mp4");
    insta::assert_json_snapshot!(args(
        &m,
        Mode::Accurate,
        Container::Mp4,
        Default::default(),
        (Some(1.0), None, Some(3.0))
    ));
}
