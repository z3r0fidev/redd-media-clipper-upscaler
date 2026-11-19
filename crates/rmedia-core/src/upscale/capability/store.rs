//! The capability record on disk: `<state>/capability.json`, read capped, written atomically under a lock, and demotions.

use super::*;

/// RFC 3339 UTC time with seconds (`2026-10-19T08:30:00Z`).
pub fn rfc3339(t: SystemTime) -> String {
    let secs = t.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

pub(super) const MAX_RECORD_BYTES: u64 = 64 * 1024;

/// `<state>/capability.json`, read and written under `capability.lock`.
#[derive(Debug, Clone)]
pub struct CapabilityStore {
    path: PathBuf,
}

impl CapabilityStore {
    pub fn in_state_dir(dir: &Path) -> CapabilityStore {
        CapabilityStore { path: dir.join("capability.json") }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The stored record, if present and plausible (a tampered or foreign file is ignored).
    pub fn load(&self) -> Option<CapabilityRecord> {
        use std::io::Read;
        let file = std::fs::File::open(&self.path).ok()?;
        let meta = file.metadata().ok()?;
        if !meta.is_file() || meta.len() > MAX_RECORD_BYTES {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(MAX_RECORD_BYTES).read_to_end(&mut bytes).ok()?;
        let r: CapabilityRecord = serde_json::from_slice(&bytes).ok()?;
        let text = |s: &str| s.len() <= 512 && !s.chars().any(char::is_control);
        let rate = |v: Option<f32>| v.is_none_or(|v| v.is_finite() && (0.0..=1e5).contains(&v));
        let tier_ok =
            |t: &TierInfo| t.reason.as_deref().is_none_or(text) && rate(t.fps) && rate(t.sec_per_mpx);
        let t = &r.tiers;
        // A usable GPU tier must name a listed, usable device.
        let gpu_device =
            !t.gpu.available || t.gpu.device.is_some_and(|i| r.device(i).is_some_and(|d| d.usable));
        let plausible = r.schema == 1
            && r.fingerprint.len() == 64
            && r.fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
            && text(&r.checked_at)
            && r.devices.len() <= MAX_DEVICES
            && r.demoted.len() <= 8
            && r.devices
                .iter()
                .all(|d| text(&d.name) && text(&d.device_id) && d.reason.as_deref().is_none_or(text))
            && r.demoted.iter().all(|d| text(&d.error) && text(&d.at))
            && [&t.cpu, &t.gpu, &t.ai].into_iter().all(tier_ok)
            && gpu_device;
        plausible.then_some(r)
    }

    /// Loads, applies `f` and writes the result atomically, all under the lock.
    pub fn update(
        &self,
        f: impl FnOnce(Option<CapabilityRecord>) -> CapabilityRecord,
    ) -> crate::Result<CapabilityRecord> {
        self.modify(|r| Some(f(r))).map(|r| r.expect("update always writes"))
    }

    /// [`update`](Self::update) where `f` may decline (`None`): nothing is written.
    pub(super) fn modify(
        &self,
        f: impl FnOnce(Option<CapabilityRecord>) -> Option<CapabilityRecord>,
    ) -> crate::Result<Option<CapabilityRecord>> {
        use std::io::Write;
        let dir = self.path.parent().unwrap_or(Path::new("."));
        let mut b = std::fs::DirBuilder::new();
        b.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        b.create(dir)?;
        let lock = std::fs::File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("capability.lock"))?;
        // Bounded wait: a stuck holder must not hang a job (the OS releases a dead holder's lock).
        let waited = Instant::now();
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if waited.elapsed() < Duration::from_secs(10) => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err(
                        std::io::Error::other("the capability record is locked by another process").into()
                    );
                }
                Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
            }
        }
        let Some(record) = f(self.load()) else {
            let _ = lock.unlock();
            return Ok(None);
        };
        let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let tmp = dir.join(format!(".capability.{}-{nanos}.tmp", std::process::id()));
        let written = std::fs::File::options()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .and_then(|mut file| {
                file.write_all(&serde_json::to_vec_pretty(&record).map_err(std::io::Error::other)?)?;
                file.sync_all()
            })
            .and_then(|()| std::fs::rename(&tmp, &self.path));
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        let _ = lock.unlock();
        written?;
        Ok(Some(record))
    }
}

/// Records that `tier` failed after an automatic choice, so later automatic choices skip it
/// until the fingerprint changes (SC-008). `error` is a fixed description, never ffmpeg's text
/// (which can name the user's files). Without a stored record there is nothing to change.
pub fn demote(store: &CapabilityStore, tier: Tier, error: &str) -> crate::Result<()> {
    let error = clean(error, 400);
    store
        .modify(|r| {
            let mut r = r?;
            r.demoted.retain(|d| d.tier != tier);
            r.demoted.push(Demotion { tier, error, at: rfc3339(SystemTime::now()) });
            Some(r)
        })
        .map(|_| ())
}
