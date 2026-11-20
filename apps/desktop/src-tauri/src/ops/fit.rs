//! Budgeted video (feature 005): the video targets and the clip panel's prediction.

use super::*;

impl AppState {
    /// Built-in and user video targets, with the files that could not be used and the default
    /// (`rmedia clip --list-fit-targets --json`).
    pub fn list_fit_targets(&self) -> Value {
        use rmedia_core::fit::target::{DEFAULT_TARGET, VideoTargetRegistry};
        let mut v = VideoTargetRegistry::load(rmedia_core::settings::config_dir().as_deref()).listing();
        v["default"] = Value::String(DEFAULT_TARGET.into());
        v
    }

    /// The fit one range would get (the clip panel's prediction, before queueing): the core's
    /// plan of a one-clip request, arithmetic only (research R1). Refusals such as
    /// `RANGE_TOO_LONG` come back as issues. A newer call cancels the older one; nothing is kept.
    pub async fn fit_plan(
        &self,
        path: &Path,
        span: RangeArg,
        options: &PartialOptions,
    ) -> OpResult<FitPlanResult> {
        let (start, end) = span.span()?;
        let path = self.require_allowed(path)?;
        let cancel = CancellationToken::new();
        if let Some(older) = lock(&self.fit_plan).replace(cancel.clone()) {
            older.cancel();
        }
        let result = async {
            use rmedia_core::request::{ClipRequest, OnExists, Origin, SourceRequest};
            let (tools, caps) = self.engine().await?;
            // The page's options over the Settings defaults (the core re-checks them). Nothing
            // is written: an existing output must not make the plan skip the job.
            let defaults = lock(&self.settings).defaults.apply(&Options::default());
            let req = BatchRequest {
                defaults: Options {
                    on_exists: OnExists::Overwrite,
                    output_dir: Some(self.cache_dir.join(FIT_PLANS)),
                    ..options.apply(&defaults)
                },
                sources: vec![SourceRequest {
                    path,
                    clips: vec![ClipRequest {
                        start,
                        end,
                        name: None,
                        overrides: PartialOptions::default(),
                        origin: Origin::Gui,
                    }],
                    overrides: PartialOptions::default(),
                    origin: Origin::Gui,
                }],
            };
            let mut p = plan::plan(&req, &tools, &caps, &cancel).await;
            if cancel.is_cancelled() {
                return Err("cancelled: a newer fit plan replaced this one".to_owned());
            }
            let job = p.jobs.pop();
            Ok(FitPlanResult {
                fit: job.as_ref().and_then(|j| j.fit.clone()),
                notices: job.map(|j| j.notices).unwrap_or_default(),
                issues: p.issues,
            })
        }
        .await;
        let mut current = lock(&self.fit_plan);
        if current.as_ref() == Some(&cancel) {
            *current = None;
        }
        result
    }
}

/// The folder a fit plan's (never written) output is named in.
const FIT_PLANS: &str = "fit-plans";

/// `fit_plan`'s result (contracts/tauri-commands.md).
#[derive(Debug, Clone, Serialize)]
pub struct FitPlanResult {
    /// `null` when the clip has no fit or it was refused.
    pub fit: Option<rmedia_core::fit::plan::FitSummary>,
    /// `FIT_LOWERED`, `FIT_TRIMMED`, `SMART_NOT_APPLIED`…
    pub notices: Vec<rmedia_core::plan::Notice>,
    /// Refusals (`RANGE_TOO_LONG`, `BUDGET_UNREACHABLE`, `ENGINE_MISSING`…) and other problems.
    pub issues: Vec<Issue>,
}
