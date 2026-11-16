//! Smart cut (feature 004): frame-exact clips that copy every whole GOP in the range and
//! re-encode only the partial GOPs at each end (research R1–R11).

pub mod args;
pub mod keyframes;

use serde::{Deserialize, Serialize};

use crate::event::Span;
use crate::plan::streams::Container;
use crate::probe::MediaInfo;
use crate::time::Ts;
use crate::upscale::encode::ColourTags;

pub use keyframes::{KeyframeEntry, KeyframeIndex, keyframes};

/// Packet times within this of a cut point count as on it (ffprobe prints microseconds).
const SLACK: Ts = Ts::from_micros(500);

/// Why a smart job runs in accurate mode instead (research R2). The serialized values are stable
/// once released (FR-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum FallbackReason {
    /// The engine has no H.264 encoder (libx264).
    EncoderMissing,
    /// The video is not H.264.
    Codec,
    /// The source or output container is not mp4, mov, mkv or ts.
    Container,
    /// The H.264 profile is not Constrained Baseline, Baseline, Main or High.
    Profile,
    /// The pixel format is not 8-bit 4:2:0.
    PixelFormat,
    Interlaced,
    /// Variable frame rate.
    Vfr,
    /// A display rotation the final mux cannot carry.
    Rotation,
    /// The range holds no complete GOP.
    NoFullGop,
    /// A GOP at a copy boundary references the previous GOP.
    OpenGop,
    /// The frames at a seam changed size or pixel format (found after encoding).
    ParamsChanged,
    /// The joined result failed verification (found after encoding).
    SeamCheck,
}

impl FallbackReason {
    pub const ALL: [FallbackReason; 12] = [
        FallbackReason::EncoderMissing,
        FallbackReason::Codec,
        FallbackReason::Container,
        FallbackReason::Profile,
        FallbackReason::PixelFormat,
        FallbackReason::Interlaced,
        FallbackReason::Vfr,
        FallbackReason::Rotation,
        FallbackReason::NoFullGop,
        FallbackReason::OpenGop,
        FallbackReason::ParamsChanged,
        FallbackReason::SeamCheck,
    ];

    /// The serialized value (`open_gop`).
    pub fn as_str(self) -> &'static str {
        match self {
            FallbackReason::EncoderMissing => "encoder_missing",
            FallbackReason::Codec => "codec",
            FallbackReason::Container => "container",
            FallbackReason::Profile => "profile",
            FallbackReason::PixelFormat => "pixel_format",
            FallbackReason::Interlaced => "interlaced",
            FallbackReason::Vfr => "vfr",
            FallbackReason::Rotation => "rotation",
            FallbackReason::NoFullGop => "no_full_gop",
            FallbackReason::OpenGop => "open_gop",
            FallbackReason::ParamsChanged => "params_changed",
            FallbackReason::SeamCheck => "seam_check",
        }
    }

    /// A plain-language reason, completing "smart cut not possible: …".
    pub fn message(self) -> &'static str {
        match self {
            FallbackReason::EncoderMissing => "the engine has no H.264 encoder (libx264)",
            FallbackReason::Codec => "the video is not H.264; smart cut handles H.264 only",
            FallbackReason::Container => "smart cut handles mp4, mov, mkv and ts files only",
            FallbackReason::Profile => "the video's H.264 profile is not Baseline, Main or High",
            FallbackReason::PixelFormat => "the video is not 8-bit 4:2:0 (for example 10-bit or 4:2:2)",
            FallbackReason::Interlaced => "the video is interlaced",
            FallbackReason::Vfr => "the video has a variable frame rate",
            FallbackReason::Rotation => "the video's rotation cannot be kept in the output",
            FallbackReason::NoFullGop => "the range holds no complete group of pictures (it is too short)",
            FallbackReason::OpenGop => "the video has open GOPs (frames that depend on the previous GOP)",
            FallbackReason::ParamsChanged => "the video's size or pixel format changes at a cut point",
            FallbackReason::SeamCheck => "the joined clip failed the seam check",
        }
    }

    /// Whether the plan can find this reason, before anything is encoded (FR-011).
    pub fn known_before_encoding(self) -> bool {
        !matches!(self, FallbackReason::ParamsChanged | FallbackReason::SeamCheck)
    }
}

impl std::fmt::Display for FallbackReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One step's frames: the span (user time) and how many frames it holds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "SmartPart"))]
pub struct Part {
    pub span: Span,
    pub frames: u32,
}

/// How a smart job cuts its video (data-model.md › SmartPlan). Times are user time.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SmartPlan {
    /// Re-encoded from the start to the first keyframe in the range; `None` when the start is
    /// on a keyframe.
    pub head: Option<Part>,
    /// The whole GOPs, copied.
    pub copy: Part,
    /// Re-encoded from the last keyframe in the range to the end; `None` when the end is on a
    /// keyframe or is the end of the video.
    pub tail: Option<Part>,
    /// Copied frames ÷ all frames.
    pub copied_share: f64,
    /// How far before K1 the copy is cut (research R3): more than the B-frame delay, so the
    /// keyframe's packet is kept, and less than half the GOP before it, so that GOP is not.
    pub copy_lead: Ts,
    /// The copy runs to the end of the video (no tail, no end boundary).
    pub to_end: bool,
    /// The spacing of the source's frames.
    pub frame: Ts,
    /// Frames in the first and in the last copied GOP (the seam checks decode them, R7).
    pub first_gop: u32,
    pub last_gop: u32,
    pub encode: PartEncode,
    /// The video stream's index in the source.
    pub video_index: u32,
    /// The source's absolute time of user time zero, and the `-ss` shift (zero point minus the
    /// container start), as for accurate mode.
    pub zero: Ts,
    pub shift: Ts,
    /// The source seeks exactly (indexed demuxer); otherwise parts are cut with `trim`.
    pub precise: bool,
    /// The output's video sample entry (mp4, m4v, mov: `avc1`, spike S1).
    pub sample_entry: Option<&'static str>,
    /// The display rotation to set again in the join (spike S2).
    pub rotation: Option<i32>,
    /// Strict mode (`smart_fallback: false`): a failed seam check fails the job (FR-009).
    pub strict: bool,
}

impl SmartPlan {
    /// Frames the clip holds.
    pub fn total_frames(&self) -> u32 {
        self.head.map_or(0, |p| p.frames) + self.copy.frames + self.tail.map_or(0, |p| p.frames)
    }
}

/// The re-encoded parts' settings, matched to the source (research R4). No free-form strings.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PartEncode {
    /// x264 profile (`baseline`, `main`, `high`).
    pub profile: &'static str,
    /// H.264 level (`3.1`).
    pub level: Option<String>,
    /// 0 when the source has no B-frames, else 3.
    pub bframes: u32,
    pub pix_fmt: &'static str,
    pub colour: ColourTags,
    /// The sample aspect ratio, when not square.
    pub sar: Option<(u32, u32)>,
    pub width: u32,
    pub height: u32,
}

/// The range to cut (user time); `to_end` = it runs to the end of the video.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    pub span: Span,
    pub to_end: bool,
}

/// The checks that need only the probe (research R2, in its order): the engine, codec,
/// containers, profile, pixel format, interlacing, frame rate and rotation.
pub fn check_source(media: &MediaInfo, has_x264: bool, output: Container) -> Result<(), FallbackReason> {
    let v = media.first_video().ok_or(FallbackReason::Codec)?;
    if !has_x264 {
        return Err(FallbackReason::EncoderMissing);
    }
    if v.codec != "h264" {
        return Err(FallbackReason::Codec);
    }
    let source_ok =
        media.format_name.split(',').any(|f| matches!(f.trim(), "mov" | "mp4" | "matroska" | "mpegts"));
    let output_ok = matches!(
        output,
        Container::Mp4 | Container::M4v | Container::Mov | Container::Mkv | Container::MpegTs
    );
    if !source_ok || !output_ok {
        return Err(FallbackReason::Container);
    }
    x264_profile(v.profile.as_deref()).ok_or(FallbackReason::Profile)?;
    if !matches!(v.pix_fmt.as_deref(), Some("yuv420p" | "yuvj420p")) {
        return Err(FallbackReason::PixelFormat);
    }
    if v.is_interlaced() {
        return Err(FallbackReason::Interlaced);
    }
    if v.is_vfr() {
        return Err(FallbackReason::Vfr);
    }
    if v.rotation.is_some() && output == Container::MpegTs {
        return Err(FallbackReason::Rotation);
    }
    Ok(())
}

/// Every check (research R2): [`check_source`], then the index rules (frame spacing, a whole
/// GOP in the range, closed GOPs at both copy boundaries), giving the plan.
pub fn check(
    media: &MediaInfo,
    index: &KeyframeIndex,
    range: Range,
    has_x264: bool,
    output: Container,
) -> Result<SmartPlan, FallbackReason> {
    check_source(media, has_x264, output)?;
    let v = media.first_video().ok_or(FallbackReason::Codec)?;
    let frame = index.frame_duration.ok_or(FallbackReason::Vfr)?;
    let (a, z) = (range.span.start, range.span.end);
    let keys = &index.keyframes;
    let k1 = keys.iter().find(|k| k.at + SLACK >= a).ok_or(FallbackReason::NoFullGop)?;
    let k2 = if range.to_end { None } else { keys.iter().rev().find(|k| k.at <= z + SLACK).copied() };
    if let Some(k2) = k2
        && k2.at <= k1.at
    {
        return Err(FallbackReason::NoFullGop);
    }
    if !k1.clean || k2.is_some_and(|k| !k.clean) {
        return Err(FallbackReason::OpenGop);
    }
    // Frames with a time in [from, to), as accurate mode cuts them.
    let frames =
        |from: Ts, to: Ts| index.frames_between(from.saturating_sub(SLACK), to.saturating_sub(SLACK));
    let copy_end = k2.map_or(z, |k| k.at);
    let part = |from: Ts, to: Ts| Part { span: Span { start: from, end: to }, frames: frames(from, to) };
    let head = (k1.at > a + SLACK).then(|| part(a, k1.at));
    let copy = part(k1.at, copy_end);
    let tail = k2.filter(|k| k.at + SLACK < z).map(|k| part(k.at, z));
    if copy.frames == 0 || head.is_some_and(|p| p.frames == 0) || tail.is_some_and(|p| p.frames == 0) {
        return Err(FallbackReason::NoFullGop);
    }
    let next_key = |after: Ts| keys.iter().find(|k| k.at > after + SLACK).map(|k| k.at);
    let first_gop = frames(k1.at, next_key(k1.at).map_or(copy_end, |n| n.min(copy_end)));
    let last_start = keys.iter().rev().find(|k| k.at + SLACK < copy_end).map_or(k1.at, |k| k.at.max(k1.at));
    let last_gop = frames(last_start, copy_end);
    let precise = media.has_indexed_seeking();
    // A stream copy drops packets by decode time, and the keyframe's comes up to the B-frame
    // delay before its presentation time (research R3).
    let lead_frames = i64::from(v.has_b_frames.unwrap_or(2) + 2).max(4);
    let prev = keys.iter().rev().find(|k| k.at + SLACK < k1.at).map(|k| k.at);
    let copy_lead = Ts::from_micros(frame.as_micros() * lead_frames)
        .min(prev.map_or(Ts::from_micros(i64::MAX / 4), |p| Ts::from_micros((k1.at - p).as_micros() / 2)));
    let width = v.width.ok_or(FallbackReason::ParamsChanged)?;
    let height = v.height.ok_or(FallbackReason::ParamsChanged)?;
    if width % 2 == 1 || height % 2 == 1 {
        // x264 cannot encode an odd-sized 4:2:0 picture, so the parts could not match.
        return Err(FallbackReason::ParamsChanged);
    }
    let pix_fmt = if v.pix_fmt.as_deref() == Some("yuvj420p") { "yuvj420p" } else { "yuv420p" };
    let encode = PartEncode {
        profile: x264_profile(v.profile.as_deref()).ok_or(FallbackReason::Profile)?,
        level: v.level.map(|l| format!("{}.{}", l / 10, l % 10)),
        bframes: if v.has_b_frames == Some(0) { 0 } else { 3 },
        pix_fmt,
        colour: ColourTags {
            space: v.color_space.clone(),
            primaries: v.color_primaries.clone(),
            transfer: v.color_transfer.clone(),
            range: v.color_range.clone(),
        },
        sar: v.sar.filter(|(n, d)| n != d),
        width,
        height,
    };
    let total = head.map_or(0, |p| p.frames) + copy.frames + tail.map_or(0, |p| p.frames);
    Ok(SmartPlan {
        head,
        copy,
        tail,
        // Four decimals, as the JSON contract shows it.
        copied_share: (f64::from(copy.frames) / f64::from(total) * 1e4).round() / 1e4,
        copy_lead,
        to_end: range.to_end,
        frame,
        first_gop,
        last_gop,
        encode,
        video_index: v.index,
        zero: media.zero_point(),
        shift: media.zero_point().saturating_sub(media.start_time),
        precise,
        sample_entry: matches!(output, Container::Mp4 | Container::M4v | Container::Mov).then_some("avc1"),
        rotation: v.rotation,
        strict: false,
    })
}

/// The x264 profile for an ffprobe H.264 profile name; `None` for the profiles smart cut does
/// not handle (High 10, 4:2:2, 4:4:4, Extended, …).
fn x264_profile(name: Option<&str>) -> Option<&'static str> {
    match name? {
        "Constrained Baseline" | "Baseline" => Some("baseline"),
        "Main" => Some("main"),
        "High" => Some("high"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialized_values_are_stable() {
        let values: Vec<String> = FallbackReason::ALL
            .iter()
            .map(|r| serde_json::to_value(r).unwrap().as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            values,
            [
                "encoder_missing",
                "codec",
                "container",
                "profile",
                "pixel_format",
                "interlaced",
                "vfr",
                "rotation",
                "no_full_gop",
                "open_gop",
                "params_changed",
                "seam_check"
            ]
        );
        for r in FallbackReason::ALL {
            assert_eq!(r.as_str(), serde_json::to_value(r).unwrap());
            let back: FallbackReason = serde_json::from_value(serde_json::json!(r.as_str())).unwrap();
            assert_eq!(back, r);
        }
    }

    #[test]
    fn messages_are_plain_sentences_and_two_reasons_need_encoding() {
        for r in FallbackReason::ALL {
            let m = r.message();
            assert!(!m.is_empty() && !m.ends_with('.') && m.chars().next().unwrap().is_lowercase(), "{m}");
        }
        let late: Vec<_> = FallbackReason::ALL.into_iter().filter(|r| !r.known_before_encoding()).collect();
        assert_eq!(late, [FallbackReason::ParamsChanged, FallbackReason::SeamCheck]);
    }
}
