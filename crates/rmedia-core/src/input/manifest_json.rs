//! JSON batch manifests (contracts/manifest.schema.json). Syntax and type errors are reported by
//! line:column; semantic errors (times, end+duration, …) by JSON path. All problems at once.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::plan::{Issue, Severity};
use crate::request::{BatchRequest, ClipRequest, Options, Origin, PartialOptions, SourceRequest};
use crate::time::{EndSpec, Ts};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    version: u32,
    #[serde(default)]
    defaults: PartialOptions,
    videos: Vec<RawVideo>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawVideo {
    input: PathBuf,
    #[serde(default)]
    options: PartialOptions,
    clips: Vec<RawClip>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawClip {
    start: Value,
    #[serde(default)]
    end: Option<Value>,
    #[serde(default)]
    duration: Option<Value>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    options: PartialOptions,
}

/// Reads and parses a manifest; `cli` supplies defaults below the manifest's own.
pub fn load(path: &Path, cli: &Options) -> Result<BatchRequest, Vec<Issue>> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(path, &text, cli),
        Err(e) => Err(vec![issue(path, "", format!("cannot read manifest: {e}"))]),
    }
}

/// Parses an in-memory manifest (the desktop app's queue). Relative paths resolve against `base`.
pub fn from_value(base: &Path, value: &Value, defaults: &Options) -> Result<BatchRequest, Vec<Issue>> {
    parse(&base.join("queue.json"), &value.to_string(), defaults)
}

pub fn parse(path: &Path, text: &str, cli: &Options) -> Result<BatchRequest, Vec<Issue>> {
    let text = text.trim_start_matches('\u{feff}');
    let raw: RawManifest = serde_json::from_str(text).map_err(|e| {
        vec![Issue {
            severity: Severity::Error,
            code: crate::error::IssueCode::InvalidInput,
            origin: Origin::Line { file: path.to_path_buf(), line: e.line() },
            message: format!("{e}"),
        }]
    })?;
    let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut issues = Vec::new();
    if raw.version != 1 {
        issues.push(issue(
            path,
            "version",
            format!("unsupported manifest version {} (expected 1)", raw.version),
        ));
    }
    if raw.videos.is_empty() {
        issues.push(issue(path, "videos", "the manifest lists no videos".into()));
    }
    let defaults = resolve_dirs(&raw.defaults, &base).apply(cli);
    let mut sources = Vec::new();
    for (vi, v) in raw.videos.into_iter().enumerate() {
        let vpath = format!("videos[{vi}]");
        if v.clips.is_empty() {
            issues.push(issue(path, &format!("{vpath}.clips"), "no clips for this video".into()));
        }
        let mut clips = Vec::new();
        for (ci, c) in v.clips.into_iter().enumerate() {
            let cpath = format!("{vpath}.clips[{ci}]");
            match clip(path, &cpath, c, &base) {
                Ok(c) => clips.push(c),
                Err(mut e) => issues.append(&mut e),
            }
        }
        sources.push(SourceRequest {
            path: resolve(&base, &v.input),
            clips,
            overrides: resolve_dirs(&v.options, &base),
            origin: Origin::JsonPath { file: path.to_path_buf(), path: format!("{vpath}.input") },
        });
    }
    if issues.is_empty() { Ok(BatchRequest { defaults, sources }) } else { Err(issues) }
}

fn clip(file: &Path, jpath: &str, c: RawClip, base: &Path) -> Result<ClipRequest, Vec<Issue>> {
    let mut issues = Vec::new();
    let start = time(&c.start).map_err(|m| issues.push(issue(file, &format!("{jpath}.start"), m))).ok();
    let end = match (&c.end, &c.duration) {
        (Some(_), Some(_)) => {
            issues.push(issue(file, jpath, "give either end or duration, not both".into()));
            None
        }
        (Some(e), None) => {
            end_value(e).map_err(|m| issues.push(issue(file, &format!("{jpath}.end"), m))).ok()
        }
        (None, Some(d)) => duration_value(d)
            .map(EndSpec::Duration)
            .map_err(|m| issues.push(issue(file, &format!("{jpath}.duration"), m)))
            .ok(),
        (None, None) => Some(EndSpec::EndOfMedia),
    };
    match (start, end) {
        (Some(start), Some(end)) if issues.is_empty() => Ok(ClipRequest {
            start,
            end,
            name: c.name.filter(|n| !n.trim().is_empty()),
            overrides: resolve_dirs(&c.options, base),
            origin: Origin::JsonPath { file: file.to_path_buf(), path: jpath.to_owned() },
        }),
        _ => Err(issues),
    }
}

/// A position: seconds (number) or the time grammar (string). No `+` (that means a duration).
fn time(v: &Value) -> Result<Ts, String> {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.is_finite() && f >= 0.0 => Ok(Ts::from_secs_f64(f)),
            _ => Err(format!("invalid time {n}")),
        },
        Value::String(s) => Ts::parse(s).map_err(|e| e.to_string()),
        other => Err(format!("expected a time (number of seconds or string), got {other}")),
    }
}

/// `end`: a number is a position; a string follows the CLI/CSV grammar (`end`, `+DURATION`, time).
fn end_value(v: &Value) -> Result<EndSpec, String> {
    match v {
        Value::String(s) => EndSpec::parse(s).map_err(|e| e.to_string()),
        other => time(other).map(EndSpec::At),
    }
}

/// `duration`: a length; a leading `+` is optional (as in CSV).
fn duration_value(v: &Value) -> Result<Ts, String> {
    match v {
        Value::String(s) => Ts::parse(s.strip_prefix('+').unwrap_or(s)).map_err(|e| e.to_string()),
        other => time(other),
    }
}

fn issue(file: &Path, jpath: &str, message: String) -> Issue {
    Issue {
        severity: Severity::Error,
        code: crate::error::IssueCode::InvalidInput,
        origin: Origin::JsonPath { file: file.to_path_buf(), path: jpath.to_owned() },
        message,
    }
}

fn resolve(base: &Path, p: &Path) -> PathBuf {
    if p.is_absolute() { p.to_path_buf() } else { base.join(p) }
}

/// Relative output folders in a manifest are relative to the manifest file.
fn resolve_dirs(o: &PartialOptions, base: &Path) -> PartialOptions {
    PartialOptions { output_dir: o.output_dir.as_deref().map(|d| resolve(base, d)), ..o.clone() }
}

// ---------------------------------------------------------------------------------------------
// Writer (GUI "export queue as manifest")

#[derive(Serialize)]
struct OutManifest<'a> {
    version: u32,
    defaults: &'a PartialOptions,
    videos: Vec<OutVideo<'a>>,
}

#[derive(Serialize)]
struct OutVideo<'a> {
    #[serde(serialize_with = "crate::pathser::path")]
    input: &'a Path,
    #[serde(skip_serializing_if = "is_empty")]
    options: &'a PartialOptions,
    clips: Vec<Value>,
}

fn is_empty(o: &&PartialOptions) -> bool {
    **o == PartialOptions::default()
}

/// Writes a manifest. Times are written as `HH:MM:SS.mmm` strings when exact to the millisecond,
/// otherwise as seconds, so a parse → write → parse round trip is lossless.
pub fn write(req: &BatchRequest, defaults: &PartialOptions) -> String {
    let videos = req
        .sources
        .iter()
        .map(|s| OutVideo {
            input: &s.path,
            options: &s.overrides,
            clips: s
                .clips
                .iter()
                .map(|c| {
                    let mut m = serde_json::Map::new();
                    m.insert("start".into(), time_value(c.start));
                    match c.end {
                        EndSpec::At(t) => {
                            m.insert("end".into(), time_value(t));
                        }
                        EndSpec::Duration(d) => {
                            m.insert("duration".into(), time_value(d));
                        }
                        EndSpec::EndOfMedia => {
                            m.insert("end".into(), Value::String("end".into()));
                        }
                    }
                    if let Some(n) = &c.name {
                        m.insert("name".into(), Value::String(n.clone()));
                    }
                    if c.overrides != PartialOptions::default() {
                        m.insert("options".into(), serde_json::to_value(&c.overrides).unwrap_or_default());
                    }
                    Value::Object(m)
                })
                .collect(),
        })
        .collect();
    let doc = OutManifest { version: 1, defaults, videos };
    serde_json::to_string_pretty(&doc).expect("manifest serializes") + "\n"
}

pub(crate) fn time_value(t: Ts) -> Value {
    if t.as_micros() % 1000 == 0 { Value::String(t.display()) } else { serde_json::json!(t.as_secs_f64()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::Mode;

    fn s(x: f64) -> Ts {
        Ts::from_secs_f64(x)
    }

    const GOOD: &str = r#"{
      "version": 1,
      "defaults": { "mode": "fast", "output_dir": "clips" },
      "videos": [
        { "input": "match12.mkv", "clips": [ { "start": "0:45", "end": "1:10", "name": "opening goal" } ] },
        { "input": "/abs/match13.mp4", "options": { "mode": "accurate" },
          "clips": [ { "start": 30, "duration": 12.5 }, { "start": "12:00", "end": "end",
                       "options": { "container": "mkv" } } ] }
      ]
    }"#;

    #[test]
    fn parses_and_layers_options() {
        let req = parse(Path::new("/m/jobs.json"), GOOD, &Options::default()).unwrap();
        assert_eq!(req.defaults.output_dir.as_deref(), Some(Path::new("/m/clips")));
        assert_eq!(req.sources[0].path, PathBuf::from("/m/match12.mkv"));
        assert_eq!(req.sources[1].path, PathBuf::from("/abs/match13.mp4"));
        let c = &req.sources[0].clips[0];
        assert_eq!(
            (c.start, c.end, c.name.as_deref()),
            (s(45.0), EndSpec::At(s(70.0)), Some("opening goal"))
        );
        assert_eq!(req.sources[1].clips[0].end, EndSpec::Duration(s(12.5)));
        assert_eq!(req.sources[1].clips[1].end, EndSpec::EndOfMedia);
        let resolved =
            req.sources[1].clips[1].overrides.apply(&req.sources[1].overrides.apply(&req.defaults));
        assert_eq!((resolved.mode, resolved.container.as_deref()), (Mode::Accurate, Some("mkv")));
    }

    #[test]
    fn cli_options_are_defaults_below_the_manifest() {
        let cli = Options { mode: Mode::Accurate, container: Some("mov".into()), ..Options::default() };
        let req = parse(Path::new("/m/jobs.json"), GOOD, &cli).unwrap();
        assert_eq!(req.defaults.mode, Mode::Fast, "manifest defaults win over CLI flags");
        assert_eq!(req.defaults.container.as_deref(), Some("mov"), "CLI fills what the manifest leaves open");
    }

    #[test]
    fn syntax_errors_have_line_numbers() {
        let err = parse(Path::new("j.json"), "{\n \"version\": 1,\n \"videos\": [ }", &Options::default())
            .unwrap_err();
        assert!(matches!(err[0].origin, Origin::Line { line: 3, .. }), "{:?}", err[0]);
        let err = parse(Path::new("j.json"), r#"{"version":1,"videos":[],"bogus":1}"#, &Options::default())
            .unwrap_err();
        assert!(err[0].message.contains("unknown field `bogus`"), "{}", err[0].message);
    }

    #[test]
    fn semantic_errors_have_json_paths_and_are_all_reported() {
        let text = r#"{"version":1,"videos":[
            {"input":"a.mp4","clips":[{"start":"1:7x","end":"0:10"},{"start":1,"end":2,"duration":3}]},
            {"input":"b.mp4","clips":[{"start":-1},{"start":"0:01","duration":"abc"}]},
            {"input":"c.mp4","clips":[]}]}"#;
        let err = parse(Path::new("j.json"), text, &Options::default()).unwrap_err();
        let paths: Vec<String> = err
            .iter()
            .map(|i| match &i.origin {
                Origin::JsonPath { path, .. } => path.clone(),
                o => format!("{o:?}"),
            })
            .collect();
        assert_eq!(
            paths,
            [
                "videos[0].clips[0].start",
                "videos[0].clips[1]",
                "videos[1].clips[0].start",
                "videos[1].clips[1].duration",
                "videos[2].clips",
            ]
        );
    }

    #[test]
    fn write_then_parse_round_trips() {
        let req = parse(Path::new("/m/jobs.json"), GOOD, &Options::default()).unwrap();
        let text = write(&req, &PartialOptions::default());
        let again = parse(Path::new("/m/jobs.json"), &text, &Options::default()).unwrap();
        for (a, b) in req.sources.iter().zip(&again.sources) {
            assert_eq!(a.path, b.path);
            assert_eq!(a.overrides, b.overrides);
            for (x, y) in a.clips.iter().zip(&b.clips) {
                assert_eq!((x.start, x.end, &x.name, &x.overrides), (y.start, y.end, &y.name, &y.overrides));
            }
        }
        let odd = Ts::from_micros(1_234_567);
        assert_eq!(time_value(odd), serde_json::json!(1.234567));
    }
}
