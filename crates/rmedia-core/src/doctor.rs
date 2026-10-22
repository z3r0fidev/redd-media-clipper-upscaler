//! Engine diagnostics (FR-026) and capability detection (Principle VI).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::plan::Caps;
use crate::plan::streams::Container;
use crate::tools::{self, LocateOptions, Tool, Tools};
use crate::upscale::capability::{
    CapabilityStore, CheckMode, Demotion, GpuDevice, ProbeEnv, TierStatus, ensure,
};

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
    /// Quality tiers, when the capability check ran (`rmedia doctor`, `upscale`).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub tiers: Option<TierStatus>,
    /// Vulkan devices found, with why unusable ones are not used.
    pub gpus: Vec<GpuDevice>,
    /// Model files: `ok` or `missing`.
    pub models: BTreeMap<String, String>,
    /// When the capability record was made, and tiers that failed since.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub capability: Option<CapabilitySummary>,
    /// User preset files that could not be used (`file: message`).
    pub preset_problems: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CapabilitySummary {
    pub checked_at: String,
    pub fingerprint: String,
    pub demoted: Vec<Demotion>,
}

/// Locates the engine and inspects its capabilities (no GPU check: see [`check_capability`]).
/// Never fails; problems are reported.
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
        tiers: None,
        gpus: Vec::new(),
        models: BTreeMap::new(),
        capability: None,
        preset_problems: Vec::new(),
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
    report.models.insert(
        crate::models::YUNET.to_owned(),
        if caps.models.get(crate::models::YUNET).is_some() { "ok" } else { "missing" }.to_owned(),
    );
    // The AI tier needs every scale of its model and the worker beside rmedia.
    let ai_models =
        crate::upscale::ai::locate_models(&crate::models::search_dirs(&crate::settings::Settings::load()));
    report.models.insert(
        format!("{} (x2, x3, x4)", crate::upscale::ai::MODEL),
        if ai_models.is_some() { "ok" } else { "missing" }.to_owned(),
    );
    report.models.insert(
        crate::upscale::ai::WORKER.to_owned(),
        if crate::upscale::ai::locate_worker().is_some() { "ok" } else { "missing" }.to_owned(),
    );
    let presets = crate::preset::PresetRegistry::load(crate::settings::config_dir().as_deref());
    report.preset_problems =
        presets.problems().iter().map(|p| format!("{}: {}", p.file.display(), p.message)).collect();
    report.ok = true;
    (report, Some((tools, caps)))
}

/// Adds the GPU/AI capability check (research R8) to a [`run`] result: the tiers, GPUs and
/// record go into `report`, and the record into the engine's `Caps` so planning can choose a
/// tier. Without an engine or a state folder there is nothing to check.
pub async fn check_capability(
    report: &mut DoctorReport,
    engine: Option<&mut (Tools, Caps)>,
    mode: CheckMode,
    cancel: &tokio_util::sync::CancellationToken,
) {
    let (Some((tools, caps)), Some(state)) = (engine, crate::settings::state_dir()) else { return };
    let presets = crate::preset::PresetRegistry::load(crate::settings::config_dir().as_deref());
    let preset = presets.get(crate::preset::DEFAULT_PRESET).cloned().expect("the default preset is built in");
    let store = CapabilityStore::in_state_dir(&state);
    let env = ProbeEnv { ai: caps.ai.clone(), ..ProbeEnv::new(preset) };
    let Some(record) = ensure(tools, &store, &env, mode, cancel).await else { return };
    report.tiers = Some(record.tiers.clone());
    report.gpus = record.devices.clone();
    report.capability = Some(CapabilitySummary {
        checked_at: record.checked_at.clone(),
        fingerprint: record.fingerprint.clone(),
        demoted: record.demoted.clone(),
    });
    caps.capability = Some(record);
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
    let settings = crate::settings::Settings::load();
    let models = crate::models::ModelSet::discover(&settings, &[crate::models::YUNET]);
    let face =
        models.get(crate::models::YUNET).cloned().map(crate::upscale::reframe::detect::DetectorSpec::Yunet);
    Ok(Caps {
        encoders: parse_encoders(&list("-encoders")?),
        filters: parse_filters(&list("-filters")?),
        face,
        models,
        reframe_cache: crate::settings::cache_root().map(|c| c.join("reframe")),
        ai: crate::upscale::ai::AiEngine::discover(&settings),
        ..Caps::default()
    })
}

/// Parses `ffmpeg -filters` rows such as ` .S zscale   V->V   Apply resizing…`. Rows are
/// recognised by their shape, not by position: the flags column is 2 characters (9.x) or 3
/// (6.x), and only 9.x prints a `------` line after the legend (`T.. = …`, `A = …`).
pub fn parse_filters(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let flags = parts.next()?;
            let name = parts.next()?;
            let io = parts.next()?;
            ((2..=3).contains(&flags.len())
                && flags.chars().all(|c| matches!(c, '.' | 'T' | 'S' | 'C'))
                && io.contains("->"))
            .then(|| name.to_owned())
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

    /// Real `ffmpeg -hide_banner -filters` heads: 9.0.2 (2 flag columns, a `------` line) and
    /// the distro 6.1.1 of Ubuntu 24.04 (3 flag columns, no `------` line). The legend lines
    /// (`T.. = …`, `A = …`, `| = …`) are not filters.
    const FILTERS_N9: &str = "Filters:\n  T.. = Timeline support\n  .S. = Slice threading\n  A = Audio input/output\n  V = Video input/output\n  N = Dynamic number and/or type of input/output\n  | = Source or sink filter\n  ------\n TS aap               AA->A      Apply Affine Projection algorithm to first audio stream.\n TS cas               V->V       Contrast Adaptive Sharpen.\n .. nullsrc           |->V       Null video source, return unprocessed video frames.\n";
    const FILTERS_N6: &str = "Filters:\n  T.. = Timeline support\n  .S. = Slice threading\n  ..C = Command support\n  A = Audio input/output\n  V = Video input/output\n  N = Dynamic number and/or type of input/output\n  | = Source or sink filter\n ... abench            A->A       Benchmark part of a filtergraph.\n TSC overlay           VV->V      Overlay a video source on top of the input.\n .SC zscale            V->V       Apply resizing, colorspace and bit depth conversion.\n ... nullsrc           |->V       Null video source, return unprocessed video frames.\n";

    #[test]
    fn parses_filter_lists_of_ffmpeg_9_and_6() {
        let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>();
        assert_eq!(parse_filters(FILTERS_N9), set(&["aap", "cas", "nullsrc"]));
        assert_eq!(parse_filters(FILTERS_N6), set(&["abench", "nullsrc", "overlay", "zscale"]));
        assert!(parse_filters("ffmpeg: unrecognized option '-filters'\n").is_empty());
    }

    #[test]
    fn parses_the_encoder_list_of_ffmpeg_6() {
        // 6.1.1: the same layout as 9.0 (a `------` line, 6 flag columns).
        let n6 = "Encoders:\n V..... = Video\n A..... = Audio\n .....D = Supports direct rendering method 1\n ------\n V....D a64multi             Multicolor charset for Commodore 64 (codec a64_multi)\n V....D libx265              libx265 H.265 / HEVC (codec hevc)\n A....D aac                  AAC (Advanced Audio Coding)\n";
        let set = parse_encoders(n6);
        assert_eq!(set.len(), 3, "{set:?}");
        assert!(set.contains("libx265") && set.contains("aac"));
    }
}
