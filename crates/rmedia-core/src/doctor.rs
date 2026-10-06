//! Engine diagnostics (FR-026) and capability detection (Principle VI).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::plan::Caps;
use crate::plan::streams::Container;
use crate::tools::{self, LocateOptions, Tool, Tools};

/// Encoders the doctor report always lists.
pub const KEY_ENCODERS: [&str; 4] = ["libx264", "aac", "libvpx-vp9", "libopus"];

const ACCURATE_CONTAINERS: [(&str, Container); 6] = [
    ("mp4", Container::Mp4),
    ("m4v", Container::M4v),
    ("mov", Container::Mov),
    ("mkv", Container::Mkv),
    ("ts", Container::MpegTs),
    ("webm", Container::Webm),
];

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub schema: u32,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub ffmpeg: Option<Tool>,
    pub ffprobe: Option<Tool>,
    pub version: Option<String>,
    pub encoders: BTreeMap<String, bool>,
    pub accurate_containers: Vec<String>,
    pub ok: bool,
    pub problems: Vec<String>,
}

/// Locates the engine and inspects its capabilities. Never fails; problems are reported.
pub fn run(opts: &LocateOptions) -> (DoctorReport, Option<(Tools, Caps)>) {
    let mut report = DoctorReport {
        schema: 1,
        kind: "doctor",
        ffmpeg: None,
        ffprobe: None,
        version: None,
        encoders: BTreeMap::new(),
        accurate_containers: Vec::new(),
        ok: false,
        problems: Vec::new(),
    };
    let tools = match tools::locate(opts) {
        Ok(t) => t,
        Err(e) => {
            report.problems.push(e.to_string());
            return (report, None);
        }
    };
    report.ffmpeg = Some(tools.ffmpeg.clone());
    report.ffprobe = Some(tools.ffprobe.clone());
    report.version = Some(tools.version.text().to_owned());
    let caps = match caps(&tools) {
        Ok(c) => c,
        Err(e) => {
            report.problems.push(e.to_string());
            return (report, None);
        }
    };
    for enc in KEY_ENCODERS {
        report.encoders.insert(enc.to_owned(), caps.encoders.contains(enc));
    }
    for (ext, c) in ACCURATE_CONTAINERS {
        if let Some(p) = c.encode_profile()
            && caps.encoders.contains(p.video_encoder)
            && caps.encoders.contains(p.audio_encoder)
        {
            report.accurate_containers.push(ext.to_owned());
        }
    }
    if report.accurate_containers.is_empty() {
        report.problems.push("no encoders for accurate mode (fast mode still works)".into());
    }
    report.ok = true;
    (report, Some((tools, caps)))
}

/// Reads the encoder list of the ffmpeg build.
pub fn caps(tools: &Tools) -> Result<Caps> {
    let out = tools::quiet_command(&tools.ffmpeg.path)
        .args(["-hide_banner", "-encoders"])
        .output()
        .map_err(|e| Error::EngineMissing(format!("cannot run ffmpeg: {e}")))?;
    Ok(Caps { encoders: parse_encoders(&String::from_utf8_lossy(&out.stdout)) })
}

/// Parses `ffmpeg -encoders` lines such as ` V....D libx264   libx264 H.264 …`.
pub fn parse_encoders(text: &str) -> BTreeSet<String> {
    text.lines()
        .skip_while(|l| !l.trim_start().starts_with("------"))
        .skip(1)
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let flags = parts.next()?;
            (flags.len() == 6).then(|| parts.next().map(str::to_owned)).flatten()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_encoder_list() {
        let text = "Encoders:\n V..... = Video\n ------\n V....D libx264              libx264 H.264\n A....D aac                  AAC\n";
        let set = parse_encoders(text);
        assert!(set.contains("libx264") && set.contains("aac"));
        assert_eq!(set.len(), 2);
    }
}
