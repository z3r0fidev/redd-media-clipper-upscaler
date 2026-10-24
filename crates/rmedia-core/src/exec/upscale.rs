//! Upscale job executor: analyze (smart reframe, unless already analysed) → render → verify
//! (data-model.md › Executor stages). US3 adds the one-time CPU retry after a GPU failure.

use super::JobCtx;
use super::clip::classify_failure;
use crate::error::{Error, Result};
use crate::event::JobMeasure;
use crate::plan::ffargs::upscale_args;
use crate::preset::VideoCodec;
use crate::probe::{self, MediaInfo, StreamKind};
use crate::time::Ts;
use crate::upscale::encode::ColourTags;
use std::path::Path;

use crate::upscale::capability::{CapabilityStore, demote};
use crate::upscale::tier::{Tier, TierDecision};
use crate::upscale::{AnalysisState, UpscaleSpec, reframe};

pub(super) async fn execute(spec: &UpscaleSpec, ctx: &mut JobCtx<'_>) -> Result<JobMeasure> {
    let span = spec.cut.requested;
    let total = span.end.saturating_sub(span.start);
    let mut spec = spec.clone();
    let analyse = spec.up.analysis == AnalysisState::Pending;
    let of = if analyse { 3 } else { 2 };
    if analyse {
        ctx.stage("analyze", 1, 3, 0.25);
        let (tools, cancel) = (ctx.tools, ctx.cancel);
        match reframe::analyze_cached(tools, &spec.reframe_input(), cancel, |f| ctx.stage_fraction(f, total))
            .await
        {
            Ok((a, cache)) => spec.up = spec.up.with_analysis(a, cache),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => {
                ctx.warn(format!("smart reframe analysis failed ({e}); using the blurred background"));
                spec.up = spec.up.with_blur();
            }
        }
    }
    ctx.stage("render", of - 1, of, if analyse { 0.7 } else { 0.9 });
    if let (Some(a), Some(dir), Some(_)) = (spec.up.analysis(), ctx.scratch_dir(), spec.up.graph.track) {
        let files = if spec.up.ai.is_some() {
            // The AI tier moves the crop in ffmpeg #1 and switches the fallback in ffmpeg #2.
            let (crop, fallback) = reframe::sendcmd::split_for_ai(a, spec.up.cmd_offset);
            vec![
                (crate::upscale::graph::AI_CROP_CMD, crop),
                (crate::upscale::graph::AI_FALLBACK_CMD, fallback),
            ]
        } else {
            vec![("reframe.cmd", reframe::sendcmd::to_sendcmd(a, spec.up.cmd_offset))]
        };
        for (name, text) in files {
            std::fs::write(dir.join(name), text)
                .map_err(|e| Error::OutputUnwritable { path: dir.join(name), message: e.to_string() })?;
        }
    }
    let mut tier_fallback = false;
    if let Err(e) = render(&spec, ctx, total).await {
        if !should_fall_back(&spec.up.tier, &e, ctx.cancel.is_cancelled()) {
            return Err(e);
        }
        // One automatic retry on the CPU tier (FR-008).
        let msg = match &e {
            Error::EngineFailed { message, .. } => message.clone(),
            other => other.to_string(),
        };
        ctx.warn(format!("GPU tier failed ({msg}); retried on CPU"));
        // ffmpeg never overwrites (`-n`): clear the failed attempt's output first.
        let _ = std::fs::remove_file(ctx.temp);
        spec.up = spec.up.with_tier_cpu("fallback: the GPU tier failed; the CPU tier");
        ctx.restart_stage("render", of - 1, of);
        render(&spec, ctx, total).await?;
        tier_fallback = true;
        // The CPU succeeded where the GPU failed, so the source is fine and the GPU is at fault:
        // remember it (SC-008). If the CPU had failed too, the source would be to blame.
        // ffmpeg's text can name the user's files, so the record keeps a fixed description.
        if let Some(dir) = ctx.state_dir().map(Path::to_path_buf) {
            let store = CapabilityStore::in_state_dir(&dir);
            let demoted = tokio::task::spawn_blocking(move || {
                demote(&store, Tier::Gpu, "a GPU render failed and the CPU tier succeeded")
            })
            .await;
            if !matches!(demoted, Ok(Ok(()))) {
                tracing::warn!("could not record the GPU failure in the capability record");
            }
        }
    }
    let spec = &spec;
    ctx.stage("verify", of, of, if analyse { 0.05 } else { 0.1 });
    let info = probe::probe(ctx.tools, ctx.temp, ctx.cancel)
        .await
        .map_err(|e| Error::VerifyFailed(e.to_string()))?;
    verify(spec, &info).map_err(Error::VerifyFailed)?;
    Ok(JobMeasure::Upscale {
        span,
        tier_used: spec.up.tier.tier,
        tier_fallback,
        fallback_ranges: spec.up.analysis().map_or_else(Vec::new, |a| a.fallback_ranges.clone()),
        no_upscale_needed: spec.up.geometry.no_upscale_needed,
    })
}

/// One render attempt in the scratch directory (so the graph can name its files relatively).
async fn render(spec: &UpscaleSpec, ctx: &mut JobCtx<'_>, total: Ts) -> Result<()> {
    let cwd = ctx.scratch_dir();
    if let Some(engine) = &spec.up.ai {
        let args = crate::plan::ffargs::ai_args(spec, ctx.temp);
        return super::pipeline::run(ctx, &engine.worker, &args, cwd, total).await;
    }
    let args = upscale_args(spec, ctx.temp);
    let out = ctx.run_ffmpeg_in(cwd, &args, total).await?;
    if out.status.success() { Ok(()) } else { Err(classify_failure(&out.stderr_tail, ctx.output)) }
}

/// An automatic GPU job retries on the CPU only when ffmpeg itself failed: never after a cancel,
/// a full disk, an unwritable output or a failed verification, and never when the GPU tier was
/// forced (research R9).
pub(crate) fn should_fall_back(tier: &TierDecision, e: &Error, cancelled: bool) -> bool {
    tier.tier == Tier::Gpu && !tier.forced && !cancelled && matches!(e, Error::EngineFailed { .. })
}

/// Checks the encoded file against the plan before it is committed: size, codec and tag,
/// stream count, colour tags and duration (data-model.md › Executor stages).
pub(crate) fn verify(spec: &UpscaleSpec, info: &MediaInfo) -> std::result::Result<(), String> {
    let up = &spec.up;
    let v = info.first_video().ok_or("the output has no video stream")?;
    let size = (v.width.unwrap_or(0), v.height.unwrap_or(0));
    if size != up.target {
        return Err(format!("output is {}x{}, expected {}x{}", size.0, size.1, up.target.0, up.target.1));
    }
    let pix = if up.graph.keep_hdr { "yuv420p10le" } else { "yuv420p" };
    if v.pix_fmt.as_deref() != Some(pix) {
        return Err(format!("output pixel format is {:?}, expected {pix}", v.pix_fmt));
    }
    let codec = match up.preset.video.codec {
        VideoCodec::Hevc => "hevc",
        VideoCodec::H264 => "h264",
    };
    if v.codec != codec {
        return Err(format!("output codec is {}, expected {codec}", v.codec));
    }
    if let Some(tag) = &up.preset.video.tag
        && v.codec_tag.as_deref() != Some(tag.as_str())
    {
        return Err(format!("output is tagged {:?}, expected {tag}", v.codec_tag));
    }
    // MP4 chapter lists are written as data tracks; only media streams are compared.
    let media = info
        .streams
        .iter()
        .filter(|s| matches!(s.kind, StreamKind::Video | StreamKind::Audio | StreamKind::Subtitle))
        .count();
    if media != spec.cut.streams.maps.len() {
        return Err(format!("expected {} streams, found {media}", spec.cut.streams.maps.len()));
    }
    let want = |field: &str, got: &Option<String>, expected: &Option<String>| match (got, expected) {
        (Some(g), Some(e)) if g == e => Ok(()),
        (_, None) => Ok(()),
        _ => Err(format!("output {field} is {got:?}, expected {expected:?}")),
    };
    let expected = if up.graph.keep_hdr { up.colour_in.clone() } else { ColourTags::bt709() };
    want("colour matrix", &v.color_space, &expected.space)?;
    want("colour primaries", &v.color_primaries, &expected.primaries)?;
    want("transfer", &v.color_transfer, &expected.transfer)?;
    want("colour range", &v.color_range, &Some("tv".to_owned()))?;
    if let (Some(expected), Some(got)) = (up.expected_video, v.duration.or(info.duration)) {
        let frame = Ts::from_secs_f64(1.0 / up.fps_out.max(1.0));
        let tolerance = frame + Ts::from_micros(50_000);
        let diff = if got > expected { got - expected } else { expected - got };
        if diff > tolerance {
            return Err(format!(
                "the output's video lasts {:.3} s but {:.3} s was expected",
                got.as_secs_f64(),
                expected.as_secs_f64()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn only_engine_failures_of_automatic_gpu_jobs_fall_back() {
        let auto_gpu = TierDecision {
            tier: Tier::Gpu,
            reason: String::new(),
            suggest: None,
            forced: false,
            device: Some(0),
        };
        let forced = TierDecision { forced: true, ..auto_gpu.clone() };
        let engine = Error::engine_failed("vk", String::new());
        assert!(should_fall_back(&auto_gpu, &engine, false));
        assert!(!should_fall_back(&auto_gpu, &engine, true), "never after a cancel");
        assert!(!should_fall_back(&forced, &engine, false), "forced tiers never fall back");
        assert!(!should_fall_back(&TierDecision::cpu("x"), &engine, false));
        for e in [
            Error::DiskFull(PathBuf::from("/o")),
            Error::OutputUnwritable { path: PathBuf::from("/o"), message: String::new() },
            Error::Cancelled,
            Error::VerifyFailed(String::new()),
        ] {
            assert!(!should_fall_back(&auto_gpu, &e, false), "{e:?}");
        }
    }
}
