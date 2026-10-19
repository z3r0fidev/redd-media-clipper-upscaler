//! CSV batch manifests (contracts/manifest-csv.md): header required, one row per clip, rows that
//! share an `input` grouped in order of first appearance. Errors are reported by line.

use std::path::{Path, PathBuf};

use crate::input::manifest_json::time_value;
use crate::plan::{Issue, Severity};
use crate::request::{BatchRequest, ClipRequest, Mode, Options, Origin, PartialOptions, SourceRequest};
use crate::time::{EndSpec, Ts};
use crate::upscale::{MAX_CRF, PartialUpscale};

const COLUMNS: [&str; 13] = [
    "input",
    "start",
    "end",
    "duration",
    "name",
    "mode",
    "output",
    "upscale_preset",
    "upscale_reframe",
    "upscale_tier",
    "upscale_hdr",
    "upscale_motion",
    "upscale_crf",
];

/// Columns a plain 001 manifest has; the `upscale_*` ones are written only when needed.
const CLIP_COLUMNS: usize = 7;

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
    // `start` may be left out when the rows can be whole videos (upscaling).
    let can_be_whole = cli.upscale.is_some() || headers.iter().any(|h| h.starts_with("upscale_"));
    for required in ["input", "start"] {
        let optional = required == "start" && can_be_whole;
        if !optional && !headers.iter().any(|h| h == required) {
            issues.push(issue(path, header_line, format!("missing required column \"{required}\"")));
        }
    }
    if !issues.is_empty() {
        return Err(issues);
    }
    let col = |name: &str| headers.iter().position(|h| h == name);
    let (c_input, c_start) = (col("input").expect("checked"), col("start"));
    let (c_end, c_dur, c_name, c_mode, c_out) =
        (col("end"), col("duration"), col("name"), col("mode"), col("output"));
    let c_up: Vec<Option<usize>> = COLUMNS[CLIP_COLUMNS..].iter().map(|c| col(c)).collect();

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
        let upscale = upscale_cells(&c_up.iter().map(|&c| get(c)).collect::<Vec<_>>(), &mut row_issues);
        // A row with upscaling on and no times is the whole video.
        let upscaling = PartialOptions { upscale: upscale.clone(), ..PartialOptions::default() }
            .apply(cli)
            .upscale
            .is_some();
        let no_times = get(c_start).is_none() && get(c_end).is_none() && get(c_dur).is_none();
        let start = match get(c_start).map(Ts::parse) {
            Some(Ok(t)) => Some(t),
            Some(Err(e)) => {
                row_issues.push(format!("start: {e}"));
                None
            }
            None if no_times && upscaling => Some(Ts::ZERO),
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
                upscale,
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

/// The lowercase word an option enum serializes to (`blur`, `cpu`, …).
fn word<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

/// The row's `upscale_*` cells (in `COLUMNS` order) as one layer: any non-empty cell turns
/// upscaling on, `upscale_preset=off` turns it off. `None` = all empty (inherited).
fn upscale_cells(cells: &[Option<&str>], issues: &mut Vec<String>) -> Option<PartialUpscale> {
    if cells.iter().all(Option::is_none) {
        return None;
    }
    fn choice<T: serde::de::DeserializeOwned>(
        col: &str,
        v: Option<&str>,
        allowed: &str,
        issues: &mut Vec<String>,
    ) -> Option<T> {
        let v = v?;
        serde_json::from_value(serde_json::Value::String(v.to_ascii_lowercase()))
            .map_err(|_| issues.push(format!("{col} must be one of {allowed}, not {v:?}")))
            .ok()
    }
    let [preset, reframe, tier, hdr, motion, crf] = cells else { unreachable!("six upscale columns") };
    if preset.is_some_and(|p| p.eq_ignore_ascii_case("off")) {
        if cells[1..].iter().any(Option::is_some) {
            issues.push("upscale_preset=off cannot be combined with other upscale_* values".into());
        }
        return Some(PartialUpscale { enabled: Some(false), ..PartialUpscale::default() });
    }
    let crf = crf.map(|c| match c.parse::<u8>() {
        Ok(n) if n <= MAX_CRF => Some(n),
        _ => {
            issues.push(format!("upscale_crf must be a whole number from 0 to {MAX_CRF}, not {c:?}"));
            None
        }
    });
    Some(PartialUpscale {
        enabled: None,
        preset: preset.map(str::to_owned),
        reframe: choice("upscale_reframe", *reframe, "smart, center, blur", issues),
        tier: choice("upscale_tier", *tier, "auto, cpu, gpu, ai", issues),
        hdr: choice("upscale_hdr", *hdr, "sdr, keep", issues),
        motion: choice("upscale_motion", *motion, "calm, normal, fast", issues),
        crf,
    })
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
        code: crate::error::IssueCode::InvalidInput,
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
    let resolved = |s: &SourceRequest, c: &ClipRequest| c.overrides.apply(&s.overrides.apply(&req.defaults));
    let upscaled = req.sources.iter().any(|s| s.clips.iter().any(|c| resolved(s, c).upscale.is_some()));
    let columns = if upscaled { &COLUMNS[..] } else { &COLUMNS[..CLIP_COLUMNS] };
    let mut w = csv::Writer::from_writer(Vec::new());
    let _ = w.write_record(columns);
    let text = |t: Ts| match time_value(t) {
        serde_json::Value::String(s) => s,
        v => v.to_string(),
    };
    for s in &req.sources {
        for c in &s.clips {
            let o = resolved(s, c);
            let (end, dur) = match c.end {
                EndSpec::At(t) => (text(t), String::new()),
                EndSpec::Duration(d) => (String::new(), text(d)),
                EndSpec::EndOfMedia => ("end".into(), String::new()),
            };
            let mode = match o.mode {
                Mode::Fast => "fast",
                Mode::Accurate => "accurate",
            };
            let mut row = vec![
                s.path.to_string_lossy().into_owned(),
                text(c.start),
                end,
                dur,
                c.name.clone().unwrap_or_default(),
                mode.to_owned(),
                o.output_dir.as_deref().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
            ];
            if upscaled {
                match &o.upscale {
                    // Written in full so the row does not depend on the defaults it is read with
                    // (except an unset crf: CSV cannot say "the preset's", so it is inherited).
                    Some(u) => row.extend([
                        u.preset.clone(),
                        word(&u.reframe),
                        word(&u.tier),
                        word(&u.hdr),
                        word(&u.motion),
                        u.crf.map(|c| c.to_string()).unwrap_or_default(),
                    ]),
                    None => {
                        row.extend(["off".to_owned(), "".into(), "".into(), "".into(), "".into(), "".into()])
                    }
                }
            }
            let _ = w.write_record(&row);
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

    fn up(req: &BatchRequest, v: usize, c: usize) -> Option<crate::upscale::UpscaleOptions> {
        let src = &req.sources[v];
        src.clips[c].overrides.apply(&src.overrides.apply(&req.defaults)).upscale
    }

    /// `upscale_*` columns, `off`, whole-video rows (T075).
    #[test]
    fn upscale_columns() {
        use crate::upscale::{Reframe, TierChoice};
        let text = "input,start,end,upscale_preset,upscale_reframe,upscale_tier,upscale_crf\n\
                    low.mp4,,,phone-fill,,,\n\
                    land.mp4,1,5,reddit-916,BLUR,cpu,20\n\
                    land.mp4,6,8,,,,\n\
                    slides.mp4,,,,center,,\n";
        let req = parse(Path::new("/m/j.csv"), text, &Options::default()).unwrap();
        let low = &req.sources[0].clips[0];
        assert_eq!((low.start, low.end), (Ts::ZERO, EndSpec::EndOfMedia), "whole video");
        assert_eq!(up(&req, 0, 0).unwrap().preset, "phone-fill");
        let land = up(&req, 1, 0).unwrap();
        assert_eq!(
            (land.preset.as_str(), land.reframe, land.tier, land.crf),
            ("reddit-916", Reframe::Blur, TierChoice::Cpu, Some(20))
        );
        assert_eq!(up(&req, 1, 1), None, "a row without upscale cells is a plain clip");
        assert_eq!(up(&req, 2, 0).unwrap().reframe, Reframe::Center);
        // Turned on from the command line, `off` turns a row back to a plain clip.
        let cli = Options { upscale: Some(crate::upscale::UpscaleOptions::default()), ..Options::default() };
        let req =
            parse(Path::new("j.csv"), "input,start,end,upscale_preset\na.mp4,1,2,off\nb.mp4,,,\n", &cli)
                .unwrap();
        assert_eq!(up(&req, 0, 0), None);
        assert_eq!(up(&req, 1, 0).unwrap().preset, "phone-fill");
        // `start` may be left out entirely when rows can be whole videos.
        let req =
            parse(Path::new("j.csv"), "input,upscale_preset\na.mp4,duo\n", &Options::default()).unwrap();
        assert_eq!(req.sources[0].clips[0].end, EndSpec::EndOfMedia);
    }

    #[test]
    fn upscale_cell_errors_are_reported_by_line() {
        let text = "input,start,end,upscale_preset,upscale_reframe,upscale_tier,upscale_hdr,upscale_motion,upscale_crf\n\
                    a.mp4,1,2,,wide,,,,\n\
                    a.mp4,1,2,,,quantum,,,\n\
                    a.mp4,1,2,,,,dolby,,\n\
                    a.mp4,1,2,,,,,warp,\n\
                    a.mp4,1,2,,,,,,52\n\
                    a.mp4,1,2,off,blur,,,,\n\
                    a.mp4,,,,,,,,\n\
                    a.mp4,,5,duo,,,,,\n";
        let err = parse(Path::new("j.csv"), text, &Options::default()).unwrap_err();
        let at: Vec<(usize, &str)> = err
            .iter()
            .map(|i| match i.origin {
                Origin::Line { line, .. } => (line, i.message.as_str()),
                _ => (0, ""),
            })
            .collect();
        let lines: Vec<usize> = at.iter().map(|a| a.0).collect();
        assert_eq!(lines, [2, 3, 4, 5, 6, 7, 8, 9], "{at:?}");
        assert!(at[0].1.contains("upscale_reframe must be one of smart, center, blur"), "{}", at[0].1);
        assert!(at[4].1.contains("0 to 51"), "{}", at[4].1);
        assert!(at[6].1.contains("start is empty"), "a plain row still needs a start: {}", at[6].1);
        assert!(
            at[7].1.contains("start is empty"),
            "an end without a start is not a whole video: {}",
            at[7].1
        );
        // Unknown columns are still rejected, and `start` is required without upscale columns.
        assert!(
            parse(Path::new("j.csv"), "input,start,upscale_scale\na,1,2\n", &Options::default()).is_err()
        );
        assert!(parse(Path::new("j.csv"), "input\na.mp4\n", &Options::default()).is_err());
    }

    #[test]
    fn upscale_rows_round_trip() {
        let text = "input,start,end,upscale_preset,upscale_tier,upscale_crf\n/v/a.mp4,,,duo,cpu,18\n/v/b.mp4,1,2,,,\n";
        let req = parse(Path::new("/m/j.csv"), text, &Options::default()).unwrap();
        let written = write(&req);
        assert!(
            written.starts_with("input,start,end,duration,name,mode,output,upscale_preset,"),
            "{written}"
        );
        let again = parse(Path::new("/m/j.csv"), &written, &Options::default()).unwrap();
        assert_eq!((up(&again, 0, 0), up(&again, 1, 0)), (up(&req, 0, 0), up(&req, 1, 0)));
        for (a, b) in req.sources.iter().zip(&again.sources) {
            assert_eq!((a.clips[0].start, a.clips[0].end), (b.clips[0].start, b.clips[0].end));
        }
        // Even when read with upscaling on from the command line.
        let cli = Options { upscale: Some(crate::upscale::UpscaleOptions::default()), ..Options::default() };
        assert_eq!(up(&parse(Path::new("/m/j.csv"), &written, &cli).unwrap(), 1, 0), None);
        // A plain manifest keeps the 001 columns.
        let plain = parse(Path::new("/m/j.csv"), "input,start\n/v/a.mp4,1\n", &Options::default()).unwrap();
        assert!(write(&plain).starts_with("input,start,end,duration,name,mode,output\n"));
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
