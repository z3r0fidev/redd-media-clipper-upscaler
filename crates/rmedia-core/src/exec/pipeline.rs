//! The AI tier's pipeline (research R13): ffmpeg #1 (decode, frame, raw RGB) → `rmedia-sr` →
//! ffmpeg #2 (scale, compose, encode). The frames go from process to process through OS pipes;
//! rmedia never copies them. All three are killed together on cancel or when one fails
//! (Principle IV): one process group on Unix, one kill-on-close Job Object each on Windows.

use std::path::Path;
use std::process::{ExitStatus, Stdio};

use process_wrap::tokio::ChildWrapper;
use tokio::io::BufReader;

use super::JobCtx;
use super::process::{self, Group};
use super::progress::ProgressParser;
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

/// What the worker said on stderr (JSON lines, contracts/rmedia-sr-protocol.md).
#[derive(Debug, Default)]
struct WorkerReport {
    /// The `error` line: (code, message).
    error: Option<(String, String)>,
    tail: Vec<String>,
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

async fn worker_report(stderr: Option<tokio::process::ChildStderr>) -> WorkerReport {
    let mut report = WorkerReport::default();
    if let Some(stderr) = stderr {
        let mut reader = BufReader::new(stderr);
        while let Ok(Some(line)) = process::next_line_bounded(&mut reader, MAX_LINE * 4).await {
            report.feed(&line);
        }
    }
    report
}

/// Spawns the three stages and stops all of them if any part fails.
struct Stages {
    children: Vec<Box<dyn ChildWrapper>>,
    pgid: u32,
}

impl Stages {
    fn spawn(ffmpeg: &Path, worker: &Path, args: &AiArgs, cwd: Option<&Path>) -> Result<Stages> {
        let mut decode = process::spawn_stage(ffmpeg, &args.decode, cwd, Stdio::null(), Group::Lead)?;
        let pgid = decode.id().unwrap_or(0);
        let mut stages = Stages { children: Vec::with_capacity(3), pgid };
        let frames: Stdio = decode.stdout().take().expect("stdout is piped").try_into()?;
        stages.children.push(decode);
        let mut sr = process::spawn_stage(worker, &args.worker, cwd, frames, Group::Join(pgid))?;
        let upscaled: Stdio = sr.stdout().take().expect("stdout is piped").try_into()?;
        stages.children.push(sr);
        let encode = process::spawn_stage(ffmpeg, &args.encode, cwd, upscaled, Group::Join(pgid))?;
        stages.children.push(encode);
        Ok(stages)
    }
}

impl Drop for Stages {
    fn drop(&mut self) {
        // An error while spawning, or the job future dropped: nothing may outlive the job.
        let alive: Vec<bool> = self.children.iter_mut().map(|c| matches!(c.try_wait(), Ok(None))).collect();
        if alive.iter().any(|a| *a) {
            #[cfg(unix)]
            process::kill_group(self.pgid);
            for (child, alive) in self.children.iter_mut().zip(alive) {
                if alive {
                    let _ = child.start_kill();
                }
            }
        }
    }
}

/// Runs the pipeline in `cwd` (the job's scratch folder: the `sendcmd` files are named
/// relatively), reporting ffmpeg #2's progress against `total`.
pub(super) async fn run(
    ctx: &mut JobCtx<'_>,
    worker: &Path,
    args: &AiArgs,
    cwd: Option<&Path>,
    total: Ts,
) -> Result<()> {
    let ffmpeg = ctx.tools.ffmpeg.path.clone();
    let cancel = ctx.cancel.clone();
    let mut stages = Stages::spawn(&ffmpeg, worker, args, cwd)?;

    let decode_err = tokio::spawn(process::collect_stderr(stages.children[DECODE].stderr().take()));
    let worker_err = tokio::spawn(worker_report(stages.children[WORKER].stderr().take()));
    let encode_err = tokio::spawn(process::collect_stderr(stages.children[ENCODE].stderr().take()));
    let (tx, mut progress) = tokio::sync::mpsc::unbounded_channel();
    if let Some(stdout) = stages.children[ENCODE].stdout().take() {
        tokio::spawn(async move {
            let mut parser = ProgressParser::default();
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(line)) = process::next_line_bounded(&mut reader, MAX_LINE).await {
                if let Some(block) = parser.feed(&line) {
                    let _ = tx.send(block);
                }
            }
        });
    }

    let mut status: [Option<ExitStatus>; 3] = [None; 3];
    // Stages killed by us (their exit says nothing about what went wrong).
    let mut killed = [false; 3];
    let mut cancelled = false;
    let mut progress_open = true;
    let pgid = stages.pgid;
    {
        let [d, w, e] = &mut stages.children[..] else { unreachable!("three stages") };
        while status.iter().any(Option::is_none) {
            let running = [status[0].is_none(), status[1].is_none(), status[2].is_none()];
            let (i, r) = tokio::select! {
                () = cancel.cancelled(), if !cancelled => {
                    cancelled = true;
                    kill_running(pgid, [d, w, e], &mut status, &mut killed);
                    continue;
                }
                block = progress.recv(), if progress_open => {
                    match block {
                        Some(b) => ctx.progress(b, total),
                        None => progress_open = false,
                    }
                    continue;
                }
                r = d.wait(), if running[DECODE] => (DECODE, r),
                r = w.wait(), if running[WORKER] => (WORKER, r),
                r = e.wait(), if running[ENCODE] => (ENCODE, r),
            };
            let st = r?;
            status[i] = Some(st);
            if !st.success() && !cancelled {
                kill_running(pgid, [d, w, e], &mut status, &mut killed);
            }
        }
    }
    while let Ok(b) = progress.try_recv() {
        ctx.progress(b, total);
    }
    let decode_tail = decode_err.await.unwrap_or_default();
    let report = worker_err.await.unwrap_or_default();
    let encode_tail = encode_err.await.unwrap_or_default();
    if cancelled {
        return Err(Error::Cancelled);
    }
    let failed = |i: usize| status[i].is_some_and(|s| !s.success()) && !killed[i];
    let cause = blame(
        failed(DECODE),
        failed(WORKER).then(|| report.error.as_ref().map(|e| e.0.as_str())),
        failed(ENCODE),
        status[ENCODE].is_some_and(|s| s.success()),
    );
    match cause {
        None => Ok(()),
        Some(ENCODE) => Err(super::clip::classify_failure(&encode_tail, ctx.output)),
        Some(WORKER) => {
            let msg = match &report.error {
                Some((code, message)) => format!("the AI worker failed ({code}): {message}"),
                None => format!("the AI worker stopped unexpectedly ({})", describe(status[WORKER])),
            };
            Err(Error::EngineFailed { message: msg, stderr_tail: report.tail.join("\n") })
        }
        Some(i) => Err(Error::EngineFailed {
            message: format!("{} failed ({})", NAMES[i], describe(status[i])),
            stderr_tail: decode_tail,
        }),
    }
}

/// Kills the stages still running and marks them killed. A stage that has already exited (but
/// was not reaped yet) keeps its own status: its failure may be the cause. On Unix the whole
/// group goes: ffmpeg #1 leads it, and its id stays the group's while any member is unreaped
/// (callers stop once all three are reaped, so the id cannot have been reused).
fn kill_running(
    pgid: u32,
    mut children: [&mut Box<dyn ChildWrapper>; 3],
    status: &mut [Option<ExitStatus>; 3],
    killed: &mut [bool; 3],
) {
    for (i, child) in children.iter_mut().enumerate() {
        if status[i].is_none()
            && let Ok(Some(st)) = child.try_wait()
        {
            status[i] = Some(st);
        }
    }
    let running = [status[0].is_none(), status[1].is_none(), status[2].is_none()];
    #[cfg(unix)]
    if running.iter().any(|r| *r) {
        process::kill_group(pgid);
    }
    #[cfg(not(unix))]
    let _ = pgid;
    for (i, child) in children.into_iter().enumerate() {
        if running[i] {
            killed[i] = true;
            let _ = child.start_kill();
        }
    }
}

/// Which stage caused a failure. A stage downstream of a failure sees a closed pipe and fails
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

fn describe(status: Option<ExitStatus>) -> String {
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
}
