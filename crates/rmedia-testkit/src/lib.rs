//! Test fixtures generated at test time with FFmpeg's lavfi sources (no binary fixtures are
//! committed — Constitution III), plus ffprobe-based assertions.
//!
//! Fixtures are generated once into `target/rmedia-fixtures/<hash>/` (temp + rename, safe for
//! parallel test processes). Set `RMEDIA_REQUIRE_FFMPEG=1` (CI) to fail instead of skipping
//! when ffmpeg is missing.

use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

/// Fixture kinds (research.md R10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fixture {
    /// 320x240 @ 25 fps, 60 s, keyframe every 2.000 s, B-frames, sine AAC audio.
    Gop50Mp4,
    /// 1920x1080 @ 30 fps, 60 s, keyframe every 2 s (for SC-001 timing).
    Hd60Mp4,
    /// MKV with 2 audio tracks and an SRT subtitle, 20 s.
    MultiMkv,
    /// VP9/Opus WebM, 10 s.
    SmallWebm,
    /// Video only, 10 s.
    NoAudioMp4,
    /// Variable frame rate MP4, ~20 s.
    VfrMp4,
    /// MPEG-TS whose timestamps start at 10 s.
    OffsetTs,
    /// Audio with cover art (not a video).
    CoverArtM4a,
    /// An MPEG-TS stream misnamed `.mp4` (approximate seeking despite the extension).
    MislabelledTs,
    /// 321x241 H.264 (odd dimensions; 4:2:0 encoders need even sizes).
    OddDimsMp4,
    /// A text file named .mp4.
    FakeMp4,
    /// A JPEG named .mp4 (a still image, not a video).
    PhotoMp4,
    // --- 002 (mobile upscaling) ----------------------------------------------------------------
    /// 1080x1920 @ 30 fps, 10 s, near-lossless, BT.709-tagged: the clean reference for VMAF.
    CleanPortrait,
    /// `CleanPortrait` scaled to 480x854 and compressed hard (x264 CRF 35): same content.
    DegradedPortrait,
    /// `CleanPortrait` scaled to 360x640 and compressed hard (x264 CRF 35).
    DegradedPortrait360,
    /// 480x854 @ 30 fps, 60 s, untagged (SC-003 bench).
    Portrait480Long,
    /// 320x240, 10 s, with two chapters and a title tag.
    ChaptersMp4,
    /// 1280x720, 10 s: an 80x80 magenta marker moving right at 150 px/s on gray for 6 s, then a
    /// hard cut to a blue-gray shot with the marker still at x=1000 (research R7).
    MovingMarker,
    /// 1280x720, 6 s: two magenta markers 1000 px apart (cannot fit one portrait crop).
    FarMarkers,
    /// `TinySource` remuxed with a 90° display rotation (coded 320x240, displayed 240x320).
    Rotated90,
    /// 720x576 with SAR 64:45 (anamorphic 16:9), 5 s.
    Anamorphic,
    /// 720x576, interlaced top field first, 5 s.
    Interlaced,
    /// 640x360 10-bit HEVC tagged BT.2020 / PQ (HDR10 signalling), 3 s.
    HdrTagged,
    /// 320x240 (≤ 240 lines: "tiny"), 3 s.
    TinySource,
    /// 320x240 @ 120 fps, 5 s.
    HighFps120,
}

impl Fixture {
    fn file_name(self) -> &'static str {
        match self {
            Fixture::Gop50Mp4 => "gop50.mp4",
            Fixture::Hd60Mp4 => "hd60.mp4",
            Fixture::MultiMkv => "multi.mkv",
            Fixture::SmallWebm => "small.webm",
            Fixture::NoAudioMp4 => "noaudio.mp4",
            Fixture::VfrMp4 => "vfr.mp4",
            Fixture::OffsetTs => "offset.ts",
            Fixture::CoverArtM4a => "coverart.m4a",
            Fixture::MislabelledTs => "mislabelled-ts.mp4",
            Fixture::OddDimsMp4 => "odd.mp4",
            Fixture::FakeMp4 => "fake.mp4",
            Fixture::PhotoMp4 => "photo.mp4",
            Fixture::CleanPortrait => "clean-portrait.mp4",
            Fixture::DegradedPortrait => "degraded-portrait.mp4",
            Fixture::DegradedPortrait360 => "degraded-portrait-360.mp4",
            Fixture::Portrait480Long => "portrait480-long.mp4",
            Fixture::ChaptersMp4 => "chapters.mp4",
            Fixture::MovingMarker => "moving-marker.mp4",
            Fixture::FarMarkers => "far-markers.mp4",
            Fixture::Rotated90 => "rotated90.mp4",
            Fixture::Anamorphic => "anamorphic.mp4",
            Fixture::Interlaced => "interlaced.mp4",
            Fixture::HdrTagged => "hdr10.mp4",
            Fixture::TinySource => "tiny.mp4",
            Fixture::HighFps120 => "fps120.mp4",
        }
    }

    /// The fixture this one is made from (its path replaces `{src}` in [`Fixture::args`]).
    fn derived_from(self) -> Option<Fixture> {
        match self {
            Fixture::DegradedPortrait | Fixture::DegradedPortrait360 => Some(Fixture::CleanPortrait),
            Fixture::Rotated90 => Some(Fixture::TinySource),
            _ => None,
        }
    }

    /// ffmpeg arguments (before the output path). `{dir}` is replaced with the fixture dir.
    fn args(self) -> Vec<&'static str> {
        match self {
            Fixture::Gop50Mp4 => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=25:duration=60",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=60",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-g",
                "50",
                "-keyint_min",
                "50",
                "-sc_threshold",
                "0",
                "-bf",
                "2",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-b:a",
                "96k",
                "-shortest",
                "-f",
                "mp4",
            ],
            Fixture::Hd60Mp4 => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=1920x1080:rate=30:duration=60",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=60",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-g",
                "60",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                "-f",
                "mp4",
            ],
            Fixture::MultiMkv => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=25:duration=20",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=20",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=880:duration=20",
                "-i",
                "{dir}/subs.srt",
                "-map",
                "0",
                "-map",
                "1",
                "-map",
                "2",
                "-map",
                "3",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-g",
                "50",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-c:s",
                "srt",
                "-shortest",
                "-f",
                "matroska",
            ],
            Fixture::SmallWebm => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=25:duration=10",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=10",
                "-c:v",
                "libvpx-vp9",
                "-deadline",
                "realtime",
                "-cpu-used",
                "8",
                "-b:v",
                "300k",
                "-g",
                "50",
                "-c:a",
                "libopus",
                "-shortest",
                "-f",
                "webm",
            ],
            Fixture::NoAudioMp4 => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=25:duration=10",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-g",
                "50",
                "-pix_fmt",
                "yuv420p",
                "-f",
                "mp4",
            ],
            Fixture::VfrMp4 => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=30:duration=20",
                "-vf",
                "select='not(mod(n\\,3))+lt(t\\,5)'",
                "-fps_mode",
                "vfr",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-g",
                "40",
                "-pix_fmt",
                "yuv420p",
                "-f",
                "mp4",
            ],
            Fixture::OffsetTs => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=25:duration=20",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=20",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-g",
                "50",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                "-output_ts_offset",
                "10",
                "-f",
                "mpegts",
            ],
            Fixture::CoverArtM4a => vec![
                "-f",
                "lavfi",
                "-i",
                "sine=duration=5",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:size=64x64:duration=1",
                "-map",
                "0",
                "-map",
                "1",
                "-frames:v",
                "1",
                "-c:a",
                "aac",
                "-c:v",
                "mjpeg",
                "-disposition:v",
                "attached_pic",
                "-f",
                "mp4",
            ],
            Fixture::PhotoMp4 => vec![
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:size=64x64",
                "-frames:v",
                "1",
                "-c:v",
                "mjpeg",
                "-f",
                "image2",
            ],
            Fixture::MislabelledTs => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=25:duration=30",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=30",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-g",
                "50",
                "-keyint_min",
                "50",
                "-sc_threshold",
                "0",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                "-f",
                "mpegts",
            ],
            Fixture::OddDimsMp4 => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=321x241:rate=25:duration=10",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-pix_fmt",
                "yuv444p",
                "-f",
                "mp4",
            ],
            Fixture::FakeMp4 => vec![],
            Fixture::CleanPortrait => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=1080x1920:rate=30:duration=10",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=10",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "10",
                "-pix_fmt",
                "yuv420p",
                "-colorspace",
                "bt709",
                "-color_primaries",
                "bt709",
                "-color_trc",
                "bt709",
                "-color_range",
                "tv",
                "-c:a",
                "aac",
                "-b:a",
                "128k",
                "-shortest",
                "-f",
                "mp4",
            ],
            Fixture::DegradedPortrait => vec![
                "-i",
                "{src}",
                "-vf",
                "scale=480:854:flags=bicubic",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "35",
                "-pix_fmt",
                "yuv420p",
                "-colorspace",
                "bt709",
                "-color_primaries",
                "bt709",
                "-color_trc",
                "bt709",
                "-color_range",
                "tv",
                "-c:a",
                "copy",
                "-f",
                "mp4",
            ],
            Fixture::DegradedPortrait360 => vec![
                "-i",
                "{src}",
                "-vf",
                "scale=360:640:flags=bicubic",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "35",
                "-pix_fmt",
                "yuv420p",
                "-colorspace",
                "bt709",
                "-color_primaries",
                "bt709",
                "-color_trc",
                "bt709",
                "-color_range",
                "tv",
                "-c:a",
                "copy",
                "-f",
                "mp4",
            ],
            Fixture::Portrait480Long => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=480x854:rate=30:duration=60",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=60",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "28",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                "-f",
                "mp4",
            ],
            Fixture::ChaptersMp4 => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=25:duration=10",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=10",
                "-i",
                "{dir}/chapters.txt",
                "-map",
                "0:v",
                "-map",
                "1:a",
                "-map_metadata",
                "2",
                "-map_chapters",
                "2",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                "-f",
                "mp4",
            ],
            Fixture::MovingMarker => vec![
                "-f",
                "lavfi",
                "-i",
                "color=c=gray:size=1280x720:rate=30:duration=6",
                "-f",
                "lavfi",
                "-i",
                "color=c=0xFF00FF:size=80x80:rate=30:duration=10",
                "-f",
                "lavfi",
                "-i",
                "color=c=0x406080:size=1280x720:rate=30:duration=4",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=10",
                "-filter_complex",
                "[1:v]split[m1][m2];[0:v][m1]overlay=x=100+150*t:y=320:eof_action=pass[s1];[2:v][m2]overlay=x=1000:y=320:eof_action=pass[s2];[s1][s2]concat=n=2:v=1:a=0[v]",
                "-map",
                "[v]",
                "-map",
                "3:a",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "18",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                "-f",
                "mp4",
            ],
            Fixture::FarMarkers => vec![
                "-f",
                "lavfi",
                "-i",
                "color=c=gray:size=1280x720:rate=30:duration=6",
                "-f",
                "lavfi",
                "-i",
                "color=c=0xFF00FF:size=80x80:rate=30:duration=6",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=6",
                "-filter_complex",
                "[1:v]split[m1][m2];[0:v][m1]overlay=x=100:y=320[a];[a][m2]overlay=x=1100:y=320[v]",
                "-map",
                "[v]",
                "-map",
                "2:a",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "18",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                "-f",
                "mp4",
            ],
            Fixture::Rotated90 => {
                vec!["-display_rotation:v:0", "90", "-i", "{src}", "-map", "0", "-c", "copy", "-f", "mp4"]
            }
            Fixture::Anamorphic => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=720x576:rate=25:duration=5",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=5",
                "-vf",
                "setsar=64/45",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                "-f",
                "mp4",
            ],
            Fixture::Interlaced => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=720x576:rate=25:duration=5",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=5",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-flags",
                "+ildct+ilme",
                "-field_order",
                "tt",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                "-f",
                "mp4",
            ],
            Fixture::HdrTagged => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=640x360:rate=25:duration=3",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=3",
                "-c:v",
                "libx265",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p10le",
                "-color_primaries",
                "bt2020",
                "-color_trc",
                "smpte2084",
                "-colorspace",
                "bt2020nc",
                "-color_range",
                "tv",
                "-x265-params",
                "colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:log-level=error",
                "-tag:v",
                "hvc1",
                "-c:a",
                "aac",
                "-shortest",
                "-f",
                "mp4",
            ],
            Fixture::TinySource => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=25:duration=3",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=3",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                "-f",
                "mp4",
            ],
            Fixture::HighFps120 => vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=120:duration=5",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=5",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                "-f",
                "mp4",
            ],
        }
    }
}

/// ffmetadata for `ChaptersMp4`: a title and two chapters.
const CHAPTERS: &str = ";FFMETADATA1\ntitle=rmedia chapters fixture\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=4000\ntitle=Intro\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=4000\nEND=10000\ntitle=Main\n";

const SRT: &str = "1\n00:00:01,000 --> 00:00:04,000\nHello\n\n2\n00:00:10,000 --> 00:00:14,000\nWorld\n";

/// Path to ffmpeg (`RMEDIA_FFMPEG` or PATH).
pub fn ffmpeg() -> PathBuf {
    std::env::var_os("RMEDIA_FFMPEG").map(PathBuf::from).unwrap_or_else(|| "ffmpeg".into())
}

/// Path to ffprobe (`RMEDIA_FFPROBE` or PATH).
pub fn ffprobe() -> PathBuf {
    std::env::var_os("RMEDIA_FFPROBE").map(PathBuf::from).unwrap_or_else(|| "ffprobe".into())
}

/// Whether media tests can run. Panics when `RMEDIA_REQUIRE_FFMPEG=1` and ffmpeg is missing.
pub fn ffmpeg_available() -> bool {
    let ok = Command::new(ffmpeg()).arg("-version").output().is_ok_and(|o| o.status.success());
    if !ok && std::env::var_os("RMEDIA_REQUIRE_FFMPEG").is_some_and(|v| v == "1") {
        panic!("ffmpeg is required (RMEDIA_REQUIRE_FFMPEG=1) but was not found");
    }
    ok
}

/// Skips the calling test (returns early) when ffmpeg is unavailable.
#[macro_export]
macro_rules! require_ffmpeg {
    () => {
        if !$crate::ffmpeg_available() {
            eprintln!("skipping: ffmpeg not available");
            return;
        }
    };
}

/// Filters and encoders the engine at [`ffmpeg()`] provides (from `-filters` and `-encoders`),
/// read once per test process.
fn engine_features() -> &'static std::collections::BTreeSet<String> {
    static FEATURES: std::sync::OnceLock<std::collections::BTreeSet<String>> = std::sync::OnceLock::new();
    FEATURES.get_or_init(|| {
        let mut set = std::collections::BTreeSet::new();
        for flag in ["-filters", "-encoders"] {
            let Ok(out) = Command::new(ffmpeg()).args(["-hide_banner", flag]).output() else { continue };
            // Both listings print "<flags> <name> ..." rows after a header; take the name column.
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let mut cols = line.split_whitespace();
                if let (Some(flags), Some(name)) = (cols.next(), cols.next())
                    && flags.chars().all(|c| c.is_ascii_uppercase() || c == '.' || c == '|')
                    && flags.len() <= 6
                {
                    set.insert(name.to_owned());
                }
            }
        }
        set
    })
}

/// Whether the engine has ffmpeg plus every filter/encoder in `needs`. Tests for 002 call this
/// (through [`require_engine!`]) so they skip on older engines, such as the distro ffmpeg 6.1 in
/// the CI minimum-engine job. With `RMEDIA_REQUIRE_FFMPEG=1` a missing feature panics instead,
/// so the bundled-engine jobs can never skip silently.
pub fn require_engine(needs: &[&str]) -> bool {
    let strict = std::env::var_os("RMEDIA_REQUIRE_FFMPEG").is_some_and(|v| v == "1");
    if !ffmpeg_available() {
        return false;
    }
    let missing: Vec<&str> = needs.iter().copied().filter(|n| !engine_features().contains(*n)).collect();
    if missing.is_empty() {
        return true;
    }
    if strict && std::env::var_os("RMEDIA_ALLOW_MISSING_FEATURES").is_none_or(|v| v != "1") {
        panic!("engine lacks {missing:?} (RMEDIA_REQUIRE_FFMPEG=1)");
    }
    eprintln!("skipping: engine lacks {missing:?}");
    false
}

/// Skips the calling test (returns early) when the engine lacks any listed filter or encoder.
#[macro_export]
macro_rules! require_engine {
    ($($need:expr),+ $(,)?) => {
        if !$crate::require_engine(&[$($need),+]) {
            return;
        }
    };
}

fn fixtures_root() -> PathBuf {
    // CARGO_TARGET_TMPDIR is only set for integration tests; fall back to target/.
    let base = option_env!("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    base.join("rmedia-fixtures")
}

/// Returns the path to a fixture, generating it if needed.
/// Hashes a fixture's spec, including the specs of the fixtures it derives from, so changing a
/// parent regenerates its children.
fn spec_hash(kind: Fixture, h: &mut Sha256) {
    if let Some(parent) = kind.derived_from() {
        spec_hash(parent, h);
        h.update([1]);
    }
    h.update(kind.file_name());
    for a in kind.args() {
        h.update(a);
        h.update([0]);
    }
}

pub fn fixture(kind: Fixture) -> PathBuf {
    let src = kind.derived_from().map(fixture);
    let mut h = Sha256::new();
    spec_hash(kind, &mut h);
    let hash = format!("{:x}", h.finalize());
    let dir = fixtures_root().join(&hash[..16]);
    let target = dir.join(kind.file_name());
    if target.exists() {
        return target;
    }
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!(".{}.{}-{n}.tmp", kind.file_name(), std::process::id()));
    if kind == Fixture::FakeMp4 {
        std::fs::write(&tmp, "this is not a video\n").expect("write fake");
    } else {
        std::fs::write(dir.join("subs.srt"), SRT).expect("write srt");
        std::fs::write(dir.join("chapters.txt"), CHAPTERS).expect("write chapters");
        let dir_s = dir.to_string_lossy().into_owned();
        let src_s = src.as_deref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let args: Vec<String> =
            kind.args().iter().map(|a| a.replace("{dir}", &dir_s).replace("{src}", &src_s)).collect();
        let out = Command::new(ffmpeg())
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
            .args(&args)
            .arg(&tmp)
            .output()
            .expect("run ffmpeg for fixture");
        assert!(out.status.success(), "fixture {kind:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    // Another test may have finished first; either copy is identical.
    if std::fs::rename(&tmp, &target).is_err() {
        let _ = std::fs::remove_file(&tmp);
        assert!(target.exists(), "fixture {kind:?} could not be created");
    }
    target
}

/// Runs ffprobe and returns its JSON (`-show_format -show_streams`).
pub fn probe_json(path: &Path) -> serde_json::Value {
    let out = Command::new(ffprobe())
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(path)
        .output()
        .expect("run ffprobe");
    assert!(out.status.success(), "ffprobe failed on {}", path.display());
    serde_json::from_slice(&out.stdout).expect("ffprobe json")
}

/// Container duration in seconds.
pub fn duration(path: &Path) -> f64 {
    probe_json(path)["format"]["duration"].as_str().and_then(|s| s.parse().ok()).expect("duration")
}

/// `codec_type` of each stream, in order.
pub fn stream_types(path: &Path) -> Vec<String> {
    probe_json(path)["streams"]
        .as_array()
        .expect("streams")
        .iter()
        .map(|s| s["codec_type"].as_str().unwrap_or("").to_owned())
        .collect()
}

pub fn sha256(path: &Path) -> String {
    let data = std::fs::read(path).expect("read for sha256");
    format!("{:x}", Sha256::digest(&data))
}

/// PSNR (dB) between the first frame of `a` and the first frame of `b` at or after `b_time`
/// seconds (user time: ffmpeg subtracts the container start time). The reference is found by
/// decoding `b` sequentially and trimming on timestamps — no seeking, so it does not share
/// demuxer-seek imprecision with the code under test. `f64::INFINITY` = identical frames.
pub fn first_frame_psnr(a: &Path, b: &Path, b_time: f64) -> f64 {
    let graph = format!(
        "[0:v]trim=end_frame=1,setpts=PTS-STARTPTS[x];\
         [1:v]trim=start={b_time:.6},trim=end_frame=1,setpts=PTS-STARTPTS[y];[x][y]psnr"
    );
    let out = Command::new(ffmpeg())
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(a)
        .arg("-i")
        .arg(b)
        .args(["-lavfi", &graph, "-f", "null", "-"])
        .output()
        .expect("run psnr");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let avg = stderr
        .lines()
        .filter(|l| l.contains("PSNR"))
        .find_map(|l| l.split("average:").nth(1))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("no PSNR in output: {stderr}"));
    if avg == "inf" { f64::INFINITY } else { avg.parse().expect("psnr value") }
}

/// Displayed size of the first video stream: coded size with SAR and display rotation applied.
pub fn display_size(path: &Path) -> (u32, u32) {
    let j = probe_json(path);
    let v =
        j["streams"].as_array().expect("streams").iter().find(|s| s["codec_type"] == "video").expect("video");
    let (w, h) = (v["width"].as_u64().expect("width") as f64, v["height"].as_u64().expect("height") as f64);
    let sar = v["sample_aspect_ratio"]
        .as_str()
        .and_then(|s| s.split_once(':'))
        .and_then(|(a, b)| Some((a.parse::<f64>().ok()?, b.parse::<f64>().ok()?)))
        .filter(|(a, b)| *a > 0.0 && *b > 0.0)
        .map_or(1.0, |(a, b)| a / b);
    let rotation = v["side_data_list"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|d| d["rotation"].as_f64())
        .unwrap_or(0.0);
    let (w, h) = ((w * sar).round() as u32, h as u32);
    if (rotation.abs() as i64) % 180 == 90 { (h, w) } else { (w, h) }
}

/// VMAF (0–100) of `distorted` against `reference` (libvmaf through the engine). The reference
/// is scaled to the distorted size with bicubic, so both must show the same picture: their
/// displayed aspect ratios must match within 1 % (asserted).
pub fn vmaf(distorted: &Path, reference: &Path) -> f64 {
    let (dw, dh) = display_size(distorted);
    let (rw, rh) = display_size(reference);
    let (da, ra) = (f64::from(dw) / f64::from(dh), f64::from(rw) / f64::from(rh));
    assert!((da / ra - 1.0).abs() < 0.01, "vmaf needs matching aspect: {dw}x{dh} vs {rw}x{rh}");
    // libvmaf writes its log relative to the working directory: no path escaping in the graph.
    let work = tempfile_dir("vmaf");
    let graph = format!(
        "[0:v]setpts=PTS-STARTPTS,format=yuv420p[d];[1:v]scale={dw}:{dh}:flags=bicubic,setpts=PTS-STARTPTS,format=yuv420p[r];\
         [d][r]libvmaf=log_fmt=json:log_path=vmaf.json:n_threads=4"
    );
    let out = Command::new(ffmpeg())
        .current_dir(&work)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-i"])
        .arg(distorted)
        .arg("-i")
        .arg(reference)
        .args(["-lavfi", &graph, "-f", "null", "-"])
        .output()
        .expect("run libvmaf");
    assert!(out.status.success(), "libvmaf failed: {}", String::from_utf8_lossy(&out.stderr));
    let log: serde_json::Value =
        serde_json::from_slice(&std::fs::read(work.join("vmaf.json")).expect("vmaf log")).expect("vmaf json");
    let _ = std::fs::remove_dir_all(&work);
    log["pooled_metrics"]["vmaf"]["mean"].as_f64().expect("pooled vmaf mean")
}

/// A plain bicubic resize of `src` to `w`x`h`, encoded like the Reddit preset (x264 CRF 18,
/// BT.709 tags): the baseline that SC-002 compares the CPU tier against. Written to `out`.
pub fn bicubic_baseline(src: &Path, w: u32, h: u32, out: &Path) -> PathBuf {
    let status = Command::new(ffmpeg())
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-i"])
        .arg(src)
        .args(["-vf", &format!("scale={w}:{h}:flags=bicubic,setsar=1"), "-an"])
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "18",
            "-profile:v",
            "high",
            "-pix_fmt",
            "yuv420p",
        ])
        .args([
            "-colorspace",
            "bt709",
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-color_range",
            "tv",
        ])
        .arg(out)
        .status()
        .expect("run ffmpeg for baseline");
    assert!(status.success(), "bicubic baseline failed");
    out.to_path_buf()
}

/// A fresh, empty working directory under the fixtures root.
fn tempfile_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = fixtures_root().join(format!(".{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create work dir");
    dir
}

#[cfg(test)]
mod tests {
    #[test]
    fn require_engine_sees_common_filters_and_encoders() {
        if !super::ffmpeg_available() {
            return;
        }
        assert!(super::require_engine(&["scale", "overlay", "aac"]));
    }
}
