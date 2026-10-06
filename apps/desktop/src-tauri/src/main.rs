//! T038 preview spike: can the webview play and seek a local video through the asset protocol?
//! Usage: RMEDIA_SPIKE_VIDEOS=a.mp4:b.webm cargo run  (prints a JSON report and exits)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use tauri::Manager;

#[tauri::command]
fn spike_videos(app: tauri::AppHandle) -> Vec<String> {
    let list = std::env::var("RMEDIA_SPIKE_VIDEOS").unwrap_or_default();
    let paths: Vec<PathBuf> = std::env::split_paths(&list)
        .filter(|p| p.is_file())
        .filter_map(|p| std::fs::canonicalize(p).ok())
        .collect();
    for p in &paths {
        // Runtime, per-file grant: the static scope stays empty.
        let _ = app.asset_protocol_scope().allow_file(p);
    }
    paths.into_iter().map(|p| p.to_string_lossy().into_owned()).collect()
}

#[tauri::command]
fn spike_http_base() -> String {
    std::env::var("RMEDIA_SPIKE_HTTP").unwrap_or_default()
}

#[tauri::command]
fn spike_report(app: tauri::AppHandle, report: serde_json::Value) {
    println!("SPIKE_REPORT {}", serde_json::to_string_pretty(&report).unwrap_or_default());
    app.exit(0);
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![spike_videos, spike_http_base, spike_report])
        .setup(|app| {
            let _ = app.get_webview_window("main");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
