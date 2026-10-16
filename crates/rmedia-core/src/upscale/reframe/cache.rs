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
    use sha2::{Digest, Sha256};
    let canonical = serde_json::to_vec(k).expect("key inputs serialise");
    format!("{:x}", Sha256::digest(canonical))
}

#[derive(Debug, Clone)]
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: PathBuf) -> Cache {
        Cache { dir }
    }

    /// The entry for `key`, if present, readable, valid and accepted by `expect` (the caller's
    /// check that it fits this job: frame count, sizes).
    pub fn get(&self, key: &str, expect: impl Fn(&ReframeAnalysis) -> bool) -> Option<ReframeAnalysis> {
        if !is_key(key) {
            return None;
        }
        use std::io::Read;
        let path = self.dir.join(format!("{key}.json"));
        // One handle, opened without following a symlink; size and type checked on it.
        let mut file = open_nofollow(std::fs::File::options().read(true), &path).ok()?;
        let meta = file.metadata().ok()?;
        if !meta.file_type().is_file() || meta.len() > MAX_ENTRY_BYTES {
            return None;
        }
        let mut bytes = Vec::new();
        (&mut file).take(MAX_ENTRY_BYTES).read_to_end(&mut bytes).ok()?;
        let a: ReframeAnalysis = serde_json::from_slice(&bytes).ok()?;
        if a.key != key || !a.is_valid() || !expect(&a) {
            return None;
        }
        // Recently used: evicted last (best effort; a read-only cache still hits).
        if let Ok(f) = open_nofollow(std::fs::File::options().write(true), &path) {
            let _ = f.set_modified(std::time::SystemTime::now());
        }
        Some(a)
    }

    /// Stores `a` under `a.key` and evicts the oldest entries beyond [`MAX_ENTRIES`].
    pub fn put(&self, a: &ReframeAnalysis) -> std::io::Result<()> {
        if !is_key(&a.key) {
            return Err(std::io::Error::other("invalid cache key"));
        }
        use std::io::Write;
        create_private_dirs(&self.dir)?;
        // A fresh, unique temp file (never an existing path or link), then an atomic rename.
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let tmp = self.dir.join(format!(".{}.{}-{nanos}.tmp", a.key, std::process::id()));
        let written = std::fs::File::options()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .and_then(|mut f| f.write_all(&serde_json::to_vec(a).map_err(std::io::Error::other)?))
            .and_then(|()| std::fs::rename(&tmp, self.dir.join(format!("{}.json", a.key))));
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        written?;
        self.evict();
        Ok(())
    }
}

impl Cache {
    /// Removes the least recently used entries beyond [`MAX_ENTRIES`] (ties by name).
    fn evict(&self) {
        // Temp files left by killed runs.
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            let day = std::time::Duration::from_secs(24 * 3600);
            for e in rd.flatten() {
                let stale = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|a| a > day);
                let name = e.file_name().to_string_lossy().into_owned();
                if stale && name.starts_with('.') && name.ends_with(".tmp") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return };
        let mut entries: Vec<(std::time::SystemTime, String)> = rd
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let key = name.strip_suffix(".json")?;
                is_key(key).then(|| {
                    (
                        e.metadata().and_then(|m| m.modified()).ok().unwrap_or(std::time::UNIX_EPOCH),
                        name.clone(),
                    )
                })
            })
            .collect();
        if entries.len() <= MAX_ENTRIES {
            return;
        }
        entries.sort();
        let excess = entries.len() - MAX_ENTRIES;
        for (_, name) in entries.into_iter().take(excess) {
            let _ = std::fs::remove_file(self.dir.join(name));
        }
    }
}

/// Opens `path` without following a final symlink (Unix; elsewhere a plain open).
fn open_nofollow(opts: &mut std::fs::OpenOptions, path: &std::path::Path) -> std::io::Result<std::fs::File> {
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(opts, libc::O_NOFOLLOW);
    opts.open(path)
}

/// The cache folder is private to the user (0700 on Unix).
fn create_private_dirs(dir: &std::path::Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir)
}

/// Cache keys are SHA-256 hex digests; anything else could escape the folder.
fn is_key(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit())
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
        type Edit = Box<dyn Fn(&mut KeyInputs)>;
        let edits: Vec<Edit> = vec![
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
        #[cfg(unix)]
        {
            // A symlinked entry is never followed.
            std::fs::create_dir_all(d.path().join("reframe")).unwrap();
            let target = d.path().join("elsewhere.json");
            std::fs::write(&target, serde_json::to_vec(&a).unwrap()).unwrap();
            std::os::unix::fs::symlink(&target, d.path().join("reframe").join(format!("{}.json", a.key)))
                .unwrap();
            assert_eq!(c.get(&a.key, |_| true), None);
            std::fs::remove_file(d.path().join("reframe").join(format!("{}.json", a.key))).unwrap();
        }
        c.put(&a).unwrap();
        assert_eq!(c.get(&a.key, |_| true), Some(a.clone()), "bit-exact round trip");
        assert_eq!(c.get(&a.key, |x| x.path.len() == 99), None, "an entry for another job shape is a miss");
        assert_eq!(c.get(&"b".repeat(64), |_| true), None);
        std::fs::write(d.path().join("reframe").join(format!("{}.json", a.key)), "{ broken").unwrap();
        assert_eq!(c.get(&a.key, |_| true), None, "a corrupt entry is a miss");
        c.put(&a).unwrap();
        assert_eq!(c.get(&a.key, |_| true), Some(a.clone()), "and is rewritten");
        let mut bad = analysis(&"c".repeat(64));
        bad.path = vec![475]; // odd x: invalid
        c.put(&bad).unwrap();
        assert_eq!(c.get(&bad.key, |_| true), None, "entries that fail validation are misses");
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
        assert_eq!(c.get(&format!("{:064}", 0), |_| true), None, "the oldest entry went");
        assert!(c.get(&format!("{MAX_ENTRIES:064}"), |_| true).is_some());
    }
}
