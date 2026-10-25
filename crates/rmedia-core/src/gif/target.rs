//! GIF targets: named, versioned presets as data (research R2, R13; contracts/gif-target.schema.json).
//!
//! The built-ins are compiled in. User targets live in `<config>/gif-targets/*.json` and replace
//! a built-in with the same id ("user override"). User files are untrusted: they are read with
//! the device-preset protections (regular files only, at most 64 KiB), at most 64 of them, and
//! every rule of the schema is checked again in code.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::preset::{PresetProblem, PresetSource, read_preset_file};

/// The default target.
pub const DEFAULT_TARGET: &str = "reddit-post";
/// The largest size budget a target may set (1 GiB).
pub const MAX_BUDGET: u64 = 1 << 30;
/// At most this many user target files are read; the rest are reported.
const MAX_USER_FILES: usize = 64;

const BUILTIN: [&str; 4] = [
    include_str!("builtin/reddit-post.json"),
    include_str!("builtin/reddit-comment.json"),
    include_str!("builtin/max-quality.json"),
    include_str!("builtin/flat-ui.json"),
];

/// A size budget in bytes: the recommended size and the size never written above (FR-011).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifBudget"))]
pub struct Budget {
    pub soft: u64,
    pub hard: u64,
}

/// Output widths by source orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifWidths"))]
pub struct Widths {
    pub landscape: u32,
    pub portrait: u32,
}

/// Palette dithering (`paletteuse`). The budget ladder lowers error diffusion → bayer 4 →
/// bayer 5; `none` is never lowered further.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Dither {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "floyd_steinberg")]
    FloydSteinberg,
    #[serde(rename = "sierra2_4a")]
    Sierra2_4a,
    /// Ordered dithering with `bayer_scale` 0–5 (higher: less pattern, smaller files).
    #[serde(rename = "bayer")]
    Bayer(u8),
}

impl Dither {
    /// Parses a CLI value, the inverse of [`Dither::label`]: `none`, `bayer` (ffmpeg's default
    /// scale, 2), `bayer:S` (S 0..=5), `floyd-steinberg` or `sierra`.
    pub fn parse(text: &str) -> Result<Dither, String> {
        let bad = || format!("unknown dither {text:?} (none, bayer, bayer:0..5, floyd-steinberg, sierra)");
        match text.trim().to_ascii_lowercase().as_str() {
            "none" => Ok(Dither::None),
            "floyd-steinberg" | "floyd_steinberg" => Ok(Dither::FloydSteinberg),
            "sierra" | "sierra2_4a" => Ok(Dither::Sierra2_4a),
            "bayer" => Ok(Dither::Bayer(2)),
            t => match t.strip_prefix("bayer:").map(str::parse::<u8>) {
                Some(Ok(scale @ 0..=5)) => Ok(Dither::Bayer(scale)),
                _ => Err(bad()),
            },
        }
    }

    /// `paletteuse` options.
    pub fn paletteuse(self) -> String {
        match self {
            Dither::None => "dither=none".into(),
            Dither::FloydSteinberg => "dither=floyd_steinberg".into(),
            Dither::Sierra2_4a => "dither=sierra2_4a".into(),
            Dither::Bayer(s) => format!("dither=bayer:bayer_scale={s}"),
        }
    }

    /// How it is written in messages and on the command line.
    pub fn label(self) -> String {
        match self {
            Dither::None => "none".into(),
            Dither::FloydSteinberg => "floyd-steinberg".into(),
            Dither::Sierra2_4a => "sierra".into(),
            Dither::Bayer(s) => format!("bayer:{s}"),
        }
    }
}

/// `palettegen` statistics: the whole frame, or only what changes (smaller, sharper motion).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum StatsMode {
    Full,
    Diff,
}

/// The engine a target prefers: the built-in palette pipeline, or the best available (gifski,
/// falling back to the built-in engine with `ENGINE_FALLBACK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum TargetEngine {
    Builtin,
    Best,
}

/// gifski parameters (research R14), 1–100 each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct GifskiParams {
    pub quality: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub motion_quality: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub lossy_quality: Option<u8>,
}

/// Reddit MP4 (silent loop) defaults of a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Mp4Params {
    pub crf: u8,
    pub max_fps: u32,
    /// GIF jobs of this target estimate the MP4's size too ("the MP4 would be smaller", US3).
    #[serde(default)]
    pub hint: bool,
}

/// A GIF target (data-model.md › GifTarget).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct GifTarget {
    pub schema: u32,
    pub id: String,
    pub version: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub description: Option<String>,
    /// `None`: no budget search.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub budget: Option<Budget>,
    /// `false` until the owner confirms the limits (FR-007).
    pub verified: bool,
    pub width: Widths,
    /// Lowest width the budget search may reach (`None`: width is not lowered).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub floor_width: Option<u32>,
    pub fps: u32,
    /// Lowest frame rate the budget search may reach (`None`: fps is not lowered).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub floor_fps: Option<u32>,
    pub colours: u16,
    pub dither: Dither,
    pub stats_mode: StatsMode,
    /// `unsharp` luma amount after the resample.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub sharpen: Option<f32>,
    pub engine: TargetEngine,
    /// Default gifsicle lossy level; the budget ladder starts here (`None` = 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub lossy: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub gifski: Option<GifskiParams>,
    /// Seconds; longer ranges warn before encoding (`LONG_RANGE`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub max_duration: Option<f64>,
    pub mp4: Mp4Params,
}

/// Characters that could rewrite or reorder a terminal line: controls, and bidi overrides,
/// embeddings and isolates.
pub(crate) fn unsafe_char(c: char) -> bool {
    c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}')
}

/// Printable text (see [`unsafe_char`]).
fn printable(s: &str) -> bool {
    !s.chars().any(unsafe_char)
}

/// `s` with unsafe characters replaced and at most 300 characters (problem messages quote
/// file names and parser errors from untrusted files).
fn shown(s: &str) -> String {
    s.chars().take(300).map(|c| if unsafe_char(c) { '?' } else { c }).collect()
}

impl GifTarget {
    /// Parses and validates a target file.
    pub fn from_json(text: &str) -> Result<GifTarget, String> {
        let t: GifTarget = serde_json::from_str(text).map_err(|e| e.to_string())?;
        t.validate()?;
        Ok(t)
    }

    /// The schema's rules plus the cross-field rules it cannot express (research R13), so
    /// built-in and user files are checked the same way at runtime.
    pub fn validate(&self) -> Result<(), String> {
        let id_ok = !self.id.is_empty()
            && self.id.len() <= 40
            && self.id.split('-').all(|part| {
                !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            });
        let widths = [self.width.landscape, self.width.portrait];
        let in_100 = |v: Option<u8>| v.is_none_or(|q| (1..=100).contains(&q));
        let checks: Vec<(bool, String)> = vec![
            (self.schema == 1, format!("unsupported GIF target schema {}", self.schema)),
            (
                id_ok,
                format!(
                    "id {:?} must be kebab-case (a-z, 0-9, single dashes), at most 40 characters",
                    self.id
                ),
            ),
            (self.version >= 1, "version must be at least 1".into()),
            (
                !self.name.trim().is_empty() && self.name.chars().count() <= 80 && printable(&self.name),
                "name must be 1 to 80 printable characters".into(),
            ),
            (
                self.description.as_deref().is_none_or(|d| d.chars().count() <= 400 && printable(d)),
                "description must be at most 400 printable characters".into(),
            ),
            (
                self.budget.is_none_or(|b| b.soft >= 1 && b.soft <= b.hard && b.hard <= MAX_BUDGET),
                format!("budget must have 1 ≤ soft ≤ hard ≤ {MAX_BUDGET} bytes"),
            ),
            (
                widths.iter().all(|w| (64..=1920).contains(w)),
                "width.landscape and width.portrait must be within 64..1920".into(),
            ),
            (
                self.floor_width.is_none_or(|f| f >= 64 && widths.iter().all(|w| f <= *w)),
                "floor_width must be at least 64 and at most both widths".into(),
            ),
            (
                (1..=crate::gif::timing::MAX_FPS).contains(&self.fps),
                format!("fps {} must be within 1..50", self.fps),
            ),
            (
                self.floor_fps.is_none_or(|f| f >= 1 && f <= self.fps),
                "floor_fps must be at least 1 and at most fps".into(),
            ),
            ((2..=256).contains(&self.colours), format!("colours {} must be within 2..256", self.colours)),
            (
                !matches!(self.dither, Dither::Bayer(s) if s > 5),
                "dither bayer scale must be within 0..5".into(),
            ),
            (
                self.sharpen.is_none_or(|a| a.is_finite() && a > 0.0 && a <= 1.5),
                "sharpen must be within (0, 1.5]".into(),
            ),
            (self.lossy.is_none_or(|l| l <= 200), "lossy must be within 0..200".into()),
            (
                self.gifski.is_none_or(|g| {
                    (1..=100).contains(&g.quality) && in_100(g.motion_quality) && in_100(g.lossy_quality)
                }),
                "gifski quality values must be within 1..100".into(),
            ),
            (
                self.max_duration.is_none_or(|d| d.is_finite() && d > 0.0 && d <= 3600.0),
                "max_duration must be within (0, 3600] seconds".into(),
            ),
            (self.mp4.crf <= 51, format!("mp4.crf {} must be within 0..51", self.mp4.crf)),
            (
                (1..=60).contains(&self.mp4.max_fps),
                format!("mp4.max_fps {} must be within 1..60", self.mp4.max_fps),
            ),
        ];
        match checks.into_iter().find(|(ok, _)| !ok) {
            Some((_, msg)) => Err(msg),
            None => Ok(()),
        }
    }

    /// The output width for a source of this displayed size (portrait sources are taller).
    pub fn width_for(&self, display: (u32, u32)) -> u32 {
        if display.1 > display.0 { self.width.portrait } else { self.width.landscape }
    }
}

/// Built-in targets plus the user's, in a stable order: built-ins first (replaced in place by a
/// user override), then user-only targets by id.
#[derive(Debug, Clone)]
pub struct GifTargetRegistry {
    targets: Vec<(GifTarget, PresetSource)>,
    problems: Vec<PresetProblem>,
}

impl GifTargetRegistry {
    /// Built-ins only.
    pub fn builtin() -> GifTargetRegistry {
        let targets = BUILTIN
            .iter()
            .map(|text| {
                (GifTarget::from_json(text).expect("built-in GIF targets are valid"), PresetSource::Builtin)
            })
            .collect();
        GifTargetRegistry { targets, problems: Vec::new() }
    }

    /// Built-ins plus `<config_dir>/gif-targets/*.json` (when `config_dir` is given). Problems
    /// with user files are collected, never fatal.
    pub fn load(config_dir: Option<&Path>) -> GifTargetRegistry {
        let mut reg = GifTargetRegistry::builtin();
        let Some(dir) = config_dir.map(|d| d.join("gif-targets")) else { return reg };
        let Ok(entries) = std::fs::read_dir(&dir) else { return reg };
        // A folder with a huge number of files is not listed in full.
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("json")))
            .take(MAX_USER_FILES * 4)
            .collect();
        files.sort();
        if files.len() > MAX_USER_FILES {
            reg.problems.push(PresetProblem {
                file: dir.clone(),
                message: format!(
                    "{}{} files ignored: at most {MAX_USER_FILES} GIF target files are read",
                    files.len() - MAX_USER_FILES,
                    if files.len() == MAX_USER_FILES * 4 { " or more" } else { "" }
                ),
            });
            files.truncate(MAX_USER_FILES);
        }
        let mut user: Vec<(GifTarget, PresetSource)> = Vec::new();
        for file in files {
            match read_preset_file(&file).and_then(|t| GifTarget::from_json(&t)).map_err(|e| shown(&e)) {
                Ok(t) if user.iter().any(|(u, _)| u.id == t.id) => reg.problems.push(PresetProblem {
                    file,
                    message: format!("another user GIF target already uses the id {:?}", t.id),
                }),
                Ok(t) => user.push((t, PresetSource::User(file))),
                Err(message) => reg.problems.push(PresetProblem { file, message }),
            }
        }
        user.sort_by(|a, b| a.0.id.cmp(&b.0.id));
        for (t, src) in user {
            match reg.targets.iter_mut().find(|(b, _)| b.id == t.id) {
                Some(slot) => *slot = (t, src),
                None => reg.targets.push((t, src)),
            }
        }
        reg
    }

    pub fn get(&self, id: &str) -> Option<&GifTarget> {
        self.targets.iter().find(|(t, _)| t.id == id).map(|(t, _)| t)
    }

    pub fn list(&self) -> impl Iterator<Item = (&GifTarget, &PresetSource)> {
        self.targets.iter().map(|(t, s)| (t, s))
    }

    pub fn ids(&self) -> Vec<&str> {
        self.targets.iter().map(|(t, _)| t.id.as_str()).collect()
    }

    pub fn problems(&self) -> &[PresetProblem] {
        &self.problems
    }

    /// "built-in", "user" or "user override" (a user file replacing a built-in id).
    pub fn label(&self, id: &str) -> Option<&'static str> {
        let (_, src) = self.targets.iter().find(|(t, _)| t.id == id)?;
        Some(match src {
            PresetSource::Builtin => "built-in",
            PresetSource::User(_) if GifTargetRegistry::builtin().get(id).is_some() => "user override",
            PresetSource::User(_) => "user",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_validator() -> jsonschema::Validator {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../specs/003-gif-export/contracts/gif-target.schema.json");
        let schema: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        jsonschema::validator_for(&schema).unwrap()
    }

    fn reddit_post() -> serde_json::Value {
        serde_json::from_str(BUILTIN[0]).unwrap()
    }

    #[test]
    fn built_ins_match_the_contract_schema_and_research() {
        let v = schema_validator();
        for text in BUILTIN {
            let json: serde_json::Value = serde_json::from_str(text).unwrap();
            let errors: Vec<String> = v.iter_errors(&json).map(|e| e.to_string()).collect();
            assert!(errors.is_empty(), "{}: {errors:?}", json["id"]);
        }
        let reg = GifTargetRegistry::builtin();
        assert_eq!(reg.ids(), ["reddit-post", "reddit-comment", "max-quality", "flat-ui"]);
        let post = reg.get(DEFAULT_TARGET).unwrap();
        assert_eq!(post.budget, Some(Budget { soft: 8_000_000, hard: 20_000_000 }));
        assert!(!post.verified, "Reddit limits ship unverified (FR-007)");
        assert_eq!((post.width.landscape, post.width.portrait, post.floor_width), (720, 480, Some(320)));
        assert_eq!(
            (post.fps, post.floor_fps, post.colours, post.dither),
            (15, Some(10), 256, Dither::Bayer(4))
        );
        assert_eq!((post.lossy, post.engine), (Some(30), TargetEngine::Builtin));
        let comment = reg.get("reddit-comment").unwrap();
        assert_eq!(comment.budget, Some(Budget { soft: 3_000_000, hard: 8_000_000 }));
        assert_eq!((comment.width.landscape, comment.fps, comment.colours), (480, 12, 128));
        let best = reg.get("max-quality").unwrap();
        assert_eq!(
            (best.budget, best.engine, best.dither),
            (None, TargetEngine::Best, Dither::FloydSteinberg)
        );
        assert_eq!(best.gifski.map(|g| g.quality), Some(100));
        let flat = reg.get("flat-ui").unwrap();
        assert_eq!((flat.colours, flat.dither, flat.sharpen), (64, Dither::None, Some(0.5)));
        assert_eq!(reg.label("reddit-post"), Some("built-in"));
    }

    #[test]
    fn dither_serialises_as_in_the_schema() {
        assert_eq!(serde_json::to_string(&Dither::Bayer(4)).unwrap(), r#"{"bayer":4}"#);
        assert_eq!(serde_json::to_string(&Dither::FloydSteinberg).unwrap(), r#""floyd_steinberg""#);
        assert_eq!(serde_json::from_str::<Dither>(r#""sierra2_4a""#).unwrap(), Dither::Sierra2_4a);
        assert_eq!(Dither::Bayer(5).paletteuse(), "dither=bayer:bayer_scale=5");
        assert_eq!(Dither::None.label(), "none");
    }

    #[test]
    fn invalid_targets_are_rejected_with_a_reason() {
        let bad = |edit: &dyn Fn(&mut serde_json::Value), needle: &str| {
            let mut j = reddit_post();
            edit(&mut j);
            let err = GifTarget::from_json(&j.to_string()).unwrap_err();
            assert!(err.contains(needle), "{err} (wanted {needle})");
        };
        bad(&|j| j["budget"]["soft"] = 30_000_000.into(), "soft ≤ hard");
        bad(&|j| j["budget"]["hard"] = (MAX_BUDGET + 1).into(), "soft ≤ hard");
        bad(&|j| j["floor_width"] = 800.into(), "floor_width");
        bad(&|j| j["floor_fps"] = 20.into(), "floor_fps");
        bad(&|j| j["fps"] = 51.into(), "fps 51");
        bad(&|j| j["name"] = "Bad\u{1b}[2Jname".into(), "printable");
        bad(&|j| j["description"] = "line\nbreak".into(), "printable");
        bad(&|j| j["id"] = "Not_Kebab".into(), "kebab-case");
        bad(&|j| j["dither"] = serde_json::json!({"bayer": 6}), "bayer");
        bad(&|j| j["colours"] = 300.into(), "colours");
        bad(&|j| j["extra"] = 1.into(), "unknown field");
        bad(&|j| j["name"] = "evil\u{202e}txt.exe".into(), "printable");
        bad(&|j| j["max_duration"] = 1e9.into(), "max_duration");
        assert_eq!(shown("a\u{1b}[2Jb\u{202e}c"), "a?[2Jb?c");
    }

    fn user_dir(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("gif-targets")).unwrap();
        for (name, text) in files {
            std::fs::write(dir.path().join("gif-targets").join(name), text).unwrap();
        }
        dir
    }

    #[test]
    fn user_targets_override_by_id_and_bad_files_are_problems() {
        let mut over = reddit_post();
        over["budget"] = serde_json::json!({"soft": 5_000_000, "hard": 10_000_000});
        let mut own = reddit_post();
        own["id"] = "my-target".into();
        let dir = user_dir(&[
            ("a-override.json", &over.to_string()),
            ("b-own.json", &own.to_string()),
            ("c-broken.json", "{ not json"),
            ("d-dup.json", &own.to_string()),
            ("notes.txt", "ignored"),
        ]);
        let reg = GifTargetRegistry::load(Some(dir.path()));
        assert_eq!(reg.ids(), ["reddit-post", "reddit-comment", "max-quality", "flat-ui", "my-target"]);
        assert_eq!(reg.get("reddit-post").unwrap().budget.unwrap().soft, 5_000_000);
        assert_eq!(reg.label("reddit-post"), Some("user override"));
        assert_eq!(reg.label("my-target"), Some("user"));
        let problems: Vec<String> = reg
            .problems()
            .iter()
            .map(|p| p.file.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(problems, ["c-broken.json", "d-dup.json"]);
        assert!(GifTargetRegistry::load(None).problems().is_empty());
    }

    #[test]
    fn oversized_symlinked_and_excess_files_are_refused() {
        let big = format!("{{\"pad\":\"{}\"}}", "x".repeat(70 * 1024));
        let dir = user_dir(&[("big.json", &big)]);
        let reg = GifTargetRegistry::load(Some(dir.path()));
        assert!(reg.problems()[0].message.contains("larger than 64 KiB"), "{:?}", reg.problems());

        #[cfg(unix)]
        {
            let dir = user_dir(&[]);
            let real = dir.path().join("real.txt");
            std::fs::write(&real, reddit_post().to_string()).unwrap();
            std::os::unix::fs::symlink(&real, dir.path().join("gif-targets/link.json")).unwrap();
            let reg = GifTargetRegistry::load(Some(dir.path()));
            assert!(reg.problems()[0].message.contains("symlinks"), "{:?}", reg.problems());
        }

        let names: Vec<String> = (0..65).map(|i| format!("t{i:02}.json")).collect();
        let files: Vec<(String, String)> = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let mut t = reddit_post();
                t["id"] = format!("user-{i}").into();
                (n.clone(), t.to_string())
            })
            .collect();
        let refs: Vec<(&str, &str)> = files.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
        let dir = user_dir(&refs);
        let reg = GifTargetRegistry::load(Some(dir.path()));
        assert_eq!(reg.ids().len(), 4 + 64);
        assert_eq!(reg.problems().len(), 1);
        assert!(reg.problems()[0].message.starts_with("1 files ignored: at most 64"), "{:?}", reg.problems());
    }

    #[test]
    fn width_follows_the_source_orientation() {
        let reg = GifTargetRegistry::builtin();
        let t = reg.get("reddit-post").unwrap();
        assert_eq!(t.width_for((1280, 720)), 720);
        assert_eq!(t.width_for((720, 1280)), 480);
        assert_eq!(t.width_for((720, 720)), 720, "square counts as landscape");
    }
}
