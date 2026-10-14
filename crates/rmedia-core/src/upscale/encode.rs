//! Preset → encoder, colour and tag arguments (research R2, R4, R11).

use serde::Serialize;

use crate::preset::{Preset, VideoCodec};

/// Colour properties of a stream, as ffprobe names them (`bt709`, `smpte2084`, `tv`, …).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ColourTags {
    pub space: Option<String>,
    pub primaries: Option<String>,
    pub transfer: Option<String>,
    pub range: Option<String>,
}

impl ColourTags {
    /// The SDR output every preset writes unless HDR is kept (FR-004).
    pub fn bt709() -> ColourTags {
        let s = |v: &str| Some(v.to_owned());
        ColourTags { space: s("bt709"), primaries: s("bt709"), transfer: s("bt709"), range: s("tv") }
    }
}

/// What the encoder needs to know besides the preset.
#[derive(Debug, Clone)]
pub struct EncodeParams<'a> {
    pub preset: &'a Preset,
    /// Overrides `preset.video.crf`.
    pub crf: Option<u8>,
    /// `Some(source tags)` keeps HDR (10-bit, source primaries/transfer/matrix); `None` = SDR.
    pub hdr_keep: Option<&'a ColourTags>,
    /// Output frame rate (for the H.264 level).
    pub fps_out: f64,
    /// Encoder threads, fixed per plan so output is byte-identical on one machine (R11).
    pub threads: u32,
    /// Peak bitrate cap (kb/s) when the size estimate exceeds a platform limit (FR-002).
    pub maxrate_kbps: Option<u32>,
}

/// Video encoder arguments, including explicit colour tags.
pub fn video_args(p: &EncodeParams<'_>) -> Vec<String> {
    let v = &p.preset.video;
    let crf = p.crf.unwrap_or(v.crf).to_string();
    let sdr = ColourTags::bt709();
    let tags = p.hdr_keep.unwrap_or(&sdr);
    let tag = |t: &Option<String>, fallback: &str| t.clone().unwrap_or_else(|| fallback.to_owned());
    let (space, prim, trc) =
        (tag(&tags.space, "bt709"), tag(&tags.primaries, "bt709"), tag(&tags.transfer, "bt709"));
    let mut a: Vec<String> = Vec::new();
    let mut push = |items: &[&str]| a.extend(items.iter().map(|s| (*s).to_owned()));
    match v.codec {
        VideoCodec::Hevc => {
            let (pix, profile) =
                if p.hdr_keep.is_some() { ("yuv420p10le", "main10") } else { ("yuv420p", "main") };
            let params = format!(
                "colorprim={prim}:transfer={trc}:colormatrix={space}:range=limited:pools={}:log-level=error",
                p.threads
            );
            push(&[
                "-c:v",
                "libx265",
                "-preset",
                &v.preset,
                "-crf",
                &crf,
                "-pix_fmt",
                pix,
                "-profile:v",
                profile,
            ]);
            if let Some(level) = &v.level {
                push(&["-level:v", level]);
            }
            if let Some(t) = &v.tag {
                push(&["-tag:v", t]);
            }
            push(&["-x265-params", &params]);
        }
        VideoCodec::H264 => {
            let level = v
                .level
                .clone()
                .unwrap_or_else(|| h264_level(p.preset.width, p.preset.height, p.fps_out).to_owned());
            let threads = p.threads.to_string();
            push(&["-c:v", "libx264", "-preset", &v.preset, "-crf", &crf, "-profile:v", &v.profile]);
            push(&["-level:v", &level, "-pix_fmt", "yuv420p", "-threads", &threads]);
        }
    }
    if let Some(k) = p.maxrate_kbps {
        push(&["-maxrate", &format!("{k}k"), "-bufsize", &format!("{}k", k * 2)]);
    }
    push(&["-colorspace", &space, "-color_primaries", &prim, "-color_trc", &trc, "-color_range", "tv"]);
    a
}

/// The lowest H.264 level (from 4.2 up) whose frame-size and macroblock-rate limits fit.
pub fn h264_level(width: u32, height: u32, fps: f64) -> &'static str {
    let mbs = f64::from(width.div_ceil(16) * height.div_ceil(16));
    // (level, max macroblocks per frame, max macroblocks per second) — ITU-T H.264 Table A-1.
    const LEVELS: [(&str, f64, f64); 4] = [
        ("4.2", 8_704.0, 522_240.0),
        ("5.0", 22_080.0, 589_824.0),
        ("5.1", 36_864.0, 983_040.0),
        ("5.2", 36_864.0, 2_073_600.0),
    ];
    LEVELS.iter().find(|(_, frame, rate)| mbs <= *frame && mbs * fps <= *rate).map_or("6.2", |(l, ..)| l)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::PresetRegistry;

    #[derive(Default)]
    struct Opt {
        crf: Option<u8>,
        hdr_keep: Option<ColourTags>,
        fps: Option<f64>,
        maxrate_kbps: Option<u32>,
    }

    fn args(id: &str, o: Opt) -> String {
        let reg = PresetRegistry::builtin();
        let preset = reg.get(id).unwrap();
        let p = EncodeParams {
            preset,
            crf: o.crf,
            hdr_keep: o.hdr_keep.as_ref(),
            fps_out: o.fps.unwrap_or(30.0),
            threads: 4,
            maxrate_kbps: o.maxrate_kbps,
        };
        video_args(&p).join(" ")
    }

    #[test]
    fn hevc_phone_preset_is_hvc1_with_explicit_bt709() {
        insta::assert_snapshot!(args("phone-fill", Opt::default()), @"-c:v libx265 -preset fast -crf 22 -pix_fmt yuv420p -profile:v main -tag:v hvc1 -x265-params colorprim=bt709:transfer=bt709:colormatrix=bt709:range=limited:pools=4:log-level=error -colorspace bt709 -color_primaries bt709 -color_trc bt709 -color_range tv");
    }

    #[test]
    fn reddit_preset_is_h264_high_with_a_computed_level() {
        insta::assert_snapshot!(args("reddit-916", Opt { fps: Some(60.0), ..Opt::default() }), @"-c:v libx264 -preset medium -crf 18 -profile:v high -level:v 4.2 -pix_fmt yuv420p -threads 4 -colorspace bt709 -color_primaries bt709 -color_trc bt709 -color_range tv");
    }

    #[test]
    fn crf_override_size_cap_and_hdr_keep() {
        assert!(args("phone-fill", Opt { crf: Some(18), ..Opt::default() }).contains("-crf 18 "));
        assert!(
            args("reddit-916", Opt { maxrate_kbps: Some(8000), ..Opt::default() })
                .contains("-maxrate 8000k -bufsize 16000k")
        );
        let src = ColourTags {
            space: Some("bt2020nc".into()),
            primaries: Some("bt2020".into()),
            transfer: Some("smpte2084".into()),
            range: Some("tv".into()),
        };
        insta::assert_snapshot!(args("phone-fill", Opt { hdr_keep: Some(src), ..Opt::default() }), @"-c:v libx265 -preset fast -crf 22 -pix_fmt yuv420p10le -profile:v main10 -tag:v hvc1 -x265-params colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:range=limited:pools=4:log-level=error -colorspace bt2020nc -color_primaries bt2020 -color_trc smpte2084 -color_range tv");
    }

    #[test]
    fn h264_levels_follow_size_and_rate() {
        assert_eq!(h264_level(1080, 1920, 60.0), "4.2");
        assert_eq!(h264_level(1080, 1350, 60.0), "4.2");
        assert_eq!(h264_level(1080, 2340, 30.0), "5.0");
        assert_eq!(h264_level(1080, 2340, 60.0), "5.1");
        assert_eq!(h264_level(1440, 3120, 60.0), "5.2", "17 550 MBs × 60 exceeds 5.1");
    }
}
