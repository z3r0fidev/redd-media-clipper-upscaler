//! Facts read from outputs: packets, frames, decode errors, durations, sizes, GIF and MP4 structure.

use super::*;

/// A video packet of the first video stream (seconds, as ffprobe reports them).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Packet {
    pub pts: Option<f64>,
    pub dts: Option<f64>,
    pub key: bool,
}

/// The first video stream's packets in decode order (feature 004).
pub fn video_packets(path: &Path) -> Vec<Packet> {
    let out = Command::new(ffprobe())
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "packet=pts_time,dts_time,flags"])
        .args(["-of", "csv=p=0"])
        .arg(path)
        .output()
        .expect("run ffprobe");
    assert!(out.status.success(), "ffprobe failed on {}", path.display());
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut f = l.split(',');
            let (pts, dts, flags) = (f.next()?, f.next()?, f.next()?);
            Some(Packet { pts: pts.parse().ok(), dts: dts.parse().ok(), key: flags.starts_with('K') })
        })
        .collect()
}

/// The number of decoded video frames (`-count_frames`).
pub fn frame_count(path: &Path) -> u64 {
    let out = Command::new(ffprobe())
        .args(["-v", "error", "-count_frames", "-select_streams", "v:0"])
        .args(["-show_entries", "stream=nb_read_frames", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .expect("run ffprobe");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .and_then(|l| l.trim().parse().ok())
        .expect("frame count")
}

/// The error lines of a full decode of every stream (`-v error -f null -`); empty = clean.
pub fn decode_errors(path: &Path) -> Vec<String> {
    let out = Command::new(ffmpeg())
        .args(["-hide_banner", "-nostdin", "-v", "error", "-i"])
        .arg(path)
        .args(["-f", "null", "-"])
        .output()
        .expect("run ffmpeg");
    String::from_utf8_lossy(&out.stderr).lines().map(str::to_owned).collect()
}

/// MD5 of each of `frames` decoded video frames starting at `from` seconds (user time, exact
/// seek), without timestamps, so frames from different files compare directly.
pub fn framemd5(path: &Path, from: f64, frames: u32) -> Vec<String> {
    let out = Command::new(ffmpeg())
        .args(["-hide_banner", "-nostdin", "-v", "error", "-ss", &format!("{from}"), "-i"])
        .arg(path)
        .args(["-map", "0:v:0", "-frames:v", &frames.to_string(), "-f", "framemd5", "-"])
        .output()
        .expect("run ffmpeg");
    assert!(out.status.success(), "framemd5 failed on {}", path.display());
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.rsplit(',').next().map(|h| h.trim().to_owned()))
        .collect()
}

/// The start times (seconds) of the first video and the first audio stream.
pub fn stream_starts(path: &Path) -> (f64, f64) {
    let json = probe_json(path);
    let start = |kind: &str| {
        json["streams"]
            .as_array()
            .and_then(|s| s.iter().find(|s| s["codec_type"] == kind))
            .and_then(|s| s["start_time"].as_str())
            .and_then(|t| t.parse().ok())
            .unwrap_or_else(|| panic!("no {kind} start time in {}", path.display()))
    };
    (start("video"), start("audio"))
}

/// The first audio stream's start minus the first video stream's, in seconds (feature 005: in
/// sync within one AAC frame, 1024 samples).
pub fn audio_start_offset(path: &Path) -> f64 {
    let (video, audio) = stream_starts(path);
    audio - video
}

/// Runs ffprobe and returns its JSON (`-show_format -show_streams`).
pub fn probe_json(path: &Path) -> serde_json::Value {
    let out = Command::new(ffprobe())
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(path)
        .output()
        .expect("run ffprobe");
    assert!(out.status.success(), "ffprobe failed on {}", path.display());
    serde_json::from_slice(&out.stdout).expect("ffprobe json")
}

/// Container duration in seconds.
pub fn duration(path: &Path) -> f64 {
    probe_json(path)["format"]["duration"].as_str().and_then(|s| s.parse().ok()).expect("duration")
}

/// `codec_type` of each stream, in order.
pub fn stream_types(path: &Path) -> Vec<String> {
    probe_json(path)["streams"]
        .as_array()
        .expect("streams")
        .iter()
        .map(|s| s["codec_type"].as_str().unwrap_or("").to_owned())
        .collect()
}

pub fn sha256(path: &Path) -> String {
    let data = std::fs::read(path).expect("read for sha256");
    hex(&Sha256::digest(&data))
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Displayed size of the first video stream: coded size with SAR and display rotation applied.
pub fn display_size(path: &Path) -> (u32, u32) {
    let j = probe_json(path);
    let v =
        j["streams"].as_array().expect("streams").iter().find(|s| s["codec_type"] == "video").expect("video");
    let (w, h) = (v["width"].as_u64().expect("width") as f64, v["height"].as_u64().expect("height") as f64);
    let sar = v["sample_aspect_ratio"]
        .as_str()
        .and_then(|s| s.split_once(':'))
        .and_then(|(a, b)| Some((a.parse::<f64>().ok()?, b.parse::<f64>().ok()?)))
        .filter(|(a, b)| *a > 0.0 && *b > 0.0)
        .map_or(1.0, |(a, b)| a / b);
    let rotation = v["side_data_list"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|d| d["rotation"].as_f64())
        .unwrap_or(0.0);
    let (w, h) = ((w * sar).round() as u32, h as u32);
    if (rotation.abs() as i64) % 180 == 90 { (h, w) } else { (w, h) }
}

/// ffprobe `packet=<entry>` values of the first video stream, in order.
pub(super) fn packet_entries(path: &Path, entry: &str) -> Vec<u64> {
    let out = Command::new(ffprobe())
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", &format!("packet={entry}")])
        .args(["-of", "csv=p=0"])
        .arg(path)
        .output()
        .expect("run ffprobe");
    assert!(out.status.success(), "ffprobe failed on {}", path.display());
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().trim_end_matches(',').parse().ok())
        .collect()
}

/// A GIF's frame delays in centiseconds (packet durations; GIF's time base is 1/100).
pub fn gif_delays(path: &Path) -> Vec<u32> {
    packet_entries(path, "duration").into_iter().map(|d| d as u32).collect()
}

/// A GIF's packet sizes in bytes (frame 0 is the full first frame, the rest are deltas).
pub fn gif_packet_sizes(path: &Path) -> Vec<u64> {
    packet_entries(path, "size")
}

/// The NETSCAPE2.0 repeat count of a GIF (`Some(0)` = forever), `None` when the block is absent
/// (the GIF plays once).
pub fn gif_loop(path: &Path) -> Option<u16> {
    let data = std::fs::read(path).expect("read gif");
    netscape_repeat(&data)
}

pub(super) fn netscape_repeat(data: &[u8]) -> Option<u16> {
    const ID: &[u8] = b"NETSCAPE2.0";
    let at = data.windows(ID.len()).position(|w| w == ID)?;
    // Application extension: id, then a 3-byte sub-block [len=3, 1, repeat lo, repeat hi].
    match data.get(at + ID.len()..at + ID.len() + 4)? {
        [3, 1, lo, hi] => Some(u16::from_le_bytes([*lo, *hi])),
        _ => None,
    }
}

/// Whether the first frame of `path`, decoded to RGBA, has no pixel with alpha below 255.
pub fn gif_opaque(path: &Path) -> bool {
    let out = Command::new(ffmpeg())
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .output()
        .expect("decode first frame");
    assert!(out.status.success() && !out.stdout.is_empty(), "cannot decode {}", path.display());
    out.stdout.chunks_exact(4).all(|px| px[3] == 255)
}

/// Whether an MP4's `moov` box comes before its `mdat` box (playable while downloading).
pub fn mp4_faststart(path: &Path) -> bool {
    let data = std::fs::read(path).expect("read mp4");
    let order = top_level_boxes(&data);
    let pos = |name: &[u8; 4]| order.iter().position(|b| b == name);
    matches!((pos(b"moov"), pos(b"mdat")), (Some(m), Some(d)) if m < d)
}

pub(super) fn top_level_boxes(data: &[u8]) -> Vec<[u8; 4]> {
    let mut boxes = Vec::new();
    let mut at = 0usize;
    while at + 8 <= data.len() {
        let size = u32::from_be_bytes(data[at..at + 4].try_into().expect("4 bytes")) as u64;
        let kind: [u8; 4] = data[at + 4..at + 8].try_into().expect("4 bytes");
        let size = match size {
            1 if at + 16 <= data.len() => {
                u64::from_be_bytes(data[at + 8..at + 16].try_into().expect("8 bytes"))
            }
            0 => (data.len() - at) as u64,
            s => s,
        };
        boxes.push(kind);
        if size < 8 {
            break;
        }
        at = at.saturating_add(size as usize);
    }
    boxes
}
