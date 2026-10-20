//! T096 (002 US6): the AI tier's pipeline (ffmpeg #1 → rmedia-sr → ffmpeg #2) with the worker's
//! test-only `builtin:nearest --gpu none` model, so it runs on any machine: output size and
//! duration, audio sync, progress, cancel (all three processes, no leftovers) and a worker crash.
//! The worker binary comes from the same Cargo build (`cargo test --workspace`).

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::event::{Event, EventSink, JobOutcome};
use rmedia_core::exec::{self, RunConfig};
use rmedia_core::plan::{Caps, JobSpec};
use rmedia_core::request::Options;
use rmedia_core::upscale::ai::{AiEngine, AiModel};
use rmedia_core::upscale::capability::{CapabilityRecord, TierInfo};
use rmedia_core::upscale::tier::Tier;
use rmedia_core::upscale::{HdrMode, Reframe, TierChoice, UpscaleOptions};
use rmedia_testkit::{Fixture, fixture, probe_json, require_engine};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::sync::mpsc;

/// Process checks look at every child of this test binary: one test at a time.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The worker built alongside these tests. CI (`RMEDIA_REQUIRE_FFMPEG`) must have it.
fn worker() -> Option<PathBuf> {
    let w = rmedia_core::upscale::ai::locate_worker();
    if w.is_none() {
        assert!(
            std::env::var_os("RMEDIA_REQUIRE_FFMPEG").is_none(),
            "rmedia-sr not built: run `cargo test --workspace`"
        );
        eprintln!("skipped: rmedia-sr not built (run `cargo test --workspace`)");
    }
    w
}

/// Caps whose AI tier is available and runs `worker` with the builtin test model.
fn ai_caps(worker: &Path) -> Caps {
    let (_, mut caps) = engine();
    let mut r = CapabilityRecord { fingerprint: "cd".repeat(32), ..CapabilityRecord::default() };
    r.tiers.ai = TierInfo { available: true, ..TierInfo::default() };
    caps.capability = Some(r);
    caps.ai = Some(AiEngine { worker: worker.to_path_buf(), model: AiModel::BuiltinNearest });
    caps
}

fn opts(out: &Path, reframe: Reframe) -> Options {
    let u = UpscaleOptions { tier: TierChoice::Ai, reframe, ..UpscaleOptions::default() };
    Options { upscale: Some(u), output_dir: Some(out.to_path_buf()), ..Options::default() }
}

fn f64_of(v: &serde_json::Value) -> f64 {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_f64()).unwrap_or(f64::NAN)
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_output_has_the_target_size_duration_and_audio_in_sync() {
    let _serial = SERIAL.lock().await;
    require_engine!("libx265", "zscale", "gblur");
    let Some(w) = worker() else { return };
    let d = tempfile::tempdir().unwrap();
    let (tools, _) = engine();
    let caps = ai_caps(&w);
    // 320x240 with AAC audio, cut from 1 s (an input seek) to 3 s; centre crop (worker input
    // 110x240, ×4) and blurred background (whole frame 320x240, ×4).
    for (reframe, name) in [(Reframe::Center, "center"), (Reframe::Blur, "blur")] {
        let out = d.path().join(name);
        let p = make_plan(
            &request(
                &fixture(Fixture::Gop50Mp4),
                vec![clip(1.0, EndSpec::At(secs(3.0)))],
                opts(&out, reframe),
            ),
            &tools,
            &caps,
        )
        .await;
        assert!(p.issues.is_empty(), "{:?}", p.issues);
        let JobSpec::Upscale(u) = &p.jobs[0].spec else { panic!() };
        assert_eq!(u.up.tier.tier, Tier::Ai);
        let pipe = p.jobs[0].pipeline().expect("an AI job has a pipeline");
        let text = |a: &[std::ffi::OsString]| rmedia_core::plan::ffargs::display_args(a).join(" ");
        assert!(text(&pipe.decode).ends_with("-f rawvideo -pix_fmt rgb24 pipe:1"), "{}", text(&pipe.decode));
        assert!(
            text(&pipe.worker).contains("--model builtin:nearest --scale 4 --gpu none"),
            "{}",
            text(&pipe.worker)
        );
        assert!(
            text(&pipe.encode).contains("-f rawvideo -pix_fmt rgb24 -video_size"),
            "{}",
            text(&pipe.encode)
        );
        assert_eq!(p.jobs[0].args(), pipe.encode, "the job's argv is the encoder's");

        let (summary, events) = run_plan(p, tools.clone(), &d.path().join("state")).await;
        let JobOutcome::Succeeded { output, .. } = &summary.jobs[0].outcome else {
            panic!("{name}: {:?}", summary.jobs[0].outcome)
        };
        let info = probe_json(output);
        let streams = info["streams"].as_array().unwrap();
        let v = streams.iter().find(|s| s["codec_type"] == "video").unwrap();
        let a = streams.iter().find(|s| s["codec_type"] == "audio").expect("audio kept");
        assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(1080), Some(2340)), "{name}");
        assert_eq!(v["codec_tag_string"], "hvc1");
        let frame = 1.0 / 25.0;
        let vd = f64_of(&v["duration"]);
        assert!((vd - 2.0).abs() <= frame + 0.05, "{name}: video lasts {vd}");
        let (vs, as_) = (f64_of(&v["start_time"]), f64_of(&a["start_time"]));
        assert!((vs - as_).abs() <= frame, "{name}: video starts at {vs}, audio at {as_}");
        assert!((f64_of(&a["duration"]) - vd).abs() <= 2.0 * frame + 0.05, "{name}: audio/video lengths");
        let fractions: Vec<f64> = events
            .iter()
            .filter_map(|e| if let Event::JobProgress { fraction, .. } = e { Some(*fraction) } else { None })
            .collect();
        assert!(fractions.len() >= 2 && fractions.windows(2).all(|w| w[1] >= w[0]), "{fractions:?}");
        assert!(partials(&out).is_empty());
    }
}

struct Notify(mpsc::UnboundedSender<Event>);
impl EventSink for Notify {
    fn emit(&self, e: Event) {
        let _ = self.0.send(e);
    }
}

/// Engine and worker processes whose parent is this test process.
fn children() -> Vec<String> {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let me = Pid::from_u32(std::process::id());
    sys.processes()
        .values()
        .filter(|p| p.parent() == Some(me))
        .map(|p| p.name().to_string_lossy().into_owned())
        .filter(|n| n.contains("ffmpeg") || n.contains("rmedia-sr") || n.contains("fake-sr"))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_stops_all_three_processes_and_leaves_nothing() {
    let _serial = SERIAL.lock().await;
    require_engine!("libx265", "zscale");
    let Some(w) = worker() else { return };
    let d = tempfile::tempdir().unwrap();
    let (tools, _) = engine();
    let p = make_plan(
        &request(
            &fixture(Fixture::Hd60Mp4),
            vec![clip(0.0, EndSpec::EndOfMedia)],
            opts(d.path(), Reframe::Center),
        ),
        &tools,
        &ai_caps(&w),
    )
    .await;
    assert!(p.issues.is_empty(), "{:?}", p.issues);
    let scratch = p.jobs[0].scratch.clone().expect("upscale jobs have a scratch folder");
    let (tx, mut rx) = mpsc::unbounded_channel();
    let cfg = RunConfig { jobs: 1, state_dir: Some(d.path().join("state")) };
    let handle = exec::start(p, tools, cfg, Arc::new(Notify(tx)));
    let started = Instant::now();
    loop {
        match tokio::time::timeout(Duration::from_secs(60), rx.recv()).await {
            Ok(Some(Event::JobProgress { .. })) => break,
            Ok(Some(_)) => {}
            other => panic!("no progress: {other:?}"),
        }
    }
    tokio::time::sleep(Duration::from_secs(1).saturating_sub(started.elapsed())).await;
    assert!(children().iter().any(|n| n.contains("rmedia-sr")), "the worker is running: {:?}", children());
    let t = Instant::now();
    handle.cancel();
    let summary = handle.wait().await;
    assert!(t.elapsed() < Duration::from_secs(2), "cancel took {:?}", t.elapsed());
    assert!(matches!(summary.jobs[0].outcome, JobOutcome::Cancelled), "{:?}", summary.jobs[0].outcome);
    assert!(children().is_empty(), "left running: {:?}", children());
    assert!(partials(d.path()).is_empty(), "{:?}", partials(d.path()));
    assert!(!scratch.exists(), "scratch folder removed");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_worker_crash_is_an_engine_failure_with_its_stderr() {
    use std::os::unix::fs::PermissionsExt;
    let _serial = SERIAL.lock().await;
    require_engine!("libx265", "zscale");
    let d = tempfile::tempdir().unwrap();
    // Reads a little, reports a GPU error and exits 6 (contracts/rmedia-sr-protocol.md).
    let fake = d.path().join("fake-sr");
    std::fs::write(
        &fake,
        "#!/bin/sh\nhead -c 100000 >/dev/null\n\
         echo '{\"type\":\"error\",\"code\":\"GPU_RUNTIME\",\"message\":\"device lost\"}' >&2\nexit 6\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (tools, _) = engine();
    let p = make_plan(
        &request(
            &fixture(Fixture::Gop50Mp4),
            vec![clip(0.0, EndSpec::At(secs(4.0)))],
            opts(d.path(), Reframe::Center),
        ),
        &tools,
        &ai_caps(&fake),
    )
    .await;
    let (summary, _) = run_plan(p, tools, &d.path().join("state")).await;
    let JobOutcome::Failed { error } = &summary.jobs[0].outcome else {
        panic!("{:?}", summary.jobs[0].outcome)
    };
    assert_eq!(error.code, "ENGINE_FAILED");
    assert!(
        error.message.contains("AI worker") && error.message.contains("device lost"),
        "{}",
        error.message
    );
    assert!(children().is_empty(), "left running: {:?}", children());
    assert!(partials(d.path()).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_needs_its_worker_and_sdr_output() {
    require_engine!("libx265", "zscale");
    let d = tempfile::tempdir().unwrap();
    let (tools, _) = engine();
    let mut caps = ai_caps(Path::new("/unused"));
    let mut o = opts(d.path(), Reframe::Center);
    o.upscale.as_mut().unwrap().hdr = HdrMode::Keep;
    let hdr = make_plan(
        &request(&fixture(Fixture::HdrTagged), vec![clip(0.0, EndSpec::EndOfMedia)], o),
        &tools,
        &caps,
    )
    .await;
    assert!(
        hdr.issues
            .iter()
            .any(|i| i.message.contains("SDR") || i.code == rmedia_core::error::IssueCode::HdrUnsupported),
        "{:?}",
        hdr.issues
    );
    caps.ai = None;
    let none = make_plan(
        &request(
            &fixture(Fixture::TinySource),
            vec![clip(0.0, EndSpec::EndOfMedia)],
            opts(d.path(), Reframe::Center),
        ),
        &tools,
        &caps,
    )
    .await;
    let issue = none.issues.iter().find(|i| i.code == rmedia_core::error::IssueCode::TierUnavailable);
    assert!(issue.is_some_and(|i| i.message.contains("rmedia-sr")), "{:?}", none.issues);
}

/// PSNR (dB, average over Y/U/V and frames) of `a` against `b`.
fn psnr(a: &Path, b: &Path) -> f64 {
    let (tools, _) = engine();
    let out = std::process::Command::new(&tools.ffmpeg.path)
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(a)
        .arg("-i")
        .arg(b)
        .args(["-lavfi", "[0:v][1:v]psnr", "-f", "null", "-"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    let avg = text.rsplit("average:").next().and_then(|s| s.split_whitespace().next()).unwrap_or("nan");
    avg.parse().unwrap_or_else(|_| panic!("no PSNR in {text}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_framing_and_colours_match_the_cpu_tier() {
    let _serial = SERIAL.lock().await;
    require_engine!("libx265", "zscale", "gblur", "sendcmd");
    let Some(w) = worker() else { return };
    let d = tempfile::tempdir().unwrap();
    let (tools, _) = engine();
    let mut ai = ai_caps(&w);
    ai.face = Some(rmedia_core::upscale::reframe::detect::DetectorSpec::Marker);
    ai.reframe_cache = Some(d.path().join("cache"));
    let mut cpu = ai.clone();
    cpu.ai = None;
    // 2 s of the moving marker, then a cut to 2 s of two far-apart markers: smart reframe pans,
    // then falls back to the blurred background for the second shot only.
    let mixed = d.path().join("mixed.mp4");
    let made = std::process::Command::new(&tools.ffmpeg.path)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-i"])
        .arg(fixture(Fixture::MovingMarker))
        .arg("-i")
        .arg(fixture(Fixture::FarMarkers))
        .args([
            "-filter_complex",
            "[0:v]trim=0:2,setpts=PTS-STARTPTS[a];[1:v]trim=0:2,setpts=PTS-STARTPTS[b];[a][b]concat=n=2:v=1[v]",
            "-map",
            "[v]",
            "-c:v",
            "libx264",
            "-crf",
            "12",
        ])
        .arg(&mixed)
        .status()
        .unwrap();
    assert!(made.success());
    for (source, reframe, name) in [
        (fixture(Fixture::MovingMarker), Reframe::Smart, "smart"),
        (mixed, Reframe::Smart, "partial fallback"),
        (fixture(Fixture::MovingMarker), Reframe::Blur, "blur"),
    ] {
        let mut outs = Vec::new();
        for (caps, tier) in [(&ai, TierChoice::Ai), (&cpu, TierChoice::Cpu)] {
            let out = d.path().join(format!("{name}-{tier:?}"));
            let mut o = opts(&out, reframe);
            o.upscale.as_mut().unwrap().tier = tier;
            let p =
                make_plan(&request(&source, vec![clip(0.0, EndSpec::At(secs(4.0)))], o), &tools, caps).await;
            assert!(p.issues.is_empty(), "{name}: {:?}", p.issues);
            let (summary, _) = run_plan(p, tools.clone(), &d.path().join("state")).await;
            let JobOutcome::Succeeded { output, measure } = &summary.jobs[0].outcome else {
                panic!("{name} {tier:?}: {:?}", summary.jobs[0].outcome)
            };
            if name == "partial fallback" {
                let rmedia_core::event::JobMeasure::Upscale { fallback_ranges, .. } = measure else {
                    panic!()
                };
                assert_eq!(fallback_ranges.len(), 1, "{fallback_ranges:?}");
            }
            outs.push(output.clone());
        }
        // Nearest-neighbour ×s against Lanczos + sharpening: the same picture, not the same pixels.
        let db = psnr(&outs[0], &outs[1]);
        eprintln!("{name}: {db:.2} dB");
        assert!(db > 40.0, "{name}: AI-tier output differs from the CPU tier's ({db:.1} dB)");
    }
}
