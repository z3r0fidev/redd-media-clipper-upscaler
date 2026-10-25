//! Output naming templates and cross-platform file name sanitizing (FR-016).
//!
//! Placeholders: `{source}` `{index}` `{index:0N}` `{name}` `{name-or-start}` `{name-or-range}`
//! `{start}` `{end}` `{range}` (`{start}_{end}`) `{ext}`, `{preset}` for upscale jobs, and
//! `{target}` and `{format}` for GIF jobs. A `/` in the template creates a sub-folder of the
//! output directory.

use std::path::PathBuf;

use crate::time::Ts;

#[derive(Debug, Clone)]
pub struct NameVars<'a> {
    pub source_stem: &'a str,
    /// 1-based index of the clip within its source.
    pub index: usize,
    pub name: Option<&'a str>,
    pub start: Ts,
    pub end: Ts,
    pub ext: &'a str,
    /// Upscale jobs: the device preset id. `{preset}` is unknown for clip jobs.
    pub preset: Option<&'a str>,
    /// GIF jobs: the GIF target id and the format (`gif`/`mp4`); unknown for other jobs.
    pub target: Option<&'a str>,
    pub format: Option<&'a str>,
}

/// Renders a template into a relative path. Errors describe the problem for the user.
pub fn render(template: &str, vars: &NameVars<'_>) -> Result<PathBuf, String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close =
            after.find('}').ok_or_else(|| format!("unclosed '{{' in name template \"{template}\""))?;
        let key = &after[..close];
        out.push_str(
            &expand(key, vars)
                .ok_or_else(|| format!("unknown placeholder {{{key}}} in name template \"{template}\""))?,
        );
        rest = &after[close + 1..];
    }
    out.push_str(rest);

    let mut path = PathBuf::new();
    for raw in out.split(['/', '\\']) {
        if raw.is_empty() || raw == "." || raw == ".." {
            return Err(format!(
                "name template \"{template}\" must produce a relative path without empty, '.' or '..' parts"
            ));
        }
        path.push(limit_len(&sanitize_component(raw)));
    }
    Ok(path)
}

fn expand(key: &str, v: &NameVars<'_>) -> Option<String> {
    let clean = |s: &str| sanitize_value(s);
    Some(match key {
        "source" => clean(v.source_stem),
        "index" => v.index.to_string(),
        "name" => clean(v.name.unwrap_or("")),
        "name-or-start" => match v.name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => clean(n),
            None => v.start.filename(),
        },
        "name-or-range" => match v.name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => clean(n),
            None => format!("{}_{}", v.start.filename(), v.end.filename()),
        },
        "range" => format!("{}_{}", v.start.filename(), v.end.filename()),
        "start" => v.start.filename(),
        "end" => v.end.filename(),
        "ext" => clean(v.ext),
        "preset" => clean(v.preset?),
        "target" => clean(v.target?),
        "format" => clean(v.format?),
        _ => {
            let width = key.strip_prefix("index:0")?;
            let width: usize = width.parse().ok().filter(|w| (1..=9).contains(w))?;
            format!("{:0width$}", v.index)
        }
    })
}

/// Placeholder values may not introduce path separators.
fn sanitize_value(s: &str) -> String {
    s.chars().map(|c| if c == '/' || c == '\\' { '_' } else { c }).collect()
}

/// Most filesystems allow 255 bytes per name; leave room for `_N` collision suffixes.
const MAX_COMPONENT_BYTES: usize = 240;

fn limit_len(name: &str) -> String {
    if name.len() <= MAX_COMPONENT_BYTES {
        return name.to_owned();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && e.len() <= 16 => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    let mut cut = MAX_COMPONENT_BYTES - ext.len();
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{ext}", stem[..cut].trim_end_matches([' ', '.']))
}

const RESERVED: [&str; 24] = [
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
    "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn is_reserved(stem: &str) -> bool {
    let stem = stem.trim_end_matches([' ', '.']);
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return true;
    }
    // COM¹ COM² COM³ / LPT¹ LPT² LPT³ are reserved too.
    let upper = stem.to_ascii_uppercase();
    ["COM", "LPT"]
        .iter()
        .any(|p| upper.strip_prefix(p).is_some_and(|rest| matches!(rest, "\u{b9}" | "\u{b2}" | "\u{b3}")))
}

/// Makes one path component valid on Windows and Linux.
pub fn sanitize_component(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    while out.ends_with(['.', ' ']) {
        out.pop();
    }
    let out = out.trim_start().to_owned();
    if out.is_empty() {
        return "_".to_owned();
    }
    let stem = out.split('.').next().unwrap_or("");
    if is_reserved(stem) {
        return format!("_{out}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars<'a>(name: Option<&'a str>) -> NameVars<'a> {
        NameVars {
            source_stem: "match 12",
            index: 7,
            name,
            start: Ts::from_secs_f64(90.5),
            end: Ts::from_secs_f64(120.0),
            ext: "mkv",
            preset: None,
            target: None,
            format: None,
        }
    }

    #[test]
    fn target_and_format_placeholders_only_for_gif_jobs() {
        let t = crate::request::GIF_NAME_TEMPLATE;
        assert!(render(t, &vars(None)).is_err(), "{{target}} is unknown for clips");
        let gif = NameVars { target: Some("reddit-post"), format: Some("gif"), ext: "gif", ..vars(None) };
        assert_eq!(render(t, &gif).unwrap(), PathBuf::from("match 12_reddit-post.gif"));
        let clip = crate::request::GIF_CLIP_NAME_TEMPLATE;
        assert_eq!(
            render(clip, &gif).unwrap(),
            PathBuf::from("match 12_007_00-01-30.500_00-02-00.000_reddit-post.gif")
        );
        assert_eq!(render("{source}.{format}", &gif).unwrap(), PathBuf::from("match 12.gif"));
    }

    #[test]
    fn preset_placeholder_only_for_upscale_jobs() {
        let t = crate::request::UPSCALE_NAME_TEMPLATE;
        assert!(render(t, &vars(None)).is_err(), "{{preset}} is unknown for clips");
        let up = NameVars { preset: Some("phone-fill"), ext: "mp4", ..vars(None) };
        assert_eq!(render(t, &up).unwrap(), PathBuf::from("match 12_phone-fill.mp4"));
        let clip = crate::request::UPSCALE_CLIP_NAME_TEMPLATE;
        assert_eq!(
            render(clip, &NameVars { name: Some("goal"), ..up }).unwrap(),
            PathBuf::from("match 12_007_goal_phone-fill.mp4")
        );
    }

    #[test]
    fn renders_default_template() {
        let t = crate::request::DEFAULT_NAME_TEMPLATE;
        assert_eq!(
            render(t, &vars(None)).unwrap(),
            PathBuf::from("match 12_007_00-01-30.500_00-02-00.000.mkv")
        );
        assert_eq!(render(t, &vars(Some("goal: 1/2"))).unwrap(), PathBuf::from("match 12_007_goal_ 1_2.mkv"));
        assert_eq!(
            render(t, &vars(Some("   "))).unwrap(),
            PathBuf::from("match 12_007_00-01-30.500_00-02-00.000.mkv")
        );
        assert_eq!(
            render("{source}_{name-or-start}.{ext}", &vars(None)).unwrap(),
            PathBuf::from("match 12_00-01-30.500.mkv")
        );
    }

    #[test]
    fn renders_subfolders_and_widths() {
        let p = render("{source}/{index:05}-{start}-{end}.{ext}", &vars(None)).unwrap();
        assert_eq!(p, PathBuf::from("match 12").join("00007-00-01-30.500-00-02-00.000.mkv"));
        assert_eq!(render("{index}.{ext}", &vars(None)).unwrap(), PathBuf::from("7.mkv"));
    }

    #[test]
    fn rejects_bad_templates() {
        for t in
            ["{nope}.mp4", "{index", "../{source}.mp4", "/abs/{source}", "a//b", "{index:0}", "{index:010}"]
        {
            assert!(render(t, &vars(None)).is_err(), "{t}");
        }
    }

    #[test]
    fn limits_name_length() {
        let long = "x".repeat(300);
        let p = render("{name}.{ext}", &vars(Some(&long))).unwrap();
        let name = p.to_string_lossy().into_owned();
        assert!(name.len() <= 240 && name.ends_with(".mkv"), "{name}");
        let wide = "日".repeat(120); // 360 bytes
        let p = render("{name}.{ext}", &vars(Some(&wide))).unwrap();
        assert!(p.to_string_lossy().len() <= 240);
    }

    #[test]
    fn sanitizes_components() {
        assert_eq!(sanitize_component("a<b>c:d\"e|f?g*h"), "a_b_c_d_e_f_g_h");
        assert_eq!(sanitize_component("trailing. . "), "trailing");
        assert_eq!(sanitize_component("CON.mp4"), "_CON.mp4");
        assert_eq!(sanitize_component("lpt1"), "_lpt1");
        assert_eq!(sanitize_component("CON .mp4"), "_CON .mp4");
        assert_eq!(sanitize_component("conout$.mkv"), "_conout$.mkv");
        assert_eq!(sanitize_component("COM\u{b9}.mp4"), "_COM\u{b9}.mp4");
        assert_eq!(sanitize_component("console.mp4"), "console.mp4");
        assert_eq!(sanitize_component("tab\there"), "tab_here");
        assert_eq!(sanitize_component("..."), "_");
        assert_eq!(sanitize_component("ü ñ 日本"), "ü ñ 日本");
    }
}
