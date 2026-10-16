//! Engine diagnostics (FR-026) and capability detection (Principle VI).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::plan::Caps;
use crate::plan::streams::Container;
use crate::tools::{self, LocateOptions, Tool, Tools};

/// Encoders the doctor report always lists.
pub const KEY_ENCODERS: [&str; 5] = ["libx264", "libx265", "aac", "libvpx-vp9", "libopus"];

/// Filters upscaling needs (research R3–R6); listed by the doctor report.
pub const KEY_FILTERS: [&str; 10] =
    ["zscale", "cas", "hqdn3d", "bwdif", "gblur", "tonemap", "sendcmd", "scdet", "libplacebo", "libvmaf"];

const ACCURATE_CONTAINERS: [(&str, Container); 6] = [
    ("mp4", Container::Mp4),
    ("m4v", Container::M4v),
    ("mov", Container::Mov),
    ("mkv", Container::Mkv),
    ("ts", Container::MpegTs),
    ("webm", Container::Webm),
];

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DoctorReport {
    pub schema: u32,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub ffmpeg: Option<Tool>,
    pub ffprobe: Option<Tool>,
    pub version: Option<String>,
    pub encoders: BTreeMap<String, bool>,
    pub filters: BTreeMap<String, bool>,
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
        filters: BTreeMap::new(),
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
    for f in KEY_FILTERS {
        report.filters.insert(f.to_owned(), caps.filters.contains(f));
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

/// Reads the encoder and filter lists of the ffmpeg build.
pub fn caps(tools: &Tools) -> Result<Caps> {
    let list = |flag: &str| {
        tools::quiet_command(&tools.ffmpeg.path)
            .args(["-hide_banner", flag])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .map_err(|e| Error::EngineMissing(format!("cannot run ffmpeg: {e}")))
    };
    let models =
        crate::models::ModelSet::discover(&crate::settings::Settings::load(), &[crate::models::YUNET]);
    let face =
        models.get(crate::models::YUNET).cloned().map(crate::upscale::reframe::detect::DetectorSpec::Yunet);
    Ok(Caps {
        encoders: parse_encoders(&list("-encoders")?),
        filters: parse_filters(&list("-filters")?),
        face,
        models,
        reframe_cache: crate::settings::cache_root().map(|c| c.join("reframe")),
        ..Caps::default()
    })
}

/// Parses `ffmpeg -filters` lines such as ` .S zscale   V->V   Apply resizing…` (the flags
/// column is 2 or 3 characters depending on the ffmpeg version).
pub fn parse_filters(text: &str) -> BTreeSet<String> {
    text.lines()
        .skip_while(|l| !l.trim_start().starts_with("------"))
        .skip(1)
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let _flags = parts.next()?;
            let name = parts.next()?;
            parts.next().is_some_and(|io| io.contains("->")).then(|| name.to_owned())
        })
        .collect()
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

    #[test]
    fn parses_filter_list_old_and_new_flag_columns() {
        let n9 = "Filters:\n  T.. = Timeline support\n  ------\n TS cas               V->V       Contrast Adaptive Sharpen.\n .. nullsrc           |->V       Null video source.\n";
        let n6 = "Filters:\n  ------\n .S. zscale            V->V       Apply resizing.\n TSC overlay           VV->V      Overlay a video.\n";
        assert_eq!(parse_filters(n9), BTreeSet::from(["cas".to_owned(), "nullsrc".to_owned()]));
        assert_eq!(parse_filters(n6), BTreeSet::from(["overlay".to_owned(), "zscale".to_owned()]));
        assert!(parse_filters("no separator\n TS cas V->V x\n").is_empty());
    }
}
