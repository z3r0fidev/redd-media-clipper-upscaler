//! What stops a runaway GIF job: the output size cap and its watchdog (research R7), and a wall-clock deadline.

use super::*;

/// Without a budget, an encode that passes this size is stopped (a runaway output).
pub(crate) const MAX_OUTPUT: u64 = 4 << 30;

/// Free space the watchdog leaves in the output folder (other writers, the file system).
pub(super) const SPACE_RESERVE: u64 = 64 << 20;

/// The size an output is stopped at, and whether the folder's free space set it (then passing
/// it is `DISK_FULL`, not a runaway encode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cap {
    pub bytes: u64,
    pub from_space: bool,
}

impl Cap {
    pub(crate) const fn size(bytes: u64) -> Cap {
        Cap { bytes, from_space: false }
    }

    /// `base`, or less when `dir`'s folder cannot hold `copies` files of that size (security
    /// review L2: without a budget, two 4 GiB files could fill the disk).
    pub(super) fn within_free_space(base: u64, dir: &Path, copies: u64) -> Cap {
        Cap::within(base, copies, crate::upscale::estimate::free_space(dir))
    }

    pub(super) fn within(base: u64, copies: u64, free: Option<u64>) -> Cap {
        match free.map(|f| f.saturating_sub(SPACE_RESERVE) / copies.max(1)) {
            Some(room) if room < base => Cap { bytes: room, from_space: true },
            _ => Cap::size(base),
        }
    }
}

/// How often [`SizeWatch`] reads the file's size.
pub(crate) const SIZE_POLL: Duration = Duration::from_millis(250);

/// Research R7's size watchdog: cancels `stop` once `file` passes `limit` bytes. Dropping it
/// stops the watching task. It reads the size every [`SIZE_POLL`], so the file can pass the
/// limit by what the encoder writes in one poll (tested) plus whatever it writes before the
/// cancel kills it; `limit` itself is allowed.
pub(crate) struct SizeWatch {
    task: tokio::task::JoinHandle<()>,
    over: Arc<AtomicBool>,
    limit: Cap,
    /// The output folder (the scratch folder's parent), named in `DISK_FULL`.
    folder: PathBuf,
}

impl SizeWatch {
    pub(crate) fn start(file: PathBuf, limit: Cap, stop: &CancellationToken) -> SizeWatch {
        let over = Arc::new(AtomicBool::new(false));
        let folder = file.parent().and_then(Path::parent).unwrap_or(&file).to_path_buf();
        let (flag, stop) = (over.clone(), stop.clone());
        let task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(SIZE_POLL).await;
                if std::fs::metadata(&file).is_ok_and(|m| m.len() > limit.bytes) {
                    flag.store(true, Ordering::SeqCst);
                    stop.cancel();
                    return;
                }
            }
        });
        SizeWatch { task, over, limit, folder }
    }

    /// Stops watching; an error when the file grew past the limit (the run was cancelled for it):
    /// `DISK_FULL` when the folder's free space set the limit.
    pub(crate) fn stop(self) -> Result<()> {
        self.task.abort();
        if self.over.load(Ordering::SeqCst) {
            if self.limit.from_space {
                return Err(Error::DiskFull(self.folder.clone()));
            }
            return Err(Error::engine_failed(
                format!("the GIF grew past {} bytes and was stopped", self.limit.bytes),
                String::new(),
            ));
        }
        Ok(())
    }
}

impl Drop for SizeWatch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Cancels `stop` after `after`; `fired()` tells whether it did.
pub(crate) struct Deadline {
    task: tokio::task::JoinHandle<()>,
    fired: Arc<AtomicBool>,
}

impl Deadline {
    pub(crate) fn start(after: Duration, stop: &CancellationToken) -> Deadline {
        let fired = Arc::new(AtomicBool::new(false));
        let (flag, stop) = (fired.clone(), stop.clone());
        let task = tokio::spawn(async move {
            tokio::time::sleep(after).await;
            flag.store(true, Ordering::SeqCst);
            stop.cancel();
        });
        Deadline { task, fired }
    }

    pub(crate) fn fired(&self) -> bool {
        self.fired.load(Ordering::SeqCst)
    }
}

impl Drop for Deadline {
    fn drop(&mut self) {
        self.task.abort();
    }
}
