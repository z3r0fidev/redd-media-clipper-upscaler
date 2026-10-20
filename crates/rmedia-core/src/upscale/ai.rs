//! The AI tier (US6, research R13): the `rmedia-sr` worker and its Real-ESRGAN models, how big
//! the worker's frames are and by how much it scales them. The worker is a frame filter between
//! two ffmpeg processes (`exec::pipeline`); everything it is told comes from here.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::Reframe;
use super::geometry::Overflow;
use super::graph::GraphInput;

/// The model family (BSD-3, `cargo xtask fetch-models`): `<MODEL>-x{2,3,4}.param/.bin`.
pub const MODEL: &str = "realesr-animevideov3";
/// Worker program name (bundled beside rmedia).
pub const WORKER: &str = "rmedia-sr";
/// Scales the models come in.
pub const SCALES: [u32; 3] = [2, 3, 4];

/// SHA-256 of every model file the AI tier runs (as pinned in `xtask/models.lock.json`). ncnn
/// parses models in C++, so only these exact files are used, wherever they are found; the
/// worker checks them again on the bytes it loads.
pub const PINNED: [(&str, &str); 6] = [
    ("realesr-animevideov3-x2.param", "b88ff4f00ebf019a7fdac17fdd45a7fd3665d37509efc5baf2e4da2e24420a04"),
    ("realesr-animevideov3-x2.bin", "548a36f9c3f4ab8da56cd3b13badf23968bee207b396dad14d04b830e5f2ab2d"),
    ("realesr-animevideov3-x3.param", "d1a5755008791d09b57e3425fc9dd0bd26b00fdf79c606210bc0e693f8230881"),
    ("realesr-animevideov3-x3.bin", "548a36f9c3f4ab8da56cd3b13badf23968bee207b396dad14d04b830e5f2ab2d"),
    ("realesr-animevideov3-x4.param", "850a248e7c14c27e5bd8cf7265113a9441036a7db63963bb8aa5169d788a435e"),
    ("realesr-animevideov3-x4.bin", "548a36f9c3f4ab8da56cd3b13badf23968bee207b396dad14d04b830e5f2ab2d"),
];

/// The pinned SHA-256 of model file `name`.
pub fn pinned_sha(name: &str) -> Option<&'static str> {
    PINNED.iter().find(|(n, _)| *n == name).map(|(_, h)| *h)
}

/// What the worker runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiModel {
    /// Model base paths (`<dir>/<name>`, without `.param`/`.bin`) by scale.
    Files(BTreeMap<u32, PathBuf>),
    /// Test only: the worker's CPU nearest-neighbour scaler (`--gpu none`); never discovered,
    /// only set by tests.
    #[cfg(any(test, feature = "test-support"))]
    BuiltinNearest,
}

/// The worker program and its models, as found on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiEngine {
    pub worker: PathBuf,
    pub model: AiModel,
}

impl AiEngine {
    /// The worker beside the executable (or `RMEDIA_SR`) and a model set with every scale in one
    /// of the model folders (research R14). `None` when either is missing.
    pub fn discover(settings: &crate::settings::Settings) -> Option<AiEngine> {
        let worker = locate_worker()?;
        let model = locate_models(&crate::models::search_dirs(settings))?;
        Some(AiEngine { worker, model: AiModel::Files(model) })
    }

    /// The worker's `--model` value for `scale`.
    pub fn model_arg(&self, scale: u32) -> Option<OsString> {
        match &self.model {
            AiModel::Files(m) => m.get(&scale).map(|p| p.clone().into_os_string()),
            #[cfg(any(test, feature = "test-support"))]
            AiModel::BuiltinNearest => Some("builtin:nearest".into()),
        }
    }

    /// The worker's `--gpu` value: the device, or `none` for the builtin test model.
    fn gpu_arg(&self, device: Option<u32>) -> String {
        match (&self.model, device) {
            #[cfg(any(test, feature = "test-support"))]
            (AiModel::BuiltinNearest, _) => "none".into(),
            (_, None) => "none".into(),
            (_, Some(d)) => d.to_string(),
        }
    }

    /// The worker argv for frames of `input` size scaled by `scale` on `device`. Real models
    /// carry their pinned hashes, which the worker checks on the bytes it loads.
    pub fn args(&self, scale: u32, device: Option<u32>, input: (u32, u32)) -> Vec<OsString> {
        let mut a: Vec<OsString> = vec!["--model".into(), self.model_arg(scale).unwrap_or_default()];
        if matches!(self.model, AiModel::Files(_)) {
            for (flag, ext) in [("--param-sha256", "param"), ("--bin-sha256", "bin")] {
                if let Some(h) = pinned_sha(&format!("{MODEL}-x{scale}.{ext}")) {
                    a.push(flag.into());
                    a.push(h.into());
                }
            }
        }
        for (k, v) in [
            ("--scale", scale.to_string()),
            ("--gpu", self.gpu_arg(device)),
            ("--width", input.0.to_string()),
            ("--height", input.1.to_string()),
        ] {
            a.push(k.into());
            a.push(v.into());
        }
        a
    }
}

/// `RMEDIA_SR`, then `rmedia-sr` beside the executable, then (development and tests) in the
/// Cargo target folder the executable is in.
pub fn locate_worker() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("RMEDIA_SR").filter(|v| !v.is_empty()) {
        let p = PathBuf::from(p);
        // The same trust as the environment itself; logged so it is never a silent swap.
        tracing::info!("AI worker from RMEDIA_SR: {}", p.display());
        return p.is_file().then_some(p);
    }
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let name = format!("{WORKER}{}", std::env::consts::EXE_SUFFIX);
    std::iter::once(exe_dir.clone())
        // target/<profile>/deps/ (test binaries) → target/<profile>/
        .chain(cfg!(debug_assertions).then(|| exe_dir.parent().map(Path::to_path_buf)).flatten())
        .map(|d| d.join(&name))
        .find(|p| p.is_file())
}

/// The first folder holding every scale of the model (`.param` and `.bin`) with the pinned
/// contents ([`PINNED`]); paths are absolute (the worker runs in a job's scratch folder).
pub fn locate_models(dirs: &[PathBuf]) -> Option<BTreeMap<u32, PathBuf>> {
    use sha2::{Digest, Sha256};
    let pinned = |p: &Path| {
        let name = p.file_name()?.to_str()?;
        let bytes = std::fs::metadata(p)
            .ok()
            .filter(|m| m.is_file() && m.len() < 64 << 20)
            .and(std::fs::read(p).ok())?;
        (format!("{:x}", Sha256::digest(&bytes)) == pinned_sha(name)?).then_some(())
    };
    dirs.iter().find_map(|d| {
        let d = crate::paths::canonicalize(d).ok()?;
        SCALES
            .iter()
            .map(|s| {
                let base = d.join(format!("{MODEL}-x{s}"));
                ["param", "bin"]
                    .iter()
                    .all(|e| pinned(&base.with_extension(e)).is_some())
                    .then_some((*s, base))
            })
            .collect::<Option<BTreeMap<_, _>>>()
    })
}

/// What the worker sees and does for a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiStage {
    /// The worker's input frame size: the reframe crop at source scale, or the whole (square-
    /// pixel) frame for the blurred background and shapes that already match — shrunk to half
    /// of `fit` when it is bigger than that (`shrunk`), so the model never makes pixels that
    /// are thrown away.
    pub input: (u32, u32),
    pub shrunk: bool,
    pub scale: u32,
    /// The picture the AI frames are scaled to: the target, or the fitted foreground over the
    /// blurred background.
    pub fit: (u32, u32),
}

impl AiStage {
    pub fn output(&self) -> (u32, u32) {
        (self.input.0 * self.scale, self.input.1 * self.scale)
    }
}

/// The worker's frames for a job's graph.
pub fn stage(g: &GraphInput) -> AiStage {
    let geo = &g.geometry;
    let blurred = g.reframe == Reframe::Blur && geo.overflow != Overflow::None;
    let (input, fit) = match (blurred, geo.crop) {
        (false, Some(c)) => ((c.w, c.h), geo.target),
        (true, _) => (geo.display, geo.fit),
        (false, None) => (geo.display, geo.target),
    };
    if input.0 * 2 >= fit.0 && input.1 * 2 >= fit.1 {
        // ×2 from half the output: the model restores detail, nothing is wasted.
        let half = (fit.0.div_ceil(2), fit.1.div_ceil(2));
        return AiStage { input: half, shrunk: half != input, scale: 2, fit };
    }
    AiStage { input, shrunk: false, scale: scale_for(input, fit), fit }
}

/// The smallest model scale that reaches `fit` from `input` (×4 at most; ffmpeg scales the rest).
pub fn scale_for(input: (u32, u32), fit: (u32, u32)) -> u32 {
    SCALES
        .iter()
        .copied()
        .find(|s| input.0 * s >= fit.0 && input.1 * s >= fit.1)
        .unwrap_or(SCALES[SCALES.len() - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_smallest_scale_that_reaches_the_target() {
        assert_eq!(scale_for((540, 1170), (1080, 2340)), 2);
        assert_eq!(scale_for((540, 1169), (1080, 2340)), 3);
        assert_eq!(scale_for((300, 640), (1080, 2340)), 4);
        assert_eq!(scale_for((100, 200), (1080, 2340)), 4, "×4 at most");
        assert_eq!(scale_for((2000, 4000), (1080, 2340)), 2, "never below ×2");
    }

    #[test]
    fn large_inputs_are_shrunk_to_half_the_output() {
        use crate::upscale::geometry;
        let g = |display, reframe| GraphInput {
            trim: None,
            deinterlace: false,
            square_pixels: false,
            tonemap: false,
            keep_hdr: false,
            colour_in: String::new(),
            fps_cap: None,
            geometry: geometry::plan(display, (1080, 2340), reframe),
            reframe,
            track: None,
            gpu: None,
            params_in: String::new(),
        };
        let blur = stage(&g((1920, 1080), Reframe::Blur));
        assert_eq!((blur.input, blur.shrunk, blur.scale, blur.fit), ((540, 304), true, 2, (1080, 608)));
        let crop = stage(&g((480, 854), Reframe::Center));
        assert_eq!((crop.input, crop.shrunk, crop.scale), ((394, 854), false, 3));
        let tiny = stage(&g((320, 240), Reframe::Center));
        assert_eq!((tiny.input, tiny.scale), ((110, 240), 4));
    }

    #[test]
    fn pins_match_the_lock_file() {
        let lock: serde_json::Value =
            serde_json::from_str(include_str!("../../../../xtask/models.lock.json")).unwrap();
        for (name, sha) in PINNED {
            assert_eq!(lock["models"][name]["sha256"], sha, "{name}");
        }
        assert_eq!(PINNED.len(), SCALES.len() * 2);
    }

    #[test]
    fn only_a_complete_set_with_the_pinned_contents_is_used() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        for s in SCALES {
            for e in ["param", "bin"] {
                std::fs::write(b.join(format!("{MODEL}-x{s}.{e}")), b"x").unwrap();
            }
        }
        assert_eq!(locate_models(std::slice::from_ref(&b)), None, "files that are not the pinned models are refused");
        // The real models, when fetched (`cargo xtask fetch-models`).
        let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models");
        if real.join(format!("{MODEL}-x4.bin")).is_file() {
            std::fs::write(a.join(format!("{MODEL}-x2.param")), b"x").unwrap();
            let m = locate_models(&[a, b, real.clone()]).unwrap();
            assert_eq!(m[&3], crate::paths::canonicalize(&real).unwrap().join(format!("{MODEL}-x3")));
        }
    }

    #[test]
    fn worker_argv() {
        let e = AiEngine {
            worker: "/w".into(),
            model: AiModel::Files(BTreeMap::from([(2, PathBuf::from("/m/realesr-animevideov3-x2"))])),
        };
        let a: Vec<String> =
            e.args(2, Some(1), (480, 270)).iter().map(|s| s.to_string_lossy().into()).collect();
        assert_eq!(
            a,
            [
                "--model",
                "/m/realesr-animevideov3-x2",
                "--param-sha256",
                "b88ff4f00ebf019a7fdac17fdd45a7fd3665d37509efc5baf2e4da2e24420a04",
                "--bin-sha256",
                "548a36f9c3f4ab8da56cd3b13badf23968bee207b396dad14d04b830e5f2ab2d",
                "--scale",
                "2",
                "--gpu",
                "1",
                "--width",
                "480",
                "--height",
                "270"
            ]
        );
        let t = AiEngine { worker: "/w".into(), model: AiModel::BuiltinNearest };
        let a: Vec<String> = t.args(3, Some(0), (4, 4)).iter().map(|s| s.to_string_lossy().into()).collect();
        assert_eq!(a[..6], ["--model", "builtin:nearest", "--scale", "3", "--gpu", "none"]);
    }
}
