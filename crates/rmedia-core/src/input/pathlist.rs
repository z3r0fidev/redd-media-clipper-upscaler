//! Path lists (`--inputs-from`): one path per line; lines starting with `#` (after optional
//! indentation) are comments (write
//! `./#name` for a file whose name starts with `#`); relative paths resolve against `base` (the
//! list file's folder, or the working directory for stdin). Lines are used verbatim apart from a
//! trailing `\r`, so names with leading/trailing spaces are preserved.

use std::path::{Path, PathBuf};

pub fn parse(base: &Path, text: &str) -> Vec<PathBuf> {
    text.lines()
        .enumerate()
        .map(|(i, l)| if i == 0 { l.trim_start_matches('\u{feff}') } else { l })
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|l| {
            let p = PathBuf::from(l);
            if p.is_absolute() { p } else { base.join(p) }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_paths() {
        let base = Path::new("/lists");
        let text = "\u{feff}# videos\r\n/abs/a.mp4\r\n\r\nrel dir/b c.mkv\n  # indented comment\n padded.mp4 \n./#1.mp4\n";
        let got = parse(base, text);
        let want = ["/abs/a.mp4", "/lists/rel dir/b c.mkv", "/lists/ padded.mp4 ", "/lists/./#1.mp4"];
        assert_eq!(got, want.map(PathBuf::from));
    }
}
