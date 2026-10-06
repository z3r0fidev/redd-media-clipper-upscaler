//! Path helpers.

use std::path::{Path, PathBuf};

/// `std::fs::canonicalize` without Windows verbatim prefixes (`\\?\C:\x` → `C:\x`) where the
/// plain form means the same thing, so paths shown to users and written to manifests stay
/// readable. UNC and over-long paths keep the prefix. Identity on other platforms.
pub fn canonicalize(p: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(p).map(simplify)
}

pub fn simplify(p: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            let b = rest.as_bytes();
            let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
            if drive && rest.len() < 260 {
                return PathBuf::from(rest);
            }
        }
    }
    p
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn strips_drive_verbatim_prefix_only() {
        use super::simplify;
        use std::path::PathBuf;
        assert_eq!(simplify(PathBuf::from(r"\\?\C:\v\a.mp4")), PathBuf::from(r"C:\v\a.mp4"));
        assert_eq!(simplify(PathBuf::from(r"\\?\UNC\srv\s\a.mp4")), PathBuf::from(r"\\?\UNC\srv\s\a.mp4"));
    }
}
