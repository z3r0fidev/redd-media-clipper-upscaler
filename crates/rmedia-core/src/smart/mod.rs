//! Smart cut (feature 004): frame-exact clips that copy every whole GOP in the range and
//! re-encode only the partial GOPs at each end (research R1–R11).

pub mod args;
pub mod keyframes;

use serde::{Deserialize, Serialize};

pub use keyframes::{KeyframeEntry, KeyframeIndex, keyframes};

/// Why a smart job runs in accurate mode instead (research R2). The serialized values are stable
/// once released (FR-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum FallbackReason {
    /// The engine has no H.264 encoder (libx264).
    EncoderMissing,
    /// The video is not H.264.
    Codec,
    /// The source or output container is not mp4, mov, mkv or ts.
    Container,
    /// The H.264 profile is not Constrained Baseline, Baseline, Main or High.
    Profile,
    /// The pixel format is not 8-bit 4:2:0.
    PixelFormat,
    Interlaced,
    /// Variable frame rate.
    Vfr,
    /// A display rotation the final mux cannot carry.
    Rotation,
    /// The range holds no complete GOP.
    NoFullGop,
    /// A GOP at a copy boundary references the previous GOP.
    OpenGop,
    /// The frames at a seam changed size or pixel format (found after encoding).
    ParamsChanged,
    /// The joined result failed verification (found after encoding).
    SeamCheck,
}

impl FallbackReason {
    pub const ALL: [FallbackReason; 12] = [
        FallbackReason::EncoderMissing,
        FallbackReason::Codec,
        FallbackReason::Container,
        FallbackReason::Profile,
        FallbackReason::PixelFormat,
        FallbackReason::Interlaced,
        FallbackReason::Vfr,
        FallbackReason::Rotation,
        FallbackReason::NoFullGop,
        FallbackReason::OpenGop,
        FallbackReason::ParamsChanged,
        FallbackReason::SeamCheck,
    ];

    /// The serialized value (`open_gop`).
    pub fn as_str(self) -> &'static str {
        match self {
            FallbackReason::EncoderMissing => "encoder_missing",
            FallbackReason::Codec => "codec",
            FallbackReason::Container => "container",
            FallbackReason::Profile => "profile",
            FallbackReason::PixelFormat => "pixel_format",
            FallbackReason::Interlaced => "interlaced",
            FallbackReason::Vfr => "vfr",
            FallbackReason::Rotation => "rotation",
            FallbackReason::NoFullGop => "no_full_gop",
            FallbackReason::OpenGop => "open_gop",
            FallbackReason::ParamsChanged => "params_changed",
            FallbackReason::SeamCheck => "seam_check",
        }
    }

    /// A plain-language reason, completing "smart cut not possible: …".
    pub fn message(self) -> &'static str {
        match self {
            FallbackReason::EncoderMissing => "the engine has no H.264 encoder (libx264)",
            FallbackReason::Codec => "the video is not H.264; smart cut handles H.264 only",
            FallbackReason::Container => "smart cut handles mp4, mov, mkv and ts files only",
            FallbackReason::Profile => "the video's H.264 profile is not Baseline, Main or High",
            FallbackReason::PixelFormat => "the video is not 8-bit 4:2:0 (for example 10-bit or 4:2:2)",
            FallbackReason::Interlaced => "the video is interlaced",
            FallbackReason::Vfr => "the video has a variable frame rate",
            FallbackReason::Rotation => "the video's rotation cannot be kept in the output",
            FallbackReason::NoFullGop => "the range holds no complete group of pictures (it is too short)",
            FallbackReason::OpenGop => "the video has open GOPs (frames that depend on the previous GOP)",
            FallbackReason::ParamsChanged => "the video's size or pixel format changes at a cut point",
            FallbackReason::SeamCheck => "the joined clip failed the seam check",
        }
    }

    /// Whether the plan can find this reason, before anything is encoded (FR-011).
    pub fn known_before_encoding(self) -> bool {
        !matches!(self, FallbackReason::ParamsChanged | FallbackReason::SeamCheck)
    }
}

impl std::fmt::Display for FallbackReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialized_values_are_stable() {
        let values: Vec<String> = FallbackReason::ALL
            .iter()
            .map(|r| serde_json::to_value(r).unwrap().as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            values,
            [
                "encoder_missing",
                "codec",
                "container",
                "profile",
                "pixel_format",
                "interlaced",
                "vfr",
                "rotation",
                "no_full_gop",
                "open_gop",
                "params_changed",
                "seam_check"
            ]
        );
        for r in FallbackReason::ALL {
            assert_eq!(r.as_str(), serde_json::to_value(r).unwrap());
            let back: FallbackReason = serde_json::from_value(serde_json::json!(r.as_str())).unwrap();
            assert_eq!(back, r);
        }
    }

    #[test]
    fn messages_are_plain_sentences_and_two_reasons_need_encoding() {
        for r in FallbackReason::ALL {
            let m = r.message();
            assert!(!m.is_empty() && !m.ends_with('.') && m.chars().next().unwrap().is_lowercase(), "{m}");
        }
        let late: Vec<_> = FallbackReason::ALL.into_iter().filter(|r| !r.known_before_encoding()).collect();
        assert_eq!(late, [FallbackReason::ParamsChanged, FallbackReason::SeamCheck]);
    }
}
