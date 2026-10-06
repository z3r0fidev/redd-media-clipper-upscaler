//! Media inspection via ffprobe.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::exec::process::run_capture;
use crate::time::Ts;
use crate::tools::Tools;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamKind {
    Video,
    Audio,
    Subtitle,
    Data,
    Attachment,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StreamInfo {
    /// Index within the input file (for `-map 0:<index>`).
    pub index: u32,
    pub kind: StreamKind,
    pub codec: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// Average frame rate as (num, den).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_rate: Option<(u32, u32)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pix_fmt: Option<String>,
    /// Cover art / thumbnail pseudo-video stream.
    pub is_attached_pic: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<Ts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<Ts>,
}

impl StreamInfo {
    pub fn is_real_video(&self) -> bool {
        self.kind == StreamKind::Video && !self.is_attached_pic
    }

    /// Text subtitles can be converted between formats; bitmap subtitles cannot.
    pub fn is_text_subtitle(&self) -> bool {
        self.kind == StreamKind::Subtitle
            && matches!(
                self.codec.as_str(),
                "subrip" | "srt" | "ass" | "ssa" | "mov_text" | "webvtt" | "text" | "microdvd"
            )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MediaInfo {
    pub path: PathBuf,
    pub size: u64,
    /// Lower-case file extension (empty if none).
    pub container: String,
    pub format_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<Ts>,
    /// Container start time (earliest stream). User time 0 is `zero_point()`.
    pub start_time: Ts,
    pub streams: Vec<StreamInfo>,
}

impl MediaInfo {
    /// A video is a file with at least one real (non cover-art) video stream.
    pub fn is_video(&self) -> bool {
        self.streams.iter().any(StreamInfo::is_real_video)
    }

    pub fn first_video(&self) -> Option<&StreamInfo> {
        self.streams.iter().find(|s| s.is_real_video())
    }

    /// True when ffprobe's demuxer is index-based (MP4/MOV family, Matroska/WebM), where demuxer
    /// seeking is exact. Based on the detected format, not the file extension.
    pub fn has_indexed_seeking(&self) -> bool {
        self.format_name.split(',').any(|f| matches!(f.trim(), "mov" | "mp4" | "matroska" | "webm"))
    }

    /// Absolute timestamp that user time 0 refers to: the first video frame, as in ffmpeg's
    /// own `-ss`/filter timeline (falls back to the container start).
    pub fn zero_point(&self) -> Ts {
        self.first_video().and_then(|v| v.start_time).unwrap_or(self.start_time)
    }

    /// Duration of one frame of the first video stream, if the rate is known.
    pub fn frame_duration(&self) -> Option<Ts> {
        let (num, den) = self.first_video()?.frame_rate?;
        (num > 0).then(|| Ts::from_micros(den as i64 * 1_000_000 / num as i64))
    }
}

/// Runs ffprobe on `path`.
pub async fn probe(tools: &Tools, path: &Path, cancel: &CancellationToken) -> Result<MediaInfo> {
    let meta = std::fs::metadata(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Error::SourceMissing(path.to_path_buf()),
        _ => Error::ProbeFailed { path: path.to_path_buf(), message: e.to_string() },
    })?;
    let args: Vec<OsString> = vec![
        "-v".into(),
        "error".into(),
        "-protocol_whitelist".into(),
        INPUT_PROTOCOLS.into(),
        "-print_format".into(),
        "json".into(),
        "-show_format".into(),
        "-show_streams".into(),
        file_arg(path),
    ];
    let out = with_timeout(run_capture(&tools.ffprobe.path, &args, cancel), path).await?;
    if !out.status.success() {
        return Err(Error::ProbeFailed {
            path: path.to_path_buf(),
            message: first_line_or(&out.stderr_tail, "ffprobe could not read the file"),
        });
    }
    let json = String::from_utf8_lossy(&out.stdout);
    parse_probe_json(path, meta.len(), &json)
}

/// Probing reads headers only; a file that takes longer is treated as unreadable.
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

async fn with_timeout<T>(fut: impl std::future::Future<Output = Result<T>>, path: &Path) -> Result<T> {
    // Dropping the future kills the child (kill-on-drop).
    tokio::time::timeout(PROBE_TIMEOUT, fut).await.unwrap_or_else(|_| {
        Err(Error::ProbeFailed { path: path.to_path_buf(), message: "ffprobe timed out".into() })
    })
}

/// Builds `MediaInfo` from ffprobe JSON (pure; used by tests with committed JSON).
pub fn parse_probe_json(path: &Path, size: u64, json: &str) -> Result<MediaInfo> {
    let raw: RawProbe = serde_json::from_str(json).map_err(|e| Error::ProbeFailed {
        path: path.to_path_buf(),
        message: format!("unexpected ffprobe output: {e}"),
    })?;
    let format = raw
        .format
        .ok_or_else(|| Error::ProbeFailed { path: path.to_path_buf(), message: "not a media file".into() })?;
    let streams =
        raw.streams
            .into_iter()
            .map(|s| StreamInfo {
                index: s.index,
                kind: match s.codec_type.as_deref() {
                    Some("video") => StreamKind::Video,
                    Some("audio") => StreamKind::Audio,
                    Some("subtitle") => StreamKind::Subtitle,
                    Some("data") => StreamKind::Data,
                    Some("attachment") => StreamKind::Attachment,
                    _ => StreamKind::Unknown,
                },
                codec: s.codec_name.unwrap_or_default(),
                width: s.width,
                height: s.height,
                frame_rate: s.avg_frame_rate.as_deref().and_then(parse_rational),
                pix_fmt: s.pix_fmt,
                is_attached_pic: s.disposition.is_some_and(|d| d.attached_pic == 1),
                language: s.tags.as_ref().and_then(|t| t.language.clone()),
                start_time: s.start_time.as_deref().and_then(parse_secs),
                // Matroska/WebM streams have no duration field; ffmpeg writes a DURATION tag instead.
                duration: s.duration.as_deref().and_then(parse_secs).or_else(|| {
                    s.tags.as_ref().and_then(|t| t.duration.as_deref()).and_then(parse_clock_tag)
                }),
            })
            .collect();
    Ok(MediaInfo {
        path: path.to_path_buf(),
        size,
        container: path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default(),
        format_name: format.format_name.unwrap_or_default(),
        duration: format.duration.as_deref().and_then(parse_secs),
        start_time: format.start_time.as_deref().and_then(parse_secs).unwrap_or(Ts::ZERO),
        streams,
    })
}

/// Finds the last keyframe at or before user time `t` in the first video stream by scanning
/// packets in a window before `t`. Demuxer seeking is not used to decide (it is imprecise for
/// MPEG-TS/PS), so the result is exact for every container. Returns user time.
pub async fn keyframe_at_or_before(
    tools: &Tools,
    media: &MediaInfo,
    t: Ts,
    cancel: &CancellationToken,
) -> Result<Option<Keyframe>> {
    let Some(video) = media.first_video() else { return Ok(None) };
    let zero = media.zero_point();
    let target = t + zero;
    for window in [Ts::from_micros(12_000_000), Ts::from_micros(65_000_000)] {
        let from = target.saturating_sub(window);
        let args: Vec<OsString> = vec![
            "-v".into(),
            "error".into(),
            "-protocol_whitelist".into(),
            INPUT_PROTOCOLS.into(),
            "-select_streams".into(),
            format!("{}", video.index).into(),
            "-read_intervals".into(),
            format!("{}%{}", from.ffmpeg(), (target + Ts::from_micros(1)).ffmpeg()).into(),
            "-show_entries".into(),
            "packet=pts_time,flags".into(),
            "-of".into(),
            "json".into(),
            file_arg(&media.path),
        ];
        let out = with_timeout(run_capture(&tools.ffprobe.path, &args, cancel), &media.path).await?;
        if !out.status.success() {
            return Ok(None);
        }
        if let Some((at, previous)) = last_keyframe_at_or_before(&out.stdout, target) {
            return Ok(Some(Keyframe {
                at: at.saturating_sub(zero),
                previous: previous.map(|p| p.saturating_sub(zero)),
            }));
        }
        if from == Ts::ZERO {
            break;
        }
    }
    Ok(None)
}

/// A keyframe position (user time) and the keyframe before it, if seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keyframe {
    pub at: Ts,
    pub previous: Option<Ts>,
}

/// From ffprobe packet JSON: the greatest keyframe pts ≤ `target` (absolute) and the one before.
fn last_keyframe_at_or_before(json: &[u8], target: Ts) -> Option<(Ts, Option<Ts>)> {
    #[derive(Deserialize)]
    struct Packets {
        #[serde(default)]
        packets: Vec<Packet>,
    }
    #[derive(Deserialize)]
    struct Packet {
        pts_time: Option<String>,
        flags: Option<String>,
    }
    let parsed: Packets = serde_json::from_slice(json).ok()?;
    let mut keys: Vec<Ts> = parsed
        .packets
        .iter()
        .filter(|p| p.flags.as_deref().is_some_and(|f| f.starts_with('K')))
        .filter_map(|p| p.pts_time.as_deref().and_then(parse_secs))
        .filter(|pts| *pts <= target)
        .collect();
    keys.sort();
    keys.dedup();
    let at = keys.pop()?;
    Some((at, keys.pop()))
}

/// Inputs may only be read from local files: playlists (HLS, concat, SDP) that reference network
/// URLs cannot make ffmpeg/ffprobe fetch anything.
pub const INPUT_PROTOCOLS: &str = "file,pipe";

/// Paths are passed with the `file:` protocol so names like `-x.mp4` or `a:b.mp4` are never
/// interpreted as options or protocols.
pub fn file_arg(path: &Path) -> OsString {
    let mut s = OsString::from("file:");
    s.push(path.as_os_str());
    s
}

fn parse_secs(v: &str) -> Option<Ts> {
    let f: f64 = v.parse().ok()?;
    (f.is_finite() && f >= 0.0).then(|| Ts::from_secs_f64(f))
}

fn parse_rational(v: &str) -> Option<(u32, u32)> {
    let (n, d) = v.split_once('/')?;
    let (n, d) = (n.parse().ok()?, d.parse().ok()?);
    (n > 0 && d > 0).then_some((n, d))
}

fn first_line_or(text: &str, fallback: &str) -> String {
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_owned())
        .unwrap_or_else(|| fallback.to_owned())
}

#[derive(Deserialize)]
struct RawProbe {
    #[serde(default)]
    streams: Vec<RawStream>,
    format: Option<RawFormat>,
}

#[derive(Deserialize)]
struct RawStream {
    index: u32,
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    pix_fmt: Option<String>,
    start_time: Option<String>,
    duration: Option<String>,
    disposition: Option<RawDisposition>,
    tags: Option<RawTags>,
}

#[derive(Deserialize)]
struct RawDisposition {
    #[serde(default)]
    attached_pic: u8,
}

#[derive(Deserialize)]
struct RawTags {
    language: Option<String>,
    #[serde(rename = "DURATION")]
    duration: Option<String>,
}

/// `HH:MM:SS.nnnnnnnnn` (Matroska DURATION tag) → Ts.
fn parse_clock_tag(v: &str) -> Option<Ts> {
    let mut parts = v.trim().splitn(3, ':');
    let h: u64 = parts.next()?.parse().ok()?;
    let m: u64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    (s.is_finite() && s >= 0.0).then(|| Ts::from_secs_f64((h * 3600 + m * 60) as f64 + s))
}

#[derive(Deserialize)]
struct RawFormat {
    format_name: Option<String>,
    duration: Option<String>,
    start_time: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "streams": [
        {"index":0,"codec_type":"video","codec_name":"h264","width":320,"height":240,
         "avg_frame_rate":"25/1","start_time":"0.000000","duration":"60.000000",
         "disposition":{"attached_pic":0}},
        {"index":1,"codec_type":"audio","codec_name":"aac","tags":{"language":"eng"}},
        {"index":2,"codec_type":"video","codec_name":"mjpeg","avg_frame_rate":"0/0",
         "disposition":{"attached_pic":1}},
        {"index":3,"codec_type":"subtitle","codec_name":"subrip"},
        {"index":4,"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle"},
        {"index":5,"codec_type":"data","codec_name":"bin_data"}
      ],
      "format": {"format_name":"mov,mp4,m4a,3gp,3g2,mj2","duration":"60.023000","start_time":"0.000000"}
    }"#;

    #[test]
    fn parses_streams_and_format() {
        let m = parse_probe_json(Path::new("/v/Clip.MP4"), 10, SAMPLE).unwrap();
        assert_eq!(m.container, "mp4");
        assert_eq!(m.duration, Some(Ts::from_micros(60_023_000)));
        assert!(m.is_video());
        assert_eq!(m.streams[0].frame_rate, Some((25, 1)));
        assert_eq!(m.frame_duration(), Some(Ts::from_micros(40_000)));
        assert!(m.streams[2].is_attached_pic && !m.streams[2].is_real_video());
        assert!(m.streams[3].is_text_subtitle());
        assert!(!m.streams[4].is_text_subtitle());
        assert_eq!(m.streams[1].language.as_deref(), Some("eng"));
        assert_eq!(m.streams[5].kind, StreamKind::Data);
        assert!(m.has_indexed_seeking());
        let ts = r#"{"streams":[],"format":{"format_name":"mpegts"}}"#;
        assert!(!parse_probe_json(Path::new("mislabelled.mp4"), 1, ts).unwrap().has_indexed_seeking());
    }

    #[test]
    fn cover_art_only_is_not_video() {
        let json = r#"{"streams":[{"index":0,"codec_type":"audio","codec_name":"aac"},
            {"index":1,"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}}],
            "format":{"format_name":"mov","duration":"3.0"}}"#;
        assert!(!parse_probe_json(Path::new("a.m4a"), 1, json).unwrap().is_video());
    }

    #[test]
    fn matroska_duration_tags() {
        let json = r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264","tags":{"DURATION":"00:00:02.280000000"}}],
            "format":{"format_name":"matroska,webm","duration":"4.08"}}"#;
        let m = parse_probe_json(Path::new("a.mkv"), 1, json).unwrap();
        assert_eq!(m.streams[0].duration, Some(Ts::from_micros(2_280_000)));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_probe_json(Path::new("x"), 1, "{}").is_err());
        assert!(parse_probe_json(Path::new("x"), 1, "nope").is_err());
    }

    #[test]
    fn picks_last_keyframe_not_after_target() {
        let json = br#"{"packets":[{"pts_time":"13.4","flags":"K__"},{"pts_time":"14.0","flags":"___"},
            {"pts_time":"15.4","flags":"K__"},{"pts_time":"16.1","flags":"___"},{"pts_time":"17.4","flags":"K__"}]}"#;
        let k = last_keyframe_at_or_before(json, Ts::from_secs_f64(16.377));
        let s = Ts::from_secs_f64;
        assert_eq!(k, Some((s(15.4), Some(s(13.4)))));
        assert_eq!(last_keyframe_at_or_before(json, s(13.0)), None);
        assert_eq!(last_keyframe_at_or_before(json, s(13.4)), Some((s(13.4), None)));
    }

    #[test]
    fn file_arg_prefixes() {
        assert_eq!(file_arg(Path::new("-x.mp4")), OsString::from("file:-x.mp4"));
    }
}
