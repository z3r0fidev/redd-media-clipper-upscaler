//! Argument builders for a smart job's head, copy, tail, audio and join steps (research R3–R6).
//! Pure functions over the plan, snapshot-tested (feature 004, US1).

use std::ffi::OsString;
use std::path::Path;

use super::{Part, SmartPlan};
use crate::plan::ffargs::{ClipSpec, common_args};
use crate::probe::{INPUT_PROTOCOLS, StreamKind, file_arg};
use crate::request::Mode;
use crate::time::Ts;

/// How far before a part an approximate (MPEG-TS) seek lands, as in accurate mode.
const COARSE_SEEK_MARGIN: Ts = Ts::from_micros(10_000_000);

/// The concat list the join reads (relative names: the join runs in the scratch folder).
pub const LIST: &str = "list.txt";
pub const HEAD: &str = "head.ts";
pub const COPY: &str = "copy.ts";
pub const TAIL: &str = "tail.ts";

/// A re-encoded part (head or tail): an exact seek, exactly `part.frames` frames, the source's
/// parameters (research R4), written as MPEG-TS with in-band parameter sets.
pub fn part_args(spec: &ClipSpec, plan: &SmartPlan, part: Part, out: &Path) -> Vec<OsString> {
    let mut a = common_args();
    let mut push = |items: &[&str]| a.extend(items.iter().map(OsString::from));
    let at = part.span.start;
    if plan.precise {
        if at + plan.shift > Ts::ZERO {
            push(&["-ss", &(at + plan.shift).ffmpeg()]);
        }
    } else {
        push(&["-copyts"]);
        let coarse = at.saturating_sub(COARSE_SEEK_MARGIN);
        if coarse > Ts::ZERO {
            push(&["-ss", &coarse.ffmpeg()]);
        }
    }
    // The copied GOPs keep the coded picture; rotation is set again on the join (spike S2).
    push(&["-noautorotate", "-protocol_whitelist", INPUT_PROTOCOLS]);
    a.push("-i".into());
    a.push(file_arg(&spec.input));
    let mut push = |items: &[&str]| a.extend(items.iter().map(OsString::from));
    push(&["-map", &format!("0:{}", plan.video_index), "-frames:v", &part.frames.to_string()]);
    let mut vf = Vec::new();
    if !plan.precise {
        vf.push(format!("trim=start={},setpts=PTS-STARTPTS", (plan.zero + at).ffmpeg()));
    }
    if let Some((n, d)) = plan.encode.sar {
        vf.push(format!("setsar={n}/{d}"));
    }
    if !vf.is_empty() {
        push(&["-vf", &vf.join(",")]);
    }
    let e = &plan.encode;
    push(&["-c:v", "libx264", "-preset", "medium", "-crf", "18", "-profile:v", e.profile]);
    if let Some(level) = &e.level {
        push(&["-level:v", level]);
    }
    push(&["-pix_fmt", e.pix_fmt, "-x264-params", &format!("bframes={}:open-gop=0:stitchable=1", e.bframes)]);
    let c = &e.colour;
    for (flag, value) in [
        ("-colorspace", &c.space),
        ("-color_primaries", &c.primaries),
        ("-color_trc", &c.transfer),
        ("-color_range", &c.range),
    ] {
        if let Some(v) = value {
            push(&[flag, v]);
        }
    }
    push(&["-fps_mode", "passthrough", "-an", "-sn", "-dn", "-f", "mpegts"]);
    a.push(file_arg(out));
    a
}

/// The copy: the GOPs [K1, K2) as a stream copy (research R3). A coarse input seek lands before
/// K1; the output seek cuts `copy_lead` before it, so the output starts on K1's packet (a copy
/// waits for a keyframe); and `-frames:v` ends it on the last packet before K2, since the GOPs are
/// closed.
pub fn copy_args(spec: &ClipSpec, plan: &SmartPlan, out: &Path) -> Vec<OsString> {
    let mut a = common_args();
    let mut push = |items: &[&str]| a.extend(items.iter().map(OsString::from));
    let k1 = plan.copy.span.start;
    let coarse = k1.saturating_sub(COARSE_SEEK_MARGIN);
    let input_seek = coarse + plan.shift;
    if input_seek > Ts::ZERO {
        push(&["-ss", &input_seek.ffmpeg()]);
    }
    push(&["-protocol_whitelist", INPUT_PROTOCOLS]);
    a.push("-i".into());
    a.push(file_arg(&spec.input));
    let mut push = |items: &[&str]| a.extend(items.iter().map(OsString::from));
    // The output timeline starts at the input seek point (or the container start).
    let cut = (k1 + plan.shift).saturating_sub(input_seek).saturating_sub(plan.copy_lead);
    if cut > Ts::ZERO {
        push(&["-ss", &cut.ffmpeg()]);
    }
    push(&["-map", &format!("0:{}", plan.video_index), "-c", "copy"]);
    if !plan.to_end {
        push(&["-frames:v", &plan.copy.frames.to_string()]);
    }
    push(&["-f", "mpegts"]);
    a.push(file_arg(out));
    a
}

/// The clip's other streams (audio, subtitles, attachments) cut exactly as accurate mode cuts
/// them (research R6). `None` when the clip has only video.
pub fn rest_spec(spec: &ClipSpec) -> Option<ClipSpec> {
    let mut streams = spec.streams.clone();
    streams.maps.retain(|m| m.kind != StreamKind::Video);
    if streams.maps.is_empty() {
        return None;
    }
    if !streams.maps.iter().any(|m| m.kind == StreamKind::Subtitle) {
        streams.subtitle_codec = None;
    }
    Some(ClipSpec { mode: Mode::Accurate, smart: None, verify_video: None, streams, ..spec.clone() })
}

/// The concat list for the parts, in order.
pub fn concat_list(parts: &[&str]) -> String {
    parts.iter().map(|p| format!("file '{p}'\n")).collect()
}

/// The join: the parts through the concat demuxer, the other streams from `rest`, the source's
/// metadata, copied into the output container (research R5). Runs in the scratch folder.
pub fn join_args(spec: &ClipSpec, plan: &SmartPlan, rest: Option<&Path>, output: &Path) -> Vec<OsString> {
    let mut a = common_args();
    let mut push = |items: &[&str]| a.extend(items.iter().map(OsString::from));
    if let Some(r) = plan.rotation {
        push(&["-display_rotation:v:0", &r.to_string()]);
    }
    push(&["-f", "concat"]);
    a.push("-i".into());
    a.push(file_arg(Path::new(LIST)));
    let mut push = |items: &[&str]| a.extend(items.iter().map(OsString::from));
    push(&["-protocol_whitelist", INPUT_PROTOCOLS]);
    a.push("-i".into());
    a.push(file_arg(&spec.input));
    if let Some(r) = rest {
        a.push("-i".into());
        a.push(file_arg(r));
    }
    let mut push = |items: &[&str]| a.extend(items.iter().map(OsString::from));
    push(&["-map", "0:v:0"]);
    if rest.is_some() {
        push(&["-map", "2"]);
    }
    push(&["-map_metadata", "1", "-map_chapters", "-1", "-c", "copy"]);
    if let Some(entry) = plan.sample_entry {
        push(&["-tag:v", entry]);
    }
    if spec.container.faststart() {
        push(&["-movflags", "+faststart"]);
    }
    push(&["-f", spec.container.muxer()]);
    a.push(file_arg(output));
    a
}
