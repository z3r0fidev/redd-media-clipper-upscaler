//! Process pipelines (refactor B3, research R7): stages joined by OS pipes, run and killed
//! together (Principle IV): one process group on Unix, one kill-on-close Job Object each on
//! Windows (killed consumer first). rmedia never copies the frames.
//!
//! Two pipelines use it:
//! - the AI tier (002 research R13): ffmpeg #1 (decode, frame, raw RGB) → `rmedia-sr` →
//!   ffmpeg #2 (scale, compose, encode), with the worker's error codes refining the blame;
//! - gifski (003): ffmpeg (frame chain, y4m) → gifski.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use process_wrap::tokio::ChildWrapper;
use tokio::io::BufReader;
use tokio_util::sync::CancellationToken;

use super::JobCtx;
use super::process::{self, Group};
use super::progress::{ProgressBlock, ProgressParser};
use crate::error::{Error, Result};
use crate::plan::ffargs::AiArgs;
use crate::time::Ts;

const DECODE: usize = 0;
const WORKER: usize = 1;
const ENCODE: usize = 2;
const NAMES: [&str; 3] = ["decoding the source (ffmpeg #1)", "the AI worker", "encoding (ffmpeg #2)"];
/// Worker stderr lines kept for error messages.
const TAIL_LINES: usize = 30;
const MAX_LINE: usize = 2048;
/// How often stage exits and the progress file are checked.
const POLL: Duration = Duration::from_millis(25);
const PROGRESS_POLL: Duration = Duration::from_millis(250);

/// A stage's place in the pipe: it decides how its failure is read (research R7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    /// Writes frames into the pipe (stdin closed).
    Producer,
    /// Reads frames and writes frames.
    Filter,
    /// Reads frames and writes the output file.
    Consumer,
}

/// How a stage's stderr is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Report {
    /// The last lines (ffmpeg).
    Tail,
    /// The last lines, stripped of control characters (less trusted programs).
    #[allow(dead_code)] // gifski (T043)
    CleanTail,
    /// The AI worker's JSON lines (contracts/rmedia-sr-protocol.md).
    Worker,
}

/// One process of a pipeline.
#[derive(Debug, Clone)]
pub(super) struct StageSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub role: Role,
    pub report: Report,
    /// Only these variables (`None`: rmedia's environment).
    pub env: Option<Vec<(OsString, OsString)>>,
}

/// Where a pipeline's progress comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ProgressFrom {
    /// The consumer's stdout carries ffmpeg `-progress pipe:1` (the AI tier's ffmpeg #2).
    ConsumerStdout,
    /// The producer writes ffmpeg `-progress` to this file (its stdout carries the frames);
    /// it is read every 250 ms.
    #[allow(dead_code)] // gifski (T043)
    File(PathBuf),
}

/// What happened to one stage.
#[derive(Debug, Default)]
pub(super) struct StageEnd {
    pub status: Option<ExitStatus>,
    /// Killed by us (its exit says nothing about what went wrong).
    pub killed: bool,
    pub tail: String,
    pub worker: WorkerReport,
}

impl StageEnd {
    /// It exited unsuccessfully on its own.
    pub fn failed(&self) -> bool {
        self.status.is_some_and(|s| !s.success()) && !self.killed
    }
}

/// What the worker said on stderr (JSON lines, contracts/rmedia-sr-protocol.md).
#[derive(Debug, Default)]
pub(super) struct WorkerReport {
    /// The `error` line: (code, message).
    pub error: Option<(String, String)>,
    pub tail: Vec<String>,
}

impl WorkerReport {
    fn feed(&mut self, line: &str) {
        let line: String = line.chars().take(MAX_LINE).collect();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line)
            && v["type"] == "error"
        {
            // The worker is less trusted: no control characters reach terminals or logs.
            let s = |k: &str| {
                v[k].as_str().unwrap_or_default().chars().filter(|c| !c.is_control()).take(500).collect()
            };
            self.error = Some((s("code"), s("message")));
        }
        if self.tail.len() == TAIL_LINES {
            self.tail.remove(0);
        }
        self.tail.push(line.chars().filter(|c| !c.is_control()).collect());
    }
}

async fn read_report(stderr: Option<tokio::process::ChildStderr>, report: Report) -> (String, WorkerReport) {
    match report {
        Report::Tail => (process::collect_stderr(stderr).await, WorkerReport::default()),
        Report::CleanTail => {
            let tail = process::collect_stderr(stderr).await;
            let clean = tail.lines().map(|l| l.chars().filter(|c| !c.is_control()).collect::<String>());
            (clean.collect::<Vec<_>>().join("\n"), WorkerReport::default())
        }
        Report::Worker => {
            let mut w = WorkerReport::default();
            if let Some(stderr) = stderr {
                let mut reader = BufReader::new(stderr);
                while let Ok(Some(line)) = process::next_line_bounded(&mut reader, MAX_LINE * 4).await {
                    w.feed(&line);
                }
            }
            (w.tail.join("\n"), w)
        }
    }
}

/// The running stages; nothing outlives the pipeline, even when the future is dropped.
struct Running {
    children: Vec<Box<dyn ChildWrapper>>,
    pgid: u32,
}

impl Running {
    fn spawn(stages: &[StageSpec], cwd: Option<&Path>) -> Result<Running> {
        let mut running = Running { children: Vec::with_capacity(stages.len()), pgid: 0 };
        let mut input = Stdio::null();
        for (i, s) in stages.iter().enumerate() {
            let group = if i == 0 { Group::Lead } else { Group::Join(running.pgid) };
            let mut child = process::spawn_stage(&s.program, &s.args, cwd, input, group, s.env.as_deref())?;
            if i == 0 {
                running.pgid = child.id().unwrap_or(0);
            }
            input = if i + 1 < stages.len() {
                child.stdout().take().expect("stdout is piped").try_into()?
            } else {
                Stdio::null()
            };
            running.children.push(child);
        }
        Ok(running)
    }

    /// Kills the stages still running (consumer first) and marks them killed. A stage that has
    /// exited already (but was not reaped yet) keeps its own status: its failure may be the
    /// cause. On Unix the whole group goes: stage 0 leads it, and its id stays the group's while
    /// any member is unreaped (callers stop once all are reaped, so the id cannot be reused).
    fn kill_running(&mut self, ends: &mut [StageEnd]) {
        for (child, end) in self.children.iter_mut().zip(ends.iter_mut()) {
            if end.status.is_none()
                && let Ok(Some(st)) = child.try_wait()
            {
                end.status = Some(st);
            }
        }
        #[cfg(unix)]
        if ends.iter().any(|e| e.status.is_none()) {
            process::kill_group(self.pgid);
        }
        for (child, end) in self.children.iter_mut().zip(ends.iter_mut()).rev() {
            if end.status.is_none() {
                end.killed = true;
                let _ = child.start_kill();
            }
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let alive: Vec<bool> = self.children.iter_mut().map(|c| matches!(c.try_wait(), Ok(None))).collect();
        if alive.iter().any(|a| *a) {
            #[cfg(unix)]
            process::kill_group(self.pgid);
            for (child, alive) in self.children.iter_mut().zip(alive).rev() {
                if alive {
                    let _ = child.start_kill();
                }
            }
        }
    }
}

/// Runs `stages` in `cwd`, passing progress blocks to `on_progress`. When a stage fails on its
/// own, the others are killed. Returns how each stage ended, or `Cancelled`.
pub(super) async fn run_stages(
    stages: &[StageSpec],
    cwd: Option<&Path>,
    progress: &ProgressFrom,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(ProgressBlock),
) -> Result<Vec<StageEnd>> {
    debug_assert!(
        stages.first().is_some_and(|s| s.role == Role::Producer)
            && stages.last().is_some_and(|s| s.role == Role::Consumer)
            && stages[1..stages.len() - 1].iter().all(|s| s.role == Role::Filter),
        "a pipe is a producer, filters, then a consumer"
    );
    let mut running = Running::spawn(stages, cwd)?;
    let reports: Vec<_> = running
        .children
        .iter_mut()
        .zip(stages)
        .map(|(c, s)| tokio::spawn(read_report(c.stderr().take(), s.report)))
        .collect();
    let (tx, mut blocks) = tokio::sync::mpsc::unbounded_channel();
    // The last stage's stdout is always drained (a full pipe would stall it).
    if let Some(stdout) = running.children.last_mut().and_then(|c| c.stdout().take()) {
        let parse = *progress == ProgressFrom::ConsumerStdout;
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut parser = ProgressParser::default();
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(line)) = process::next_line_bounded(&mut reader, MAX_LINE).await {
                if parse && let Some(block) = parser.feed(&line) {
                    let _ = tx.send(block);
                }
            }
        });
    }
    let tail_file = match progress {
        ProgressFrom::File(path) => Some(tokio::spawn(tail_progress(path.clone(), tx))),
        ProgressFrom::ConsumerStdout => {
            drop(tx);
            None
        }
    };

    let mut ends: Vec<StageEnd> = stages.iter().map(|_| StageEnd::default()).collect();
    let mut cancelled = false;
    let mut poll = tokio::time::interval(POLL);
    while ends.iter().any(|e| e.status.is_none()) {
        tokio::select! {
            () = cancel.cancelled(), if !cancelled => {
                cancelled = true;
                running.kill_running(&mut ends);
                continue;
            }
            Some(block) = blocks.recv() => {
                on_progress(block);
                continue;
            }
            _ = poll.tick() => {}
        }
        let mut failed = false;
        for (child, end) in running.children.iter_mut().zip(ends.iter_mut()) {
            if end.status.is_none()
                && let Some(st) = child.try_wait()?
            {
                end.status = Some(st);
                failed |= !st.success();
            }
        }
        if failed && !cancelled {
            running.kill_running(&mut ends);
        }
    }
    if let Some(t) = tail_file {
        t.abort();
    }
    while let Ok(b) = blocks.try_recv() {
        on_progress(b);
    }
    for (end, report) in ends.iter_mut().zip(reports) {
        let (tail, worker) = report.await.unwrap_or_default();
        end.tail = tail;
        end.worker = worker;
    }
    if cancelled {
        return Err(Error::Cancelled);
    }
    Ok(ends)
}

/// Follows an ffmpeg `-progress` file, sending each block (the file may not exist yet).
async fn tail_progress(path: PathBuf, tx: tokio::sync::mpsc::UnboundedSender<ProgressBlock>) {
    let mut parser = ProgressParser::default();
    let mut offset = 0usize;
    let mut partial = String::new();
    loop {
        tokio::time::sleep(PROGRESS_POLL).await;
        let Ok(data) = tokio::fs::read(&path).await else { continue };
        if data.len() <= offset {
            continue;
        }
        partial.push_str(&String::from_utf8_lossy(&data[offset..]));
        offset = data.len();
        while let Some(i) = partial.find('\n') {
            let line: String = partial.drain(..=i).collect();
            if let Some(block) = parser.feed(line.trim_end()) {
                let _ = tx.send(block);
            }
        }
    }
}

/// A stage that only lost its reader: SIGPIPE, or ffmpeg reporting a broken pipe.
#[allow(dead_code)] // gifski (T043)
fn lost_reader(end: &StageEnd) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if end.status.and_then(|s| s.signal()) == Some(libc::SIGPIPE) {
            return true;
        }
    }
    end.tail.contains("Broken pipe") || end.tail.contains("broken pipe") || end.tail.contains("End of file")
}

/// The cause of a producer → consumer pipe's failure (research R7): a producer that failed on
/// its own is the cause, even when the consumer exited 0 (its output is from a cut-short
/// stream); otherwise a failed consumer is; a producer that only lost its reader is not.
#[allow(dead_code)] // gifski (T043)
pub(super) fn blame_pair(producer: &StageEnd, consumer: &StageEnd) -> Option<usize> {
    if producer.failed() && !lost_reader(producer) {
        Some(0)
    } else if consumer.failed() {
        Some(1)
    } else {
        None
    }
}

/// Runs the AI pipeline in `cwd` (the job's scratch folder: the `sendcmd` files are named
/// relatively), reporting ffmpeg #2's progress against `total`.
pub(super) async fn run(
    ctx: &mut JobCtx<'_>,
    worker: &Path,
    args: &AiArgs,
    cwd: Option<&Path>,
    total: Ts,
) -> Result<()> {
    let ffmpeg = ctx.tools.ffmpeg.path.clone();
    let stage = |program: &Path, args: &[OsString], role, report| StageSpec {
        program: program.to_path_buf(),
        args: args.to_vec(),
        role,
        report,
        env: None,
    };
    let stages = [
        stage(&ffmpeg, &args.decode, Role::Producer, Report::Tail),
        stage(worker, &args.worker, Role::Filter, Report::Worker),
        stage(&ffmpeg, &args.encode, Role::Consumer, Report::Tail),
    ];
    let cancel = ctx.cancel.clone();
    let ends =
        run_stages(&stages, cwd, &ProgressFrom::ConsumerStdout, &cancel, |b| ctx.progress(b, total)).await?;
    let cause = blame(
        ends[DECODE].failed(),
        ends[WORKER].failed().then(|| ends[WORKER].worker.error.as_ref().map(|e| e.0.as_str())),
        ends[ENCODE].failed(),
        ends[ENCODE].status.is_some_and(|s| s.success()),
    );
    match cause {
        None => Ok(()),
        Some(ENCODE) => Err(super::clip::classify_failure(&ends[ENCODE].tail, ctx.output)),
        Some(WORKER) => {
            let report = &ends[WORKER].worker;
            let msg = match &report.error {
                Some((code, message)) => format!("the AI worker failed ({code}): {message}"),
                None => format!("the AI worker stopped unexpectedly ({})", describe(ends[WORKER].status)),
            };
            Err(Error::engine_failed(msg, report.tail.join("\n")))
        }
        Some(i) => Err(Error::engine_failed(
            format!("{} failed ({})", NAMES[i], describe(ends[i].status)),
            ends[i].tail.clone(),
        )),
    }
}

/// Which AI stage caused a failure. A stage downstream of a failure sees a closed pipe and fails
/// too; the worker's error code tells the two apart (`WRITE`: ffmpeg #2 stopped reading;
/// `SHORT_FRAME`/`READ`: ffmpeg #1 stopped mid-frame). `worker` is `Some(code)` when the worker
/// failed (`None` inside when it reported no error line, e.g. a crash). When ffmpeg #2 finished
/// cleanly (`encode_ok`), stages upstream that only lost their reader are not failures (the
/// verify stage checks the output's length).
fn blame(decode: bool, worker: Option<Option<&str>>, encode: bool, encode_ok: bool) -> Option<usize> {
    if encode_ok && matches!(worker, None | Some(Some("WRITE"))) {
        return None;
    }
    match worker {
        Some(Some("WRITE")) if encode => Some(ENCODE),
        Some(Some("SHORT_FRAME" | "READ")) if decode => Some(DECODE),
        Some(_) => Some(WORKER),
        None if encode => Some(ENCODE),
        None if decode => Some(DECODE),
        None => None,
    }
}

pub(super) fn describe(status: Option<ExitStatus>) -> String {
    match status {
        Some(s) => s.code().map_or_else(|| "killed by a signal".to_owned(), |c| format!("exit code {c}")),
        None => "did not finish".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_cause_is_blamed_not_the_closed_pipe() {
        assert_eq!(blame(false, None, false, true), None);
        assert_eq!(blame(false, Some(Some("GPU_RUNTIME")), false, true), Some(WORKER));
        assert_eq!(
            blame(true, Some(None), false, false),
            Some(WORKER),
            "a crashed worker breaks ffmpeg #1's pipe"
        );
        assert_eq!(blame(false, Some(Some("WRITE")), true, false), Some(ENCODE), "ffmpeg #2 stopped reading");
        assert_eq!(
            blame(true, Some(Some("SHORT_FRAME")), false, false),
            Some(DECODE),
            "ffmpeg #1 stopped mid-frame"
        );
        assert_eq!(blame(true, None, false, false), Some(DECODE));
        assert_eq!(blame(false, None, true, false), Some(ENCODE));
        // ffmpeg #2 finished early (its -t): the producers only lost their reader.
        assert_eq!(blame(true, Some(Some("WRITE")), false, true), None);
        assert_eq!(blame(true, None, false, true), None);
    }

    #[test]
    fn worker_errors_are_read_from_json_lines() {
        let mut r = WorkerReport::default();
        r.feed(r#"{"type":"device","index":0,"name":"GPU"}"#);
        r.feed("not json");
        r.feed(r#"{"type":"error","code":"GPU_RUNTIME","message":"lost device"}"#);
        assert_eq!(r.error, Some(("GPU_RUNTIME".into(), "lost device".into())));
        assert_eq!(r.tail.len(), 3);
    }

    // --- 2-stage pipes (003, T020) with fake stages: this test binary run as a program -------

    /// Set to a stage's behaviour when this binary runs as a fake stage.
    const FAKE: &str = "RMEDIA_FAKE_STAGE";

    /// The fake stages. When `RMEDIA_FAKE_STAGE` is set, this test is the whole program:
    /// - `produce:<bytes>:<exit>[:<progress file>]` writes y4m-like bytes, then exits;
    /// - `produce-forever` writes until it is killed;
    /// - `consume:<exit>` reads stdin to the end, then exits.
    #[test]
    fn fake_stage() {
        use std::io::{Read, Write};
        let Some(mode) = std::env::var_os(FAKE) else { return };
        let mode = mode.to_string_lossy().into_owned();
        let parts: Vec<&str> = mode.split(':').collect();
        match parts.as_slice() {
            ["produce", bytes, code, rest @ ..] => {
                let mut out = std::io::stdout().lock();
                let n: usize = bytes.parse().unwrap();
                let frame = b"FRAME\n".repeat(1000);
                let mut written = 0;
                while written < n {
                    let k = frame.len().min(n - written);
                    if out.write_all(&frame[..k]).is_err() {
                        eprintln!("av_interleaved_write_frame(): Broken pipe");
                        std::process::exit(1);
                    }
                    written += k;
                }
                let _ = out.flush();
                if let Some(path) = rest.first() {
                    std::fs::write(path, "out_time_us=1000000\nspeed=2.0x\nprogress=end\n").unwrap();
                }
                std::process::exit(code.parse().unwrap());
            }
            ["produce-forever"] => {
                let mut out = std::io::stdout().lock();
                loop {
                    if out.write_all(b"FRAME\n").is_err() {
                        std::process::exit(1);
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            ["consume", code] => {
                let mut sink = Vec::new();
                let _ = std::io::stdin().read_to_end(&mut sink);
                eprintln!("consumed {} bytes", sink.len());
                std::process::exit(code.parse().unwrap());
            }
            other => panic!("unknown fake stage {other:?}"),
        }
    }

    fn fake(mode: &str, role: Role) -> StageSpec {
        let me = std::env::current_exe().unwrap();
        let args = ["--exact", "exec::pipeline::tests::fake_stage", "--nocapture", "--test-threads=1"];
        StageSpec {
            program: me,
            args: args.iter().map(OsString::from).collect(),
            role,
            report: Report::CleanTail,
            env: Some(
                crate::gif::engines::engine_env().into_iter().chain([(FAKE.into(), mode.into())]).collect(),
            ),
        }
    }

    async fn pair(
        producer: &str,
        consumer: &str,
        progress: ProgressFrom,
    ) -> (Result<Vec<StageEnd>>, Vec<ProgressBlock>) {
        let stages = [fake(producer, Role::Producer), fake(consumer, Role::Consumer)];
        let mut blocks = Vec::new();
        let r = run_stages(&stages, None, &progress, &CancellationToken::new(), |b| blocks.push(b)).await;
        (r, blocks)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_two_stage_pipe_runs_and_reports_file_progress() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("progress.log");
        let producer = format!("produce:200000:0:{}", log.display());
        let (ends, blocks) = pair(&producer, "consume:0", ProgressFrom::File(log)).await;
        let ends = ends.unwrap();
        assert!(ends.iter().all(|e| e.status.is_some_and(|s| s.success())), "{ends:?}");
        assert_eq!(blame_pair(&ends[0], &ends[1]), None);
        assert!(ends[1].tail.contains("consumed"), "{}", ends[1].tail);
        // The file is read every 250 ms; the producer wrote it just before exiting, so the last
        // poll may miss it: progress is best-effort, never required.
        assert!(blocks.iter().all(|b| b.out_time_us == Some(1_000_000)), "{blocks:?}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_consumer_failure_beats_the_producers_broken_pipe() {
        let (ends, _) = pair("produce:50000000:0", "consume:3", ProgressFrom::ConsumerStdout).await;
        let ends = ends.unwrap();
        assert!(ends[1].failed());
        assert_eq!(blame_pair(&ends[0], &ends[1]), Some(1), "{ends:?}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_producer_failure_discards_the_consumers_success() {
        let (ends, _) = pair("produce:1000:2", "consume:0", ProgressFrom::ConsumerStdout).await;
        let ends = ends.unwrap();
        // The consumer saw a short stream: it exited 0, or was stopped when the producer failed.
        assert!(!ends[1].failed(), "{ends:?}");
        assert_eq!(blame_pair(&ends[0], &ends[1]), Some(0), "{ends:?}");
        let ok = StageEnd { status: ends[1].status, killed: false, ..StageEnd::default() };
        let exited_0 = StageEnd { status: Some(success()), ..ok };
        assert_eq!(
            blame_pair(&ends[0], &exited_0),
            Some(0),
            "a consumer's exit 0 does not clear the producer"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_kills_both_stages_within_two_seconds() {
        let stages = [fake("produce-forever", Role::Producer), fake("consume:0", Role::Consumer)];
        let cancel = CancellationToken::new();
        let later = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            later.cancel();
        });
        let started = std::time::Instant::now();
        let r = run_stages(&stages, None, &ProgressFrom::ConsumerStdout, &cancel, |_| {}).await;
        assert!(matches!(r, Err(Error::Cancelled)), "{r:?}");
        assert!(started.elapsed() < Duration::from_millis(2500), "took {:?}", started.elapsed());
    }

    fn success() -> ExitStatus {
        #[cfg(unix)]
        return std::os::unix::process::ExitStatusExt::from_raw(0);
        #[cfg(windows)]
        return std::os::windows::process::ExitStatusExt::from_raw(0);
    }

    #[test]
    fn lost_readers_are_recognised() {
        let end = |tail: &str| StageEnd { tail: tail.into(), ..StageEnd::default() };
        assert!(lost_reader(&end("av_interleaved_write_frame(): Broken pipe")));
        assert!(!lost_reader(&end("Invalid data found when processing input")));
    }
}
