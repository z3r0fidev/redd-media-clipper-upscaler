//! Smart cut (feature 004): keyframe indexes for the timeline and the smart plan of one range.

use super::*;

impl AppState {
    /// The whole-file keyframe index of an opened source (feature 004, research R1), for the
    /// timeline's markers. Served from the cache while the file's size and modification time are
    /// unchanged; capped by the core (`truncated`).
    pub async fn keyframes(&self, request_id: String, path: &Path) -> OpResult<KeyframeIndex> {
        if request_id.is_empty() || request_id.len() > 64 {
            return Err("invalid request id".into());
        }
        let path = self.require_allowed(path)?;
        if probe::is_playlist(&path) {
            return Err(format!("{} is a playlist/stream description, not a video file", path.display()));
        }
        let meta = std::fs::metadata(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let key = KeyframeKey { path: path.clone(), len: meta.len(), modified: meta.modified().ok() };
        if let Some((_, index)) = lock(&self.keyframe_cache).iter().find(|(k, _)| *k == key) {
            return Ok((**index).clone());
        }
        let registered = self
            .keyframe_requests
            .start(&request_id, Others::Keep)
            .map_err(|()| "this request is already running")?;
        let cancel = registered.cancel.clone();
        let result = async {
            // At most `MAX_KEYFRAME_SCANS` scans at once (item 27); waiting stays cancellable.
            let _slot = tokio::select! {
                slot = self.keyframe_slots.acquire() => slot.map_err(|e| e.to_string())?,
                () = cancel.cancelled() => return Err("the keyframe index was cancelled".to_owned()),
            };
            let (tools, _) = self.engine().await?;
            let media = probe::probe(&tools, &path, &cancel).await.map_err(|e| e.to_string())?;
            rmedia_core::smart::keyframes(&tools, &media, None, &cancel).await.map_err(|e| e.to_string())
        }
        .await;
        drop(registered);
        if cancel.is_cancelled() {
            return Err("the keyframe index was cancelled".into());
        }
        let index = Arc::new(result?);
        let mut cache = lock(&self.keyframe_cache);
        cache.retain(|(k, _)| k.path != key.path);
        cache.push_back((key, index.clone()));
        while cache.len() > MAX_KEYFRAME_INDEXES {
            cache.pop_front();
        }
        Ok((*index).clone())
    }

    /// Uncached keyframe scans running now (tests).
    pub fn keyframe_scans(&self) -> usize {
        MAX_KEYFRAME_SCANS - self.keyframe_slots.available_permits()
    }

    pub fn cancel_keyframes(&self, request_id: &str) {
        self.keyframe_requests.cancel(request_id);
    }

    /// What smart mode would do for one range of an opened source: the core's plan, as
    /// `rmedia clip --mode smart --dry-run` makes it (feature 004), with the job's notices (the
    /// fallback or "does not apply" message). No summary: smart does not apply (the options
    /// upscale or make a GIF). With `smart_fallback: false` a reason is an error. Nothing is
    /// written. A newer call cancels this one.
    pub async fn smart_plan(
        &self,
        path: &Path,
        span: RangeArg,
        options: &PartialOptions,
    ) -> OpResult<SmartPlanResult> {
        let (start, end) = span.span()?;
        let path = self.require_allowed(path)?;
        let cancel = CancellationToken::new();
        if let Some(older) = lock(&self.smart_plan).replace(cancel.clone()) {
            older.cancel();
        }
        let result = async {
            use rmedia_core::request::{ClipRequest, Mode, OnExists, Origin, SourceRequest};
            let (tools, caps) = self.engine().await?;
            // The page's options over the Settings defaults (the core re-checks them), in smart
            // mode. Nothing is written: an existing output must not make the plan skip the job.
            let defaults = lock(&self.settings).defaults.apply(&Options::default());
            let req = BatchRequest {
                defaults: Options {
                    mode: Mode::Smart,
                    on_exists: OnExists::Overwrite,
                    output_dir: Some(self.cache_dir.join(SMART_PLANS)),
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
                return Err("cancelled: a newer smart plan replaced this one".to_owned());
            }
            let errors: Vec<String> = p
                .issues
                .iter()
                .filter(|i| i.severity == rmedia_core::plan::Severity::Error)
                .map(|i| i.message.clone())
                .collect();
            if !errors.is_empty() {
                return Err(errors.join("; "));
            }
            let job = p.jobs.pop().ok_or("nothing to plan")?;
            Ok(SmartPlanResult { summary: job.smart, notices: job.notices })
        }
        .await;
        // Clear the slot unless a newer call already took it.
        let mut current = lock(&self.smart_plan);
        if current.as_ref() == Some(&cancel) {
            *current = None;
        }
        result
    }
}

/// The output folder smart plans are named in (nothing is written there).
pub(super) const SMART_PLANS: &str = "smart-plans";
pub(super) const MAX_KEYFRAME_INDEXES: usize = 32;
/// Uncached keyframe scans that may run at once; the frontend needs one (it cancels the old
/// source's), the second covers a cancel still winding down.
pub(super) const MAX_KEYFRAME_SCANS: usize = 2;

/// What a cached keyframe index was read from: a file with another size or time is read again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct KeyframeKey {
    path: PathBuf,
    len: u64,
    modified: Option<std::time::SystemTime>,
}
/// `smart_plan`'s result (contracts/tauri-commands.md, feature 004).
#[derive(Debug, Clone, Serialize)]
pub struct SmartPlanResult {
    pub summary: Option<rmedia_core::plan::SmartSummary>,
    pub notices: Vec<rmedia_core::plan::Notice>,
}
