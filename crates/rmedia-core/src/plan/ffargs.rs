//! Pure ffmpeg argument builders (research.md R2/R3). Snapshot-tested.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::streams::{Container, EncodeProfile, StreamPlan};
use crate::event::Span;
use crate::probe::{INPUT_PROTOCOLS, StreamKind, file_arg};
use crate::request::Mode;
use crate::time::Ts;
use crate::upscale::{UpscaleSpec, graph};

/// Everything needed to cut one clip.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClipSpec {
    #[serde(serialize_with = "crate::pathser::path")]
    pub input: PathBuf,
    /// Input-side `-ss` (fast, possibly approximate demuxer seek). User time base.
    pub input_seek: Option<Ts>,
    /// Output-side `-ss` (exact, relative to the input seek point).
    pub output_seek: Option<Ts>,
    /// Output `-t`, measured from the output seek point. `None` = to the end of the source.
    pub duration: Option<Ts>,
    /// Accurate mode on sources without exact seeking: cut with `trim`/`atrim` filters at these
    /// absolute source timestamps (`-copyts`), independent of how the coarse seek landed.
    pub trim: Option<Trim>,
    /// Accurate mode: (expected video length, tolerance) checked after encoding.
    #[serde(skip)]
    pub verify_video: Option<(Ts, Ts)>,
    pub mode: Mode,
    /// The span the user asked for (user time base).
    pub requested: Span,
    /// Fast mode: the span the copy is expected to produce (starts on a keyframe).
    pub predicted: Option<Span>,
    pub container: Container,
    #[serde(skip)]
    pub encode: Option<EncodeProfile>,
    pub streams: StreamPlan,
}

/// Absolute source timestamps for filter-based cutting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Trim {
    pub start: Ts,
    pub end: Option<Ts>,
}

impl Trim {
    fn params(&self) -> String {
        match self.end {
            Some(e) => format!("start={}:end={}", self.start.ffmpeg(), e.ffmpeg()),
            None => format!("start={}", self.start.ffmpeg()),
        }
    }
}

/// Arguments common to every ffmpeg invocation.
pub fn common_args() -> Vec<OsString> {
    [
        "-hide_banner",
        "-nostdin",
        "-n",
        "-loglevel",
        "warning",
        "-progress",
        "pipe:1",
        "-nostats",
        "-stats_period",
        "0.25",
    ]
    .iter()
    .map(OsString::from)
    .collect()
}

/// The input and the source range: `[-copyts] [-ss] -i file:<input> [-ss] [-t]`.
pub fn input_args(spec: &ClipSpec) -> Vec<OsString> {
    let (mut a, out) = input_parts(spec);
    a.extend(out);
    a
}

/// [`input_args`] in two parts: through `-i file:<input>`, and the output-side `-ss`/`-t`
/// (which must follow any further input, or they would apply to it).
pub(crate) fn input_parts(spec: &ClipSpec) -> (Vec<OsString>, Vec<OsString>) {
    let mut a = Vec::new();
    let mut push = |items: &[&str]| a.extend(items.iter().map(OsString::from));
    if spec.trim.is_some() {
        push(&["-copyts"]);
    }
    if let Some(ss) = spec.input_seek.filter(|t| *t > Ts::ZERO) {
        push(&["-ss", &ss.ffmpeg()]);
    }
    push(&["-protocol_whitelist", INPUT_PROTOCOLS]);
    a.push("-i".into());
    a.push(file_arg(&spec.input));
    let mut out = Vec::new();
    let mut push = |items: &[&str]| out.extend(items.iter().map(OsString::from));
    if let Some(ss) = spec.output_seek.filter(|t| *t > Ts::ZERO) {
        push(&["-ss", &ss.ffmpeg()]);
    }
    if let Some(d) = spec.duration {
        push(&["-t", &d.ffmpeg()]);
    }
    (a, out)
}

pub fn clip_args(spec: &ClipSpec, output: &Path) -> Vec<OsString> {
    let mut a = common_args();
    a.extend(input_args(spec));
    let mut push = |items: &[&str]| a.extend(items.iter().map(OsString::from));
    for m in &spec.streams.maps {
        push(&["-map", &format!("0:{}", m.index)]);
    }
    push(&["-map_metadata", "0", "-map_chapters", "-1"]);

    match (spec.mode, spec.encode) {
        (Mode::Accurate, Some(p)) => {
            push(&["-fps_mode", "passthrough", "-c:v", p.video_encoder]);
            let vf: Vec<String> = spec
                .trim
                .map(|t| format!("trim={},setpts=PTS-STARTPTS", t.params()))
                .into_iter()
                .chain(p.video_filter.map(str::to_owned))
                .collect();
            if !vf.is_empty() {
                push(&["-vf", &vf.join(",")]);
            }
            push(p.video_args);
            if has(spec, StreamKind::Audio) {
                push(&["-c:a", p.audio_encoder]);
                if let Some(t) = spec.trim {
                    push(&["-af", &format!("atrim={},asetpts=PTS-STARTPTS", t.params())]);
                }
                push(p.audio_args);
            }
            if has(spec, StreamKind::Attachment) {
                push(&["-c:t", "copy"]);
            }
        }
        _ => {
            push(&["-c", "copy"]);
        }
    }
    if let Some(codec) = spec.streams.subtitle_codec {
        push(&["-c:s", codec]);
    } else if spec.mode == Mode::Accurate && has(spec, StreamKind::Subtitle) {
        push(&["-c:s", "copy"]);
    }
    if spec.mode == Mode::Fast {
        push(&["-avoid_negative_ts", "make_zero"]);
    }
    if spec.container.faststart() {
        push(&["-movflags", "+faststart"]);
    }
    push(&["-f", spec.container.muxer()]);
    a.push(file_arg(output));
    a
}

/// One-pass upscale: the clip's input and range, the upscale filtergraph for video, the clip's
/// audio and subtitle streams, chapters and metadata, and the preset's encoder (research R2, R5).
pub fn upscale_args(spec: &UpscaleSpec, output: &Path) -> Vec<OsString> {
    if spec.up.ai.is_some() {
        return ai_args(spec, output).encode;
    }
    let (cut, up) = (&spec.cut, &spec.up);
    let mut a = common_args();
    if let Some(device) = up.graph.gpu {
        // The GPU tier pins its Vulkan device, so a software renderer is never picked (R9).
        a.extend([
            "-init_hw_device".into(),
            format!("vulkan=vk:{device}").into(),
            OsString::from("-filter_hw_device"),
            "vk".into(),
        ]);
    }
    a.extend(input_args(cut));
    a.extend(["-filter_complex".into(), OsString::from(graph::build(&up.graph))]);
    encode_tail(spec, &mut a, output);
    a
}

/// The AI tier's three argvs (research R13): ffmpeg #1 writes the framed picture as raw RGB to
/// stdout, the worker upscales it, ffmpeg #2 reads it on stdin as its second input (the source
/// stays the first, for audio, subtitles, chapters and the blurred background) and encodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiArgs {
    pub decode: Vec<OsString>,
    pub worker: Vec<OsString>,
    pub encode: Vec<OsString>,
}

/// The AI pipeline of an AI-tier job. Panics if the job has no AI engine (`up.ai`).
pub fn ai_args(spec: &UpscaleSpec, output: &Path) -> AiArgs {
    let (cut, up) = (&spec.cut, &spec.up);
    let engine = up.ai.as_ref().expect("an AI-tier job has its engine");
    let stage = crate::upscale::ai::stage(&up.graph);
    let rate = up.rate();

    let mut decode: Vec<OsString> =
        ["-hide_banner", "-nostdin", "-loglevel", "warning"].iter().map(OsString::from).collect();
    decode.extend(input_args(cut));
    for s in
        ["-filter_complex", &graph::ai_decode(&up.graph, &rate), "-map", "[v]", "-fps_mode", "passthrough"]
    {
        decode.push(s.into());
    }
    decode.extend(["-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"].iter().map(OsString::from));

    let worker = engine.args(stage.scale, up.tier.device, stage.input);

    let (out_w, out_h) = stage.output();
    let (source, range) = input_parts(cut);
    let mut encode = common_args();
    encode.extend(source);
    let mut raw: Vec<String> =
        ["-f", "rawvideo", "-pix_fmt", "rgb24"].iter().map(|s| (*s).to_owned()).collect();
    raw.extend(["-video_size".into(), format!("{out_w}x{out_h}"), "-framerate".into(), rate.clone()]);
    // A whole video whose audio starts first: the frames start where the source's video does.
    if up.cmd_offset > 0.0 {
        raw.extend(["-itsoffset".into(), format!("{:.6}", up.cmd_offset)]);
    }
    raw.extend(["-i".into(), "pipe:0".into()]);
    encode.extend(raw.into_iter().map(OsString::from));
    encode.extend(range);
    encode.extend(["-filter_complex".into(), OsString::from(graph::ai_finish(&up.graph, &stage, &rate))]);
    encode_tail(spec, &mut encode, output);
    AiArgs { decode, worker, encode }
}

/// Everything after the filtergraph: the video label, the clip's other streams, chapters and
/// metadata (from the source, input 0), the preset's encoder, audio, subtitles and the output.
fn encode_tail(spec: &UpscaleSpec, a: &mut Vec<OsString>, output: &Path) {
    let (cut, up) = (&spec.cut, &spec.up);
    let mut push = |items: &[&str]| a.extend(items.iter().map(OsString::from));
    push(&["-map", "[v]"]);
    for m in cut.streams.maps.iter().filter(|m| m.kind != StreamKind::Video) {
        push(&["-map", &format!("0:{}", m.index)]);
    }
    // Unlike clips, a whole-video upscale keeps its chapters (FR-020).
    push(&[
        "-map_metadata",
        "0",
        "-map_chapters",
        if cut.trim.is_none() && cut.input_seek.is_none() { "0" } else { "-1" },
    ]);
    push(&["-fps_mode", "passthrough"]);
    let video: Vec<&str> = up.video_args.iter().map(String::as_str).collect();
    push(&video);
    if has(cut, StreamKind::Audio) {
        let trim = cut.trim.map(|t| format!("atrim={},asetpts=PTS-STARTPTS", t.params()));
        if up.audio_copy && trim.is_none() {
            push(&["-c:a", "copy"]);
        } else {
            push(&["-c:a", "aac", "-b:a", &format!("{}k", up.preset.audio.bitrate_k)]);
            if let Some(t) = &trim {
                push(&["-af", t]);
            }
        }
    }
    if has(cut, StreamKind::Subtitle) {
        push(&["-c:s", cut.streams.subtitle_codec.unwrap_or("mov_text")]);
    }
    push(&["-movflags", "+faststart", "-f", "mp4"]);
    a.push(file_arg(output));
}

fn has(spec: &ClipSpec, kind: StreamKind) -> bool {
    spec.streams.maps.iter().any(|m| m.kind == kind)
}

/// Lossy, human-readable rendering of an argv (for dry runs and logs).
pub fn display_args(args: &[OsString]) -> Vec<String> {
    args.iter().map(|a| a.to_string_lossy().into_owned()).collect()
}
