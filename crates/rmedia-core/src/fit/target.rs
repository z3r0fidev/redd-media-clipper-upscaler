//! Video targets as data (research R10; contracts/video-target.md): the built-in `reddit-video`
//! (Reddit's limits as of 2026-11-13, `research/10`), and user files in
//! `<config>/video-targets/*.json`, which replace a built-in with the same id. User files are
//! untrusted: they are read through `data_files` and every rule below is checked in code.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::preset::{PresetProblem, PresetSource};

/// The default video target.
pub const DEFAULT_TARGET: &str = "reddit-video";

const BUILTIN: [&str; 1] = [include_str!("builtin/reddit-video.json")];

/// The quality a fit aims for when the cap is not the limit, and the least it accepts when it is
/// (research R3, R4). Bits per pixel per frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "FitQuality"))]
pub struct Quality {
    /// x264 CRF of the quality mode.
    pub crf: u8,
    /// The quality ceiling: no more bits than this, however large the cap.
    pub max_bpp: f64,
    /// Below this the size is lowered (the ladder).
    pub min_bpp: f64,
}

/// The audio bitrate, and the lowest it may be lowered to (research R4, R6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "FitAudio"))]
pub struct Audio {
    pub kbps: u32,
    pub floor_kbps: u32,
}

/// A destination's limits and defaults (data-model.md › VideoTarget).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct VideoTarget {
    pub schema: u32,
    pub id: String,
    pub version: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub description: Option<String>,
    /// `false` until the owner confirms the limits (FR-003).
    pub verified: bool,
    /// The hard size cap in bytes: an output is never larger.
    pub max_bytes: u64,
    /// The length limit in seconds.
    pub max_duration: f64,
    /// The largest output's long side in pixels (never enlarged).
    pub long_side: u32,
    /// The size ladder stops at this short side.
    pub floor_short_side: u32,
    pub max_fps: u32,
    pub quality: Quality,
    pub audio: Audio,
}

impl crate::data_files::DataFile for VideoTarget {
    const KIND: &'static str = "video target";
    fn parse(text: &str) -> Result<VideoTarget, String> {
        VideoTarget::from_json(text)
    }
    fn id(&self) -> &str {
        &self.id
    }
}

impl VideoTarget {
    /// Parses and validates a target file.
    pub fn from_json(text: &str) -> Result<VideoTarget, String> {
        let t: VideoTarget = serde_json::from_str(text).map_err(|e| e.to_string())?;
        t.validate()?;
        Ok(t)
    }

    /// The schema rules (data-model.md › VideoTarget), checked again in code for user files.
    pub fn validate(&self) -> Result<(), String> {
        let printable = |s: &str| s.chars().count() <= 300 && !s.chars().any(crate::gif::target::unsafe_char);
        let q = &self.quality;
        let a = &self.audio;
        let checks: [(bool, &str); 14] = [
            (self.schema == 1, "schema must be 1"),
            (
                !self.id.is_empty()
                    && self.id.len() <= 40
                    && self.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "id must be 1 to 40 characters a-z, 0-9 or -",
            ),
            (self.version >= 1, "version must be at least 1"),
            (
                printable(&self.name) && !self.name.is_empty(),
                "name must be printable text (1 to 300 characters)",
            ),
            (
                self.description.as_deref().is_none_or(printable),
                "description must be printable text (at most 300 characters)",
            ),
            ((64 << 10..=4 << 30).contains(&self.max_bytes), "max_bytes must be within 64 KiB and 4 GiB"),
            (
                self.max_duration.is_finite() && self.max_duration > 0.0 && self.max_duration <= 14_400.0,
                "max_duration must be within (0, 14400] seconds",
            ),
            (
                (256..=3840).contains(&self.long_side) && self.long_side.is_multiple_of(2),
                "long_side must be even, within 256 and 3840",
            ),
            (
                self.floor_short_side >= 144
                    && u64::from(self.floor_short_side) < u64::from(self.long_side) * 9 / 16,
                "floor_short_side must be at least 144 and below the short side of long_side at 16:9",
            ),
            ((1..=120).contains(&self.max_fps), "max_fps must be within 1 and 120"),
            (q.crf <= 51, "quality.crf must be within 0 and 51"),
            (
                q.min_bpp.is_finite()
                    && q.max_bpp.is_finite()
                    && q.min_bpp > 0.0
                    && q.min_bpp < q.max_bpp
                    && q.max_bpp <= 1.0,
                "quality needs 0 < min_bpp < max_bpp <= 1",
            ),
            (a.floor_kbps >= 32 && a.floor_kbps <= a.kbps, "audio needs 32 <= floor_kbps <= kbps"),
            (a.kbps <= 320, "audio.kbps must be at most 320"),
        ];
        match checks.iter().find(|(ok, _)| !ok) {
            Some((_, why)) => Err((*why).to_owned()),
            None => Ok(()),
        }
    }
}

/// Built-in targets plus the user's, in a stable order: built-ins first (replaced in place by a
/// user override), then user-only targets by id.
#[derive(Debug, Clone)]
pub struct VideoTargetRegistry {
    targets: Vec<(VideoTarget, PresetSource)>,
    problems: Vec<PresetProblem>,
}

impl Default for VideoTargetRegistry {
    fn default() -> Self {
        VideoTargetRegistry::builtin()
    }
}

impl VideoTargetRegistry {
    /// Built-ins only.
    pub fn builtin() -> VideoTargetRegistry {
        let targets = BUILTIN
            .iter()
            .map(|t| {
                (VideoTarget::from_json(t).expect("built-in video targets are valid"), PresetSource::Builtin)
            })
            .collect();
        VideoTargetRegistry { targets, problems: Vec::new() }
    }

    /// Built-ins plus `<config_dir>/video-targets/*.json` (when `config_dir` is given).
    pub fn load(config_dir: Option<&Path>) -> VideoTargetRegistry {
        let reg = VideoTargetRegistry::builtin();
        let Some(dir) = config_dir.map(|d| d.join("video-targets")) else { return reg };
        let mut problems = Vec::new();
        let targets = crate::data_files::load(reg.targets, &dir, &mut problems);
        VideoTargetRegistry { targets, problems }
    }

    pub fn get(&self, id: &str) -> Option<&VideoTarget> {
        self.targets.iter().find(|(t, _)| t.id == id).map(|(t, _)| t)
    }

    /// The targets with their `source` (and `file` for user targets), and the target files that
    /// could not be used: `rmedia clip --list-fit-targets --json` and the desktop app's list.
    pub fn listing(&self) -> serde_json::Value {
        use serde_json::json;
        let targets: Vec<_> = self
            .list()
            .map(|(t, src)| {
                let mut v = serde_json::to_value(t).expect("a target serialises");
                match src {
                    PresetSource::Builtin => v["source"] = json!("builtin"),
                    PresetSource::User(f) => {
                        v["source"] = json!("user");
                        v["file"] = json!(crate::pathser::lossy(f));
                    }
                }
                v
            })
            .collect();
        let problems: Vec<_> = self
            .problems()
            .iter()
            .map(|p| json!({"file": crate::pathser::lossy(&p.file), "message": p.message}))
            .collect();
        json!({"targets": targets, "problems": problems})
    }

    pub fn list(&self) -> impl Iterator<Item = (&VideoTarget, &PresetSource)> {
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
            PresetSource::User(_) if VideoTargetRegistry::builtin().get(id).is_some() => "user override",
            PresetSource::User(_) => "user",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reddit() -> serde_json::Value {
        serde_json::from_str(BUILTIN[0]).unwrap()
    }

    /// The built-in carries research/10's limits, unverified (FR-001, FR-003).
    #[test]
    fn the_built_in_has_reddits_limits_as_of_2026_11_13() {
        let reg = VideoTargetRegistry::builtin();
        assert_eq!(reg.ids(), [DEFAULT_TARGET]);
        let t = reg.get("reddit-video").unwrap();
        assert!(!t.verified);
        assert_eq!((t.max_bytes, t.max_duration, t.long_side, t.max_fps), (1 << 30, 900.0, 1920, 60));
        assert_eq!((t.floor_short_side, t.audio), (480, Audio { kbps: 128, floor_kbps: 64 }));
        assert_eq!(t.quality, Quality { crf: 20, max_bpp: 0.12, min_bpp: 0.03 });
        assert_eq!(reg.label("reddit-video"), Some("built-in"));
    }

    #[test]
    fn every_rule_refuses_its_bad_value_with_a_reason() {
        type Edit = fn(&mut serde_json::Value);
        let cases: [(Edit, &str); 21] = [
            (|j| j["schema"] = 2.into(), "schema"),
            (|j| j["id"] = "Bad Id".into(), "id must"),
            (|j| j["id"] = "x".repeat(41).into(), "id must"),
            (|j| j["version"] = 0.into(), "version"),
            (|j| j["name"] = "a\u{1b}[2J".into(), "name"),
            (|j| j["description"] = "\u{202e}x".into(), "description"),
            (|j| j["max_bytes"] = 1000.into(), "max_bytes"),
            (|j| j["max_bytes"] = (5u64 << 30).into(), "max_bytes"),
            (|j| j["max_duration"] = 0.into(), "max_duration"),
            (|j| j["max_duration"] = 20_000.into(), "max_duration"),
            (|j| j["long_side"] = 1921.into(), "long_side"),
            (|j| j["long_side"] = 100.into(), "long_side"),
            (|j| j["long_side"] = u32::MAX.into(), "long_side"),
            (|j| j["floor_short_side"] = 1080.into(), "floor_short_side"),
            (|j| j["max_fps"] = 0.into(), "max_fps"),
            (|j| j["quality"]["crf"] = 52.into(), "crf"),
            (|j| j["quality"]["min_bpp"] = 0.2.into(), "min_bpp"),
            (|j| j["quality"]["max_bpp"] = 2.into(), "min_bpp"),
            (|j| j["audio"]["floor_kbps"] = 200.into(), "floor_kbps"),
            (|j| j["audio"]["kbps"] = 400.into(), "audio.kbps"),
            (|j| j["extra"] = 1.into(), "unknown field"),
        ];
        for (edit, want) in cases {
            let mut j = reddit();
            edit(&mut j);
            let e = VideoTarget::from_json(&j.to_string()).unwrap_err();
            assert!(e.contains(want), "{e} lacks {want}");
        }
    }

    #[test]
    fn a_user_file_overrides_by_id_or_adds_and_bad_files_are_problems() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("video-targets");
        std::fs::create_dir(&folder).unwrap();
        let mut own = reddit();
        own["max_bytes"] = 50_000_000.into();
        std::fs::write(folder.join("a.json"), own.to_string()).unwrap();
        let mut sub = reddit();
        sub["id"] = "sub-50mb".into();
        std::fs::write(folder.join("b.json"), sub.to_string()).unwrap();
        std::fs::write(folder.join("c.json"), "{").unwrap();
        let reg = VideoTargetRegistry::load(Some(dir.path()));
        assert_eq!(reg.ids(), ["reddit-video", "sub-50mb"]);
        assert_eq!(reg.get("reddit-video").unwrap().max_bytes, 50_000_000);
        assert_eq!(reg.label("reddit-video"), Some("user override"));
        assert_eq!(reg.label("sub-50mb"), Some("user"));
        assert_eq!(reg.problems().len(), 1);
        assert!(reg.problems()[0].file.ends_with("c.json"));
    }
}
