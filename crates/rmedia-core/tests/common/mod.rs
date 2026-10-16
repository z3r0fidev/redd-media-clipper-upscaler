#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rmedia_core::event::{Event, EventSink, RunSummary};
use rmedia_core::exec::{self, RunConfig};
use rmedia_core::plan::{self, Caps, Plan};
use rmedia_core::request::{BatchRequest, ClipRequest, Options, Origin, PartialOptions, SourceRequest};
use rmedia_core::tools::{LocateOptions, Tools};
use rmedia_core::{EndSpec, Ts};
use tokio_util::sync::CancellationToken;

pub fn engine() -> (Tools, Caps) {
    let (report, found) = rmedia_core::doctor::run(&LocateOptions::default());
    found.unwrap_or_else(|| panic!("engine unavailable: {:?}", report.problems))
}

pub fn secs(s: f64) -> Ts {
    Ts::from_secs_f64(s)
}

pub fn clip(start: f64, end: EndSpec) -> ClipRequest {
    ClipRequest {
        start: secs(start),
        end,
        name: None,
        overrides: PartialOptions::default(),
        origin: Origin::Cli,
    }
}

pub fn request(source: &Path, clips: Vec<ClipRequest>, defaults: Options) -> BatchRequest {
    BatchRequest {
        defaults,
        sources: vec![SourceRequest {
            path: source.to_path_buf(),
            clips,
            overrides: PartialOptions::default(),
            origin: Origin::Cli,
        }],
    }
}

pub fn opts(out: &Path, mode: rmedia_core::request::Mode) -> Options {
    Options { mode, output_dir: Some(out.to_path_buf()), ..Options::default() }
}

#[derive(Default)]
pub struct Collect(pub Mutex<Vec<Event>>);

impl EventSink for Collect {
    fn emit(&self, e: Event) {
        self.0.lock().unwrap().push(e);
    }
}

/// Plans with the built-in presets only, so a developer's own preset folder never leaks in.
pub async fn make_plan(req: &BatchRequest, tools: &Tools, caps: &Caps) -> Plan {
    let presets = rmedia_core::preset::PresetRegistry::builtin();
    plan::plan_with(req, tools, caps, presets, &CancellationToken::new()).await
}

pub async fn run_plan(p: Plan, tools: Tools, state: &Path) -> (RunSummary, Vec<Event>) {
    let sink = Arc::new(Collect::default());
    let cfg = RunConfig { jobs: 2, state_dir: Some(state.to_path_buf()) };
    let summary = exec::start(p, tools, cfg, sink.clone()).wait().await;
    let events = std::mem::take(&mut *sink.0.lock().unwrap());
    (summary, events)
}

/// Files under `dir` whose name ends in `.part`.
pub fn partials(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                found.extend(partials(&p));
            } else if p.extension().is_some_and(|x| x == "part") {
                found.push(p);
            }
        }
    }
    found
}

/// A model file fetched into the workspace `models/` folder (`cargo xtask fetch-models`).
/// Release builds never search that folder by themselves, so tests ask for it explicitly.
pub fn workspace_model(name: &str) -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models").join(name);
    p.is_file().then_some(p)
}

/// The real YuNet detector from the workspace `models/` folder.
pub fn yunet() -> Option<rmedia_core::upscale::reframe::detect::DetectorSpec> {
    workspace_model(rmedia_core::models::YUNET)
        .map(rmedia_core::upscale::reframe::detect::DetectorSpec::Yunet)
}
