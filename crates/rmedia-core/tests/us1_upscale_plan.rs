//! T026 (002 US1): planning upscale jobs — naming, output folder, validation codes, warnings.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::plan::{Caps, Disposition, Plan, Severity};
use rmedia_core::request::{OnExists, Options};
use rmedia_core::upscale::{HdrMode, Reframe, TierChoice, UpscaleOptions};
use rmedia_testkit::{Fixture, fixture, require_engine};

fn copy(kind: Fixture, dir: &Path, name: &str) -> PathBuf {
    let dst = dir.join(name);
    std::fs::copy(fixture(kind), &dst).unwrap();
    dst
}

fn up(edit: impl FnOnce(&mut UpscaleOptions)) -> Options {
    let mut u = UpscaleOptions::default();
    edit(&mut u);
    Options { upscale: Some(u), ..Options::default() }
}

async fn plan_whole(src: &Path, opts: Options, caps: Option<Caps>) -> Plan {
    let (tools, engine_caps) = engine();
    let req = request(src, vec![clip(0.0, EndSpec::EndOfMedia)], opts);
    make_plan(&req, &tools, &caps.unwrap_or(engine_caps)).await
}

fn codes(p: &Plan) -> Vec<&str> {
    p.issues.iter().map(|i| i.code.as_str()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn default_output_is_in_upscaled_with_the_preset_in_the_name() {
    require_engine!("libx265", "zscale", "cas");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::DegradedPortrait, d.path(), "low.mp4");
    let p = plan_whole(&src, up(|_| {}), None).await;
    assert!(p.issues.is_empty(), "{:?}", p.issues);
    let job = &p.jobs[0];
    assert_eq!(job.kind(), "upscale");
    assert_eq!(job.output, d.path().join("upscaled/low_phone-fill.mp4"));
    assert!(job.scratch.as_ref().is_some_and(|s| s.extension().is_some_and(|e| e == "d")));
    assert_eq!(job.mode(), None, "mode does not apply to upscaling");
    let argv = rmedia_core::plan::ffargs::display_args(&job.args()).join(" ");
    assert!(
        argv.contains("-filter_complex") && argv.contains("libx265") && argv.contains("-tag:v hvc1"),
        "{argv}"
    );
    assert!(argv.contains("-map_chapters 0"), "whole videos keep chapters: {argv}");
}

#[tokio::test(flavor = "multi_thread")]
async fn preset_variable_container_and_custom_template() {
    require_engine!("libx264", "zscale", "cas");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::DegradedPortrait, d.path(), "low.mp4");
    let mut opts = up(|u| u.preset = "reddit-916".into());
    opts.name_template = "{source}-{preset}.{ext}".into();
    opts.container = Some("mkv".into());
    opts.output_dir = Some(d.path().join("out"));
    let p = plan_whole(&src, opts, None).await;
    let job = &p.jobs[0];
    assert_eq!(job.output, d.path().join("out/low-reddit-916.mp4"), "always .mp4");
    assert!(job.warnings.iter().any(|w| w.contains("always .mp4")), "{:?}", job.warnings);
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_requests_get_specific_codes() {
    require_engine!("libx265", "zscale", "cas");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::DegradedPortrait, d.path(), "low.mp4");
    let unknown = plan_whole(&src, up(|u| u.preset = "nokia-3310".into()), None).await;
    assert_eq!(codes(&unknown), ["PRESET_UNKNOWN"]);
    assert!(unknown.issues[0].message.contains("phone-fill"), "lists the known presets");
    let hdr = plan_whole(
        &src,
        up(|u| {
            u.preset = "reddit-916".into();
            u.hdr = HdrMode::Keep;
        }),
        None,
    )
    .await;
    assert_eq!(codes(&hdr), ["HDR_UNSUPPORTED"]);
    for tier in [TierChoice::Gpu, TierChoice::Ai] {
        let p = plan_whole(&src, up(|u| u.tier = tier), None).await;
        assert_eq!(codes(&p), ["TIER_UNAVAILABLE"], "{tier:?}");
        assert!(p.jobs.is_empty() && p.has_errors());
    }
    let cpu = plan_whole(&src, up(|u| u.tier = TierChoice::Cpu), None).await;
    assert!(cpu.issues.is_empty(), "forcing the CPU tier always works: {:?}", cpu.issues);
}

#[tokio::test(flavor = "multi_thread")]
async fn engine_without_the_encoder_is_an_engine_error() {
    require_engine!("libx265", "zscale", "cas");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::DegradedPortrait, d.path(), "low.mp4");
    let (_, mut caps) = engine();
    caps.encoders.remove("libx265");
    let p = plan_whole(&src, up(|_| {}), Some(caps)).await;
    assert_eq!(codes(&p), ["ENGINE_MISSING"]);
    assert!(p.engine_errors_only(), "exit 4, not 3");
    let (_, mut caps) = engine();
    caps.filters.remove("scdet");
    let degraded = plan_whole(&src, up(|_| {}), Some(caps)).await;
    assert!(degraded.issues.is_empty(), "missing smart filters degrade, not fail: {:?}", degraded.issues);
    assert!(
        degraded.jobs[0].warnings.iter().any(|w| w.contains("lacks scdet")),
        "{:?}",
        degraded.jobs[0].warnings
    );
    let (_, mut caps) = engine();
    caps.filters.remove("zscale");
    assert_eq!(codes(&plan_whole(&src, up(|_| {}), Some(caps)).await), ["ENGINE_MISSING"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn warnings_for_tiny_sources_and_smart_without_a_face_model() {
    require_engine!("libx265", "zscale", "cas", "gblur");
    let d = tempfile::tempdir().unwrap();
    let tiny = copy(Fixture::TinySource, d.path(), "tiny.mp4");
    let (_, mut no_model) = engine();
    no_model.face = None;
    let p = plan_whole(&tiny, up(|_| {}), Some(no_model)).await;
    let w = &p.jobs[0].warnings;
    assert!(w.iter().any(|w| w.contains("very small source (320x240)")), "{w:?}");
    assert!(w.iter().any(|w| w == "face model not found; using blurred background"), "{w:?}");
    let center = plan_whole(&tiny, up(|u| u.reframe = Reframe::Center), None).await;
    assert!(!center.jobs[0].warnings.iter().any(|w| w.contains("face model")));
}

#[tokio::test(flavor = "multi_thread")]
async fn output_collisions_follow_on_exists_and_never_hit_the_source() {
    require_engine!("libx265", "zscale", "cas");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::DegradedPortrait, d.path(), "low.mp4");
    std::fs::create_dir(d.path().join("upscaled")).unwrap();
    std::fs::write(d.path().join("upscaled/low_phone-fill.mp4"), b"old").unwrap();
    let with = |policy: OnExists| Options { on_exists: policy, ..up(|_| {}) };
    let skip = plan_whole(&src, with(OnExists::Skip), None).await;
    assert!(matches!(skip.jobs[0].disposition, Disposition::Skip { .. }));
    let rename = plan_whole(&src, with(OnExists::Rename), None).await;
    assert_eq!(rename.jobs[0].output, d.path().join("upscaled/low_phone-fill_1.mp4"));
    let over = plan_whole(&src, with(OnExists::Overwrite), None).await;
    assert!(over.jobs[0].warnings.iter().any(|w| w.contains("replaced")));

    let mut onto_source = up(|_| {});
    onto_source.output_dir = Some(d.path().to_path_buf());
    onto_source.name_template = "{source}.{ext}".into();
    onto_source.on_exists = OnExists::Overwrite;
    let p = plan_whole(&src, onto_source, None).await;
    assert_eq!(codes(&p), ["INVALID_INPUT"]);
    assert!(p.issues[0].message.contains("overwrite a source"));
    assert!(p.issues.iter().all(|i| i.severity == Severity::Error));
}
