//! Pure ffmpeg argument builders (research.md R2/R3). Snapshot-tested.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::streams::{Container, EncodeProfile, StreamPlan};
use crate::event::Span;
use crate::probe::{INPUT_PROTOCOLS, StreamKind, file_arg};
use crate::request::Mode;
use crate::time::Ts;

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

pub fn clip_args(spec: &ClipSpec, output: &Path) -> Vec<OsString> {
    let mut a = common_args();
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
    let mut push = |items: &[&str]| a.extend(items.iter().map(OsString::from));
    if let Some(ss) = spec.output_seek.filter(|t| *t > Ts::ZERO) {
        push(&["-ss", &ss.ffmpeg()]);
    }
    if let Some(d) = spec.duration {
        push(&["-t", &d.ffmpeg()]);
    }
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

fn has(spec: &ClipSpec, kind: StreamKind) -> bool {
    spec.streams.maps.iter().any(|m| m.kind == kind)
}

/// Lossy, human-readable rendering of an argv (for dry runs and logs).
pub fn display_args(args: &[OsString]) -> Vec<String> {
    args.iter().map(|a| a.to_string_lossy().into_owned()).collect()
}
