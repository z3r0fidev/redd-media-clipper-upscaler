//! Path lists (`--inputs-from`): one path per line, `#` comments; relative paths resolve against
//! `base` (the list file's folder, or the working directory for stdin).

use std::path::{Path, PathBuf};

pub fn parse(base: &Path, text: &str) -> Vec<PathBuf> {
    text.lines()
        .enumerate()
        .map(|(i, l)| if i == 0 { l.trim_start_matches('\u{feff}') } else { l })
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|l| {
            let p = PathBuf::from(l.trim());
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
        let got =
            parse(base, "\u{feff}# videos\r\n/abs/a.mp4\r\n\r\nrel dir/b c.mkv\n  # indented comment\n");
        assert_eq!(got, [PathBuf::from("/abs/a.mp4"), PathBuf::from("/lists/rel dir/b c.mkv")]);
    }
}
