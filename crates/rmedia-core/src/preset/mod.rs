//! Device presets as data (FR-001, research R1): built-ins compiled in, plus user presets.
//!
//! A preset is a versioned JSON document (contracts/preset.schema.json). Built-ins ship inside
//! the binary; users add or override presets by `id` with files in `<config>/presets/*.json`.
//! Invalid user files are reported and skipped, never fatal.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::time::Ts;

/// The default output preset (owner decision: "phone fill" 1080x2340).
pub const DEFAULT_PRESET: &str = "phone-fill";

const BUILTIN: [&str; 7] = [
    include_str!("builtin/phone-fill.json"),
    include_str!("builtin/phone-fill-hd.json"),
    include_str!("builtin/pixel-tall.json"),
    include_str!("builtin/reddit-916.json"),
    include_str!("builtin/reddit-feed-45.json"),
    include_str!("builtin/fold-inner.json"),
    include_str!("builtin/duo.json"),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Preset {
    pub schema: u32,
    /// kebab-case, unique; a user file with the same id replaces the built-in.
    pub id: String,
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Device classes the preset targets, shown to users (FR-002).
    pub devices: Vec<String>,
    pub width: u32,
    pub height: u32,
    pub max_fps: u32,
    pub video: VideoFormat,
    pub audio: AudioFormat,
    #[serde(default)]
    pub hdr_capable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub limits: Option<Limits>,
    /// Bits per pixel per frame, for the output size estimate (research R12).
    pub est_bpp: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum VideoCodec {
    Hevc,
    H264,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct VideoFormat {
    pub codec: VideoCodec,
    pub crf: u8,
    /// x264/x265 speed preset.
    pub preset: String,
    pub profile: String,
    /// Codec level; omitted = computed from size and frame rate (research R2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub level: Option<String>,
    /// Container tag, e.g. `hvc1` (required by Apple players for HEVC).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum AudioCodec {
    Aac,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AudioFormat {
    pub codec: AudioCodec,
    pub bitrate_k: u32,
    /// Copy AAC source audio instead of re-encoding it.
    #[serde(default = "yes")]
    pub copy_if_aac: bool,
}

fn yes() -> bool {
    true
}

/// Platform limits (e.g. Reddit's upload limits); exceeding one is a warning (FR-002).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Limits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub max_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub max_duration: Option<Ts>,
}

const SPEED_PRESETS: [&str; 8] =
    ["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower"];

impl Preset {
    /// Parses and validates one preset document.
    pub fn from_json(text: &str) -> Result<Preset, String> {
        let p: Preset = serde_json::from_str(text).map_err(|e| e.to_string())?;
        p.validate()?;
        Ok(p)
    }

    /// Rules the schema cannot express, plus the schema's own ranges (so built-in and user files
    /// are checked the same way at runtime).
    pub fn validate(&self) -> Result<(), String> {
        let id_ok = !self.id.is_empty()
            && self.id.len() <= 40
            && self.id.split('-').all(|part| {
                !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            });
        let checks: [(bool, String); 12] = [
            (self.schema == 1, format!("unsupported preset schema {}", self.schema)),
            (id_ok, format!("id {:?} must be kebab-case (a-z, 0-9, single dashes)", self.id)),
            (self.version >= 1, "version must be at least 1".into()),
            (!self.name.trim().is_empty(), "name must not be empty".into()),
            (!self.devices.is_empty(), "devices must list at least one device class".into()),
            (
                (144..=4320).contains(&self.width) && (144..=4320).contains(&self.height),
                format!("size {}x{} must be within 144..4320", self.width, self.height),
            ),
            (
                self.width.is_multiple_of(2) && self.height.is_multiple_of(2),
                format!("size {}x{} must be even (4:2:0 video)", self.width, self.height),
            ),
            ((1..=120).contains(&self.max_fps), format!("max_fps {} must be within 1..120", self.max_fps)),
            (self.video.crf <= 51, format!("crf {} must be within 0..51", self.video.crf)),
            (
                SPEED_PRESETS.contains(&self.video.preset.as_str()),
                format!("unknown encoder preset {:?}", self.video.preset),
            ),
            (
                self.video.tag.is_none()
                    || (self.video.tag.as_deref() == Some("hvc1") && self.video.codec == VideoCodec::Hevc),
                "tag \"hvc1\" is only valid with hevc".into(),
            ),
            (!self.hdr_capable || self.video.codec == VideoCodec::Hevc, "hdr_capable requires hevc".into()),
        ];
        if let Some((_, msg)) = checks.into_iter().find(|(ok, _)| !ok) {
            return Err(msg);
        }
        let profile_ok = match self.video.codec {
            VideoCodec::Hevc => self.video.profile == "main",
            VideoCodec::H264 => self.video.profile == "high",
        };
        if !profile_ok {
            return Err(format!(
                "profile {:?} does not match codec {:?}",
                self.video.profile, self.video.codec
            ));
        }
        match (&self.video.level, self.video.codec) {
            (None, _) => {}
            (Some(_), VideoCodec::Hevc) => return Err("level is only supported for h264 presets".into()),
            (Some(l), VideoCodec::H264) => {
                let ok = matches!(l.as_bytes(), [b'1'..=b'6'] | [b'1'..=b'6', b'.', b'0'..=b'2']);
                if !ok {
                    return Err(format!("level {l:?} must look like 4.2"));
                }
            }
        }
        let text_ok = |s: &str, max: usize| s.chars().count() <= max && !s.chars().any(char::is_control);
        if !text_ok(&self.name, 80) || !text_ok(&self.description, 500) {
            return Err(
                "name (≤ 80) and description (≤ 500) must be short, without control characters".into()
            );
        }
        if self.devices.len() > 20 || !self.devices.iter().all(|d| text_ok(d, 80)) {
            return Err("devices: at most 20 entries of ≤ 80 characters, without control characters".into());
        }
        if !(32..=512).contains(&self.audio.bitrate_k) {
            return Err(format!("audio bitrate {} kb/s must be within 32..512", self.audio.bitrate_k));
        }
        if !(self.est_bpp > 0.0 && self.est_bpp <= 1.0) {
            return Err(format!("est_bpp {} must be within (0, 1]", self.est_bpp));
        }
        Ok(())
    }

    /// Height / width (portrait presets are above 1).
    pub fn aspect(&self) -> f64 {
        f64::from(self.height) / f64::from(self.width)
    }
}

/// Largest preset file read (real presets are about 1 kB).
const MAX_PRESET_BYTES: u64 = 64 * 1024;

/// Reads a user preset: a regular file (not a symlink) of at most 64 KiB.
fn read_preset_file(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.file_type().is_file() {
        return Err("not a regular file (symlinks are not followed)".into());
    }
    if meta.len() > MAX_PRESET_BYTES {
        return Err(format!("larger than {} KiB", MAX_PRESET_BYTES / 1024));
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_PRESET_BYTES).read_to_string(&mut text))
        .map_err(|e| e.to_string())?;
    Ok(text)
}

/// Where a preset came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase", tag = "source", content = "file")]
pub enum PresetSource {
    Builtin,
    User(#[serde(serialize_with = "crate::pathser::path")] PathBuf),
}

/// A user preset file that could not be used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PresetProblem {
    #[serde(serialize_with = "crate::pathser::path")]
    pub file: PathBuf,
    pub message: String,
}

/// Built-in presets plus the user's, in a stable order: built-ins first (in their shipped order,
/// replaced in place by a user override), then user-only presets by id.
#[derive(Debug, Clone)]
pub struct PresetRegistry {
    presets: Vec<(Preset, PresetSource)>,
    problems: Vec<PresetProblem>,
}

impl PresetRegistry {
    /// Built-ins only.
    pub fn builtin() -> PresetRegistry {
        let presets = BUILTIN
            .iter()
            .map(|text| (Preset::from_json(text).expect("built-in presets are valid"), PresetSource::Builtin))
            .collect();
        PresetRegistry { presets, problems: Vec::new() }
    }

    /// Built-ins plus `<config_dir>/presets/*.json` (when `config_dir` is given).
    pub fn load(config_dir: Option<&Path>) -> PresetRegistry {
        let mut reg = PresetRegistry::builtin();
        let Some(dir) = config_dir.map(|d| d.join("presets")) else { return reg };
        let Ok(entries) = std::fs::read_dir(&dir) else { return reg };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("json")))
            .collect();
        files.sort();
        let mut user: Vec<(Preset, PresetSource)> = Vec::new();
        for file in files {
            let parsed = read_preset_file(&file).and_then(|t| Preset::from_json(&t));
            match parsed {
                Ok(p) if user.iter().any(|(u, _)| u.id == p.id) => reg.problems.push(PresetProblem {
                    file,
                    message: format!("another user preset already uses the id {:?}", p.id),
                }),
                Ok(p) => user.push((p, PresetSource::User(file))),
                Err(message) => reg.problems.push(PresetProblem { file, message }),
            }
        }
        user.sort_by(|a, b| a.0.id.cmp(&b.0.id));
        for (p, src) in user {
            match reg.presets.iter_mut().find(|(b, _)| b.id == p.id) {
                Some(slot) => *slot = (p, src),
                None => reg.presets.push((p, src)),
            }
        }
        reg
    }

    pub fn get(&self, id: &str) -> Option<&Preset> {
        self.presets.iter().find(|(p, _)| p.id == id).map(|(p, _)| p)
    }

    pub fn list(&self) -> impl Iterator<Item = (&Preset, &PresetSource)> {
        self.presets.iter().map(|(p, s)| (p, s))
    }

    pub fn ids(&self) -> Vec<&str> {
        self.presets.iter().map(|(p, _)| p.id.as_str()).collect()
    }

    pub fn problems(&self) -> &[PresetProblem] {
        &self.problems
    }

    pub fn default_id(&self) -> &'static str {
        DEFAULT_PRESET
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_validator() -> jsonschema::Validator {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../specs/002-mobile-upscale/contracts/preset.schema.json");
        let schema: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        jsonschema::validator_for(&schema).unwrap()
    }

    #[test]
    fn built_ins_match_the_contract_schema_and_research() {
        let v = schema_validator();
        for text in BUILTIN {
            let json: serde_json::Value = serde_json::from_str(text).unwrap();
            let errors: Vec<String> = v.iter_errors(&json).map(|e| e.to_string()).collect();
            assert!(errors.is_empty(), "{}: {errors:?}", json["id"]);
        }
        let reg = PresetRegistry::builtin();
        assert_eq!(
            reg.ids(),
            [
                "phone-fill",
                "phone-fill-hd",
                "pixel-tall",
                "reddit-916",
                "reddit-feed-45",
                "fold-inner",
                "duo"
            ]
        );
        let size = |id: &str| reg.get(id).map(|p| (p.width, p.height));
        assert_eq!(size("phone-fill"), Some((1080, 2340)));
        assert_eq!(size("phone-fill-hd"), Some((1440, 3120)));
        assert_eq!(size("pixel-tall"), Some((1080, 2400)));
        assert_eq!(size("reddit-916"), Some((1080, 1920)));
        assert_eq!(size("reddit-feed-45"), Some((1080, 1350)));
        assert_eq!(size("fold-inner"), Some((1080, 1200)));
        assert_eq!(size("duo"), Some((1080, 1566)));
        assert_eq!(reg.default_id(), "phone-fill");
        let reddit = reg.get("reddit-916").unwrap();
        assert_eq!(reddit.video.codec, VideoCodec::H264);
        let limits = reddit.limits.as_ref().unwrap();
        assert_eq!((limits.max_bytes, limits.max_duration), (Some(1 << 30), Some(Ts::from_secs_f64(900.0))));
        let hdr: Vec<&str> = reg.list().filter(|(p, _)| p.hdr_capable).map(|(p, _)| p.id.as_str()).collect();
        assert_eq!(hdr, ["phone-fill", "phone-fill-hd"]);
    }

    fn with(edit: impl FnOnce(&mut serde_json::Value)) -> Result<Preset, String> {
        let mut v: serde_json::Value = serde_json::from_str(BUILTIN[0]).unwrap();
        edit(&mut v);
        Preset::from_json(&v.to_string())
    }

    #[test]
    fn validation_table() {
        use serde_json::json;
        type Edit = Box<dyn FnOnce(&mut serde_json::Value)>;
        let cases: Vec<(&str, Edit)> = vec![
            ("odd width", Box::new(|v| v["width"] = json!(1081))),
            ("too small", Box::new(|v| v["height"] = json!(100))),
            ("crf above 51", Box::new(|v| v["video"]["crf"] = json!(60))),
            (
                "hvc1 with h264",
                Box::new(|v| {
                    v["video"]["codec"] = json!("h264");
                    v["video"]["profile"] = json!("high");
                }),
            ),
            (
                "hdr_capable with h264",
                Box::new(|v| {
                    v["video"]["codec"] = json!("h264");
                    v["video"]["profile"] = json!("high");
                    v["video"].as_object_mut().unwrap().remove("tag");
                }),
            ),
            ("bad id", Box::new(|v| v["id"] = json!("Phone Fill"))),
            ("double dash id", Box::new(|v| v["id"] = json!("phone--fill"))),
            ("unknown field", Box::new(|v| v["colour"] = json!("red"))),
            ("schema 2", Box::new(|v| v["schema"] = json!(2))),
            ("no devices", Box::new(|v| v["devices"] = json!([]))),
            ("unknown speed preset", Box::new(|v| v["video"]["preset"] = json!("placebo"))),
            ("profile mismatch", Box::new(|v| v["video"]["profile"] = json!("high"))),
            ("est_bpp zero", Box::new(|v| v["est_bpp"] = json!(0.0))),
            ("level on hevc", Box::new(|v| v["video"]["level"] = json!("5.1"))),
            ("escape in name", Box::new(|v| v["name"] = json!("x\u{1b}[31m"))),
            ("long description", Box::new(|v| v["description"] = json!("x".repeat(501)))),
            ("fps 0", Box::new(|v| v["max_fps"] = json!(0))),
        ];
        for (name, edit) in cases {
            assert!(with(edit).is_err(), "{name} must be rejected");
        }
        assert!(with(|_| {}).is_ok());
    }

    #[test]
    fn user_presets_override_by_id_and_bad_files_are_reported() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("presets");
        std::fs::create_dir(&dir).unwrap();
        let mut over: serde_json::Value = serde_json::from_str(BUILTIN[0]).unwrap();
        over["name"] = "My phone fill".into();
        over["video"]["crf"] = 20.into();
        std::fs::write(dir.join("a-override.json"), over.to_string()).unwrap();
        let mut extra: serde_json::Value = serde_json::from_str(BUILTIN[0]).unwrap();
        extra["id"] = "ios-pro".into();
        extra["width"] = 1206.into();
        extra["height"] = 2622.into();
        std::fs::write(dir.join("b-ios-pro.json"), extra.to_string()).unwrap();
        std::fs::write(dir.join("c-broken.json"), "{ not json").unwrap();
        std::fs::write(dir.join("d-dup.json"), extra.to_string()).unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();

        let reg = PresetRegistry::load(Some(d.path()));
        let pf = reg.get("phone-fill").unwrap();
        assert_eq!((pf.name.as_str(), pf.video.crf), ("My phone fill", 20));
        assert_eq!(reg.ids().first(), Some(&"phone-fill"), "override keeps the built-in's place");
        assert_eq!(reg.ids().last(), Some(&"ios-pro"), "user-only presets come last");
        assert_eq!(reg.get("ios-pro").map(|p| p.width), Some(1206));
        let bad: Vec<String> = reg
            .problems()
            .iter()
            .map(|p| p.file.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(bad, ["c-broken.json", "d-dup.json"]);
        assert!(matches!(reg.list().next().unwrap().1, PresetSource::User(_)));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_and_oversized_preset_files_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("presets");
        std::fs::create_dir(&dir).unwrap();
        let real = d.path().join("elsewhere.json");
        std::fs::write(&real, BUILTIN[0]).unwrap();
        std::os::unix::fs::symlink(&real, dir.join("link.json")).unwrap();
        std::fs::write(dir.join("huge.json"), " ".repeat(70 * 1024)).unwrap();
        let reg = PresetRegistry::load(Some(d.path()));
        let msgs: Vec<&str> = reg.problems().iter().map(|p| p.message.as_str()).collect();
        assert_eq!(msgs.len(), 2, "{msgs:?}");
        assert!(
            msgs.iter().any(|m| m.contains("larger than 64 KiB"))
                && msgs.iter().any(|m| m.contains("symlinks"))
        );
    }

    #[test]
    fn missing_config_dir_gives_built_ins() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(PresetRegistry::load(Some(&d.path().join("nope"))).ids().len(), 7);
        assert_eq!(PresetRegistry::load(None).problems().len(), 0);
    }
}
