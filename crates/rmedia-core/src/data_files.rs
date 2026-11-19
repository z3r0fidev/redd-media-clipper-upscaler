//! User data files shared by the GIF and video target registries (research R10): the built-ins,
//! then `<config>/<dir>/*.json`, capped and validated, problems collected, never fatal.
//!
//! User files are untrusted: they are read with the device-preset protections (regular files
//! only, at most 64 KiB), at most [`MAX_USER_FILES`] of them, and the type's own validation runs
//! on every one.

use std::path::{Path, PathBuf};

use crate::preset::{PresetProblem, PresetSource, read_preset_file};

/// At most this many user files are read from one folder; the rest are reported.
pub(crate) const MAX_USER_FILES: usize = 64;

/// A kind of user data file.
pub(crate) trait DataFile: Sized {
    /// Named in problem messages ("GIF target", "video target").
    const KIND: &'static str;
    /// Parses and validates one file's text.
    fn parse(text: &str) -> Result<Self, String>;
    fn id(&self) -> &str;
}

/// `items` (the built-ins, in their order) with the user files in `dir` applied: a user file
/// replaces the built-in with its id in place; user-only items follow, by id. Problems are
/// appended to `problems`. A missing or unreadable folder adds nothing.
pub(crate) fn load<T: DataFile>(
    mut items: Vec<(T, PresetSource)>,
    dir: &Path,
    problems: &mut Vec<PresetProblem>,
) -> Vec<(T, PresetSource)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return items };
    // A folder with a huge number of files is not listed in full.
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("json")))
        .take(MAX_USER_FILES * 4)
        .collect();
    files.sort();
    if files.len() > MAX_USER_FILES {
        problems.push(PresetProblem {
            file: dir.to_path_buf(),
            message: format!(
                "{}{} files ignored: at most {MAX_USER_FILES} {} files are read",
                files.len() - MAX_USER_FILES,
                if files.len() == MAX_USER_FILES * 4 { " or more" } else { "" },
                T::KIND
            ),
        });
        files.truncate(MAX_USER_FILES);
    }
    let mut user: Vec<(T, PresetSource)> = Vec::new();
    for file in files {
        match read_preset_file(&file).and_then(|t| T::parse(&t)).map_err(|e| shown(&e)) {
            Ok(t) if user.iter().any(|(u, _)| u.id() == t.id()) => problems.push(PresetProblem {
                file,
                message: format!("another user {} already uses the id {:?}", T::KIND, t.id()),
            }),
            Ok(t) => user.push((t, PresetSource::User(file))),
            Err(message) => problems.push(PresetProblem { file, message }),
        }
    }
    user.sort_by(|a, b| a.0.id().cmp(b.0.id()));
    for (t, src) in user {
        match items.iter_mut().find(|(b, _)| b.id() == t.id()) {
            Some(slot) => *slot = (t, src),
            None => items.push((t, src)),
        }
    }
    items
}

/// `s` with unsafe characters replaced and at most 300 characters (problem messages quote
/// untrusted file content).
pub(crate) fn shown(s: &str) -> String {
    s.chars().take(300).map(|c| if crate::gif::target::unsafe_char(c) { '?' } else { c }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `{"id": "...", "n": 1}`; `n` must be positive.
    #[derive(Debug, PartialEq)]
    struct Item {
        id: String,
        n: i64,
    }

    impl DataFile for Item {
        const KIND: &'static str = "test item";
        fn parse(text: &str) -> Result<Item, String> {
            let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
            let id = v["id"].as_str().ok_or("no id")?.to_owned();
            let n = v["n"].as_i64().filter(|n| *n > 0).ok_or("n must be positive\u{202e}")?;
            Ok(Item { id, n })
        }
        fn id(&self) -> &str {
            &self.id
        }
    }

    fn builtins() -> Vec<(Item, PresetSource)> {
        ["b", "a"].map(|id| (Item { id: id.into(), n: 1 }, PresetSource::Builtin)).into()
    }

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        p
    }

    fn ids(items: &[(Item, PresetSource)]) -> Vec<(&str, i64, bool)> {
        items.iter().map(|(i, s)| (i.id.as_str(), i.n, matches!(s, PresetSource::User(_)))).collect()
    }

    #[test]
    fn a_missing_folder_keeps_the_built_ins() {
        let mut problems = Vec::new();
        let items = load(builtins(), Path::new("/nonexistent/rmedia-test"), &mut problems);
        assert_eq!(ids(&items), [("b", 1, false), ("a", 1, false)]);
        assert!(problems.is_empty());
    }

    #[test]
    fn user_files_replace_built_ins_in_place_and_new_ones_follow_by_id() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "1.json", r#"{"id":"a","n":5}"#);
        write(dir.path(), "2.json", r#"{"id":"z","n":2}"#);
        write(dir.path(), "3.json", r#"{"id":"c","n":3}"#);
        write(dir.path(), "notes.txt", "ignored: not .json");
        let mut problems = Vec::new();
        let items = load(builtins(), dir.path(), &mut problems);
        assert_eq!(ids(&items), [("b", 1, false), ("a", 5, true), ("c", 3, true), ("z", 2, true)]);
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn bad_and_duplicate_files_are_problems_with_clean_messages() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "1.json", r#"{"id":"x","n":1}"#);
        let dup = write(dir.path(), "2.json", r#"{"id":"x","n":2}"#);
        let bad = write(dir.path(), "3.json", r#"{"id":"y","n":0}"#);
        let mut problems = Vec::new();
        let items = load(builtins(), dir.path(), &mut problems);
        assert_eq!(ids(&items).last(), Some(&("x", 1, true)), "the first file by name wins");
        assert_eq!(problems.len(), 2);
        assert_eq!(problems[0].file, dup);
        assert_eq!(problems[0].message, "another user test item already uses the id \"x\"");
        assert_eq!(problems[1].file, bad);
        assert_eq!(problems[1].message, "n must be positive?", "control characters are replaced");
    }

    #[test]
    fn at_most_64_files_are_read() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..70 {
            write(dir.path(), &format!("{i:03}.json"), &format!(r#"{{"id":"u{i:03}","n":1}}"#));
        }
        let mut problems = Vec::new();
        let items = load::<Item>(Vec::new(), dir.path(), &mut problems);
        assert_eq!(items.len(), MAX_USER_FILES);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].message, "6 files ignored: at most 64 test item files are read");
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_and_oversized_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let real = write(dir.path(), "real.data", r#"{"id":"s","n":1}"#);
        std::os::unix::fs::symlink(&real, dir.path().join("link.json")).unwrap();
        write(dir.path(), "big.json", &format!(r#"{{"id":"big","n":1,"pad":"{}"}}"#, "x".repeat(70_000)));
        let mut problems = Vec::new();
        let items = load::<Item>(Vec::new(), dir.path(), &mut problems);
        assert!(items.is_empty());
        assert_eq!(problems.len(), 2, "{problems:?}");
    }
}
