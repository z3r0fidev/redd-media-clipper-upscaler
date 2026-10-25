//! Checks of a finished GIF or Reddit MP4 before commit (research R11, FR-018, SC-001).
//!
//! The facts are read from the file (header, NETSCAPE block, MP4 box order) and from ffprobe
//! (packet durations, stream sizes); the checks are pure, so they are unit-tested on facts.

use std::ffi::OsString;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use tokio_util::sync::CancellationToken;

use super::LoopCount;
use super::timing::MIN_DELAY_CS;
use crate::error::{Error, Result};
use crate::probe::{MediaInfo, StreamKind, file_arg};
use crate::time::Ts;
use crate::tools::Tools;

/// How far into a GIF the NETSCAPE2.0 block is looked for: it follows the header and the
/// global colour table (at most 781 bytes), before the first frame.
const NETSCAPE_WINDOW: u64 = 4096;

/// What a GIF must be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GifExpect {
    pub width: u32,
    pub height: u32,
    /// Planned frame count (±1 allowed).
    pub frames: u64,
    /// Set when the engine may merge frames or round delays its own way (gifsicle `-O2` merges
    /// identical consecutive frames and adds up their delays; gifski times frames itself): fewer
    /// frames are allowed, and this planned display time (centiseconds) is checked instead,
    /// within one frame.
    pub length_cs: Option<u64>,
    pub loop_count: LoopCount,
    /// The hard size limit, when there is a budget.
    pub max_bytes: Option<u64>,
}

/// What a GIF is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GifFacts {
    pub gif89a: bool,
    /// The NETSCAPE2.0 repeat count (`None`: no block).
    pub netscape: Option<u16>,
    /// Frame delays in centiseconds.
    pub delays: Vec<u32>,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}

/// Checks `facts` against `expect`.
pub fn check_gif(f: &GifFacts, e: &GifExpect) -> std::result::Result<(), String> {
    if !f.gif89a {
        return Err("the output is not a GIF89a file".into());
    }
    if (f.width, f.height) != (e.width, e.height) {
        return Err(format!("the GIF is {}x{}, expected {}x{}", f.width, f.height, e.width, e.height));
    }
    if let Some(d) = f.delays.iter().find(|d| **d < MIN_DELAY_CS) {
        return Err(format!("a frame delay is {d} cs, under the {MIN_DELAY_CS} cs minimum"));
    }
    let frames = f.delays.len() as u64;
    match e.length_cs {
        None if frames.abs_diff(e.frames) > 1 => {
            return Err(format!("the GIF has {frames} frames, expected {}", e.frames));
        }
        None => {}
        Some(_) if frames > e.frames + 1 => {
            return Err(format!("the GIF has {frames} frames, expected at most {}", e.frames + 1));
        }
        Some(planned) => {
            let shown: u64 = f.delays.iter().map(|&d| u64::from(d)).sum();
            if shown.abs_diff(planned) > planned.div_ceil(e.frames.max(1)) + 1 {
                return Err(format!("the GIF plays for {shown} cs, expected {planned} cs"));
            }
        }
    }
    if f.netscape != e.loop_count.netscape() {
        let shown = |n: Option<u16>| {
            n.map_or_else(|| "no loop block (plays once)".to_owned(), |r| format!("repeat {r}"))
        };
        return Err(format!(
            "the GIF has {}, expected {}",
            shown(f.netscape),
            shown(e.loop_count.netscape())
        ));
    }
    if let Some(max) = e.max_bytes
        && f.bytes > max
    {
        return Err(format!("the GIF is {} bytes, over the {max}-byte limit", f.bytes));
    }
    Ok(())
}

/// Reads the facts of the GIF at `path`.
pub async fn gif_facts(tools: &Tools, path: &Path, cancel: &CancellationToken) -> Result<GifFacts> {
    let (gif89a, netscape, bytes) = {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || -> std::io::Result<(bool, Option<u16>, u64)> {
            let mut file = std::fs::File::open(&path)?;
            let bytes = file.metadata()?.len();
            let mut head = Vec::new();
            (&mut file).take(NETSCAPE_WINDOW).read_to_end(&mut head)?;
            Ok((head.starts_with(b"GIF89a"), netscape_repeat(&head), bytes))
        })
        .await
        .map_err(|e| Error::VerifyFailed(e.to_string()))??
    };
    let probe =
        crate::probe::probe(tools, path, cancel).await.map_err(|e| Error::VerifyFailed(e.to_string()))?;
    let v =
        probe.first_video().ok_or_else(|| Error::VerifyFailed("the output has no video stream".into()))?;
    let args: Vec<OsString> =
        ["-v", "error", "-select_streams", "v:0", "-show_entries", "packet=duration", "-of", "csv=p=0"]
            .into_iter()
            .map(OsString::from)
            .chain([file_arg(path)])
            .collect();
    let out = crate::exec::process::run_capture(&tools.ffprobe.path, &args, cancel).await?;
    if !out.status.success() {
        return Err(Error::VerifyFailed(format!("cannot read the GIF's frames: {}", out.stderr_tail.trim())));
    }
    let delays = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().trim_end_matches(',').parse::<u32>().ok())
        .collect();
    Ok(GifFacts {
        gif89a,
        netscape,
        delays,
        width: v.width.unwrap_or(0),
        height: v.height.unwrap_or(0),
        bytes,
    })
}

/// The repeat count of the first NETSCAPE2.0 application extension in `head`.
pub fn netscape_repeat(head: &[u8]) -> Option<u16> {
    const ID: &[u8] = b"NETSCAPE2.0";
    let at = head.windows(ID.len()).position(|w| w == ID)?;
    match head.get(at + ID.len()..at + ID.len() + 4)? {
        [3, 1, lo, hi] => Some(u16::from_le_bytes([*lo, *hi])),
        _ => None,
    }
}

/// What a Reddit MP4 must be.
#[derive(Debug, Clone, PartialEq)]
pub struct Mp4Expect {
    pub width: u32,
    pub height: u32,
    pub duration: Ts,
    /// One frame at the output rate (the duration tolerance).
    pub frame: Ts,
    pub max_bytes: Option<u64>,
}

/// Checks a probed MP4 (`faststart`: `moov` before `mdat`).
pub fn check_mp4(info: &MediaInfo, faststart: bool, e: &Mp4Expect) -> std::result::Result<(), String> {
    let others: Vec<&str> = info
        .streams
        .iter()
        .filter(|s| s.kind != StreamKind::Video || s.is_attached_pic)
        .map(|s| s.codec.as_str())
        .collect();
    if !others.is_empty() {
        return Err(format!("the MP4 has streams besides its video: {}", others.join(", ")));
    }
    let v = info.first_video().ok_or("the MP4 has no video stream")?;
    if v.codec != "h264" {
        return Err(format!("the MP4's video is {}, expected h264", v.codec));
    }
    if (v.width, v.height) != (Some(e.width), Some(e.height)) {
        return Err(format!("the MP4 is {:?}x{:?}, expected {}x{}", v.width, v.height, e.width, e.height));
    }
    if !faststart {
        return Err("the MP4 is not fast-start (moov after mdat)".into());
    }
    if let Some(got) = v.duration.or(info.duration) {
        let diff = if got > e.duration { got - e.duration } else { e.duration - got };
        if diff > e.frame + Ts::from_micros(20_000) {
            return Err(format!(
                "the MP4 lasts {:.3} s, expected {:.3} s",
                got.as_secs_f64(),
                e.duration.as_secs_f64()
            ));
        }
    }
    if let Some(max) = e.max_bytes
        && info.size > max
    {
        return Err(format!("the MP4 is {} bytes, over the {max}-byte limit", info.size));
    }
    Ok(())
}

/// Whether the file's top-level `moov` box comes before its `mdat` box. Only box headers are
/// read.
pub fn mp4_faststart(path: &Path) -> std::io::Result<bool> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let mut at = 0u64;
    while at + 8 <= len {
        f.seek(SeekFrom::Start(at))?;
        let mut head = [0u8; 16];
        let n = f.read(&mut head)?;
        if n < 8 {
            break;
        }
        let kind = &head[4..8];
        let size = match u32::from_be_bytes(head[..4].try_into().expect("4 bytes")) {
            1 if n >= 16 => u64::from_be_bytes(head[8..16].try_into().expect("8 bytes")),
            0 => len - at,
            s => u64::from(s),
        };
        match kind {
            b"moov" => return Ok(true),
            b"mdat" => return Ok(false),
            _ => {}
        }
        if size < 8 {
            break;
        }
        at = at.saturating_add(size);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect() -> GifExpect {
        GifExpect {
            width: 720,
            height: 405,
            frames: 90,
            length_cs: None,
            loop_count: LoopCount::Forever,
            max_bytes: Some(8_000_000),
        }
    }

    fn facts() -> GifFacts {
        GifFacts {
            gif89a: true,
            netscape: Some(0),
            delays: crate::gif::timing::delays((15, 1), 90),
            width: 720,
            height: 405,
            bytes: 6_000_000,
        }
    }

    #[test]
    fn a_good_gif_passes() {
        assert_eq!(check_gif(&facts(), &expect()), Ok(()));
        let one_more = GifFacts { delays: crate::gif::timing::delays((15, 1), 91), ..facts() };
        assert_eq!(check_gif(&one_more, &expect()), Ok(()), "±1 frame");
        assert_eq!(check_gif(&facts(), &GifExpect { max_bytes: None, ..expect() }), Ok(()));
    }

    #[test]
    fn each_rule_fails_with_its_reason() {
        let bad = |f: GifFacts, e: GifExpect, needle: &str| {
            let err = check_gif(&f, &e).unwrap_err();
            assert!(err.contains(needle), "{err} (wanted {needle})");
        };
        bad(GifFacts { gif89a: false, ..facts() }, expect(), "GIF89a");
        bad(GifFacts { width: 718, ..facts() }, expect(), "718x405");
        let mut fast = facts();
        fast.delays[10] = 1;
        bad(fast, expect(), "1 cs");
        bad(GifFacts { delays: vec![7; 88], ..facts() }, expect(), "88 frames");
        bad(facts(), GifExpect { loop_count: LoopCount::Plays(1), ..expect() }, "plays once");
        bad(GifFacts { netscape: None, ..facts() }, expect(), "repeat 0");
        bad(facts(), GifExpect { loop_count: LoopCount::Plays(3), ..expect() }, "repeat 2");
        bad(GifFacts { bytes: 8_000_001, ..facts() }, expect(), "over the 8000000-byte limit");
    }

    #[test]
    fn a_merged_gif_may_lose_frames_but_not_change_its_length() {
        let planned: u64 = facts().delays.iter().map(|&d| u64::from(d)).sum();
        let opt = GifExpect { length_cs: Some(planned), ..expect() };
        let merged = GifFacts { delays: vec![200, 200, planned as u32 - 400], ..facts() };
        assert_eq!(check_gif(&merged, &opt), Ok(()), "3 merged frames, same length");
        assert_eq!(check_gif(&facts(), &opt), Ok(()));
        let short = GifFacts { delays: vec![200, 200], ..facts() };
        assert!(check_gif(&short, &opt).unwrap_err().contains("plays for 400 cs"));
        let more = GifFacts { delays: crate::gif::timing::delays((15, 1), 92), ..facts() };
        assert!(check_gif(&more, &opt).unwrap_err().contains("at most 91"));
        assert!(check_gif(&merged, &expect()).unwrap_err().contains("3 frames"), "built-in: exact count");
    }

    #[test]
    fn netscape_blocks_are_read() {
        let mut gif = b"GIF89a....!\xff\x0bNETSCAPE2.0".to_vec();
        gif.extend([3, 1, 2, 0, 0]);
        assert_eq!(netscape_repeat(&gif), Some(2));
        assert_eq!(netscape_repeat(b"GIF89a"), None);
    }

    fn mp4(boxes: &[(&[u8; 4], u32)]) -> tempfile::NamedTempFile {
        let mut data = Vec::new();
        for (kind, len) in boxes {
            let start = data.len();
            data.extend(len.to_be_bytes());
            data.extend(*kind);
            data.resize(start + *len as usize, 0);
        }
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(f.path(), data).unwrap();
        f
    }

    #[test]
    fn faststart_reads_only_box_headers() {
        assert!(mp4_faststart(mp4(&[(b"ftyp", 24), (b"moov", 100), (b"mdat", 1000)]).path()).unwrap());
        assert!(!mp4_faststart(mp4(&[(b"ftyp", 24), (b"mdat", 1000), (b"moov", 100)]).path()).unwrap());
        assert!(!mp4_faststart(mp4(&[(b"ftyp", 24)]).path()).unwrap());
    }
}
