//! Upscale job executor: render → verify (data-model.md › Executor stages). US2 adds an analyze
//! stage in front; US3 adds the one-time CPU retry after a GPU failure.

use super::JobCtx;
use super::clip::classify_failure;
use crate::error::{Error, Result};
use crate::event::JobMeasure;
use crate::plan::ffargs::upscale_args;
use crate::preset::VideoCodec;
use crate::probe::{self, MediaInfo, StreamKind};
use crate::time::Ts;
use crate::upscale::UpscaleSpec;
use crate::upscale::encode::ColourTags;

pub(super) async fn execute(spec: &UpscaleSpec, ctx: &mut JobCtx<'_>) -> Result<JobMeasure> {
    let span = spec.cut.requested;
    let total = span.end.saturating_sub(span.start);
    ctx.stage("render", 1, 2, 0.9);
    let args = upscale_args(spec, ctx.temp);
    // ffmpeg runs in the scratch directory so the graph can name its files relatively.
    let cwd = ctx.scratch_dir();
    let out = ctx.run_ffmpeg_in(cwd, &args, total).await?;
    if !out.status.success() {
        return Err(classify_failure(&out.stderr_tail, ctx.output));
    }
    ctx.stage("verify", 2, 2, 0.1);
    let info = probe::probe(ctx.tools, ctx.temp, ctx.cancel)
        .await
        .map_err(|e| Error::VerifyFailed(e.to_string()))?;
    verify(spec, &info).map_err(Error::VerifyFailed)?;
    Ok(JobMeasure::Upscale {
        span,
        tier_used: spec.up.tier.tier,
        tier_fallback: false,
        fallback_ranges: Vec::new(),
        no_upscale_needed: spec.up.geometry.no_upscale_needed,
    })
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
