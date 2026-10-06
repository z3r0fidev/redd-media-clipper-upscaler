//! CSV batch manifests (contracts/manifest-csv.md): header required, one row per clip, rows that
//! share an `input` grouped in order of first appearance. Errors are reported by line.

use std::path::{Path, PathBuf};

use crate::input::manifest_json::time_value;
use crate::plan::{Issue, Severity};
use crate::request::{BatchRequest, ClipRequest, Mode, Options, Origin, PartialOptions, SourceRequest};
use crate::time::{EndSpec, Ts};

const COLUMNS: [&str; 7] = ["input", "start", "end", "duration", "name", "mode", "output"];

pub fn load(path: &Path, cli: &Options) -> Result<BatchRequest, Vec<Issue>> {
    match std::fs::read(path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => parse(path, &text, cli),
            Err(e) => Err(vec![issue(
                path,
                crate::input::cliplist::utf8_error_line(e.as_bytes(), &e),
                "not valid UTF-8 text".into(),
            )]),
        },
        Err(e) => Err(vec![issue(path, 0, format!("cannot read manifest: {e}"))]),
    }
}

pub fn parse(path: &Path, text: &str, cli: &Options) -> Result<BatchRequest, Vec<Issue>> {
    let text = text.trim_start_matches('\u{feff}');
    let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut rdr = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .trim(csv::Trim::All)
        .flexible(false)
        .from_reader(text.as_bytes());
    let mut issues = Vec::new();
    let headers: Vec<String> = match rdr.headers() {
        Ok(h) => h.iter().map(|s| s.to_ascii_lowercase()).collect(),
        Err(e) => return Err(vec![issue(path, 1, format!("cannot read the header row: {e}"))]),
    };
    let header_line = line_at(text, 0);
    for h in &headers {
        if !COLUMNS.contains(&h.as_str()) {
            issues.push(issue(
                path,
                header_line,
                format!("unknown column \"{h}\" (allowed: {})", COLUMNS.join(", ")),
            ));
        }
    }
    for (i, h) in headers.iter().enumerate() {
        if headers[..i].contains(h) {
            issues.push(issue(path, header_line, format!("column \"{h}\" appears more than once")));
        }
    }
    for required in ["input", "start"] {
        if !headers.iter().any(|h| h == required) {
            issues.push(issue(path, header_line, format!("missing required column \"{required}\"")));
        }
    }
    if !issues.is_empty() {
        return Err(issues);
    }
    let col = |name: &str| headers.iter().position(|h| h == name);
    let (c_input, c_start) = (col("input").expect("checked"), col("start").expect("checked"));
    let (c_end, c_dur, c_name, c_mode, c_out) =
        (col("end"), col("duration"), col("name"), col("mode"), col("output"));

    let mut sources: Vec<SourceRequest> = Vec::new();
    for record in rdr.records() {
        let record = match record {
            Ok(r) => r,
            Err(e) => {
                let line = e.position().map_or(0, |p| line_at(text, p.byte()));
                issues.push(issue(path, line, format!("malformed row: {e}")));
                continue;
            }
        };
        let line = record.position().map_or(0, |p| line_at(text, p.byte()));
        let get = |c: Option<usize>| c.and_then(|i| record.get(i)).filter(|v| !v.is_empty());
        let mut row_issues = Vec::new();
        let input = match get(Some(c_input)) {
            Some(i) => normalize(&resolve(&base, Path::new(i))),
            None => {
                row_issues.push("input is empty".to_owned());
                PathBuf::new()
            }
        };
        let start = match get(Some(c_start)).map(Ts::parse) {
            Some(Ok(t)) => Some(t),
            Some(Err(e)) => {
                row_issues.push(format!("start: {e}"));
                None
            }
            None => {
                row_issues.push("start is empty".into());
                None
            }
        };
        let end = match (get(c_end), get(c_dur)) {
            (Some(_), Some(_)) => {
                row_issues.push("give either end or duration, not both".into());
                None
            }
            (Some(e), None) => EndSpec::parse(e).map_err(|e| row_issues.push(format!("end: {e}"))).ok(),
            (None, Some(d)) => Ts::parse(d.trim_start_matches('+'))
                .map(EndSpec::Duration)
                .map_err(|e| row_issues.push(format!("duration: {e}")))
                .ok(),
            (None, None) => Some(EndSpec::EndOfMedia),
        };
        let mode = match get(c_mode).map(str::to_ascii_lowercase).as_deref() {
            None => None,
            Some("fast") => Some(Mode::Fast),
            Some("accurate") => Some(Mode::Accurate),
            Some(other) => {
                row_issues.push(format!("mode must be fast or accurate, not \"{other}\""));
                None
            }
        };
        if !row_issues.is_empty() {
            issues.extend(row_issues.into_iter().map(|m| issue(path, line, m)));
            continue;
        }
        let clip = ClipRequest {
            start: start.expect("checked"),
            end: end.expect("checked"),
            name: get(c_name).map(str::to_owned),
            overrides: PartialOptions {
                mode,
                output_dir: get(c_out).map(|o| resolve(&base, Path::new(o))),
                ..PartialOptions::default()
            },
            origin: Origin::Line { file: path.to_path_buf(), line },
        };
        match sources.iter_mut().find(|s| s.path == input) {
            Some(s) => s.clips.push(clip),
            None => sources.push(SourceRequest {
                path: input,
                clips: vec![clip],
                overrides: PartialOptions::default(),
                origin: Origin::Line { file: path.to_path_buf(), line },
            }),
        }
    }
    if issues.is_empty() && sources.is_empty() {
        issues.push(issue(path, 0, "the manifest contains no clips".into()));
    }
    if issues.is_empty() { Ok(BatchRequest { defaults: cli.clone(), sources }) } else { Err(issues) }
}

/// 1-based line of the record starting at `byte`. The csv crate reports a record that follows a
/// comment at the comment's position (and its own line counter skips comments), so skip forward
/// over comment and blank lines.
fn line_at(text: &str, byte: u64) -> usize {
    let mut pos = (byte as usize).min(text.len());
    let mut line = text.as_bytes()[..pos].iter().filter(|&&b| b == b'\n').count() + 1;
    loop {
        let rest = &text[pos..];
        let this = rest.split('\n').next().unwrap_or("");
        let skippable = this.trim_start().starts_with('#') || this.trim().is_empty();
        if !skippable || this.len() == rest.len() {
            return line;
        }
        pos += this.len() + 1;
        line += 1;
    }
}

fn issue(file: &Path, line: usize, message: String) -> Issue {
    Issue {
        severity: Severity::Error,
        code: "INVALID_INPUT",
        origin: Origin::Line { file: file.to_path_buf(), line },
        message,
    }
}

/// Drops `.` components so `a.mp4` and `./a.mp4` group as the same source.
fn normalize(p: &Path) -> PathBuf {
    p.components().filter(|c| !matches!(c, std::path::Component::CurDir)).collect()
}

fn resolve(base: &Path, p: &Path) -> PathBuf {
    if p.is_absolute() { p.to_path_buf() } else { base.join(p) }
}

/// Writes a CSV manifest. Per-source options are folded into each row; options CSV cannot express
/// (container, name template, streams…) are not written — use JSON to keep them.
pub fn write(req: &BatchRequest) -> String {
    let mut w = csv::Writer::from_writer(Vec::new());
    let _ = w.write_record(COLUMNS);
    let text = |t: Ts| match time_value(t) {
        serde_json::Value::String(s) => s,
        v => v.to_string(),
    };
    for s in &req.sources {
        for c in &s.clips {
            let o = c.overrides.apply(&s.overrides.apply(&req.defaults));
            let (end, dur) = match c.end {
                EndSpec::At(t) => (text(t), String::new()),
                EndSpec::Duration(d) => (String::new(), text(d)),
                EndSpec::EndOfMedia => ("end".into(), String::new()),
            };
            let mode = match o.mode {
                Mode::Fast => "fast",
                Mode::Accurate => "accurate",
            };
            let _ = w.write_record([
                s.path.to_string_lossy().as_ref(),
                &text(c.start),
                &end,
                &dur,
                c.name.as_deref().unwrap_or(""),
                mode,
                &o.output_dir.as_deref().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
            ]);
        }
    }
    String::from_utf8(w.into_inner().unwrap_or_default()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: f64) -> Ts {
        Ts::from_secs_f64(x)
    }

    #[test]
    fn parses_groups_and_resolves() {
        let text = "\u{feff}input,start,end,duration,name,mode,output\n\
                    # comment\n\
                    match12.mkv,0:45,1:10,,opening goal,,\n\
                    /abs/match13.mp4,30,,12.5,,accurate,\n\
                    match12.mkv, 2:00 ,end,,\"a, b\",FAST,out\n";
        let req = parse(Path::new("/m/jobs.csv"), text, &Options::default()).unwrap();
        assert_eq!(req.sources.len(), 2);
        assert_eq!(req.sources[0].path, PathBuf::from("/m/match12.mkv"));
        let c = &req.sources[0].clips;
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].start, c[0].end), (s(45.0), EndSpec::At(s(70.0))));
        assert_eq!(
            (c[1].start, c[1].end, c[1].name.as_deref()),
            (s(120.0), EndSpec::EndOfMedia, Some("a, b"))
        );
        assert_eq!(c[1].overrides.output_dir.as_deref(), Some(Path::new("/m/out")));
        assert_eq!(c[1].overrides.mode, Some(Mode::Fast));
        assert_eq!(req.sources[1].clips[0].end, EndSpec::Duration(s(12.5)));
        assert!(matches!(c[0].origin, Origin::Line { line: 3, .. }));
    }

    #[test]
    fn column_order_is_free_and_optional_columns_may_be_missing() {
        let req = parse(Path::new("j.csv"), "start,input\n5,a.mp4\n", &Options::default()).unwrap();
        assert_eq!(req.sources[0].clips[0].end, EndSpec::EndOfMedia);
    }

    #[test]
    fn reports_errors_by_line() {
        let err =
            parse(Path::new("j.csv"), "input,start,bogus\na.mp4,1,x\n", &Options::default()).unwrap_err();
        assert!(err[0].message.contains("unknown column \"bogus\""));
        let err = parse(Path::new("j.csv"), "end,name\n1,x\n", &Options::default()).unwrap_err();
        assert_eq!(err.len(), 2, "both required columns reported");
        let text = "input,start,end,duration,mode\na.mp4,1,2,,\na.mp4,1:7x,2,,\n,1,2,,\na.mp4,1,2,3,\na.mp4,1,2,,slow\na.mp4,1,2\n";
        let err = parse(Path::new("j.csv"), text, &Options::default()).unwrap_err();
        let lines: Vec<usize> = err
            .iter()
            .map(|i| match i.origin {
                Origin::Line { line, .. } => line,
                _ => 0,
            })
            .collect();
        assert_eq!(lines, [3, 4, 5, 6, 7]);
    }

    #[test]
    fn comments_above_header_quoting_and_duplicates() {
        let text = "# jobs\n\n# more\ninput,start,end,name\n# c\na.mp4,1,2,\"a, \"\"quoted\"\"\nname\"\nb.mp4,1:7x,2,\n";
        let err = parse(Path::new("j.csv"), text, &Options::default()).unwrap_err();
        assert_eq!(err.len(), 1);
        assert!(matches!(err[0].origin, Origin::Line { line: 8, .. }), "{:?}", err[0]);
        let ok = parse(Path::new("j.csv"), &text.replace("1:7x", "3"), &Options::default()).unwrap();
        assert_eq!(ok.sources[0].clips[0].name.as_deref(), Some("a, \"quoted\"\nname"));
        assert!(matches!(ok.sources[0].clips[0].origin, Origin::Line { line: 6, .. }));
        let err =
            parse(Path::new("j.csv"), "# x\ninput,start,input\na,1,b\n", &Options::default()).unwrap_err();
        assert!(err[0].message.contains("more than once"));
        assert!(matches!(err[0].origin, Origin::Line { line: 2, .. }), "{:?}", err[0]);
    }

    #[test]
    fn invalid_utf8_is_an_error_with_a_line() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("j.csv");
        std::fs::write(&p, b"input,start\na.mp4,1\nb\xff.mp4,2\n").unwrap();
        let err = load(&p, &Options::default()).unwrap_err();
        assert!(err[0].message.contains("UTF-8") && matches!(err[0].origin, Origin::Line { line: 3, .. }));
    }

    #[test]
    fn write_then_parse_round_trips() {
        let text = "input,start,end,duration,name,mode,output\n/v/a.mp4,00:00:01.500,,2,x,accurate,/o\n/v/b.mp4,1.234567,end,,,fast,\n";
        let req = parse(Path::new("/m/j.csv"), text, &Options::default()).unwrap();
        let again = parse(Path::new("/m/j.csv"), &write(&req), &Options::default()).unwrap();
        for (a, b) in req.sources.iter().zip(&again.sources) {
            assert_eq!(a.path, b.path);
            for (x, y) in a.clips.iter().zip(&b.clips) {
                assert_eq!((x.start, x.end, &x.name), (y.start, y.end, &y.name));
                assert_eq!(x.overrides.output_dir, y.overrides.output_dir);
            }
        }
    }
}
