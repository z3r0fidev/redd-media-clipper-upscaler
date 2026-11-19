//! The queue: folder scans, clip lists and manifests, planning and running, and serving a run's outputs.

use super::*;

impl AppState {
    pub async fn scan(
        &self,
        scan_id: String,
        roots: Vec<PathBuf>,
        mut opts: ScanOptions,
        on_event: &(dyn Fn(ScanEvent) + Send + Sync),
    ) -> OpResult<ScanResult> {
        let roots = roots.iter().map(|r| self.require_allowed(r)).collect::<OpResult<Vec<_>>>()?;
        // rmedia's own output folders are never sources, whatever the page sends (issue #37).
        opts.exclude_dir_names.extend(discover::DEFAULT_OUTPUT_DIRS.iter().map(|d| (*d).to_owned()));
        let (tools, _) = self.engine().await?;
        let task = self.scans.start(&scan_id, Others::Keep).map_err(|()| "this scan is already running")?;
        discover::scan(&tools, &roots, &opts, on_event, &task.cancel).await.map_err(|e| e.to_string())
    }

    pub fn cancel_scan(&self, scan_id: &str) {
        self.scans.cancel(scan_id);
    }

    /// A clip list applied to `sources`, returned as a manifest (the queue format).
    pub fn load_cliplist(&self, list: &Path, sources: &[PathBuf]) -> OpResult<Value> {
        if !lock(&self.allowed).is_list(list) {
            return Err("open the clip list with Import list…".into());
        }
        for s in sources {
            self.require_allowed(s)?;
        }
        let clips = cliplist::load(list).map_err(issues_text)?;
        let req = BatchRequest {
            defaults: Options::default(),
            sources: sources
                .iter()
                .map(|p| rmedia_core::request::SourceRequest {
                    path: p.clone(),
                    clips: clips.clone(),
                    overrides: PartialOptions::default(),
                    origin: rmedia_core::request::Origin::Gui,
                })
                .collect(),
        };
        manifest_value(&req, &PartialOptions::default())
    }

    /// Opens a manifest; its videos become readable sources (the user chose the manifest), but
    /// clips of them are written only to a folder the user picked, never next to them.
    pub fn load_manifest(&self, path: &Path) -> OpResult<Value> {
        if !lock(&self.allowed).is_list(path) {
            return Err("open the manifest with Open manifest…".into());
        }
        let req = match ext(path).as_str() {
            "json" => manifest_json::load(path, &Options::default()),
            "csv" => manifest_csv::load(path, &Options::default()),
            _ => return Err("choose a .json or .csv manifest".into()),
        }
        .map_err(issues_text)?;
        // A manifest may only grant access to video files it names (not arbitrary files).
        let videos: Vec<PathBuf> =
            req.sources.iter().map(|s| s.path.clone()).filter(|p| is_video_name(p) && p.is_file()).collect();
        {
            let mut a = lock(&self.allowed);
            for v in &videos {
                a.allow_from_manifest(v);
            }
        }
        // Keep the manifest's own defaults (e.g. output folder) in the queue.
        manifest_value(&req, &PartialOptions::diff(&req.defaults, &Options::default()))
    }

    /// Writes the queue to a path chosen in the save dialog (.json keeps every option).
    pub fn export_manifest(&self, manifest: &Value, path: &Path) -> OpResult<()> {
        if !lock(&self.allowed).is_save_target(path) {
            return Err("choose where to save with the Save dialog".into());
        }
        let req = self.queue_request(manifest)?;
        let text = match ext(path).as_str() {
            // CSV folds every resolved option into its rows.
            "csv" => manifest_csv::write(&req),
            // The queue is a v1 manifest; fold the app's Settings defaults into its `defaults`
            // (queue values win) so `rmedia batch` reproduces exactly what the app would run.
            _ => {
                serde_json::to_string_pretty(&with_settings_defaults(
                    manifest,
                    &lock(&self.settings).defaults,
                ))
                .map_err(|e| e.to_string())?
                    + "\n"
            }
        };
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    pub(super) fn queue_request(&self, manifest: &Value) -> OpResult<BatchRequest> {
        let defaults = lock(&self.settings).defaults.apply(&Options::default());
        let req = manifest_json::from_value(Path::new("/"), manifest, &defaults).map_err(issues_text)?;
        for s in &req.sources {
            self.require_allowed(&s.path)?;
        }
        Ok(req)
    }

    /// Validates the queue and plans it. The plan is kept for `run` under its run id.
    pub async fn plan(&self, manifest: &Value) -> OpResult<Plan> {
        let req = self.queue_request(manifest)?;
        let (tools, caps) = self.engine().await?;
        let mut plan = plan::plan(&req, &tools, &caps, &CancellationToken::new()).await;
        {
            let allowed = lock(&self.allowed);
            for job in &plan.jobs {
                if !allowed.may_write(&job.output, &job.source) {
                    plan.issues.push(Issue {
                        severity: rmedia_core::plan::Severity::Error,
                        code: rmedia_core::error::IssueCode::InvalidInput,
                        origin: rmedia_core::request::Origin::Gui,
                        message: format!(
                            "{} is outside the folders you chose; pick the output folder in Settings",
                            job.output.display()
                        ),
                        detail: None,
                    });
                }
            }
        }
        self.plans.insert(&plan.run_id, plan.clone());
        Ok(plan)
    }

    pub async fn run(&self, run_id: &str, sink: Arc<dyn EventSink>) -> OpResult<RunSummary> {
        let plan = self.plans.remove(run_id).ok_or("plan not found; plan the queue again")?;
        if plan.has_errors() {
            return Err("the queue has errors; fix them before running".into());
        }
        let (tools, _) = self.engine().await?;
        let jobs = lock(&self.settings).jobs.map_or_else(exec::default_jobs, usize::from);
        let handle = exec::start(plan, tools, RunConfig { jobs, ..RunConfig::default() }, sink);
        self.runs.insert(run_id, handle.control());
        // Estimates pause while a run encodes (they would compete for the CPU).
        self.estimates.cancel_all();
        let summary = handle.wait().await;
        self.runs.remove(run_id);
        let done: Vec<(u32, PathBuf)> = summary
            .jobs
            .iter()
            .filter_map(|j| match &j.outcome {
                rmedia_core::event::JobOutcome::Succeeded { output, .. } => Some((j.id, output.clone())),
                _ => None,
            })
            .collect();
        let mut outputs = lock(&self.outputs);
        outputs.push_back((run_id.to_owned(), done));
        while outputs.len() > MAX_RUNS_KEPT {
            outputs.pop_front();
        }
        drop(outputs);
        Ok(summary)
    }

    pub fn cancel_run(&self, run_id: &str, job: Option<u32>) {
        self.runs.with(run_id, |c| match job {
            Some(j) => c.cancel_job(j),
            None => c.cancel(),
        });
    }

    /// Registers a committed output of one of this app's runs with the media server. The page
    /// names the run and job; the path comes from the run's own outcome (contracts › Path policy).
    pub async fn open_output(&self, run_id: &str, job_id: u32) -> OpResult<OutputInfo> {
        let path = lock(&self.outputs)
            .iter()
            .find(|(id, _)| id == run_id)
            .and_then(|(_, jobs)| jobs.iter().find(|(id, _)| *id == job_id).map(|(_, p)| p.clone()))
            .ok_or("no such output of this app's runs")?;
        // The file itself, never a link swapped in: the parent resolved, the name kept, and the
        // registered (canonical) path must be exactly that.
        let not_ours = || format!("{} is no longer the file the run wrote", path.display());
        let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !meta.file_type().is_file() {
            return Err(not_ours());
        }
        let expected = match (path.parent().map(std::fs::canonicalize), path.file_name()) {
            (Some(Ok(dir)), Some(name)) => dir.join(name),
            _ => return Err(not_ours()),
        };
        let url = self.media.register(&path).map_err(|e| e.to_string())?;
        if std::fs::canonicalize(&path).ok().as_deref() != Some(expected.as_path()) {
            self.media.forget(&path);
            return Err(not_ours());
        }
        {
            let mut files = lock(&self.output_files);
            if !files.contains(&expected) {
                files.push_back(expected.clone());
            }
            while files.len() > MAX_PREVIEW_URLS {
                if let Some(old) = files.pop_front() {
                    self.media.forget(&old);
                }
            }
        }
        let (tools, _) = self.engine().await?;
        let info =
            probe::probe(&tools, &expected, &CancellationToken::new()).await.map_err(|e| e.to_string())?;
        let v = info.first_video().ok_or("the output has no video")?;
        let kind =
            if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("gif")) { "image" } else { "video" };
        Ok(OutputInfo {
            url,
            kind,
            bytes: meta.len(),
            width: v.width.unwrap_or(0),
            height: v.height.unwrap_or(0),
        })
    }
}

/// A run's id and its committed outputs by job id.
pub(super) type RunOutputs = (String, Vec<(u32, PathBuf)>);
/// How many runs' outputs `open_output` remembers.
pub(super) const MAX_RUNS_KEPT: usize = 16;

/// `open_output`'s result: a media-server URL for the page's `<img>` or `<video>`.
#[derive(Debug, Clone, Serialize)]
pub struct OutputInfo {
    pub url: String,
    pub kind: &'static str,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
}

/// The queue manifest with `defaults` = Settings defaults overlaid by the queue's own defaults.
pub(super) fn with_settings_defaults(manifest: &Value, settings: &PartialOptions) -> Value {
    let mut out = manifest.clone();
    let mut merged = serde_json::to_value(settings).unwrap_or_else(|_| Value::Object(Default::default()));
    if let (Some(m), Some(q)) = (merged.as_object_mut(), manifest.get("defaults").and_then(Value::as_object))
    {
        for (k, v) in q {
            m.insert(k.clone(), v.clone());
        }
    }
    if merged.as_object().is_some_and(|m| !m.is_empty()) {
        out["defaults"] = merged;
    }
    out
}

pub(super) fn is_video_name(p: &Path) -> bool {
    rmedia_core::discover::VIDEO_EXTENSIONS.contains(&ext(p).as_str())
}

/// Canonicalizes the nearest existing ancestor and re-appends the rest. `None` when the path has
/// a `..` or `.` step: kept after a missing folder it passes a lexical prefix check but resolves
/// elsewhere once the folders are created (`<picked>/nope/../../elsewhere`).
pub(super) fn resolve_existing(p: &Path) -> Option<PathBuf> {
    if p.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return None;
    }
    let mut existing = p.to_path_buf();
    let mut rest = Vec::new();
    while !existing.exists() {
        match (existing.file_name().map(ToOwned::to_owned), existing.parent()) {
            (Some(n), Some(parent)) => {
                rest.push(n);
                existing = parent.to_path_buf();
            }
            _ => break,
        }
    }
    let mut out = std::fs::canonicalize(&existing).unwrap_or(existing);
    for part in rest.iter().rev() {
        out.push(part);
    }
    Some(out)
}

pub(super) fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

pub(super) fn issues_text(issues: Vec<Issue>) -> String {
    issues.iter().map(|i| i.to_string()).collect::<Vec<_>>().join("\n")
}

/// Canonical manifest JSON (absolute paths) for the queue.
pub(super) fn manifest_value(req: &BatchRequest, defaults: &PartialOptions) -> OpResult<Value> {
    serde_json::from_str(&manifest_json::write(req, defaults)).map_err(|e| e.to_string())
}
