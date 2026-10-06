//! Directory discovery (US4, research.md R6): walk, filter by extension, confirm with ffprobe.
//!
//! "Video archive" means a collection of video files; compressed archives are never opened.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use globset::{GlobSet, GlobSetBuilder};
use serde::Serialize;
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

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, deny_unknown_fields)]
pub struct ScanOptions {
    /// Only look at the top level of each root (default: descend into sub-folders).
    pub no_recursive: bool,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub max_depth: Option<usize>,
    /// Globs matched against the path relative to the root, e.g. `*.mp4`; `**` spans folders.
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub hidden: bool,
    pub follow_symlinks: bool,
    /// Folders never descended into, compared as real paths (not globs).
    #[cfg_attr(feature = "ts", ts(skip))]
    #[serde(skip)]
    pub exclude_dirs: Vec<PathBuf>,
    /// Folder names never descended into (exact, case-insensitive), e.g. `clips`.
    pub exclude_dir_names: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Rejected {
    #[serde(serialize_with = "crate::pathser::path")]
    pub path: PathBuf,
    pub reason: RejectReason,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Skipped {
    pub hidden: usize,
    pub temp: usize,
    pub symlink: usize,
    pub excluded: usize,
    /// Online-only cloud files (OneDrive etc.); reading them would download them.
    pub cloud: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ScanResult {
    #[serde(serialize_with = "crate::pathser::paths")]
    pub roots: Vec<PathBuf>,
    pub videos: Vec<MediaInfo>,
    pub rejected: Vec<Rejected>,
    pub skipped: Skipped,
    /// Folders skipped because of `ScanOptions::exclude_dir_names`.
    #[serde(serialize_with = "crate::pathser::paths")]
    pub excluded_named: Vec<PathBuf>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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
    let Walk { candidates, mut rejected, skipped, visited, excluded_named } = walk;
    on_event(ScanEvent::Progress { visited, candidates: candidates.len(), probed: 0 });

    // Probe with a fixed-size pool: at most `workers` tasks exist at any time, however large the
    // tree. Probing spawns ffprobe from runtime workers (PDEATHSIG-safe).
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).min(16);
    let tools = Arc::new(tools.clone());
    let mut set: JoinSet<(PathBuf, Result<MediaInfo>)> = JoinSet::new();
    let mut in_flight: std::collections::HashMap<tokio::task::Id, PathBuf> = std::collections::HashMap::new();
    let mut pending = candidates.iter();
    let mut videos = Vec::new();
    let mut probed = 0;
    loop {
        while set.len() < workers {
            let Some(path) = pending.next() else { break };
            // The file may have been swapped for a symlink since the walk; never follow it then.
            if !opts.follow_symlinks
                && std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
            {
                rejected.push(Rejected {
                    path: path.clone(),
                    reason: RejectReason::Inaccessible,
                    detail: "replaced by a symbolic link during the scan".into(),
                });
                probed += 1;
                continue;
            }
            let (tools, cancel, p) = (tools.clone(), cancel.clone(), path.clone());
            let handle = set.spawn(async move {
                let r = probe::probe(&tools, &p, &cancel).await;
                (p, r)
            });
            in_flight.insert(handle.id(), path.clone());
        }
        let next = tokio::select! {
            n = set.join_next_with_id() => n,
            () = cancel.cancelled() => {
                set.abort_all(); // dropping probe futures kills their ffprobe processes
                return Err(Error::Cancelled);
            }
        };
        let Some(joined) = next else { break };
        probed += 1;
        let outcome = match joined {
            Ok((id, (path, result))) => {
                in_flight.remove(&id);
                classify(path, result)
            }
            Err(e) => {
                let path = in_flight.remove(&e.id()).unwrap_or_default();
                Err(Some(Rejected {
                    path,
                    reason: RejectReason::Unreadable,
                    detail: format!("internal error: {e}"),
                }))
            }
        };
        match outcome {
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
        excluded_named,
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
    excluded_named: Vec<PathBuf>,
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
    let mut out = Walk {
        excluded_named: Vec::new(),
        candidates: Vec::new(),
        rejected: Vec::new(),
        skipped: Skipped::default(),
        visited: 0,
    };
    let mut seen = std::collections::HashSet::new();
    let excluded_dirs: Vec<std::ffi::OsString> = opts
        .exclude_dirs
        .iter()
        .map(|d| path_key(&std::fs::canonicalize(d).unwrap_or_else(|_| d.clone())))
        .collect();
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
                let by_name = opts.exclude_dir_names.iter().any(|n| n.eq_ignore_ascii_case(&name));
                if by_name {
                    out.excluded_named.push(entry.path().to_path_buf());
                }
                let skip_excluded = exclude.is_some_and(|g| g.is_match(rel) || g.is_match(rel.join("x")))
                    || by_name
                    || (!excluded_dirs.is_empty()
                        && excluded_dirs.contains(&path_key(
                            &std::fs::canonicalize(entry.path())
                                .unwrap_or_else(|_| entry.path().to_path_buf()),
                        )));
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
            if is_cloud_placeholder(&entry) {
                out.skipped.cloud += 1;
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
            // A file given explicitly as a root is always considered (like the extension check).
            if !is_root
                && (include.is_some_and(|g| !g.is_match(rel)) || exclude.is_some_and(|g| g.is_match(rel)))
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

/// Identity key for a path: exact bytes on Unix (non-UTF-8 names stay distinct), case-insensitive
/// on Windows.
pub fn path_key(p: &Path) -> std::ffi::OsString {
    if cfg!(windows) { p.to_string_lossy().to_lowercase().into() } else { p.as_os_str().to_owned() }
}

/// Partial downloads and editor/OS temp files, including rmedia's own `.part` outputs.
fn is_temp_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".part", ".crdownload", ".tmp", ".ytdl", ".download", ".!qb", ".partial"]
        .iter()
        .any(|s| lower.ends_with(s))
        || name.starts_with("._")
}

/// Windows cloud-sync placeholders (recall-on-access/open, offline). Always false elsewhere.
fn is_cloud_placeholder(entry: &walkdir::DirEntry) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const OFFLINE: u32 = 0x1000;
        const RECALL_ON_OPEN: u32 = 0x4_0000;
        const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
        if let Ok(m) = entry.metadata() {
            return m.file_attributes() & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0;
        }
    }
    let _ = entry;
    false
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
