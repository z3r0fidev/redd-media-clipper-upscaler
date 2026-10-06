//! rmedia desktop app. All media logic is in rmedia-core; `ops` adapts it, `commands` exposes it.

pub mod commands;
pub mod media_server;
pub mod ops;

use tauri::{Emitter, Manager, WindowEvent};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let settings = rmedia_core::settings::Settings::load();
            let cache = rmedia_core::settings::cache_dir()
                .unwrap_or_else(|| std::env::temp_dir().join("rmedia-preview"));
            let state = tauri::async_runtime::block_on(ops::AppState::new(settings, cache))?;
            // `rmedia-desktop FILE|FOLDER...` (and "Open with rmedia"): the user chose these.
            let opened: Vec<std::path::PathBuf> =
                std::env::args_os().skip(1).map(std::path::PathBuf::from).filter(|p| p.exists()).collect();
            state.allow(&opened);
            state.set_opened(opened);
            app.manage(state);
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Dropped paths are registered on the Rust side before the page sees them.
            WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) => {
                if let Some(state) = window.try_state::<ops::AppState>() {
                    state.allow(paths);
                }
                let shown: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
                let _ = window.emit("rmedia://dropped", shown);
            }
            WindowEvent::CloseRequested { .. } => {
                if let Some(state) = window.try_state::<ops::AppState>() {
                    state.cancel_all();
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::doctor,
            commands::take_opened,
            commands::pick_files,
            commands::pick_folder,
            commands::pick_save_path,
            commands::probe,
            commands::scan,
            commands::cancel_scan,
            commands::parse_time,
            commands::parse_end,
            commands::load_cliplist,
            commands::load_manifest,
            commands::export_manifest,
            commands::plan,
            commands::run,
            commands::cancel_run,
            commands::preview_source,
            commands::ensure_proxy,
            commands::frame_at,
            commands::thumbnails,
            commands::get_settings,
            commands::set_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running rmedia");
}
