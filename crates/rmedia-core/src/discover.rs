//! Directory discovery (US4, research.md R6): walk, filter by extension, confirm with ffprobe.
//!
//! "Video archive" means a collection of video files; compressed archives are never opened.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use globset::{GlobSet, GlobSetBuilder};
use serde::Serialize;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::probe::{self, MediaInfo};
use crate::tools::Tools;

/// Case-insensitive extensions considered video candidates.
pub const VIDEO_EXTENSIONS: [&str; 22] = [
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "divx", "wmv", "asf", "flv", "f4v", "mpg", "mpeg", "m2ts",
    "mts", "ts", "vob", "3gp", "3g2", "ogv", "mxf", "qt",
];

/// Folders never descended into (system, trash, thumbnail and VCS folders).
const SKIP_DIRS: [&str; 7] =
    ["$RECYCLE.BIN", "System Volume Information", "@eaDir", ".git", ".Trashes", ".thumbnails", "lost+found"];

#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    /// Descend into sub-folders (default true).
    pub no_recursive: bool,
    pub max_depth: Option<usize>,
    /// Globs matched against the path relative to the root (e.g. `*.mp4`, `**/2024/**`).
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub hidden: bool,
    pub follow_symlinks: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// ffprobe could not read it (corrupt, truncated, not media).
    Unreadable,
    /// Media without a real video stream (audio, cover art only).
    NoVideoStream,
    /// A still image with a video extension.
    Image,
    /// The file or folder could not be accessed.
    Inaccessible,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rejected {
    pub path: PathBuf,
    pub reason: RejectReason,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Skipped {
    pub hidden: usize,
    pub temp: usize,
    pub symlink: usize,
    pub excluded: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScanResult {
    pub roots: Vec<PathBuf>,
    pub videos: Vec<MediaInfo>,
    pub rejected: Vec<Rejected>,
    pub skipped: Skipped,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScanEvent {
    Progress { visited: usize, candidates: usize, probed: usize },
    Found { video: MediaInfo },
    Rejected { rejected: Rejected },
}

/// Scans roots (folders or files). Emits events as it goes; cancellable.
pub async fn scan(
    tools: &Tools,
    roots: &[PathBuf],
    opts: &ScanOptions,
    on_event: &(dyn Fn(ScanEvent) + Send + Sync),
    cancel: &CancellationToken,
) -> Result<ScanResult> {
    let started = Instant::now();
    let include = build_globs(&opts.include)?;
    let exclude = build_globs(&opts.exclude)?;

    // Walking is blocking I/O: do it off the async workers (no processes are spawned there).
    let walk = {
        let (roots, opts, cancel) = (roots.to_vec(), opts.clone(), cancel.clone());
        tokio::task::spawn_blocking(move || walk(&roots, &opts, include.as_ref(), exclude.as_ref(), &cancel))
            .await
            .map_err(|e| Error::Io(std::io::Error::other(e)))?
    };
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let Walk { candidates, mut rejected, skipped, visited } = walk;
    on_event(ScanEvent::Progress { visited, candidates: candidates.len(), probed: 0 });

    // Probe in parallel (bounded). Probing spawns ffprobe from runtime workers (PDEATHSIG-safe).
    let limit = Arc::new(Semaphore::new(std::thread::available_parallelism().map_or(4, |n| n.get()).min(16)));
    let tools = Arc::new(tools.clone());
    let mut set = JoinSet::new();
    for path in candidates.iter().cloned() {
        let (limit, tools, cancel) = (limit.clone(), tools.clone(), cancel.clone());
        set.spawn(async move {
            let _permit = limit.acquire_owned().await.expect("semaphore open");
            let r = probe::probe(&tools, &path, &cancel).await;
            (path, r)
        });
    }
    let mut videos = Vec::new();
    let mut probed = 0;
    loop {
        let next = tokio::select! {
            n = set.join_next() => n,
            () = cancel.cancelled() => {
                set.abort_all(); // dropping probe futures kills their ffprobe processes
                return Err(Error::Cancelled);
            }
        };
        let Some(joined) = next else { break };
        let Ok((path, result)) = joined else { continue };
        probed += 1;
        match classify(path, result) {
            Ok(v) => {
                on_event(ScanEvent::Found { video: v.clone() });
                videos.push(v);
            }
            Err(Some(r)) => {
                on_event(ScanEvent::Rejected { rejected: r.clone() });
                rejected.push(r);
            }
            Err(None) => return Err(Error::Cancelled),
        }
        if probed % 25 == 0 {
            on_event(ScanEvent::Progress { visited, candidates: candidates.len(), probed });
        }
    }
    on_event(ScanEvent::Progress { visited, candidates: candidates.len(), probed });

    videos.sort_by(|a, b| natural_cmp(&a.path, &b.path));
    rejected.sort_by(|a, b| natural_cmp(&a.path, &b.path));
    Ok(ScanResult {
        roots: roots.to_vec(),
        videos,
        rejected,
        skipped,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

/// `Ok(video)`, `Err(Some(rejection))`, or `Err(None)` when cancelled.
fn classify(path: PathBuf, result: Result<MediaInfo>) -> std::result::Result<MediaInfo, Option<Rejected>> {
    match result {
        Ok(m) if is_image_format(&m.format_name) => {
            Err(Some(Rejected { path, reason: RejectReason::Image, detail: m.format_name }))
        }
        Ok(m) if m.is_video() => Ok(m),
        Ok(_) => Err(Some(Rejected {
            path,
            reason: RejectReason::NoVideoStream,
            detail: "no video stream (audio or cover art only)".into(),
        })),
        Err(Error::Cancelled) => Err(None),
        Err(e) => Err(Some(Rejected { path, reason: RejectReason::Unreadable, detail: e.to_string() })),
    }
}

fn is_image_format(format_name: &str) -> bool {
    format_name.split(',').any(|f| f == "image2" || f.ends_with("_pipe"))
}

struct Walk {
    candidates: Vec<PathBuf>,
    rejected: Vec<Rejected>,
    skipped: Skipped,
    visited: usize,
}

fn walk(
    roots: &[PathBuf],
    opts: &ScanOptions,
    include: Option<&GlobSet>,
    exclude: Option<&GlobSet>,
    cancel: &CancellationToken,
) -> Walk {
    let mut out =
        Walk { candidates: Vec::new(), rejected: Vec::new(), skipped: Skipped::default(), visited: 0 };
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        let depth = if opts.no_recursive { Some(1) } else { opts.max_depth };
        let mut walker = walkdir::WalkDir::new(root).follow_links(opts.follow_symlinks);
        if let Some(d) = depth {
            walker = walker.max_depth(d);
        }
        let mut it = walker.into_iter();
        while let Some(entry) = it.next() {
            if cancel.is_cancelled() {
                return out;
            }
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    let path = e.path().map(Path::to_path_buf).unwrap_or_else(|| root.clone());
                    // Symlink loops (when following) and permission errors land here.
                    out.rejected.push(Rejected {
                        path,
                        reason: RejectReason::Inaccessible,
                        detail: e.to_string(),
                    });
                    continue;
                }
            };
            out.visited += 1;
            let is_root = entry.depth() == 0;
            let name = entry.file_name().to_string_lossy();
            let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
            let ft = entry.file_type();

            if !is_root && ft.is_symlink() {
                out.skipped.symlink += 1;
                continue;
            }
            if ft.is_dir() {
                if is_root {
                    continue;
                }
                let skip_hidden = !opts.hidden && is_hidden(&entry);
                let skip_system =
                    SKIP_DIRS.iter().any(|d| d.eq_ignore_ascii_case(&name)) || name.starts_with(".Trash-");
                let skip_excluded = exclude.is_some_and(|g| g.is_match(rel) || g.is_match(rel.join("x")));
                if skip_hidden || skip_system || skip_excluded {
                    if skip_excluded {
                        out.skipped.excluded += 1;
                    } else {
                        out.skipped.hidden += 1;
                    }
                    it.skip_current_dir();
                }
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            if !is_root {
                if is_temp_name(&name) {
                    out.skipped.temp += 1;
                    continue;
                }
                if !opts.hidden && is_hidden(&entry) {
                    out.skipped.hidden += 1;
                    continue;
                }
            }
            let ext = entry
                .path()
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if !is_root && !VIDEO_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }
            let rel_for_globs = if is_root { Path::new(entry.file_name()) } else { rel };
            if include.is_some_and(|g| !g.is_match(rel_for_globs))
                || exclude.is_some_and(|g| g.is_match(rel_for_globs))
            {
                out.skipped.excluded += 1;
                continue;
            }
            let key = std::fs::canonicalize(entry.path()).unwrap_or_else(|_| entry.path().to_path_buf());
            if seen.insert(path_key(&key)) {
                out.candidates.push(entry.path().to_path_buf());
            }
        }
    }
    out
}

fn path_key(p: &Path) -> String {
    let s = p.to_string_lossy();
    if cfg!(windows) { s.to_lowercase() } else { s.into_owned() }
}

/// Partial downloads and editor/OS temp files, including rmedia's own `.part` outputs.
fn is_temp_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".part", ".crdownload", ".tmp", ".ytdl", ".download", ".!qb", ".partial"]
        .iter()
        .any(|s| lower.ends_with(s))
        || name.starts_with("._")
}

fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    if entry.file_name().to_string_lossy().starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        if let Ok(m) = entry.metadata() {
            return m.file_attributes() & (HIDDEN | SYSTEM) != 0;
        }
    }
    false
}

fn build_globs(patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        let glob = globset::GlobBuilder::new(p)
            .case_insensitive(true)
            .build()
            .map_err(|e| Error::InvalidInput(format!("invalid pattern \"{p}\": {e}")))?;
        b.add(glob);
    }
    b.build().map(Some).map_err(|e| Error::InvalidInput(e.to_string()))
}

/// Natural ordering: "clip2" < "clip10".
pub fn natural_cmp(a: &Path, b: &Path) -> Ordering {
    let (a, b) = (a.to_string_lossy().to_lowercase(), b.to_string_lossy().to_lowercase());
    let (mut x, mut y) = (a.as_str(), b.as_str());
    loop {
        match (x.chars().next(), y.chars().next()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let nx = x.find(|c: char| !c.is_ascii_digit()).unwrap_or(x.len());
                let ny = y.find(|c: char| !c.is_ascii_digit()).unwrap_or(y.len());
                let (dx, dy) = (x[..nx].trim_start_matches('0'), y[..ny].trim_start_matches('0'));
                let o = dx.len().cmp(&dy.len()).then_with(|| dx.cmp(dy));
                if o != Ordering::Equal {
                    return o;
                }
                x = &x[nx..];
                y = &y[ny..];
            }
            (Some(c), Some(d)) => {
                if c != d {
                    return c.cmp(&d);
                }
                x = &x[c.len_utf8()..];
                y = &y[d.len_utf8()..];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_sort() {
        let mut v: Vec<PathBuf> =
            ["clip10.mp4", "Clip2.mp4", "clip1.mp4", "a/clip002.mp4"].iter().map(PathBuf::from).collect();
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["a/clip002.mp4", "clip1.mp4", "Clip2.mp4", "clip10.mp4"].map(PathBuf::from));
    }

    #[test]
    fn temp_names() {
        for n in ["a.mp4.part", ".x.rmedia-1-1.part", "dl.crdownload", "._a.mp4", "v.mp4.!qB", "a.ytdl"] {
            assert!(is_temp_name(n), "{n}");
        }
        assert!(!is_temp_name("party.mp4"));
    }

    #[test]
    fn image_formats() {
        assert!(is_image_format("image2"));
        assert!(is_image_format("jpeg_pipe"));
        assert!(!is_image_format("mov,mp4,m4a,3gp,3g2,mj2"));
    }

    #[test]
    fn bad_globs_are_input_errors() {
        assert!(matches!(build_globs(&["a[".into()]), Err(Error::InvalidInput(_))));
    }
}
