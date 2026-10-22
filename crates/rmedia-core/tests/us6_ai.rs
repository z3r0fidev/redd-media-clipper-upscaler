//! T098 (002 US6): the AI tier with the real Real-ESRGAN model on a real GPU. `#[ignore]`d: they
//! need a hardware Vulkan GPU, `cargo xtask fetch-models` and minutes of GPU time. The owner runs
//! them on a mid-range discrete GPU (checklists/validation.md › US6):
//!
//! ```text
//! cargo build --release -p rmedia-sr
//! RMEDIA_FFMPEG=$PWD/binaries/rmedia-ffmpeg-x86_64-unknown-linux-gnu \
//! RMEDIA_FFPROBE=$PWD/binaries/rmedia-ffprobe-x86_64-unknown-linux-gnu \
//!   cargo test --release -p rmedia-core --test us6_ai -- --ignored --nocapture --test-threads 1
//! ```
//!
//! Running them is a request for the numbers, so a missing FFmpeg feature fails instead of
//! skipping (the system FFmpeg usually has no libvmaf; the bundled one has).
//!
//! They bypass the availability gate (so they also run, slowly, on an integrated GPU for
//! development numbers); SC-009 then fails there, as it should.

mod common;

use std::path::{Path, PathBuf};
use std::time::Instant;

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::JobOutcome;
use rmedia_core::plan::Caps;
use rmedia_core::request::Options;
use rmedia_core::upscale::ai::{AiEngine, AiModel, locate_models, locate_worker};
use rmedia_core::upscale::capability::{TierInfo, parse_selftest, parse_sr_devices, pick_sr_device};
use rmedia_core::upscale::{Reframe, TierChoice, UpscaleOptions};
use rmedia_testkit::{Fixture, fixture, vmaf};

/// SC-002 for the AI tier: VMAF points over the GPU tier on the degraded 360p fixture.
const AI_MARGIN: f64 = 3.0;

/// Fails (never skips) when the engine lacks a feature these measurements need.
fn need(features: &[&str]) {
    assert!(
        rmedia_testkit::require_engine(features),
        "the FFmpeg in use lacks one of {features:?}: point RMEDIA_FFMPEG and RMEDIA_FFPROBE at the bundled \
         binaries/rmedia-ffmpeg-* and rmedia-ffprobe-* (see the top of this file)"
    );
}

/// The worker built in this profile: test binaries run from `target/<profile>/deps/`, and the
/// shipped lookup only looks there in debug builds.
fn built_worker() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.parent()?.join(format!("rmedia-sr{}", std::env::consts::EXE_SUFFIX));
    p.is_file().then_some(p)
}

/// The worker, its models from the workspace `models/`, and the GPU it would use.
fn real_engine() -> (AiEngine, u32, String) {
    let profile = if cfg!(debug_assertions) { "" } else { " --release" };
    let worker = locate_worker()
        .or_else(built_worker)
        .unwrap_or_else(|| panic!("rmedia-sr is not built: run `cargo build{profile} -p rmedia-sr`"));
    eprintln!("AI worker: {}", worker.display());
    let models = locate_models(&[Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models")])
        .expect("models fetched (cargo xtask fetch-models)");
    let out = std::process::Command::new(&worker).arg("--list-gpus").output().expect("run rmedia-sr");
    let devices = parse_sr_devices(&String::from_utf8_lossy(&out.stdout));
    let d = pick_sr_device(&devices).expect("a hardware Vulkan GPU").clone();
    eprintln!("AI tier on device {} ({}, {})", d.index, d.name, d.kind);
    (AiEngine { worker, model: AiModel::Files(models) }, d.index, d.name)
}

/// The worker's self-test on `device`: seconds per input megapixel.
fn self_test(engine: &AiEngine, device: u32) -> f64 {
    let model = engine.model_arg(2).unwrap();
    let out = std::process::Command::new(&engine.worker)
        .args(["--selftest", "--scale", "2", "--width", "480", "--height", "270", "--frames", "8", "--model"])
        .arg(model)
        .args(["--gpu", &device.to_string()])
        .output()
        .unwrap();
    parse_selftest(&String::from_utf8_lossy(&out.stderr)).expect("a self-test result")
}

/// Caps with the AI tier forced available on `device` (and the self-test speed, if known).
fn ai_caps(ai: &AiEngine, device: u32, sec_per_mpx: Option<f64>) -> Caps {
    let (_, mut caps) = engine();
    let mut r = rmedia_core::upscale::capability::CapabilityRecord::default();
    r.tiers.ai = TierInfo {
        available: true,
        device: Some(device),
        sec_per_mpx: sec_per_mpx.map(|s| s as f32),
        ..TierInfo::default()
    };
    // Keep whatever the machine's GPU tier is (for the comparisons).
    let state = rmedia_core::settings::state_dir();
    if let Some(stored) =
        state.and_then(|s| rmedia_core::upscale::capability::CapabilityStore::in_state_dir(&s).load())
    {
        r.tiers.gpu = stored.tiers.gpu;
        r.tiers.cpu = stored.tiers.cpu;
        r.devices = stored.devices;
    }
    caps.capability = Some(r);
    caps.ai = Some(ai.clone());
    caps
}

/// Copies renders to `RMEDIA_US6_KEEP`, when set, for measuring anti-flicker candidates offline
/// (T104, research R13).
fn keep(files: &[(&PathBuf, &str)]) {
    if let Some(dir) = std::env::var_os("RMEDIA_US6_KEEP") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (f, name) in files {
            std::fs::copy(f, dir.join(name)).unwrap();
        }
    }
}

async fn render(
    src: &Path,
    out: &Path,
    caps: &Caps,
    tier: TierChoice,
    preset: &str,
    end: EndSpec,
) -> PathBuf {
    let u =
        UpscaleOptions { preset: preset.into(), reframe: Reframe::Center, tier, ..UpscaleOptions::default() };
    let opts = Options { upscale: Some(u), output_dir: Some(out.to_path_buf()), ..Options::default() };
    let (tools, _) = engine();
    let p = make_plan(&request(src, vec![clip(0.0, end)], opts), &tools, caps).await;
    assert!(p.issues.is_empty(), "{:?}", p.issues);
    let (summary, _) = run_plan(p, tools, &out.join("state")).await;
    match &summary.jobs[0].outcome {
        JobOutcome::Succeeded { output, .. } => output.clone(),
        other => panic!("{tier:?} upscale failed: {other:?}"),
    }
}

/// The classical tier to compare with: the GPU tier when this machine has one, else the CPU.
fn classical(caps: &Caps) -> TierChoice {
    match caps.capability.as_ref() {
        Some(r) if r.tiers.gpu.available => TierChoice::Gpu,
        _ => {
            eprintln!("no GPU tier here: comparing with the CPU tier");
            TierChoice::Cpu
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a Vulkan GPU, the AI models and GPU minutes"]
async fn ai_beats_the_gpu_tier_on_degraded_360p() {
    need(&["libx264", "zscale", "libvmaf"]);
    let (engine, device, _) = real_engine();
    let caps = ai_caps(&engine, device, None);
    let d = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::DegradedPortrait360);
    let clean = fixture(Fixture::CleanPortrait);
    let other = classical(&caps);
    let t = Instant::now();
    let ai =
        render(&src, &d.path().join("ai"), &caps, TierChoice::Ai, "reddit-916", EndSpec::EndOfMedia).await;
    eprintln!("AI render: {:.0} s", t.elapsed().as_secs_f64());
    let base = render(&src, &d.path().join("c"), &caps, other, "reddit-916", EndSpec::EndOfMedia).await;
    let (a, b) = (vmaf(&ai, &clean), vmaf(&base, &clean));
    eprintln!("SC-002: AI VMAF {a:.2}, {other:?} tier {b:.2}, margin {:.2}", a - b);
    keep(&[(&ai, "quality-ai.mp4"), (&base, "quality-classical.mp4")]);
    assert!(a >= b + AI_MARGIN, "SC-002: AI {a:.2} vs {other:?} {b:.2} (need +{AI_MARGIN})");
}

/// A still picture with fresh noise every frame, compressed hard with a keyframe every 10
/// frames (3 s, 360x640; its own frame difference is about 2.5): anything that changes between
/// frames of the output is flicker. (Without the short GOP, x264 skips the static blocks and
/// the source has no flicker to measure.)
fn static_noisy_source(dir: &Path) -> PathBuf {
    let (tools, _) = engine();
    let out = dir.join("static.mp4");
    let ok = std::process::Command::new(&tools.ffmpeg.path)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-i"])
        .arg(fixture(Fixture::CleanPortrait))
        .args([
            "-vf",
            "trim=start_frame=30:end_frame=31,loop=loop=89:size=1:start=0,setpts=N/30/TB,scale=360:640,noise=alls=20:allf=t",
            "-c:v",
            "libx264",
            "-crf",
            "30",
            "-g",
            "10",
            "-an",
        ])
        .arg(&out)
        .status()
        .unwrap();
    assert!(ok.success());
    out
}

/// Mean luma difference between consecutive frames (0–255), over the whole frame.
pub fn flicker(path: &Path) -> f64 {
    let (tools, _) = engine();
    let out = std::process::Command::new(&tools.ffmpeg.path)
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(path)
        .args([
            "-vf",
            "format=gray,tblend=all_mode=difference,signalstats,metadata=print:key=lavfi.signalstats.YAVG",
            "-f",
            "null",
            "-",
        ])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    let v: Vec<f64> =
        text.lines().filter_map(|l| l.split("lavfi.signalstats.YAVG=").nth(1)?.trim().parse().ok()).collect();
    assert!(!v.is_empty(), "no frame differences measured: {text}");
    v.iter().sum::<f64>() / v.len() as f64
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a Vulkan GPU, the AI models and GPU minutes"]
async fn ai_does_not_flicker_more_than_the_gpu_tier() {
    need(&["libx264", "zscale"]);
    let (engine, device, _) = real_engine();
    let caps = ai_caps(&engine, device, None);
    let d = tempfile::tempdir().unwrap();
    let src = static_noisy_source(d.path());
    let other = classical(&caps);
    let ai =
        render(&src, &d.path().join("ai"), &caps, TierChoice::Ai, "reddit-916", EndSpec::EndOfMedia).await;
    let base = render(&src, &d.path().join("c"), &caps, other, "reddit-916", EndSpec::EndOfMedia).await;
    let (fa, fb, fs) = (flicker(&ai), flicker(&base), flicker(&src));
    eprintln!("flicker (mean frame difference): AI {fa:.3}, {other:?} tier {fb:.3}, source {fs:.3}");
    keep(&[(&ai, "ai.mp4"), (&base, "classical.mp4"), (&src, "source.mp4")]);
    assert!(fa <= fb, "US6 scenario 4: AI flickers more ({fa:.3}) than the {other:?} tier ({fb:.3})");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a mid-range discrete GPU, the AI models and up to 10 GPU minutes"]
async fn sixty_seconds_of_480p_in_ten_minutes_as_predicted() {
    need(&["libx265", "zscale"]);
    let (ai, device, name) = real_engine();
    let spm = self_test(&ai, device);
    let caps = ai_caps(&ai, device, Some(spm));
    let d = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::Portrait480Long);
    let (tools, _) = engine();
    let u = UpscaleOptions { reframe: Reframe::Center, tier: TierChoice::Ai, ..UpscaleOptions::default() };
    let opts = Options { upscale: Some(u), output_dir: Some(d.path().to_path_buf()), ..Options::default() };
    let p = make_plan(&request(&src, vec![clip(0.0, EndSpec::EndOfMedia)], opts), &tools, &caps).await;
    let rmedia_core::plan::JobSpec::Upscale(u) = &p.jobs[0].spec else { panic!() };
    let predicted = u.up.estimate.secs;
    let t = Instant::now();
    let (summary, _) = run_plan(p, tools, &d.path().join("state")).await;
    let took = t.elapsed().as_secs_f64();
    assert!(matches!(summary.jobs[0].outcome, JobOutcome::Succeeded { .. }), "{:?}", summary.jobs[0].outcome);
    eprintln!("SC-009 on {name}: {took:.0} s (predicted {predicted:.0} s, self-test {spm:.3} s/Mpx)");
    assert!(took <= 600.0, "SC-009: 60 s of 480p took {took:.0} s (limit 600 s)");
    let err = (predicted - took).abs() / took;
    assert!(err <= 0.30, "SC-009: predicted {predicted:.0} s, took {took:.0} s ({:.0}% off)", err * 100.0);
}
