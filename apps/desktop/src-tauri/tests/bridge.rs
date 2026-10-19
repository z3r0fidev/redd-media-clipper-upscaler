//! T057: the desktop operations without a webview, including CLI↔GUI parity (SC-008).

// `std::env::set_var` is unsafe in edition 2024; it is called once before any reader (see `state`).
#![allow(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use rmedia_core::event::{Event, EventSink, JobOutcome};
use rmedia_core::settings::Settings;
use rmedia_core::upscale::{Reframe, TierChoice, UpscaleOptions};
use rmedia_desktop::ops::{AnalysisEvent, AppState, RangeArg};
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
    // Never touch the developer's real settings file.
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let cfg = std::env::temp_dir().join(format!("rmedia-test-config-{}", std::process::id()));
        let cache = std::env::temp_dir().join(format!("rmedia-test-cache-{}", std::process::id()));
        // SAFETY: set once, before any test thread reads the variables.
        unsafe {
            std::env::set_var("RMEDIA_CONFIG_DIR", cfg);
            std::env::set_var("RMEDIA_CACHE_DIR", cache);
        }
    });
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

/// Security review H1: the page cannot choose the engine executable through settings.
#[tokio::test(flavor = "multi_thread")]
async fn settings_from_the_page_cannot_set_engine_paths() {
    let d = tempfile::tempdir().unwrap();
    let st = state(d.path()).await;
    let mut s = st.get_settings();
    s.ffmpeg_path = Some("/bin/sh".into());
    s.ffprobe_path = Some("/bin/sh".into());
    s.jobs = Some(3);
    st.set_settings(s).unwrap();
    let now = st.get_settings();
    assert_eq!((now.ffmpeg_path, now.ffprobe_path), (None, None));
    assert!(st.set_engine_path(Some("/bin/sh".into())).is_err(), "only a program named ffmpeg is accepted");
}

/// Security review M1/M4: outputs must stay in chosen folders; lists must be picked as lists;
/// manifests only grant access to video files.
#[tokio::test(flavor = "multi_thread")]
async fn outputs_lists_and_manifests_are_confined() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let src = d.path().join("a.mp4");
    std::fs::copy(fixture(Fixture::Gop50Mp4), &src).unwrap();
    let st = state(d.path()).await;
    st.allow(std::slice::from_ref(&src));

    let outside = json!({"version":1,"defaults":{"output_dir": elsewhere.path()},"videos":[{"input": src, "clips":[{"start":1,"end":2}]}]});
    let plan = st.plan(&outside).await.unwrap();
    assert!(
        plan.issues.iter().any(|i| i.message.contains("outside the folders you chose")),
        "{:?}",
        plan.issues
    );
    let next_to_source = json!({"version":1,"videos":[{"input": src, "clips":[{"start":1,"end":2}]}]});
    assert!(st.plan(&next_to_source).await.unwrap().issues.is_empty());
    st.allow(&[elsewhere.path().to_path_buf()]);
    assert!(st.plan(&outside).await.unwrap().issues.is_empty(), "a picked folder is a valid output");

    let list = d.path().join("clips.txt");
    std::fs::write(&list, "0:01 0:02 x\n").unwrap();
    st.allow(std::slice::from_ref(&list)); // e.g. dropped: still not usable as a list
    assert!(st.load_cliplist(&list, std::slice::from_ref(&src)).is_err());
    st.allow_lists(std::slice::from_ref(&list));
    assert!(st.load_cliplist(&list, std::slice::from_ref(&src)).is_ok());

    let secret = d.path().join("notes.txt");
    let other_video = elsewhere.path().join("b.mp4");
    std::fs::write(&secret, "private").unwrap();
    std::fs::copy(fixture(Fixture::Gop50Mp4), &other_video).unwrap();
    let fresh = state(d.path()).await;
    let manifest = d.path().join("m.json");
    std::fs::write(
        &manifest,
        json!({"version":1,"videos":[{"input": other_video,"clips":[{"start":1}]},{"input": secret,"clips":[{"start":1}]}]}).to_string(),
    )
    .unwrap();
    assert!(fresh.load_manifest(&manifest).is_err(), "must be opened with Open manifest…");
    fresh.allow_lists(std::slice::from_ref(&manifest));
    fresh.load_manifest(&manifest).unwrap();
    assert!(fresh.probe(&other_video).await.is_ok(), "manifest videos become usable");
    assert!(fresh.probe(&secret).await.is_err(), "non-video entries are not granted");
}

/// Docs review: a JSON export reproduces the app's run, including Settings defaults; and a picked
/// output folder saved in Settings stays usable after a restart (but an unpicked one is refused).
#[tokio::test(flavor = "multi_thread")]
async fn export_carries_settings_defaults_and_saved_output_survives_restart() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    let src = d.path().join("a.mp4");
    std::fs::copy(fixture(Fixture::Gop50Mp4), &src).unwrap();
    let st = state(d.path()).await;
    st.allow(std::slice::from_ref(&src));

    let mut s = st.get_settings();
    s.defaults.output_dir = Some(out_dir.path().to_path_buf());
    assert!(st.set_settings(s.clone()).is_err(), "unpicked folder refused");
    st.allow(&[out_dir.path().to_path_buf()]); // as pick_folder does
    s.defaults.mode = Some(rmedia_core::request::Mode::Accurate);
    st.set_settings(s.clone()).unwrap();

    let queue = json!({"version":1,"videos":[{"input": src, "clips":[{"start":1,"end":2}]}]});
    let export = d.path().join("q.json");
    st.allowed.lock().unwrap().allow_save(&export);
    st.export_manifest(&queue, &export).unwrap();
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&export).unwrap()).unwrap();
    assert_eq!(written["defaults"]["mode"], "accurate");
    assert_eq!(written["defaults"]["output_dir"], json!(out_dir.path()));

    // "Restart": a fresh state from the saved settings can plan into the saved folder.
    let restarted = AppState::new(s, d.path().join("cache2")).await.unwrap();
    restarted.allow(std::slice::from_ref(&src));
    let plan = restarted.plan(&queue).await.unwrap();
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
}

/// The workspace's face model (`cargo xtask fetch-models`), if fetched.
fn yunet() -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../models").join(rmedia_core::models::YUNET);
    p.is_file().then(|| std::fs::canonicalize(p).unwrap())
}

fn cpu(reframe: Reframe) -> UpscaleOptions {
    UpscaleOptions { tier: TierChoice::Cpu, reframe, ..UpscaleOptions::default() }
}

/// T083: the presets the view offers.
#[tokio::test(flavor = "multi_thread")]
async fn presets_are_listed_with_the_default() {
    let d = tempfile::tempdir().unwrap();
    let st = state(d.path()).await;
    let list = st.list_presets();
    assert_eq!(list["default"], "phone-fill");
    let ids: Vec<&str> =
        list["presets"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert!(ids.len() >= 7 && ids.contains(&"phone-fill") && ids.contains(&"reddit-916"), "{ids:?}");
    assert!(list["presets"][0]["devices"].is_array() && list["problems"].is_array());
}

/// T083: the before/after still is served by the media server; only opened sources qualify.
#[tokio::test(flavor = "multi_thread")]
async fn upscale_preview_serves_two_pngs_for_opened_sources_only() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("a.mp4");
    std::fs::copy(fixture(Fixture::TinySource), &src).unwrap();
    let st = state(d.path()).await;
    let opts = cpu(Reframe::Blur);
    let err = st.upscale_preview(&src, None, 1.0, &opts).await.unwrap_err();
    assert!(err.contains("was not opened"), "{err}");
    st.allow(std::slice::from_ref(&src));
    let p = st.upscale_preview(&src, None, 1.0, &opts).await.unwrap();
    assert_eq!(p.size, (1080, 2340));
    assert!(!p.approximate && !p.in_fallback);
    for url in [&p.before, &p.after] {
        assert!(url.starts_with("http://127.0.0.1:"), "{url}");
        let (head, body) = http_get(url).await;
        assert!(
            head.starts_with("HTTP/1.1 200") && head.to_ascii_lowercase().contains("content-type: image/png"),
            "{head}"
        );
        assert_eq!(&body[..8], b"\x89PNG\r\n\x1a\n");
    }
    // The page never gets a filesystem path: stills live in the app cache.
    assert!(!p.before.contains(&*d.path().to_string_lossy()));
    let range = Some(RangeArg { start: 0.5, end: Some(2.0) });
    assert!(st.upscale_preview(&src, range, 2.5, &opts).await.unwrap_err().contains("outside"));
    for bad in [f64::NAN, f64::INFINITY, -1.0, 1e300] {
        assert!(
            st.upscale_preview(&src, None, bad, &opts).await.unwrap_err().contains("invalid time"),
            "{bad}"
        );
        let range = Some(RangeArg { start: 0.0, end: Some(bad) });
        assert!(st.upscale_preview(&src, range, 1.0, &opts).await.is_err(), "{bad}");
    }
}

/// Minimal HTTP GET against the local media server: (status line and headers, body).
async fn http_get(url: &str) -> (String, Vec<u8>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let rest = url.strip_prefix("http://").unwrap();
    let (host, path) = rest.split_once('/').unwrap();
    let mut s = tokio::net::TcpStream::connect(host).await.unwrap();
    s.write_all(format!("GET /{path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    (String::from_utf8_lossy(&buf[..split]).into_owned(), buf[split + 4..].to_vec())
}

/// T083: the desktop analysis reports progress, shares the CLI's cache, and can be cancelled.
#[tokio::test(flavor = "multi_thread")]
async fn analysis_reports_progress_shares_the_cli_cache_and_cancels() {
    require_ffmpeg!();
    let Some(model) = yunet() else {
        eprintln!("skipped: the YuNet model is not in models/ (cargo xtask fetch-models)");
        return;
    };
    if !cli_bin().is_file() {
        panic!("build the CLI first (cargo build -p rmedia-cli) or set RMEDIA_CLI_BIN");
    }
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("land.mp4");
    std::fs::copy(fixture(Fixture::MovingMarker), &src).unwrap();
    let st = state(d.path()).await;
    // `<cache root>/models` is searched by the app and the CLI alike; the settings file is
    // shared by every test of this process, so it is left alone.
    let models = rmedia_core::settings::cache_root().unwrap().join("models");
    std::fs::create_dir_all(&models).unwrap();
    std::fs::copy(&model, models.join(rmedia_core::models::YUNET)).unwrap();
    st.allow(std::slice::from_ref(&src));

    // A cancelled analysis stops promptly.
    let id = "cancel-me".to_string();
    let started = std::time::Instant::now();
    let smart = cpu(Reframe::Smart);
    let (r, ()) = tokio::join!(st.analyze_reframe(id.clone(), &src, None, &smart, &|_| {}), async {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        st.cancel_analysis(&id);
    });
    assert!(r.unwrap_err().to_lowercase().contains("cancel"));
    assert!(started.elapsed().as_secs_f64() < 2.5, "{:?}", started.elapsed());
    // A forced tier that is not available here does not stop the analysis (it is tier-free).
    let ai = UpscaleOptions { tier: TierChoice::Ai, ..cpu(Reframe::Smart) };
    let first = st.analyze_reframe("ai".into(), &src, None, &ai, &|_| {});
    let second = async {
        // A newer analysis cancels the older one: the view shows one source at a time.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        st.analyze_reframe("newer".into(), &src, None, &cpu(Reframe::Smart), &|_| {}).await
    };
    let (first, second) = tokio::join!(first, second);
    assert!(first.unwrap_err().to_lowercase().contains("cancel"));
    assert!(second.unwrap().analysis.is_some());

    let events = Mutex::new(Vec::new());
    let a = st
        .analyze_reframe("a1".into(), &src, None, &cpu(Reframe::Smart), &|e| events.lock().unwrap().push(e))
        .await
        .unwrap()
        .analysis
        .expect("a landscape source is analysed");
    assert!(a.path.len() > 100 && a.crop.0 < a.source_size.0);
    let events = events.into_inner().unwrap();
    assert!(matches!(events.last(), Some(AnalysisEvent::Done)), "{events:?}");
    assert!(events.iter().any(|e| matches!(e, AnalysisEvent::Progress { .. })));

    // The CLI's dry run of the same job finds the analysis in the shared cache.
    let o = Command::new(cli_bin())
        .args(["upscale", "--tier", "cpu", "--dry-run", "--json"])
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let plan: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(plan["jobs"][0]["upscale"]["reframe_analysis"]["cache"], "hit", "{plan}");

    // The preview now follows the path instead of the centred guess.
    let p = st.upscale_preview(&src, None, 3.0, &cpu(Reframe::Smart)).await.unwrap();
    assert!(!p.approximate, "{p:?}");
}

/// Security: the models folder is chosen in Rust only, never by the page.
#[tokio::test(flavor = "multi_thread")]
async fn settings_from_the_page_cannot_set_the_models_folder() {
    let d = tempfile::tempdir().unwrap();
    let st = state(d.path()).await;
    let mut s = st.get_settings();
    s.models_dir = Some("/tmp/evil".into());
    st.set_settings(s).unwrap();
    assert_eq!(st.get_settings().models_dir, None);
    st.set_models_dir(Some(d.path().to_path_buf())).unwrap();
    assert_eq!(st.get_settings().models_dir.as_deref(), Some(d.path()));
    st.set_models_dir(None).unwrap();
    assert_eq!(st.get_settings().models_dir, None);
}

/// FR-022: an upscale queue run in the app and exported to `rmedia batch` gives identical files.
#[tokio::test(flavor = "multi_thread")]
async fn upscale_queue_and_exported_manifest_give_identical_outputs() {
    require_ffmpeg!();
    if !cli_bin().is_file() {
        panic!("build the CLI first (cargo build -p rmedia-cli) or set RMEDIA_CLI_BIN");
    }
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("a.mp4");
    std::fs::copy(fixture(Fixture::TinySource), &src).unwrap();
    let st = state(d.path()).await;
    st.allow(std::slice::from_ref(&src));
    let gui_out = d.path().join("gui");
    let full = |reframe: &str| json!({"preset":"phone-fill","reframe":reframe,"tier":"cpu","hdr":"sdr","motion":"normal"});
    let manifest = json!({"version":1,"defaults":{"output_dir": gui_out},"videos":[{"input": src, "clips":[
        {"start":0.5,"end":2,"options":{"upscale": full("center")}},
        {"start":0,"end":"end","options":{"upscale": full("blur")}},
        {"start":1,"end":2,"name":"plain"}]}]});

    let plan = st.plan(&manifest).await.unwrap();
    assert!(!plan.has_errors(), "{:?}", plan.issues);
    let summary = st.run(&plan.run_id, Arc::new(Collect::default())).await.unwrap();
    assert_eq!(summary.succeeded, 3, "{summary:?}");

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
        let twin = cli_out.join(output.strip_prefix(&gui_out).unwrap());
        assert_eq!(sha256(output), sha256(&twin), "{} differs between app and CLI", output.display());
    }
}
