//! The optional engines' runs: the ffmpeg → gifski pipe and the gifsicle post-pass (less trusted than ffmpeg: cleared environment, cleaned messages).

use super::*;

/// The gifski pipe is stopped when ffmpeg reports no progress for this long. gifski holds only
/// a few frames, so it finishes soon after ffmpeg does.
pub(super) const PIPE_STALL: Duration = Duration::from_secs(300);

/// The gifsicle post-pass is stopped after this long (it takes seconds on a Reddit-sized GIF).
pub(super) const GIFSICLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// The ffmpeg → gifski pipe (research R7), writing `output` in `dir`.
pub(crate) struct Gifski<'a> {
    pub program: &'a Path,
    pub dir: &'a Path,
    pub output: &'a str,
    /// The size watchdog's limit.
    pub limit: Cap,
    /// A wall-clock limit, for runs without a progress file (no stall can be seen there).
    pub timeout: Option<Duration>,
}

impl Gifski<'_> {
    /// Runs `producer` (an ffmpeg argv writing y4m to stdout, and its progress to `progress`
    /// when set) into gifski with the settings of `s`. A play-once GIF loses the loop block
    /// gifski writes anyway. `job_output` names the output in disk errors.
    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        &self,
        tools: &Tools,
        producer: Vec<OsString>,
        s: &Settings,
        loop_count: LoopCount,
        progress: Option<&str>,
        job_output: &Path,
        cancel: &CancellationToken,
        on_progress: impl FnMut(ProgressBlock),
    ) -> Result<()> {
        let params =
            s.gifski.ok_or_else(|| Error::InvalidInput("a gifski encode needs its parameters".into()))?;
        let file = self.dir.join(self.output);
        let _ = std::fs::remove_file(&file);
        let from = match progress {
            Some(p) => {
                let path = self.dir.join(p);
                let _ = std::fs::remove_file(&path);
                ProgressFrom::File(path)
            }
            None => ProgressFrom::ConsumerStdout,
        };
        let stages = [
            StageSpec {
                program: tools.ffmpeg.path.clone(),
                args: producer,
                role: Role::Producer,
                report: Report::Tail,
                env: None,
            },
            StageSpec {
                program: self.program.to_path_buf(),
                args: gifski_args(s.rate, params, loop_count, self.output),
                role: Role::Consumer,
                report: Report::CleanTail,
                env: Some(crate::gif::engines::engine_env()),
            },
        ];
        let stop = cancel.child_token();
        let watch = SizeWatch::start(file.clone(), self.limit, &stop);
        // Without a progress file nothing reports progress, so nothing can show a stall.
        let stall = progress.map(|_| PIPE_STALL);
        let deadline = self.timeout.map(|t| Deadline::start(t, &stop));
        let r = run_stages(&stages, Some(self.dir), &from, stall, &stop, on_progress).await;
        watch.stop()?;
        if let (Some(d), Some(t)) = (&deadline, self.timeout)
            && d.fired()
            && !cancel.is_cancelled()
        {
            return Err(Error::engine_failed(
                format!("the gifski pipe took longer than {} s and was stopped", t.as_secs()),
                String::new(),
            ));
        }
        let ends = r?;
        match blame_pair(&ends[0], &ends[1]) {
            Some(0) => return Err(classify_failure(&ends[0].tail, job_output)),
            Some(_) if !ends[1].failed() => {
                return Err(Error::EngineFailed {
                    tool: "gifski".into(),
                    message: "gifski stopped reading frames before the end".into(),
                    stderr_tail: ends[1].tail.clone(),
                });
            }
            Some(_) => return Err(tool_failed("gifski", ends[1].status, &ends[1].tail)),
            None => {}
        }
        if loop_count == LoopCount::Plays(1) {
            let unwritable = |e: std::io::Error| Error::OutputUnwritable {
                path: job_output.to_path_buf(),
                message: e.to_string(),
            };
            let path = file.clone();
            tokio::task::spawn_blocking(move || strip_loop_block(&path))
                .await
                .map_err(|e| unwritable(std::io::Error::other(e)))?
                .map_err(unwritable)?;
        }
        Ok(())
    }
}

/// A failed optional engine, named by `tool`, with its last stderr line (control characters
/// removed: these programs are less trusted than ffmpeg).
pub(super) fn tool_failed(tool: &str, status: Option<ExitStatus>, stderr_tail: &str) -> Error {
    let tail: String =
        stderr_tail.chars().filter(|c| !crate::gif::target::unsafe_char(*c) || *c == '\n').collect();
    let last = tail.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no message").trim().to_owned();
    Error::EngineFailed {
        tool: tool.into(),
        message: format!("{last} ({})", crate::exec::pipeline::describe(status)),
        stderr_tail: tail,
    }
}

/// The gifsicle post-pass (`-O2`, lossy when set) with a cleared environment, the size watchdog
/// on its output and a wall-clock limit.
pub(super) async fn run_gifsicle(
    ctx: &mut JobCtx<'_>,
    gifsicle: &Path,
    scratch: &Path,
    lossy: u16,
    limit: Cap,
    (input, output): (&str, &str),
) -> Result<()> {
    let env = crate::gif::engines::engine_env();
    let args = gifsicle_args(lossy, input, output);
    let stop = ctx.cancel.child_token();
    let watch = SizeWatch::start(scratch.join(output), limit, &stop);
    let deadline = Deadline::start(GIFSICLE_TIMEOUT, &stop);
    let r = process::run_lines_env(gifsicle, &args, Some(scratch), Some(&env), &stop, |_| {}).await;
    watch.stop()?;
    if deadline.fired() && !ctx.cancel.is_cancelled() {
        return Err(Error::EngineFailed {
            tool: "gifsicle".into(),
            message: format!(
                "gifsicle took longer than {} min and was stopped",
                GIFSICLE_TIMEOUT.as_secs() / 60
            ),
            stderr_tail: String::new(),
        });
    }
    let out = r?;
    if out.status.success() {
        Ok(())
    } else {
        Err(tool_failed("gifsicle", Some(out.status), &out.stderr_tail))
    }
}
