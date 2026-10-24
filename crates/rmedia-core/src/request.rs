//! Input model: what parsers, CLI flags and the GUI produce (data-model.md).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::gif::{GifOptions, PartialGif};
use crate::time::{EndSpec, Ts};
use crate::upscale::{PartialUpscale, UpscaleOptions};

pub const DEFAULT_NAME_TEMPLATE: &str = "{source}_{index:03}_{name-or-range}.{ext}";
pub const MULTI_SOURCE_NAME_TEMPLATE: &str = "{source}/{source}_{index:03}_{name-or-range}.{ext}";
/// Default name of a whole-video upscale (in `<source dir>/upscaled/`).
pub const UPSCALE_NAME_TEMPLATE: &str = "{source}_{preset}.{ext}";
/// Default name of an upscaled clip (in `<source dir>/clips/`).
pub const UPSCALE_CLIP_NAME_TEMPLATE: &str = "{source}_{index:03}_{name-or-range}_{preset}.{ext}";
/// `MULTI_SOURCE_NAME_TEMPLATE` for upscaled clips (many sources into one output folder).
pub const UPSCALE_MULTI_CLIP_NAME_TEMPLATE: &str =
    "{source}/{source}_{index:03}_{name-or-range}_{preset}.{ext}";
/// Default name of a whole-video GIF (in `<source dir>/gifs/`, feature 003).
pub const GIF_NAME_TEMPLATE: &str = "{source}_{target}.{ext}";
/// Default name of a GIF made from a clip.
pub const GIF_CLIP_NAME_TEMPLATE: &str = "{source}_{index:03}_{name-or-range}_{target}.{ext}";
/// `MULTI_SOURCE_NAME_TEMPLATE` for GIF clips (many sources into one output folder).
pub const GIF_MULTI_CLIP_NAME_TEMPLATE: &str = "{source}/{source}_{index:03}_{name-or-range}_{target}.{ext}";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Mode {
    /// Stream copy; start snaps to the preceding keyframe.
    #[default]
    Fast,
    /// Frame-accurate re-encode.
    Accurate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum OnExists {
    #[default]
    Skip,
    Overwrite,
    Rename,
}

/// Audio selection. Track numbers are 1-based, in source order of audio streams.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum AudioSel {
    #[default]
    All,
    None,
    #[serde(untagged)]
    Tracks(Vec<u32>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum SubSel {
    #[default]
    All,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StreamSelection {
    pub audio: AudioSel,
    pub subtitles: SubSel,
}

/// Fully resolved options for a clip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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
    /// Upscale each job for phones (feature 002); `None` = plain clipping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub upscale: Option<UpscaleOptions>,
    /// Upscale fields from the command line that do not turn upscaling on (`batch --preset`):
    /// a layer that turns it on starts from these instead of the built-in defaults.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub upscale_fill: Option<UpscaleOptions>,
    /// Make each job a GIF or Reddit MP4 (feature 003); `None` = no GIF export.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub gif: Option<GifOptions>,
    /// GIF fields from the command line that do not turn GIF export on (`batch --target`
    /// without `--gif`): a layer that turns it on starts from these.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub gif_fill: Option<GifOptions>,
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
            upscale: None,
            upscale_fill: None,
            gif: None,
            gif_fill: None,
        }
    }
}

/// Overrides layered as defaults < source < clip.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PartialOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub mode: Option<Mode>,
    #[serde(default, skip_serializing_if = "Option::is_none", serialize_with = "crate::pathser::opt_path")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub output_dir: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub name_template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub container: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub on_exists: Option<OnExists>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub audio: Option<AudioSel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub subtitles: Option<SubSel>,
    /// Present = upscaling on at this layer (unless `enabled: false`); absent = inherited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub upscale: Option<PartialUpscale>,
    /// Present = GIF export on at this layer (unless `enabled: false`); absent = inherited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub gif: Option<PartialGif>,
}

impl PartialOptions {
    /// The overrides that turn `base` into `opts` (fields equal to `base` are left unset).
    pub fn diff(opts: &Options, base: &Options) -> PartialOptions {
        let pick = |same: bool| !same;
        PartialOptions {
            mode: pick(opts.mode == base.mode).then_some(opts.mode),
            output_dir: pick(opts.output_dir == base.output_dir).then(|| opts.output_dir.clone()).flatten(),
            name_template: pick(opts.name_template == base.name_template).then(|| opts.name_template.clone()),
            container: pick(opts.container == base.container).then(|| opts.container.clone()).flatten(),
            on_exists: pick(opts.on_exists == base.on_exists).then_some(opts.on_exists),
            audio: pick(opts.streams.audio == base.streams.audio).then(|| opts.streams.audio.clone()),
            subtitles: pick(opts.streams.subtitles == base.streams.subtitles)
                .then_some(opts.streams.subtitles),
            upscale: match (&base.upscale, &base.upscale_fill) {
                // `apply` starts a re-enabling layer from the fill, so diff against it too.
                (None, Some(fill)) if opts.upscale.is_some() => {
                    PartialUpscale::diff(opts.upscale.as_ref(), Some(fill))
                        .map(|p| PartialUpscale { enabled: None, ..p })
                        .or_else(|| Some(PartialUpscale::default()))
                }
                _ => PartialUpscale::diff(opts.upscale.as_ref(), base.upscale.as_ref()),
            },
            gif: match (&base.gif, &base.gif_fill) {
                (None, Some(fill)) if opts.gif.is_some() => PartialGif::diff(opts.gif.as_ref(), Some(fill))
                    .map(|p| PartialGif { enabled: None, ..p })
                    .or_else(|| Some(PartialGif::default())),
                _ => PartialGif::diff(opts.gif.as_ref(), base.gif.as_ref()),
            },
        }
    }

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
            upscale: match &self.upscale {
                None => base.upscale.clone(),
                Some(p) => p.apply(base.upscale.as_ref().or(base.upscale_fill.as_ref())),
            },
            upscale_fill: base.upscale_fill.clone(),
            gif: match &self.gif {
                None => base.gif.clone(),
                Some(p) => p.apply(base.gif.as_ref().or(base.gif_fill.as_ref())),
            },
            gif_fill: base.gif_fill.clone(),
        }
    }
}

/// Where a request came from, for error messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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

impl BatchRequest {
    /// Each clip's resolved options (defaults < source < clip).
    pub fn resolved(&self) -> impl Iterator<Item = Options> + '_ {
        self.sources.iter().flat_map(move |s| {
            let base = s.overrides.apply(&self.defaults);
            s.clips.iter().map(move |c| c.overrides.apply(&base))
        })
    }

    /// Whether planning needs the machine's GPU/tier check (research R8): some job is upscaled
    /// on a tier other than a forced CPU.
    pub fn needs_capability_check(&self) -> bool {
        self.resolved().any(|o| o.upscale.is_some_and(|u| u.tier != crate::upscale::TierChoice::Cpu))
    }
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
    fn diff_inverts_apply() {
        let base = Options::default();
        let p = PartialOptions {
            mode: Some(Mode::Accurate),
            container: Some("mkv".into()),
            ..Default::default()
        };
        assert_eq!(PartialOptions::diff(&p.apply(&base), &base), p);
        assert_eq!(PartialOptions::diff(&base, &base), PartialOptions::default());
    }

    fn layer(json: &str) -> PartialOptions {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn gif_layers_in_order_defaults_source_clip() {
        use crate::gif::target::Dither;
        use crate::gif::{BudgetSpec, Format, LoopCount, Shape};
        let resolve = |d: &str, v: &str, c: &str| {
            let defaults = layer(d).apply(&Options::default());
            layer(c).apply(&layer(v).apply(&defaults)).gif
        };
        assert_eq!(resolve("{}", "{}", "{}"), None, "off unless a layer turns it on");
        let on = resolve(r#"{"gif":{}}"#, "{}", "{}").unwrap();
        assert_eq!(on, GifOptions::default());
        assert_eq!((on.target.as_str(), on.format, on.optimise), ("reddit-post", Format::Gif, true));
        let g = resolve(
            r#"{"gif":{"target":"reddit-comment","shape":"4:5","loop":3}}"#,
            r#"{"gif":{"dither":{"bayer":5},"width":400}}"#,
            r#"{"gif":{"format":"mp4","allow_trim":true}}"#,
        )
        .unwrap();
        assert_eq!(g.target, "reddit-comment");
        assert_eq!(
            (g.shape, g.loop_count, g.dither, g.width),
            (Shape::Feed, LoopCount::Plays(3), Some(Dither::Bayer(5)), Some(400))
        );
        assert_eq!((g.format, g.allow_trim), (Format::Mp4, true));
        assert_eq!(resolve(r#"{"gif":{"target":"flat-ui"}}"#, r#"{"gif":{"enabled":false}}"#, "{}"), None);
        assert_eq!(resolve(r#"{"gif":{}}"#, "{}", r#"{"gif":{"enabled":false}}"#), None);
        let back =
            resolve(r#"{"gif":{"target":"flat-ui"}}"#, r#"{"gif":{"enabled":false}}"#, r#"{"gif":{}}"#)
                .unwrap();
        assert_eq!(back.target, "reddit-post", "re-enabling starts from the defaults, as upscale does");
        // `null` turns the budget search off; an object sets it.
        let off =
            resolve(r#"{"gif":{"budget":{"soft":5000000}}}"#, "{}", r#"{"gif":{"budget":null}}"#).unwrap();
        assert_eq!(off.budget, None);
        let set = resolve(r#"{"gif":{"budget":{"soft":5000000}}}"#, "{}", r#"{"gif":{}}"#).unwrap();
        assert_eq!(set.budget, Some(BudgetSpec { soft: Some(5_000_000), hard: None }));
        let target =
            resolve(r#"{"gif":{"budget":{"soft":5000000}}}"#, "{}", r#"{"gif":{"budget":{}}}"#).unwrap();
        assert_eq!(target.budget, Some(BudgetSpec::default()), "an empty budget is the target's own");
        // `null` clears an override set below.
        let cleared =
            resolve(r#"{"gif":{"fps":12,"dither":"none"}}"#, "{}", r#"{"gif":{"fps":null}}"#).unwrap();
        assert_eq!((cleared.fps, cleared.dither), (None, Some(Dither::None)));
    }

    #[test]
    fn gif_budget_soft_alone_sets_the_hard_limit() {
        use crate::gif::BudgetSpec;
        use crate::gif::target::Budget;
        let target = Some(Budget { soft: 8_000_000, hard: 20_000_000 });
        let spec = |soft, hard| BudgetSpec { soft, hard };
        assert_eq!(
            spec(Some(3_000_000), None).resolve(target),
            Some(Budget { soft: 3_000_000, hard: 3_000_000 })
        );
        assert_eq!(
            spec(None, Some(5_000_000)).resolve(target),
            Some(Budget { soft: 5_000_000, hard: 5_000_000 })
        );
        assert_eq!(
            spec(None, Some(10_000_000)).resolve(target),
            Some(Budget { soft: 8_000_000, hard: 10_000_000 })
        );
        assert_eq!(spec(Some(1), Some(2)).resolve(None), Some(Budget { soft: 1, hard: 2 }));
        assert_eq!(spec(None, None).resolve(target), target);
        assert_eq!(spec(None, Some(4)).resolve(None), Some(Budget { soft: 4, hard: 4 }));
    }

    #[test]
    fn gif_layers_reject_unknown_keys_and_bad_values() {
        assert!(serde_json::from_str::<PartialOptions>(r#"{"gif":{"bogus":1}}"#).is_err());
        assert!(serde_json::from_str::<PartialOptions>(r#"{"gif":{"loop":0}}"#).is_err());
        assert!(serde_json::from_str::<PartialOptions>(r#"{"gif":{"loop":"always"}}"#).is_err());
        assert!(serde_json::from_str::<PartialOptions>(r#"{"gif":{"shape":"16:9"}}"#).is_err());
        assert!(serde_json::from_str::<PartialOptions>(r#"{"gif":{"budget":{"max":1}}}"#).is_err());
        assert!(
            serde_json::from_str::<PartialOptions>(r#"{"gif":{"loop":"forever","format":"mp4"}}"#).is_ok()
        );
    }

    #[test]
    fn gif_diff_inverts_apply() {
        use crate::gif::{BudgetSpec, Format, PartialGif, Shape};
        let d = GifOptions::default();
        let custom = GifOptions {
            target: "flat-ui".into(),
            format: Format::Mp4,
            shape: Shape::Square,
            fps: Some(12),
            budget: Some(BudgetSpec { soft: Some(1_000_000), hard: None }),
            ..d.clone()
        };
        let off = GifOptions { budget: None, ..d.clone() };
        for (opts, base) in [
            (None, None),
            (Some(d.clone()), None),
            (None, Some(d.clone())),
            (Some(custom.clone()), Some(d.clone())),
            (Some(off.clone()), Some(custom.clone())),
            (Some(custom.clone()), None),
        ] {
            let p = PartialGif::diff(opts.as_ref(), base.as_ref());
            let back = match &p {
                None => base.clone(),
                Some(p) => p.apply(base.as_ref()),
            };
            assert_eq!(back, opts, "diff {p:?}");
        }
        let text = serde_json::to_string(&PartialGif::diff(Some(&off), Some(&d)).unwrap()).unwrap();
        assert_eq!(text, r#"{"budget":null}"#);
        let text = serde_json::to_string(&PartialGif::diff(Some(&d), Some(&custom)).unwrap()).unwrap();
        assert_eq!(
            text,
            r#"{"target":"reddit-post","format":"gif","shape":"source","fps":null,"budget":{}}"#
        );
        let full = PartialOptions::diff(
            &Options { gif: Some(custom.clone()), ..Options::default() },
            &Options::default(),
        );
        assert_eq!(full.gif.unwrap().apply(None), Some(custom));
    }

    #[test]
    fn gif_name_templates() {
        assert_eq!(GIF_NAME_TEMPLATE, "{source}_{target}.{ext}");
        assert_eq!(GIF_CLIP_NAME_TEMPLATE, "{source}_{index:03}_{name-or-range}_{target}.{ext}");
        assert_eq!(
            GIF_MULTI_CLIP_NAME_TEMPLATE,
            "{source}/{source}_{index:03}_{name-or-range}_{target}.{ext}"
        );
    }

    #[test]
    fn gif_loop_counts_map_to_repeats() {
        use crate::gif::LoopCount;
        assert_eq!((LoopCount::Forever.repeat_arg(), LoopCount::Forever.netscape()), (0, Some(0)));
        assert_eq!((LoopCount::Plays(1).repeat_arg(), LoopCount::Plays(1).netscape()), (-1, None));
        assert_eq!((LoopCount::Plays(3).repeat_arg(), LoopCount::Plays(3).netscape()), (2, Some(2)));
        assert_eq!(serde_json::to_string(&LoopCount::Plays(3)).unwrap(), "3");
        assert_eq!(serde_json::to_string(&LoopCount::Forever).unwrap(), r#""forever""#);
    }

    /// defaults < video < clip, per upscale field (T074).
    #[test]
    fn upscale_layers_in_order() {
        use crate::upscale::{Reframe, TierChoice};
        let resolve = |d: &str, v: &str, c: &str| {
            let defaults = layer(d).apply(&Options::default());
            layer(c).apply(&layer(v).apply(&defaults)).upscale
        };
        let get = |d, v, c| resolve(d, v, c).map(|u| (u.preset, u.reframe, u.tier, u.crf));
        let preset = |p: &str| p.to_owned();
        // (defaults, video, clip) → resolved
        let table = [
            ("{}", "{}", "{}", None),
            (
                r#"{"upscale":{}}"#,
                "{}",
                "{}",
                Some((preset("phone-fill"), Reframe::Smart, TierChoice::Auto, None)),
            ),
            (
                "{}",
                r#"{"upscale":{}}"#,
                "{}",
                Some((preset("phone-fill"), Reframe::Smart, TierChoice::Auto, None)),
            ),
            (
                r#"{"upscale":{"preset":"reddit-916","crf":20}}"#,
                r#"{"upscale":{"reframe":"blur"}}"#,
                r#"{"upscale":{"tier":"cpu"}}"#,
                Some((preset("reddit-916"), Reframe::Blur, TierChoice::Cpu, Some(20))),
            ),
            (
                r#"{"upscale":{"preset":"reddit-916"}}"#,
                r#"{"upscale":{"preset":"duo"}}"#,
                r#"{"upscale":{"preset":"phone-fill"}}"#,
                Some((preset("phone-fill"), Reframe::Smart, TierChoice::Auto, None)),
            ),
            (
                r#"{"upscale":{"crf":20}}"#,
                "{}",
                r#"{"upscale":{"crf":null}}"#,
                Some((preset("phone-fill"), Reframe::Smart, TierChoice::Auto, None)),
            ),
            // `enabled: false` turns it off at that level; a lower layer turns it back on and
            // inherits nothing from above the switch-off.
            (r#"{"upscale":{"preset":"duo"}}"#, r#"{"upscale":{"enabled":false}}"#, "{}", None),
            (r#"{"upscale":{"preset":"duo"}}"#, "{}", r#"{"upscale":{"enabled":false}}"#, None),
            (
                r#"{"upscale":{"preset":"duo"}}"#,
                r#"{"upscale":{"enabled":false}}"#,
                r#"{"upscale":{}}"#,
                Some((preset("phone-fill"), Reframe::Smart, TierChoice::Auto, None)),
            ),
            // Other fields leave upscaling as inherited.
            ("{}", r#"{"mode":"accurate"}"#, "{}", None),
        ];
        for (d, v, c, want) in table {
            assert_eq!(get(d, v, c), want, "defaults {d} video {v} clip {c}");
        }
    }

    /// Command-line fields that do not turn upscaling on (`batch --preset`) fill the layer that
    /// does, under the manifest's own values.
    #[test]
    fn upscale_fill_is_used_only_when_a_layer_enables() {
        use crate::upscale::UpscaleOptions;
        let cli = Options {
            upscale_fill: Some(UpscaleOptions { preset: "duo".into(), ..UpscaleOptions::default() }),
            ..Options::default()
        };
        assert_eq!(layer("{}").apply(&cli).upscale, None, "the fill alone does not enable");
        assert_eq!(layer(r#"{"mode":"accurate"}"#).apply(&cli).upscale, None);
        let video = layer(r#"{"upscale":{}}"#).apply(&cli);
        assert_eq!(video.upscale.as_ref().unwrap().preset, "duo");
        assert_eq!(
            layer(r#"{"upscale":{"preset":"reddit-916"}}"#).apply(&cli).upscale.unwrap().preset,
            "reddit-916"
        );
        // Inherited through the levels, and an enabled base wins over the fill.
        let clip = layer(r#"{"upscale":{"tier":"cpu"}}"#).apply(&layer("{}").apply(&cli));
        assert_eq!(clip.upscale.unwrap().preset, "duo");
        let base = layer(r#"{"upscale":{"preset":"reddit-916"}}"#).apply(&cli);
        assert_eq!(layer(r#"{"upscale":{"tier":"cpu"}}"#).apply(&base).upscale.unwrap().preset, "reddit-916");
        // Never written out.
        assert!(!serde_json::to_string(&cli).unwrap().contains("fill"));
    }

    #[test]
    fn upscale_diff_writes_only_changed_nested_fields_and_round_trips() {
        let base = layer(r#"{"upscale":{"preset":"reddit-916","crf":20}}"#).apply(&Options::default());
        let cases = [
            r#"{}"#,
            r#"{"upscale":{"reframe":"blur"}}"#,
            r#"{"upscale":{"preset":"duo","tier":"cpu"}}"#,
            r#"{"upscale":{"crf":null}}"#,
            r#"{"upscale":{"enabled":false}}"#,
            r#"{"mode":"accurate","upscale":{"motion":"fast"}}"#,
        ];
        for json in cases {
            let opts = layer(json).apply(&base);
            let d = PartialOptions::diff(&opts, &base);
            assert_eq!(d, layer(json), "only the changed fields of {json}");
            assert_eq!(d.apply(&base), opts, "round trip of {json}");
        }
        let plain = Options::default();
        let d = PartialOptions::diff(&base, &plain);
        assert_eq!(
            serde_json::to_value(&d).unwrap(),
            serde_json::json!({"upscale":{"preset":"reddit-916","crf":20}})
        );
        assert_eq!(d.apply(&plain), base);
    }

    /// A diff against options that carry a command-line fill round-trips too.
    #[test]
    fn upscale_diff_round_trips_over_a_fill() {
        use crate::upscale::UpscaleOptions;
        let base = Options {
            upscale_fill: Some(UpscaleOptions { preset: "duo".into(), ..UpscaleOptions::default() }),
            ..Options::default()
        };
        for json in [
            r#"{"upscale":{}}"#,
            r#"{"upscale":{"preset":"phone-fill"}}"#,
            r#"{"upscale":{"tier":"cpu"}}"#,
            "{}",
        ] {
            let opts = layer(json).apply(&base);
            assert_eq!(PartialOptions::diff(&opts, &base).apply(&base), opts, "round trip of {json}");
        }
    }

    #[test]
    fn capability_check_is_needed_unless_every_upscale_is_forced_cpu() {
        let req = |d: &str, clips: &[&str]| BatchRequest {
            defaults: layer(d).apply(&Options::default()),
            sources: vec![SourceRequest {
                path: "a.mp4".into(),
                clips: clips
                    .iter()
                    .map(|c| ClipRequest {
                        start: Ts::ZERO,
                        end: EndSpec::EndOfMedia,
                        name: None,
                        overrides: layer(c),
                        origin: Origin::Cli,
                    })
                    .collect(),
                overrides: PartialOptions::default(),
                origin: Origin::Cli,
            }],
        };
        assert!(!req("{}", &["{}"]).needs_capability_check(), "plain clips");
        assert!(req(r#"{"upscale":{}}"#, &["{}"]).needs_capability_check(), "auto");
        assert!(!req(r#"{"upscale":{"tier":"cpu"}}"#, &["{}", "{}"]).needs_capability_check(), "forced cpu");
        assert!(
            req(r#"{"upscale":{"tier":"cpu"}}"#, &["{}", r#"{"upscale":{"tier":"gpu"}}"#])
                .needs_capability_check()
        );
        assert!(!req(r#"{"upscale":{}}"#, &[r#"{"upscale":{"enabled":false}}"#]).needs_capability_check());
    }

    #[test]
    fn audio_selection_serde() {
        let all: AudioSel = serde_json::from_str("\"all\"").unwrap();
        let tracks: AudioSel = serde_json::from_str("[1,2]").unwrap();
        assert_eq!(all, AudioSel::All);
        assert_eq!(tracks, AudioSel::Tracks(vec![1, 2]));
    }
}
