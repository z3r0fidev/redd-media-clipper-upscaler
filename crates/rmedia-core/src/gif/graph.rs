//! The GIF encode argv (research R1, R7): the two-pass palette pipeline on top of the frame
//! chain (refactor B1), the gifsicle post-pass, and the gifski pipe. Every file name reaching a
//! filtergraph, gifsicle or gifski is a generated, relative one: the tools run in the job's
//! scratch folder.

use std::ffi::OsString;

use super::LoopCount;
use super::plan::{GifSpec, Settings};
use super::target::{GifskiParams, StatsMode};
use crate::plan::ffargs::{common_args, input_parts};

/// The encode output (the built-in engine's or gifski's), in the scratch folder.
pub const ENCODED: &str = "enc.gif";
/// gifsicle's output, in the scratch folder.
pub const OPTIMISED: &str = "opt.gif";
/// The gifski pipe's ffmpeg progress, in the scratch folder (its stdout carries the frames).
pub const PROGRESS: &str = "progress.log";

/// The frames gifski reads: full-range BT.709 4:4:4, which it is told with
/// `--y4m-color-override` (by default it guesses the matrix from the frame size, and the range
/// from the header's `XCOLORRANGE`).
const Y4M: &str =
    "scale=out_color_matrix=bt709:out_range=pc:flags=accurate_rnd+full_chroma_int,format=yuv444p";

fn os(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

/// The palette file for `s`: keyed by the chain and the palette settings, so search steps that
/// only change dithering or lossy reuse it.
pub fn palette_name(spec: &GifSpec, s: &Settings) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(spec.frame.chain(s));
    h.update(format!("|{}|{:?}", s.colours, s.stats));
    format!("p-{}.png", &crate::hex(&h.finalize())[..16])
}

fn stats(m: StatsMode) -> &'static str {
    match m {
        StatsMode::Full => "full",
        StatsMode::Diff => "diff",
    }
}

/// The `palettegen` filter of `s`.
pub fn palette_settings(s: &Settings) -> String {
    format!("palettegen=max_colors={}:stats_mode={}:reserve_transparent=1", s.colours, stats(s.stats))
}

/// Pass 1: the palette of the final-size frames (FR-002) into `palette` (relative).
pub fn palette_args(spec: &GifSpec, s: &Settings, palette: &str) -> Vec<OsString> {
    let (source, range) = input_parts(&spec.cut);
    let mut a = common_args();
    a.extend(source);
    a.extend(range);
    let graph = format!("{},{}[p]", spec.frame.chain(s), palette_settings(s));
    a.extend(os(&[
        "-filter_complex",
        &graph,
        "-map",
        "[p]",
        "-frames:v",
        "1",
        "-update",
        "1",
        "-f",
        "image2",
        palette,
    ]));
    a
}

/// Pass 2: the frames mapped onto the palette, written as `output` (relative). The range
/// options follow the palette input, so they apply to the output.
pub fn encode_args(spec: &GifSpec, s: &Settings, palette: &str, output: &str) -> Vec<OsString> {
    let (source, range) = input_parts(&spec.cut);
    let mut a = common_args();
    a.extend(source);
    a.extend(os(&["-i", palette]));
    a.extend(range);
    let graph = format!(
        "{}[x];[x][1:v]paletteuse={}:diff_mode=rectangle[g]",
        spec.frame.chain(s),
        s.dither.paletteuse()
    );
    a.extend(os(&["-filter_complex", &graph, "-map", "[g]", "-an", "-sn", "-dn", "-map_metadata", "-1"]));
    a.extend(os(&[
        "-f",
        "gif",
        "-fflags",
        "+bitexact",
        "-loop",
        &spec.gif.loop_count.repeat_arg().to_string(),
        output,
    ]));
    a
}

/// The start of an ffmpeg argv whose stdout carries frames: `common_args` with the progress
/// written to `progress` (relative), or not at all.
pub fn pipe_head(progress: Option<&str>) -> Vec<OsString> {
    let mut a = os(&["-hide_banner", "-nostdin", "-loglevel", "warning"]);
    if let Some(p) = progress {
        a.extend(os(&["-progress", &format!("file:{p}"), "-nostats", "-stats_period", "0.25"]));
    }
    a
}

/// `chain` (an open chain) as the y4m frames of the gifski pipe, labelled `[y]`.
pub fn y4m_graph(chain: &str) -> String {
    format!("{chain},{Y4M}[y]")
}

/// The output half of the gifski pipe's ffmpeg: `[y]` as y4m on stdout.
pub fn y4m_tail() -> Vec<OsString> {
    os(&["-map", "[y]", "-an", "-sn", "-dn", "-map_metadata", "-1", "-f", "yuv4mpegpipe", "pipe:1"])
}

/// The gifski pipe's ffmpeg: the job's frames as y4m, progress to `progress` (relative).
pub fn y4m_args(spec: &GifSpec, s: &Settings, progress: &str) -> Vec<OsString> {
    let (source, range) = input_parts(&spec.cut);
    let mut a = pipe_head(Some(progress));
    a.extend(source);
    a.extend(range);
    a.extend(os(&["-filter_complex", &y4m_graph(&spec.frame.chain(s))]));
    a.extend(y4m_tail());
    a
}

/// gifski reading y4m from stdin into `output` (relative). `--fps` is rounded up: gifski drops a
/// frame whenever the stream runs ahead of it, so it must never be below the stream's rate.
/// `--repeat -1` still writes a loop block (gif 0.13 writes count 0, which loops forever): the
/// executor removes it ([`strip_loop_block`]).
pub fn gifski_args(rate: (u32, u32), p: GifskiParams, loop_count: LoopCount, output: &str) -> Vec<OsString> {
    let fps = (f64::from(rate.0) / f64::from(rate.1) * 1000.0).ceil() / 1000.0;
    let mut a = os(&["--quiet", "--fps", &fps.to_string(), "--quality", &p.quality.to_string()]);
    if let Some(m) = p.motion_quality {
        a.extend(os(&["--motion-quality", &m.to_string()]));
    }
    if let Some(l) = p.lossy_quality {
        a.extend(os(&["--lossy-quality", &l.to_string()]));
    }
    a.extend(os(&[
        "--repeat",
        &loop_count.repeat_arg().to_string(),
        "--y4m-color-override",
        "bt709",
        "-o",
        output,
        "-",
    ]));
    a
}

/// The loop block gifski writes first, after the header and the screen descriptor (it writes no
/// global palette), when told to play once: NETSCAPE2.0, repeat 0.
const ONCE_BLOCK: [u8; 19] = *b"\x21\xff\x0bNETSCAPE2.0\x03\x01\x00\x00\x00";
const SCREEN_END: usize = 13;

/// Removes gifski's loop block from `gif` when it is exactly where and what gifski writes;
/// otherwise leaves the file as it is (verification then reports the loop count).
pub fn strip_loop_block(gif: &mut Vec<u8>) -> bool {
    let at_start = gif.get(10).is_some_and(|flags| flags & 0x80 == 0)
        && gif.get(SCREEN_END..SCREEN_END + ONCE_BLOCK.len()) == Some(&ONCE_BLOCK[..]);
    if at_start {
        gif.drain(SCREEN_END..SCREEN_END + ONCE_BLOCK.len());
    }
    at_start
}

/// The gifsicle post-pass: `-O3`, optionally lossy, from `input` to `output` (both relative;
/// `--` ends the options, so no name is read as one).
pub fn gifsicle_args(lossy: u16, input: &str, output: &str) -> Vec<OsString> {
    let mut a = os(&["--no-warnings", "-O3"]);
    if lossy > 0 {
        a.push(format!("--lossy={lossy}").into());
    }
    a.extend(os(&["-o", output, "--", input]));
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gifsicle_takes_generated_names_after_the_end_of_options() {
        let a: Vec<String> = gifsicle_args(40, "enc.gif", "opt.gif")
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert_eq!(a, ["--no-warnings", "-O3", "--lossy=40", "-o", "opt.gif", "--", "enc.gif"]);
        let plain: Vec<String> =
            gifsicle_args(0, "a.gif", "b.gif").iter().map(|s| s.to_string_lossy().into_owned()).collect();
        assert!(!plain.iter().any(|s| s.starts_with("--lossy")));
    }

    fn strings(a: &[OsString]) -> Vec<String> {
        a.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn gifski_reads_bt709_y4m_from_stdin_at_a_rate_rounded_up() {
        let p = GifskiParams { quality: 90, motion_quality: None, lossy_quality: Some(80) };
        let a = strings(&gifski_args((30000, 1001), p, LoopCount::Plays(3), "enc.gif"));
        assert_eq!(
            a,
            [
                "--quiet",
                "--fps",
                "29.971",
                "--quality",
                "90",
                "--lossy-quality",
                "80",
                "--repeat",
                "2",
                "--y4m-color-override",
                "bt709",
                "-o",
                "enc.gif",
                "-"
            ]
        );
        let once = strings(&gifski_args((10, 1), p, LoopCount::Plays(1), "s.gif"));
        assert_eq!(&once[1..3], ["--fps", "10"]);
        assert!(once.windows(2).any(|w| w == ["--repeat", "-1"]), "{once:?}");
    }

    #[test]
    fn the_pipe_head_sends_progress_to_a_file_never_stdout() {
        let a = strings(&pipe_head(Some(PROGRESS)));
        assert!(a.windows(2).any(|w| w == ["-progress", "file:progress.log"]), "{a:?}");
        assert!(!a.iter().any(|s| s == "pipe:1"));
        assert!(!strings(&pipe_head(None)).iter().any(|s| s == "-progress"));
        assert_eq!(strings(&y4m_tail()).last().map(String::as_str), Some("pipe:1"));
        assert!(y4m_graph("[0:v]fps=10").ends_with("format=yuv444p[y]"));
    }

    fn gif_with(block: &[u8]) -> Vec<u8> {
        let mut g = b"GIF89a\x40\x00\x30\x00\x00\x00\x00".to_vec();
        g.extend_from_slice(block);
        g.extend_from_slice(b"\x21\xfe\x07gif.ski\x00\x3b");
        g
    }

    #[test]
    fn only_gifskis_play_once_block_is_removed() {
        let mut once = gif_with(&ONCE_BLOCK);
        assert!(strip_loop_block(&mut once));
        assert_eq!(once, gif_with(&[]));
        let forever_twice: &[u8] = b"\x21\xff\x0bNETSCAPE2.0\x03\x01\x02\x00\x00";
        let mut repeat = gif_with(forever_twice);
        assert!(!strip_loop_block(&mut repeat), "a real repeat count stays");
        let mut palette = gif_with(&ONCE_BLOCK);
        palette[10] = 0x80;
        assert!(!strip_loop_block(&mut palette), "with a global palette the block is elsewhere");
    }
}
