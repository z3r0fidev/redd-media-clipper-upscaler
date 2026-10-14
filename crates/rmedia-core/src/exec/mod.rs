//! Job scheduler: concurrency, progress, cancellation, commit and cleanup.
//!
//! The scheduler never looks inside a job kind; `JobSpec` variants run through `JobCtx`, which
//! provides the temp output, progress reporting and cancellation. Feature 002/003 job kinds plug
//! in by adding a `JobSpec` variant and a match arm in `run_job`.

mod clip;
pub mod commit;
pub mod journal;
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
use crate::plan::{Disposition, JobSpec, Plan, PlannedJob};
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
    });
    shared.emit(Event::RunStarted {
        run_id: plan.run_id.clone(),
        total_jobs: plan.jobs.len(),
        total_media: Ts::from_secs_f64(shared.total_media),
    });

    let mut results: HashMap<JobId, JobResult> = HashMap::new();
    let semaphore = Arc::new(Semaphore::new(cfg.jobs.max(1)));
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
        let (sem, shared, tools, journal) =
            (semaphore.clone(), shared.clone(), tools.clone(), journal.clone());
        tasks.insert(job.id, job.clone());
        let task_job = job.id;
        let handle = set.spawn(async move {
            let outcome = tokio::select! {
                permit = sem.acquire_owned() => {
                    let _permit = permit.expect("semaphore open");
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
                let temp = job.temp.clone();
                let _ = tokio::task::spawn_blocking(move || commit::remove_with_retry(&temp)).await;
                let outcome = JobOutcome::Failed {
                    error: ErrorInfo {
                        code: "INTERNAL".into(),
                        message: format!("internal error while processing this clip: {e}"),
                        stderr_tail: None,
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

/// Context handed to a job kind's executor.
pub struct JobCtx<'a> {
    pub tools: &'a Tools,
    pub temp: &'a Path,
    pub output: &'a Path,
    pub cancel: &'a CancellationToken,
    id: JobId,
    shared: &'a Shared,
    throttle: Throttle,
}

impl std::fmt::Debug for JobCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobCtx").field("id", &self.id).field("temp", &self.temp).finish_non_exhaustive()
    }
}

impl JobCtx<'_> {
    /// Reports ffmpeg progress for a job producing `total` media time.
    pub fn progress(&mut self, block: ProgressBlock, total: Ts) {
        let Some(out_us) = block.out_time_us else { return };
        if !self.throttle.ready(block.done) {
            return;
        }
        let total_s = total.as_secs_f64();
        let fraction = if total_s > 0.0 { (out_us as f64 / 1e6 / total_s).clamp(0.0, 1.0) } else { 0.0 };
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
    for w in &job.warnings {
        shared.emit(Event::JobWarning { job_id: job.id, message: w.clone() });
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
    let mut ctx = JobCtx {
        tools,
        temp: &job.temp,
        output: &job.output,
        cancel,
        id: job.id,
        shared,
        throttle: Throttle::new(Duration::from_millis(100)),
    };
    let result = match &job.spec {
        JobSpec::Clip(spec) => clip::execute(spec, &mut ctx).await,
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
    let temp = job.temp.clone();
    blocking(move || commit::remove_with_retry(&temp)).await;
    if let Some(j) = journal {
        j.remove(&job.temp);
    }
    outcome
}

/// Runs blocking filesystem work (renames, deletes with retries) off the async workers.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f).await.expect("blocking filesystem task panicked")
}
