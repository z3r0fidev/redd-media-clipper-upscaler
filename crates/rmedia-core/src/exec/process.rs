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

/// How a child joins a process group (Unix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Group {
    /// Leads its own group; the wrapper kills and reaps the whole group (single tools).
    Own,
    /// Leads a group other pipeline stages join; each stage is reaped on its own.
    Lead,
    /// Joins the group led by this pid.
    Join(u32),
}

/// The environment a child gets: rmedia's own (ffmpeg, the AI worker), or only the listed
/// variables (the optional GIF engines, research R7).
pub(crate) type Env<'a> = Option<&'a [(OsString, OsString)]>;

fn build(program: &Path, args: &[OsString], cwd: Option<&Path>, env: Env<'_>) -> CommandWrap {
    build_with(program, args, cwd, Stdio::null(), Group::Own, env)
}

fn build_with(
    program: &Path,
    args: &[OsString],
    cwd: Option<&Path>,
    stdin: Stdio,
    group: Group,
    env: Env<'_>,
) -> CommandWrap {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args).stdin(stdin).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(vars) = env {
        cmd.env_clear().envs(vars.iter().map(|(k, v)| (k, v)));
    }
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    #[cfg(target_os = "linux")]
    set_parent_death_signal(&mut cmd);
    // process_wrap's group wrapper reaps every member when the leader is waited for, which
    // would take the other stages' exit statuses: pipeline groups are set here instead.
    #[cfg(unix)]
    match group {
        Group::Own => {}
        Group::Lead => {
            cmd.process_group(0);
        }
        Group::Join(pgid) => {
            cmd.process_group(pgid as i32);
        }
    }

    let mut wrap = CommandWrap::from(cmd);
    #[cfg(unix)]
    if group == Group::Own {
        wrap.wrap(process_wrap::tokio::ProcessGroup::leader());
    }
    #[cfg(not(unix))]
    let _ = group;
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

fn spawn(
    program: &Path,
    args: &[OsString],
    cwd: Option<&Path>,
    env: Env<'_>,
) -> Result<Box<dyn ChildWrapper>> {
    spawned(program, build(program, args, cwd, env).spawn())
}

/// Spawns one stage of a pipeline (the AI tier, US6): `stdin` is the previous stage's stdout;
/// stdout and stderr are piped. On Unix every stage is in the group of the first (`Group::Lead`
/// then `Group::Join(first pid)`), so [`kill_group`] stops them all at once; on Windows each has
/// its own kill-on-close Job Object and is killed one by one.
pub(crate) fn spawn_stage(
    program: &Path,
    args: &[OsString],
    cwd: Option<&Path>,
    stdin: Stdio,
    group: Group,
    env: Env<'_>,
) -> Result<Box<dyn ChildWrapper>> {
    spawned(program, build_with(program, args, cwd, stdin, group, env).spawn())
}

/// SIGKILLs every process in the group `pgid` (pipelines). Errors are ignored: the group may
/// be gone already.
#[cfg(unix)]
#[allow(unsafe_code)]
pub(crate) fn kill_group(pgid: u32) {
    if let Ok(pgid) = libc::pid_t::try_from(pgid)
        && pgid > 1
    {
        // SAFETY: killpg only sends a signal; the group is one this run created and has not
        // fully reaped yet (callers stop calling once every stage is reaped).
        unsafe {
            libc::killpg(pgid, libc::SIGKILL);
        }
    }
}

fn spawned(program: &Path, r: std::io::Result<Box<dyn ChildWrapper>>) -> Result<Box<dyn ChildWrapper>> {
    r.map_err(|e| {
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
    let mut out = run(program, args, None, None, cancel, |stdout| async {
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
    on_line: impl FnMut(&str) + Send,
) -> Result<ProcessOutput> {
    run_lines_in(program, args, None, cancel, on_line).await
}

/// [`run_lines`] with a working directory (e.g. a job's scratch directory, so filter graphs can
/// name their files relatively instead of escaping full paths).
pub async fn run_lines_in(
    program: &Path,
    args: &[OsString],
    cwd: Option<&Path>,
    cancel: &CancellationToken,
    on_line: impl FnMut(&str) + Send,
) -> Result<ProcessOutput> {
    run_lines_env(program, args, cwd, None, cancel, on_line).await
}

/// [`run_lines_in`] with only the variables of `env` (the optional GIF engines).
pub(crate) async fn run_lines_env(
    program: &Path,
    args: &[OsString],
    cwd: Option<&Path>,
    env: Env<'_>,
    cancel: &CancellationToken,
    mut on_line: impl FnMut(&str) + Send,
) -> Result<ProcessOutput> {
    run(program, args, cwd, env, cancel, |stdout| async move {
        let mut reader = BufReader::new(stdout);
        while let Some(line) = next_line_bounded(&mut reader, MAX_LINE_CHARS * 4).await? {
            on_line(&line);
        }
        Ok(())
    })
    .await
}

/// Runs a tool and hands its stdout to `consume`, with no size limit (e.g. raw video frames).
/// Kills the process tree on cancel or when `consume` fails.
pub async fn run_stream<F, Fut>(
    program: &Path,
    args: &[OsString],
    cwd: Option<&Path>,
    cancel: &CancellationToken,
    consume: F,
) -> Result<ProcessOutput>
where
    F: FnOnce(tokio::process::ChildStdout) -> Fut,
    Fut: std::future::Future<Output = std::io::Result<()>>,
{
    run(program, args, cwd, None, cancel, consume).await
}

/// Spawns, drains stdout with `consume` (racing cancellation), then reaps the child.
async fn run<F, Fut>(
    program: &Path,
    args: &[OsString],
    cwd: Option<&Path>,
    env: Env<'_>,
    cancel: &CancellationToken,
    consume: F,
) -> Result<ProcessOutput>
where
    F: FnOnce(tokio::process::ChildStdout) -> Fut,
    Fut: std::future::Future<Output = std::io::Result<()>>,
{
    let mut child = spawn(program, args, cwd, env)?;
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

pub(crate) async fn kill(child: &mut dyn ChildWrapper) {
    // Kills the whole process group / job, then reaps the child.
    let _ = Box::into_pin(child.kill()).await;
}

/// Reads one line (without its `\n`), keeping at most `max` bytes of it: the rest of a longer
/// line is read and dropped, so a child printing endless text without a newline cannot make us
/// buffer it. `None` at end of input.
pub(crate) async fn next_line_bounded<R: tokio::io::AsyncBufRead + Unpin>(
    r: &mut R,
    max: usize,
) -> std::io::Result<Option<String>> {
    let mut kept: Vec<u8> = Vec::new();
    let mut seen = false;
    loop {
        let buf = r.fill_buf().await?;
        if buf.is_empty() {
            return Ok(seen.then(|| String::from_utf8_lossy(&kept).into_owned()));
        }
        seen = true;
        let (chunk, done) = match buf.iter().position(|b| *b == b'\n') {
            Some(i) => (&buf[..i], Some(i + 1)),
            None => (buf, None),
        };
        let room = max.saturating_sub(kept.len());
        kept.extend_from_slice(&chunk[..chunk.len().min(room)]);
        let used = done.unwrap_or(buf.len());
        r.consume(used);
        if done.is_some() {
            if kept.last() == Some(&b'\r') {
                kept.pop();
            }
            return Ok(Some(String::from_utf8_lossy(&kept).into_owned()));
        }
    }
}

pub(crate) async fn collect_stderr(stderr: Option<tokio::process::ChildStderr>) -> String {
    let Some(stderr) = stderr else { return String::new() };
    let mut tail: VecDeque<String> = VecDeque::with_capacity(STDERR_TAIL_LINES);
    let mut reader = BufReader::new(stderr);
    while let Ok(Some(mut line)) = next_line_bounded(&mut reader, MAX_LINE_CHARS * 4).await {
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

#[cfg(test)]
mod line_tests {
    use super::next_line_bounded;

    #[tokio::test]
    async fn long_lines_are_cut_without_buffering_them() {
        let mut big = vec![b'x'; 100_000];
        big.extend_from_slice(b"\nnext\r\nlast");
        let mut r = tokio::io::BufReader::with_capacity(64, &big[..]);
        assert_eq!(next_line_bounded(&mut r, 10).await.unwrap().unwrap(), "xxxxxxxxxx");
        assert_eq!(next_line_bounded(&mut r, 10).await.unwrap().unwrap(), "next");
        assert_eq!(next_line_bounded(&mut r, 10).await.unwrap().unwrap(), "last");
        assert_eq!(next_line_bounded(&mut r, 10).await.unwrap(), None);
    }
}
