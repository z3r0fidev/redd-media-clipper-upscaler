//! Fit jobs (feature 005, research R1, R2, R9): two passes or one quality pass, the size measured
//! before commit, and bounded retries. Nothing over the cap is ever committed: the `.part` file
//! is measured here and re-encoded lower, or the job fails `FIT_FAILED`.

use super::JobCtx;
use crate::error::{Error, Result};
use crate::fit::FitAttempt;
use crate::fit::plan::{FitPlan, MARGIN, Mode};
use crate::plan::ffargs::{ClipSpec, FitPass, ai_pass_args, fit_args, upscale_pass_args};
use crate::time::Ts;
use crate::upscale::UpscaleSpec;

/// At most this many re-encodes after the first (FR-010).
pub(crate) const MAX_RETRIES: usize = 3;

/// What a fit encodes: a clip, or an upscale on any tier (research R7).
#[derive(Clone, Copy)]
pub(super) enum FitJob<'a> {
    Clip(&'a ClipSpec),
    Upscale(&'a UpscaleSpec),
}

impl<'a> FitJob<'a> {
    fn cut(self) -> &'a ClipSpec {
        match self {
            FitJob::Clip(c) => c,
            FitJob::Upscale(u) => &u.cut,
        }
    }

    fn fit(self) -> &'a FitPlan {
        self.cut().fit.as_ref().expect("a fit job")
    }
}

/// The encode stages of a fit (contracts/json-output.md › Progress): `pass1`, `pass2` in budget
/// mode, `encode` in quality mode. The caller's `verify` stage comes after them.
pub(super) fn stages(fit: &FitPlan) -> u32 {
    match fit.mode {
        Mode::Budget => 2,
        Mode::Quality => 1,
    }
}

/// Encodes `job` (which has a fit) into the job's temp file, measuring and retrying until it is
/// at most the cap. Returns every attempt; the last one is what the temp file holds. Its stages
/// are numbered after `before` earlier ones, out of `of`, and take `share` of the progress; the
/// caller enters the `verify` stage and verifies.
pub(super) async fn encode(
    job: FitJob<'_>,
    ctx: &mut JobCtx<'_>,
    before: u32,
    of: u32,
    share: f64,
) -> Result<Vec<FitAttempt>> {
    let fit = job.fit();
    let scratch = ctx.scratch_dir().expect("fit jobs have a scratch folder").to_path_buf();
    let total = Ts::from_secs_f64(fit.len);
    let temp = ctx.temp.to_path_buf();
    let mut attempts = Vec::new();
    let mut pass = match fit.mode {
        Mode::Budget => {
            ctx.stage("pass1", before + 1, of, share / 2.0);
            run(ctx, job, FitPass::First, &scratch, total).await?;
            ctx.stage("pass2", before + 2, of, share / 2.0);
            FitPass::Second { video_kbps: fit.video_kbps }
        }
        Mode::Quality => {
            ctx.stage("encode", before + 1, of, share);
            FitPass::Quality
        }
    };
    loop {
        run(ctx, job, pass, &scratch, total).await?;
        let bytes = measured(&temp, fit.cap)?;
        let video_kbps = match pass {
            FitPass::Second { video_kbps } => video_kbps,
            _ => fit.video_kbps,
        };
        attempts.push(FitAttempt { video_kbps, bytes });
        if bytes <= fit.cap {
            return Ok(attempts);
        }
        let next = retry_kbps(fit, video_kbps, bytes);
        // Out of retries, or the next try would fall below the target's least acceptable
        // bitrate (x264's two-pass also refuses very low rates): fail, keeping nothing.
        if attempts.len() > MAX_RETRIES || f64::from(next) < min_kbps(fit) {
            remove(&temp).await;
            return Err(Error::FitFailed { cap: fit.cap, attempts });
        }
        // Over the cap: pass 2 again, lower (research R2). A quality pass that overshot falls
        // back to budget mode, which needs pass 1's stats first.
        if pass == FitPass::Quality {
            run(ctx, job, FitPass::First, &scratch, total).await?;
        }
        remove(&temp).await;
        pass = FitPass::Second { video_kbps: next };
        ctx.restart_stage("pass2", before + 2, of);
    }
}

/// The least video bitrate the target accepts at the planned size (research R4's `min_bpp`).
fn min_kbps(fit: &FitPlan) -> f64 {
    let fps = f64::from(fit.fps.0) / f64::from(fit.fps.1);
    fit.target.quality.min_bpp * f64::from(fit.size.0) * f64::from(fit.size.1) * fps / 1000.0
}

/// The lower bitrate for the next attempt (research R2): the video's share scaled by how far the
/// last attempt overshot, and always at least 1 kb/s lower.
pub(crate) fn retry_kbps(fit: &FitPlan, video_kbps: u32, bytes: u64) -> u32 {
    let usable = fit.cap as f64 * (1.0 - MARGIN);
    let audio = f64::from(fit.audio_kbps.unwrap_or(0)) * 1000.0 / 8.0 * fit.len;
    let ratio = ((usable - audio) / (bytes as f64 - audio).max(1.0)).clamp(0.05, 1.0);
    ((f64::from(video_kbps) * ratio).floor() as u32).min(video_kbps.saturating_sub(1)).max(1)
}

async fn run(
    ctx: &mut JobCtx<'_>,
    job: FitJob<'_>,
    pass: FitPass,
    scratch: &std::path::Path,
    total: Ts,
) -> Result<()> {
    let args = match job {
        FitJob::Clip(spec) => fit_args(spec, pass, ctx.temp),
        FitJob::Upscale(spec) if spec.up.ai.is_some() => {
            // The AI tier runs its whole pipeline in each pass (R7).
            let engine = spec.up.ai.as_ref().expect("checked");
            let args = ai_pass_args(spec, Some(pass), ctx.temp);
            return super::pipeline::run(ctx, &engine.worker, &args, Some(scratch), total).await;
        }
        FitJob::Upscale(spec) => upscale_pass_args(spec, Some(pass), ctx.temp),
    };
    let out = ctx.run_ffmpeg_in(Some(scratch), &args, total).await?;
    if out.status.success() {
        Ok(())
    } else {
        Err(super::clip::classify_failure(&out.stderr_tail, ctx.output))
    }
}

/// The job outcome's fit block from the plan and the attempts.
pub(super) fn outcome(fit: &FitPlan, attempts: Vec<FitAttempt>) -> crate::fit::FitOutcome {
    let last = *attempts.last().expect("at least one encode");
    crate::fit::FitOutcome {
        bytes: last.bytes,
        cap: fit.cap,
        mode: fit.mode,
        video_kbps: last.video_kbps,
        audio_kbps: fit.audio_kbps,
        attempts,
        trimmed: fit.trimmed_from.is_some(),
        size: fit.size,
        fps: fit.fps_f64(),
    }
}

/// The temp file's size (with the test hook's forced overshoot).
fn measured(temp: &std::path::Path, cap: u64) -> Result<u64> {
    let bytes = std::fs::metadata(temp).map_err(|e| Error::VerifyFailed(format!("no output: {e}")))?.len();
    Ok(forced(bytes, cap))
}

/// `test-support`: a forced overshoot reports 10% over the cap.
#[cfg(any(test, feature = "test-support"))]
fn forced(bytes: u64, cap: u64) -> u64 {
    if crate::fit::test_hooks::take() { bytes.max(cap) + cap / 10 } else { bytes }
}

#[cfg(not(any(test, feature = "test-support")))]
fn forced(bytes: u64, _cap: u64) -> u64 {
    bytes
}

async fn remove(temp: &std::path::Path) {
    let temp = temp.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || super::commit::remove_with_retry(&temp)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fit::FitOptions;
    use crate::fit::plan::{Source, plan_fit};
    use crate::fit::target::VideoTargetRegistry;

    fn plan() -> FitPlan {
        let t = VideoTargetRegistry::builtin().get("reddit-video").unwrap().clone();
        let o = FitOptions { max_size: Some(20_000_000), ..FitOptions::default() };
        plan_fit(&t, &o, Source { len: 60.0, size: (1920, 1080), fps: (30, 1), has_audio: true }).unwrap()
    }

    /// Research R2's retry formula: scaled by the overshoot, never the same or higher.
    #[test]
    fn a_retry_lowers_the_bitrate_by_the_overshoot() {
        let p = plan();
        let k = p.video_kbps;
        // 10% over the usable share (audio excluded): about 10% lower.
        let audio = 128.0 * 125.0 * 60.0;
        let over = ((20_000_000.0 * 0.98 - audio) * 1.1 + audio) as u64;
        let r = retry_kbps(&p, k, over);
        assert!(
            r < k && f64::from(r) > f64::from(k) * 0.88 && f64::from(r) < f64::from(k) * 0.92,
            "{k} → {r}"
        );
        // Just over the cap: at least down to the usable share (about 2% lower).
        let r = retry_kbps(&p, k, 20_000_001);
        assert!(r < k && f64::from(r) < f64::from(k) * 0.985, "{k} → {r}");
        // Never the same bitrate twice, even when the overshoot is negligible.
        assert_eq!(retry_kbps(&p, k, (20_000_000.0 * 0.98) as u64), k - 1);
        assert!(retry_kbps(&p, 1, u64::MAX / 2) >= 1);
    }
}
