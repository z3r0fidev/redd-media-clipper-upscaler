//! T057: the desktop operations without a webview, including CLI↔GUI parity (SC-008).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use rmedia_core::event::{Event, EventSink, JobOutcome};
use rmedia_core::settings::Settings;
use rmedia_desktop::ops::AppState;
use rmedia_testkit::{Fixture, fixture, require_ffmpeg, sha256};
use serde_json::json;

#[derive(Default)]
struct Collect(Mutex<Vec<Event>>);
impl EventSink for Collect {
    fn emit(&self, e: Event) {
        self.0.lock().unwrap().push(e);
    }
}

fn cli_bin() -> PathBuf {
    std::env::var_os("RMEDIA_CLI_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/debug/rmedia"))
}

async fn state(dir: &Path) -> AppState {
    AppState::new(Settings::default(), dir.join("cache")).await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn only_user_chosen_paths_are_usable() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("a.mp4");
    std::fs::copy(fixture(Fixture::Gop50Mp4), &src).unwrap();
    let st = state(d.path()).await;
    assert!(st.probe(&src).await.unwrap_err().contains("was not opened"));
    let manifest = json!({"version":1,"videos":[{"input": src, "clips":[{"start":1,"end":2}]}]});
    assert!(st.plan(&manifest).await.is_err());
    assert!(st.export_manifest(&manifest, &d.path().join("q.json")).unwrap_err().contains("Save dialog"));
    st.allow(std::slice::from_ref(&src));
    assert!(st.probe(&src).await.is_ok());
    // A picked folder allows what is inside it, but not its siblings.
    let other = tempfile::tempdir().unwrap();
    std::fs::copy(fixture(Fixture::Gop50Mp4), other.path().join("b.mp4")).unwrap();
    st.allow(&[d.path().to_path_buf()]);
    assert!(st.probe(&src).await.is_ok());
    assert!(st.probe(&other.path().join("b.mp4")).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn preview_direct_or_proxy() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    let mp4 = d.path().join("a.mp4");
    let mkv = d.path().join("b.mkv");
    std::fs::copy(fixture(Fixture::Gop50Mp4), &mp4).unwrap();
    std::fs::copy(fixture(Fixture::MultiMkv), &mkv).unwrap();
    let st = state(d.path()).await;
    st.allow(&[mp4.clone(), mkv.clone()]);
    let p = st.preview_source(&mp4).await.unwrap();
    assert!(p.direct_url.as_deref().is_some_and(|u| u.starts_with("http://127.0.0.1:")));
    let p = st.preview_source(&mkv).await.unwrap();
    assert!(p.direct_url.is_none(), "mkv needs a proxy");
    let url = st.ensure_proxy(&mkv, rmedia_core::preview::ProxyCodec::H264, &|_| {}).await.unwrap();
    assert!(url.starts_with("http://127.0.0.1:"));
    let jpeg = st.frame_at(&mkv, 2.0, 120).await.unwrap();
    assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
    let thumbs = st.thumbnails(&mkv, 4).await.unwrap();
    assert!(thumbs.len() == 4 && thumbs[0].starts_with("data:image/jpeg;base64,"));
}

/// SC-008: the same queue run through the app and exported to `rmedia batch` gives identical files.
#[tokio::test(flavor = "multi_thread")]
async fn gui_and_cli_outputs_are_identical() {
    require_ffmpeg!();
    if !cli_bin().is_file() {
        panic!("build the CLI first (cargo build -p rmedia-cli) or set RMEDIA_CLI_BIN");
    }
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("a.mp4");
    std::fs::copy(fixture(Fixture::Gop50Mp4), &src).unwrap();
    let st = state(d.path()).await;
    st.allow(std::slice::from_ref(&src));
    let gui_out = d.path().join("gui");
    let manifest = json!({"version":1,"defaults":{"output_dir": gui_out},"videos":[{"input": src, "clips":[
        {"start":"0:01","end":"0:03","name":"fast"},
        {"start":"0:10.5","duration":2,"name":"exact","options":{"mode":"accurate"}}]}]});

    let plan = st.plan(&manifest).await.unwrap();
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    let sink = Arc::new(Collect::default());
    let summary = st.run(&plan.run_id, sink.clone()).await.unwrap();
    assert_eq!(summary.succeeded, 2, "{summary:?}");
    assert!(sink.0.lock().unwrap().iter().any(|e| matches!(e, Event::JobProgress { .. })));

    // Export (via the save-dialog allow list), point it at another folder, run the CLI.
    let export = d.path().join("queue.json");
    st.allowed.lock().unwrap().allow_save(&export);
    st.export_manifest(&manifest, &export).unwrap();
    let cli_out = d.path().join("cli");
    let text = std::fs::read_to_string(&export)
        .unwrap()
        .replace(&*gui_out.to_string_lossy(), &cli_out.to_string_lossy());
    std::fs::write(&export, text).unwrap();
    let o = Command::new(cli_bin())
        .arg("batch")
        .arg(&export)
        .arg("-q")
        .env("RMEDIA_STATE_DIR", d.path())
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));

    for job in &summary.jobs {
        let JobOutcome::Succeeded { output, .. } = &job.outcome else { panic!() };
        let twin = cli_out.join(output.file_name().unwrap());
        assert_eq!(sha256(output), sha256(&twin), "{} differs between app and CLI", output.display());
    }
}
