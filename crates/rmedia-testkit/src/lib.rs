//! Test fixtures generated at test time with FFmpeg's lavfi sources (no binary fixtures are
//! committed — Constitution III), plus ffprobe-based assertions.
//!
//! Fixtures are generated once into `target/rmedia-fixtures/<hash>/` (temp + rename, safe for
//! parallel test processes). Set `RMEDIA_REQUIRE_FFMPEG=1` (CI) to fail instead of skipping
//! when ffmpeg is missing.

use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

mod engine;
mod fixtures;
mod gif_tools;
mod inspect;
mod quality;

pub use engine::*;
pub use fixtures::*;
pub use gif_tools::*;
pub use inspect::*;
pub use quality::*;

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
    let hash = hex(&h.finalize());
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

/// The two halves of [`res_change`]: (size, file).
const RES_CHANGE_PARTS: [(&str, &str); 2] = [("1280x720", "a.ts"), ("960x540", "b.ts")];

/// 25 fps H.264, 8 s, 2 s closed GOPs, no audio, whose picture changes from 1280x720 to 960x540
/// at 4 s (feature 004, T032): two encodes joined with the concat demuxer in MPEG-TS, then
/// remuxed to MP4. Built in steps, so it is not a [`Fixture`]. (With audio, the join would leave
/// a gap in the video where the first half's audio runs longer.)
pub fn res_change() -> PathBuf {
    let parts: Vec<Vec<&'static str>> = RES_CHANGE_PARTS
        .iter()
        .map(|(size, _)| smart_args(size, 4, "veryfast", "bframes=3", false))
        .collect();
    let mut h = Sha256::new();
    h.update("res-change.mp4");
    for a in parts.iter().flatten() {
        h.update(a);
        h.update([0]);
    }
    let dir = fixtures_root().join(&hex(&h.finalize())[..16]);
    let target = dir.join("res-change.mp4");
    if target.exists() {
        return target;
    }
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    let run = |args: &[&str], out: &Path| {
        let o = Command::new(ffmpeg())
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
            .args(args)
            .arg(out)
            .output()
            .expect("run ffmpeg for fixture");
        assert!(o.status.success(), "res_change fixture failed: {}", String::from_utf8_lossy(&o.stderr));
    };
    let tag = format!("{}", std::process::id());
    let mut list = String::new();
    for ((_, name), args) in RES_CHANGE_PARTS.iter().zip(&parts) {
        let mut a = args.clone();
        let fmt = a.len() - 1;
        a[fmt] = "mpegts";
        let part = dir.join(format!("{tag}-{name}"));
        run(&a, &part);
        list.push_str(&format!("file '{}'\n", part.file_name().unwrap().to_string_lossy()));
    }
    let list_path = dir.join(format!("{tag}-list.txt"));
    std::fs::write(&list_path, list).expect("write concat list");
    let tmp = dir.join(format!(".res-change.mp4.{tag}.tmp"));
    let list_s = list_path.to_string_lossy().into_owned();
    run(&["-f", "concat", "-safe", "0", "-i", &list_s, "-map", "0", "-c", "copy", "-f", "mp4"], &tmp);
    for (_, name) in RES_CHANGE_PARTS {
        let _ = std::fs::remove_file(dir.join(format!("{tag}-{name}")));
    }
    let _ = std::fs::remove_file(&list_path);
    // Another test may have finished first; either copy is identical.
    if std::fs::rename(&tmp, &target).is_err() {
        let _ = std::fs::remove_file(&tmp);
        assert!(target.exists(), "fixture res_change could not be created");
    }
    target
}

/// Skips the calling test (returns early) when an optional GIF engine is unavailable:
/// `require_tool!(gifsicle)` or `require_tool!(gifski)`.
#[macro_export]
macro_rules! require_tool {
    (gifsicle) => {
        if !$crate::require_gif_tool($crate::GifTool::Gifsicle) {
            return;
        }
    };
    (gifski) => {
        if !$crate::require_gif_tool($crate::GifTool::Gifski) {
            return;
        }
    };
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
    use super::*;

    /// A GIF made by the engine under test (two frames at 10 fps, so 10 cs each).
    fn tiny_gif(loop_arg: &str, dir: &Path) -> PathBuf {
        let out = dir.join(format!("loop{loop_arg}.gif"));
        let status = Command::new(ffmpeg())
            .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc2=size=64x48:rate=10:duration=0.3")
            .args(["-loop", loop_arg, "-f", "gif"])
            .arg(&out)
            .status()
            .expect("run ffmpeg");
        assert!(status.success());
        out
    }

    #[test]
    fn gif_helpers_read_delays_sizes_loop_and_opacity() {
        if !ffmpeg_available() {
            return;
        }
        let dir = tempfile_dir("gif-helpers");
        let forever = tiny_gif("0", &dir);
        assert_eq!(gif_delays(&forever), vec![10, 10, 10]);
        let sizes = gif_packet_sizes(&forever);
        assert_eq!(sizes.len(), 3);
        assert!(sizes.iter().all(|s| *s > 0));
        assert_eq!(gif_loop(&forever), Some(0));
        assert_eq!(gif_loop(&tiny_gif("2", &dir)), Some(2));
        assert_eq!(gif_loop(&tiny_gif("-1", &dir)), None, "one play has no NETSCAPE block");
        assert!(gif_opaque(&forever));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn netscape_block_parsing() {
        let mut gif = b"GIF89a....!\xff\x0bNETSCAPE2.0".to_vec();
        gif.extend([3, 1, 0x34, 0x12, 0]);
        assert_eq!(netscape_repeat(&gif), Some(0x1234));
        assert_eq!(netscape_repeat(b"GIF89a no block"), None);
        assert_eq!(netscape_repeat(b"NETSCAPE2.0\x03"), None, "truncated");
    }

    #[test]
    fn faststart_compares_top_level_box_order() {
        let boxed = |kind: &[u8; 4], len: u32| {
            let mut b = len.to_be_bytes().to_vec();
            b.extend(kind);
            b.resize(len as usize, 0);
            b
        };
        let fast = [boxed(b"ftyp", 16), boxed(b"moov", 24), boxed(b"mdat", 12)].concat();
        let slow = [boxed(b"ftyp", 16), boxed(b"mdat", 12), boxed(b"moov", 24)].concat();
        assert_eq!(top_level_boxes(&fast), vec![*b"ftyp", *b"moov", *b"mdat"]);
        let dir = tempfile_dir("faststart");
        std::fs::write(dir.join("fast.mp4"), &fast).unwrap();
        std::fs::write(dir.join("slow.mp4"), &slow).unwrap();
        assert!(mp4_faststart(&dir.join("fast.mp4")));
        assert!(!mp4_faststart(&dir.join("slow.mp4")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gif_tools_come_from_their_environment_variables() {
        // A variable naming nothing runnable is "not available", never a panic by itself.
        assert_eq!(GifTool::Gifsicle.env(), "RMEDIA_GIFSICLE");
        assert_eq!(GifTool::Gifski.env(), "RMEDIA_GIFSKI");
        if let Some(p) = gif_tool(GifTool::Gifsicle) {
            assert!(p.exists());
        }
    }

    #[test]
    fn gif_fixtures_have_the_documented_shape() {
        if !ffmpeg_available() {
            return;
        }
        for (kind, size, secs) in [
            (Fixture::Land6, (1280, 720), 6.0),
            (Fixture::Narrow, (640, 360), 4.0),
            (Fixture::Static6, (1280, 720), 6.0),
            (Fixture::FlatUi, (1280, 720), 6.0),
            (Fixture::HiddenBusy10, (1280, 720), 10.0),
        ] {
            let p = fixture(kind);
            assert_eq!(display_size(&p), size, "{kind:?}");
            assert!((duration(&p) - secs).abs() < 0.1, "{kind:?}");
        }
        let alpha = probe_json(&fixture(Fixture::Alpha));
        assert_eq!(alpha["streams"][0]["pix_fmt"], "yuva420p");
    }

    #[test]
    fn smart_fixtures_have_two_second_closed_gops() {
        if !require_engine(&["libx264"]) {
            return;
        }
        for kind in [
            Fixture::SmartMp4,
            Fixture::SmartMov,
            Fixture::SmartMkv,
            Fixture::SmartTs,
            Fixture::SmartBaseline,
        ] {
            let p = fixture(kind);
            let packets = video_packets(&p);
            let zero = packets.iter().filter_map(|p| p.pts).fold(f64::INFINITY, f64::min);
            let keys: Vec<f64> =
                packets.iter().filter(|p| p.key).filter_map(|p| p.pts).map(|t| t - zero).collect();
            assert!(keys.len() >= 10, "{kind:?}: {keys:?}");
            assert!(
                keys.iter().enumerate().all(|(i, k)| (k - 2.0 * i as f64).abs() < 0.001),
                "{kind:?}: {keys:?}"
            );
            assert!(decode_errors(&p).is_empty(), "{kind:?}");
        }
        assert_eq!(frame_count(&fixture(Fixture::SmartBaseline)), 500);
        assert_eq!(stream_types(&fixture(Fixture::SmartBaseline)), ["video"]);
        let (v, a) = stream_starts(&fixture(Fixture::SmartMp4));
        assert!(v.abs() < 0.001 && a.abs() < 0.03, "{v} {a}");
        let ten = probe_json(&fixture(Fixture::High10));
        assert_eq!(ten["streams"][0]["pix_fmt"], "yuv420p10le");
    }

    #[test]
    fn framemd5_hashes_frames_without_timestamps() {
        if !require_engine(&["libx264"]) {
            return;
        }
        let p = fixture(Fixture::SmartBaseline);
        let a = framemd5(&p, 4.0, 3);
        assert_eq!(a.len(), 3);
        assert_eq!(framemd5(&p, 4.04, 2), a[1..], "an exact seek starts on the next frame");
    }

    #[test]
    fn require_engine_sees_common_filters_and_encoders() {
        if !super::ffmpeg_available() {
            return;
        }
        assert!(super::require_engine(&["scale", "overlay", "aac"]));
    }
}
