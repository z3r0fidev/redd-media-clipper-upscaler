//! Container capabilities, accurate-mode encode profiles, and stream selection (research.md R4).

use serde::Serialize;

use crate::probe::{MediaInfo, StreamKind};
use crate::request::{AudioSel, Mode, StreamSelection, SubSel};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Container {
    Mp4,
    M4v,
    Mov,
    Mkv,
    Webm,
    MpegTs,
    Avi,
    Asf,
    Flv,
    Mpeg,
    Vob,
    ThreeGp,
    Ogg,
    Mxf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitlePolicy {
    CopyAll,
    /// Text subtitles converted to this codec; bitmap subtitles dropped.
    ConvertText(&'static str),
    Drop,
}

/// Accurate-mode encoder settings ("visually lossless", widely compatible).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodeProfile {
    pub video_encoder: &'static str,
    /// Filter applied to every encoded video stream (e.g. even-dimension crop for 4:2:0).
    pub video_filter: Option<&'static str>,
    pub video_args: &'static [&'static str],
    pub audio_encoder: &'static str,
    pub audio_args: &'static [&'static str],
}

pub const X264_AAC: EncodeProfile = EncodeProfile {
    video_encoder: "libx264",
    // 4:2:0 needs even dimensions: crop (never scale) at most one pixel row/column.
    video_filter: Some("crop=trunc(iw/2)*2:trunc(ih/2)*2"),
    video_args: &["-preset", "medium", "-crf", "18", "-pix_fmt", "yuv420p"],
    audio_encoder: "aac",
    audio_args: &["-b:a", "192k"],
};

pub const VP9_OPUS: EncodeProfile = EncodeProfile {
    video_encoder: "libvpx-vp9",
    video_filter: None,
    video_args: &["-crf", "24", "-b:v", "0", "-row-mt", "1", "-deadline", "good", "-cpu-used", "2"],
    audio_encoder: "libopus",
    audio_args: &["-b:a", "160k"],
};

impl Container {
    pub fn from_ext(ext: &str) -> Option<Container> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "mp4" => Container::Mp4,
            "m4v" => Container::M4v,
            "mov" => Container::Mov,
            "mkv" => Container::Mkv,
            "webm" => Container::Webm,
            "ts" | "m2ts" | "mts" => Container::MpegTs,
            "avi" | "divx" => Container::Avi,
            "wmv" | "asf" => Container::Asf,
            "flv" | "f4v" => Container::Flv,
            "mpg" | "mpeg" => Container::Mpeg,
            "vob" => Container::Vob,
            "3gp" | "3g2" => Container::ThreeGp,
            "ogv" | "ogg" => Container::Ogg,
            "mxf" => Container::Mxf,
            _ => return None,
        })
    }

    /// ffmpeg muxer name (`-f`), needed because temp outputs end in `.part`.
    pub fn muxer(self) -> &'static str {
        match self {
            Container::Mp4 | Container::M4v => "mp4",
            Container::Mov => "mov",
            Container::Mkv => "matroska",
            Container::Webm => "webm",
            Container::MpegTs => "mpegts",
            Container::Avi => "avi",
            Container::Asf => "asf",
            Container::Flv => "flv",
            Container::Mpeg => "mpeg",
            Container::Vob => "vob",
            Container::ThreeGp => "3gp",
            Container::Ogg => "ogg",
            Container::Mxf => "mxf",
        }
    }

    /// Whether demuxer seeking is index-based and lands exactly (MP4/MOV/MKV/WebM). Others
    /// (MPEG-TS/PS, AVI, …) seek approximately and need a coarse+precise two-stage seek.
    pub fn precise_seek(self) -> bool {
        matches!(
            self,
            Container::Mp4
                | Container::M4v
                | Container::Mov
                | Container::Mkv
                | Container::Webm
                | Container::ThreeGp
        )
    }

    pub fn faststart(self) -> bool {
        matches!(self, Container::Mp4 | Container::M4v | Container::Mov | Container::ThreeGp)
    }

    /// `None` = accurate mode cannot encode into this container (falls back to mp4).
    pub fn encode_profile(self) -> Option<EncodeProfile> {
        match self {
            Container::Mp4 | Container::M4v | Container::Mov | Container::Mkv | Container::MpegTs => {
                Some(X264_AAC)
            }
            Container::Webm => Some(VP9_OPUS),
            _ => None,
        }
    }

    pub fn subtitles(self) -> SubtitlePolicy {
        match self {
            Container::Mkv => SubtitlePolicy::CopyAll,
            Container::Mp4 | Container::M4v | Container::Mov => SubtitlePolicy::ConvertText("mov_text"),
            Container::Webm => SubtitlePolicy::ConvertText("webvtt"),
            _ => SubtitlePolicy::Drop,
        }
    }

    pub fn supports_attachments(self) -> bool {
        self == Container::Mkv
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MappedStream {
    pub index: u32,
    pub kind: StreamKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StreamPlan {
    pub maps: Vec<MappedStream>,
    /// Subtitle codec to convert to (`-c:s`), if any subtitles are mapped and need conversion.
    pub subtitle_codec: Option<&'static str>,
    pub warnings: Vec<String>,
}

/// Chooses which input streams go into the output. Never maps cover art or data streams.
pub fn select_streams(
    media: &MediaInfo,
    sel: &StreamSelection,
    container: Container,
    _mode: Mode,
) -> Result<StreamPlan, String> {
    let mut maps = Vec::new();
    let mut warnings = Vec::new();

    for s in media.streams.iter().filter(|s| s.is_real_video()) {
        maps.push(MappedStream { index: s.index, kind: StreamKind::Video });
    }
    if maps.is_empty() {
        return Err("source has no video stream".into());
    }

    let audio: Vec<_> = media.streams.iter().filter(|s| s.kind == StreamKind::Audio).collect();
    match &sel.audio {
        AudioSel::All => {
            maps.extend(audio.iter().map(|s| MappedStream { index: s.index, kind: StreamKind::Audio }))
        }
        AudioSel::None => {}
        AudioSel::Tracks(tracks) => {
            let mut seen = std::collections::HashSet::new();
            for &t in tracks.iter().filter(|t| seen.insert(**t)) {
                let s = (t as usize)
                    .checked_sub(1)
                    .and_then(|i| audio.get(i))
                    .ok_or_else(|| format!("audio track {t} does not exist (source has {})", audio.len()))?;
                maps.push(MappedStream { index: s.index, kind: StreamKind::Audio });
            }
        }
    }

    let mut subtitle_codec = None;
    if sel.subtitles == SubSel::All {
        let subs: Vec<_> = media.streams.iter().filter(|s| s.kind == StreamKind::Subtitle).collect();
        match container.subtitles() {
            SubtitlePolicy::CopyAll => {
                maps.extend(subs.iter().map(|s| MappedStream { index: s.index, kind: StreamKind::Subtitle }))
            }
            SubtitlePolicy::ConvertText(codec) => {
                for s in &subs {
                    if s.is_text_subtitle() {
                        maps.push(MappedStream { index: s.index, kind: StreamKind::Subtitle });
                        subtitle_codec = Some(codec);
                    } else {
                        warnings.push(format!(
                            "subtitle stream {} ({}) cannot be stored in .{}; dropped",
                            s.index,
                            s.codec,
                            container_label(container)
                        ));
                    }
                }
            }
            SubtitlePolicy::Drop => {
                if !subs.is_empty() {
                    warnings.push(format!(
                        "{} subtitle stream(s) cannot be stored in .{}; dropped",
                        subs.len(),
                        container_label(container)
                    ));
                }
            }
        }
    }

    if container.supports_attachments() {
        maps.extend(
            media
                .streams
                .iter()
                .filter(|s| s.kind == StreamKind::Attachment)
                .map(|s| MappedStream { index: s.index, kind: StreamKind::Attachment }),
        );
    }

    Ok(StreamPlan { maps, subtitle_codec, warnings })
}

fn container_label(c: Container) -> &'static str {
    match c {
        Container::MpegTs => "ts",
        Container::Mkv => "mkv",
        Container::Ogg => "ogv",
        Container::ThreeGp => "3gp",
        Container::Asf => "wmv",
        other => other.muxer(),
    }
}
