//! T021: exact ffmpeg argv for each mode × container × stream layout, from committed ffprobe
//! JSON (text). Paths are normalized so snapshots are platform independent.

use std::path::{Path, PathBuf};

use rmedia_core::Ts;
use rmedia_core::event::Span;
use rmedia_core::plan::ffargs::{ClipSpec, Trim, clip_args, display_args};
use rmedia_core::plan::streams::{Container, select_streams};
use rmedia_core::probe::{MediaInfo, parse_probe_json};
use rmedia_core::request::{AudioSel, Mode, StreamSelection, SubSel};
use rmedia_core::smart::{Part, PartEncode, SmartPlan};

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
        smart: None,
        smart_fallback: None,
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
        smart: None,
        smart_fallback: None,
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

/// Feature 004, T021: a smart job's five argvs (head, copy, tail, audio, join), from a smart plan
/// written out by hand. `precise`: an indexed source (exact seeks), else MPEG-TS (`trim`).
fn smart_args(
    m: &MediaInfo,
    container: Container,
    precise: bool,
    plan_extra: impl Fn(&mut SmartPlan),
) -> Vec<String> {
    use rmedia_core::smart::args::{concat_list, copy_args, join_args, part_args, rest_spec};
    let streams = select_streams(m, &Default::default(), container, Mode::Accurate).unwrap();
    let s = |v: f64| Ts::from_secs_f64(v);
    let part = |a: f64, b: f64, frames: u32| Part { span: Span { start: s(a), end: s(b) }, frames };
    let mut plan = SmartPlan {
        head: Some(part(3.32, 4.0, 17)),
        copy: part(4.0, 20.0, 400),
        tail: Some(part(20.0, 20.72, 18)),
        copied_share: 400.0 / 435.0,
        bytes: 0,
        copy_lead: s(0.16),
        to_end: false,
        frame: s(0.04),
        first_gop: 50,
        last_gop: 50,
        encode: PartEncode {
            profile: "high",
            level: Some("3.1".into()),
            bframes: 3,
            pix_fmt: "yuv420p",
            colour: Default::default(),
            sar: None,
            width: 320,
            height: 240,
        },
        video_index: 0,
        zero: if precise { Ts::ZERO } else { s(1.4) },
        shift: Ts::ZERO,
        precise,
        sample_entry: matches!(container, Container::Mp4 | Container::Mov).then_some("avc1"),
        rotation: None,
        strict: false,
    };
    plan_extra(&mut plan);
    let spec = ClipSpec {
        input: m.path.clone(),
        input_seek: Some(s(3.32)),
        output_seek: None,
        duration: Some(s(17.4)),
        trim: None,
        verify_video: None,
        mode: Mode::Accurate,
        requested: Span { start: s(3.32), end: s(20.72) },
        predicted: None,
        container,
        encode: container.encode_profile(),
        streams,
        smart: None,
        smart_fallback: None,
    };
    let mut out = Vec::new();
    let mut add = |name: &str, args: Vec<std::ffi::OsString>| {
        out.push(format!("## {name}"));
        out.extend(display_args(&args));
    };
    add("head", part_args(&spec, &plan, plan.head.unwrap(), Path::new("S/head.ts")));
    add("copy", copy_args(&spec, &plan, Path::new("S/copy.ts")));
    add("tail", part_args(&spec, &plan, plan.tail.unwrap(), Path::new("S/tail.ts")));
    let rest = rest_spec(&spec);
    if let Some(r) = &rest {
        add("audio", clip_args(r, Path::new("S/rest.av")));
    }
    add(
        "join",
        join_args(&spec, &plan, rest.as_ref().map(|_| Path::new("S/rest.av")), Path::new("OUT.part")),
    );
    out.push(format!("## list\n{}", concat_list(&["head.ts", "copy.ts", "tail.ts"])));
    out
}

#[test]
fn smart_mp4_parts_and_join() {
    insta::assert_json_snapshot!(smart_args(&media("gop50", "mp4"), Container::Mp4, true, |_| {}));
}

#[test]
fn smart_ts_source_cuts_parts_with_trim() {
    insta::assert_json_snapshot!(smart_args(&media("offset_ts", "ts"), Container::MpegTs, false, |_| {}));
}

#[test]
fn smart_keeps_colour_sar_and_rotation() {
    insta::assert_json_snapshot!(smart_args(&media("gop50", "mp4"), Container::Mkv, true, |p| {
        p.encode.colour = rmedia_core::upscale::encode::ColourTags::bt709();
        p.encode.sar = Some((64, 45));
        p.encode.bframes = 0;
        p.encode.profile = "main";
        p.rotation = Some(90);
        p.shift = Ts::from_secs_f64(0.023);
    }));
}
