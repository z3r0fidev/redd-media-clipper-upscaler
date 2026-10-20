//! `rmedia-sr`: the AI tier's frame filter (contracts/rmedia-sr-protocol.md). It reads packed
//! `rgb24` frames on stdin, upscales each with a Real-ESRGAN ncnn model on a Vulkan GPU and writes
//! them to stdout in order. It makes no media decisions; rmedia-core picks the model, scale, GPU
//! and frame size, and runs it between two ffmpeg processes. stderr carries JSON lines only.

mod args;
mod ffi;
mod upscale;

use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::{Duration, Instant};

use serde_json::json;

use args::{Fp16, Job, Mode, Model};
use upscale::Upscaler;

/// A failure, with the protocol's exit code and error code.
#[derive(Debug)]
pub enum Failure {
    Usage(String),
    Gpu(&'static str, String),
    Io(&'static str, String),
    Model(String),
    Runtime(String),
}

impl Failure {
    fn exit_code(&self) -> i32 {
        match self {
            Failure::Usage(_) => 2,
            Failure::Gpu(..) => 3,
            Failure::Io(..) => 4,
            Failure::Model(_) => 5,
            Failure::Runtime(_) => 6,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Failure::Usage(_) => "USAGE",
            Failure::Gpu(c, _) | Failure::Io(c, _) => c,
            Failure::Model(_) => "MODEL_LOAD",
            Failure::Runtime(_) => "GPU_RUNTIME",
        }
    }

    fn message(&self) -> &str {
        match self {
            Failure::Usage(m)
            | Failure::Gpu(_, m)
            | Failure::Io(_, m)
            | Failure::Model(m)
            | Failure::Runtime(m) => m,
        }
    }
}

/// The JSON-lines channel on (the original) stderr.
pub struct Protocol(Box<dyn Write + Send>);

impl std::fmt::Debug for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Protocol")
    }
}

impl Protocol {
    pub fn send(&mut self, v: serde_json::Value) {
        // A closed stderr only loses diagnostics; the frames still flow.
        let _ = writeln!(self.0, "{v}").and_then(|()| self.0.flush());
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut proto = Protocol(ffi::protocol_stream());
    let code = match run(&argv, &mut proto) {
        Ok(()) => 0,
        Err(f) => {
            proto.send(json!({"type": "error", "code": f.code(), "message": f.message()}));
            f.exit_code()
        }
    };
    std::process::exit(code);
}

fn run(argv: &[String], proto: &mut Protocol) -> Result<(), Failure> {
    match args::parse(argv).map_err(Failure::Usage)? {
        Mode::Version => {
            println!("rmedia-sr {} (ncnn {})", env!("CARGO_PKG_VERSION"), ffi::version());
            Ok(())
        }
        Mode::ListGpus => {
            let devices: Vec<_> = match ffi::Gpu::init() {
                Some(gpu) => gpu
                    .devices()
                    .iter()
                    .map(|d| {
                        json!({
                            "index": d.index,
                            "name": d.name,
                            "kind": upscale::kind_name(d.kind),
                            "software": upscale::is_software(d),
                            "fp16": d.fp16,
                            "heap_budget_mb": d.heap_budget_mb,
                        })
                    })
                    .collect(),
                None => Vec::new(),
            };
            println!("{}", json!({ "devices": devices }));
            Ok(())
        }
        Mode::Run(job) => with_upscaler(&job, proto, |up, proto| stream(&job, up, proto)),
        Mode::SelfTest(job, frames) => {
            with_upscaler(&job, proto, |up, proto| self_test(&job, up, frames, proto))
        }
    }
}

/// Builds the job's upscaler (opening the GPU when it needs one) and runs `f` with it.
fn with_upscaler(
    job: &Job,
    proto: &mut Protocol,
    f: impl FnOnce(&mut dyn Upscaler, &mut Protocol) -> Result<(), Failure>,
) -> Result<(), Failure> {
    if job.model == Model::Nearest {
        return f(&mut upscale::Nearest::new(job), proto);
    }
    let gpu =
        ffi::Gpu::init().ok_or_else(|| Failure::Gpu("NO_VULKAN", "no Vulkan driver is available".into()))?;
    let mut sr = upscale::Sr::open(&gpu, job)?;
    let d = sr.device();
    proto.send(json!({
        "type": "device",
        "index": d.index,
        "name": d.name,
        "kind": upscale::kind_name(d.kind),
        "fp16": d.fp16 && job.fp16 != Fp16::Off,
        "tile": sr.tile(),
    }));
    f(&mut sr, proto)
}

const PROGRESS_EVERY: Duration = Duration::from_millis(250);
/// Frames buffered between the reader, the GPU and the writer.
const QUEUE: usize = 2;

/// The frame loop: a reader thread, the upscaler on this thread, a writer thread, so reading,
/// GPU work and writing overlap. Input buffers are recycled; memory stays at a few frames.
fn stream(job: &Job, up: &mut dyn Upscaler, proto: &mut Protocol) -> Result<(), Failure> {
    let in_len = job.width as usize * job.height as usize * 3;
    let out_len = in_len * (job.scale * job.scale) as usize;
    let (full_tx, full_rx) = sync_channel::<Vec<u8>>(QUEUE);
    let (free_tx, free_rx) = sync_channel::<Vec<u8>>(QUEUE + 2);
    let (done_tx, done_rx) = sync_channel::<Vec<u8>>(QUEUE);
    for _ in 0..QUEUE + 2 {
        let _ = free_tx.send(vec![0u8; in_len]);
    }
    // Detached threads: on an upscaler error the worker exits at once (the core is killing the
    // pipeline anyway), without waiting for a reader blocked on stdin.
    let reader = std::thread::spawn(move || read_frames(in_len, &free_rx, &full_tx));
    let writer = std::thread::spawn(move || write_frames(&done_rx));
    let started = Instant::now();
    let mut last = started;
    let mut frames = 0u64;
    for frame in full_rx.iter() {
        let mut out = vec![0u8; out_len];
        up.process(&frame, &mut out)?;
        let _ = free_tx.send(frame);
        if done_tx.send(out).is_err() {
            break; // the writer failed; its error is reported below
        }
        frames += 1;
        if last.elapsed() >= PROGRESS_EVERY {
            last = Instant::now();
            let fps = frames as f64 / started.elapsed().as_secs_f64().max(1e-6);
            proto.send(json!({"type": "progress", "frames": frames, "fps": (fps * 100.0).round() / 100.0}));
        }
    }
    drop(done_tx);
    writer.join().map_err(|_| Failure::Io("WRITE", "the writer thread panicked".into()))??;
    drop(free_tx);
    reader.join().map_err(|_| Failure::Io("READ", "the reader thread panicked".into()))??;
    let fps = frames as f64 / started.elapsed().as_secs_f64().max(1e-6);
    proto.send(json!({"type": "progress", "frames": frames, "fps": (fps * 100.0).round() / 100.0}));
    Ok(())
}

fn read_frames(len: usize, free: &Receiver<Vec<u8>>, full: &SyncSender<Vec<u8>>) -> Result<(), Failure> {
    let mut stdin = std::io::stdin().lock();
    for mut buf in free.iter() {
        buf.resize(len, 0);
        let mut filled = 0;
        while filled < len {
            match stdin.read(&mut buf[filled..]) {
                Ok(0) if filled == 0 => return Ok(()),
                Ok(0) => {
                    return Err(Failure::Io(
                        "SHORT_FRAME",
                        format!("the input ended inside a frame ({filled} of {len} bytes)"),
                    ));
                }
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(Failure::Io("READ", format!("reading frames: {e}"))),
            }
        }
        if full.send(buf).is_err() {
            return Ok(()); // the upscaler stopped
        }
    }
    Ok(())
}

fn write_frames(done: &Receiver<Vec<u8>>) -> Result<(), Failure> {
    let mut stdout = std::io::stdout().lock();
    for frame in done.iter() {
        stdout.write_all(&frame).map_err(|e| Failure::Io("WRITE", format!("writing frames: {e}")))?;
    }
    stdout.flush().map_err(|e| Failure::Io("WRITE", format!("writing frames: {e}")))
}

/// Times `frames` synthetic frames (after one untimed warm-up that builds the GPU pipelines)
/// and reports seconds per frame and per input megapixel.
fn self_test(job: &Job, up: &mut dyn Upscaler, frames: u32, proto: &mut Protocol) -> Result<(), Failure> {
    let (w, h) = (job.width as usize, job.height as usize);
    // A smooth gradient with fine texture: representative work, deterministic.
    let frame: Vec<u8> = (0..w * h * 3)
        .map(|i| {
            let (p, c) = (i / 3, i % 3);
            let (x, y) = (p % w, p / w);
            ((x * 255 / w.max(1)) as u32 + (y * 7 + c * 31) as u32 % 23 + ((x ^ y) & 7) as u32) as u8
        })
        .collect();
    let mut out = vec![0u8; frame.len() * (job.scale * job.scale) as usize];
    up.process(&frame, &mut out)?;
    let started = Instant::now();
    for _ in 0..frames {
        up.process(&frame, &mut out)?;
    }
    let per_frame = started.elapsed().as_secs_f64() / f64::from(frames);
    let mpx = (w * h) as f64 / 1e6;
    proto.send(json!({
        "type": "selftest",
        "frames": frames,
        "width": job.width,
        "height": job.height,
        "scale": job.scale,
        "sec_per_frame": (per_frame * 1e4).round() / 1e4,
        "sec_per_mpx": (per_frame / mpx * 1e4).round() / 1e4,
    }));
    Ok(())
}
