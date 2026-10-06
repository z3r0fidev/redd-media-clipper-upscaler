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
        }
    }
}

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

fn fixtures_root() -> PathBuf {
    // CARGO_TARGET_TMPDIR is only set for integration tests; fall back to target/.
    let base = option_env!("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    base.join("rmedia-fixtures")
}

/// Returns the path to a fixture, generating it if needed.
pub fn fixture(kind: Fixture) -> PathBuf {
    let mut h = Sha256::new();
    h.update(kind.file_name());
    for a in kind.args() {
        h.update(a);
        h.update([0]);
    }
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
        let dir_s = dir.to_string_lossy().into_owned();
        let args: Vec<String> = kind.args().iter().map(|a| a.replace("{dir}", &dir_s)).collect();
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
