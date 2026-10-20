//! The worker's argv (contracts/rmedia-sr-protocol.md › Invocation). Hand-parsed so every usage
//! error is a JSON line and exit code 2, like every other failure.

use std::path::PathBuf;

/// Largest input side accepted; the core never sends frames bigger than its sources (≤ 8K).
pub const MAX_SIDE: u32 = 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Model {
    /// `<DIR>/<name>`: the files `<name>.param` and `<name>.bin`.
    Files(PathBuf),
    /// Test only: CPU nearest-neighbour ×s, no GPU (`builtin:nearest`).
    Nearest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fp16 {
    Auto,
    On,
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub model: Model,
    pub scale: u32,
    /// `None` = `--gpu none` (builtin models only).
    pub gpu: Option<u32>,
    pub width: u32,
    pub height: u32,
    /// Input tile side; 0 = chosen from the device's memory.
    pub tile: u32,
    pub fp16: Fp16,
    pub threads: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Run(Job),
    SelfTest(Job, u32),
    ListGpus,
    Version,
}

pub fn parse(args: &[String]) -> Result<Mode, String> {
    match args.first().map(String::as_str) {
        Some("--version") if args.len() == 1 => return Ok(Mode::Version),
        Some("--list-gpus") if args.len() == 1 => return Ok(Mode::ListGpus),
        None => return Err("no arguments (see --version, --list-gpus, --selftest)".into()),
        _ => {}
    }
    let (mut model, mut scale, mut gpu, mut width, mut height) = (None, None, None, None, None);
    let (mut tile, mut fp16, mut threads, mut frames, mut selftest) = (0, Fp16::Auto, 4, None, false);
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        if flag == "--selftest" {
            selftest = true;
            continue;
        }
        let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--model" => {
                model = Some(match value.strip_prefix("builtin:") {
                    Some("nearest") => Model::Nearest,
                    Some(other) => return Err(format!("unknown builtin model {other:?}")),
                    None if value.is_empty() => return Err("--model is empty".into()),
                    None => Model::Files(PathBuf::from(value)),
                })
            }
            "--scale" => scale = Some(number(flag, value, 2, 4)?),
            "--gpu" => gpu = Some(if value == "none" { None } else { Some(number(flag, value, 0, 63)?) }),
            "--width" => width = Some(number(flag, value, 1, MAX_SIDE)?),
            "--height" => height = Some(number(flag, value, 1, MAX_SIDE)?),
            "--tile" => tile = if value == "0" { 0 } else { number(flag, value, 32, 4096)? },
            "--threads" => threads = number(flag, value, 1, 64)?,
            "--frames" => frames = Some(number(flag, value, 1, 64)?),
            "--fp16" => {
                fp16 = match value.as_str() {
                    "auto" => Fp16::Auto,
                    "on" => Fp16::On,
                    "off" => Fp16::Off,
                    _ => return Err(format!("--fp16 takes auto, on or off, not {value:?}")),
                }
            }
            _ => return Err(format!("unknown argument {flag:?}")),
        }
    }
    let need = |name: &str| format!("--{name} is required");
    let job = Job {
        model: model.ok_or_else(|| need("model"))?,
        scale: scale.ok_or_else(|| need("scale"))?,
        gpu: gpu.ok_or_else(|| need("gpu"))?,
        width: width.ok_or_else(|| need("width"))?,
        height: height.ok_or_else(|| need("height"))?,
        tile,
        fp16,
        threads,
    };
    if job.gpu.is_none() && job.model != Model::Nearest {
        return Err("--gpu none is only accepted with a builtin model".into());
    }
    match (selftest, frames) {
        (true, f) => Ok(Mode::SelfTest(job, f.unwrap_or(8))),
        (false, Some(_)) => Err("--frames is only for --selftest".into()),
        (false, None) => Ok(Mode::Run(job)),
    }
}

fn number(flag: &str, value: &str, min: u32, max: u32) -> Result<u32, String> {
    value
        .parse::<u32>()
        .ok()
        .filter(|n| (min..=max).contains(n))
        .ok_or_else(|| format!("{flag} takes a whole number from {min} to {max}, not {value:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn parses_a_job_and_rejects_bad_ones() {
        let m = parse(&args("--model /m/x2 --scale 2 --gpu 1 --width 640 --height 360 --tile 0 --fp16 off"))
            .unwrap();
        let Mode::Run(j) = m else { panic!("{m:?}") };
        assert_eq!((j.gpu, j.width, j.height, j.fp16), (Some(1), 640, 360, Fp16::Off));
        assert_eq!(j.model, Model::Files("/m/x2".into()));
        for bad in [
            "--model /m --scale 5 --gpu 0 --width 2 --height 2",
            "--model /m --scale 2 --gpu none --width 2 --height 2",
            "--model builtin:nearest --scale 2 --gpu none --width 0 --height 2",
            "--model builtin:nearest --scale 2 --gpu none --width 2",
            "--model builtin:x --scale 2 --gpu none --width 2 --height 2",
            "--model builtin:nearest --scale 2 --gpu none --width 2 --height 2 --frames 3",
            "--model builtin:nearest --scale 2 --gpu none --width 2 --height 2 --bogus 1",
            "--version extra",
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad}");
        }
        assert_eq!(parse(&args("--version")).unwrap(), Mode::Version);
        let st = parse(&args("--selftest --model builtin:nearest --scale 3 --gpu none --width 8 --height 8"))
            .unwrap();
        assert!(matches!(st, Mode::SelfTest(_, 8)));
    }
}
