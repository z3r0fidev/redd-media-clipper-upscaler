//! T050 (002 US2, ignored): smart reframe with the real YuNet model on a real face. The public
//! domain test portrait is fetched by `cargo xtask fetch-models` (never committed). Run with
//! `cargo test -p rmedia-core --test us2_yunet -- --ignored`.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::plan::JobSpec;
use rmedia_core::request::Options;
use rmedia_core::upscale::{AnalysisState, UpscaleOptions, prepare_plan};
use rmedia_testkit::{ffmpeg, require_engine};
use tokio_util::sync::CancellationToken;

const FACE: &str = "test_face_nasa_jsc2003e41874.jpg";

/// A 1920x1080, 6 s video: the portrait (480 px tall) slides from x=200 to x=1200 at
/// 1000/6 px/s ≈ 0.33 crop widths per second.
fn panning_face(face: &Path, dir: &Path) -> PathBuf {
    let out = dir.join("face-pan.mp4");
    let status = Command::new(ffmpeg())
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=0x303840:s=1920x1080:r=30:d=6",
            "-loop",
            "1",
            "-i",
        ])
        .arg(face)
        .args([
            "-filter_complex",
            "[1:v]scale=-2:480[f];[0:v][f]overlay=x=200+1000*t/6:y=300:shortest=1,format=yuv420p[v]",
        ])
        .args(["-map", "[v]", "-t", "6", "-c:v", "libx264", "-preset", "veryfast", "-crf", "18"])
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());
    out
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the fetched test face; slow in debug builds"]
async fn yunet_follows_a_real_face() {
    require_engine!("libx264", "sendcmd", "scdet");
    let face = workspace_model(FACE).expect("run `cargo xtask fetch-models` first");
    let d = tempfile::tempdir().unwrap();
    let src = panning_face(&face, d.path());
    let (tools, mut caps) = engine();
    caps.reframe_cache = Some(d.path().join("cache"));
    caps.face = Some(yunet().expect("YuNet must be fetched"));
    let opts = Options {
        upscale: Some(UpscaleOptions::default()),
        output_dir: Some(d.path().to_path_buf()),
        ..Options::default()
    };
    let mut p = make_plan(&request(&src, vec![clip(0.0, EndSpec::EndOfMedia)], opts), &tools, &caps).await;
    prepare_plan(&mut p, &tools, &CancellationToken::new()).await.unwrap();
    let JobSpec::Upscale(u) = &p.jobs[0].spec else { panic!() };
    let AnalysisState::Ready(a, _) = &u.up.analysis else { panic!("{:?}", u.up.analysis) };
    assert!(
        a.detections >= a.samples * 9 / 10,
        "the face is found in most samples: {}/{}",
        a.detections,
        a.samples
    );
    assert!(a.fallback_ranges.is_empty(), "{:?}", a.fallback_ranges);
    // The face (≈384 px wide, centre at 200 + 192 + 1000 t/6) stays inside the 498 px crop.
    for k in (0..a.path.len()).step_by(15) {
        let t = k as f64 / a.fps;
        let centre = 200.0 + 192.0 + 1000.0 * t / 6.0;
        let x = f64::from(a.path[k]);
        assert!(
            centre > x && centre < x + f64::from(a.crop.0),
            "frame {k}: face centre {centre:.0}, crop {x}..{}",
            x + f64::from(a.crop.0)
        );
    }
}
