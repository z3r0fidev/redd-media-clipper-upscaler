//! The bounded, deterministic size-budget search (research R5). [`search`] walks a fixed ladder
//! of settings (gifsicle lossy, dither, colours, frame rate, width) through a [`Sampler`], so the
//! decisions depend only on byte counts, and records every step.

use serde::Serialize;
use serde_json::{Value, json};

use super::plan::{Lowered, Settings};
use super::target::{Budget, Dither, GifskiParams};
use crate::error::{Error, Result};

/// The highest gifsicle `--lossy` level the search uses.
pub const MAX_LOSSY: u16 = 200;
/// The lossy rungs: the target's level plus these.
const LOSSY_STEPS: [u16; 2] = [30, 60];
/// The frame rates the fps rung steps through (down to the target's floor).
const FPS_RUNGS: [u32; 4] = [20, 15, 12, 10];
/// The colours rung stops here.
const MIN_COLOURS: u16 = 128;
/// gifski's `lossy_quality` steps (R14): unset (100) → 80 → 60.
const GIFSKI_LOSSY: [u8; 2] = [80, 60];
/// 7 samples + 1 full + 1 retry (R5); the executor asserts ≤ 10.
pub const MAX_ENCODES: u8 = 9;

/// What a sample (or a post-pass on it) predicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Estimate {
    /// The sample file's size.
    pub sample_bytes: u64,
    /// The predicted size of the whole GIF.
    pub bytes: u64,
    /// The part that does not grow with the length: header and first frame.
    pub fixed: u64,
}

/// The encodes the search asks for. The executor's implementation runs ffmpeg (and gifsicle,
/// gifski); the tests script the sizes.
#[allow(async_fn_in_trait)]
pub trait Sampler {
    /// Encodes a sample at `s` (an encode).
    async fn sample(&mut self, s: &Settings) -> Result<Estimate>;
    /// Re-runs gifsicle on the last sample at `s.lossy` (a post-pass, not an encode).
    async fn post(&mut self, s: &Settings) -> Result<Estimate>;
    /// Encodes the whole GIF at `s`, over `len` seconds from the range's start (shorter when
    /// trimmed); its size in bytes.
    async fn full(&mut self, s: &Settings, len: f64) -> Result<u64>;
}

/// What the search starts from.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchInput {
    pub start: Settings,
    pub floor_width: u32,
    pub floor_fps: u32,
    pub budget: Budget,
    /// gifsicle is there: the lossy rungs apply.
    pub optimiser: bool,
    /// The user allows trimming to fit (FR-011).
    pub allow_trim: bool,
    /// The range's length in seconds.
    pub len: f64,
}

/// The search's result: the full GIF at `chosen` was encoded and is `bytes` long.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub chosen: Settings,
    pub bytes: u64,
    pub record: SearchRecord,
    /// The range was trimmed to this many seconds (`allow_trim`).
    pub trimmed_to: Option<f64>,
    /// Over the soft limit, within the hard one (`OVER_RECOMMENDED`).
    pub over_recommended: bool,
}

/// The ladder's rungs, in order (FR-009). The built-in engine takes dither and colours; gifski
/// ignores those and takes its `lossy_quality` twice instead (R14). Each sees `None` for the
/// other engine's rungs, so either ladder is at most four samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rung {
    Dither,
    Colours,
    GifskiLossy,
    Fps,
    Width,
}

const RUNGS: [Rung; 6] =
    [Rung::Dither, Rung::Colours, Rung::GifskiLossy, Rung::GifskiLossy, Rung::Fps, Rung::Width];

fn lower_dither(d: Dither) -> Option<Dither> {
    match d {
        Dither::FloydSteinberg | Dither::Sierra2_4a => Some(Dither::Bayer(4)),
        Dither::Bayer(k) if k < 4 => Some(Dither::Bayer(4)),
        Dither::Bayer(4) => Some(Dither::Bayer(5)),
        _ => None,
    }
}

fn fps_of(rate: (u32, u32)) -> f64 {
    f64::from(rate.0) / f64::from(rate.1.max(1))
}

/// `s` at width `w`, its height scaled to keep the shape (even, at least 2).
fn at_width(s: &Settings, w: u32) -> Settings {
    let h = (f64::from(w) * f64::from(s.height) / f64::from(s.width.max(1)) / 2.0).round() as u32 * 2;
    Settings { width: w, height: h.max(2), ..s.clone() }
}

/// One step down on `rung`, or `None` when `s` is at its floor there.
fn lower(rung: Rung, s: &Settings, input: &SearchInput) -> Option<Settings> {
    let builtin = s.gifski.is_none();
    match rung {
        Rung::Dither if builtin => lower_dither(s.dither).map(|dither| Settings { dither, ..s.clone() }),
        Rung::Colours if builtin => (s.colours > MIN_COLOURS)
            .then(|| Settings { colours: (s.colours * 3 / 4).max(MIN_COLOURS), ..s.clone() }),
        Rung::Dither | Rung::Colours => None,
        Rung::GifskiLossy => {
            let p = s.gifski?;
            let now = p.lossy_quality.unwrap_or(100);
            let next = GIFSKI_LOSSY.into_iter().find(|&q| q < now)?;
            Some(Settings { gifski: Some(GifskiParams { lossy_quality: Some(next), ..p }), ..s.clone() })
        }
        Rung::Fps => {
            let fps = fps_of(s.rate);
            FPS_RUNGS
                .iter()
                .find(|&&r| f64::from(r) < fps - 1e-9 && r >= input.floor_fps)
                .map(|&r| Settings { rate: (r, 1), ..s.clone() })
        }
        Rung::Width => {
            let w = ((f64::from(s.width) * 0.85) as u32 & !1).max(input.floor_width);
            (w < s.width).then(|| at_width(s, w))
        }
    }
}

/// `s` with every setting at its floor, and the highest lossy level when gifsicle runs.
fn floors(s: &Settings, input: &SearchInput) -> Settings {
    let mut f = s.clone();
    for rung in RUNGS {
        while let Some(next) = lower(rung, &f, input) {
            f = next;
        }
    }
    if input.optimiser && f.gifski.is_none() {
        f.lossy = f.lossy.max(max_lossy(input));
    }
    f
}

fn max_lossy(input: &SearchInput) -> u16 {
    (input.start.lossy + LOSSY_STEPS[LOSSY_STEPS.len() - 1]).min(MAX_LOSSY)
}

/// The length that fits `hard` at estimate `e` for a range of `len` s (R5, no extra margin).
fn fits_secs(len: f64, hard: u64, e: Estimate) -> f64 {
    let room = hard.saturating_sub(e.fixed) as f64;
    let grows = e.bytes.saturating_sub(e.fixed) as f64;
    if grows <= 0.0 { len } else { (len * room / grows).clamp(0.0, len) }
}

/// Every setting lowered from `start` to `chosen`, in ladder order (FR-012).
fn lowered(start: &Settings, chosen: &Settings) -> Vec<Lowered> {
    let mut out = Vec::new();
    let mut push = |setting: &str, from: Value, to: Value| {
        if from != to {
            out.push(Lowered { setting: setting.into(), from, to });
        }
    };
    push("lossy", json!(start.lossy), json!(chosen.lossy));
    push("dither", json!(start.dither), json!(chosen.dither));
    push("colours", json!(start.colours), json!(chosen.colours));
    push(
        "fps",
        json!((fps_of(start.rate) * 1000.0).round() / 1000.0),
        json!((fps_of(chosen.rate) * 1000.0).round() / 1000.0),
    );
    push("width", json!(start.width), json!(chosen.width));
    let lossy_quality = |s: &Settings| s.gifski.map(|p| p.lossy_quality.unwrap_or(100));
    push("lossy_quality", json!(lossy_quality(start)), json!(lossy_quality(chosen)));
    out
}

struct Log {
    steps: Vec<SearchStep>,
    encodes: u8,
}

impl Log {
    fn push(&mut self, kind: StepKind, s: &Settings, bytes: u64, estimate: u64) {
        if kind != StepKind::Post {
            self.encodes += 1;
        }
        let n = self.steps.len() as u32 + 1;
        self.steps.push(SearchStep { n, kind, bytes, estimate, settings: s.to_json() });
    }
}

/// Finds the settings that fit `input.budget` and encodes the GIF at them (research R5).
/// `Error::BudgetUnreachable` when even the floors (or the retried full encode) are over the
/// hard limit and trimming is not allowed; nothing is kept then.
pub async fn search(input: &SearchInput, sampler: &mut impl Sampler) -> Result<Found> {
    let Budget { soft, hard } = input.budget;
    let mut log = Log { steps: Vec::new(), encodes: 0 };
    let mut cur = input.start.clone();

    // s1, then the free lossy rungs on it.
    let mut est = sampler.sample(&cur).await?;
    log.push(StepKind::Sample, &cur, est.sample_bytes, est.bytes);
    if est.bytes > soft && input.optimiser && cur.gifski.is_none() {
        for add in LOSSY_STEPS {
            let l = (input.start.lossy + add).min(MAX_LOSSY);
            if l <= cur.lossy {
                continue;
            }
            cur.lossy = l;
            est = sampler.post(&cur).await?;
            log.push(StepKind::Post, &cur, est.sample_bytes, est.bytes);
            if est.bytes <= soft {
                break;
            }
        }
    }

    // s2…s5: one step down per rung, in order.
    let mut last_rung = None;
    if est.bytes > soft {
        for (i, rung) in RUNGS.iter().enumerate() {
            let Some(next) = lower(*rung, &cur, input) else { continue };
            cur = next;
            est = sampler.sample(&cur).await?;
            log.push(StepKind::Sample, &cur, est.sample_bytes, est.bytes);
            last_rung = Some(i);
            if est.bytes <= soft {
                break;
            }
        }
    }

    // s6, the floors; then s7, interpolating the width; or trimming.
    let mut len = input.len;
    let mut trimmed_to = None;
    if est.bytes > soft {
        let (last, last_est) = (cur.clone(), est);
        let floor = floors(&cur, input);
        if floor != cur {
            cur = floor;
            est = sampler.sample(&cur).await?;
            log.push(StepKind::Sample, &cur, est.sample_bytes, est.bytes);
        }
        if est.bytes > hard {
            let fits = fits_secs(input.len, hard, est);
            let one_frame = 1.0 / fps_of(cur.rate);
            if !input.allow_trim || fits < one_frame {
                return Err(Error::BudgetUnreachable {
                    fits_secs: Some(fits),
                    best_bytes: Some(est.bytes),
                    min_bytes: None,
                });
            }
            len = fits;
            trimmed_to = Some(fits);
        } else if est.bytes <= soft && last.width > cur.width && last_est.bytes > est.bytes {
            // The size grows with the area: interpolate width² between the floors and the last
            // rung (R5's linear-in-width form overshoots soft; see research R5).
            let share = (soft - est.bytes) as f64 / (last_est.bytes - est.bytes) as f64;
            let (wf, wl) = (f64::from(cur.width), f64::from(last.width));
            let w = (wf * wf + (wl * wl - wf * wf) * share).sqrt() as u32 & !1;
            if w > cur.width && w < last.width {
                let wider = at_width(&cur, w);
                let e = sampler.sample(&wider).await?;
                log.push(StepKind::Sample, &wider, e.sample_bytes, e.bytes);
                if e.bytes <= soft {
                    (cur, est) = (wider, e);
                }
            }
        }
    }

    // The full encode, and one more step down when it misses soft.
    let mut bytes = sampler.full(&cur, len).await?;
    log.push(StepKind::Full, &cur, bytes, est.bytes);
    if bytes > soft && log.encodes < MAX_ENCODES {
        let next_rung = RUNGS
            .iter()
            .skip(last_rung.map_or(0, |i| i + 1))
            .find_map(|r| lower(*r, &cur, input))
            .or_else(|| (cur.width > input.floor_width).then(|| at_width(&cur, input.floor_width)));
        if let Some(next) = next_rung {
            cur = next;
            bytes = sampler.full(&cur, len).await?;
            log.push(StepKind::Full, &cur, bytes, est.bytes);
        }
    }
    if bytes > hard {
        let real = Estimate { sample_bytes: bytes, bytes, fixed: est.fixed };
        return Err(Error::BudgetUnreachable {
            fits_secs: Some(fits_secs(len, hard, real)),
            best_bytes: Some(bytes),
            min_bytes: None,
        });
    }
    let fit = if bytes <= soft { Fit::Soft } else { Fit::Hard };
    let record =
        SearchRecord { encodes: log.encodes, fit, lowered: lowered(&input.start, &cur), steps: log.steps };
    Ok(Found { chosen: cur, bytes, record, trimmed_to, over_recommended: bytes > soft })
}

/// What a step of the search was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifStepKind"))]
pub enum StepKind {
    /// A sample encode (counts as an encode).
    Sample,
    /// A gifsicle re-run on a sample (not an encode).
    Post,
    /// A full encode.
    Full,
}

/// One step of the search.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifSearchStep"))]
pub struct SearchStep {
    pub n: u32,
    pub kind: StepKind,
    pub bytes: u64,
    /// The predicted size of the whole GIF at this step's settings.
    pub estimate: u64,
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub settings: serde_json::Value,
}

/// Which limit the result is under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifFit"))]
pub enum Fit {
    Soft,
    Hard,
    None,
}

/// The search's record (data-model.md › SearchRecord).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifSearchRecord"))]
pub struct SearchRecord {
    pub encodes: u8,
    pub fit: Fit,
    pub lowered: Vec<Lowered>,
    pub steps: Vec<SearchStep>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gif::target::StatsMode;

    fn start() -> Settings {
        Settings {
            width: 720,
            height: 405,
            rate: (15, 1),
            colours: 256,
            dither: Dither::Sierra2_4a,
            stats: StatsMode::Diff,
            lossy: 30,
            gifski: None,
            crf: None,
        }
    }

    fn input(soft: u64, hard: u64) -> SearchInput {
        SearchInput {
            start: start(),
            floor_width: 320,
            floor_fps: 10,
            budget: Budget { soft, hard },
            optimiser: true,
            allow_trim: false,
            len: 10.0,
        }
    }

    /// A size model: bytes per second grow with area, fps, colours, dither and fall with lossy.
    /// `full_bias` scales the full encodes against the samples' prediction.
    struct Fake {
        per_sec: f64,
        full_bias: f64,
        fixed: u64,
        calls: Vec<String>,
        last_sample: Option<Settings>,
        fulls: Vec<f64>,
    }

    impl Fake {
        fn new(per_sec: f64) -> Fake {
            Fake {
                per_sec,
                full_bias: 1.0,
                fixed: 20_000,
                calls: Vec::new(),
                last_sample: None,
                fulls: Vec::new(),
            }
        }

        fn size(&self, s: &Settings, len: f64) -> u64 {
            let area = f64::from(s.width * s.height) / (720.0 * 405.0);
            let dither = match s.dither {
                Dither::FloydSteinberg | Dither::Sierra2_4a => 1.0,
                Dither::Bayer(k) => 0.95 - 0.03 * f64::from(k),
                Dither::None => 0.7,
            };
            let colours = (f64::from(s.colours) / 256.0).sqrt();
            let lossy = 1.0 - f64::from(s.lossy) / 400.0;
            self.fixed + (self.per_sec * len * area * fps_of(s.rate) / 15.0 * dither * colours * lossy) as u64
        }

        fn estimate(&self, s: &Settings) -> Estimate {
            Estimate { sample_bytes: self.size(s, 3.0), bytes: self.size(s, 10.0), fixed: self.fixed }
        }
    }

    impl Sampler for Fake {
        async fn sample(&mut self, s: &Settings) -> Result<Estimate> {
            self.calls.push(format!(
                "sample {}x{} {}fps c{} {:?} l{}",
                s.width,
                s.height,
                fps_of(s.rate),
                s.colours,
                s.dither,
                s.lossy
            ));
            self.last_sample = Some(s.clone());
            Ok(self.estimate(s))
        }

        async fn post(&mut self, s: &Settings) -> Result<Estimate> {
            let last = self.last_sample.as_ref().expect("a post-pass follows a sample");
            assert_eq!(Settings { lossy: s.lossy, ..last.clone() }, *s, "a post-pass only changes lossy");
            self.calls.push(format!("post l{}", s.lossy));
            Ok(self.estimate(s))
        }

        async fn full(&mut self, s: &Settings, len: f64) -> Result<u64> {
            self.calls.push(format!("full {}x{}", s.width, s.height));
            self.fulls.push(len);
            Ok((self.size(s, len) as f64 * self.full_bias) as u64)
        }
    }

    fn run(i: &SearchInput, f: &mut Fake) -> Result<Found> {
        tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(search(i, f))
    }

    #[test]
    fn a_fitting_start_is_one_sample_and_one_full() {
        let mut f = Fake::new(100_000.0);
        let found = run(&input(8_000_000, 20_000_000), &mut f).unwrap();
        assert_eq!(f.calls, ["sample 720x405 15fps c256 Sierra2_4a l30", "full 720x405"]);
        assert_eq!((found.record.encodes, found.record.fit), (2, Fit::Soft));
        assert!(found.record.lowered.is_empty());
        assert!(!found.over_recommended);
    }

    #[test]
    fn the_ladder_runs_lossy_then_dither_colours_fps_width() {
        // Over soft until width: every rung is taken once, in order.
        let mut f = Fake::new(1_200_000.0);
        let i = input(4_000_000, 8_000_000);
        let found = run(&i, &mut f).unwrap();
        let kinds: Vec<&str> = f.calls.iter().map(|c| c.split(' ').next().unwrap()).collect();
        assert_eq!(
            &kinds[..7],
            ["sample", "post", "post", "sample", "sample", "sample", "sample"],
            "{:#?}",
            f.calls
        );
        assert!(f.calls[1].ends_with("l60") && f.calls[2].ends_with("l90"), "{:?}", f.calls);
        assert!(f.calls[3].contains("Bayer(4)"), "dither first: {}", f.calls[3]);
        assert!(f.calls[4].contains("c192"), "then colours: {}", f.calls[4]);
        assert!(f.calls[5].contains("12fps"), "then fps: {}", f.calls[5]);
        assert!(f.calls[6].starts_with("sample 612x"), "then width: {}", f.calls[6]);
        let settings: Vec<&str> = found.record.lowered.iter().map(|l| l.setting.as_str()).collect();
        assert_eq!(settings[..4], ["lossy", "dither", "colours", "fps"], "{settings:?}");
        let posts = found.record.steps.iter().filter(|s| s.kind == StepKind::Post).count() as u8;
        assert_eq!(
            found.record.encodes as usize,
            found.record.steps.len() - posts as usize,
            "posts are not encodes"
        );
    }

    #[test]
    fn without_gifsicle_there_are_no_lossy_rungs() {
        let mut f = Fake::new(1_200_000.0);
        let i = SearchInput { optimiser: false, ..input(4_000_000, 8_000_000) };
        run(&i, &mut f).unwrap();
        assert!(!f.calls.iter().any(|c| c.starts_with("post")), "{:?}", f.calls);
    }

    #[test]
    fn rungs_at_their_floor_are_skipped() {
        let mut f = Fake::new(1_200_000.0);
        let mut i = input(4_000_000, 8_000_000);
        i.start.colours = 128;
        i.start.dither = Dither::None;
        run(&i, &mut f).unwrap();
        let samples: Vec<&String> = f.calls.iter().filter(|c| c.starts_with("sample")).collect();
        assert!(samples[1].contains("12fps"), "dither and colours skipped: {:?}", f.calls);
    }

    #[test]
    fn floors_over_hard_are_unreachable_with_fits_secs() {
        let mut f = Fake::new(20_000_000.0);
        let i = input(3_000_000, 8_000_000);
        let Err(Error::BudgetUnreachable { fits_secs: Some(fits), best_bytes: Some(best), .. }) =
            run(&i, &mut f)
        else {
            panic!("{:?}", f.calls)
        };
        let floor = floors(&f.last_sample.clone().unwrap(), &i);
        let e = f.estimate(&floor);
        assert_eq!(best, e.bytes);
        let want = 10.0 * (8_000_000 - e.fixed) as f64 / (e.bytes - e.fixed) as f64;
        assert!((fits - want).abs() < 1e-9, "{fits} vs {want}: no extra margin");
        assert!(!f.calls.iter().any(|c| c.starts_with("full")), "nothing is encoded in full");
    }

    #[test]
    fn allow_trim_encodes_the_part_that_fits() {
        let mut f = Fake::new(20_000_000.0);
        let i = SearchInput { allow_trim: true, ..input(3_000_000, 8_000_000) };
        let found = run(&i, &mut f).unwrap();
        let fits = found.trimmed_to.expect("trimmed");
        assert!(fits > 0.0 && fits < 10.0, "{fits}");
        assert_eq!(f.fulls, [fits], "the full encode covers the trimmed range");
        assert!(found.bytes <= 8_000_000);
    }

    #[test]
    fn the_width_is_interpolated_between_the_floors_and_the_last_rung() {
        // The last rung is just over soft and the floors well under: s7 lands in between.
        let mut f = Fake::new(1_300_000.0);
        let i = input(3_600_000, 8_000_000);
        let found = run(&i, &mut f).unwrap();
        let samples: Vec<&SearchStep> =
            found.record.steps.iter().filter(|s| s.kind == StepKind::Sample).collect();
        let widths: Vec<u64> = samples.iter().map(|s| s.settings["width"].as_u64().unwrap()).collect();
        let (last, floor, interp) =
            (widths[widths.len() - 3], widths[widths.len() - 2], widths[widths.len() - 1]);
        assert_eq!(floor, 320, "{widths:?}");
        assert!(interp > floor && interp < last, "{widths:?}");
        assert_eq!(found.chosen.width as u64, interp, "the wider fitting sample is kept");
        assert!(found.bytes <= 3_600_000);
    }

    #[test]
    fn a_full_encode_over_soft_is_retried_one_step_down() {
        let mut f = Fake::new(100_000.0);
        f.full_bias = 2.0;
        let soft = f.size(&start(), 10.0) + 1000;
        let found = run(&input(soft, soft * 3), &mut f).unwrap();
        let fulls: Vec<&String> = f.calls.iter().filter(|c| c.starts_with("full")).collect();
        assert_eq!(fulls.len(), 2, "{:?}", f.calls);
        assert!(
            found.record.lowered.iter().any(|l| l.setting == "dither"),
            "the next rung: {:?}",
            found.record.lowered
        );
        assert!(found.over_recommended, "still over soft, under hard");
        assert_eq!(found.record.fit, Fit::Hard);
    }

    #[test]
    fn a_retried_full_encode_over_hard_is_unreachable() {
        let mut f = Fake::new(100_000.0);
        f.full_bias = 5.0;
        let soft = f.size(&start(), 10.0) + 1000;
        let r = run(&input(soft, soft + 1), &mut f);
        assert!(matches!(r, Err(Error::BudgetUnreachable { best_bytes: Some(_), .. })), "{r:?}");
    }

    #[test]
    fn the_search_is_deterministic() {
        let go = || {
            let mut f = Fake::new(1_300_000.0);
            run(&input(3_600_000, 8_000_000), &mut f).unwrap().record
        };
        assert_eq!(go(), go());
    }

    #[test]
    fn gifski_lowers_its_lossy_quality_instead_of_dither_and_colours() {
        let mut f = Fake::new(1_200_000.0);
        let mut i = input(4_000_000, 8_000_000);
        i.start.gifski = Some(GifskiParams { quality: 90, motion_quality: None, lossy_quality: None });
        let found = run(&i, &mut f).unwrap();
        assert!(
            !f.calls.iter().any(|c| c.starts_with("post")),
            "no gifsicle on gifski output: {:?}",
            f.calls
        );
        let lq: Vec<Option<u8>> = found
            .record
            .steps
            .iter()
            .filter(|s| s.kind == StepKind::Sample)
            .map(|s| s.settings["gifski"]["lossy_quality"].as_u64().map(|v| v as u8))
            .collect();
        assert_eq!(lq[..3], [None, Some(80), Some(60)], "{:?}", found.record.steps);
        let samples: Vec<&String> = f.calls.iter().filter(|c| c.starts_with("sample")).collect();
        assert!(samples[1].contains("c256 Sierra2_4a"), "dither and colours untouched: {samples:?}");
        assert!(found.record.lowered.iter().any(|l| l.setting == "lossy_quality"));
    }

    #[test]
    fn no_scenario_takes_more_than_nine_encodes() {
        // Random monotone models: per-second size, full-encode bias, budgets and floors.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..2000 {
            let mut f = Fake::new((next() % 30_000_000) as f64 + 1000.0);
            f.full_bias = 0.5 + (next() % 300) as f64 / 100.0;
            let soft = 70_000 + next() % 20_000_000;
            let mut i = input(soft, soft + next() % 20_000_000);
            i.optimiser = next() % 2 == 0;
            i.allow_trim = next() % 2 == 0;
            i.start.colours = [256, 192, 128][(next() % 3) as usize];
            i.start.rate = [(25, 1), (20, 1), (15, 1), (10, 1)][(next() % 4) as usize];
            if next() % 3 == 0 {
                i.start.gifski =
                    Some(GifskiParams { quality: 90, motion_quality: None, lossy_quality: None });
            }
            if let Ok(found) = run(&i, &mut f) {
                assert!(found.record.encodes <= MAX_ENCODES, "{:?}", found.record);
                assert!(found.bytes <= i.budget.hard);
                assert_eq!(found.over_recommended, found.bytes > soft);
            }
            let encodes = f.calls.iter().filter(|c| !c.starts_with("post")).count();
            assert!(encodes <= MAX_ENCODES as usize, "{:#?}", f.calls);
        }
    }
}
