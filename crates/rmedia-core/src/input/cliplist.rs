//! Clip list files (contracts/cliplist-format.md): one clip per line, `START END|+DUR [NAME]`.

use std::path::Path;

use crate::plan::{Issue, Severity};
use crate::request::{ClipRequest, Origin, PartialOptions};
use crate::time::{EndSpec, Ts};

/// Reads and parses a clip list file.
pub fn load(path: &Path) -> Result<Vec<ClipRequest>, Vec<Issue>> {
    match std::fs::read(path) {
        Ok(bytes) => parse(path, &String::from_utf8_lossy(&bytes)),
        Err(e) => Err(vec![issue(path, 0, format!("cannot read clip list: {e}"))]),
    }
}

/// Parses clip list text. `path` is used for error origins only.
pub fn parse(path: &Path, text: &str) -> Result<Vec<ClipRequest>, Vec<Issue>> {
    let mut clips = Vec::new();
    let mut issues = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line_no = i + 1;
        let line = if i == 0 { raw.trim_start_matches('\u{feff}') } else { raw };
        let line = strip_comment(line).trim();
        if line.is_empty() {
            continue;
        }
        match parse_line(line) {
            Ok((start, end, name)) => clips.push(ClipRequest {
                start,
                end,
                name,
                overrides: PartialOptions::default(),
                origin: Origin::Line { file: path.to_path_buf(), line: line_no },
            }),
            Err(msg) => issues.push(issue(path, line_no, msg)),
        }
    }
    if issues.is_empty() && clips.is_empty() {
        issues.push(issue(path, 0, "the clip list contains no clips".into()));
    }
    if issues.is_empty() { Ok(clips) } else { Err(issues) }
}

fn issue(path: &Path, line: usize, message: String) -> Issue {
    Issue {
        severity: Severity::Error,
        code: "INVALID_INPUT",
        origin: Origin::Line { file: path.to_path_buf(), line },
        message,
    }
}

/// A comment is `#` at the start of a (trimmed) line, or ` # ` — a `#` with whitespace (or the
/// line end) on both sides — so names like `goal #3` keep their `#`.
fn strip_comment(line: &str) -> &str {
    if line.trim_start().starts_with('#') {
        return "";
    }
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        let before = i > 0 && bytes[i - 1].is_ascii_whitespace();
        let after = bytes.get(i + 1).is_none_or(u8::is_ascii_whitespace);
        if b == b'#' && before && after {
            return &line[..i];
        }
    }
    line
}

fn is_sep(c: char) -> bool {
    c.is_whitespace() || c == ','
}

/// Splits off the next field. A comma directly between `:SS` and exactly three digits is a
/// decimal separator (`00:01:30,500`), not a field separator.
fn next_field(s: &str) -> (&str, &str) {
    let s = s.trim_start_matches(is_sep);
    let mut end = s.len();
    for (i, c) in s.char_indices() {
        if c.is_whitespace() {
            end = i;
            break;
        }
        if c == ',' && !is_decimal_comma(s, i) {
            end = i;
            break;
        }
    }
    (&s[..end], &s[end..])
}

fn is_decimal_comma(s: &str, i: usize) -> bool {
    let before = &s[..i];
    let after = &s[i + 1..];
    let digits_after = after.bytes().take_while(u8::is_ascii_digit).count();
    let ends_ok = after[digits_after..].chars().next().is_none_or(is_sep);
    let b = before.as_bytes();
    let colon_ss = b.len() >= 3 && b[b.len() - 3] == b':' && b[b.len() - 2..].iter().all(u8::is_ascii_digit);
    colon_ss && digits_after == 3 && ends_ok
}

fn parse_line(line: &str) -> Result<(Ts, EndSpec, Option<String>), String> {
    let (start_s, rest) = next_field(line);
    let (end_s, rest) = next_field(rest);
    if end_s.is_empty() {
        return Err(format!("missing end after \"{start_s}\" (use END, +DURATION or end)"));
    }
    let start = Ts::parse(start_s).map_err(|e| e.to_string())?;
    let end = EndSpec::parse(end_s).map_err(|e| e.to_string())?;
    // The name is the rest of the line; a single leading comma separator is dropped.
    let name = rest.trim_start();
    let name = name.strip_prefix(',').unwrap_or(name).trim();
    let name = (!name.is_empty()).then(|| name.to_owned());
    Ok((start, end, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: f64) -> Ts {
        Ts::from_secs_f64(x)
    }

    fn ok(text: &str) -> Vec<(Ts, EndSpec, Option<String>, usize)> {
        parse(Path::new("c.txt"), text)
            .unwrap_or_else(|e| panic!("{text:?}: {e:?}"))
            .into_iter()
            .map(|c| {
                let line = match c.origin {
                    Origin::Line { line, .. } => line,
                    _ => 0,
                };
                (c.start, c.end, c.name, line)
            })
            .collect()
    }

    #[test]
    fn parses_valid_lines() {
        let cases: &[(&str, Ts, EndSpec, Option<&str>)] = &[
            ("0:45 1:10 opening goal", s(45.0), EndSpec::At(s(70.0)), Some("opening goal")),
            ("00:03:15.250  +20    save", s(195.25), EndSpec::Duration(s(20.0)), Some("save")),
            ("12:00,500 end", s(720.5), EndSpec::EndOfMedia, None),
            ("90,100", s(90.0), EndSpec::At(s(100.0)), None),
            ("90, 100, my clip", s(90.0), EndSpec::At(s(100.0)), Some("my clip")),
            ("00:01:30,500,00:01:40,250,x", s(90.5), EndSpec::At(s(100.25)), Some("x")),
            ("1:30\t2:00", s(90.0), EndSpec::At(s(120.0)), None),
            ("1m +30s a, b", s(60.0), EndSpec::Duration(s(30.0)), Some("a, b")),
            ("5 10 goal #3", s(5.0), EndSpec::At(s(10.0)), Some("goal #3")),
            ("5 10 goal # note", s(5.0), EndSpec::At(s(10.0)), Some("goal")),
        ];
        for (text, start, end, name) in cases {
            let got = ok(text);
            assert_eq!(got.len(), 1, "{text}");
            assert_eq!((got[0].0, got[0].1, got[0].2.as_deref()), (*start, *end, *name), "{text}");
        }
    }

    #[test]
    fn skips_comments_blank_lines_bom_crlf_and_numbers_lines() {
        let got = ok("\u{feff}# header\r\n\r\n0:01 0:02 a\r\n   # indented comment\n0:03 +1\n");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].2.as_deref(), Some("a"));
        assert_eq!((got[0].3, got[1].3), (3, 5));
    }

    #[test]
    fn reports_every_bad_line_with_its_number() {
        let err =
            parse(Path::new("c.txt"), "0:01 0:02\n0:05\n1:7x 0:10\n0:01 0:02 ok\n0:01 nope\n").unwrap_err();
        let lines: Vec<usize> = err
            .iter()
            .map(|i| match i.origin {
                Origin::Line { line, .. } => line,
                _ => 0,
            })
            .collect();
        assert_eq!(lines, [2, 3, 5]);
        assert!(err[0].message.contains("missing end"));
        assert!(err[1].message.contains("1:7x"));
        assert_eq!(err[0].to_string(), "c.txt:2: missing end after \"0:05\" (use END, +DURATION or end)");
    }

    #[test]
    fn empty_list_is_an_error() {
        assert!(parse(Path::new("c.txt"), "# nothing\n\n").is_err());
    }
}
