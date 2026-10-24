//! Preview media for the desktop app (research.md R8): still frames, browser-playable proxies and
//! thumbnail strips. Only the GUI uses this, but it is media logic, so it lives in the core.
//!
//! Time base: the webview's `currentTime` for a proxy (or a directly played file) is ffmpeg's
//! output timeline, which starts at the container start. User time 0 is the first video frame, so
//! `user = currentTime - time_offset(media)`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::exec::commit::remove_with_retry;
use crate::exec::process::{run_capture, run_lines};
use crate::exec::progress::ProgressParser;
use crate::probe::{INPUT_PROTOCOLS, MediaInfo, StreamKind, file_arg};
use crate::time::Ts;
use crate::tools::Tools;

/// Seconds to subtract from a player's `currentTime` to get user time.
pub fn time_offset(media: &MediaInfo) -> Ts {
    media.zero_point().saturating_sub(media.start_time)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum ProxyCodec {
    /// H.264 + AAC in MP4 (WebView2, most WebKitGTK installs).
    H264,
    /// VP8 + Opus in WebM (WebKitGTK without an H.264 decoder).
    Vp8,
}

/// One JPEG frame at user time `t`, at most `max_height` pixels tall.
pub async fn frame_at(
    tools: &Tools,
    media: &MediaInfo,
    t: Ts,
    max_height: u32,
    cancel: &CancellationToken,
) -> Result<Vec<u8>> {
    let seek = t + time_offset(media);
    let mut args: Vec<OsString> =
        ["-hide_banner", "-nostdin", "-loglevel", "error"].map(OsString::from).to_vec();
    args.extend(["-protocol_whitelist", INPUT_PROTOCOLS, "-ss"].map(OsString::from));
    args.push(seek.ffmpeg().into());
    args.push("-i".into());
    args.push(file_arg(&media.path));
    args.extend(
        [
            "-map",
            "0:v:0",
            "-frames:v",
            "1",
            "-vf",
            &format!("scale=-2:'min({max_height},ih)'"),
            "-c:v",
            "mjpeg",
            "-q:v",
            "4",
            "-f",
            "image2pipe",
            "pipe:1",
        ]
        .map(OsString::from),
    );
    let out = run_capture(&tools.ffmpeg.path, &args, cancel).await?;
    if !out.status.success() || out.stdout.is_empty() {
        return Err(Error::engine_failed(format!("could not extract a frame at {t}"), out.stderr_tail));
    }
    Ok(out.stdout)
}

/// Proxy and thumbnail cache with a size cap (least recently used files are evicted).
#[derive(Debug, Clone)]
pub struct PreviewCache {
    pub dir: PathBuf,
    pub max_bytes: u64,
}

impl PreviewCache {
    /// Stable key for a source version: canonical path, size and modification time.
    pub fn key(media: &MediaInfo) -> String {
        let canonical = std::fs::canonicalize(&media.path).unwrap_or_else(|_| media.path.clone());
        let mtime = std::fs::metadata(&media.path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        let mut h = Sha256::new();
        h.update(canonical.as_os_str().as_encoded_bytes());
        h.update(media.size.to_le_bytes());
        h.update(mtime.to_le_bytes());
        crate::hex(&h.finalize())[..32].to_owned()
    }

    fn proxy_path(&self, media: &MediaInfo, codec: ProxyCodec) -> PathBuf {
        let ext = match codec {
            ProxyCodec::H264 => "mp4",
            ProxyCodec::Vp8 => "webm",
        };
        self.dir.join(format!("{}-proxy.{ext}", Self::key(media)))
    }

    /// Deletes least-recently-used cache files until the cache fits `max_bytes`.
    pub fn evict(&self, keep: Option<&Path>) {
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return };
        let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = rd
            .flatten()
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                if m.is_file() { Some((m.modified().ok()?, m.len(), e.path())) } else { None }
            })
            .collect();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        files.sort_by_key(|f| f.0);
        for (_, len, path) in files {
            if total <= self.max_bytes {
                break;
            }
            if Some(path.as_path()) == keep {
                continue;
            }
            remove_with_retry(&path);
            total = total.saturating_sub(len);
        }
    }
}

/// Returns a browser-playable 480p proxy, encoding it if needed. `on_progress` gets 0.0–1.0.
/// Timestamps are passed through, so proxy time equals source time.
pub async fn ensure_proxy(
    tools: &Tools,
    media: &MediaInfo,
    cache: &PreviewCache,
    codec: ProxyCodec,
    on_progress: &(dyn Fn(f64) + Send + Sync),
    cancel: &CancellationToken,
) -> Result<PathBuf> {
    let target = cache.proxy_path(media, codec);
    if target.is_file() {
        touch(&target);
        on_progress(1.0);
        return Ok(target);
    }
    std::fs::create_dir_all(&cache.dir)?;
    let temp = target.with_extension(format!("{}.part", std::process::id()));
    let has_audio = media.streams.iter().any(|s| s.kind == StreamKind::Audio);
    let mut args: Vec<OsString> =
        ["-hide_banner", "-nostdin", "-y", "-loglevel", "warning"].map(OsString::from).to_vec();
    args.extend(
        ["-progress", "pipe:1", "-nostats", "-protocol_whitelist", INPUT_PROTOCOLS, "-i"].map(OsString::from),
    );
    args.push(file_arg(&media.path));
    args.extend(["-map", "0:v:0"].map(OsString::from));
    if has_audio {
        args.extend(["-map", "0:a:0"].map(OsString::from));
    }
    args.extend(
        ["-vf", "scale=-2:'min(480,ih)'", "-fps_mode", "passthrough", "-g", "15"].map(OsString::from),
    );
    let codec_args: &[&str] = match codec {
        ProxyCodec::H264 => &[
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-crf",
            "28",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "96k",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
        ],
        ProxyCodec::Vp8 => &[
            "-c:v",
            "libvpx",
            "-deadline",
            "realtime",
            "-cpu-used",
            "8",
            "-b:v",
            "1M",
            "-c:a",
            "libopus",
            "-b:a",
            "96k",
            "-f",
            "webm",
        ],
    };
    args.extend(codec_args.iter().map(OsString::from));
    args.push(file_arg(&temp));

    let total = media.duration.unwrap_or(Ts::ZERO).as_secs_f64();
    let mut parser = ProgressParser::default();
    let result = run_lines(&tools.ffmpeg.path, &args, cancel, |line| {
        if let Some(b) = parser.feed(line)
            && let Some(us) = b.out_time_us
            && total > 0.0
        {
            on_progress((us as f64 / 1e6 / total).clamp(0.0, 1.0));
        }
    })
    .await;
    match result {
        Ok(out) if out.status.success() => {
            std::fs::rename(&temp, &target)?;
            on_progress(1.0);
            cache.evict(Some(&target));
            Ok(target)
        }
        Ok(out) => {
            remove_with_retry(&temp);
            Err(Error::engine_failed("could not create a preview", out.stderr_tail))
        }
        Err(e) => {
            remove_with_retry(&temp);
            Err(e)
        }
    }
}

/// `n` evenly spaced thumbnails (JPEG, 90 px tall), cached. Returns file paths in time order.
pub async fn thumbnails(
    tools: &Tools,
    media: &MediaInfo,
    n: usize,
    cache: &PreviewCache,
    cancel: &CancellationToken,
) -> Result<Vec<PathBuf>> {
    let n = n.clamp(1, 60);
    let duration = media.duration.unwrap_or(Ts::ZERO);
    let key = PreviewCache::key(media);
    std::fs::create_dir_all(&cache.dir)?;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let path = cache.dir.join(format!("{key}-thumb{n}-{i}.jpg"));
        if !path.is_file() {
            let t = Ts::from_micros(duration.as_micros() * (2 * i as i64 + 1) / (2 * n as i64));
            let jpeg = frame_at(tools, media, t, 90, cancel).await?;
            std::fs::write(&path, jpeg)?;
        } else {
            touch(&path);
        }
        out.push(path);
    }
    cache.evict(None);
    Ok(out)
}

fn touch(path: &Path) {
    if let Ok(f) = std::fs::File::options().append(true).open(path) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
}
