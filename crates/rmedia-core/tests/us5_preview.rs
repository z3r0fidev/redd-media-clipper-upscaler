//! T082 (002 US5): the desktop's before/after still. Both pictures come from one ffmpeg run at
//! the preset size within 5 s; smart reframe uses the cached crop path (centred and flagged
//! approximate without one), and a time in a fallback range shows the blurred background.

mod common;

use std::path::{Path, PathBuf};
use std::time::Instant;

use common::*;
use rmedia_core::plan::Caps;
use rmedia_core::preset::PresetRegistry;
use rmedia_core::upscale::preview::{PreviewRequest, preview_frame, single_job};
use rmedia_core::upscale::reframe::{analyze_cached, detect::DetectorSpec};
use rmedia_core::upscale::{TierChoice, UpscaleOptions};
use rmedia_core::{EndSpec, Ts};
use rmedia_testkit::{Fixture, fixture, require_engine, sha256};
use tokio_util::sync::CancellationToken;

fn caps(cache: &Path) -> Caps {
    let (_, mut caps) = engine();
    caps.face = Some(DetectorSpec::Marker);
    caps.reframe_cache = Some(cache.to_path_buf());
    caps
}

fn cpu() -> UpscaleOptions {
    UpscaleOptions { tier: TierChoice::Cpu, ..UpscaleOptions::default() }
}

fn req(path: &Path, at: f64) -> PreviewRequest {
    PreviewRequest { path: path.to_path_buf(), span: None, at: Ts::from_secs_f64(at), options: cpu() }
}

/// Width and height from a PNG's IHDR chunk.
fn png_size(file: &Path) -> (u32, u32) {
    let b = std::fs::read(file).unwrap();
    assert_eq!(&b[..8], b"\x89PNG\r\n\x1a\n", "{} is not a PNG", file.display());
    let be = |i: usize| u32::from_be_bytes(b[i..i + 4].try_into().unwrap());
    (be(16), be(20))
}

fn names(dir: &Path) -> Vec<String> {
    let mut n: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    n.sort();
    n
}

#[tokio::test(flavor = "multi_thread")]
async fn before_and_after_at_the_preset_size_within_5_s() {
    require_engine!("zscale", "cas", "gblur");
    let d = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::Hd60Mp4);
    let before_hash = sha256(&src);
    let (tools, _) = engine();
    let caps = caps(&d.path().join("cache"));
    let out = d.path().join("stills");
    let started = Instant::now();
    let p = preview_frame(
        &tools,
        &caps,
        PresetRegistry::builtin(),
        &req(&src, 10.0),
        &out,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let took = started.elapsed();
    assert!(took.as_secs_f64() < 5.0, "US5 scenario 1: preview took {took:?}");
    assert_eq!(p.size, (1080, 2340));
    assert_eq!(png_size(&p.after), (1080, 2340));
    assert_eq!(png_size(&p.before), (1080, 2340));
    assert_ne!(sha256(&p.after), sha256(&p.before), "the baseline differs from the processed picture");
    // Not analysed yet: the centred crop, flagged as approximate.
    let spec =
        single_job(&tools, &caps, PresetRegistry::builtin(), &src, None, &cpu(), &CancellationToken::new())
            .await
            .unwrap();
    assert!(p.approximate && !p.in_fallback);
    assert_eq!(p.crop_x, spec.up.geometry.crop.map(|c| c.x));
    assert_eq!(names(&out).len(), 2, "only the two stills: {:?}", names(&out));
    assert!(names(&out).iter().all(|n| n.ends_with(".png")));
    assert_eq!(sha256(&src), before_hash, "the source is never written");

    // The same request again reuses the stills.
    let again = preview_frame(
        &tools,
        &caps,
        PresetRegistry::builtin(),
        &req(&src, 10.0),
        &out,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!((again.before.clone(), again.after.clone()), (p.before.clone(), p.after.clone()));
}

#[tokio::test(flavor = "multi_thread")]
async fn smart_reframe_follows_the_cached_path_and_its_fallback_ranges() {
    require_engine!("zscale", "cas", "gblur", "scdet");
    let d = tempfile::tempdir().unwrap();
    let (tools, _) = engine();
    let caps = caps(&d.path().join("cache"));
    let cancel = CancellationToken::new();
    let analysed =
        async |f: Fixture| -> (PathBuf, std::sync::Arc<rmedia_core::upscale::reframe::ReframeAnalysis>) {
            let src = fixture(f);
            let spec = single_job(&tools, &caps, PresetRegistry::builtin(), &src, None, &cpu(), &cancel)
                .await
                .unwrap();
            let (a, _) = analyze_cached(&tools, &spec, &cancel, |_| {}).await.unwrap();
            (src, a)
        };

    let (src, a) = analysed(Fixture::MovingMarker).await;
    let p = preview_frame(
        &tools,
        &caps,
        PresetRegistry::builtin(),
        &req(&src, 3.0),
        &d.path().join("s"),
        &cancel,
    )
    .await
    .unwrap();
    assert!(!p.approximate && !p.in_fallback, "{p:?}");
    let k = (3.0 * a.fps).round() as usize;
    assert_eq!(p.crop_x, Some(a.path[k].max(0) as u32));
    assert_eq!(png_size(&p.after), (1080, 2340));

    let (src, a) = analysed(Fixture::FarMarkers).await;
    assert!(!a.fallback_ranges.is_empty());
    let p = preview_frame(
        &tools,
        &caps,
        PresetRegistry::builtin(),
        &req(&src, 3.0),
        &d.path().join("s"),
        &cancel,
    )
    .await
    .unwrap();
    assert!(p.in_fallback && !p.approximate, "{p:?}");
    assert_eq!(p.crop_x, None);

    // A clip range: the time is source time, inside the range.
    let span = Some((Ts::from_secs_f64(2.0), EndSpec::At(Ts::from_secs_f64(5.0))));
    let src = fixture(Fixture::MovingMarker);
    let outside = PreviewRequest { span, ..req(&src, 1.0) };
    let err = preview_frame(&tools, &caps, PresetRegistry::builtin(), &outside, &d.path().join("s"), &cancel)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("outside"), "{err}");
}
