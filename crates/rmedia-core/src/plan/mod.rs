//! Turning a `BatchRequest` into concrete, validated jobs (no files are written here).

pub mod ffargs;
pub mod naming;
pub mod streams;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::error::{Error, IssueCode};
use crate::event::{JobId, Span};
use crate::exec::commit::{scratch_path, temp_path};
use crate::preset::{Preset, PresetRegistry, VideoCodec};
use crate::probe::{self, MediaInfo};
use crate::request::{
    BatchRequest, ClipRequest, DEFAULT_NAME_TEMPLATE, GIF_CLIP_NAME_TEMPLATE, GIF_MULTI_CLIP_NAME_TEMPLATE,
    GIF_NAME_TEMPLATE, MULTI_SOURCE_NAME_TEMPLATE, Mode, OnExists, Origin, SourceRequest,
    UPSCALE_CLIP_NAME_TEMPLATE, UPSCALE_MULTI_CLIP_NAME_TEMPLATE, UPSCALE_NAME_TEMPLATE,
};
use crate::time::{EndSpec, Ts};
use crate::tools::Tools;
use crate::upscale::encode::{self, ColourTags, EncodeParams};
use crate::upscale::geometry::{self, Overflow};
use crate::upscale::graph::{GraphInput, Track};
use crate::upscale::reframe;
use crate::upscale::tier::{Tier, TierDecision};
use crate::upscale::{AnalysisState, HdrMode, Reframe, UpscaleOptions, UpscalePlan, UpscaleSpec, estimate};
use ffargs::ClipSpec;
use streams::Container;

mod model;
mod planner;

pub use model::*;
use planner::*;

/// Filters smart reframe needs besides the common upscale ones (research R5, R6).
pub(crate) const SMART_FILTERS: [&str; 6] = ["sendcmd", "scdet", "metadata", "split", "overlay", "crop"];

/// How far past a smart job's end its keyframe index is read (feature 004).
const INDEX_RUNOUT: Ts = Ts::from_micros(3_000_000);

/// How far before the target an approximate (coarse) input seek lands.
const COARSE_SEEK_MARGIN: Ts = Ts::from_micros(10_000_000);

/// Validates every request and resolves outputs. Reports all problems at once (FR-003).
/// Upscale presets come from the built-ins plus the user's preset folder.
pub async fn plan(req: &BatchRequest, tools: &Tools, caps: &Caps, cancel: &CancellationToken) -> Plan {
    let config = crate::settings::config_dir();
    let presets = PresetRegistry::load(config.as_deref());
    let targets = crate::gif::target::GifTargetRegistry::load(config.as_deref());
    plan_with_targets(req, tools, caps, presets, targets, cancel).await
}

/// [`plan`] with an explicit preset registry and the built-in GIF targets (tests).
pub async fn plan_with(
    req: &BatchRequest,
    tools: &Tools,
    caps: &Caps,
    presets: PresetRegistry,
    cancel: &CancellationToken,
) -> Plan {
    plan_with_targets(req, tools, caps, presets, crate::gif::target::GifTargetRegistry::builtin(), cancel)
        .await
}

/// [`plan`] with explicit preset and GIF target registries.
pub async fn plan_with_targets(
    req: &BatchRequest,
    tools: &Tools,
    caps: &Caps,
    presets: PresetRegistry,
    targets: crate::gif::target::GifTargetRegistry,
    cancel: &CancellationToken,
) -> Plan {
    let mut planner = Planner {
        tools,
        caps,
        cancel,
        run_id: new_run_id(),
        jobs: Vec::new(),
        issues: Vec::new(),
        claimed: HashSet::new(),
        sources: HashSet::new(),
        probes: HashMap::new(),
        presets,
        targets,
    };
    for s in &req.sources {
        if let Ok(c) = std::fs::canonicalize(&s.path) {
            planner.sources.insert(path_key(&c));
        }
    }
    for source in &req.sources {
        planner.plan_source(req, source).await;
    }
    planner.check_free_space();
    Plan {
        run_id: planner.run_id,
        smart: SmartTotals::of(&planner.jobs),
        jobs: planner.jobs,
        issues: planner.issues,
        cancelled: cancel.is_cancelled(),
    }
}

/// Normalized comparison key (case-insensitive on Windows).
use crate::discover::path_key;
