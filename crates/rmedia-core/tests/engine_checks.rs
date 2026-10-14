//! T018 (002): engine probes. They check behaviour of the bundled FFmpeg that the upscale filter
//! graph relies on (research R5), so they pass from the start and guard against engine
//! regressions rather than failing first:
//! (a) a `sendcmd` file toggles `overlay@fb`'s timeline `enable` at the commanded times;
//! (b) the same holds on a VFR source behind a leading `trim,setpts` (times are clip-relative);
//! (c) `crop@trk x` commands move the crop window;
//! (d) a relative `f=reframe.cmd` resolves against the process working directory, even when that
//!     directory's path has spaces and non-ASCII characters.

use std::path::{Path, PathBuf};
use std::process::Command;

use rmedia_testkit::{Fixture, ffmpeg, fixture, require_engine};

/// A scratch dir whose path needs quoting everywhere (spaces, non-ASCII).
fn scratch(test: &str) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join(format!("job dir é {test}"));
    std::fs::create_dir(&dir).unwrap();
    (root, dir)
}

/// Renders `graph` (inputs given as lavfi or file args) to `out.mp4` inside `cwd`.
fn render(cwd: &Path, inputs: &[&str], graph: &str, cmd_file: &str) -> PathBuf {
    std::fs::write(cwd.join("reframe.cmd"), cmd_file).unwrap();
    let out = cwd.join("out.mp4");
    let o = Command::new(ffmpeg())
        .current_dir(cwd)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y"])
        .args(inputs)
        .args(["-filter_complex", graph, "-map", "[v]", "-c:v", "libx264", "-preset", "ultrafast"])
        .args(["-pix_fmt", "yuv444p", "-qp", "0"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(o.status.success(), "ffmpeg failed: {}", String::from_utf8_lossy(&o.stderr));
    out
}

/// Mean RGB of the frame at `t` seconds.
fn colour_at(file: &Path, t: f64) -> [u8; 3] {
    let o = Command::new(ffmpeg())
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-ss", &format!("{t}"), "-i"])
        .arg(file)
        .args(["-frames:v", "1", "-vf", "scale=1:1:flags=area", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .unwrap();
    assert!(
        o.status.success() && o.stdout.len() == 3,
        "sample at {t}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    [o.stdout[0], o.stdout[1], o.stdout[2]]
}

fn is_blue([r, g, b]: [u8; 3]) -> bool {
    b > 200 && r < 60 && g < 60
}
fn is_red([r, g, b]: [u8; 3]) -> bool {
    r > 200 && g < 60 && b < 60
}
fn is_green([r, g, b]: [u8; 3]) -> bool {
    g > 100 && r < 60 && b < 60
}

const TOGGLE: &str = "2.0 [enter] overlay@fb enable 1;\n4.0 [enter] overlay@fb enable 0;\n";

#[test]
fn a_sendcmd_toggles_overlay_enable() {
    require_engine!("sendcmd", "overlay", "libx264");
    let (_root, dir) = scratch("a");
    let out = render(
        &dir,
        &[
            "-f",
            "lavfi",
            "-i",
            "color=red:s=64x64:r=30:d=6",
            "-f",
            "lavfi",
            "-i",
            "color=blue:s=64x64:r=30:d=6",
        ],
        "[0:v]sendcmd=f=reframe.cmd,null[a];[a][1:v]overlay@fb=0:0:enable=0[v]",
        TOGGLE,
    );
    assert!(is_red(colour_at(&out, 1.0)), "before the range");
    assert!(is_blue(colour_at(&out, 3.0)), "inside the range");
    assert!(is_red(colour_at(&out, 5.0)), "after the range");
}

#[test]
fn b_sendcmd_times_are_clip_relative_on_vfr_behind_trim() {
    require_engine!("sendcmd", "overlay", "trim", "libx264");
    let (_root, dir) = scratch("b");
    let src = fixture(Fixture::VfrMp4);
    let src = src.to_str().unwrap();
    let out = render(
        &dir,
        &["-i", src, "-f", "lavfi", "-i", "color=blue:s=64x64:r=30:d=30"],
        "[0:v]trim=start=6:end=14,setpts=PTS-STARTPTS,scale=64:64,format=rgb24,sendcmd=f=reframe.cmd[a];\
         [a][1:v]overlay@fb=0:0:enable=0:shortest=1[v]",
        TOGGLE,
    );
    // Times are relative to the trimmed clip: 2 s and 4 s after source time 6 s.
    assert!(!is_blue(colour_at(&out, 1.0)), "before the range");
    assert!(is_blue(colour_at(&out, 3.0)), "inside the range");
    assert!(!is_blue(colour_at(&out, 5.0)), "after the range");
}

#[test]
fn c_sendcmd_moves_the_tracking_crop() {
    require_engine!("sendcmd", "crop", "hstack", "libx264");
    let (_root, dir) = scratch("c");
    let out = render(
        &dir,
        &[
            "-f",
            "lavfi",
            "-i",
            "color=red:s=64x64:r=30:d=4",
            "-f",
            "lavfi",
            "-i",
            "color=green:s=64x64:r=30:d=4",
        ],
        "[0:v][1:v]hstack,sendcmd=f=reframe.cmd,crop@trk=w=64:h=64:x=0:y=0[v]",
        "2.0 crop@trk x 64;\n",
    );
    assert!(is_red(colour_at(&out, 1.0)), "left half first");
    assert!(is_green(colour_at(&out, 3.0)), "right half after the command");
}
