//! Per-user directories. Settings persistence arrives with the desktop app (US5).

use std::path::PathBuf;

use directories::ProjectDirs;

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
    project_dirs().map(|d| d.cache_dir().to_path_buf())
}
