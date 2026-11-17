//! Keyframe index (research R1, R2): the first video stream's packets read with ffprobe, without
//! decoding. A plan reads a window around the range and keeps every packet, so frames can be
//! counted; the desktop reads the whole file and keeps keyframes only.

use std::ffi::OsString;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::event::Span;
use crate::exec::process::run_capture;
use crate::probe::{INPUT_PROTOCOLS, MediaInfo, file_arg};
use crate::time::Ts;
use crate::tools::Tools;

/// A whole-file index stops at this many keyframes (FR-016).
pub const MAX_KEYFRAMES: usize = 20_000;
/// A whole-file index stops after this long (FR-016).
pub const MAX_INDEX_TIME: Duration = Duration::from_secs(30);
/// A whole-file index reads the file in chunks of this length, so each ffprobe call's output is
/// bounded and the caps are checked between calls.
const CHUNK: Ts = Ts::from_micros(600_000_000);
/// Chunks overlap by this much: seeks in files without an index (MPEG-TS) can land after the
/// requested point, and the overlap keeps a keyframe near a chunk boundary from being missed.
const OVERLAP: Ts = Ts::from_micros(30_000_000);
/// A window is read from this far before its start, then from further back (as
/// `probe::keyframe_at_or_before` does), until it holds a keyframe at or before its start.
const LEAD_INS: [Ts; 2] = [Ts::from_micros(12_000_000), Ts::from_micros(65_000_000)];
/// One ffprobe call may take this long before the index gives up on it.
const CALL_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct KeyframeIndex {
    /// The range read (user time); `None` = the whole file.
    pub window: Option<Span>,
    /// Sorted by time.
    pub keyframes: Vec<KeyframeEntry>,
    /// The packet spacing when it is even (constant frame rate); `None` when it is uneven.
    pub frame_duration: Option<Ts>,
    /// A cap ([`MAX_KEYFRAMES`] or [`MAX_INDEX_TIME`]) was hit.
    pub truncated: bool,
    /// Every packet read, in decode order (plan-time indexes only).
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub packets: Vec<Packet>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct KeyframeEntry {
    /// User time.
    pub at: Ts,
    /// False when a packet after this keyframe in decode order shows an earlier time: a leading
    /// picture that references the previous GOP (open GOP, research R2).
    pub clean: bool,
}

/// One video packet, in user time. `dts` may be before zero (B-frame delay).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet {
    pub pts: Ts,
    pub dts: Option<Ts>,
    pub key: bool,
}

impl KeyframeIndex {
    /// Builds the index from packets in decode order (absolute time) and the media's zero point.
    pub fn from_packets(
        raw: &[RawPacket],
        zero: Ts,
        window: Option<Span>,
        keep_packets: bool,
    ) -> KeyframeIndex {
        let user = |t: Ts| t - zero;
        // A read that starts with a seek can begin inside a GOP (MPEG-TS): the packets before the
        // first keyframe belong to a GOP whose start was not read, so they are left out.
        let packets: Vec<Packet> = raw
            .iter()
            .skip_while(|p| !p.key)
            .filter_map(|p| Some(Packet { pts: user(p.pts?), dts: p.dts.map(user), key: p.key }))
            .collect();
        let mut keyframes: Vec<KeyframeEntry> = Vec::new();
        for (i, p) in packets.iter().enumerate().filter(|(_, p)| p.key) {
            let gop = packets[i + 1..].iter().take_while(|q| !q.key);
            let clean = gop.clone().all(|q| q.pts > p.pts);
            keyframes.push(KeyframeEntry { at: p.pts, clean });
        }
        keyframes.sort_by_key(|k| k.at);
        keyframes.dedup_by_key(|k| k.at);
        KeyframeIndex {
            window,
            keyframes,
            frame_duration: frame_duration(&packets),
            truncated: false,
            packets: if keep_packets { packets } else { Vec::new() },
        }
    }

    /// The number of packets (frames) whose time is in `[from, to)`.
    pub fn frames_between(&self, from: Ts, to: Ts) -> u32 {
        self.packets.iter().filter(|p| p.pts >= from && p.pts < to).count() as u32
    }
}

/// The even packet spacing, or `None`. Packet times are rounded to the microsecond, so spacings
/// within 2 µs (or 0.1%) of the median count as equal.
fn frame_duration(packets: &[Packet]) -> Option<Ts> {
    let mut pts: Vec<i64> = packets.iter().map(|p| p.pts.as_micros()).collect();
    pts.sort_unstable();
    pts.dedup();
    let mut steps: Vec<i64> = pts.windows(2).map(|w| w[1] - w[0]).collect();
    if steps.is_empty() {
        return None;
    }
    steps.sort_unstable();
    let median = steps[steps.len() / 2];
    let slack = (median / 1000).max(2);
    let even = steps.first().is_some_and(|s| median - s <= slack)
        && steps.last().is_some_and(|s| s - median <= slack);
    (even && median > 0).then(|| Ts::from_micros(median))
}

/// A packet as ffprobe reports it (absolute time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawPacket {
    pub pts: Option<Ts>,
    pub dts: Option<Ts>,
    pub key: bool,
}

/// Parses `-show_entries packet=pts_time,dts_time,flags -of csv=p=0` output: one
/// `pts,dts,flags[,…]` line per packet. Lines that do not parse are skipped.
pub fn parse_packets(csv: &str) -> Vec<RawPacket> {
    csv.lines()
        .filter_map(|line| {
            let mut f = line.trim().split(',');
            let (pts, dts, flags) = (f.next()?, f.next()?, f.next()?);
            Some(RawPacket { pts: secs(pts), dts: secs(dts), key: flags.starts_with('K') })
        })
        .collect()
}

/// Signed seconds (`-0.080000`, `N/A` → `None`).
fn secs(v: &str) -> Option<Ts> {
    let f: f64 = v.parse().ok()?;
    // Within ±1e12 s (about 30,000 years): a hostile file's absurd time cannot overflow later
    // arithmetic.
    (f.is_finite() && f.abs() < 1e12).then(|| Ts::from_micros((f * 1e6).round() as i64))
}

/// Reads the index of the first video stream. `window` (user time) limits the read to its end,
/// starting early enough to include a keyframe at or before its start, and keeps every packet (a
/// plan's index). `None` reads the whole file in chunks, keeping keyframes only, within the caps.
pub async fn keyframes(
    tools: &Tools,
    media: &MediaInfo,
    window: Option<Span>,
    cancel: &CancellationToken,
) -> Result<KeyframeIndex> {
    keyframes_in_chunks(tools, media, window, CHUNK, cancel).await
}

/// [`keyframes`] with a chosen chunk length (tests use short chunks on short fixtures).
#[doc(hidden)]
pub async fn keyframes_in_chunks(
    tools: &Tools,
    media: &MediaInfo,
    window: Option<Span>,
    chunk: Ts,
    cancel: &CancellationToken,
) -> Result<KeyframeIndex> {
    let Some(video) = media.first_video() else {
        return Err(Error::InvalidInput("the file has no video stream".into()));
    };
    let zero = media.zero_point();
    // `None` = from the start of the file (no seek).
    let read = |from: Option<Ts>, to: Ts| {
        let interval = match from {
            Some(from) => format!("{}%{}", from.ffmpeg(), to.ffmpeg()),
            None => format!("%{}", to.ffmpeg()),
        };
        let args: Vec<OsString> = vec![
            "-v".into(),
            "error".into(),
            "-protocol_whitelist".into(),
            INPUT_PROTOCOLS.into(),
            "-select_streams".into(),
            format!("{}", video.index).into(),
            "-read_intervals".into(),
            interval.into(),
            "-show_entries".into(),
            "packet=pts_time,dts_time,flags".into(),
            "-of".into(),
            "csv=p=0".into(),
            file_arg(&media.path),
        ];
        async move {
            let run = run_capture(&tools.ffprobe.path, &args, cancel);
            let out = tokio::time::timeout(CALL_TIMEOUT, run).await.unwrap_or_else(|_| {
                Err(Error::ProbeFailed { path: media.path.clone(), message: "ffprobe timed out".into() })
            })?;
            if !out.status.success() {
                return Err(Error::ProbeFailed {
                    path: media.path.clone(),
                    message: "ffprobe could not read the packets".into(),
                });
            }
            Ok(parse_packets(&String::from_utf8_lossy(&out.stdout)))
        }
    };
    if let Some(w) = window {
        // A lead-in that reaches the start of the file reads it from the start, without a seek.
        let starts = LEAD_INS.iter().map(|l| (w.start > *l).then(|| w.start - *l + zero)).chain([None]);
        let mut index = None;
        for from in starts {
            let idx = KeyframeIndex::from_packets(&read(from, w.end + zero).await?, zero, Some(w), true);
            let found = idx.keyframes.first().is_some_and(|k| k.at <= w.start);
            index = Some(idx);
            if found || from.is_none() {
                break;
            }
        }
        return Ok(index.expect("at least one read"));
    }
    let started = Instant::now();
    let end = media.duration.map(|d| d + media.start_time);
    let mut index = KeyframeIndex {
        window: None,
        keyframes: Vec::new(),
        frame_duration: None,
        truncated: false,
        packets: Vec::new(),
    };
    let mut from = zero;
    for n in 0.. {
        let to = from + chunk;
        let start = (n > 0).then(|| from.saturating_sub(OVERLAP));
        let raw = read(start, to).await?;
        // Without a duration, read until a chunk holds nothing at or after its start (the end of
        // the file); the time and count caps still apply (T059 review).
        let past_end = raw.iter().all(|p| p.pts.or(p.dts).is_none_or(|t| t < from));
        let part = KeyframeIndex::from_packets(&raw, zero, None, false);
        index.frame_duration = match n {
            0 => part.frame_duration,
            _ => index.frame_duration.filter(|d| part.frame_duration.is_none_or(|p| p == *d)),
        };
        index.keyframes = merge(std::mem::take(&mut index.keyframes), part.keyframes);
        if index.keyframes.len() > MAX_KEYFRAMES || started.elapsed() > MAX_INDEX_TIME {
            index.keyframes.truncate(MAX_KEYFRAMES);
            index.truncated = true;
            break;
        }
        match end {
            Some(e) if to >= e => break,
            None if past_end => break,
            _ => {}
        }
        from = to;
    }
    Ok(index)
}

/// Joins two chunks' keyframes. A read starts at the keyframe before its start, so the chunks
/// share one keyframe; only the later chunk sees that keyframe's whole GOP, so a keyframe is
/// clean only if every chunk that saw it found it clean.
fn merge(mut a: Vec<KeyframeEntry>, b: Vec<KeyframeEntry>) -> Vec<KeyframeEntry> {
    a.extend(b);
    a.sort_by_key(|k| k.at);
    a.dedup_by(|later, kept| {
        let same = later.at == kept.at;
        if same {
            kept.clean &= later.clean;
        }
        same
    });
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(csv: &str) -> Vec<RawPacket> {
        parse_packets(csv)
    }

    const CLOSED: &str = "0.000000,-0.080000,K__\n0.080000,-0.040000,___\n0.040000,0.000000,___\n\
        0.120000,0.040000,___\n2.000000,1.920000,K__\n2.080000,1.960000,___\n2.040000,2.000000,___\n";
    const OPEN: &str = "0.000000,-0.080000,K__\n0.080000,-0.040000,___\n0.040000,0.000000,___\n\
        2.000000,1.880000,K__\n1.960000,1.920000,___\n2.080000,1.960000,___\n2.040000,2.000000,___\n";

    #[test]
    fn closed_gops_are_clean() {
        let idx = KeyframeIndex::from_packets(&raw(CLOSED), Ts::ZERO, None, false);
        let at: Vec<f64> = idx.keyframes.iter().map(|k| k.at.as_secs_f64()).collect();
        assert_eq!(at, [0.0, 2.0]);
        assert!(idx.keyframes.iter().all(|k| k.clean));
        assert!(idx.packets.is_empty(), "a whole-file index keeps keyframes only");
    }

    #[test]
    fn a_leading_picture_marks_its_keyframe_unclean() {
        let idx = KeyframeIndex::from_packets(&raw(OPEN), Ts::ZERO, None, true);
        let clean: Vec<bool> = idx.keyframes.iter().map(|k| k.clean).collect();
        assert_eq!(clean, [true, false]);
        assert_eq!(idx.packets.len(), 7);
    }

    #[test]
    fn times_are_user_times_and_dts_may_be_negative() {
        let idx = KeyframeIndex::from_packets(
            &raw("10.000000,9.920000,K__,\n"),
            Ts::from_micros(10_000_000),
            None,
            true,
        );
        assert_eq!(idx.keyframes[0].at, Ts::ZERO);
        assert_eq!(idx.packets[0].dts, Some(Ts::from_micros(-80_000)));
    }

    #[test]
    fn frame_duration_is_even_spacing_only() {
        let even = KeyframeIndex::from_packets(&raw(CLOSED), Ts::ZERO, None, true);
        assert_eq!(even.frame_duration, None, "the gap from 0.12 to 2.0 is uneven");
        let cfr: String = (0..50)
            .map(|i| {
                format!(
                    "{:.6},{:.6},{}\n",
                    i as f64 * 0.04,
                    i as f64 * 0.04,
                    if i % 25 == 0 { "K__" } else { "___" }
                )
            })
            .collect();
        let idx = KeyframeIndex::from_packets(&raw(&cfr), Ts::ZERO, None, true);
        assert_eq!(idx.frame_duration, Some(Ts::from_micros(40_000)));
        let flag = |i: i32| if i == 0 { "K__" } else { "___" };
        let ntsc: String = (0..60)
            .map(|i| format!("{:.6},{:.6},{}\n", f64::from(i) * 1001.0 / 30000.0, 0.0, flag(i)))
            .collect();
        let idx = KeyframeIndex::from_packets(&raw(&ntsc), Ts::ZERO, None, true);
        let d = idx.frame_duration.expect("29.97 fps rounds to the microsecond but is even").as_micros();
        assert!((33_366..=33_367).contains(&d), "{d}");
    }

    #[test]
    fn merging_chunks_keeps_a_shared_keyframe_unclean_if_either_saw_it_so() {
        let k = |s: i64, clean| KeyframeEntry { at: Ts::from_micros(s * 1_000_000), clean };
        let m = merge(vec![k(0, true), k(2, true)], vec![k(2, false), k(4, true)]);
        assert_eq!(m, [k(0, true), k(2, false), k(4, true)]);
    }

    #[test]
    fn frames_between_counts_presentation_times() {
        let idx = KeyframeIndex::from_packets(&raw(CLOSED), Ts::ZERO, None, true);
        assert_eq!(idx.frames_between(Ts::ZERO, Ts::from_micros(2_000_000)), 4);
        assert_eq!(idx.frames_between(Ts::from_micros(40_000), Ts::from_micros(2_050_000)), 5);
    }

    #[test]
    fn unparseable_lines_are_skipped_and_na_is_none() {
        let p = parse_packets("garbage\nN/A,N/A,K_\n1.0,0.9,__\n");
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].pts, p[0].key), (None, true));
    }
}
