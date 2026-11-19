//! Running a smart job: head, copy, tail, audio, join and verify, in the job's scratch folder
//! (research R7, R9; feature 004, US1).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::JobCtx;
use super::clip::classify_failure;
use super::process::{run_capture, run_lines};
use crate::error::{Error, Result};
use crate::plan::ffargs::{ClipSpec, clip_args};
use crate::plan::streams::Container;
use crate::probe::{INPUT_PROTOCOLS, file_arg};
use crate::smart::args::{self, COPY, HEAD, LIST, TAIL};
use crate::smart::{FallbackReason, SmartPlan};
use crate::time::Ts;
use tokio_util::sync::CancellationToken;

/// The clip's other streams, written by the audio step.
pub(crate) const REST: &str = "rest.av";

/// The job's stages with their weights (research R9: an encoded frame costs 1, a copied one
/// 0.02, audio 0.05 per frame of the clip, the join 0.02 per frame, a verified frame 0.3).
struct Stages {
    list: Vec<(&'static str, f64)>,
    next: usize,
}

impl Stages {
    fn new(plan: &SmartPlan, rest: bool) -> Stages {
        let total = f64::from(plan.total_frames());
        let mut list = Vec::new();
        if let Some(h) = plan.head {
            list.push(("head", f64::from(h.frames)));
        }
        list.push(("copy", 0.02 * f64::from(plan.copy.frames)));
        if let Some(t) = plan.tail {
            list.push(("tail", f64::from(t.frames)));
        }
        if rest {
            list.push(("audio", 0.05 * total));
        }
        list.push(("join", 0.02 * total));
        list.push(("verify", 0.3 * f64::from(4 * (plan.first_gop + plan.last_gop))));
        let sum: f64 = list.iter().map(|(_, w)| w).sum();
        Stages { list: list.into_iter().map(|(n, w)| (n, w / sum)).collect(), next: 0 }
    }

    fn enter(&mut self, ctx: &mut JobCtx<'_>) {
        let (name, weight) = self.list[self.next];
        self.next += 1;
        ctx.stage(name, self.next as u32, self.list.len() as u32, weight);
    }
}

/// Makes the smart clip in `ctx.temp`. `Ok(Err(reason))`: the result failed verification and
/// was not kept; the caller falls back (or fails, in strict mode).
pub(super) async fn execute(
    spec: &ClipSpec,
    plan: &SmartPlan,
    ctx: &mut JobCtx<'_>,
) -> Result<std::result::Result<(), FallbackReason>> {
    let dir: PathBuf = ctx
        .scratch_dir()
        .ok_or_else(|| Error::VerifyFailed("a smart job needs its scratch folder".into()))?
        .to_path_buf();
    let rest = args::rest_spec(spec);
    let mut stages = Stages::new(plan, rest.is_some());
    let secs = |frames: u32| Ts::from_micros(plan.frame.as_micros() * i64::from(frames));

    let mut parts: Vec<String> = Vec::new();
    if let Some(head) = plan.head {
        stages.enter(ctx);
        let r = run(ctx, &dir, &args::part_args(spec, plan, head, &dir.join(HEAD)), secs(head.frames)).await;
        if let Err(reason) = fall_back_on(r, "head")? {
            return Ok(Err(reason));
        }
        parts.push(HEAD.to_owned());
    }
    stages.enter(ctx);
    #[allow(unused_mut)]
    let mut copy = args::copy_args(spec, plan, &dir.join(COPY));
    #[cfg(any(test, feature = "test-support"))]
    if crate::smart::test_hooks::fails_copy(&spec.input) {
        copy.insert(0, "-no-such-option".into());
    }
    if let Err(reason) = fall_back_on(run(ctx, &dir, &copy, secs(plan.copy.frames)).await, "copy")? {
        return Ok(Err(reason));
    }
    parts.push(COPY.to_owned());
    if let Some(tail) = plan.tail {
        stages.enter(ctx);
        let r = run(ctx, &dir, &args::part_args(spec, plan, tail, &dir.join(TAIL)), secs(tail.frames)).await;
        if let Err(reason) = fall_back_on(r, "tail")? {
            return Ok(Err(reason));
        }
        parts.push(TAIL.to_owned());
    }
    let rest_path = dir.join(REST);
    if let Some(r) = &rest {
        stages.enter(ctx);
        let total = r.requested.end.saturating_sub(r.requested.start);
        run(ctx, &dir, &clip_args(r, &rest_path), total).await?;
    }
    #[cfg(any(test, feature = "test-support"))]
    if crate::smart::test_hooks::reverses(&spec.input) {
        parts.reverse();
    }
    let names: Vec<&str> = parts.iter().map(String::as_str).collect();
    tokio::fs::write(dir.join(LIST), args::concat_list(&names)).await?;
    stages.enter(ctx);
    let join = args::join_args(spec, plan, rest.is_some().then_some(rest_path.as_path()), ctx.temp);
    if let Err(reason) = fall_back_on(run(ctx, &dir, &join, secs(plan.total_frames())).await, "join")? {
        return Ok(Err(reason));
    }
    stages.enter(ctx);
    verify(ctx, spec, plan).await
}

/// A video step that ffmpeg could not do is a reason to fall back (FR-008: a source accurate mode
/// can cut never fails in smart mode). A cancel, a full disk or an unwritable output stays the
/// job's error: accurate mode would meet it too. The audio step uses accurate mode's own
/// settings, so its failure stays an error as well.
fn fall_back_on(r: Result<()>, step: &str) -> Result<std::result::Result<(), FallbackReason>> {
    match r {
        Ok(()) => Ok(Ok(())),
        Err(Error::EngineFailed { message, .. }) => {
            tracing::warn!("smart cut: the {step} step failed ({message}); cutting in accurate mode");
            Ok(Err(FallbackReason::SeamCheck))
        }
        Err(e) => Err(e),
    }
}

/// Runs one ffmpeg step in the scratch folder; a failure is the job's error.
async fn run(ctx: &mut JobCtx<'_>, dir: &Path, args: &[OsString], total: Ts) -> Result<()> {
    let out = ctx.run_ffmpeg_in(Some(dir), args, total).await?;
    if out.status.success() { Ok(()) } else { Err(classify_failure(&out.stderr_tail, ctx.output)) }
}

/// Research R7: a packet scan of the joined file, then decodes of the frames around each seam:
/// no decode errors, every frame the source's size, and the copied frames identical to the
/// source's.
async fn verify(
    ctx: &mut JobCtx<'_>,
    spec: &ClipSpec,
    plan: &SmartPlan,
) -> Result<std::result::Result<(), FallbackReason>> {
    let total = plan.total_frames();
    let scan = scan_packets(&ctx.tools.ffprobe.path, ctx.temp, total as usize, ctx.cancel).await?;
    if scan.count != total as usize {
        tracing::warn!("smart cut: {} video frames joined, {total} planned", scan.count);
        return Ok(Err(FallbackReason::SeamCheck));
    }
    let mut pts = scan.times;
    pts.sort_unstable();
    let step = plan.frame.as_micros();
    let slack = (step / 20).max(2_000);
    if pts.windows(2).any(|w| (w[1] - w[0] - step).abs() > slack) {
        tracing::warn!("smart cut: uneven frame steps in the joined clip");
        return Ok(Err(FallbackReason::SeamCheck));
    }
    ctx.stage_fraction(0.2, Ts::from_micros(1_000_000));

    let output_precise = !matches!(spec.container, Container::MpegTs);
    let head = plan.head.map_or(0, |h| h.frames);
    // (first output frame of the window, frames before the seam, frames after it, whether the
    // copied frames are after the seam, the source time of the first copied frame in the window)
    let mut seams = Vec::new();
    // Always checked, even without a head (a clip that starts on a keyframe): the first copied
    // GOP must be the source's.
    let before = head.min(plan.first_gop);
    let after = plan.first_gop.min(plan.copy.frames);
    seams.push((head - before, before, after, true, plan.copy.span.start));
    if let Some(tail) = plan.tail {
        let at = head + plan.copy.frames;
        let before = plan.last_gop.min(plan.copy.frames);
        let after = tail.frames.min(plan.first_gop.max(1));
        let from = plan.copy.span.end - Ts::from_micros(step * i64::from(before));
        seams.push((at - before, before, after, false, from));
    }
    // A crafted size could overflow the product: then nothing matches and the job falls back.
    let Some(frame_bytes) = (plan.encode.width as usize)
        .checked_mul(plan.encode.height as usize)
        .and_then(|n| n.checked_mul(3))
        .map(|n| n / 2)
    else {
        return Ok(Err(FallbackReason::ParamsChanged));
    };
    for (i, (first, before, after, copied_after, source_from)) in seams.into_iter().enumerate() {
        let at = Ts::from_micros(step * i64::from(first));
        let Some(window) = decode(ctx, ctx.temp, false, at, before + after, output_precise, plan).await?
        else {
            return Ok(Err(FallbackReason::SeamCheck));
        };
        if window.len() != (before + after) as usize {
            return Ok(Err(FallbackReason::SeamCheck));
        }
        if window.iter().any(|(bytes, _)| *bytes != frame_bytes) {
            return Ok(Err(FallbackReason::ParamsChanged));
        }
        let copied = if copied_after { &window[before as usize..] } else { &window[..before as usize] };
        let n = if copied_after { after } else { before };
        let Some(source) = decode(ctx, &spec.input, true, source_from, n, plan.precise, plan).await? else {
            return Ok(Err(FallbackReason::SeamCheck));
        };
        if source.iter().map(|(_, h)| h).ne(copied.iter().map(|(_, h)| h)) {
            tracing::warn!("smart cut: copied frames differ from the source at seam {}", i + 1);
            return Ok(Err(FallbackReason::SeamCheck));
        }
        ctx.stage_fraction(0.2 + 0.4 * (i + 1) as f64, Ts::from_micros(1_000_000));
    }
    Ok(Ok(()))
}

/// The first video stream's packets in a file: how many, and their presentation times (µs).
struct PacketScan {
    count: usize,
    /// At most `planned + 1` times: more is a mismatch already.
    times: Vec<i64>,
}

/// Lists the packets' presentation times line by line (item 27: `run_capture`'s 16 MiB limit
/// is about 1.3 million packets, which a multi-hour clip passes).
async fn scan_packets(
    ffprobe: &Path,
    file: &Path,
    planned: usize,
    cancel: &CancellationToken,
) -> Result<PacketScan> {
    let args: Vec<OsString> =
        ["-v", "error", "-select_streams", "v:0", "-show_entries", "packet=pts_time", "-of", "csv=p=0"]
            .iter()
            .map(OsString::from)
            .chain([file_arg(file)])
            .collect();
    let mut scan = PacketScan { count: 0, times: Vec::with_capacity(planned.min(1 << 20) + 1) };
    run_lines(ffprobe, &args, cancel, |line| {
        let Ok(s) = line.trim().trim_end_matches(',').parse::<f64>() else { return };
        // Within ±1e12 s, so the differences below cannot overflow (T059 review).
        if !(s.is_finite() && s.abs() < 1e12) {
            return;
        }
        scan.count += 1;
        if scan.times.len() <= planned {
            scan.times.push((s * 1e6).round() as i64);
        }
    })
    .await?;
    Ok(scan)
}

/// Decodes `frames` video frames from `at` (user time of the file) and returns each frame's
/// (decoded size in bytes, MD5). `source`: the file is the source, cut like the parts (an exact
/// seek, or `trim` at the absolute time when it seeks approximately); otherwise it is the joined
/// clip. `None`: the decoder reported an error.
async fn decode(
    ctx: &JobCtx<'_>,
    file: &Path,
    source: bool,
    at: Ts,
    frames: u32,
    precise: bool,
    plan: &SmartPlan,
) -> Result<Option<Vec<(usize, String)>>> {
    // Half a frame early, so the frame at `at` is the first one kept.
    let half = Ts::from_micros(plan.frame.as_micros() / 2);
    let shift = if source { plan.shift } else { Ts::ZERO };
    let seek = (at + shift).saturating_sub(half);
    let mut a: Vec<OsString> =
        ["-hide_banner", "-nostdin", "-v", "error"].iter().map(OsString::from).collect();
    let mut trim = None;
    match (precise, source) {
        (true, _) => a.extend(["-ss".into(), OsString::from(seek.ffmpeg())]),
        (false, true) => {
            let coarse = at.saturating_sub(Ts::from_micros(10_000_000));
            a.extend(["-copyts".into(), "-ss".into(), OsString::from(coarse.ffmpeg())]);
            trim = Some(format!("trim=start={}", (plan.zero + at).saturating_sub(half).ffmpeg()));
        }
        (false, false) => {}
    }
    a.extend(["-protocol_whitelist".into(), INPUT_PROTOCOLS.into(), "-i".into(), file_arg(file)]);
    if !precise && !source && seek > Ts::ZERO {
        // A joined MPEG-TS clip: decode from its start and drop what is before the window.
        a.extend(["-ss".into(), OsString::from(seek.ffmpeg())]);
    }
    let map = if source { format!("0:{}", plan.video_index) } else { "0:v:0".to_owned() };
    a.extend(["-map".into(), map.into()]);
    if let Some(t) = trim {
        a.extend(["-vf".into(), t.into()]);
    }
    a.extend(
        ["-frames:v", &frames.to_string(), "-noautoscale", "-f", "framemd5", "-"].iter().map(OsString::from),
    );
    let out = run_capture(&ctx.tools.ffmpeg.path, &a, ctx.cancel).await?;
    // Decoding the source after a coarse seek can report frames before the window whose
    // references were not read ("co located POCs unavailable"); that is not about the clip, and
    // the source's frames are compared by MD5 anyway. Only the joined clip must decode silently.
    if !out.status.success() || (!source && !out.stderr_tail.trim().is_empty()) {
        tracing::warn!("smart cut: decode check: {}", out.stderr_tail.trim());
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let mut f = l.rsplit(',');
                let hash = f.next()?.trim().to_owned();
                let bytes = f.next()?.trim().parse().ok()?;
                Some((bytes, hash))
            })
            .collect(),
    ))
}

// Its one test needs a shell script, so the module is Unix-only (an unused import on Windows).
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Item 27: the packet scan of a multi-hour clip prints more than `run_capture`'s 16 MiB
    /// (about 1.3 million packets); it is read line by line instead, and only what the check
    /// needs is kept.
    #[tokio::test]
    async fn packet_scan_reads_past_16_mib() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let probe = d.path().join("ffprobe");
        // 2,000,000 lines of 15 bytes: about 29 MiB.
        std::fs::write(&probe, "#!/bin/sh\nyes 12345.678000, | head -n 2000000\n").unwrap();
        std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let cancel = tokio_util::sync::CancellationToken::new();
        // The old route: capturing the whole output fails past 16 MiB.
        let err = run_capture(&probe, &[], &cancel).await.unwrap_err();
        assert!(err.to_string().contains("16 MiB"), "{err}");
        let scan = scan_packets(&probe, Path::new("joined.mp4"), 2_000_000, &cancel).await.unwrap();
        assert_eq!((scan.count, scan.times.len()), (2_000_000, 2_000_000));
        assert_eq!(scan.times[0], 12_345_678_000);
        // More packets than planned: counted, but only the planned number plus one is kept.
        let scan = scan_packets(&probe, Path::new("joined.mp4"), 10, &cancel).await.unwrap();
        assert_eq!((scan.count, scan.times.len()), (2_000_000, 11));
    }
}
