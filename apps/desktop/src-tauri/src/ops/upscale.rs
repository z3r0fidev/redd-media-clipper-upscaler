//! Upscaling (feature 002): presets, the smart-reframe analysis and the before/after still.

use super::*;

impl AppState {
    /// Built-in and user presets for the upscale view (`rmedia upscale --list-presets --json`).
    pub fn list_presets(&self) -> Value {
        let reg = PresetRegistry::load(rmedia_core::settings::config_dir().as_deref());
        let presets: Vec<_> = reg.list().map(|(p, _)| p.clone()).collect();
        serde_json::json!({ "default": reg.default_id(), "presets": presets, "problems": reg.problems() })
    }

    /// Smart-reframe analysis of one source (or range) for the crop overlay, through the same
    /// cache as the render and the CLI. `analysis` is `None` when the job has nothing to analyse
    /// (another reframe, matching shape, or no face model; `note` then says why). Starting an
    /// analysis cancels any other: the view shows one source at a time.
    pub async fn analyze_reframe(
        &self,
        analysis_id: String,
        path: &Path,
        span: Option<RangeArg>,
        options: &UpscaleOptions,
        on_event: &(dyn Fn(AnalysisEvent) + Send + Sync),
    ) -> OpResult<ReframeResult> {
        if analysis_id.is_empty() || analysis_id.len() > 64 {
            return Err("invalid analysis id".into());
        }
        let span = span.map(RangeArg::span).transpose()?;
        let path = self.require_allowed(path)?;
        let (tools, caps) = self.engine().await?;
        let task = self
            .analyses
            .start(&analysis_id, Others::Cancel)
            .map_err(|()| "this analysis is already running")?;
        let cancel = task.cancel.clone();
        // The analysis does not depend on the tier; the CPU tier always plans (an unavailable
        // tier must not stop the crop path from being found).
        let options = UpscaleOptions { tier: TierChoice::Cpu, ..options.clone() };
        let result = async {
            let presets = PresetRegistry::load(rmedia_core::settings::config_dir().as_deref());
            let (spec, warnings) =
                still::planned_job(&tools, &caps, presets, &path, span, &options, &cancel).await?;
            if spec.up.analysis != AnalysisState::Pending {
                let note =
                    (options.reframe == Reframe::Smart && !warnings.is_empty()).then(|| warnings.join(" · "));
                return Ok(ReframeResult { analysis: None, note });
            }
            let mut sent = -1.0;
            let (a, _) = reframe::analyze_cached(&tools, &spec.reframe_input(), &cancel, |f| {
                // At most one event per percent.
                if f - sent >= 0.01 || f >= 1.0 {
                    sent = f;
                    on_event(AnalysisEvent::Progress { fraction: f });
                }
            })
            .await?;
            Ok(ReframeResult { analysis: Some((*a).clone()), note: None })
        }
        .await;
        drop(task);
        if result.is_ok() {
            on_event(AnalysisEvent::Done);
        }
        result.map_err(|e: rmedia_core::Error| e.to_string())
    }

    pub fn cancel_analysis(&self, analysis_id: &str) {
        self.analyses.cancel(analysis_id);
    }

    /// The before/after still at source time `at`, as media-server URLs (the page never sees
    /// the files' paths). Stills live in the app cache.
    pub async fn upscale_preview(
        &self,
        path: &Path,
        span: Option<RangeArg>,
        at: f64,
        options: &UpscaleOptions,
    ) -> OpResult<UpscalePreview> {
        let span = span.map(RangeArg::span).transpose()?;
        let at = seconds(at)?;
        let path = self.require_allowed(path)?;
        let (tools, caps) = self.engine().await?;
        let presets = PresetRegistry::load(rmedia_core::settings::config_dir().as_deref());
        let req = PreviewRequest { path, span, at, options: options.clone() };
        let out = self.cache_dir.join("upscale-preview");
        // Only the latest preview matters: the one before it stops.
        let cancel = CancellationToken::new();
        if let Some(previous) = lock(&self.preview).replace(cancel.clone()) {
            previous.cancel();
        }
        let p = still::preview_frame(&tools, &caps, presets, &req, &out, &cancel)
            .await
            .map_err(|e| e.to_string())?;
        Ok(UpscalePreview {
            before: self.register_preview(&p.before)?,
            after: self.register_preview(&p.after)?,
            size: p.size,
            approximate: p.approximate,
            crop_x: p.crop_x,
            in_fallback: p.in_fallback,
            classical: p.classical,
        })
    }
}

/// `analyze_reframe`'s result: the analysis, or why there is none.
#[derive(Debug, Clone, Serialize)]
pub struct ReframeResult {
    pub analysis: Option<ReframeAnalysis>,
    pub note: Option<String>,
}

/// The before/after still as media-server URLs.
#[derive(Debug, Clone, Serialize)]
pub struct UpscalePreview {
    pub before: String,
    pub after: String,
    pub size: (u32, u32),
    pub approximate: bool,
    pub crop_x: Option<u32>,
    pub in_fallback: bool,
    /// AI tier: the "after" still comes from the standard scaler.
    pub classical: bool,
}
