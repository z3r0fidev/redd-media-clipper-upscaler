//! Analysis cache keyed by source, span, parameters, algorithm and model (research R10).
//!
//! Entries are JSON files `<key>.json` in the cache folder, written via a temp file and a
//! rename, validated when read, and evicted oldest-first beyond [`MAX_ENTRIES`]. The cache
//! stores analyses only, never media.

use std::path::PathBuf;

use serde::Serialize;

use super::ReframeAnalysis;
use crate::upscale::Motion;

pub const MAX_ENTRIES: usize = 500;
/// Largest entry read (a 10-minute 60 fps path is well under 1 MB).
pub const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;

/// Everything an analysis depends on.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KeyInputs {
    pub source: String,
    pub size: u64,
    pub mtime_ns: i128,
    pub span: (f64, f64),
    pub chain: String,
    pub analysis_size: (u32, u32),
    pub crop: (u32, u32),
    pub fps: f64,
    pub motion: Motion,
    pub algo: u32,
    pub detector: String,
    pub ffmpeg: String,
}

/// SHA-256 (hex) of the canonical JSON of `k`.
pub fn key(k: &KeyInputs) -> String {
    let _ = k;
    todo!()
}

#[derive(Debug, Clone)]
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: PathBuf) -> Cache {
        Cache { dir }
    }

    /// The entry for `key`, if present, readable and valid.
    pub fn get(&self, key: &str) -> Option<ReframeAnalysis> {
        let _ = key;
        todo!()
    }

    /// Stores `a` under `a.key` and evicts the oldest entries beyond [`MAX_ENTRIES`].
    pub fn put(&self, a: &ReframeAnalysis) -> std::io::Result<()> {
        let _ = a;
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Span;
    use crate::time::Ts;
    use crate::upscale::reframe::{ALGO_VERSION, Shot, ShotKind};

    fn inputs() -> KeyInputs {
        KeyInputs {
            source: "/v/a.mp4".into(),
            size: 10,
            mtime_ns: 1,
            span: (0.0, 10.0),
            chain: "scale".into(),
            analysis_size: (576, 324),
            crop: (332, 720),
            fps: 30.0,
            motion: Motion::Normal,
            algo: ALGO_VERSION,
            detector: "yunet:abc".into(),
            ffmpeg: "n9".into(),
        }
    }

    fn analysis(key: &str) -> ReframeAnalysis {
        ReframeAnalysis {
            key: key.into(),
            algo_version: ALGO_VERSION,
            source_size: (1280, 720),
            crop: (332, 720),
            crop_y: 0,
            fps: 29.97,
            clip_start: Ts::from_secs_f64(1.5),
            shots: vec![Shot {
                span: Span { start: Ts::from_secs_f64(1.5), end: Ts::from_secs_f64(1.6) },
                kind: ShotKind::Tracked,
            }],
            fallback_ranges: vec![],
            path: vec![474, 476, 478],
            detections: 3,
            samples: 1,
        }
    }

    #[test]
    fn key_changes_with_every_input() {
        let base = key(&inputs());
        assert_eq!(base.len(), 64);
        let edits: Vec<Box<dyn Fn(&mut KeyInputs)>> = vec![
            Box::new(|k| k.mtime_ns += 1),
            Box::new(|k| k.size += 1),
            Box::new(|k| k.span.1 = 9.0),
            Box::new(|k| k.motion = Motion::Calm),
            Box::new(|k| k.algo += 1),
            Box::new(|k| k.detector = "yunet:def".into()),
            Box::new(|k| k.crop = (330, 720)),
            Box::new(|k| k.ffmpeg = "n10".into()),
        ];
        for (i, e) in edits.iter().enumerate() {
            let mut k = inputs();
            e(&mut k);
            assert_ne!(key(&k), base, "edit {i}");
        }
    }

    #[test]
    fn round_trip_eviction_and_corrupt_entries() {
        let d = tempfile::tempdir().unwrap();
        let c = Cache::new(d.path().join("reframe"));
        let a = analysis(&"a".repeat(64));
        c.put(&a).unwrap();
        assert_eq!(c.get(&a.key), Some(a.clone()), "bit-exact round trip");
        assert_eq!(c.get(&"b".repeat(64)), None);
        std::fs::write(d.path().join("reframe").join(format!("{}.json", a.key)), "{ broken").unwrap();
        assert_eq!(c.get(&a.key), None, "a corrupt entry is a miss");
        c.put(&a).unwrap();
        assert_eq!(c.get(&a.key), Some(a.clone()), "and is rewritten");
        let mut bad = analysis(&"c".repeat(64));
        bad.path = vec![475]; // odd x: invalid
        c.put(&bad).unwrap();
        assert_eq!(c.get(&bad.key), None, "entries that fail validation are misses");
    }

    #[test]
    fn evicts_the_oldest_beyond_the_limit() {
        let d = tempfile::tempdir().unwrap();
        let c = Cache::new(d.path().to_path_buf());
        for i in 0..=MAX_ENTRIES {
            c.put(&analysis(&format!("{i:064}"))).unwrap();
        }
        let n = std::fs::read_dir(d.path()).unwrap().count();
        assert_eq!(n, MAX_ENTRIES);
        assert_eq!(c.get(&format!("{:064}", 0)), None, "the oldest entry went");
        assert!(c.get(&format!("{MAX_ENTRIES:064}")).is_some());
    }
}
