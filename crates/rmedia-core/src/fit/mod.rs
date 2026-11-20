//! Budgeted video (feature 005): an MP4 with audio that fits a video target's size cap and
//! length limit (specs/005-budgeted-reddit-video). `target` holds the targets as data, `plan`
//! the pure planner; `exec::fit` encodes, measures and retries.

pub mod plan;
pub mod target;

use serde::{Deserialize, Deserializer, Serialize};

use crate::time::Ts;

/// The largest size cap a fit may have (`VideoTarget::max_bytes`'s bound).
pub const MAX_CAP: u64 = 4 << 30;

/// Parses a size cap such as `50MB`, `8MiB` or `1GB` (the GIF size syntax), at most 4 GiB.
pub fn parse_size(text: &str) -> Result<u64, String> {
    crate::gif::parse_size_within(text, MAX_CAP, "4 GiB")
}

/// One encode of a fit job: the video bitrate it used and the size it came out at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FitAttempt {
    pub video_kbps: u32,
    pub bytes: u64,
}

/// What a fit job measured (data-model.md › FitOutcome; contracts/json-output.md › Outcome).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FitOutcome {
    /// The committed size; never more than `cap`.
    pub bytes: u64,
    pub cap: u64,
    pub mode: plan::Mode,
    /// The last encode's bitrates.
    pub video_kbps: u32,
    pub audio_kbps: Option<u32>,
    /// Every encode, in order (1 to 4).
    pub attempts: Vec<FitAttempt>,
    pub trimmed: bool,
    pub size: (u32, u32),
    pub fps: f64,
}

/// Test-only switches (`test-support`): make the size check see an overshoot, so the retry
/// path runs deterministically (tasks T013).
#[cfg(any(test, feature = "test-support"))]
pub mod test_hooks {
    use std::sync::atomic::{AtomicU32, Ordering};

    static OVERSHOOTS: AtomicU32 = AtomicU32::new(0);

    /// The next `n` size measurements report 10% over the cap.
    pub fn force_overshoot(n: u32) {
        OVERSHOOTS.store(n, Ordering::SeqCst);
    }

    /// Takes one forced overshoot, if any are left.
    pub(crate) fn take() -> bool {
        OVERSHOOTS.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok()
    }
}

/// Fully resolved fit options for one job (data-model.md › FitOptions).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FitOptions {
    pub target: String,
    /// Lowers the target's size cap for this job (bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub max_size: Option<u64>,
    /// Lowers the target's length limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub max_length: Option<Ts>,
    pub allow_trim: bool,
}

impl FitOptions {
    /// Why these overrides are refused for their target (an override may only lower a limit),
    /// or `None` (also when the target is unknown: the planner reports that).
    pub fn override_error(&self, targets: &target::VideoTargetRegistry) -> Option<String> {
        plan::limits(targets.get(&self.target)?, self).err()
    }
}

impl Default for FitOptions {
    fn default() -> Self {
        FitOptions {
            target: target::DEFAULT_TARGET.to_owned(),
            max_size: None,
            max_length: None,
            allow_trim: false,
        }
    }
}

/// One layer of fit overrides (manifest defaults, a video, a clip, the CLI). Present = a fit
/// at this layer (unless `enabled: false`); absent = inherited.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PartialFit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub target: Option<String>,
    /// `"50MB"`, `"8MiB"` or bytes; `null` clears an override set by a layer below.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "size_or_null")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number | string | null"))]
    pub max_size: Option<Option<u64>>,
    /// A duration (`"5:00"`, seconds); `null` clears an override set by a layer below.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::gif::some_or_null")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number | string | null"))]
    pub max_length: Option<Option<Ts>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub allow_trim: Option<bool>,
}

/// A size as bytes or as text in [`parse_size`]'s syntax; `null` is `Some(None)`.
fn size_or_null<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<u64>>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Bytes(u64),
        Text(String),
    }
    match Option::<Raw>::deserialize(d)? {
        None => Ok(Some(None)),
        Some(Raw::Bytes(b)) if (1..=MAX_CAP).contains(&b) => Ok(Some(Some(b))),
        Some(Raw::Bytes(b)) => {
            Err(serde::de::Error::custom(format!("size {b} must be more than 0 and at most 4 GiB")))
        }
        Some(Raw::Text(t)) => parse_size(&t).map(|b| Some(Some(b))).map_err(serde::de::Error::custom),
    }
}

impl PartialFit {
    /// Layers this over `base`: any layer that is present turns the fit on, filling unset fields
    /// from `base` or the defaults; `enabled: false` turns it off.
    pub fn apply(&self, base: Option<&FitOptions>) -> Option<FitOptions> {
        if self.enabled == Some(false) {
            return None;
        }
        let b = base.cloned().unwrap_or_default();
        Some(FitOptions {
            target: self.target.clone().unwrap_or(b.target),
            max_size: self.max_size.unwrap_or(b.max_size),
            max_length: self.max_length.unwrap_or(b.max_length),
            allow_trim: self.allow_trim.unwrap_or(b.allow_trim),
        })
    }

    /// The layer that turns `base` into `opts` (`None`: nothing to override); only changed
    /// fields are written, a cleared override as `null`.
    pub fn diff(opts: Option<&FitOptions>, base: Option<&FitOptions>) -> Option<PartialFit> {
        if opts == base {
            return None;
        }
        let Some(o) = opts else {
            return Some(PartialFit { enabled: Some(false), ..PartialFit::default() });
        };
        let b = base.cloned().unwrap_or_default();
        fn changed<T: PartialEq + Clone>(o: &T, b: &T) -> Option<T> {
            (o != b).then(|| o.clone())
        }
        Some(PartialFit {
            enabled: None,
            target: changed(&o.target, &b.target),
            max_size: changed(&o.max_size, &b.max_size),
            max_length: changed(&o.max_length, &b.max_length),
            allow_trim: changed(&o.allow_trim, &b.allow_trim),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `--fit-max-size` and the manifest's `max_size` (contracts/cli.md): the GIF size syntax,
    /// up to a video target's 4 GiB.
    #[test]
    fn sizes_parse_decimal_unless_binary_up_to_4_gib() {
        for (text, want) in [
            ("50MB", 50_000_000),
            ("8MiB", 8 << 20),
            ("1GB", 1_000_000_000),
            ("4GiB", 4 << 30),
            ("123", 123),
            (" 2.5 GB ", 2_500_000_000),
        ] {
            assert_eq!(parse_size(text), Ok(want), "{text}");
        }
        for bad in ["", "0", "-1MB", "5GiB", "1e3", "MB", "12XB", "1.5.2M"] {
            assert!(parse_size(bad).is_err(), "{bad:?}");
        }
        assert_eq!(parse_size("5GiB").unwrap_err(), "size \"5GiB\" must be more than 0 and at most 4 GiB");
    }
}
