//! Quality measures: PSNR, VMAF (videos and GIFs) and the baselines they are compared with.

use super::*;

/// PSNR (dB) between the first frame of `a` and the first frame of `b` at or after `b_time`
/// seconds (user time: ffmpeg subtracts the container start time). The reference is found by
/// decoding `b` sequentially and trimming on timestamps — no seeking, so it does not share
/// demuxer-seek imprecision with the code under test. `f64::INFINITY` = identical frames.
pub fn first_frame_psnr(a: &Path, b: &Path, b_time: f64) -> f64 {
    let graph = format!(
        "[0:v]trim=end_frame=1,setpts=PTS-STARTPTS[x];\
         [1:v]trim=start={b_time:.6},trim=end_frame=1,setpts=PTS-STARTPTS[y];[x][y]psnr"
    );
    let out = Command::new(ffmpeg())
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(a)
        .arg("-i")
        .arg(b)
        .args(["-lavfi", &graph, "-f", "null", "-"])
        .output()
        .expect("run psnr");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let avg = stderr
        .lines()
        .filter(|l| l.contains("PSNR"))
        .find_map(|l| l.split("average:").nth(1))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("no PSNR in output: {stderr}"));
    if avg == "inf" { f64::INFINITY } else { avg.parse().expect("psnr value") }
}

/// VMAF (0–100) of the GIF `gif` against `source`'s frames at `fps` (feature 003). Both sides
/// are renumbered to `fps` so frame `n` of the GIF is compared with frame `n` of the source
/// after the same frame-rate conversion; the source is scaled to the GIF's size with bicubic.
/// Both go through RGB (the source with its own tagged matrix) and then the same RGB → YUV
/// conversion, so only the picture is compared, not a matrix convention.
pub fn vmaf_gif(gif: &Path, source: &Path, fps: u32) -> f64 {
    vmaf_gif_at(gif, source, fps, 0.0)
}

/// [`vmaf_gif`] for a GIF of the source from `start` seconds. The GIF's frames are retimed by
/// their own timestamps (`fps`), so frames that gifsicle merged count for every frame they
/// stand for; both sides are compared in 4:4:4 (a GIF may have an odd size).
pub fn vmaf_gif_at(gif: &Path, source: &Path, fps: u32, start: f64) -> f64 {
    let (w, h) = display_size(gif);
    let work = tempfile_dir("vmaf-gif");
    let graph = format!(
        "[0:v]fps={fps},setpts=N/({fps}*TB),scale={w}:{h}:flags=neighbor,format=rgb24,format=yuv444p[d];\
         [1:v]fps={fps},scale={w}:{h}:flags=bicubic,format=rgb24,setpts=N/({fps}*TB),format=yuv444p[r];\
         [d][r]libvmaf=log_fmt=json:log_path=vmaf.json:n_threads=4:shortest=1"
    );
    let out = Command::new(ffmpeg())
        .current_dir(&work)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-i"])
        .arg(gif)
        .args(["-ss", &format!("{start:.3}"), "-i"])
        .arg(source)
        .args(["-lavfi", &graph, "-f", "null", "-"])
        .output()
        .expect("run libvmaf");
    assert!(out.status.success(), "libvmaf failed: {}", String::from_utf8_lossy(&out.stderr));
    let log: serde_json::Value =
        serde_json::from_slice(&std::fs::read(work.join("vmaf.json")).expect("vmaf log")).expect("vmaf json");
    let _ = std::fs::remove_dir_all(&work);
    log["pooled_metrics"]["vmaf"]["mean"].as_f64().expect("pooled vmaf mean")
}

/// A plain one-pass GIF (`-vf fps,scale`, ffmpeg's default palette handling): the SC-005
/// baseline.
pub fn plain_gif(source: &Path, width: u32, fps: u32, out: &Path) {
    let st = Command::new(ffmpeg())
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-i"])
        .arg(source)
        .args(["-vf", &format!("fps={fps},scale={width}:-2:flags=bicubic"), "-f", "gif"])
        .arg(out)
        .status()
        .expect("run ffmpeg");
    assert!(st.success(), "plain GIF encode failed");
}

/// VMAF (0–100) of `distorted` against `reference` (libvmaf through the engine). The reference
/// is scaled to the distorted size with bicubic, so both must show the same picture: their
/// displayed aspect ratios must match within 1 % (asserted).
pub fn vmaf(distorted: &Path, reference: &Path) -> f64 {
    let (dw, dh) = display_size(distorted);
    let (rw, rh) = display_size(reference);
    let (da, ra) = (f64::from(dw) / f64::from(dh), f64::from(rw) / f64::from(rh));
    assert!((da / ra - 1.0).abs() < 0.01, "vmaf needs matching aspect: {dw}x{dh} vs {rw}x{rh}");
    // libvmaf writes its log relative to the working directory: no path escaping in the graph.
    let work = tempfile_dir("vmaf");
    let graph = format!(
        "[0:v]setpts=PTS-STARTPTS,format=yuv420p[d];[1:v]scale={dw}:{dh}:flags=bicubic,setpts=PTS-STARTPTS,format=yuv420p[r];\
         [d][r]libvmaf=log_fmt=json:log_path=vmaf.json:n_threads=4"
    );
    let out = Command::new(ffmpeg())
        .current_dir(&work)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-i"])
        .arg(distorted)
        .arg("-i")
        .arg(reference)
        .args(["-lavfi", &graph, "-f", "null", "-"])
        .output()
        .expect("run libvmaf");
    assert!(out.status.success(), "libvmaf failed: {}", String::from_utf8_lossy(&out.stderr));
    let log: serde_json::Value =
        serde_json::from_slice(&std::fs::read(work.join("vmaf.json")).expect("vmaf log")).expect("vmaf json");
    let _ = std::fs::remove_dir_all(&work);
    log["pooled_metrics"]["vmaf"]["mean"].as_f64().expect("pooled vmaf mean")
}

/// A plain bicubic resize of `src` to `w`x`h`, encoded like the Reddit preset (x264 CRF 18,
/// BT.709 tags): the baseline that SC-002 compares the CPU tier against. Written to `out`.
pub fn bicubic_baseline(src: &Path, w: u32, h: u32, out: &Path) -> PathBuf {
    let status = Command::new(ffmpeg())
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-i"])
        .arg(src)
        .args(["-vf", &format!("scale={w}:{h}:flags=bicubic,setsar=1"), "-an"])
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "18",
            "-profile:v",
            "high",
            "-pix_fmt",
            "yuv420p",
        ])
        .args([
            "-colorspace",
            "bt709",
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-color_range",
            "tv",
        ])
        .arg(out)
        .status()
        .expect("run ffmpeg for baseline");
    assert!(status.success(), "bicubic baseline failed");
    out.to_path_buf()
}
