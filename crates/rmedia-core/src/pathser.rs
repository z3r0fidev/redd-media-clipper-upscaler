//! Lossy path serialization: JSON must never fail on file names that are not valid UTF-8
//! (legal on Linux). Invalid bytes become U+FFFD; such names cannot be round-tripped through JSON.

use std::path::{Path, PathBuf};

use serde::Serializer;

pub fn path<S: Serializer>(p: &Path, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&p.to_string_lossy())
}

pub fn opt_path<S: Serializer>(p: &Option<PathBuf>, s: S) -> Result<S::Ok, S::Error> {
    match p {
        Some(p) => s.serialize_some(&*p.to_string_lossy()),
        None => s.serialize_none(),
    }
}

pub fn paths<S: Serializer>(ps: &[PathBuf], s: S) -> Result<S::Ok, S::Error> {
    s.collect_seq(ps.iter().map(|p| p.to_string_lossy()))
}

/// Lossy string form for hand-built JSON documents.
pub fn lossy(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}
