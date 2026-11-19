//! Desktop operations: thin adapters over rmedia-core (Constitution I), testable without a webview.
//! Tauri commands in `commands.rs` only unwrap arguments and call these.
//!
//! Path policy (research.md §R8 #3/#11): the webview may only name files the user chose — through
//! the native dialogs, drag-and-drop, a picked folder (and what scanning it found), or a manifest
//! the user opened. Anything else is refused, so a compromised page cannot read arbitrary files.

use std::collections::{HashSet, VecDeque};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use rmedia_core::discover::{self, ScanEvent, ScanOptions, ScanResult};
use rmedia_core::doctor::{self, DoctorReport};
use rmedia_core::event::{EventSink, RunSummary};
use rmedia_core::exec::{self, RunConfig, RunControl};
use rmedia_core::gif::GifOptions;
use rmedia_core::gif::engines::EngineKind;
use rmedia_core::input::{cliplist, manifest_csv, manifest_json};
use rmedia_core::plan::JobSpec;
use rmedia_core::plan::{self, Caps, Issue, Plan};
use rmedia_core::preset::PresetRegistry;
use rmedia_core::preview::{self, PreviewCache, ProxyCodec};
use rmedia_core::probe::{self, MediaInfo};
use rmedia_core::request::{BatchRequest, Options, PartialOptions};
use rmedia_core::settings::Settings;
use rmedia_core::smart::KeyframeIndex;
use rmedia_core::tools::{LocateOptions, Tools};
use rmedia_core::upscale::capability::CheckMode;
use rmedia_core::upscale::preview::{self as still, PreviewRequest};
use rmedia_core::upscale::reframe::{self, ReframeAnalysis};
use rmedia_core::upscale::{AnalysisState, Reframe, TierChoice, UpscaleOptions};
use rmedia_core::{EndSpec, Ts};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::media_server::MediaServer;

mod allowed;
mod gif;
mod previews;
mod queue;
mod registry;
mod smart;
mod upscale;

pub use allowed::*;
pub use gif::*;
pub use previews::*;
pub use queue::*;
use registry::{Others, Registry, Tasks};
pub use smart::*;
pub use upscale::*;

pub type OpResult<T> = Result<T, String>;

/// Where the settings say the engines are.
fn locate_options(s: &Settings) -> LocateOptions {
    LocateOptions {
        ffmpeg: s.ffmpeg_path.clone(),
        ffprobe: s.ffprobe_path.clone(),
        gifsicle: s.gifsicle_path.clone(),
        gifski: s.gifski_path.clone(),
        bundled_dir: None,
    }
}

pub struct AppState {
    pub settings: Mutex<Settings>,
    engine: RwLock<Option<(Tools, Caps)>>,
    pub allowed: Mutex<Allowed>,
    pub media: MediaServer,
    plans: Registry<Plan>,
    runs: Registry<RunControl>,
    scans: Tasks,
    analyses: Tasks,
    /// GIF size estimates in flight (one at a time: a new one cancels the others).
    estimates: Tasks,
    /// Each run's committed outputs by job id: the only files `open_output` serves (the last
    /// `MAX_RUNS_KEPT` runs).
    outputs: Mutex<VecDeque<RunOutputs>>,
    /// Outputs registered with the media server, oldest first (capped like preview stills).
    output_files: Mutex<VecDeque<PathBuf>>,
    /// The preview being rendered (a new one cancels it: only the latest is shown).
    preview: Mutex<Option<CancellationToken>>,
    /// Preview stills registered with the media server, oldest first (capped).
    preview_files: Mutex<VecDeque<PathBuf>>,
    /// Whole-file keyframe indexes in flight, by request id (feature 004).
    keyframe_requests: Tasks,
    /// Finished keyframe indexes, most recent last, keyed by canonical path, size and
    /// modification time (a changed file is read again). Capped at `MAX_KEYFRAME_INDEXES`.
    keyframe_cache: Mutex<VecDeque<(KeyframeKey, Arc<KeyframeIndex>)>>,
    /// Slots for uncached keyframe scans (`MAX_KEYFRAME_SCANS`): further requests wait.
    keyframe_slots: tokio::sync::Semaphore,
    /// The smart plan being made (a new one cancels it: only the latest range is shown).
    smart_plan: Mutex<Option<CancellationToken>>,
    /// Held across read-modify-save of the settings, so concurrent saves never lose a field.
    settings_save: Mutex<()>,
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
        // GIF estimates sample in private folders under the app cache; a crash can leave some.
        let _ = std::fs::remove_dir_all(cache_dir.join(GIF_ESTIMATES));
        // The saved default output folder was chosen with the folder dialog (set_settings only
        // keeps picked folders), so it stays usable across restarts.
        let mut allowed = Allowed::default();
        if let Some(dir) = &settings.defaults.output_dir {
            allowed.allow(dir);
        }
        Ok(AppState {
            settings: Mutex::new(settings),
            engine: RwLock::new(None),
            allowed: Mutex::new(allowed),
            media: MediaServer::start().await?,
            plans: Registry::default(),
            runs: Registry::default(),
            scans: Registry::default(),
            analyses: Registry::default(),
            estimates: Registry::default(),
            outputs: Mutex::new(VecDeque::new()),
            output_files: Mutex::new(VecDeque::new()),
            preview: Mutex::new(None),
            preview_files: Mutex::new(VecDeque::new()),
            keyframe_requests: Registry::default(),
            keyframe_cache: Mutex::new(VecDeque::new()),
            keyframe_slots: tokio::sync::Semaphore::new(MAX_KEYFRAME_SCANS),
            smart_plan: Mutex::new(None),
            settings_save: Mutex::new(()),
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
            rmedia_core::paths::canonicalize(p).map_err(|e| e.to_string())
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
    /// Sets (or resets) the ffmpeg program, with the ffprobe next to it. Only from the native file
    /// dialog in Rust, never from the page. Both must identify themselves ([`check_engine`]).
    pub async fn set_engine_path(&self, ffmpeg: Option<PathBuf>) -> OpResult<()> {
        let pair = match ffmpeg {
            Some(p) => {
                let probe = check_engine(&p).await?;
                Some((p, probe))
            }
            None => None,
        };
        let _saving = lock(&self.settings_save);
        let mut s = lock(&self.settings).clone();
        (s.ffmpeg_path, s.ffprobe_path) = pair.unzip();
        s.save().map_err(|e| e.to_string())?;
        *lock(&self.settings) = s;
        *self.engine.write().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(())
    }

    fn cache(&self) -> PreviewCache {
        let mb = lock(&self.settings).proxy_cache_max_mb;
        PreviewCache { dir: self.cache_dir.clone(), max_bytes: mb.saturating_mul(1024 * 1024) }
    }

    /// Locates the engine (cached until settings change), without the GPU check.
    pub async fn doctor(&self) -> DoctorReport {
        let s = lock(&self.settings).clone();
        let (report, found) = doctor::run(&locate_options(&s), &s).await;
        *self.engine.write().unwrap_or_else(std::sync::PoisonError::into_inner) = found;
        report
    }

    /// The engine check plus the machine capability check: `Cached` reuses a stored record that
    /// still matches this machine (start-up, settings saved); `Recheck` runs the tier self-tests
    /// again (Settings › Check again; a few seconds). One recheck at a time.
    pub async fn doctor_checked(&self, mode: CheckMode) -> OpResult<DoctorReport> {
        static RECHECKING: AtomicBool = AtomicBool::new(false);
        struct Done;
        impl Drop for Done {
            fn drop(&mut self) {
                RECHECKING.store(false, Ordering::SeqCst);
            }
        }
        let _done = if mode == CheckMode::Recheck {
            if RECHECKING.swap(true, Ordering::SeqCst) {
                return Err("the machine check is already running".into());
            }
            Some(Done)
        } else {
            None
        };
        let s = lock(&self.settings).clone();
        let (mut report, mut found) = doctor::run(&locate_options(&s), &s).await;
        doctor::check_capability(&mut report, found.as_mut(), mode, &CancellationToken::new()).await;
        *self.engine.write().unwrap_or_else(std::sync::PoisonError::into_inner) = found;
        Ok(report)
    }

    async fn engine(&self) -> OpResult<(Tools, Caps)> {
        if let Some(e) = self.engine.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone() {
            return Ok(e);
        }
        let report = self.doctor().await;
        self.engine
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| report.problems.join("; "))
    }

    pub async fn probe(&self, path: &Path) -> OpResult<MediaInfo> {
        let path = self.require_allowed(path)?;
        let (tools, _) = self.engine().await?;
        probe::probe(&tools, &path, &CancellationToken::new()).await.map_err(|e| e.to_string())
    }

    /// Cancels everything (window close).
    pub fn cancel_all(&self) {
        self.runs.for_each(RunControl::cancel);
        self.scans.cancel_all();
        self.analyses.cancel_all();
        self.estimates.cancel_all();
        self.keyframe_requests.cancel_all();
        if let Some(c) = lock(&self.preview).as_ref() {
            c.cancel();
        }
    }

    pub fn get_settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    /// Saves settings from the page. Engine paths are never taken from the page (they would let
    /// it choose an executable); they change only through `set_engine_path`.
    pub fn set_settings(&self, mut s: Settings) -> OpResult<()> {
        let _saving = lock(&self.settings_save);
        // The page may not start an unbounded number of engine processes at once.
        s.jobs = s.jobs.map(|j| j.clamp(1, 64));
        {
            let current = lock(&self.settings);
            s.ffmpeg_path = current.ffmpeg_path.clone();
            s.ffprobe_path = current.ffprobe_path.clone();
            // The GIF engines too: they are executables (research R13).
            s.gifsicle_path = current.gifsicle_path.clone();
            s.gifski_path = current.gifski_path.clone();
            // The models folder is code the app loads: it is chosen in Rust (`set_models_dir`).
            s.models_dir = current.models_dir.clone();
            // A default output folder must be one the user picked (it is trusted after restart).
            if let Some(dir) = &s.defaults.output_dir
                && current.defaults.output_dir.as_ref() != Some(dir)
                && !lock(&self.allowed).is_allowed(dir)
            {
                return Err("choose the output folder with Choose…".into());
            }
        }
        s.save().map_err(|e| e.to_string())?;
        *lock(&self.settings) = s;
        Ok(())
    }

    /// The models folder can only be set here — from the native folder dialog in Rust — never by
    /// the page. The engine capabilities are reloaded so the models are found again.
    pub fn set_models_dir(&self, dir: Option<PathBuf>) -> OpResult<()> {
        let _saving = lock(&self.settings_save);
        let mut s = lock(&self.settings).clone();
        s.models_dir = match dir {
            Some(d) => Some(std::fs::canonicalize(&d).map_err(|e| format!("{}: {e}", d.display()))?),
            None => None,
        };
        s.save().map_err(|e| e.to_string())?;
        *lock(&self.settings) = s;
        *self.engine.write().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(())
    }

    /// Window closing: stop work and revoke every media URL.
    pub fn shutdown(&self) {
        self.cancel_all();
        self.media.revoke_all();
    }
}

/// A clip range from the page, in seconds of source time (`end: None` = to the end).
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct RangeArg {
    pub start: f64,
    pub end: Option<f64>,
}

impl RangeArg {
    fn span(self) -> OpResult<(Ts, EndSpec)> {
        let end = match self.end {
            None => EndSpec::EndOfMedia,
            Some(e) => EndSpec::At(seconds(e)?),
        };
        Ok((seconds(self.start)?, end))
    }
}

/// A time from the page: finite, not negative, at most about 115 days.
fn seconds(t: f64) -> OpResult<Ts> {
    if t.is_finite() && (0.0..=1e7).contains(&t) {
        Ok(Ts::from_secs_f64(t))
    } else {
        Err(format!("invalid time {t}"))
    }
}

/// Keyframe indexes kept (one per source; the oldest is dropped first).
/// Checks a chosen ffmpeg program and returns the ffprobe next to it: the name must be ffmpeg,
/// ffprobe must be in the same folder, and both must report their version (security L3), so a
/// renamed program is refused before it is saved.
pub async fn check_engine(ffmpeg: &Path) -> OpResult<PathBuf> {
    let name = ffmpeg.file_stem().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !name.ends_with("ffmpeg") {
        return Err("choose the ffmpeg program (ffprobe must be in the same folder)".into());
    }
    let probe = ffmpeg.with_file_name(format!(
        "{}{}",
        name.replace("ffmpeg", "ffprobe"),
        std::env::consts::EXE_SUFFIX
    ));
    if !probe.is_file() {
        return Err(format!("{} was not found next to ffmpeg", probe.display()));
    }
    rmedia_core::tools::check_chosen(ffmpeg, &probe).await.map_err(|e| e.to_string())?;
    Ok(probe)
}

/// `analyze_reframe` channel events (contracts/tauri-commands.md).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AnalysisEvent {
    Progress { fraction: f64 },
    Done,
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
