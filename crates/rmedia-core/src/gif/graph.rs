//! The GIF encode argv (research R1, R7): the two-pass palette pipeline on top of the frame
//! chain (refactor B1), and the gifsicle post-pass. Every file name reaching a filtergraph or
//! gifsicle is a generated, relative one: the tools run in the job's scratch folder.

use std::ffi::OsString;

use super::plan::{GifSpec, Settings};
use super::target::StatsMode;
use crate::plan::ffargs::{common_args, input_parts};

/// The built-in engine's encode output, in the scratch folder.
pub const ENCODED: &str = "enc.gif";
/// gifsicle's output, in the scratch folder.
pub const OPTIMISED: &str = "opt.gif";

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

/// Pass 1: the palette of the final-size frames (FR-002) into `palette` (relative).
pub fn palette_args(spec: &GifSpec, s: &Settings, palette: &str) -> Vec<OsString> {
    let (source, range) = input_parts(&spec.cut);
    let mut a = common_args();
    a.extend(source);
    a.extend(range);
    let graph = format!(
        "{},palettegen=max_colors={}:stats_mode={}:reserve_transparent=1[p]",
        spec.frame.chain(s),
        s.colours,
        stats(s.stats)
    );
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
}
