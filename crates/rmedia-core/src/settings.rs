//! Per-user directories and persisted desktop settings (`settings.json` in the config folder).

use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::gif::PartialGif;
use crate::request::PartialOptions;
use crate::upscale::PartialUpscale;

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

/// Root of rmedia's caches: `preview/` (proxies, thumbnails), `reframe/` (smart-reframe
/// analyses) and `models/` (downloaded model files). `RMEDIA_CACHE_DIR` overrides it (tests).
pub fn cache_root() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("RMEDIA_CACHE_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(p));
    }
    project_dirs().map(|d| d.cache_dir().to_path_buf())
}

/// Directory for preview proxies and thumbnails.
pub fn cache_dir() -> Option<PathBuf> {
    cache_root().map(|d| d.join("preview"))
}

/// The config folder (`settings.json`, `presets/`). `RMEDIA_CONFIG_DIR` overrides it (tests).
pub fn config_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("RMEDIA_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(p));
    }
    project_dirs().map(|d| d.config_dir().to_path_buf())
}

fn settings_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("settings.json"))
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
    /// Folder holding model files (face detector, AI upscaler); searched first (research R14).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub models_dir: Option<PathBuf>,
    /// The desktop upscale view's last-used choices. Separate from `defaults`, so clip runs
    /// never turn upscaling on by accident.
    pub upscale_defaults: PartialUpscale,
    /// Explicit paths of the optional GIF engines (empty = `RMEDIA_GIFSICLE`/`RMEDIA_GIFSKI`,
    /// then bundled, then PATH; feature 003).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub gifsicle_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub gifski_path: Option<PathBuf>,
    /// The desktop GIF view's last-used choices (like `upscale_defaults`).
    pub gif_defaults: PartialGif,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            ffmpeg_path: None,
            ffprobe_path: None,
            defaults: PartialOptions::default(),
            jobs: None,
            proxy_cache_max_mb: 5120,
            models_dir: None,
            upscale_defaults: PartialUpscale::default(),
            gifsicle_path: None,
            gifski_path: None,
            gif_defaults: PartialGif::default(),
        }
    }
}

impl Settings {
    /// Loads settings; a missing or unreadable file yields defaults.
    pub fn load() -> Settings {
        settings_path().map(|p| Settings::load_from(&p)).unwrap_or_default()
    }

    fn load_from(path: &std::path::Path) -> Settings {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Settings::default();
        };
        serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("{} is not valid ({e}); using the default settings", path.display());
            Settings::default()
        })
    }

    /// Saves atomically (temp file + rename).
    pub fn save(&self) -> std::io::Result<()> {
        let path = settings_path().ok_or_else(|| std::io::Error::other("no config directory"))?;
        self.save_to(&path)
    }

    /// A file that does not parse (a hand edit with a typo, or one written by a newer version)
    /// is renamed to `settings.invalid-<unix time>.json` first, so the save does not lose it.
    fn save_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if let Ok(text) = std::fs::read_to_string(path)
            && serde_json::from_str::<Settings>(&text).is_err()
        {
            let secs =
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let kept = path.with_file_name(format!("settings.invalid-{secs}.json"));
            std::fs::rename(path, &kept)?;
            tracing::warn!("{} was not valid; kept it as {}", path.display(), kept.display());
        }
        // A unique temporary name: two saves at once (the app and the CLI) must not rename
        // each other's file away.
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let tmp = path.with_extension(format!("json.{}-{nanos}.tmp", std::process::id()));
        let written = std::fs::write(&tmp, serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)
            .and_then(|()| std::fs::rename(&tmp, path));
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        written
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

    fn backups(dir: &std::path::Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("settings.invalid-"))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn an_invalid_settings_file_is_kept_before_a_save_replaces_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let typo = "{\"jobs\": 2,}\n";
        std::fs::write(&path, typo).unwrap();
        // It loads as the defaults...
        assert_eq!(Settings::load_from(&path), Settings::default());
        // ...and saving keeps the user's file instead of overwriting it.
        Settings { jobs: Some(3), ..Settings::default() }.save_to(&path).unwrap();
        let kept = backups(dir.path());
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(std::fs::read_to_string(&kept[0]).unwrap(), typo);
        assert_eq!(Settings::load_from(&path).jobs, Some(3));
        // A later save over the now-valid file makes no further copy.
        Settings::default().save_to(&path).unwrap();
        assert_eq!(backups(dir.path()).len(), 1);
    }

    #[test]
    fn a_settings_file_from_a_newer_version_is_kept_too() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"jobs":2,"a_field_from_later":true}"#).unwrap();
        Settings::default().save_to(&path).unwrap();
        assert_eq!(backups(dir.path()).len(), 1);
    }

    #[test]
    fn settings_from_before_002_still_load() {
        let old = r#"{"defaults":{"mode":"accurate"},"jobs":2,"proxy_cache_max_mb":100}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(
            (s.jobs, s.models_dir.as_ref(), &s.upscale_defaults),
            (Some(2), None, &PartialUpscale::default())
        );
        let new = r#"{"models_dir":"/m","upscale_defaults":{"preset":"reddit-916","tier":"cpu"}}"#;
        let s: Settings = serde_json::from_str(new).unwrap();
        assert_eq!(s.upscale_defaults.preset.as_deref(), Some("reddit-916"));
        assert!(serde_json::from_str::<Settings>(r#"{"upscale_defaults":{"bogus":1}}"#).is_err());
    }

    #[test]
    fn settings_from_before_003_still_load() {
        let old = r#"{"jobs":2,"models_dir":"/m","upscale_defaults":{"preset":"duo"}}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(
            (&s.gifsicle_path, &s.gifski_path, &s.gif_defaults),
            (&None, &None, &PartialGif::default())
        );
        let new = r#"{"gifsicle_path":"/opt/g","gif_defaults":{"target":"reddit-comment","loop":2}}"#;
        let s: Settings = serde_json::from_str(new).unwrap();
        assert_eq!(s.gifsicle_path.as_deref(), Some(std::path::Path::new("/opt/g")));
        assert_eq!(s.gif_defaults.target.as_deref(), Some("reddit-comment"));
        assert!(serde_json::from_str::<Settings>(r#"{"gif_defaults":{"bogus":1}}"#).is_err());
        let text = serde_json::to_string(&Settings::default()).unwrap();
        assert!(!text.contains("gifsicle_path"), "unset engine paths are not written: {text}");
    }
}
