//! GIF export (feature 003): engine paths, targets and size estimates.

use super::*;

impl AppState {
    /// Sets (or resets) a GIF engine's program. Only from the native file dialog in Rust, never
    /// from the page: a regular file (no symlink, no folder) that identifies itself as the engine.
    pub async fn set_gif_engine_path(&self, kind: EngineKind, path: Option<PathBuf>) -> OpResult<()> {
        let path = match path {
            None => None,
            Some(p) => {
                let meta = std::fs::symlink_metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                if !meta.file_type().is_file() {
                    return Err(format!(
                        "{} is not a program file (links and folders are refused)",
                        p.display()
                    ));
                }
                let status = rmedia_core::gif::engines::locate(kind, Some(&p), None, None, &[]).await;
                if !status.ok {
                    return Err(format!(
                        "{}: {}",
                        p.display(),
                        status.reason.unwrap_or_else(|| format!("not {}", kind.name()))
                    ));
                }
                Some(p)
            }
        };
        let _saving = lock(&self.settings_save);
        let mut s = lock(&self.settings).clone();
        match kind {
            EngineKind::Gifsicle => s.gifsicle_path = path,
            EngineKind::Gifski => s.gifski_path = path,
        }
        s.save().map_err(|e| e.to_string())?;
        *lock(&self.settings) = s;
        *self.engine.write().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(())
    }

    /// Built-in and user GIF targets for the GIF view (`rmedia gif --list-targets --json`).
    pub fn list_gif_targets(&self) -> Value {
        let reg =
            rmedia_core::gif::target::GifTargetRegistry::load(rmedia_core::settings::config_dir().as_deref());
        let targets: Vec<_> = reg.list().map(|(t, _)| t.clone()).collect();
        let problems: Vec<_> = reg
            .problems()
            .iter()
            .map(|p| serde_json::json!({"file": rmedia_core::pathser::lossy(&p.file), "message": p.message}))
            .collect();
        serde_json::json!({ "default": rmedia_core::gif::target::DEFAULT_TARGET, "targets": targets, "problems": problems })
    }

    /// The size a GIF job of `path` would have (research R4: the budget search's prediction from
    /// samples, nothing encoded in full), in a private folder under the app cache that is
    /// removed on every path. One estimate runs at a time; a run pauses them.
    pub async fn gif_estimate(
        &self,
        estimate_id: String,
        path: &Path,
        span: Option<RangeArg>,
        options: &GifOptions,
        on_event: &(dyn Fn(AnalysisEvent) + Send + Sync),
    ) -> OpResult<GifEstimate> {
        if estimate_id.is_empty() || estimate_id.len() > 64 {
            return Err("invalid estimate id".into());
        }
        let (start, end) = span.map(RangeArg::span).transpose()?.unwrap_or((Ts::ZERO, EndSpec::EndOfMedia));
        let path = self.require_allowed(path)?;
        let (tools, caps) = self.engine().await?;
        let task = {
            // Under the runs lock: a run that starts after this check cancels the estimate
            // (`run` registers itself, then cancels the estimates), and one already running
            // refuses it.
            let runs = self.runs.lock();
            if !runs.is_empty() {
                return Err("estimates wait while a run encodes".into());
            }
            self.estimates
                .start(&estimate_id, Others::Cancel)
                .map_err(|()| "this estimate is already running")?
        };
        let cancel = task.cancel.clone();
        let result = async {
            use rmedia_core::request::{ClipRequest, OnExists, Origin, SourceRequest};
            // The core re-checks every option (the page's values are untrusted). Nothing is
            // written: an existing output must not make the plan skip the job.
            let req = BatchRequest {
                defaults: Options {
                    gif: Some(options.clone()),
                    on_exists: OnExists::Overwrite,
                    output_dir: Some(self.cache_dir.join(GIF_ESTIMATES)),
                    ..Options::default()
                },
                sources: vec![SourceRequest {
                    path: path.clone(),
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
            let errors: Vec<String> = p
                .issues
                .iter()
                .filter(|i| i.severity == rmedia_core::plan::Severity::Error)
                .map(|i| i.message.clone())
                .collect();
            if !errors.is_empty() {
                return Err(errors.join("; "));
            }
            on_event(AnalysisEvent::Progress { fraction: 0.05 });
            let parent = self.cache_dir.join(GIF_ESTIMATES);
            std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
            rmedia_core::gif::estimate::prepare_in(&mut p, &tools, &cancel, &parent)
                .await
                .map_err(|e| e.to_string())?;
            let job = p.jobs.pop().ok_or("nothing to estimate")?;
            let JobSpec::Gif(g) = job.spec else { return Err("not a GIF job".into()) };
            // The `estimate` object of the JSON contract (json-output.md).
            Ok(GifEstimate {
                estimate: g.gif.to_json()["estimate"].clone(),
                notes: job.warnings,
                notices: job.notices,
            })
        }
        .await;
        drop(task);
        if result.is_ok() {
            on_event(AnalysisEvent::Done);
        }
        result
    }

    pub fn cancel_estimate(&self, estimate_id: &str) {
        self.estimates.cancel(estimate_id);
    }
}

/// The app-cache folder GIF estimates sample in.
pub(super) const GIF_ESTIMATES: &str = "gif-estimates";
/// `gif_estimate`'s result (contracts/tauri-commands.md).
#[derive(Debug, Clone, Serialize)]
pub struct GifEstimate {
    pub estimate: Value,
    pub notes: Vec<String>,
    pub notices: Vec<rmedia_core::plan::Notice>,
}
