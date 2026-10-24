//! GIF export for Reddit (feature 003): targets, timing, the frame chain and encode argv, the
//! size estimate and budget search, the optional engines and output verification.

pub mod budget;
pub mod engines;
pub mod estimate;
pub mod graph;
pub mod target;
pub mod timing;
pub mod verify;

use serde::{Deserialize, Serialize};

use crate::upscale::Reframe;
use target::Dither;

/// GIF, or the Reddit MP4 (a silent H.264 loop; recommended for posts, research R6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifFormat"))]
pub enum Format {
    #[default]
    Gif,
    Mp4,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Gif => "gif",
            Format::Mp4 => "mp4",
        }
    }
}

/// Which GIF engine: the target's (`auto`), the built-in palette pipeline, or gifski.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifEngineChoice"))]
pub enum EngineChoice {
    #[default]
    Auto,
    Builtin,
    Gifski,
}

/// How many times the GIF plays (research R15). JSON: `"forever"` or a number of plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopCount {
    #[default]
    Forever,
    /// 1..=65535 plays (1 = no NETSCAPE block).
    Plays(u16),
}

#[cfg(feature = "ts")]
impl ts_rs::TS for LoopCount {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;
    fn name(_: &ts_rs::Config) -> String {
        "\"forever\" | number".into()
    }
    fn inline(c: &ts_rs::Config) -> String {
        Self::name(c)
    }
}

impl Serialize for LoopCount {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            LoopCount::Forever => s.serialize_str("forever"),
            LoopCount::Plays(n) => s.serialize_u16(*n),
        }
    }
}

impl<'de> Deserialize<'de> for LoopCount {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Text(String),
            Plays(u64),
        }
        match Raw::deserialize(d)? {
            Raw::Text(t) if t == "forever" => Ok(LoopCount::Forever),
            Raw::Text(t) => Err(serde::de::Error::custom(format!(
                "loop must be \"forever\" or 1..65535 plays, not {t:?}"
            ))),
            Raw::Plays(n @ 1..=65535) => Ok(LoopCount::Plays(n as u16)),
            Raw::Plays(n) => Err(serde::de::Error::custom(format!("loop must be 1..65535 plays, not {n}"))),
        }
    }
}

impl LoopCount {
    /// The gif muxer's `-loop` (and gifski's `--repeat`) value: 0 forever, N-1 repeats, -1 once.
    pub fn repeat_arg(self) -> i32 {
        match self {
            LoopCount::Forever => 0,
            LoopCount::Plays(1) => -1,
            LoopCount::Plays(n) => i32::from(n) - 1,
        }
    }

    /// The NETSCAPE2.0 repeat count a GIF must carry (`None`: no block, it plays once).
    pub fn netscape(self) -> Option<u16> {
        match self {
            LoopCount::Forever => Some(0),
            LoopCount::Plays(1) => None,
            LoopCount::Plays(n) => Some(n - 1),
        }
    }
}

/// The output shape: the source's, or a portrait/square shape reached with 002's reframe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifShape"))]
pub enum Shape {
    #[default]
    #[serde(rename = "source")]
    Source,
    #[serde(rename = "9:16")]
    Tall,
    #[serde(rename = "4:5")]
    Feed,
    #[serde(rename = "1:1")]
    Square,
}

impl Shape {
    /// Width:height, or `None` for the source's shape.
    pub fn ratio(self) -> Option<(u32, u32)> {
        match self {
            Shape::Source => None,
            Shape::Tall => Some((9, 16)),
            Shape::Feed => Some((4, 5)),
            Shape::Square => Some((1, 1)),
        }
    }
}

/// A size budget set by a layer: either limit may be left to the target's (research R5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BudgetSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub soft: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub hard: Option<u64>,
}

impl BudgetSpec {
    /// The limits this gives over a target's `budget`: `soft` alone sets `hard = soft`; `hard`
    /// alone keeps the target's soft limit, capped at `hard`. `None`: no budget search.
    pub fn resolve(self, target: Option<target::Budget>) -> Option<target::Budget> {
        match (self.soft, self.hard) {
            (Some(soft), Some(hard)) => Some(target::Budget { soft, hard }),
            (Some(soft), None) => Some(target::Budget { soft, hard: soft }),
            (None, Some(hard)) => {
                Some(target::Budget { soft: target.map_or(hard, |t| t.soft.min(hard)), hard })
            }
            (None, None) => target,
        }
    }
}

/// Fully resolved GIF options for one job (data-model.md › GifOptions).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct GifOptions {
    pub target: String,
    pub format: Format,
    pub engine: EngineChoice,
    pub strict_engine: bool,
    pub optimise: bool,
    #[serde(rename = "loop")]
    pub loop_count: LoopCount,
    pub shape: Shape,
    pub framing: Reframe,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub fps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub colours: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub dither: Option<Dither>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub lossy: Option<u16>,
    /// `None`: no budget search. `Some(spec)`: the target's budget with `spec`'s limits over it
    /// (an empty spec is the target's own).
    pub budget: Option<BudgetSpec>,
    pub allow_trim: bool,
}

impl Default for GifOptions {
    fn default() -> Self {
        GifOptions {
            target: target::DEFAULT_TARGET.to_owned(),
            format: Format::Gif,
            engine: EngineChoice::Auto,
            strict_engine: false,
            optimise: true,
            loop_count: LoopCount::Forever,
            shape: Shape::Source,
            framing: Reframe::Smart,
            width: None,
            fps: None,
            colours: None,
            dither: None,
            lossy: None,
            budget: Some(BudgetSpec::default()),
            allow_trim: false,
        }
    }
}

/// Keeps an explicit `null` apart from a missing field (`#[serde(default)]` covers the latter).
fn some_or_null<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

/// One layer of GIF overrides (manifest defaults, a video, a clip, the CLI, settings).
/// `enabled: false` turns GIF export off at that layer; any other layer turns it on.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PartialGif {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub format: Option<Format>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub engine: Option<EngineChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub strict_engine: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub optimise: Option<bool>,
    #[serde(default, rename = "loop", skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub loop_count: Option<LoopCount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub shape: Option<Shape>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub framing: Option<Reframe>,
    /// `null` clears an override set by a layer below (back to the target's).
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "some_or_null")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number | null"))]
    pub width: Option<Option<u32>>,
    /// `null` clears an override set by a layer below (back to the target's).
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "some_or_null")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number | null"))]
    pub fps: Option<Option<u32>>,
    /// `null` clears an override set by a layer below (back to the target's).
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "some_or_null")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number | null"))]
    pub colours: Option<Option<u16>>,
    /// `null` clears an override set by a layer below (back to the target's).
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "some_or_null")]
    #[cfg_attr(feature = "ts", ts(optional, type = "Dither | null"))]
    pub dither: Option<Option<Dither>>,
    /// `null` clears an override set by a layer below (back to the target's).
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "some_or_null")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number | null"))]
    pub lossy: Option<Option<u16>>,
    /// `null` turns the budget search off; an object sets limits (`{}`: the target's).
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "some_or_null")]
    #[cfg_attr(feature = "ts", ts(optional, type = "BudgetSpec | null"))]
    pub budget: Option<Option<BudgetSpec>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub allow_trim: Option<bool>,
}

impl PartialGif {
    /// Layers this over `base` (the resolved options below it). Any layer that is present turns
    /// GIF export on, filling unset fields from `base` or the defaults; `enabled: false` turns
    /// it off.
    pub fn apply(&self, base: Option<&GifOptions>) -> Option<GifOptions> {
        if self.enabled == Some(false) {
            return None;
        }
        let b = base.cloned().unwrap_or_default();
        Some(GifOptions {
            target: self.target.clone().unwrap_or(b.target),
            format: self.format.unwrap_or(b.format),
            engine: self.engine.unwrap_or(b.engine),
            strict_engine: self.strict_engine.unwrap_or(b.strict_engine),
            optimise: self.optimise.unwrap_or(b.optimise),
            loop_count: self.loop_count.unwrap_or(b.loop_count),
            shape: self.shape.unwrap_or(b.shape),
            framing: self.framing.unwrap_or(b.framing),
            width: self.width.unwrap_or(b.width),
            fps: self.fps.unwrap_or(b.fps),
            colours: self.colours.unwrap_or(b.colours),
            dither: self.dither.unwrap_or(b.dither),
            lossy: self.lossy.unwrap_or(b.lossy),
            budget: self.budget.unwrap_or(b.budget),
            allow_trim: self.allow_trim.unwrap_or(b.allow_trim),
        })
    }

    /// The layer that turns `base` into `opts` (`None` = nothing to override), so that applying
    /// it to `base` gives `opts` back. Only changed fields are written; an override cleared here
    /// is written as `null`.
    pub fn diff(opts: Option<&GifOptions>, base: Option<&GifOptions>) -> Option<PartialGif> {
        if opts == base {
            return None;
        }
        let Some(o) = opts else {
            return Some(PartialGif { enabled: Some(false), ..PartialGif::default() });
        };
        let b = base.cloned().unwrap_or_default();
        fn changed<T: PartialEq + Clone>(o: &T, b: &T) -> Option<T> {
            (o != b).then(|| o.clone())
        }
        Some(PartialGif {
            enabled: None,
            target: changed(&o.target, &b.target),
            format: changed(&o.format, &b.format),
            engine: changed(&o.engine, &b.engine),
            strict_engine: changed(&o.strict_engine, &b.strict_engine),
            optimise: changed(&o.optimise, &b.optimise),
            loop_count: changed(&o.loop_count, &b.loop_count),
            shape: changed(&o.shape, &b.shape),
            framing: changed(&o.framing, &b.framing),
            width: changed(&o.width, &b.width),
            fps: changed(&o.fps, &b.fps),
            colours: changed(&o.colours, &b.colours),
            dither: changed(&o.dither, &b.dither),
            lossy: changed(&o.lossy, &b.lossy),
            budget: changed(&o.budget, &b.budget),
            allow_trim: changed(&o.allow_trim, &b.allow_trim),
        })
    }
}
