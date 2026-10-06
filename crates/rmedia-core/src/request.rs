//! Input model: what parsers, CLI flags and the GUI produce (data-model.md).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::time::{EndSpec, Ts};

pub const DEFAULT_NAME_TEMPLATE: &str = "{source}_{index:03}_{name-or-range}.{ext}";
pub const MULTI_SOURCE_NAME_TEMPLATE: &str = "{source}/{source}_{index:03}_{name-or-range}.{ext}";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Stream copy; start snaps to the preceding keyframe.
    #[default]
    Fast,
    /// Frame-accurate re-encode.
    Accurate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnExists {
    #[default]
    Skip,
    Overwrite,
    Rename,
}

/// Audio selection. Track numbers are 1-based, in source order of audio streams.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioSel {
    #[default]
    All,
    None,
    #[serde(untagged)]
    Tracks(Vec<u32>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubSel {
    #[default]
    All,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StreamSelection {
    pub audio: AudioSel,
    pub subtitles: SubSel,
}

/// Fully resolved options for a clip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Options {
    pub mode: Mode,
    /// `None` = `<source dir>/clips`.
    #[serde(serialize_with = "crate::pathser::opt_path")]
    pub output_dir: Option<PathBuf>,
    pub name_template: String,
    /// Output extension; `None` = same as source.
    pub container: Option<String>,
    pub on_exists: OnExists,
    pub streams: StreamSelection,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            mode: Mode::Fast,
            output_dir: None,
            name_template: DEFAULT_NAME_TEMPLATE.to_owned(),
            container: None,
            on_exists: OnExists::Skip,
            streams: StreamSelection::default(),
        }
    }
}

/// Overrides layered as defaults < source < clip.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    #[serde(default, skip_serializing_if = "Option::is_none", serialize_with = "crate::pathser::opt_path")]
    pub output_dir: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_exists: Option<OnExists>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioSel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitles: Option<SubSel>,
}

impl PartialOptions {
    pub fn apply(&self, base: &Options) -> Options {
        Options {
            mode: self.mode.unwrap_or(base.mode),
            output_dir: self.output_dir.clone().or_else(|| base.output_dir.clone()),
            name_template: self.name_template.clone().unwrap_or_else(|| base.name_template.clone()),
            container: self.container.clone().or_else(|| base.container.clone()),
            on_exists: self.on_exists.unwrap_or(base.on_exists),
            streams: StreamSelection {
                audio: self.audio.clone().unwrap_or_else(|| base.streams.audio.clone()),
                subtitles: self.subtitles.unwrap_or(base.streams.subtitles),
            },
        }
    }
}

/// Where a request came from, for error messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Origin {
    Cli,
    Gui,
    Line {
        #[serde(serialize_with = "crate::pathser::path")]
        file: PathBuf,
        line: usize,
    },
    JsonPath {
        #[serde(serialize_with = "crate::pathser::path")]
        file: PathBuf,
        path: String,
    },
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Origin::Cli => f.write_str("command line"),
            Origin::Gui => f.write_str("queue"),
            Origin::Line { file, line: 0 } => write!(f, "{}", file.display()),
            Origin::Line { file, line } => write!(f, "{}:{line}", file.display()),
            Origin::JsonPath { file, path } if path.is_empty() => write!(f, "{}", file.display()),
            Origin::JsonPath { file, path } => write!(f, "{}:{path}", file.display()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipRequest {
    pub start: Ts,
    pub end: EndSpec,
    pub name: Option<String>,
    pub overrides: PartialOptions,
    pub origin: Origin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRequest {
    pub path: PathBuf,
    pub clips: Vec<ClipRequest>,
    pub overrides: PartialOptions,
    pub origin: Origin,
}

/// The single normalized input form for planning.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BatchRequest {
    pub defaults: Options,
    pub sources: Vec<SourceRequest>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_layer_in_order() {
        let defaults = Options::default();
        let source = PartialOptions {
            mode: Some(Mode::Accurate),
            container: Some("mkv".into()),
            ..Default::default()
        };
        let clip = PartialOptions { container: Some("mp4".into()), ..Default::default() };
        let resolved = clip.apply(&source.apply(&defaults));
        assert_eq!(resolved.mode, Mode::Accurate);
        assert_eq!(resolved.container.as_deref(), Some("mp4"));
        assert_eq!(resolved.on_exists, OnExists::Skip);
    }

    #[test]
    fn audio_selection_serde() {
        let all: AudioSel = serde_json::from_str("\"all\"").unwrap();
        let tracks: AudioSel = serde_json::from_str("[1,2]").unwrap();
        assert_eq!(all, AudioSel::All);
        assert_eq!(tracks, AudioSel::Tracks(vec![1, 2]));
    }
}
