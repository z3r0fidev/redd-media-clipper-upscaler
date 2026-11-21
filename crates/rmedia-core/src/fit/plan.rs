//! The pure fit planner (research R2–R5; data-model.md › FitPlan): trim to the length limit,
//! choose budget or quality mode, step the size down to the floor, lower the audio, then trim
//! or refuse. Arithmetic only; nothing is encoded or sampled.

use serde::Serialize;

use super::FitOptions;
use super::target::VideoTarget;
use crate::gif::plan::Lowered;

/// The share of the cap kept free for the container and the encoder's error (research R2).
pub const MARGIN: f64 = 0.02;
/// Each ladder step multiplies the long side by this (research R4).
pub const STEP: f64 = 0.75;
/// The VBV buffer in seconds at the video rate (`-bufsize 2×`): quality mode's largest excess.
pub const VBV_SECS: f64 = 2.0;
/// A trim never leaves less than this (a shorter range is refused instead).
const MIN_TRIMMED_SECS: f64 = 1.0;

/// How a fit job is encoded (research R1, R3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "FitMode"))]
pub enum Mode {
    /// The cap is the limit: two passes at `video_kbps`, under a VBV ceiling of the same rate.
    Budget,
    /// The cap is generous: one pass at the target's CRF, capped at the quality ceiling.
    Quality,
}

/// What the planner decided for one job.
#[derive(Debug, Clone, PartialEq)]
pub struct FitPlan {
    pub target: VideoTarget,
    /// The effective size cap in bytes.
    pub cap: u64,
    /// The effective length limit in seconds.
    pub max_length: f64,
    /// The range's length after any trim, in seconds.
    pub len: f64,
    /// The requested length, when the range was trimmed.
    pub trimmed_from: Option<f64>,
    /// The output size (even), never larger than the source.
    pub size: (u32, u32),
    /// The output frame rate as (num, den).
    pub fps: (u32, u32),
    /// The source is faster than the target allows: the output is resampled to `fps`.
    pub resample_fps: bool,
    /// The source audio has more than 2 channels: it is downmixed to stereo (set by the
    /// planner, which knows the streams).
    pub downmix: bool,
    /// An HDR source: zscale's input options for 002's tone-map to SDR BT.709 (set by the
    /// planner, which knows the colour tags).
    pub tonemap: Option<String>,
    pub mode: Mode,
    /// Budget mode: the target bitrate; quality mode: the ceiling.
    pub video_kbps: u32,
    /// Quality mode's CRF.
    pub crf: Option<u8>,
    /// `None`: the output has no audio.
    pub audio_kbps: Option<u32>,
    /// The size the plan expects (budget mode) or will not pass (quality mode).
    pub predicted_bytes: u64,
    pub lowered: Vec<Lowered>,
}

impl FitPlan {
    /// The output frame rate in frames per second.
    pub fn fps_f64(&self) -> f64 {
        f64::from(self.fps.0) / f64::from(self.fps.1.max(1))
    }

    /// Encoder passes: two in budget mode, one in quality mode.
    pub fn passes(&self) -> u32 {
        match self.mode {
            Mode::Budget => 2,
            Mode::Quality => 1,
        }
    }

    /// The dry run's `fit` block (contracts/json-output.md › Dry run).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self.summary()).expect("a fit summary serialises")
    }

    /// The plan as the dry run and the desktop app show it (contracts/json-output.md › `fit`).
    pub fn summary(&self) -> FitSummary {
        FitSummary {
            target: self.target.id.clone(),
            target_verified: self.target.verified,
            cap: self.cap,
            max_length: self.max_length,
            len: self.len,
            trimmed_from: self.trimmed_from,
            mode: self.mode,
            size: self.size,
            fps: (self.fps_f64() * 1000.0).round() / 1000.0,
            video_kbps: self.video_kbps,
            crf: self.crf,
            audio_kbps: self.audio_kbps,
            predicted_bytes: self.predicted_bytes,
            lowered: self.lowered.clone(),
            passes: self.passes(),
        }
    }
}

/// A planned fit, as the dry run's JSON `fit` block and the desktop queue show it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FitSummary {
    pub target: String,
    pub target_verified: bool,
    pub cap: u64,
    /// Seconds.
    pub max_length: f64,
    /// The range's length after any trim, in seconds.
    pub len: f64,
    /// The requested length, when the range was trimmed.
    pub trimmed_from: Option<f64>,
    pub mode: Mode,
    pub size: (u32, u32),
    /// Frames per second, to three decimals.
    pub fps: f64,
    pub video_kbps: u32,
    pub crf: Option<u8>,
    /// `null`: the output has no audio.
    pub audio_kbps: Option<u32>,
    pub predicted_bytes: u64,
    pub lowered: Vec<Lowered>,
    /// 2 in budget mode, 1 in quality mode.
    pub passes: u32,
}

/// Why a fit cannot be planned.
#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    /// The range is longer than the limit (`RANGE_TOO_LONG`). Seconds.
    TooLong { len: f64, max_length: f64 },
    /// Even at the floor, with the least audio, the cap is too small (`BUDGET_UNREACHABLE`);
    /// `min_cap` is the smallest cap that would fit the range.
    Unreachable { min_cap: u64, floor: (u32, u32), len: f64, cap: u64 },
    /// `max_size` or `max_length` is above the target's own limit (an override may only lower).
    AboveTarget(String),
}

impl Refusal {
    pub fn message(&self, target: &str) -> String {
        let human = crate::upscale::estimate::human_bytes;
        match self {
            Refusal::TooLong { len, max_length } => {
                let (len, max) = clock_pair(*len, *max_length);
                format!(
                    "the range ({len}) is longer than {target} allows ({max}); use --fit-allow-trim to keep the first {max}"
                )
            }
            Refusal::Unreachable { min_cap, floor, len, cap } => format!(
                "{} s cannot fit {} even at {}x{} (needs at least {}); raise --fit-max-size or use --fit-allow-trim",
                (len * 10.0).round() / 10.0,
                human(*cap),
                floor.0,
                floor.1,
                human(*min_cap)
            ),
            Refusal::AboveTarget(m) => m.clone(),
        }
    }
}

fn clock(secs: f64) -> String {
    let s = secs.round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Two lengths that must read differently: whole seconds, or tenths when they round the same.
fn clock_pair(a: f64, b: f64) -> (String, String) {
    if a.round() != b.round() {
        return (clock(a), clock(b));
    }
    let tenths = |secs: f64| {
        let t = (secs * 10.0).round() as u64;
        format!("{}:{:02}.{}", t / 600, t / 10 % 60, t % 10)
    };
    (tenths(a), tenths(b))
}

/// The source facts the planner needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Source {
    /// The range's length in seconds.
    pub len: f64,
    /// The displayed picture size (or the device preset's output size).
    pub size: (u32, u32),
    /// The frame rate as (num, den).
    pub fps: (u32, u32),
    pub has_audio: bool,
}

/// The effective cap (bytes) and length limit (seconds) of `target` with the user's
/// overrides, or why an override is refused: it may only lower a limit (contracts/cli.md, exit 2).
pub fn limits(target: &VideoTarget, opts: &FitOptions) -> Result<(u64, f64), String> {
    let human = crate::upscale::estimate::human_bytes;
    let cap = match opts.max_size {
        Some(s) if s > target.max_bytes => {
            return Err(format!(
                "--fit-max-size {} is above {}'s cap of {}",
                human(s),
                target.id,
                human(target.max_bytes)
            ));
        }
        Some(s) => s,
        None => target.max_bytes,
    };
    let max_length = match opts.max_length.map(|t| t.as_secs_f64()) {
        Some(l) if l > target.max_duration + 1e-6 => {
            return Err(format!(
                "--fit-max-length {} is above {}'s limit of {}",
                clock(l),
                target.id,
                clock(target.max_duration)
            ));
        }
        Some(l) => l,
        None => target.max_duration,
    };
    Ok((cap, max_length))
}

/// Plans a fit of `src` for `target` with the user's `opts` (data-model.md › planning order).
pub fn plan_fit(target: &VideoTarget, opts: &FitOptions, src: Source) -> Result<FitPlan, Refusal> {
    // 1. The effective limits (an override may only lower them).
    let (cap, max_length) = limits(target, opts).map_err(Refusal::AboveTarget)?;
    // 2. Size (never enlarged) and frame rate.
    let start = fit_long_side(src.size, target.long_side);
    let fps = cap_fps(src.fps, target.max_fps);
    let fps_f = f64::from(fps.0) / f64::from(fps.1);
    // 3. The length limit, in whole output frames: a frame that starts before the limit lasts
    // past it (at 29.97 fps, 10 s holds 299 frames, 9.977 s; 300 would last 10.010 s).
    let frames = (max_length * fps_f + 1e-6).floor();
    let longest = frames / fps_f;
    let mut len = src.len;
    let mut trimmed_from = None;
    if len > max_length + 1e-6 {
        if !opts.allow_trim {
            return Err(Refusal::TooLong { len, max_length });
        }
        trimmed_from = Some(len);
        len = longest;
    } else if len * fps_f > frames + 1e-6 {
        // Within the limit, but its last frame would end past it: less than a frame shorter.
        len = longest;
    }
    let floor_short = target.floor_short_side.min(start.0.min(start.1));
    let kbps = |size: (u32, u32), bpp: f64| bpp * f64::from(size.0) * f64::from(size.1) * fps_f / 1000.0;
    let usable = cap as f64 * (1.0 - MARGIN);
    let mut audio = src.has_audio.then_some(target.audio.kbps);
    let budget = |len: f64, audio: Option<u32>| usable * 8.0 / len / 1000.0 - f64::from(audio.unwrap_or(0));
    let mut lowered = Vec::new();

    // 4–5. Quality mode when the budget covers the ceiling and its VBV buffer (2 s at the
    // ceiling: a capped CRF can exceed `rate × length` by up to the buffer).
    let ceiling = kbps(start, target.quality.max_bpp);
    if budget(len, audio) * len >= ceiling * (len + VBV_SECS) {
        let video_kbps = (ceiling.round() as u32).max(1);
        return Ok(FitPlan {
            target: target.clone(),
            cap,
            max_length,
            len,
            trimmed_from,
            size: start,
            fps,
            resample_fps: fps != src.fps,
            downmix: false,
            tonemap: None,
            mode: Mode::Quality,
            video_kbps,
            crf: Some(target.quality.crf),
            audio_kbps: audio,
            predicted_bytes: bytes(video_kbps, audio, len)
                + (f64::from(video_kbps) * 125.0 * VBV_SECS) as u64,
            lowered,
        });
    }

    // 6. The ladder: lower the size while the budget is below the minimum. Every step scales the
    // starting size (no rounding drift), and the short side stops at the floor.
    let mut size = start;
    let mut scale = 1.0;
    while budget(len, audio) < kbps(size, target.quality.min_bpp) && size.0.min(size.1) > floor_short {
        scale *= STEP;
        size = scaled(start, scale, floor_short);
    }
    if size != start {
        lowered.push(Lowered {
            setting: "size".into(),
            from: serde_json::json!([start.0, start.1]),
            to: serde_json::json!([size.0, size.1]),
        });
    }
    let min = kbps(size, target.quality.min_bpp);

    // 7. The audio, down to its floor.
    if let Some(a) = audio
        && budget(len, audio) < min
    {
        let total = usable * 8.0 / len / 1000.0;
        let room = (total - min).floor();
        if room >= f64::from(target.audio.floor_kbps) && room < f64::from(a) {
            let to = room as u32;
            lowered.push(Lowered { setting: "audio_kbps".into(), from: a.into(), to: to.into() });
            audio = Some(to);
        }
    }

    // 8. Still too small: trim to the longest start that fits, or refuse.
    if budget(len, audio) < min {
        let floor_audio = audio.map(|_| target.audio.floor_kbps);
        let need = min + f64::from(floor_audio.unwrap_or(0));
        let min_cap = (need * 1000.0 / 8.0 * len / (1.0 - MARGIN)).ceil() as u64;
        let fits = usable * 8.0 / (need * 1000.0);
        if !opts.allow_trim || fits < MIN_TRIMMED_SECS {
            return Err(Refusal::Unreachable { min_cap, floor: size, len, cap });
        }
        if let (Some(a), Some(f)) = (audio, floor_audio)
            && a != f
        {
            lowered.push(Lowered { setting: "audio_kbps".into(), from: a.into(), to: f.into() });
        }
        audio = floor_audio;
        trimmed_from.get_or_insert(len);
        len = fits;
    }
    let video_kbps = (budget(len, audio).floor() as u32).max(1);
    Ok(FitPlan {
        target: target.clone(),
        cap,
        max_length,
        len,
        trimmed_from,
        size,
        fps,
        resample_fps: fps != src.fps,
        downmix: false,
        tonemap: None,
        mode: Mode::Budget,
        video_kbps,
        crf: None,
        audio_kbps: audio,
        predicted_bytes: bytes(video_kbps, audio, len),
        lowered,
    })
}

/// The bytes `video` + `audio` kbit/s take over `len` seconds.
fn bytes(video: u32, audio: Option<u32>, len: f64) -> u64 {
    ((f64::from(video) + f64::from(audio.unwrap_or(0))) * 1000.0 / 8.0 * len).floor() as u64
}

fn even(x: f64) -> u32 {
    ((x / 2.0).round() as u32 * 2).max(2)
}

/// `size` scaled down so its long side is at most `long`, keeping the aspect; even.
fn fit_long_side(size: (u32, u32), long: u32) -> (u32, u32) {
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    let scale = (f64::from(long) / w.max(h)).min(1.0);
    let (w, h) = (w * scale, h * scale);
    // Rounding to even never enlarges: an odd source dimension rounds down.
    let down = |x: f64, src: u32| even(x).min(src & !1).max(2);
    (down(w, size.0), down(h, size.1))
}

/// `start` scaled by `scale` (even), with its short side no lower than `floor`.
fn scaled(start: (u32, u32), scale: f64, floor: u32) -> (u32, u32) {
    let (w, h) = (f64::from(start.0), f64::from(start.1));
    let scale = scale.max(f64::from(floor) / w.min(h));
    let at_floor = |x: f64, is_short: bool| if is_short { floor & !1 } else { even(x) };
    let short_is_w = w <= h;
    if (w.min(h) * scale - f64::from(floor)).abs() < 0.5 {
        return (at_floor(w * scale, short_is_w), at_floor(h * scale, !short_is_w));
    }
    (even(w * scale), even(h * scale))
}

/// `fps` capped at `max` frames per second.
fn cap_fps(fps: (u32, u32), max: u32) -> (u32, u32) {
    if fps.1 == 0 || f64::from(fps.0) / f64::from(fps.1) > f64::from(max) { (max, 1) } else { fps }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fit::target::VideoTargetRegistry;
    use crate::time::Ts;

    fn reddit() -> VideoTarget {
        VideoTargetRegistry::builtin().get("reddit-video").unwrap().clone()
    }

    fn src(len: f64, size: (u32, u32)) -> Source {
        Source { len, size, fps: (30, 1), has_audio: true }
    }

    fn opts(max_size: Option<u64>) -> FitOptions {
        FitOptions { max_size, ..FitOptions::default() }
    }

    /// The length limit is whole frames: at 29.97 fps a 10 s limit is 299 frames (9.977 s), since
    /// 300 frames would last 10.010 s. A range at the limit loses less than a frame and is not
    /// a trim; a longer one is trimmed to 299 frames.
    #[test]
    fn the_length_limit_is_whole_frames() {
        let ntsc = |len| Source { fps: (30000, 1001), ..src(len, (640, 360)) };
        let ten = FitOptions { max_length: Some(Ts::from_secs_f64(10.0)), allow_trim: true, ..opts(None) };
        let frames = 299.0 * 1001.0 / 30000.0;
        let at = plan_fit(&reddit(), &ten, ntsc(10.0)).unwrap();
        assert!((at.len - frames).abs() < 1e-9 && at.trimmed_from.is_none(), "{at:?}");
        let over = plan_fit(&reddit(), &ten, ntsc(12.0)).unwrap();
        assert!((over.len - frames).abs() < 1e-9 && over.trimmed_from == Some(12.0), "{over:?}");
        // Within a frame of the limit but whole frames: unchanged.
        let under = plan_fit(&reddit(), &ten, ntsc(9.97)).unwrap();
        assert_eq!(under.len, 9.97);
        // A picture too small for 1 kb/s still gets a real bitrate.
        let tiny = plan_fit(&reddit(), &opts(None), Source { fps: (30, 1), ..src(5.0, (2, 2)) }).unwrap();
        assert!(tiny.video_kbps >= 1, "{tiny:?}");
        // At 30 fps the limit is exact.
        let exact = plan_fit(&reddit(), &ten, src(10.0, (640, 360))).unwrap();
        assert_eq!(exact.len, 10.0);
    }

    /// Research R3: a generous cap is not filled.
    #[test]
    fn a_generous_cap_is_quality_mode_at_the_ceiling() {
        let p = plan_fit(&reddit(), &opts(None), src(20.0, (1920, 1080))).unwrap();
        assert_eq!((p.mode, p.video_kbps, p.crf, p.audio_kbps), (Mode::Quality, 7465, Some(20), Some(128)));
        assert_eq!((p.size, p.fps, p.cap), ((1920, 1080), (30, 1), 1 << 30));
        assert_eq!(p.predicted_bytes, (7465 + 128) * 125 * 20 + 7465 * 125 * 2, "with the VBV buffer");
        // A short range needs room for the buffer too: 2 s at the ceiling only in budget mode.
        let short = plan_fit(&reddit(), &opts(Some(3_000_000)), src(2.0, (1920, 1080))).unwrap();
        assert_eq!(short.mode, Mode::Budget);
        assert!(p.lowered.is_empty() && p.trimmed_from.is_none());
    }

    /// contracts/cli.md's dry-run example.
    #[test]
    fn a_tight_cap_is_budget_mode_with_its_bitrate() {
        let p = plan_fit(&reddit(), &opts(Some(100_000_000)), src(180.0, (1920, 1080))).unwrap();
        assert_eq!((p.mode, p.video_kbps, p.crf, p.predicted_bytes), (Mode::Budget, 4227, None, 97_987_500));
    }

    /// Research R4: below `min_bpp` the size steps down by 0.75.
    #[test]
    fn the_ladder_lowers_the_size_until_the_minimum_fits() {
        let p = plan_fit(&reddit(), &opts(Some(50_000_000)), src(300.0, (1920, 1080))).unwrap();
        assert_eq!((p.mode, p.size), (Mode::Budget, (1440, 810)));
        assert_eq!(p.lowered.len(), 1);
        assert_eq!(
            (p.lowered[0].setting.as_str(), &p.lowered[0].to),
            ("size", &serde_json::json!([1440, 810]))
        );
    }

    #[test]
    fn at_the_floor_the_audio_is_lowered_then_the_job_is_refused() {
        // 600 s at 854x480 needs 368.9 kb/s of video; 33.2 MB leaves room for 64 kb/s of audio.
        let p = plan_fit(&reddit(), &opts(Some(33_200_000)), src(600.0, (1920, 1080))).unwrap();
        assert_eq!((p.size, p.audio_kbps, p.video_kbps), ((854, 480), Some(64), 369));
        assert_eq!(p.lowered.iter().map(|l| l.setting.as_str()).collect::<Vec<_>>(), ["size", "audio_kbps"]);
        let e = plan_fit(&reddit(), &opts(Some(8_000_000)), src(600.0, (1920, 1080))).unwrap_err();
        assert_eq!(
            e,
            Refusal::Unreachable { min_cap: 33_132_245, floor: (854, 480), len: 600.0, cap: 8_000_000 }
        );
        assert!(
            e.message("reddit-video")
                .starts_with("600 s cannot fit 8.0 MB even at 854x480 (needs at least 33.1 MB)"),
            "{}",
            e.message("reddit-video")
        );
    }

    #[test]
    fn trimming_keeps_the_longest_start_that_fits() {
        let o = FitOptions { allow_trim: true, ..opts(Some(8_000_000)) };
        let p = plan_fit(&reddit(), &o, src(600.0, (1920, 1080))).unwrap();
        assert_eq!(p.trimmed_from, Some(600.0));
        assert!((p.len - 144.87).abs() < 0.01, "{}", p.len);
        assert_eq!((p.size, p.audio_kbps, p.video_kbps), ((854, 480), Some(64), 368));
        assert!(p.predicted_bytes <= 7_840_000);
    }

    /// Research R5.
    #[test]
    fn a_range_over_the_length_limit_is_refused_or_trimmed() {
        let e = plan_fit(&reddit(), &opts(None), src(1200.0, (1920, 1080))).unwrap_err();
        assert_eq!(e, Refusal::TooLong { len: 1200.0, max_length: 900.0 });
        assert_eq!(
            e.message("reddit-video"),
            "the range (20:00) is longer than reddit-video allows (15:00); use --fit-allow-trim to keep the first 15:00"
        );
        let o = FitOptions { allow_trim: true, ..opts(None) };
        let p = plan_fit(&reddit(), &o, src(1200.0, (1920, 1080))).unwrap();
        assert_eq!((p.len, p.trimmed_from), (900.0, Some(1200.0)));
        let short = FitOptions { max_length: Some(Ts::from_secs_f64(60.0)), ..opts(None) };
        assert_eq!(
            plan_fit(&reddit(), &short, src(61.0, (1920, 1080))).unwrap_err(),
            Refusal::TooLong { len: 61.0, max_length: 60.0 }
        );
        // Less than a second over: the two lengths never read the same.
        let e = Refusal::TooLong { len: 900.4, max_length: 900.0 };
        assert_eq!(
            e.message("reddit-video"),
            "the range (15:00.4) is longer than reddit-video allows (15:00.0); use --fit-allow-trim to keep the first 15:00.0"
        );
    }

    #[test]
    fn an_override_may_only_lower_the_targets_limits() {
        let big = opts(Some(2 << 30));
        assert!(matches!(plan_fit(&reddit(), &big, src(10.0, (1920, 1080))), Err(Refusal::AboveTarget(_))));
        let long = FitOptions { max_length: Some(Ts::from_secs_f64(1000.0)), ..opts(None) };
        assert!(matches!(plan_fit(&reddit(), &long, src(10.0, (1920, 1080))), Err(Refusal::AboveTarget(_))));
    }

    #[test]
    fn without_audio_the_whole_budget_is_video() {
        let s = Source { has_audio: false, ..src(180.0, (1920, 1080)) };
        let p = plan_fit(&reddit(), &opts(Some(100_000_000)), s).unwrap();
        assert_eq!((p.audio_kbps, p.video_kbps), (None, 4355));
    }

    #[test]
    fn sizes_are_never_enlarged_and_fps_is_capped() {
        let plan = |size, fps| plan_fit(&reddit(), &opts(None), Source { fps, ..src(10.0, size) }).unwrap();
        assert_eq!(plan((640, 360), (30, 1)).size, (640, 360));
        assert_eq!(plan((3840, 2160), (30, 1)).size, (1920, 1080));
        assert_eq!(plan((1080, 1920), (30, 1)).size, (1080, 1920), "portrait keeps its long side");
        assert_eq!(plan((1281, 721), (30, 1)).size, (1280, 720), "odd sizes round down to even");
        assert_eq!(plan((1920, 1080), (120, 1)).fps, (60, 1));
        assert_eq!(plan((1920, 1080), (30000, 1001)).fps, (30000, 1001));
    }

    /// The invariant behind FR-005: every accepted plan predicts at most the usable share of the
    /// cap, with even sizes no larger than the source.
    #[test]
    fn every_accepted_plan_stays_within_the_usable_cap() {
        let mut accepted = 0;
        for cap in [70_000, 1_000_000, 8_000_000, 25_000_000, 100_000_000, 1 << 30] {
            for len in [0.5, 2.0, 30.0, 180.0, 600.0, 900.0] {
                for size in [(3840, 2160), (1920, 1080), (1080, 1920), (1280, 720), (640, 360), (1440, 1440)]
                {
                    for audio in [true, false] {
                        for allow_trim in [false, true] {
                            let o = FitOptions { allow_trim, ..opts(Some(cap)) };
                            let s = Source { has_audio: audio, ..src(len, size) };
                            let Ok(p) = plan_fit(&reddit(), &o, s) else { continue };
                            accepted += 1;
                            assert!(p.predicted_bytes as f64 <= cap as f64 * (1.0 - MARGIN), "{p:?}");
                            assert!(p.video_kbps > 0 && p.len > 0.0 && p.len <= len, "{p:?}");
                            assert!(p.size.0.is_multiple_of(2) && p.size.1.is_multiple_of(2), "{p:?}");
                            assert!(p.size.0 <= size.0 && p.size.1 <= size.1, "{p:?}");
                        }
                    }
                }
            }
        }
        assert!(accepted > 300, "{accepted}");
    }
}
