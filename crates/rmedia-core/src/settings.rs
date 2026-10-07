//! Per-user directories and persisted desktop settings (`settings.json` in the config folder).

use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::request::PartialOptions;

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("com", "OpenLogic", "rmedia")
}

/// Directory for the crash journal. `RMEDIA_STATE_DIR` overrides it (used by tests).
pub fn state_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("RMEDIA_STATE_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(p));
    }
    project_dirs().map(|d| d.state_dir().unwrap_or_else(|| d.data_local_dir()).to_path_buf())
}

/// Directory for preview proxies and thumbnails.
pub fn cache_dir() -> Option<PathBuf> {
    project_dirs().map(|d| d.cache_dir().join("preview"))
}

/// `RMEDIA_CONFIG_DIR` overrides the config folder (used by tests).
fn settings_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("RMEDIA_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(p).join("settings.json"));
    }
    project_dirs().map(|d| d.config_dir().join("settings.json"))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Explicit engine paths (empty = `RMEDIA_FFMPEG`/`RMEDIA_FFPROBE`, then bundled, then PATH).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub ffmpeg_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub ffprobe_path: Option<PathBuf>,
    /// Defaults for new clips (mode, output folder, naming, …).
    pub defaults: PartialOptions,
    /// Clips processed at once (empty = half the CPU cores, max 4).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub jobs: Option<u16>,
    pub proxy_cache_max_mb: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            ffmpeg_path: None,
            ffprobe_path: None,
            defaults: PartialOptions::default(),
            jobs: None,
            proxy_cache_max_mb: 5120,
        }
    }
}

impl Settings {
    /// Loads settings; a missing or unreadable file yields defaults.
    pub fn load() -> Settings {
        settings_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Saves atomically (temp file + rename).
    pub fn save(&self) -> std::io::Result<()> {
        let path = settings_path().ok_or_else(|| std::io::Error::other("no config directory"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_reject_unknown_fields() {
        let s = Settings { jobs: Some(2), ..Settings::default() };
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&text).unwrap(), s);
        assert_eq!(serde_json::from_str::<Settings>("{}").unwrap(), Settings::default());
        assert!(serde_json::from_str::<Settings>(r#"{"bogus":1}"#).is_err());
    }
}
