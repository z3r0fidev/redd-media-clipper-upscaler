//! Desktop operations: thin adapters over rmedia-core (Constitution I), testable without a webview.
//! Tauri commands in `commands.rs` only unwrap arguments and call these.
//!
//! Path policy (research.md §R8 #3/#11): the webview may only name files the user chose — through
//! the native dialogs, drag-and-drop, a picked folder (and what scanning it found), or a manifest
//! the user opened. Anything else is refused, so a compromised page cannot read arbitrary files.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use rmedia_core::discover::{self, ScanEvent, ScanOptions, ScanResult};
use rmedia_core::doctor::{self, DoctorReport};
use rmedia_core::event::{EventSink, RunSummary};
use rmedia_core::exec::{self, RunConfig, RunControl};
use rmedia_core::input::{cliplist, manifest_csv, manifest_json};
use rmedia_core::plan::{self, Caps, Issue, Plan};
use rmedia_core::preview::{self, PreviewCache, ProxyCodec};
use rmedia_core::probe::{self, MediaInfo};
use rmedia_core::request::{BatchRequest, Options, PartialOptions};
use rmedia_core::settings::Settings;
use rmedia_core::tools::{LocateOptions, Tools};
use rmedia_core::{EndSpec, Ts};
use serde::Serialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::media_server::MediaServer;

pub type OpResult<T> = Result<T, String>;

/// Paths the user has chosen.
#[derive(Debug, Default)]
pub struct Allowed {
    files: HashSet<OsString>,
    dirs: Vec<PathBuf>,
    save_targets: HashSet<OsString>,
    /// Clip lists and manifests picked as such (only these may be parsed).
    lists: HashSet<OsString>,
}

impl Allowed {
    fn key(p: &Path) -> Option<(PathBuf, OsString)> {
        let c = std::fs::canonicalize(p).ok()?;
        let k = rmedia_core::discover::path_key(&c);
        Some((c, k))
    }

    pub fn allow(&mut self, p: &Path) {
        if let Some((c, k)) = Self::key(p) {
            if c.is_dir() {
                self.dirs.push(c);
            } else {
                self.files.insert(k);
            }
        }
    }

    pub fn allow_list(&mut self, p: &Path) {
        if let Some((_, k)) = Self::key(p) {
            self.lists.insert(k);
        }
    }

    fn is_list(&self, p: &Path) -> bool {
        Self::key(p).is_some_and(|(_, k)| self.lists.contains(&k))
    }

    /// Where a clip may be written: inside a folder the user picked, or next to its source
    /// (the default `clips/` folder and template sub-folders).
    fn may_write(&self, output: &Path, source: &Path) -> bool {
        let resolved = resolve_existing(output);
        let source_dir = source.parent().and_then(|d| std::fs::canonicalize(d).ok());
        self.dirs.iter().any(|d| resolved.starts_with(d))
            || source_dir.is_some_and(|d| resolved.starts_with(d))
    }

    pub fn allow_save(&mut self, p: &Path) {
        self.save_targets.insert(rmedia_core::discover::path_key(p));
    }

    pub fn is_allowed(&self, p: &Path) -> bool {
        let Some((c, k)) = Self::key(p) else { return false };
        self.files.contains(&k) || self.dirs.iter().any(|d| c.starts_with(d))
    }

    fn is_save_target(&self, p: &Path) -> bool {
        self.save_targets.contains(&rmedia_core::discover::path_key(p))
    }
}

pub struct AppState {
    pub settings: Mutex<Settings>,
    engine: RwLock<Option<(Tools, Caps)>>,
    pub allowed: Mutex<Allowed>,
    pub media: MediaServer,
    plans: Mutex<HashMap<String, Plan>>,
    runs: Mutex<HashMap<String, RunControl>>,
    scans: Mutex<HashMap<String, CancellationToken>>,
    cache_dir: PathBuf,
    opened: Mutex<Vec<PathBuf>>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl AppState {
    pub async fn new(settings: Settings, cache_dir: PathBuf) -> std::io::Result<AppState> {
        Ok(AppState {
            settings: Mutex::new(settings),
            engine: RwLock::new(None),
            allowed: Mutex::new(Allowed::default()),
            media: MediaServer::start().await?,
            plans: Mutex::new(HashMap::new()),
            runs: Mutex::new(HashMap::new()),
            scans: Mutex::new(HashMap::new()),
            cache_dir,
            opened: Mutex::new(Vec::new()),
        })
    }

    pub fn allow(&self, paths: &[PathBuf]) {
        let mut a = lock(&self.allowed);
        for p in paths {
            a.allow(p);
        }
    }

    /// Paths given on the command line, handed to the page once.
    pub fn set_opened(&self, paths: Vec<PathBuf>) {
        *lock(&self.opened) = paths;
    }

    pub fn take_opened(&self) -> Vec<PathBuf> {
        std::mem::take(&mut *lock(&self.opened))
    }

    /// Returns the canonical path, which is what callers then use (no check/use race on links).
    fn require_allowed(&self, p: &Path) -> OpResult<PathBuf> {
        if lock(&self.allowed).is_allowed(p) {
            std::fs::canonicalize(p).map_err(|e| e.to_string())
        } else {
            Err(format!("{} was not opened in rmedia (use Add files/folder or drag it in)", p.display()))
        }
    }

    pub fn allow_lists(&self, paths: &[PathBuf]) {
        let mut a = lock(&self.allowed);
        for p in paths {
            a.allow_list(p);
        }
    }

    /// Engine paths can only be set here — from the native file dialog in Rust — never by the page.
    pub fn set_engine_path(&self, ffmpeg: Option<PathBuf>) -> OpResult<()> {
        let mut s = lock(&self.settings).clone();
        match ffmpeg {
            Some(p) => {
                let name =
                    p.file_stem().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                if !name.ends_with("ffmpeg") {
                    return Err("choose the ffmpeg program (ffprobe must be in the same folder)".into());
                }
                let probe = p.with_file_name(format!(
                    "{}{}",
                    name.replace("ffmpeg", "ffprobe"),
                    std::env::consts::EXE_SUFFIX
                ));
                if !probe.is_file() {
                    return Err(format!("{} was not found next to ffmpeg", probe.display()));
                }
                s.ffmpeg_path = Some(p);
                s.ffprobe_path = Some(probe);
            }
            None => {
                s.ffmpeg_path = None;
                s.ffprobe_path = None;
            }
        }
        s.save().map_err(|e| e.to_string())?;
        *lock(&self.settings) = s;
        *self.engine.write().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(())
    }

    fn cache(&self) -> PreviewCache {
        let mb = lock(&self.settings).proxy_cache_max_mb;
        PreviewCache { dir: self.cache_dir.clone(), max_bytes: mb.saturating_mul(1024 * 1024) }
    }

    /// Locates the engine (cached until settings change).
    pub fn doctor(&self) -> DoctorReport {
        let s = lock(&self.settings).clone();
        let (report, found) =
            doctor::run(&LocateOptions { ffmpeg: s.ffmpeg_path, ffprobe: s.ffprobe_path, bundled_dir: None });
        *self.engine.write().unwrap_or_else(std::sync::PoisonError::into_inner) = found;
        report
    }

    fn engine(&self) -> OpResult<(Tools, Caps)> {
        if let Some(e) = self.engine.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone() {
            return Ok(e);
        }
        let report = self.doctor();
        self.engine
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| report.problems.join("; "))
    }

    pub async fn probe(&self, path: &Path) -> OpResult<MediaInfo> {
        let path = self.require_allowed(path)?;
        let (tools, _) = self.engine()?;
        probe::probe(&tools, &path, &CancellationToken::new()).await.map_err(|e| e.to_string())
    }

    pub async fn scan(
        &self,
        scan_id: String,
        roots: Vec<PathBuf>,
        opts: ScanOptions,
        on_event: &(dyn Fn(ScanEvent) + Send + Sync),
    ) -> OpResult<ScanResult> {
        let roots = roots.iter().map(|r| self.require_allowed(r)).collect::<OpResult<Vec<_>>>()?;
        let (tools, _) = self.engine()?;
        let cancel = CancellationToken::new();
        lock(&self.scans).insert(scan_id.clone(), cancel.clone());
        let result = discover::scan(&tools, &roots, &opts, on_event, &cancel).await;
        lock(&self.scans).remove(&scan_id);
        result.map_err(|e| e.to_string())
    }

    pub fn cancel_scan(&self, scan_id: &str) {
        if let Some(c) = lock(&self.scans).get(scan_id) {
            c.cancel();
        }
    }

    /// A clip list applied to `sources`, returned as a manifest (the queue format).
    pub fn load_cliplist(&self, list: &Path, sources: &[PathBuf]) -> OpResult<Value> {
        if !lock(&self.allowed).is_list(list) {
            return Err("open the clip list with Import list…".into());
        }
        for s in sources {
            self.require_allowed(s)?;
        }
        let clips = cliplist::load(list).map_err(issues_text)?;
        let req = BatchRequest {
            defaults: Options::default(),
            sources: sources
                .iter()
                .map(|p| rmedia_core::request::SourceRequest {
                    path: p.clone(),
                    clips: clips.clone(),
                    overrides: PartialOptions::default(),
                    origin: rmedia_core::request::Origin::Gui,
                })
                .collect(),
        };
        manifest_value(&req, &PartialOptions::default())
    }

    /// Opens a manifest; its videos become allowed sources (the user chose the manifest).
    pub fn load_manifest(&self, path: &Path) -> OpResult<Value> {
        if !lock(&self.allowed).is_list(path) {
            return Err("open the manifest with Open manifest…".into());
        }
        let req = match ext(path).as_str() {
            "json" => manifest_json::load(path, &Options::default()),
            "csv" => manifest_csv::load(path, &Options::default()),
            _ => return Err("choose a .json or .csv manifest".into()),
        }
        .map_err(issues_text)?;
        // A manifest may only grant access to video files it names (not arbitrary files).
        let videos: Vec<PathBuf> =
            req.sources.iter().map(|s| s.path.clone()).filter(|p| is_video_name(p) && p.is_file()).collect();
        self.allow(&videos);
        // Keep the manifest's own defaults (e.g. output folder) in the queue.
        manifest_value(&req, &PartialOptions::diff(&req.defaults, &Options::default()))
    }

    /// Writes the queue to a path chosen in the save dialog (.json keeps every option).
    pub fn export_manifest(&self, manifest: &Value, path: &Path) -> OpResult<()> {
        if !lock(&self.allowed).is_save_target(path) {
            return Err("choose where to save with the Save dialog".into());
        }
        let req = self.queue_request(manifest)?;
        let text = match ext(path).as_str() {
            // CSV folds every resolved option into its rows.
            "csv" => manifest_csv::write(&req),
            // The queue already is a validated v1 manifest: write it as is.
            _ => serde_json::to_string_pretty(manifest).map_err(|e| e.to_string())? + "\n",
        };
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    fn queue_request(&self, manifest: &Value) -> OpResult<BatchRequest> {
        let defaults = lock(&self.settings).defaults.apply(&Options::default());
        let req = manifest_json::from_value(Path::new("/"), manifest, &defaults).map_err(issues_text)?;
        for s in &req.sources {
            self.require_allowed(&s.path)?;
        }
        Ok(req)
    }

    /// Validates the queue and plans it. The plan is kept for `run` under its run id.
    pub async fn plan(&self, manifest: &Value) -> OpResult<Plan> {
        let req = self.queue_request(manifest)?;
        let (tools, caps) = self.engine()?;
        let mut plan = plan::plan(&req, &tools, &caps, &CancellationToken::new()).await;
        {
            let allowed = lock(&self.allowed);
            for job in &plan.jobs {
                if !allowed.may_write(&job.output, &job.source) {
                    plan.issues.push(Issue {
                        severity: rmedia_core::plan::Severity::Error,
                        code: "INVALID_INPUT",
                        origin: rmedia_core::request::Origin::Gui,
                        message: format!(
                            "{} is outside the folders you chose; pick the output folder in Settings",
                            job.output.display()
                        ),
                    });
                }
            }
        }
        lock(&self.plans).insert(plan.run_id.clone(), plan.clone());
        Ok(plan)
    }

    pub async fn run(&self, run_id: &str, sink: Arc<dyn EventSink>) -> OpResult<RunSummary> {
        let plan = lock(&self.plans).remove(run_id).ok_or("plan not found; plan the queue again")?;
        if plan.has_errors() {
            return Err("the queue has errors; fix them before running".into());
        }
        let (tools, _) = self.engine()?;
        let jobs = lock(&self.settings).jobs.map_or_else(exec::default_jobs, usize::from);
        let handle = exec::start(plan, tools, RunConfig { jobs, ..RunConfig::default() }, sink);
        lock(&self.runs).insert(run_id.to_owned(), handle.control());
        let summary = handle.wait().await;
        lock(&self.runs).remove(run_id);
        Ok(summary)
    }

    pub fn cancel_run(&self, run_id: &str, job: Option<u32>) {
        if let Some(c) = lock(&self.runs).get(run_id) {
            match job {
                Some(j) => c.cancel_job(j),
                None => c.cancel(),
            }
        }
    }

    /// Cancels everything (window close).
    pub fn cancel_all(&self) {
        for c in lock(&self.runs).values() {
            c.cancel();
        }
        for c in lock(&self.scans).values() {
            c.cancel();
        }
    }

    pub async fn preview_source(&self, path: &Path) -> OpResult<PreviewInfo> {
        let info = self.probe(path).await?;
        let direct = direct_playable(&info);
        let direct_url =
            if direct { Some(self.media.register(path).map_err(|e| e.to_string())?) } else { None };
        Ok(PreviewInfo { time_offset: preview::time_offset(&info).as_secs_f64(), direct_url, info })
    }

    pub async fn ensure_proxy(
        &self,
        path: &Path,
        codec: ProxyCodec,
        on_progress: &(dyn Fn(f64) + Send + Sync),
    ) -> OpResult<String> {
        let info = self.probe(path).await?;
        let (tools, _) = self.engine()?;
        let proxy = preview::ensure_proxy(
            &tools,
            &info,
            &self.cache(),
            codec,
            on_progress,
            &CancellationToken::new(),
        )
        .await
        .map_err(|e| e.to_string())?;
        self.media.register(&proxy).map_err(|e| e.to_string())
    }

    pub async fn frame_at(&self, path: &Path, t: f64, height: u32) -> OpResult<Vec<u8>> {
        let info = self.probe(path).await?;
        let (tools, _) = self.engine()?;
        preview::frame_at(
            &tools,
            &info,
            Ts::from_secs_f64(t.max(0.0)),
            height.clamp(32, 1080),
            &CancellationToken::new(),
        )
        .await
        .map_err(|e| e.to_string())
    }

    /// Thumbnails as `data:` URLs (no file access from the webview needed).
    pub async fn thumbnails(&self, path: &Path, n: usize) -> OpResult<Vec<String>> {
        use base64::Engine as _;
        let info = self.probe(path).await?;
        let (tools, _) = self.engine()?;
        let files = preview::thumbnails(&tools, &info, n, &self.cache(), &CancellationToken::new())
            .await
            .map_err(|e| e.to_string())?;
        files
            .iter()
            .map(|f| {
                let bytes = std::fs::read(f).map_err(|e| e.to_string())?;
                Ok(format!(
                    "data:image/jpeg;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                ))
            })
            .collect()
    }

    pub fn get_settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    /// Saves settings from the page. Engine paths are never taken from the page (they would let
    /// it choose an executable); they change only through `set_engine_path`.
    pub fn set_settings(&self, mut s: Settings) -> OpResult<()> {
        {
            let current = lock(&self.settings);
            s.ffmpeg_path = current.ffmpeg_path.clone();
            s.ffprobe_path = current.ffprobe_path.clone();
        }
        s.save().map_err(|e| e.to_string())?;
        *lock(&self.settings) = s;
        Ok(())
    }

    /// Window closing: stop work and revoke every media URL.
    pub fn shutdown(&self) {
        self.cancel_all();
        self.media.revoke_all();
    }
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(Clone))]
pub struct PreviewInfo {
    pub info: MediaInfo,
    /// Present when the webview can likely play the source as is.
    pub direct_url: Option<String>,
    /// Seconds to subtract from the player's `currentTime` to get clip time.
    pub time_offset: f64,
}

/// Containers/codecs both WebView2 and WebKitGTK commonly play; anything else uses a proxy. The
/// frontend still falls back to a proxy if direct playback errors.
fn direct_playable(m: &MediaInfo) -> bool {
    let v = m.first_video().map(|s| s.codec.as_str());
    let a: Vec<&str> = m
        .streams
        .iter()
        .filter(|s| s.kind == rmedia_core::probe::StreamKind::Audio)
        .map(|s| s.codec.as_str())
        .collect();
    match m.container.as_str() {
        "mp4" | "m4v" | "mov" => v == Some("h264") && a.iter().all(|c| *c == "aac"),
        "webm" => {
            matches!(v, Some("vp8" | "vp9" | "av1")) && a.iter().all(|c| matches!(*c, "opus" | "vorbis"))
        }
        _ => false,
    }
}

fn is_video_name(p: &Path) -> bool {
    rmedia_core::discover::VIDEO_EXTENSIONS.contains(&ext(p).as_str())
}

/// Canonicalizes the nearest existing ancestor and re-appends the rest.
fn resolve_existing(p: &Path) -> PathBuf {
    let mut existing = p.to_path_buf();
    let mut rest = Vec::new();
    while !existing.exists() {
        match (existing.file_name().map(ToOwned::to_owned), existing.parent()) {
            (Some(n), Some(parent)) => {
                rest.push(n);
                existing = parent.to_path_buf();
            }
            _ => break,
        }
    }
    let mut out = std::fs::canonicalize(&existing).unwrap_or(existing);
    for part in rest.iter().rev() {
        out.push(part);
    }
    out
}

fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

fn issues_text(issues: Vec<Issue>) -> String {
    issues.iter().map(|i| i.to_string()).collect::<Vec<_>>().join("\n")
}

/// Canonical manifest JSON (absolute paths) for the queue.
fn manifest_value(req: &BatchRequest, defaults: &PartialOptions) -> OpResult<Value> {
    serde_json::from_str(&manifest_json::write(req, defaults)).map_err(|e| e.to_string())
}

/// Parses user-typed time text for the queue editor.
pub fn parse_time(text: &str) -> OpResult<f64> {
    Ts::parse(text).map(Ts::as_secs_f64).map_err(|e| e.to_string())
}

/// Parses an end field (`end`, `+duration`, time) — returned as JSON for the manifest.
pub fn parse_end(text: &str) -> OpResult<Value> {
    match EndSpec::parse(text).map_err(|e| e.to_string())? {
        EndSpec::EndOfMedia => Ok(Value::String("end".into())),
        EndSpec::At(t) => Ok(serde_json::json!(t.as_secs_f64())),
        EndSpec::Duration(d) => Ok(Value::String(format!("+{}", d.as_secs_f64()))),
    }
}
