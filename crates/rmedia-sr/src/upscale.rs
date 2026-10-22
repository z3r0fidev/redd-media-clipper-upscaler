//! The upscalers: the ncnn model on a GPU (tiled), and the test-only nearest-neighbour one.

use crate::Failure;
use crate::args::{Fp16, Job, Model};
use crate::ffi::{DeviceInfo, Gpu, ModelSource, Net, NetOptions};

pub trait Upscaler {
    /// Upscales one packed RGB frame of the job's size into `out` (`scale²` times larger).
    fn process(&mut self, frame: &[u8], out: &mut [u8]) -> Result<(), Failure>;
}

/// ncnn's device type as the protocol names it.
pub fn kind_name(kind: i32) -> &'static str {
    match kind {
        0 => "discrete",
        1 => "integrated",
        2 => "virtual",
        3 => "cpu",
        _ => "other",
    }
}

/// Software rasterisers (llvmpipe, lavapipe, SwiftShader) and CPU devices are never used: they
/// are far too slow and would hide a missing GPU (research R9).
pub fn is_software(d: &DeviceInfo) -> bool {
    let name = d.name.to_ascii_lowercase();
    d.kind == 3 || ["llvmpipe", "lavapipe", "swiftshader", "softpipe"].iter().any(|s| name.contains(s))
}

/// Test only: each pixel becomes an s×s block.
#[derive(Debug)]
pub struct Nearest {
    w: usize,
    h: usize,
    s: usize,
}

impl Nearest {
    pub fn new(job: &Job) -> Nearest {
        Nearest { w: job.width as usize, h: job.height as usize, s: job.scale as usize }
    }
}

impl Upscaler for Nearest {
    fn process(&mut self, frame: &[u8], out: &mut [u8]) -> Result<(), Failure> {
        let (w, s) = (self.w, self.s);
        let row = w * s * 3;
        for y in 0..self.h {
            let src = &frame[y * w * 3..(y + 1) * w * 3];
            let first = y * s * row;
            for (x, px) in src.chunks_exact(3).enumerate() {
                for k in 0..s {
                    let o = first + (x * s + k) * 3;
                    out[o..o + 3].copy_from_slice(px);
                }
            }
            for k in 1..s {
                out.copy_within(first..first + row, first + k * row);
            }
        }
        Ok(())
    }
}

/// Context kept around each tile so seams match whole-frame inference (the reference tool's
/// prepadding for these models).
const PAD: u32 = 10;

/// The Real-ESRGAN model on one Vulkan device.
#[derive(Debug)]
pub struct Sr<'g> {
    net: Net<'g>,
    device: DeviceInfo,
    size: (u32, u32),
    scale: u32,
    tile: u32,
}

impl<'g> Sr<'g> {
    pub fn open(gpu: &'g Gpu, job: &Job) -> Result<Sr<'g>, Failure> {
        let Model::Files(base) = &job.model else { unreachable!("builtin models never open a GPU") };
        let index = job.gpu.expect("a GPU index (checked by the argv parser)");
        let devices = gpu.devices();
        let device = devices.iter().find(|d| d.index == index).cloned().ok_or_else(|| {
            Failure::Gpu("NO_DEVICE", format!("there is no Vulkan device {index} ({} found)", devices.len()))
        })?;
        if is_software(&device) {
            return Err(Failure::Gpu(
                "SOFTWARE_DEVICE",
                format!("device {index} ({}) is a software renderer, not a GPU", device.name),
            ));
        }
        let with_ext = |ext: &str| {
            let mut p = base.clone().into_os_string();
            p.push(ext);
            std::path::PathBuf::from(p)
        };
        let (param, bin) = (with_ext(".param"), with_ext(".bin"));
        for f in [&param, &bin] {
            if !f.is_file() {
                return Err(Failure::Model(format!("{} does not exist", f.display())));
            }
        }
        let fp16 = match job.fp16 {
            Fp16::Auto => None,
            Fp16::On => Some(true),
            Fp16::Off => Some(false),
        };
        let opts = NetOptions { device: index, fp16, threads: job.threads, winograd: device.kind != 1 };
        let source = match (&job.param_sha256, &job.bin_sha256) {
            (Some(ps), Some(bs)) => {
                ModelSource::Verified { param: verified(&param, ps)?, bin: verified(&bin, bs)? }
            }
            _ => ModelSource::Files { param: &param, bin: &bin },
        };
        let net = Net::load(gpu, opts, source).map_err(Failure::Model)?;
        let tile = if job.tile > 0 { job.tile } else { auto_tile(device.heap_budget_mb) };
        Ok(Sr { net, device, size: (job.width, job.height), scale: job.scale, tile })
    }

    pub fn device(&self) -> &DeviceInfo {
        &self.device
    }

    pub fn tile(&self) -> u32 {
        self.tile
    }
}

/// The bytes of `path` if their SHA-256 is `sha` (read once: what is checked is what loads).
fn verified(path: &std::path::Path, sha: &str) -> Result<Vec<u8>, Failure> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).map_err(|e| Failure::Model(format!("{}: {e}", path.display())))?;
    if Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect::<String>() != sha {
        return Err(Failure::Model(format!(
            "{} is not the expected model file (SHA-256 differs)",
            path.display()
        )));
    }
    Ok(bytes)
}

/// The reference tool's input tile sizes by GPU memory budget.
fn auto_tile(heap_mb: u32) -> u32 {
    match heap_mb {
        // Unknown budget: a middle size rather than the slowest.
        0 => 100,
        m if m > 1900 => 200,
        m if m > 550 => 100,
        m if m > 190 => 64,
        _ => 32,
    }
}

/// The tiles covering `len` pixels: `(start, end)` of each.
fn spans(len: u32, tile: u32) -> impl Iterator<Item = (u32, u32)> {
    (0..len.div_ceil(tile)).map(move |i| (i * tile, ((i + 1) * tile).min(len)))
}

impl Upscaler for Sr<'_> {
    fn process(&mut self, frame: &[u8], out: &mut [u8]) -> Result<(), Failure> {
        let ((w, h), s) = (self.size, self.scale);
        let out_row = (w * s * 3) as usize;
        for (y0, y1) in spans(h, self.tile) {
            for (x0, x1) in spans(w, self.tile) {
                // The tile plus its context, clamped to the frame.
                let (rx0, ry0) = (x0.saturating_sub(PAD), y0.saturating_sub(PAD));
                let (rx1, ry1) = ((x1 + PAD).min(w), (y1 + PAD).min(h));
                let ow = (rx1 - rx0) * s;
                let pixels = self
                    .net
                    .run(frame, (w, h), (rx0, ry0, rx1 - rx0, ry1 - ry0), (ow, (ry1 - ry0) * s))
                    .map_err(Failure::Runtime)?;
                // Copy the tile's own part (without the context) into place.
                let (cx, cy) = ((x0 - rx0) * s, (y0 - ry0) * s);
                let (cw, ch) = ((x1 - x0) * s, (y1 - y0) * s);
                for row in 0..ch {
                    let src = (((cy + row) * ow + cx) * 3) as usize;
                    let dst = ((y0 * s + row) as usize) * out_row + (x0 * s * 3) as usize;
                    let n = (cw * 3) as usize;
                    out[dst..dst + n].copy_from_slice(&pixels[src..src + n]);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_makes_blocks() {
        let job = Job {
            model: Model::Nearest,
            scale: 2,
            gpu: None,
            width: 2,
            height: 1,
            tile: 0,
            fp16: Fp16::Auto,
            threads: 1,
            param_sha256: None,
            bin_sha256: None,
        };
        let mut out = vec![0; 2 * 4 * 3];
        Nearest::new(&job).process(&[1, 2, 3, 4, 5, 6], &mut out).unwrap();
        let row = [1, 2, 3, 1, 2, 3, 4, 5, 6, 4, 5, 6];
        assert_eq!(&out[..12], &row);
        assert_eq!(&out[12..], &row);
    }

    #[test]
    fn tiles_cover_the_frame_and_software_devices_are_refused() {
        assert_eq!(spans(250, 100).collect::<Vec<_>>(), [(0, 100), (100, 200), (200, 250)]);
        assert_eq!(spans(64, 100).collect::<Vec<_>>(), [(0, 64)]);
        assert_eq!((auto_tile(4000), auto_tile(600), auto_tile(200), auto_tile(100)), (200, 100, 64, 32));
        assert_eq!(auto_tile(0), 100, "unknown budget");
        let d = |name: &str, kind| DeviceInfo {
            index: 0,
            name: name.into(),
            kind,
            fp16: true,
            heap_budget_mb: 0,
        };
        assert!(is_software(&d("llvmpipe (LLVM 20.1.2, 256 bits)", 3)));
        assert!(is_software(&d("SwiftShader Device", 2)));
        assert!(!is_software(&d("Intel(R) Graphics (RPL-U)", 1)));
        assert!(!is_software(&d("NVIDIA GeForce RTX 4070", 0)));
    }
}
