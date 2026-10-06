//! Temp-file naming and atomic, no-clobber commits (research.md R7, Principle V).

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::event::JobId;
use crate::request::OnExists;

/// `<dir>/.<stem>.rmedia-<run>-<job>.part` — hidden, and skipped by discovery.
pub fn temp_path(output: &Path, run_id: &str, job: JobId) -> PathBuf {
    let stem = output.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    output.with_file_name(format!(".{stem}.rmedia-{run_id}-{job}.part"))
}

/// Moves a verified temp file to its final name according to `policy`.
/// Returns the path actually written (may differ for `Rename`).
pub fn commit(temp: &Path, output: &Path, policy: OnExists) -> Result<PathBuf> {
    // Flush the encoded data to disk so a power loss cannot leave an empty "finished" clip.
    // Opened for writing: on Windows FlushFileBuffers needs write access (read-only fails).
    std::fs::OpenOptions::new()
        .write(true)
        .open(temp)
        .and_then(|f| f.sync_all())
        .map_err(|e| Error::OutputUnwritable { path: output.to_path_buf(), message: e.to_string() })?;
    let result = match policy {
        OnExists::Overwrite => std::fs::rename(temp, output).map(|()| output.to_path_buf()),
        OnExists::Skip => no_clobber(temp, output).map(|()| output.to_path_buf()),
        OnExists::Rename => rename_free(temp, output),
    };
    result.map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => Error::OutputExists(output.to_path_buf()),
        _ => Error::OutputUnwritable { path: output.to_path_buf(), message: e.to_string() },
    })
}

/// Links `temp` to `target` only if `target` does not exist (atomic where hard links work).
fn no_clobber(temp: &Path, target: &Path) -> io::Result<()> {
    match std::fs::hard_link(temp, target) {
        Ok(()) => {
            remove_with_retry(temp);
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(e),
        // FAT/exFAT and some network shares lack hard links: atomically reserve the name with
        // create_new (fails if it exists), then replace our own placeholder with the temp file.
        Err(_) => {
            std::fs::OpenOptions::new().write(true).create_new(true).open(target)?;
            std::fs::rename(temp, target)
        }
    }
}

fn rename_free(temp: &Path, output: &Path) -> io::Result<PathBuf> {
    match no_clobber(temp, output) {
        Ok(()) => return Ok(output.to_path_buf()),
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
        Err(_) => {}
    }
    let stem = output.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = output.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for i in 1..10_000 {
        let candidate = output.with_file_name(format!("{stem}_{i}{ext}"));
        match no_clobber(temp, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::from(io::ErrorKind::AlreadyExists))
}

/// Removes a file, retrying briefly (Windows antivirus/indexers can hold files open).
pub fn remove_with_retry(path: &Path) {
    for attempt in 0..10 {
        match std::fs::remove_file(path) {
            Ok(()) => return,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return,
            Err(_) if attempt < 9 => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => tracing::warn!("could not remove {}: {e}", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, data: &str) {
        std::fs::write(p, data).unwrap();
    }

    #[test]
    fn temp_path_is_hidden_part() {
        let t = temp_path(Path::new("/o/clip.mp4"), "abc", 3);
        assert_eq!(t, PathBuf::from("/o/.clip.rmedia-abc-3.part"));
    }

    #[test]
    fn skip_policy_never_clobbers() {
        let d = tempfile::tempdir().unwrap();
        let (tmp, out) = (d.path().join("t.part"), d.path().join("o.mp4"));
        write(&tmp, "new");
        write(&out, "old");
        assert!(matches!(commit(&tmp, &out, OnExists::Skip), Err(Error::OutputExists(_))));
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "old");
        write(&tmp, "new");
        std::fs::remove_file(&out).unwrap();
        assert_eq!(commit(&tmp, &out, OnExists::Skip).unwrap(), out);
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "new");
        assert!(!tmp.exists());
    }

    #[test]
    fn overwrite_and_rename() {
        let d = tempfile::tempdir().unwrap();
        let (tmp, out) = (d.path().join("t.part"), d.path().join("o.mp4"));
        write(&out, "old");
        write(&tmp, "new");
        commit(&tmp, &out, OnExists::Overwrite).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "new");
        write(&tmp, "third");
        let renamed = commit(&tmp, &out, OnExists::Rename).unwrap();
        assert_eq!(renamed, d.path().join("o_1.mp4"));
        assert_eq!(std::fs::read_to_string(&renamed).unwrap(), "third");
    }
}
