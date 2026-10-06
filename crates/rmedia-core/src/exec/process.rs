//! Spawning external tools so that cancelling, dropping, or killing rmedia also kills them.
//!
//! - Unix: each child leads its own process group (Ctrl-C in the terminal does not reach it; we
//!   kill the whole group). On Linux the child also gets `PR_SET_PDEATHSIG(SIGKILL)`, so it dies
//!   if rmedia is SIGKILLed. PDEATHSIG fires when the *spawning thread* exits, so children must be
//!   spawned from long-lived runtime worker threads, never from `spawn_blocking`.
//! - Windows: each child runs in a Job Object with kill-on-close and no console window.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::Path;
use std::process::{ExitStatus, Stdio};

use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};

const STDERR_TAIL_LINES: usize = 50;
/// ffprobe JSON for real media is far smaller; hostile files cannot make us buffer more.
const MAX_CAPTURE_BYTES: u64 = 16 * 1024 * 1024;
/// Longest stderr line kept (characters); longer lines are truncated.
const MAX_LINE_CHARS: usize = 4096;

#[derive(Debug)]
pub struct ProcessOutput {
    pub status: ExitStatus,
    /// Captured stdout (empty when stdout was consumed line by line).
    pub stdout: Vec<u8>,
    /// Last lines of stderr, for error messages.
    pub stderr_tail: String,
}

fn build(program: &Path, args: &[OsString]) -> CommandWrap {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(target_os = "linux")]
    set_parent_death_signal(&mut cmd);

    let mut wrap = CommandWrap::from(cmd);
    #[cfg(unix)]
    wrap.wrap(process_wrap::tokio::ProcessGroup::leader());
    #[cfg(windows)]
    {
        wrap.wrap(process_wrap::tokio::JobObject);
        wrap.wrap(process_wrap::tokio::CreationFlags(windows::Win32::System::Threading::CREATE_NO_WINDOW));
    }
    wrap.wrap(KillOnDrop);
    wrap
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn set_parent_death_signal(cmd: &mut tokio::process::Command) {
    let parent = std::process::id() as libc::pid_t;
    // SAFETY: the closure runs in the forked child before exec and only calls the
    // async-signal-safe functions prctl(2) and getppid(2); it does not allocate.
    unsafe {
        cmd.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // The parent may have died between fork and prctl.
            if libc::getppid() != parent {
                return Err(std::io::Error::from_raw_os_error(libc::ESRCH));
            }
            Ok(())
        });
    }
}

fn spawn(program: &Path, args: &[OsString]) -> Result<Box<dyn ChildWrapper>> {
    build(program, args).spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::EngineMissing(format!("{} not found", program.display()))
        } else {
            Error::Io(e)
        }
    })
}

/// Runs a tool to completion, capturing stdout. Kills the process tree on cancel.
pub async fn run_capture(
    program: &Path,
    args: &[OsString],
    cancel: &CancellationToken,
) -> Result<ProcessOutput> {
    let mut buf = Vec::new();
    let mut out = run(program, args, cancel, |stdout| async {
        let mut limited = stdout.take(MAX_CAPTURE_BYTES);
        limited.read_to_end(&mut buf).await?;
        if buf.len() as u64 >= MAX_CAPTURE_BYTES {
            return Err(std::io::Error::other("tool output exceeded 16 MiB"));
        }
        Ok(())
    })
    .await?;
    out.stdout = buf;
    Ok(out)
}

/// Runs a tool, passing each stdout line to `on_line`. Kills the process tree on cancel.
pub async fn run_lines(
    program: &Path,
    args: &[OsString],
    cancel: &CancellationToken,
    mut on_line: impl FnMut(&str) + Send,
) -> Result<ProcessOutput> {
    run(program, args, cancel, |stdout| async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Some(line) = lines.next_line().await? {
            on_line(&line);
        }
        Ok(())
    })
    .await
}

/// Spawns, drains stdout with `consume` (racing cancellation), then reaps the child.
async fn run<F, Fut>(
    program: &Path,
    args: &[OsString],
    cancel: &CancellationToken,
    consume: F,
) -> Result<ProcessOutput>
where
    F: FnOnce(tokio::process::ChildStdout) -> Fut,
    Fut: std::future::Future<Output = std::io::Result<()>>,
{
    let mut child = spawn(program, args)?;
    let stdout = child.stdout().take().expect("stdout is piped");
    let stderr_task = tokio::spawn(collect_stderr(child.stderr().take()));
    let drained = tokio::select! {
        r = consume(stdout) => Some(r),
        () = cancel.cancelled() => None,
    };
    match drained {
        None => {
            kill(child.as_mut()).await;
            stderr_task.abort();
            return Err(Error::Cancelled);
        }
        Some(Err(e)) => {
            kill(child.as_mut()).await;
            return Err(Error::Io(e));
        }
        Some(Ok(())) => {}
    }
    // stdout reached EOF, so the process is exiting; reaping is immediate.
    let status = child.wait().await?;
    let stderr_tail = stderr_task.await.unwrap_or_default();
    Ok(ProcessOutput { status, stdout: Vec::new(), stderr_tail })
}

async fn kill(child: &mut dyn ChildWrapper) {
    // Kills the whole process group / job, then reaps the child.
    let _ = Box::into_pin(child.kill()).await;
}

async fn collect_stderr(stderr: Option<tokio::process::ChildStderr>) -> String {
    let Some(stderr) = stderr else { return String::new() };
    let mut tail: VecDeque<String> = VecDeque::with_capacity(STDERR_TAIL_LINES);
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(mut line)) = lines.next_line().await {
        if line.len() > MAX_LINE_CHARS {
            let cut = (0..=MAX_LINE_CHARS).rev().find(|i| line.is_char_boundary(*i)).unwrap_or(0);
            line.truncate(cut);
        }
        if tail.len() == STDERR_TAIL_LINES {
            tail.pop_front();
        }
        tail.push_back(line);
    }
    tail.into_iter().collect::<Vec<_>>().join("\n")
}
