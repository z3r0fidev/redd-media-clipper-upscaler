//! Job scheduler: concurrency, progress, cancellation, commit and cleanup.
//!
//! The scheduler never looks inside a job kind; `JobSpec` variants run through `JobCtx`, which
//! provides the temp output, progress reporting and cancellation. Feature 002/003 job kinds plug
//! in by adding a `JobSpec` variant and a match arm in `run_job`.

mod clip;
pub mod commit;
mod gif;
pub mod journal;
mod pipeline;
pub mod process;
pub mod progress;
mod upscale;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::error::{Error, ErrorInfo};
use crate::event::{Event, EventSink, JobId, JobOutcome, JobResult, RunSummary};
use crate::plan::{Disposition, JobSpec, Plan, PlannedJob, Resource};
use crate::time::Ts;
use crate::tools::Tools;
use journal::Journal;
use progress::{ProgressBlock, Throttle};

#[derive(Debug, Clone)]
pub struct RunConfig {
    pub jobs: usize,
    /// Where the crash journal lives; `None` disables it.
    pub state_dir: Option<PathBuf>,
}

impl Default for RunConfig {
    fn default() -> Self {
        RunConfig { jobs: default_jobs(), state_dir: crate::settings::state_dir() }
    }
}

/// Half the CPU cores, between 1 and 4.
pub fn default_jobs() -> usize {
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    (cpus / 2).clamp(1, 4)
}

/// Cancels a run or individual jobs; cheap to clone.
#[derive(Debug, Clone)]
pub struct RunControl {
    cancel: CancellationToken,
    job_tokens: HashMap<JobId, CancellationToken>,
}

impl RunControl {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn cancel_job(&self, id: JobId) {
        if let Some(t) = self.job_tokens.get(&id) {
            t.cancel();
        }
    }
}

/// Handle to a running batch.
#[derive(Debug)]
pub struct RunHandle {
    cancel: CancellationToken,
    job_tokens: HashMap<JobId, CancellationToken>,
    task: tokio::task::JoinHandle<RunSummary>,
}

impl RunHandle {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn cancel_job(&self, id: JobId) {
        if let Some(t) = self.job_tokens.get(&id) {
            t.cancel();
        }
    }

    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// A cloneable handle for cancelling the run or single jobs while `wait()` is pending.
    pub fn control(&self) -> RunControl {
        RunControl { cancel: self.cancel.clone(), job_tokens: self.job_tokens.clone() }
    }

    pub async fn wait(self) -> RunSummary {
        self.task.await.expect("run task panicked")
    }
}

/// Starts executing a plan. Must be called within a Tokio runtime.
pub fn start(plan: Plan, tools: Tools, cfg: RunConfig, sink: Arc<dyn EventSink>) -> RunHandle {
    let cancel = CancellationToken::new();
    let job_tokens: HashMap<JobId, CancellationToken> =
        plan.jobs.iter().map(|j| (j.id, cancel.child_token())).collect();
    let task = tokio::spawn(run(plan, tools, cfg, sink, cancel.clone(), job_tokens.clone()));
    RunHandle { cancel, job_tokens, task }
}

struct Shared {
    sink: Arc<dyn EventSink>,
    started: Instant,
    total_jobs: usize,
    total_media: f64,
    state: Mutex<RunState>,
    /// The run's state folder (crash journal, capability record).
    state_dir: Option<PathBuf>,
}

#[derive(Default)]
struct RunState {
    done: usize,
    finished_media: f64,
    running: HashMap<JobId, (f64, f64)>, // (fraction, media secs)
    throttle: Option<Throttle>,
}

impl Shared {
    fn emit(&self, e: Event) {
        self.sink.emit(e);
    }

    fn state(&self) -> std::sync::MutexGuard<'_, RunState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    // RunProgress is emitted while holding the state lock so consumers see it in order.
    fn job_progress(&self, id: JobId, fraction: f64, media: f64) {
        let mut s = self.state();
        s.running.insert(id, (fraction, media));
        let throttle = s.throttle.get_or_insert_with(|| Throttle::new(Duration::from_millis(250)));
        if throttle.ready(false) {
            let e = self.run_progress(&s);
            self.emit(e);
        }
    }

    fn job_done(&self, id: JobId, media: f64) {
        let mut s = self.state();
        s.running.remove(&id);
        s.done += 1;
        s.finished_media += media;
        let e = self.run_progress(&s);
        self.emit(e);
    }

    fn run_progress(&self, s: &RunState) -> Event {
        let running: f64 = s.running.values().map(|(f, m)| f * m).sum();
        let fraction = if self.total_media > 0.0 {
            ((s.finished_media + running) / self.total_media).clamp(0.0, 1.0)
        } else if self.total_jobs > 0 {
            s.done as f64 / self.total_jobs as f64
        } else {
            1.0
        };
        let elapsed = self.started.elapsed().as_secs_f64();
        let eta_ms = (fraction > 0.02 && fraction < 1.0)
            .then(|| (elapsed * (1.0 - fraction) / fraction * 1000.0) as u64);
        Event::RunProgress { done: s.done, total: self.total_jobs, fraction, eta_ms }
    }
}

async fn run(
    plan: Plan,
    tools: Tools,
    cfg: RunConfig,
    sink: Arc<dyn EventSink>,
    cancel: CancellationToken,
    job_tokens: HashMap<JobId, CancellationToken>,
) -> RunSummary {
    let journal = cfg.state_dir.as_deref().and_then(|d| {
        journal::purge_stale(d);
        Journal::open(d, &plan.run_id).map_err(|e| tracing::warn!("crash journal unavailable: {e}")).ok()
    });
    let journal = Arc::new(journal);
    let to_run: Vec<&PlannedJob> = plan.jobs.iter().filter(|j| j.disposition == Disposition::Run).collect();
    let shared = Arc::new(Shared {
        sink,
        started: Instant::now(),
        total_jobs: plan.jobs.len(),
        total_media: to_run.iter().map(|j| j.media_len().as_secs_f64()).sum(),
        state: Mutex::new(RunState::default()),
        state_dir: cfg.state_dir.clone(),
    });
    shared.emit(Event::RunStarted {
        run_id: plan.run_id.clone(),
        total_jobs: plan.jobs.len(),
        total_media: Ts::from_secs_f64(shared.total_media),
    });

    let mut results: HashMap<JobId, JobResult> = HashMap::new();
    let slots = Arc::new(Slots::new(cfg.jobs));
    let tools = Arc::new(tools);
    let mut set = JoinSet::new();
    let mut tasks: HashMap<JobId, PlannedJob> = HashMap::new();
    let mut task_ids: HashMap<tokio::task::Id, JobId> = HashMap::new();

    for job in &plan.jobs {
        if let Disposition::Skip { reason } = &job.disposition {
            let outcome = JobOutcome::Skipped { reason: reason.clone() };
            shared.emit(Event::JobFinished { job_id: job.id, outcome: outcome.clone() });
            shared.job_done(job.id, 0.0);
            results.insert(job.id, result_of(job, outcome));
            continue;
        }
        let job = job.clone();
        let token = job_tokens[&job.id].clone();
        let (slots, shared, tools, journal) = (slots.clone(), shared.clone(), tools.clone(), journal.clone());
        tasks.insert(job.id, job.clone());
        let task_job = job.id;
        let handle = set.spawn(async move {
            let outcome = tokio::select! {
                permits = slots.acquire(job.resource()) => {
                    let _permits = permits;
                    run_job(&job, &tools, (*journal).as_ref(), &token, &shared).await
                }
                () = token.cancelled() => JobOutcome::Cancelled,
            };
            shared.emit(Event::JobFinished { job_id: job.id, outcome: outcome.clone() });
            shared.job_done(job.id, job.media_len().as_secs_f64());
            result_of(&job, outcome)
        });
        task_ids.insert(handle.id(), task_job);
    }
    while let Some(r) = set.join_next_with_id().await {
        match r {
            Ok((_, res)) => {
                results.insert(res.id, res);
            }
            Err(e) => {
                tracing::error!("job task failed: {e}");
                let Some(job) = task_ids.get(&e.id()).and_then(|id| tasks.get(id)) else { continue };
                let (temp, scratch) = (job.temp.clone(), job.scratch.clone());
                let _ = tokio::task::spawn_blocking(move || {
                    commit::remove_with_retry(&temp);
                    if let Some(s) = scratch {
                        commit::remove_with_retry(&s);
                    }
                })
                .await;
                let outcome = JobOutcome::Failed {
                    error: ErrorInfo {
                        code: "INTERNAL".into(),
                        message: format!("internal error while processing this clip: {e}"),
                        stderr_tail: None,
                        detail: None,
                    },
                };
                shared.emit(Event::JobFinished { job_id: job.id, outcome: outcome.clone() });
                shared.job_done(job.id, job.media_len().as_secs_f64());
                results.insert(job.id, result_of(job, outcome));
            }
        }
    }

    let mut jobs: Vec<JobResult> = results.into_values().collect();
    jobs.sort_by_key(|j| j.id);
    let count = |f: fn(&JobOutcome) -> bool| jobs.iter().filter(|j| f(&j.outcome)).count();
    let mut summary = RunSummary {
        succeeded: count(|o| matches!(o, JobOutcome::Succeeded { .. })),
        skipped: count(|o| matches!(o, JobOutcome::Skipped { .. })),
        failed: count(|o| matches!(o, JobOutcome::Failed { .. })),
        cancelled: count(|o| matches!(o, JobOutcome::Cancelled)),
        exit_code: 0,
        jobs,
    };
    summary.exit_code = if cancel.is_cancelled() {
        130
    } else if summary.failed > 0 || summary.cancelled > 0 {
        1
    } else {
        0
    };
    shared.emit(Event::RunFinished { summary: summary.clone() });
    summary
}

fn result_of(job: &PlannedJob, outcome: JobOutcome) -> JobResult {
    let output = match &outcome {
        JobOutcome::Succeeded { output, .. } => output.clone(),
        _ => job.output.clone(),
    };
    JobResult { id: job.id, source: job.source.clone(), output, requested: job.requested, outcome }
}

/// The stage after `prev`: it starts where `prev` ended.
fn next_stage(prev: Option<(f64, f64)>, weight: f64) -> (f64, f64) {
    (prev.map_or(0.0, |(b, w)| b + w).min(1.0), weight)
}

/// Maps progress within the current stage to progress of the whole job.
fn job_fraction(stage: Option<(f64, f64)>, stage_fraction: f64) -> f64 {
    match stage {
        None => stage_fraction,
        Some((base, weight)) => (base + stage_fraction * weight).clamp(0.0, 1.0),
    }
}

/// Job slots: `--jobs` general slots plus one GPU slot (refactor A3).
#[derive(Debug)]
struct Slots {
    cpu: Arc<Semaphore>,
    gpu: Arc<Semaphore>,
}

impl Slots {
    fn new(jobs: usize) -> Slots {
        Slots { cpu: Arc::new(Semaphore::new(jobs.max(1))), gpu: Arc::new(Semaphore::new(1)) }
    }

    /// Waits for the permits `resource` needs. The GPU slot is taken first, so a GPU job that is
    /// waiting for the device does not hold a general slot meanwhile.
    async fn acquire(&self, resource: Resource) -> Vec<tokio::sync::OwnedSemaphorePermit> {
        let mut permits = Vec::with_capacity(2);
        if resource == Resource::Gpu {
            permits.push(self.gpu.clone().acquire_owned().await.expect("semaphore open"));
        }
        permits.push(self.cpu.clone().acquire_owned().await.expect("semaphore open"));
        permits
    }
}

/// Context handed to a job kind's executor.
pub struct JobCtx<'a> {
    pub tools: &'a Tools,
    pub temp: &'a Path,
    pub output: &'a Path,
    pub cancel: &'a CancellationToken,
    scratch: Option<&'a Path>,
    /// Current stage as (start of the stage within the whole job, its weight); `None` until the
    /// executor calls [`JobCtx::stage`], in which case progress maps one-to-one.
    stage: Option<(f64, f64)>,
    /// Last job fraction reported, so a repeated stage never moves the bar backwards.
    last_fraction: f64,
    id: JobId,
    shared: &'a Shared,
    throttle: Throttle,
}

impl std::fmt::Debug for JobCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobCtx").field("id", &self.id).field("temp", &self.temp).finish_non_exhaustive()
    }
}

impl<'a> JobCtx<'a> {
    /// The job's scratch directory, when the plan asked for one (it exists while the job runs).
    pub fn scratch_dir(&self) -> Option<&'a Path> {
        self.scratch
    }

    /// Runs ffmpeg with `args` in `cwd`, reporting its `-progress` output against `total` media
    /// time. The caller checks the exit status.
    pub async fn run_ffmpeg_in(
        &mut self,
        cwd: Option<&Path>,
        args: &[std::ffi::OsString],
        total: Ts,
    ) -> crate::error::Result<process::ProcessOutput> {
        let mut parser = progress::ProgressParser::default();
        let (ffmpeg, cancel) = (self.tools.ffmpeg.path.clone(), self.cancel);
        process::run_lines_in(&ffmpeg, args, cwd, cancel, |line| {
            if let Some(block) = parser.feed(line) {
                self.progress(block, total);
            }
        })
        .await
    }

    /// Enters stage `index` of `of` (1-based), which accounts for `weight` of the job's progress.
    /// Weights of a job's stages should add up to 1.
    pub fn stage(&mut self, name: &str, index: u32, of: u32, weight: f64) {
        self.stage = Some(next_stage(self.stage, weight));
        self.shared.emit(Event::JobStage { job_id: self.id, stage: name.to_owned(), index, of });
    }

    /// Starts the current stage over (a retry): progress continues from where the bar is and
    /// fills the rest of the stage's share, so it never moves backwards.
    pub fn restart_stage(&mut self, name: &str, index: u32, of: u32) {
        if let Some((base, weight)) = self.stage {
            let from = self.last_fraction.clamp(base, base + weight);
            self.stage = Some((from, base + weight - from));
        }
        self.shared.emit(Event::JobStage { job_id: self.id, stage: name.to_owned(), index, of });
    }

    /// The run's state folder, where the capability record lives.
    pub fn state_dir(&self) -> Option<&Path> {
        self.shared.state_dir.as_deref()
    }

    /// Reports a warning for this job.
    pub fn warn(&self, message: String) {
        self.shared.emit(Event::JobWarning { job_id: self.id, message, code: None });
    }

    /// Reports a warning that has a code (feature 003).
    pub fn warn_code(&self, code: crate::error::IssueCode, message: String) {
        self.shared.emit(Event::JobWarning { job_id: self.id, message, code: Some(code) });
    }

    /// Reports progress within the current stage as a fraction (for stages without ffmpeg
    /// `-progress`, such as the reframe analysis).
    pub fn stage_fraction(&mut self, fraction: f64, total: Ts) {
        let us = (fraction.clamp(0.0, 1.0) * total.as_secs_f64() * 1e6) as i64;
        self.progress(ProgressBlock { out_time_us: Some(us), speed: None, done: fraction >= 1.0 }, total);
    }

    /// Reports ffmpeg progress for a job producing `total` media time (within the current stage).
    pub fn progress(&mut self, block: ProgressBlock, total: Ts) {
        let Some(out_us) = block.out_time_us else { return };
        if !self.throttle.ready(block.done) {
            return;
        }
        let total_s = total.as_secs_f64();
        let fraction = if total_s > 0.0 { (out_us as f64 / 1e6 / total_s).clamp(0.0, 1.0) } else { 0.0 };
        let fraction = job_fraction(self.stage, fraction).max(self.last_fraction);
        self.last_fraction = fraction;
        let eta_ms = block
            .speed
            .filter(|s| *s > 0.0)
            .map(|speed| (((total_s - out_us as f64 / 1e6).max(0.0) / speed as f64) * 1000.0) as u64);
        self.shared.emit(Event::JobProgress {
            job_id: self.id,
            fraction,
            out_time: Ts::from_micros(out_us),
            speed: block.speed,
            eta_ms,
        });
        self.shared.job_progress(self.id, fraction, total_s);
    }
}

async fn run_job(
    job: &PlannedJob,
    tools: &Tools,
    journal: Option<&Journal>,
    cancel: &CancellationToken,
    shared: &Shared,
) -> JobOutcome {
    if cancel.is_cancelled() {
        return JobOutcome::Cancelled;
    }
    shared.emit(Event::JobStarted { job_id: job.id });
    // Warnings that have a code (notices) are sent with it; the rest as in 001/002.
    for w in &job.warnings {
        let code = job.notices.iter().find(|n| &n.message == w).map(|n| n.code);
        shared.emit(Event::JobWarning { job_id: job.id, message: w.clone(), code });
    }
    if let Some(dir) = job.temp.parent().map(Path::to_path_buf) {
        let made = blocking(move || std::fs::create_dir_all(&dir).map_err(|e| (dir, e))).await;
        if let Err((dir, e)) = made {
            let err = Error::OutputUnwritable { path: dir, message: e.to_string() };
            return JobOutcome::Failed { error: err.info() };
        }
    }
    if let Some(j) = journal {
        j.add(&job.temp);
    }
    if let Some(scratch) = job.scratch.clone() {
        // Journal first, so a crash between the two never leaves an unrecorded directory.
        if let Some(j) = journal {
            j.add(&scratch);
        }
        let made = blocking(move || commit::create_scratch(&scratch).map_err(|e| (scratch, e))).await;
        if let Err((dir, e)) = made {
            if let Some(j) = journal {
                j.remove(&dir);
                j.remove(&job.temp);
            }
            let err = Error::OutputUnwritable { path: dir, message: e.to_string() };
            return JobOutcome::Failed { error: err.info() };
        }
    }
    let mut ctx = JobCtx {
        tools,
        temp: &job.temp,
        output: &job.output,
        cancel,
        scratch: job.scratch.as_deref(),
        stage: None,
        last_fraction: 0.0,
        id: job.id,
        shared,
        throttle: Throttle::new(Duration::from_millis(100)),
    };
    let result = match &job.spec {
        JobSpec::Clip(spec) => clip::execute(spec, &mut ctx).await,
        JobSpec::Upscale(spec) => upscale::execute(spec, &mut ctx).await,
    };
    let committed = match result {
        Ok(measure) => {
            let (temp, output, policy) = (job.temp.clone(), job.output.clone(), job.on_exists);
            Ok((measure, blocking(move || commit::commit(&temp, &output, policy)).await))
        }
        Err(e) => Err(e),
    };
    let outcome = match committed {
        Ok((measure, committed)) => match committed {
            Ok(output) => JobOutcome::Succeeded { output, measure },
            Err(Error::OutputExists(_)) => {
                JobOutcome::Skipped { reason: "output appeared while the clip was being made".into() }
            }
            Err(e) => JobOutcome::Failed { error: e.info() },
        },
        Err(Error::Cancelled) => JobOutcome::Cancelled,
        Err(e) => JobOutcome::Failed { error: e.info() },
    };
    let (temp, scratch) = (job.temp.clone(), job.scratch.clone());
    blocking(move || {
        commit::remove_with_retry(&temp);
        if let Some(s) = scratch {
            commit::remove_with_retry(&s);
        }
    })
    .await;
    if let Some(j) = journal {
        j.remove(&job.temp);
        if let Some(s) = &job.scratch {
            j.remove(s);
        }
    }
    outcome
}

/// Runs blocking filesystem work (renames, deletes with retries) off the async workers.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f).await.expect("blocking filesystem task panicked")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn gpu_jobs_never_overlap_but_cpu_jobs_still_run() {
        let slots = Slots::new(4);
        let first = slots.acquire(Resource::Gpu).await;
        let second = tokio::time::timeout(Duration::from_millis(50), slots.acquire(Resource::Gpu)).await;
        assert!(second.is_err(), "a second GPU job must wait for the device");
        let cpu = tokio::time::timeout(Duration::from_millis(50), slots.acquire(Resource::Cpu)).await;
        assert!(cpu.is_ok(), "CPU jobs are not blocked by a running GPU job");
        drop(first);
        let second = tokio::time::timeout(Duration::from_millis(50), slots.acquire(Resource::Gpu)).await;
        assert!(second.is_ok(), "the GPU slot is free again");
    }

    #[test]
    fn stage_weights_map_to_whole_job_progress() {
        assert_eq!(job_fraction(None, 0.4), 0.4, "single-stage jobs (clips) map one-to-one");
        let analyze = next_stage(None, 0.25);
        assert_eq!(job_fraction(Some(analyze), 0.0), 0.0);
        assert_eq!(job_fraction(Some(analyze), 1.0), 0.25);
        let render = next_stage(Some(analyze), 0.7);
        assert_eq!(job_fraction(Some(render), 0.5), 0.6);
        let verify = next_stage(Some(render), 0.05);
        assert!((job_fraction(Some(verify), 1.0) - 1.0).abs() < 1e-9);
        assert_eq!(job_fraction(Some((0.9, 0.5)), 1.0), 1.0, "never above 1");
    }

    #[tokio::test]
    async fn general_slots_bound_all_jobs() {
        let slots = Slots::new(1);
        let _gpu = slots.acquire(Resource::Gpu).await;
        let cpu = tokio::time::timeout(Duration::from_millis(50), slots.acquire(Resource::Cpu)).await;
        assert!(cpu.is_err(), "--jobs 1 runs one job at a time, GPU or not");
    }
}
