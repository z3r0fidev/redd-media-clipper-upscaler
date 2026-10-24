//! T005 (003): engine probes. They check behaviour of FFmpeg that GIF export relies on
//! (research R1, R3, R4, R15), so they pass from the start and guard against engine regressions:
//! (a) the two-pass palette pipeline gives byte-identical output across runs;
//! (b) 15 fps is written as 6/7 cs delays, and 20/25/50 fps as exact 5/4/2 cs delays;
//! (c) `-loop 0` writes NETSCAPE repeat 0, `-loop 2` repeat 2, `-loop -1` no block;
//! (d) per-window `-ss` inputs joined with `concat`, each with its own `sendcmd` file whose times
//!     are relative to its window, move each window's `crop@trk` at the expected frames;
//! (e) the overlay-on-black flatten of an alpha source is opaque and black where it was clear;
//! (f) with cwd = the job's scratch folder, a relative `p-<key>.png` palette is written and read.
//!
//! Exact delays and byte identity are asserted on the bundled engine only (the minimum engine
//! checks validity and the ≥ 2 cs rule).

use std::path::{Path, PathBuf};
use std::process::Command;

use rmedia_testkit::{
    Fixture, bundled_engine, ffmpeg, fixture, gif_delays, gif_loop, gif_opaque, require_engine,
};

/// A scratch dir whose path needs quoting everywhere (spaces, non-ASCII).
fn scratch(test: &str) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join(format!("job dir é {test}"));
    std::fs::create_dir(&dir).unwrap();
    (root, dir)
}

fn ffmpeg_in(cwd: &Path, args: &[&str]) {
    let o = Command::new(ffmpeg())
        .current_dir(cwd)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-n"])
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "ffmpeg {args:?} failed: {}", String::from_utf8_lossy(&o.stderr));
}

/// The two-pass encode of research R1: palette from the final-size frames, then paletteuse.
fn two_pass(cwd: &Path, src: &Path, chain: &str, fps: u32, out: &str, extra: &[&str]) -> PathBuf {
    let src = src.to_str().unwrap();
    let chain = format!("{chain},fps={fps},scale=640:-2:flags=lanczos+accurate_rnd+full_chroma_int");
    ffmpeg_in(
        cwd,
        &[
            "-i",
            src,
            "-vf",
            &format!("{chain},palettegen=max_colors=256:stats_mode=diff:reserve_transparent=1"),
            "-frames:v",
            "1",
            "-update",
            "1",
            "p-test.png",
        ],
    );
    let lavfi = format!("[0:v]{chain}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle");
    let mut args =
        vec!["-i", src, "-i", "p-test.png", "-lavfi", &lavfi, "-an", "-f", "gif", "-fflags", "+bitexact"];
    args.extend(extra);
    args.push(out);
    ffmpeg_in(cwd, &args);
    let _ = std::fs::remove_file(cwd.join("p-test.png"));
    cwd.join(out)
}

#[test]
fn a_two_pass_palette_output_is_byte_identical_across_runs() {
    require_engine!("palettegen", "paletteuse");
    let src = fixture(Fixture::Land6);
    let (_r1, d1) = scratch("a1");
    let (_r2, d2) = scratch("a2");
    let one = two_pass(&d1, &src, "trim=duration=4", 15, "out.gif", &[]);
    let two = two_pass(&d2, &src, "trim=duration=4", 15, "out.gif", &[]);
    assert!(std::fs::read(&one).unwrap().starts_with(b"GIF89a"));
    if bundled_engine() {
        assert_eq!(std::fs::read(&one).unwrap(), std::fs::read(&two).unwrap(), "two runs differ");
    }
}

#[test]
fn b_frame_rates_map_to_centisecond_delays() {
    require_engine!("palettegen", "paletteuse");
    let src = fixture(Fixture::Land6);
    let (_root, dir) = scratch("b");
    for (fps, allowed) in [(15, &[6, 7][..]), (20, &[5][..]), (25, &[4][..]), (50, &[2][..])] {
        let out = two_pass(&dir, &src, "trim=duration=2", fps, &format!("f{fps}.gif"), &[]);
        let delays = gif_delays(&out);
        assert!(delays.iter().all(|d| *d >= 2), "{fps} fps: a delay under 2 cs in {delays:?}");
        let total: u32 = delays.iter().sum();
        assert!((195..=205).contains(&total), "{fps} fps lasts {total} cs, not 200");
        if bundled_engine() {
            let mut seen: Vec<u32> = delays.clone();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen, allowed, "{fps} fps delays");
            assert_eq!(delays.len(), 2 * fps as usize, "{fps} fps frame count");
            let predicted = rmedia_core::gif::timing::delays((fps, 1), delays.len());
            assert_eq!(delays, predicted, "{fps} fps: timing::delays matches the muxer");
        }
    }
}

#[test]
fn c_loop_option_writes_the_netscape_repeat_count() {
    require_engine!("palettegen", "paletteuse");
    let src = fixture(Fixture::Narrow);
    let (_root, dir) = scratch("c");
    for (arg, want) in [("0", Some(0)), ("2", Some(2)), ("-1", None)] {
        let out = two_pass(&dir, &src, "trim=duration=0.5", 10, &format!("l{arg}.gif"), &["-loop", arg]);
        assert_eq!(gif_loop(&out), want, "-loop {arg}");
    }
}

/// Mean RGB of frame `n` of a decoded file.
fn colour_of_frame(file: &Path, n: u32) -> [u8; 3] {
    let o = Command::new(ffmpeg())
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-i"])
        .arg(file)
        .args(["-vf", &format!("select=eq(n\\,{n}),scale=1:1:flags=area"), "-frames:v", "1"])
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .unwrap();
    assert!(o.status.success() && o.stdout.len() == 3, "frame {n}: {}", String::from_utf8_lossy(&o.stderr));
    [o.stdout[0], o.stdout[1], o.stdout[2]]
}

#[test]
fn d_per_window_sendcmd_files_move_each_windows_crop() {
    require_engine!("sendcmd", "crop", "concat", "libx264");
    let (_root, dir) = scratch("d");
    // Left half red, right half blue, 6 s at 30 fps.
    ffmpeg_in(
        &dir,
        &[
            "-f",
            "lavfi",
            "-i",
            "color=red:s=640x360:r=30:d=6,drawbox=x=320:y=0:w=320:h=360:color=blue:t=fill",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-qp",
            "0",
            "-pix_fmt",
            "yuv444p",
            "src.mp4",
        ],
    );
    // Window k starts at 1 + 2k s; only window 1 moves its crop (to the blue half) 0.5 s in. An
    // empty sendcmd file is an error ("No commands were specified"), so every window's file
    // starts with the crop's starting position.
    std::fs::write(dir.join("w0.cmd"), "0.0 [enter] crop@trk0 x 40;\n").unwrap();
    std::fs::write(dir.join("w1.cmd"), "0.0 [enter] crop@trk1 x 40;\n0.5 [enter] crop@trk1 x 400;\n")
        .unwrap();
    std::fs::write(dir.join("w2.cmd"), "0.0 [enter] crop@trk2 x 40;\n").unwrap();
    let chain = |k: u32| {
        format!("[{k}:v]setpts=PTS-STARTPTS,sendcmd=f=w{k}.cmd,crop@trk{k}=w=160:h=360:x=40:y=0[w{k}]")
    };
    let graph = format!("{};{};{};[w0][w1][w2]concat=n=3:v=1:a=0[v]", chain(0), chain(1), chain(2));
    let mut args: Vec<String> = Vec::new();
    for k in 0..3 {
        args.extend([
            "-ss".into(),
            format!("{}", 1 + 2 * k),
            "-t".into(),
            "1".into(),
            "-i".into(),
            "src.mp4".into(),
        ]);
    }
    args.extend(["-filter_complex".into(), graph, "-map".into(), "[v]".into()]);
    args.extend(
        ["-c:v", "libx264", "-preset", "ultrafast", "-qp", "0", "-pix_fmt", "yuv444p", "out.mp4"]
            .map(String::from),
    );
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    ffmpeg_in(&dir, &refs);
    let out = dir.join("out.mp4");
    let red = |c: [u8; 3]| c[0] > 200 && c[2] < 60;
    let blue = |c: [u8; 3]| c[2] > 200 && c[0] < 60;
    assert!(red(colour_of_frame(&out, 10)), "window 0 stays on the red half");
    assert!(red(colour_of_frame(&out, 35)), "window 1 before its command");
    assert!(blue(colour_of_frame(&out, 55)), "window 1 after its command (0.5 s in)");
    assert!(red(colour_of_frame(&out, 75)), "window 2 is not moved by window 1's file");
}

#[test]
fn e_overlay_on_black_flatten_is_opaque() {
    require_engine!("palettegen", "paletteuse", "overlay", "drawbox");
    let src = fixture(Fixture::Alpha);
    let (_root, dir) = scratch("e");
    let flatten = "split[fa][fb];[fa]format=yuv420p,drawbox=c=black:t=fill[fk];[fk][fb]overlay=format=auto";
    let out = two_pass_complex(&dir, &src, flatten);
    assert!(gif_opaque(&out), "no pixel is transparent");
    // The margin was transparent red: it must come out black, not red.
    let o = Command::new(ffmpeg())
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-i"])
        .arg(&out)
        .args([
            "-vf",
            "crop=8:8:0:0,scale=1:1:flags=area",
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-",
        ])
        .output()
        .unwrap();
    assert!(o.stdout.len() == 3 && o.stdout.iter().all(|c| *c < 24), "corner is {:?}", o.stdout);
}

/// [`two_pass`] for a chain with internal labels (it must run in `-filter_complex`).
fn two_pass_complex(cwd: &Path, src: &Path, flatten: &str) -> PathBuf {
    let src = src.to_str().unwrap();
    let chain = format!("[0:v]{flatten},scale=320:-2:flags=lanczos+accurate_rnd+full_chroma_int");
    ffmpeg_in(
        cwd,
        &[
            "-i",
            src,
            "-filter_complex",
            &format!("{chain},palettegen=reserve_transparent=1[p]"),
            "-map",
            "[p]",
            "-frames:v",
            "1",
            "-update",
            "1",
            "p-alpha.png",
        ],
    );
    let lavfi = format!("{chain}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle[g]");
    ffmpeg_in(
        cwd,
        &["-i", src, "-i", "p-alpha.png", "-filter_complex", &lavfi, "-map", "[g]", "-f", "gif", "alpha.gif"],
    );
    cwd.join("alpha.gif")
}

#[test]
fn f_relative_palette_names_resolve_in_the_scratch_folder() {
    require_engine!("palettegen", "paletteuse");
    let src = fixture(Fixture::Narrow);
    let (_root, dir) = scratch("f");
    let out = two_pass(&dir, &src, "trim=duration=0.5", 10, "rel.gif", &[]);
    assert!(out.is_file(), "the encode found p-test.png relative to its working directory");
    assert!(!dir.join("p-test.png").exists(), "removed after use");
}
