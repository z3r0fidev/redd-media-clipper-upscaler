//! Model files (face detector, AI upscaler weights): where they are and whether they are present
//! (research R14). Models are never committed; `cargo xtask fetch-models` and the installers
//! place them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::settings::{Settings, cache_root};

/// The YuNet face detector used by smart reframe.
pub const YUNET: &str = "face_detection_yunet_2026may.onnx";

/// Model files found for this run, by file name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ModelSet {
    pub found: BTreeMap<String, PathBuf>,
}

impl ModelSet {
    /// Looks up each of `names` in the model search path.
    pub fn discover(settings: &Settings, names: &[&str]) -> ModelSet {
        let dirs = search_dirs(settings);
        let found = names.iter().filter_map(|n| locate_in(n, &dirs).map(|p| ((*n).to_owned(), p))).collect();
        ModelSet { found }
    }

    pub fn get(&self, name: &str) -> Option<&PathBuf> {
        self.found.get(name)
    }
}

/// Model folders in lookup order: `settings.models_dir`, then `models/` beside the executable
/// (installers put them there; a development build also looks in the workspace's `models/`),
/// then `<cache root>/models`.
pub fn search_dirs(settings: &Settings) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    dirs.extend(settings.models_dir.clone());
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        dirs.push(exe_dir.join("models"));
        // Development builds only: target/<profile>/ (and …/deps/ for tests) → workspace models/.
        for up in exe_dir.ancestors().skip(1).take(if cfg!(debug_assertions) { 3 } else { 0 }) {
            if up.join("Cargo.toml").is_file() && up.join("models").is_dir() {
                dirs.push(up.join("models"));
                break;
            }
        }
    }
    dirs.extend(cache_root().map(|c| c.join("models")));
    dirs
}

/// The first regular file named `name` in `dirs`. `name` must be a plain file name.
pub fn locate_in(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return None;
    }
    dirs.iter().map(|d| d.join(name)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_folder_with_the_file_wins() {
        let d = tempfile::tempdir().unwrap();
        let (a, b, c) = (d.path().join("a"), d.path().join("b"), d.path().join("c"));
        for dir in [&a, &b, &c] {
            std::fs::create_dir(dir).unwrap();
        }
        std::fs::write(b.join("m.onnx"), b"x").unwrap();
        std::fs::write(c.join("m.onnx"), b"x").unwrap();
        std::fs::create_dir(a.join("dir.onnx")).unwrap();
        let dirs = vec![a.clone(), b.clone(), c.clone()];
        assert_eq!(locate_in("m.onnx", &dirs), Some(b.join("m.onnx")));
        assert_eq!(locate_in("dir.onnx", &dirs), None, "directories are not model files");
        assert_eq!(locate_in("missing.onnx", &dirs), None);
        assert_eq!(locate_in("../b/m.onnx", &dirs), None, "names cannot escape the folders");
    }

    #[test]
    fn settings_folder_comes_first() {
        let s = Settings { models_dir: Some(PathBuf::from("/explicit")), ..Settings::default() };
        assert_eq!(search_dirs(&s).first(), Some(&PathBuf::from("/explicit")));
    }
}
