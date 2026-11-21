//! User data files shared by the GIF and video target registries (research R10): the built-ins,
//! then `<config>/<dir>/*.json`, capped and validated, problems collected, never fatal.
//!
//! User files are untrusted: they are read with the device-preset protections (regular files
//! only, at most 64 KiB), at most [`MAX_USER_FILES`] of them (the first by name), never one whose
//! name has control or text-direction characters, and the type's own validation runs on every
//! one. Device presets (002), GIF targets (003) and video targets (005) all load through here.

use std::collections::BTreeSet;
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
    // The first files by name, whatever order the folder lists them in: the whole listing is
    // walked, but only the first MAX_USER_FILES names are kept.
    let mut files: BTreeSet<PathBuf> = BTreeSet::new();
    let mut total = 0usize;
    let mut unsafe_names = Vec::new();
    let mut unsafe_count = 0usize;
    for path in entries.flatten().map(|e| e.path()) {
        if !path.extension().is_some_and(|x| x.eq_ignore_ascii_case("json")) {
            continue;
        }
        // A name with control or text-direction characters could rewrite a terminal line or
        // disguise itself when printed: it is not read, and only its cleaned name is shown.
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.chars().any(crate::gif::target::unsafe_char) {
            unsafe_count += 1;
            if unsafe_names.len() < 3 {
                unsafe_names.push(shown(&name));
            }
            continue;
        }
        total += 1;
        files.insert(path);
        if files.len() > MAX_USER_FILES {
            files.pop_last();
        }
    }
    if total > MAX_USER_FILES {
        problems.push(PresetProblem {
            file: dir.to_path_buf(),
            message: format!(
                "{} files ignored: at most {MAX_USER_FILES} {} files are read",
                total - MAX_USER_FILES,
                T::KIND
            ),
        });
    }
    if unsafe_count > 0 {
        problems.push(PresetProblem {
            file: dir.to_path_buf(),
            message: format!(
                "{unsafe_count} file{} ignored: {} control or text-direction characters ({}{})",
                if unsafe_count == 1 { "" } else { "s" },
                if unsafe_count == 1 { "its name has" } else { "their names have" },
                unsafe_names.join(", "),
                if unsafe_count > unsafe_names.len() { ", …" } else { "" }
            ),
        });
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

    /// Which files are read does not depend on the folder's listing order: always the first 64
    /// by name, however many files there are.
    #[test]
    fn the_first_64_by_name_are_read_however_many_there_are() {
        let dir = tempfile::tempdir().unwrap();
        for i in (0..300).rev() {
            write(dir.path(), &format!("{i:03}.json"), &format!(r#"{{"id":"u{i:03}","n":1}}"#));
        }
        let mut problems = Vec::new();
        let items = load::<Item>(Vec::new(), dir.path(), &mut problems);
        let want: Vec<String> = (0..64).map(|i| format!("u{i:03}")).collect();
        assert_eq!(items.iter().map(|(i, _)| i.id.clone()).collect::<Vec<_>>(), want);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].message, "236 files ignored: at most 64 test item files are read");
    }

    /// A file whose name has control or text-direction characters is not read, and its name is
    /// shown with those characters replaced (the problem names the folder, not the file).
    #[cfg(unix)]
    #[test]
    fn files_with_unsafe_names_are_skipped_and_never_printed_raw() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "ok.json", r#"{"id":"ok","n":1}"#);
        write(dir.path(), "a\u{202e}nosj.json", r#"{"id":"a","n":1}"#);
        write(dir.path(), "b\u{1b}[2J.json", r#"{"id":"b","n":1}"#);
        let mut problems = Vec::new();
        let items = load::<Item>(Vec::new(), dir.path(), &mut problems);
        assert_eq!(ids(&items), [("ok", 1, true)]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(problems[0].file, dir.path());
        assert_eq!(
            problems[0].message,
            "2 files ignored: their names have control or text-direction characters (a?nosj.json, b?[2J.json)"
        );
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
