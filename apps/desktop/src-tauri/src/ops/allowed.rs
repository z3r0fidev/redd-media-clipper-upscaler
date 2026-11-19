//! The paths the page may read and write (contracts › Path policy).

use super::*;

/// Paths the user has chosen.
#[derive(Debug, Default)]
pub struct Allowed {
    files: HashSet<OsString>,
    /// Videos named by an opened manifest: readable, but they never unlock writes next to them.
    manifest_files: HashSet<OsString>,
    dirs: Vec<PathBuf>,
    save_targets: HashSet<OsString>,
    /// Clip lists and manifests picked as such (only these may be parsed).
    lists: HashSet<OsString>,
}

impl Allowed {
    pub(super) fn key(p: &Path) -> Option<(PathBuf, OsString)> {
        let c = std::fs::canonicalize(p).ok()?;
        let k = rmedia_core::discover::path_key(&c);
        Some((c, k))
    }

    pub fn allow(&mut self, p: &Path) {
        if let Some((c, k)) = Self::key(p) {
            if c.is_dir() {
                self.dirs.push(c);
            } else {
                self.files.insert(k);
            }
        }
    }

    pub(super) fn allow_from_manifest(&mut self, p: &Path) {
        if let Some((c, k)) = Self::key(p)
            && c.is_file()
        {
            self.manifest_files.insert(k);
        }
    }

    /// Opened by the user, or inside a folder they picked (not merely named by a manifest).
    pub(super) fn is_picked(&self, p: &Path) -> bool {
        let Some((c, k)) = Self::key(p) else { return false };
        self.files.contains(&k) || self.dirs.iter().any(|d| c.starts_with(d))
    }

    pub fn allow_list(&mut self, p: &Path) {
        if let Some((_, k)) = Self::key(p) {
            self.lists.insert(k);
        }
    }

    pub(super) fn is_list(&self, p: &Path) -> bool {
        Self::key(p).is_some_and(|(_, k)| self.lists.contains(&k))
    }

    /// Where a clip may be written: inside a folder the user picked, or next to a source the user
    /// picked (the default `clips/` folder and template sub-folders).
    pub(super) fn may_write(&self, output: &Path, source: &Path) -> bool {
        let Some(resolved) = resolve_existing(output) else { return false };
        let source_dir = Some(source)
            .filter(|s| self.is_picked(s))
            .and_then(|s| s.parent())
            .and_then(|d| std::fs::canonicalize(d).ok());
        self.dirs.iter().any(|d| resolved.starts_with(d))
            || source_dir.is_some_and(|d| resolved.starts_with(d))
    }

    pub fn allow_save(&mut self, p: &Path) {
        self.save_targets.insert(rmedia_core::discover::path_key(p));
    }

    pub fn is_allowed(&self, p: &Path) -> bool {
        self.is_picked(p) || Self::key(p).is_some_and(|(_, k)| self.manifest_files.contains(&k))
    }

    pub(super) fn is_save_target(&self, p: &Path) -> bool {
        self.save_targets.contains(&rmedia_core::discover::path_key(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub(super) fn may_write_rejects_parent_dir_escapes() {
        let picked = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let src_dir = tempfile::tempdir().unwrap();
        let source = src_dir.path().join("a.mp4");
        std::fs::write(&source, b"").unwrap();
        let mut allowed = Allowed::default();
        allowed.allow(picked.path());
        allowed.allow(&source);
        // Missing folders are fine while the path stays inside the picked folder.
        assert!(allowed.may_write(&picked.path().join("new/sub/a.mp4"), &source));
        // `nope` does not exist, so the `..` steps were kept lexically and passed the prefix check.
        let escape =
            picked.path().join("nope/../..").join(elsewhere.path().file_name().unwrap()).join("x.mp4");
        assert!(!allowed.may_write(&escape, &source));
        assert!(!allowed.may_write(&picked.path().join("../x.mp4"), &source));
        // The next-to-source rule is held to the same standard.
        assert!(allowed.may_write(&src_dir.path().join("clips/a.mp4"), &source));
        assert!(!allowed.may_write(&src_dir.path().join("clips/../../x.mp4"), &source));
    }

    #[test]
    pub(super) fn manifest_sources_are_readable_but_never_unlock_writes_next_to_them() {
        let src_dir = tempfile::tempdir().unwrap();
        let picked = tempfile::tempdir().unwrap();
        let source = src_dir.path().join("a.mp4");
        std::fs::write(&source, b"").unwrap();
        let mut allowed = Allowed::default();
        allowed.allow_from_manifest(&source);
        assert!(allowed.is_allowed(&source));
        assert!(!allowed.may_write(&src_dir.path().join("clips/a.mp4"), &source));
        // A picked output folder still works.
        allowed.allow(picked.path());
        assert!(allowed.may_write(&picked.path().join("a.mp4"), &source));
        // Opening the video itself restores the next-to-source default.
        allowed.allow(&source);
        assert!(allowed.may_write(&src_dir.path().join("clips/a.mp4"), &source));
    }
}
