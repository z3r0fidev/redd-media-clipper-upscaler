//! Shows rmedia-core's `tracing` events on stderr as `warning: …` lines, in the CLI's own style.
//! Warnings and errors by default, `-v` adds info, `-vv` debug, `-q` errors only. `RMEDIA_LOG`
//! (a level such as `debug`, or targets such as `rmedia_core=debug`) overrides the flags.

use std::io::Write;
use std::sync::Mutex;

use indicatif::MultiProgress;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields, FormattedFields, MakeWriter};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;

use crate::render::clean;

/// The progress bars on screen; a log line is written while they are hidden.
static BARS: Mutex<Option<MultiProgress>> = Mutex::new(None);

pub fn init(verbose: u8, quiet: bool) {
    let level = match (quiet, verbose) {
        (true, _) => LevelFilter::ERROR,
        (false, 0) => LevelFilter::WARN,
        (false, 1) => LevelFilter::INFO,
        _ => LevelFilter::DEBUG,
    };
    let filter =
        match std::env::var("RMEDIA_LOG").ok().filter(|s| !s.is_empty()).map(|s| s.parse::<Targets>()) {
            Some(Ok(targets)) => targets,
            Some(Err(e)) => {
                eprintln!("warning: RMEDIA_LOG ignored ({e})");
                Targets::new().with_default(level)
            }
            None => Targets::new().with_default(level),
        };
    let layer = tracing_subscriber::fmt::layer().event_format(Line).with_writer(Stderr);
    let _ = tracing_subscriber::registry().with(layer).with(filter).try_init();
}

/// Log lines from now on suspend these bars while they print.
pub fn set_progress(bars: &MultiProgress) {
    if let Ok(mut b) = BARS.lock() {
        *b = Some(bars.clone());
    }
}

struct Line;

impl<S, N> FormatEvent<S, N> for Line
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut w: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let label = match *event.metadata().level() {
            Level::ERROR => "error",
            Level::WARN => "warning",
            Level::INFO => "info",
            _ => "debug",
        };
        // Messages carry paths; control characters must not reach the terminal.
        let mut fields = FormattedFields::<N>::new(String::new());
        ctx.field_format().format_fields(fields.as_writer(), event)?;
        writeln!(w, "{label}: {}", clean(&fields))
    }
}

struct Stderr;

impl<'a> MakeWriter<'a> for Stderr {
    type Writer = Pending;
    fn make_writer(&'a self) -> Pending {
        Pending(Vec::new())
    }
}

/// One formatted line, written to stderr in one piece when dropped.
struct Pending(Vec<u8>);

impl Write for Pending {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        let line = std::mem::take(&mut self.0);
        let write = || {
            let _ = std::io::stderr().lock().write_all(&line);
        };
        match BARS.lock().map(|b| b.clone()).unwrap_or(None) {
            Some(bars) => bars.suspend(write),
            None => write(),
        }
    }
}
