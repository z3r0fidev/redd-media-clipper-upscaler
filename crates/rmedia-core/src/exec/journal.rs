//! In-flight journal: records temp files of a running process so that a crash or kill never
//! leaves partial outputs behind for good (FR-021).
//!
//! Each run holds an exclusive OS lock on its journal file. At startup, any journal that can be
//! locked belongs to a dead process; its temp files are deleted. Locks are released by the OS
//! when a process dies, so no PID bookkeeping is needed.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::commit::remove_with_retry;

#[derive(Debug)]
pub struct Journal {
    path: PathBuf,
    inner: Mutex<(File, Vec<PathBuf>)>,
}

impl Journal {
    pub fn open(state_dir: &Path, run_id: &str) -> std::io::Result<Journal> {
        let dir = state_dir.join("inflight");
        create_private_dir(&dir)?;
        let path = dir.join(format!("{run_id}-{}.json", std::process::id()));
        // Lock before truncating: a concurrent purge_stale must never see an empty, unlocked file.
        let file = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&path)?;
        file.lock()?;
        file.set_len(0)?;
        let journal = Journal { path, inner: Mutex::new((file, Vec::new())) };
        journal.flush();
        Ok(journal)
    }

    pub fn add(&self, temp: &Path) {
        let temp = std::path::absolute(temp).unwrap_or_else(|_| temp.to_path_buf());
        let mut g = self.inner.lock().expect("journal lock");
        g.1.push(temp);
        drop(g);
        self.flush();
    }

    pub fn remove(&self, temp: &Path) {
        let temp = std::path::absolute(temp).unwrap_or_else(|_| temp.to_path_buf());
        let mut g = self.inner.lock().expect("journal lock");
        g.1.retain(|p| *p != temp);
        drop(g);
        self.flush();
    }

    fn flush(&self) {
        let mut g = self.inner.lock().expect("journal lock");
        let body = serde_json::to_vec(&g.1).unwrap_or_default();
        let file = &mut g.0;
        let _ = file.set_len(0).and_then(|()| file.seek(SeekFrom::Start(0))).and_then(|_| {
            file.write_all(&body)?;
            file.flush()
        });
    }
}

impl Drop for Journal {
    fn drop(&mut self) {
        if let Ok(g) = self.inner.get_mut() {
            for temp in &g.1 {
                remove_with_retry(temp);
            }
            let _ = g.0.unlock();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Deletes temp files left by dead processes. Returns how many temp files were removed.
pub fn purge_stale(state_dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(state_dir.join("inflight")) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Ok(mut file) = OpenOptions::new().read(true).write(true).open(&path) else { continue };
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => continue, // owner still running
            Err(TryLockError::Error(_)) => continue,
        }
        // Journal files are named `<run_id>-<pid>.json`; entries must belong to that run.
        let Some(run_id) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.rsplit_once('-'))
            .map(|(run, _pid)| run.to_owned())
        else {
            continue;
        };
        let mut body = String::new();
        let _ = file.read_to_string(&mut body);
        // A brand-new journal may be created but not yet locked by its owner; leave fresh empty
        // files alone so a starting run never loses its journal.
        let fresh = file
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < std::time::Duration::from_secs(60));
        if body.trim().is_empty() && fresh {
            continue;
        }
        let temps: Vec<PathBuf> = serde_json::from_str(&body).unwrap_or_default();
        for temp in temps {
            if is_own_temp(&temp, &run_id) {
                remove_with_retry(&temp);
                removed += 1;
            }
        }
        drop(file);
        let _ = std::fs::remove_file(&path);
    }
    removed
}

/// A journal entry is deleted only if it is exactly one of our temp outputs from `run_id`:
/// absolute and named `.<stem>.rmedia-<run_id>-<job>.part` (a regular file) or
/// `.<stem>.rmedia-<run_id>-<job>.d` (a scratch directory). Symlinks never qualify.
fn is_own_temp(path: &Path, run_id: &str) -> bool {
    if !path.is_absolute() || run_id.is_empty() {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return false };
    let Some(name) = name.strip_prefix('.') else { return false };
    let (rest, want_dir) = match (name.strip_suffix(".part"), name.strip_suffix(".d")) {
        (Some(r), _) => (r, false),
        (None, Some(r)) => (r, true),
        (None, None) => return false,
    };
    let marker = format!(".rmedia-{run_id}-");
    let Some((stem, job)) = rest.rsplit_once(&marker) else { return false };
    if stem.is_empty() || job.is_empty() || !job.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    std::fs::symlink_metadata(path).is_ok_and(|m| {
        let t = m.file_type();
        if want_dir { t.is_dir() } else { t.is_file() }
    })
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_journal_is_not_purged_and_cleans_up_on_drop() {
        let d = tempfile::tempdir().unwrap();
        let temp = d.path().join(".x.rmedia-r-1.part");
        std::fs::write(&temp, b"partial").unwrap();
        let j = Journal::open(d.path(), "r").unwrap();
        j.add(&temp);
        assert_eq!(purge_stale(d.path()), 0, "locked journal must be left alone");
        assert!(temp.exists());
        drop(j);
        assert!(!temp.exists());
        assert_eq!(std::fs::read_dir(d.path().join("inflight")).unwrap().count(), 0);
    }

    #[test]
    fn stale_journal_is_purged() {
        let d = tempfile::tempdir().unwrap();
        let temp = d.path().join(".x.rmedia-dead-1.part");
        let unrelated = d.path().join("keep.mp4");
        let other_run = d.path().join(".y.rmedia-otherrun-1.part");
        let lookalike = d.path().join("x.rmedia-dead-1.part"); // no leading dot
        for f in [&temp, &unrelated, &other_run, &lookalike] {
            std::fs::write(f, b"x").unwrap();
        }
        std::fs::create_dir_all(d.path().join("inflight")).unwrap();
        let entries = vec![
            temp.clone(),
            unrelated.clone(),
            other_run.clone(),
            lookalike.clone(),
            std::path::PathBuf::from(".x.rmedia-dead-2.part"), // relative
        ];
        std::fs::write(d.path().join("inflight/dead-1.json"), serde_json::to_vec(&entries).unwrap()).unwrap();
        assert_eq!(purge_stale(d.path()), 1);
        assert!(!temp.exists());
        for kept in [&unrelated, &other_run, &lookalike] {
            assert!(kept.exists(), "must not delete {}", kept.display());
        }
    }

    #[test]
    fn stale_scratch_dirs_are_purged_but_lookalikes_are_not() {
        let d = tempfile::tempdir().unwrap();
        let scratch = d.path().join(".x.rmedia-dead-1.d");
        std::fs::create_dir(&scratch).unwrap();
        std::fs::write(scratch.join("reframe.cmd"), b"0 crop@trk x 2;").unwrap();
        let file_named_d = d.path().join(".y.rmedia-dead-2.d"); // a file, not a directory
        std::fs::write(&file_named_d, b"x").unwrap();
        let other_run = d.path().join(".z.rmedia-otherrun-1.d");
        std::fs::create_dir(&other_run).unwrap();
        std::fs::create_dir_all(d.path().join("inflight")).unwrap();
        let entries = vec![scratch.clone(), file_named_d.clone(), other_run.clone()];
        std::fs::write(d.path().join("inflight/dead-1.json"), serde_json::to_vec(&entries).unwrap()).unwrap();
        assert_eq!(purge_stale(d.path()), 1);
        assert!(!scratch.exists());
        assert!(file_named_d.exists() && other_run.exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_scratch_dirs_are_never_followed() {
        let d = tempfile::tempdir().unwrap();
        let victim = d.path().join("victim");
        std::fs::create_dir(&victim).unwrap();
        std::fs::write(victim.join("precious.mp4"), b"precious").unwrap();
        let link = d.path().join(".x.rmedia-dead-1.d");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        std::fs::create_dir_all(d.path().join("inflight")).unwrap();
        std::fs::write(d.path().join("inflight/dead-1.json"), serde_json::to_vec(&vec![&link]).unwrap())
            .unwrap();
        assert_eq!(purge_stale(d.path()), 0);
        assert!(victim.join("precious.mp4").exists());
    }

    #[cfg(unix)]
    #[test]
    fn removing_a_scratch_dir_does_not_follow_symlinks_inside_it() {
        let d = tempfile::tempdir().unwrap();
        let victim = d.path().join("victim");
        std::fs::create_dir(&victim).unwrap();
        std::fs::write(victim.join("precious.mp4"), b"precious").unwrap();
        let scratch = d.path().join(".x.rmedia-r-1.d");
        std::fs::create_dir(&scratch).unwrap();
        std::os::unix::fs::symlink(&victim, scratch.join("planted")).unwrap();
        remove_with_retry(&scratch);
        assert!(!scratch.exists());
        assert!(victim.join("precious.mp4").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_entries_are_never_followed() {
        let d = tempfile::tempdir().unwrap();
        let victim = d.path().join("victim.mp4");
        std::fs::write(&victim, b"precious").unwrap();
        let link = d.path().join(".x.rmedia-dead-1.part");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        std::fs::create_dir_all(d.path().join("inflight")).unwrap();
        std::fs::write(d.path().join("inflight/dead-1.json"), serde_json::to_vec(&vec![&link]).unwrap())
            .unwrap();
        assert_eq!(purge_stale(d.path()), 0);
        assert!(victim.exists() && link.exists());
    }
}
