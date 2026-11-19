//! rmedia desktop app. All media logic is in rmedia-core; `ops` adapts it, `commands` exposes it.

pub mod commands;
pub mod logging;
pub mod media_server;
pub mod ops;

use tauri::{Emitter, Manager, WindowEvent};

pub fn run() {
    match logging::init() {
        Some(log) => {
            tracing::info!("rmedia {} started; logging to {}", env!("CARGO_PKG_VERSION"), log.display())
        }
        None => tracing::warn!("rmedia {} started; no log file (stderr only)", env!("CARGO_PKG_VERSION")),
    }
    // The state (and its media server) comes first: the page's CSP names the server's port (L6).
    let settings = rmedia_core::settings::Settings::load();
    let cache =
        rmedia_core::settings::cache_dir().unwrap_or_else(|| std::env::temp_dir().join("rmedia-preview"));
    let state = tauri::async_runtime::block_on(ops::AppState::new(settings, cache))
        .expect("cannot start the local media server");
    let mut context = tauri::generate_context!();
    let security = &mut context.config_mut().app.security;
    let base = security.csp.as_ref().map(ToString::to_string).unwrap_or_default();
    let csp = csp_for_media_port(&base, state.media.port())
        .expect("the configured CSP has no media source to narrow (http://127.0.0.1:*)");
    security.csp = Some(tauri::utils::config::Csp::Policy(csp));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
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
                    state.shutdown();
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::doctor,
            commands::doctor_recheck,
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
            commands::pick_engine,
            commands::get_settings,
            commands::set_settings,
            commands::list_presets,
            commands::analyze_reframe,
            commands::cancel_analysis,
            commands::upscale_preview,
            commands::pick_models_dir,
            commands::list_gif_targets,
            commands::gif_estimate,
            commands::cancel_estimate,
            commands::open_output,
            commands::keyframes,
            commands::cancel_keyframes,
            commands::smart_plan,
        ])
        .run(context)
        .expect("error while running rmedia");
}

/// The configured CSP with its media source (`http://127.0.0.1:*`, any port) narrowed to this
/// run's media server (security L6). `None` when the policy has no such source: the caller
/// refuses to start rather than run with a policy it did not narrow.
pub fn csp_for_media_port(base: &str, port: u16) -> Option<String> {
    const ANY_PORT: &str = "http://127.0.0.1:*";
    base.contains(ANY_PORT).then(|| base.replace(ANY_PORT, &format!("http://127.0.0.1:{port}")))
}

#[cfg(test)]
mod tests {
    /// Security L6: the page may load images and media only from this run's media server port,
    /// not from any port on 127.0.0.1.
    #[test]
    fn the_csp_names_only_the_media_server_port() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let base = conf["app"]["security"]["csp"].as_str().unwrap();
        let csp = super::csp_for_media_port(base, 51234).unwrap();
        assert!(!csp.contains("127.0.0.1:*"), "{csp}");
        assert!(csp.contains("img-src 'self' data: blob: http://127.0.0.1:51234;"), "{csp}");
        assert!(csp.contains("media-src http://127.0.0.1:51234;"), "{csp}");
        // Fails closed: a policy without the placeholder is not used as it is.
        assert!(super::csp_for_media_port("default-src 'self'", 51234).is_none());
    }
}
