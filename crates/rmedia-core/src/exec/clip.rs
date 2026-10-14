//! Executes a clip job: run ffmpeg into the temp file, then verify and measure the result.

use std::path::Path;

use super::JobCtx;
use super::process::run_lines;
use super::progress::ProgressParser;
use crate::error::{Error, Result};
use crate::event::{JobMeasure, Span};
use crate::plan::ffargs::{ClipSpec, clip_args};
use crate::probe;
use crate::request::Mode;
use crate::time::Ts;

/// Deviation above which a produced span counts as "adjusted" (FR-014).
const ADJUST_TOLERANCE: Ts = Ts::from_micros(50_000);

pub(super) async fn execute(spec: &ClipSpec, ctx: &mut JobCtx<'_>) -> Result<JobMeasure> {
    let (requested, predicted) = (spec.requested, spec.predicted);
    let args = clip_args(spec, ctx.temp);
    let total = requested.end.saturating_sub(requested.start);
    let mut parser = ProgressParser::default();
    let (tools, cancel) = (ctx.tools, ctx.cancel);
    let out = run_lines(&tools.ffmpeg.path, &args, cancel, |line| {
        if let Some(block) = parser.feed(line) {
            ctx.progress(block, total);
        }
    })
    .await?;
    if !out.status.success() {
        return Err(classify_failure(&out.stderr_tail, ctx.output));
    }

    // Verify the output before committing it.
    let info = probe::probe(ctx.tools, ctx.temp, ctx.cancel)
        .await
        .map_err(|e| Error::VerifyFailed(e.to_string()))?;
    if info.streams.len() != spec.streams.maps.len() {
        return Err(Error::VerifyFailed(format!(
            "expected {} streams, found {}",
            spec.streams.maps.len(),
            info.streams.len()
        )));
    }
    let produced = info
        .first_video()
        .and_then(|v| v.duration)
        .or(info.duration)
        .filter(|d| *d > Ts::ZERO)
        .ok_or_else(|| Error::VerifyFailed("output has no duration".into()))?;
    if let (Some((expected, tolerance)), Some(got)) =
        (spec.verify_video, info.first_video().and_then(|v| v.duration))
    {
        let diff = if got > expected { got - expected } else { expected - got };
        if diff > tolerance {
            return Err(Error::VerifyFailed(format!(
                "the clip's video lasts {:.3} s but {:.3} s was requested; the source may be damaged \
                 or its timestamps unreliable",
                got.as_secs_f64(),
                expected.as_secs_f64()
            )));
        }
    }
    let start = match spec.mode {
        Mode::Fast => predicted.map_or(requested.start, |p| p.start),
        Mode::Accurate => requested.start,
    };
    let produced = match spec.mode {
        Mode::Fast => produced,
        Mode::Accurate => info.duration.unwrap_or(produced),
    };
    let actual = Span { start, end: start + produced };
    Ok(JobMeasure::Clip { actual, adjusted: adjusted(requested, actual) })
}

fn adjusted(requested: Span, actual: Span) -> bool {
    let diff = |a: Ts, b: Ts| if a > b { a - b } else { b - a };
    diff(requested.start, actual.start) > ADJUST_TOLERANCE
        || diff(requested.end, actual.end) > ADJUST_TOLERANCE
}

/// Maps ffmpeg's stderr to a user-meaningful error.
pub fn classify_failure(stderr_tail: &str, output: &Path) -> Error {
    let lower = stderr_tail.to_ascii_lowercase();
    if lower.contains("no space left on device") || lower.contains("there is not enough space") {
        Error::DiskFull(output.to_path_buf())
    } else if lower.contains("permission denied") || lower.contains("read-only file system") {
        Error::OutputUnwritable { path: output.to_path_buf(), message: "permission denied".into() }
    } else {
        const GENERIC: [&str; 6] = [
            "nothing was written into output file",
            "conversion failed",
            "error while opening encoder",
            "could not open encoder before eof",
            "error sending frames to consumers",
            "task finished with error code",
        ];
        let specific = |l: &&str| {
            let l = l.trim().to_ascii_lowercase();
            !l.is_empty() && !GENERIC.iter().any(|g| l.contains(g))
        };
        let message = stderr_tail
            .lines()
            .rev()
            .find(specific)
            .or_else(|| stderr_tail.lines().rev().find(|l| !l.trim().is_empty()))
            .unwrap_or("ffmpeg exited with an error")
            .trim()
            .to_owned();
        Error::EngineFailed { message, stderr_tail: stderr_tail.to_owned() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_failures() {
        let out = Path::new("/o/x.mp4");
        let e = classify_failure("[mp4 @ 0x1] Error writing trailer: No space left on device", out);
        assert_eq!(e.code(), "DISK_FULL");
        let e = classify_failure("file:/o/.x.part: Permission denied", out);
        assert_eq!(e.code(), "OUTPUT_UNWRITABLE");
        let e = classify_failure("line one\nInvalid data found when processing input\n", out);
        assert_eq!(e.code(), "ENGINE_FAILED");
        assert_eq!(e.to_string(), "ffmpeg failed: Invalid data found when processing input");
        let e = classify_failure(
            "[libx264 @ 0x1] height not divisible by 2 (321x241)\nError while opening encoder\n\
             [out#0/mp4] Nothing was written into output file, because at least one of its streams received no packets.\nConversion failed!",
            out,
        );
        assert_eq!(e.to_string(), "ffmpeg failed: [libx264 @ 0x1] height not divisible by 2 (321x241)");
    }
}
