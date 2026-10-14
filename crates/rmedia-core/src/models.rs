//! Model files (face detector, AI upscaler weights): where they are and whether they are present
//! (research R14). Models are never committed; `cargo xtask fetch-models` and the installers
//! place them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

/// Model files found for this run, by file name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ModelSet {
    pub found: BTreeMap<String, PathBuf>,
}

impl ModelSet {
    pub fn get(&self, name: &str) -> Option<&PathBuf> {
        self.found.get(name)
    }
}
