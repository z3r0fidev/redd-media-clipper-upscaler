//! The AI tier's check (feature 002 US6): the worker's devices, its timed self-test and the speed gate.

use super::*;

/// SC-009's reference job: 60 s of 480p (854x480) at 30 fps.
pub(super) const AI_REF_FRAMES: f64 = 1800.0;

pub(super) const AI_REF_MPX: f64 = 854.0 * 480.0 / 1e6;

/// AI is offered only when the reference job is predicted to take at most this long.
pub const AI_LIMIT_SECS: f64 = 600.0;

/// Self-test frames (480x270, after an untimed warm-up frame).
pub(super) const AI_TEST_SIZE: (u32, u32) = (480, 270);

pub(super) const AI_TEST_FRAMES: u32 = 4;

/// A device as the worker lists it (`rmedia-sr --list-gpus`; its indices are ncnn's, which can
/// differ from ffmpeg's listing).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SrDevice {
    pub index: u32,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub software: bool,
}

/// Parses `--list-gpus` output: `{"devices":[…]}`.
pub fn parse_sr_devices(stdout: &str) -> Vec<SrDevice> {
    #[derive(Deserialize)]
    struct Listing {
        devices: Vec<SrDevice>,
    }
    serde_json::from_str::<Listing>(stdout.trim()).map(|l| l.devices).unwrap_or_default()
}

/// The AI tier's device: discrete first, then integrated, then virtual; never software.
pub fn pick_sr_device(devices: &[SrDevice]) -> Option<&SrDevice> {
    ["discrete", "integrated", "virtual"]
        .iter()
        .find_map(|k| devices.iter().find(|d| !d.software && d.kind == *k))
}

/// Seconds per input megapixel from the worker's `selftest` line (stderr JSON lines).
pub fn parse_selftest(stderr: &str) -> Option<f64> {
    stderr.lines().find_map(|l| {
        let v: serde_json::Value = serde_json::from_str(l).ok()?;
        (v["type"] == "selftest")
            .then(|| v["sec_per_mpx"].as_f64())
            .flatten()
            .filter(|x| x.is_finite() && *x > 0.0)
    })
}

/// The worker's error message from its stderr JSON lines.
pub(super) fn sr_error(stderr: &str) -> Option<String> {
    stderr.lines().find_map(|l| {
        let v: serde_json::Value = serde_json::from_str(l).ok()?;
        (v["type"] == "error").then(|| v["message"].as_str().map(|m| clean(m, 200))).flatten()
    })
}

/// The predicted processing time (s) of `frames` worker frames of `mpx` megapixels.
pub fn ai_secs(sec_per_mpx: f64, frames: f64, mpx: f64) -> f64 {
    sec_per_mpx * frames * mpx
}

/// The AI tier's status from a self-test on `device` (SC-009): available when 60 s of 480p is
/// predicted to take at most 10 minutes; otherwise the measured estimate is the reason.
pub fn ai_gate(device: &SrDevice, sec_per_mpx: f64) -> TierInfo {
    let reference = ai_secs(sec_per_mpx, AI_REF_FRAMES, AI_REF_MPX);
    let fps = (1.0 / (sec_per_mpx * AI_REF_MPX)) as f32;
    let base = TierInfo {
        device: Some(device.index),
        fps: Some(fps),
        sec_per_mpx: Some(sec_per_mpx as f32),
        ..TierInfo::default()
    };
    if reference <= AI_LIMIT_SECS {
        return TierInfo { available: true, ..base };
    }
    TierInfo {
        reason: Some(format!(
            "too slow on {} ({}): about {} min for 60 s of 480p (limit {} min)",
            clean(&device.name, 120),
            device.kind,
            (reference / 60.0).ceil(),
            (AI_LIMIT_SECS / 60.0).round()
        )),
        ..base
    }
}

/// Identity of the AI tier's files for the fingerprint: the worker (path, size, mtime) and the
/// SHA-256 of every model file.
pub(super) fn ai_identity(engine: Option<&crate::upscale::ai::AiEngine>) -> Vec<String> {
    let Some(e) = engine else { return vec!["none".into()] };
    let meta = |p: &Path| {
        std::fs::metadata(p).map_or_else(
            |_| format!("{}: missing", p.display()),
            |m| format!("{}: {} {:?}", p.display(), m.len(), m.modified().ok()),
        )
    };
    let mut out = vec![meta(&e.worker)];
    #[allow(irrefutable_let_patterns)] // the other variant exists in test builds only
    if let crate::upscale::ai::AiModel::Files(models) = &e.model {
        for base in models.values() {
            for ext in ["param", "bin"] {
                let f = base.with_extension(ext);
                let sha = std::fs::read(&f)
                    .map_or_else(|_| "missing".to_owned(), |b| crate::hex(&Sha256::digest(b)));
                out.push(format!("{}: {sha}", f.display()));
            }
        }
    }
    out
}

/// Runs the worker within `timeout` and before `deadline`. `Err(true)` on a timeout,
/// `Err(false)` when cancelled or out of budget.
pub(super) async fn run_worker(
    worker: &Path,
    args: &[String],
    timeout: Duration,
    deadline: Instant,
    cancel: &CancellationToken,
) -> Result<process::ProcessOutput, bool> {
    let left = deadline.saturating_duration_since(Instant::now()).min(timeout);
    if left.is_zero() || cancel.is_cancelled() {
        return Err(false);
    }
    let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
    match tokio::time::timeout(left, process::run_capture(worker, &argv, cancel)).await {
        Err(_) => Err(left == timeout),
        Ok(Err(crate::Error::Cancelled)) => Err(false),
        Ok(Err(e)) => Ok(process::ProcessOutput {
            status: failed_status(),
            stdout: Vec::new(),
            stderr_tail: format!(
                "{{\"type\":\"error\",\"message\":{}}}",
                serde_json::Value::from(e.to_string())
            ),
        }),
        Ok(Ok(out)) => Ok(out),
    }
}

#[cfg(unix)]
pub(super) fn failed_status() -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(1 << 8)
}

#[cfg(windows)]
pub(super) fn failed_status() -> std::process::ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(1)
}

/// The AI tier's check: the worker's devices, then a self-test on the preferred one. `None`
/// when the check was cut short (cancelled or out of budget): nothing learnt.
pub(super) async fn ai_check(
    env: &ProbeEnv,
    deadline: Instant,
    cancel: &CancellationToken,
) -> Option<TierInfo> {
    let unavailable = |r: String| Some(TierInfo { reason: Some(r), ..TierInfo::default() });
    let Some(engine) = &env.ai else {
        return unavailable("the AI worker (rmedia-sr) or its models are not installed".into());
    };
    let listing =
        match run_worker(&engine.worker, &["--list-gpus".into()], env.timeout, deadline, cancel).await {
            Ok(out) => out,
            Err(true) => return unavailable("the AI worker did not answer".into()),
            Err(false) => return None,
        };
    if !listing.status.success() {
        let why = sr_error(&listing.stderr_tail)
            .or_else(|| listing.stderr_tail.lines().last().map(|l| clean(l, 200)))
            .unwrap_or_else(|| "no output".into());
        return unavailable(format!("the AI worker could not run: {why}"));
    }
    let devices = parse_sr_devices(&String::from_utf8_lossy(&listing.stdout));
    let Some(device) = pick_sr_device(&devices) else {
        return unavailable(if devices.is_empty() {
            "no Vulkan device for the AI tier".into()
        } else {
            "no hardware GPU for the AI tier (only software renderers)".into()
        });
    };
    let scale = 2;
    let model = engine.model_arg(scale).unwrap_or_default().to_string_lossy().into_owned();
    let args: Vec<String> = [
        "--selftest",
        "--model",
        &model,
        "--scale",
        &scale.to_string(),
        "--gpu",
        &device.index.to_string(),
        "--width",
        &AI_TEST_SIZE.0.to_string(),
        "--height",
        &AI_TEST_SIZE.1.to_string(),
        "--frames",
        &AI_TEST_FRAMES.to_string(),
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    let base = TierInfo { device: Some(device.index), ..TierInfo::default() };
    match run_worker(&engine.worker, &args, env.timeout, deadline, cancel).await {
        Ok(out) => match parse_selftest(&out.stderr_tail) {
            Some(spm) if out.status.success() => Some(ai_gate(device, spm)),
            _ => Some(TierInfo {
                reason: Some(format!(
                    "AI self-test failed on {}: {}",
                    clean(&device.name, 120),
                    sr_error(&out.stderr_tail).unwrap_or_else(|| "no result".into())
                )),
                ..base
            }),
        },
        // Not even five small frames in the time allowed: far beyond the limit.
        Err(true) => Some(TierInfo {
            reason: Some(format!(
                "too slow on {} ({}): the self-test did not finish in {} s",
                clean(&device.name, 120),
                device.kind,
                env.timeout.as_secs()
            )),
            ..base
        }),
        Err(false) => None,
    }
}
