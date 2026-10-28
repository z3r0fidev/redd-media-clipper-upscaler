//! rmedia-core's `tracing` events go to `<state dir>/logs/desktop.log` (and to stderr, which only
//! a debug build started from a terminal shows). rmedia's own events from info up, other crates'
//! from warn; `RMEDIA_LOG` (as for the CLI) overrides that. The file is private to the user and
//! moves to `desktop.log.1` once it passes 1 MiB, so at most two are kept.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const LIMIT: u64 = 1024 * 1024;

/// Starts logging; returns the log file, if one could be opened.
pub fn init() -> Option<PathBuf> {
    let filter =
        std::env::var("RMEDIA_LOG").ok().and_then(|s| s.parse::<Targets>().ok()).unwrap_or_else(|| {
            Targets::new()
                .with_default(LevelFilter::WARN)
                .with_target("rmedia_core", LevelFilter::INFO)
                .with_target("rmedia_desktop", LevelFilter::INFO)
        });
    let opened = rmedia_core::settings::state_dir().and_then(|d| {
        let path = d.join("logs").join("desktop.log");
        open(&path).ok().map(|f| (path, f))
    });
    let (path, file) = match opened {
        Some((p, f)) => (Some(p), Some(f)),
        None => (None, None),
    };
    let writer = std::io::stderr.and(Mutex::new(Sink(file)));
    let layer = tracing_subscriber::fmt::layer().with_ansi(false).with_writer(writer);
    let _ = tracing_subscriber::registry().with(layer).with(filter).try_init();
    path
}

/// Opens the log for appending, first moving a full one to `.1`.
pub fn open(path: &Path) -> std::io::Result<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > LIMIT) {
        let mut old = path.as_os_str().to_owned();
        old.push(".1");
        std::fs::rename(path, old)?;
    }
    let mut o = OpenOptions::new();
    o.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(path)
}

/// The log file, or nowhere when it could not be opened.
struct Sink(Option<File>);

impl std::io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match &mut self.0 {
            Some(f) => f.write(buf),
            None => Ok(buf.len()),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.as_mut().map_or(Ok(()), |f| f.flush())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_log_moves_aside_and_a_new_one_starts() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("logs").join("desktop.log");
        drop(open(&path).unwrap());
        std::fs::write(&path, vec![b'x'; LIMIT as usize + 1]).unwrap();
        drop(open(&path).unwrap());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        assert_eq!(std::fs::metadata(d.path().join("logs").join("desktop.log.1")).unwrap().len(), LIMIT + 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}
