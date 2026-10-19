//! Tauri commands (contracts/tauri-commands.md): argument plumbing only.

use std::path::PathBuf;
use std::sync::Arc;

use rmedia_core::discover::{ScanEvent, ScanOptions, ScanResult};
use rmedia_core::doctor::DoctorReport;
use rmedia_core::event::{Event, EventSink, RunSummary};
use rmedia_core::plan::Plan;
use rmedia_core::preview::ProxyCodec;
use rmedia_core::probe::MediaInfo;
use rmedia_core::settings::Settings;
use serde_json::Value;
use tauri::ipc::{Channel, Response};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::ops::{self, AnalysisEvent, AppState, OpResult, PreviewInfo, RangeArg, UpscalePreview};
use rmedia_core::upscale::UpscaleOptions;
use rmedia_core::upscale::capability::CheckMode;
use rmedia_core::upscale::reframe::ReframeAnalysis;

const VIDEO_EXTENSIONS: &[&str] = &rmedia_core::discover::VIDEO_EXTENSIONS;

/// The engine and the stored machine check (`rmedia doctor` without new self-tests).
#[tauri::command]
pub async fn doctor(state: State<'_, AppState>) -> OpResult<DoctorReport> {
    state.doctor_checked(CheckMode::Cached).await
}

/// Runs the GPU and tier checks again (`rmedia doctor --recheck`).
#[tauri::command]
pub async fn doctor_recheck(state: State<'_, AppState>) -> OpResult<DoctorReport> {
    state.doctor_checked(CheckMode::Recheck).await
}

#[tauri::command]
pub fn take_opened(state: State<'_, AppState>) -> Vec<String> {
    state.take_opened().iter().map(|p| p.to_string_lossy().into_owned()).collect()
}

/// Native file picker. `kind`: "video", "cliplist" or "manifest". Picked files become allowed.
#[tauri::command]
pub async fn pick_files(app: AppHandle, state: State<'_, AppState>, kind: String) -> OpResult<Vec<String>> {
    let dialog = app.dialog().file();
    let dialog = match kind.as_str() {
        "cliplist" => dialog.add_filter("Clip list", &["txt"]),
        "manifest" => dialog.add_filter("Manifest", &["json", "csv"]),
        _ => dialog.add_filter("Video", VIDEO_EXTENSIONS),
    };
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_files())
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let paths: Vec<PathBuf> = picked.into_iter().filter_map(|p| p.into_path().ok()).collect();
    if kind == "video" {
        state.allow(&paths);
    } else {
        state.allow_lists(&paths);
    }
    Ok(paths.iter().map(|p| p.to_string_lossy().into_owned()).collect())
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle, state: State<'_, AppState>) -> OpResult<Option<String>> {
    let dialog = app.dialog().file();
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_folder())
        .await
        .map_err(|e| e.to_string())?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else { return Ok(None) };
    state.allow(std::slice::from_ref(&path));
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Native save dialog; the chosen path becomes the only allowed export target.
#[tauri::command]
pub async fn pick_save_path(
    app: AppHandle,
    state: State<'_, AppState>,
    default_name: String,
) -> OpResult<Option<String>> {
    let dialog = app.dialog().file().set_file_name(&default_name).add_filter("Manifest", &["json", "csv"]);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file())
        .await
        .map_err(|e| e.to_string())?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else { return Ok(None) };
    state.allowed.lock().unwrap_or_else(std::sync::PoisonError::into_inner).allow_save(&path);
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn probe(state: State<'_, AppState>, path: PathBuf) -> OpResult<MediaInfo> {
    state.probe(&path).await
}

#[tauri::command]
pub async fn scan(
    state: State<'_, AppState>,
    scan_id: String,
    roots: Vec<PathBuf>,
    options: ScanOptions,
    on_event: Channel<ScanEvent>,
) -> OpResult<ScanResult> {
    state.scan(scan_id, roots, options, &move |e| drop(on_event.send(e))).await
}

#[tauri::command]
pub fn cancel_scan(state: State<'_, AppState>, scan_id: String) {
    state.cancel_scan(&scan_id);
}

#[tauri::command]
pub fn parse_time(text: String) -> OpResult<f64> {
    ops::parse_time(&text)
}

#[tauri::command]
pub fn parse_end(text: String) -> OpResult<Value> {
    ops::parse_end(&text)
}

#[tauri::command]
pub fn load_cliplist(state: State<'_, AppState>, path: PathBuf, sources: Vec<PathBuf>) -> OpResult<Value> {
    state.load_cliplist(&path, &sources)
}

#[tauri::command]
pub fn load_manifest(state: State<'_, AppState>, path: PathBuf) -> OpResult<Value> {
    state.load_manifest(&path)
}

#[tauri::command]
pub fn export_manifest(state: State<'_, AppState>, manifest: Value, path: PathBuf) -> OpResult<()> {
    state.export_manifest(&manifest, &path)
}

#[tauri::command]
pub async fn plan(state: State<'_, AppState>, manifest: Value) -> OpResult<Plan> {
    state.plan(&manifest).await
}

struct ChannelSink(Channel<Event>);

impl EventSink for ChannelSink {
    fn emit(&self, e: Event) {
        let _ = self.0.send(e);
    }
}

#[tauri::command]
pub async fn run(
    state: State<'_, AppState>,
    run_id: String,
    on_event: Channel<Event>,
) -> OpResult<RunSummary> {
    state.run(&run_id, Arc::new(ChannelSink(on_event))).await
}

#[tauri::command]
pub fn cancel_run(state: State<'_, AppState>, run_id: String, job_id: Option<u32>) {
    state.cancel_run(&run_id, job_id);
}

#[tauri::command]
pub async fn preview_source(state: State<'_, AppState>, path: PathBuf) -> OpResult<PreviewInfo> {
    state.preview_source(&path).await
}

#[tauri::command]
pub async fn ensure_proxy(
    state: State<'_, AppState>,
    path: PathBuf,
    codec: ProxyCodec,
    on_progress: Channel<f64>,
) -> OpResult<String> {
    state.ensure_proxy(&path, codec, &move |f| drop(on_progress.send(f))).await
}

#[tauri::command]
pub async fn frame_at(state: State<'_, AppState>, path: PathBuf, t: f64, height: u32) -> OpResult<Response> {
    state.frame_at(&path, t, height).await.map(Response::new)
}

#[tauri::command]
pub async fn thumbnails(state: State<'_, AppState>, path: PathBuf, n: usize) -> OpResult<Vec<String>> {
    state.thumbnails(&path, n).await
}

/// Choose the ffmpeg program with the native dialog (ffprobe must sit next to it), or reset to
/// automatic (bundled, then PATH) with `reset = true`.
#[tauri::command]
pub async fn pick_engine(app: AppHandle, state: State<'_, AppState>, reset: bool) -> OpResult<DoctorReport> {
    if reset {
        state.set_engine_path(None)?;
    } else {
        let dialog = app.dialog().file().set_title("Choose the ffmpeg program");
        let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_file())
            .await
            .map_err(|e| e.to_string())?;
        let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
            return state.doctor_checked(CheckMode::Cached).await;
        };
        state.set_engine_path(Some(path))?;
    }
    state.doctor_checked(CheckMode::Cached).await
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.get_settings()
}

#[tauri::command]
pub fn set_settings(state: State<'_, AppState>, settings: Settings) -> OpResult<()> {
    state.set_settings(settings)
}

/// Device presets for the upscale view.
#[tauri::command]
pub fn list_presets(state: State<'_, AppState>) -> Value {
    state.list_presets()
}

#[tauri::command]
pub async fn analyze_reframe(
    state: State<'_, AppState>,
    analysis_id: String,
    path: PathBuf,
    span: Option<RangeArg>,
    options: UpscaleOptions,
    on_event: Channel<AnalysisEvent>,
) -> OpResult<Option<ReframeAnalysis>> {
    state.analyze_reframe(analysis_id, &path, span, &options, &move |e| drop(on_event.send(e))).await
}

#[tauri::command]
pub fn cancel_analysis(state: State<'_, AppState>, analysis_id: String) {
    state.cancel_analysis(&analysis_id);
}

#[tauri::command]
pub async fn upscale_preview(
    state: State<'_, AppState>,
    path: PathBuf,
    span: Option<RangeArg>,
    at: f64,
    options: UpscaleOptions,
) -> OpResult<UpscalePreview> {
    state.upscale_preview(&path, span, at, &options).await
}

/// Choose the models folder with the native dialog, or reset to the default search (`reset`).
#[tauri::command]
pub async fn pick_models_dir(app: AppHandle, state: State<'_, AppState>, reset: bool) -> OpResult<Settings> {
    if reset {
        state.set_models_dir(None)?;
    } else {
        let dialog = app.dialog().file().set_title("Choose the models folder");
        let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_folder())
            .await
            .map_err(|e| e.to_string())?;
        if let Some(dir) = picked.and_then(|p| p.into_path().ok()) {
            state.set_models_dir(Some(dir))?;
        }
    }
    Ok(state.get_settings())
}
