//! Machine capability record: GPU listing, fingerprint, self-tests, demotions (research R8).
//!
//! The cheap Vulkan listing runs on every check; the self-tests run only when the fingerprint
//! (app and ffmpeg versions, the listing, the Vulkan driver files) changes, or on request
//! (`doctor --recheck`, FR-009). The record lives in `<state>/capability.json`
//! (contracts/capability-record.schema.json), written atomically under a lock.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::tier::Tier;
use tokio_util::sync::CancellationToken;

use crate::exec::process;
use crate::preset::Preset;
use crate::tools::Tools;

/// What one Vulkan device is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum DeviceKind {
    Integrated,
    Discrete,
    Virtual,
    Cpu,
    Software,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct GpuDevice {
    pub index: u32,
    pub name: String,
    pub kind: DeviceKind,
    pub device_id: String,
    pub usable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TierInfo {
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub device: Option<u32>,
    /// Self-test throughput (frames/s) at the default preset size, encode included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub fps: Option<f32>,
    /// AI only: seconds per input megapixel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub sec_per_mpx: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TierStatus {
    pub cpu: TierInfo,
    pub gpu: TierInfo,
    pub ai: TierInfo,
}

/// A tier that failed after being chosen automatically (SC-008).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Demotion {
    pub tier: Tier,
    pub error: String,
    pub at: String,
}

/// The machine's cached capability record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CapabilityRecord {
    pub schema: u32,
    pub fingerprint: String,
    pub checked_at: String,
    pub devices: Vec<GpuDevice>,
    pub tiers: TierStatus,
    pub demoted: Vec<Demotion>,
}

impl Default for CapabilityRecord {
    /// CPU only (no check has run).
    fn default() -> Self {
        CapabilityRecord {
            schema: 1,
            fingerprint: "0".repeat(64),
            checked_at: rfc3339(SystemTime::UNIX_EPOCH),
            devices: Vec::new(),
            tiers: TierStatus {
                cpu: TierInfo { available: true, ..TierInfo::default() },
                gpu: TierInfo { reason: Some("no GPU check has run".into()), ..TierInfo::default() },
                ai: ai_not_built(),
            },
            demoted: Vec::new(),
        }
    }
}

fn ai_not_built() -> TierInfo {
    TierInfo { reason: Some("the AI tier is not built yet".into()), ..TierInfo::default() }
}

impl CapabilityRecord {
    pub fn is_demoted(&self, tier: Tier) -> Option<&Demotion> {
        self.demoted.iter().find(|d| d.tier == tier)
    }

    pub fn device(&self, index: u32) -> Option<&GpuDevice> {
        self.devices.iter().find(|d| d.index == index)
    }
}

// --- listing ---------------------------------------------------------------------------------

/// One line of ffmpeg's Vulkan "GPU listing".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedDevice {
    pub index: u32,
    pub name: String,
    pub kind: DeviceKind,
    pub device_id: String,
}

/// Parses the `GPU listing:` block ffmpeg prints with `-v verbose -init_hw_device vulkan`:
/// `[Vulkan @ 0x…]     0: Intel(R) Graphics (RPL-U) (integrated) (0xa721)`. Lines from other
/// sources (Mesa `FINISHME` warnings) are skipped; the block ends at the first Vulkan line that
/// is not a device.
pub fn parse_listing(stderr: &str) -> Vec<ListedDevice> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in stderr.lines() {
        let Some(rest) =
            line.strip_prefix("[Vulkan @ ").and_then(|r| r.split_once("] ").map(|(_, r)| r.trim()))
        else {
            continue;
        };
        if rest == "GPU listing:" {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        match parse_device(rest) {
            Some(d) => out.push(d),
            None => break,
        }
    }
    out
}

fn parse_device(s: &str) -> Option<ListedDevice> {
    let (index, rest) = s.split_once(": ")?;
    let index = index.parse().ok()?;
    let (rest, id) = rest.strip_suffix(')')?.rsplit_once(" (")?;
    let (name, kind) = rest.strip_suffix(')')?.rsplit_once(" (")?;
    let kind = match kind {
        "integrated" => DeviceKind::Integrated,
        "discrete" => DeviceKind::Discrete,
        "virtual" => DeviceKind::Virtual,
        "cpu" => DeviceKind::Cpu,
        "software" => DeviceKind::Software,
        _ => DeviceKind::Other,
    };
    Some(ListedDevice { index, name: name.to_owned(), kind, device_id: id.to_owned() })
}

/// Why a device can never be the GPU tier: software rasterisers (FR-007).
pub fn reject_reason(d: &ListedDevice) -> Option<&'static str> {
    let name = d.name.to_ascii_lowercase();
    let software = matches!(d.kind, DeviceKind::Software | DeviceKind::Cpu)
        || ["llvmpipe", "lavapipe", "swiftshader"].iter().any(|n| name.contains(n));
    software.then_some("software renderer")
}

/// The device the GPU tier uses: the first usable discrete one, else integrated, else virtual.
pub fn preferred(devices: &[GpuDevice]) -> Option<u32> {
    [DeviceKind::Discrete, DeviceKind::Integrated, DeviceKind::Virtual]
        .iter()
        .find_map(|k| devices.iter().find(|d| d.usable && d.kind == *k).map(|d| d.index))
}

// --- fingerprint -----------------------------------------------------------------------------

/// Everything that, when it changes, means the self-tests must run again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FingerprintInputs {
    pub app_version: String,
    pub ffmpeg_version: String,
    pub listing: Vec<String>,
    /// AI model SHA-256s (US6).
    pub model_shas: Vec<String>,
    /// Vulkan driver identity (manifests, libraries).
    pub drivers: Vec<String>,
    /// Vulkan loader variables that change which drivers load.
    pub vk_env: Vec<String>,
}

pub fn fingerprint(i: &FingerprintInputs) -> String {
    let mut h = Sha256::new();
    let mut put = |tag: &str, items: &[String]| {
        h.update(tag.as_bytes());
        h.update((items.len() as u64).to_le_bytes());
        for it in items {
            h.update((it.len() as u64).to_le_bytes());
            h.update(it.as_bytes());
        }
    };
    put("app", std::slice::from_ref(&i.app_version));
    put("ffmpeg", std::slice::from_ref(&i.ffmpeg_version));
    put("listing", &i.listing);
    put("models", &i.model_shas);
    put("drivers", &i.drivers);
    put("env", &i.vk_env);
    format!("{:x}", h.finalize())
}

/// Identity of Vulkan driver manifests: each manifest's path and content hash, plus its
/// `library_path`'s size and mtime (the listing has no driver version). Sorted.
pub fn manifest_identity(manifests: &[PathBuf]) -> Vec<String> {
    use std::io::Read;
    let mut out: Vec<String> = manifests
        .iter()
        .filter_map(|m| {
            // Regular files only (a FIFO planted here would block the open).
            if !std::fs::metadata(m).ok()?.is_file() {
                return None;
            }
            let mut text = Vec::new();
            std::fs::File::open(m).ok()?.take(64 * 1024).read_to_end(&mut text).ok()?;
            let json: serde_json::Value = serde_json::from_slice(&text).unwrap_or_default();
            let lib = json["ICD"]["library_path"].as_str().map(|l| {
                let p = Path::new(l);
                let p = if p.is_absolute() {
                    p.to_path_buf()
                } else if l.contains(['/', '\\']) {
                    m.parent().unwrap_or(Path::new("")).join(p)
                } else {
                    // A bare name is found by the dynamic loader: look in its usual folders.
                    LIBRARY_DIRS
                        .iter()
                        .map(|d| Path::new(d).join(l))
                        .find(|c| c.is_file())
                        .unwrap_or_else(|| p.to_path_buf())
                };
                match std::fs::metadata(&p) {
                    Ok(meta) => {
                        let mtime = meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                            .map_or(0, |d| d.as_nanos());
                        format!("{} {} {mtime}", p.display(), meta.len())
                    }
                    Err(_) => l.to_owned(),
                }
            });
            Some(format!("{} {:x} {}", m.display(), Sha256::digest(&text), lib.unwrap_or_default()))
        })
        .collect();
    out.sort();
    out
}

/// Where the dynamic loader finds a driver named without a path (Linux; harmless elsewhere).
const LIBRARY_DIRS: &[&str] = &[
    "/usr/lib/x86_64-linux-gnu",
    "/usr/lib/aarch64-linux-gnu",
    "/usr/lib64",
    "/usr/lib",
    "/lib/x86_64-linux-gnu",
    "/lib64",
    "/lib",
    "/usr/local/lib",
];

/// Linux: the `*.json` manifests in the given `icd.d` folders.
pub fn driver_identity_dirs(dirs: &[PathBuf]) -> Vec<String> {
    let mut manifests: Vec<PathBuf> = dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flat_map(|rd| rd.flatten().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    manifests.sort();
    manifest_identity(&manifests)
}

/// The system's Vulkan driver identity.
pub fn driver_identity() -> Vec<String> {
    #[cfg(windows)]
    {
        manifest_identity(&windows_manifests())
    }
    #[cfg(not(windows))]
    {
        // The loader's search folders: XDG data dirs (default /usr/local/share, /usr/share), the
        // user's data dir, and /etc.
        let data_dirs = std::env::var("XDG_DATA_DIRS")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        let mut dirs: Vec<PathBuf> = data_dirs
            .split(':')
            .filter(|d| !d.is_empty())
            .take(16)
            .map(|d| Path::new(d).join("vulkan/icd.d"))
            .collect();
        let home_data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")));
        dirs.extend(home_data.map(|d| d.join("vulkan/icd.d")));
        dirs.extend(["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d"].map(PathBuf::from));
        dirs.sort();
        dirs.dedup();
        driver_identity_dirs(&dirs)
    }
}

/// The value names under `HKLM\SOFTWARE\Khronos\Vulkan\Drivers` are manifest paths.
#[cfg(windows)]
fn windows_manifests() -> Vec<PathBuf> {
    use windows::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegEnumValueW, RegOpenKeyExW,
    };
    use windows::core::{PCWSTR, PWSTR};
    let sub: Vec<u16> = "SOFTWARE\\Khronos\\Vulkan\\Drivers\0".encode_utf16().collect();
    let mut key = HKEY::default();
    let mut out = Vec::new();
    #[allow(unsafe_code)]
    // SAFETY: plain Win32 registry calls with buffers we own; the key is closed below.
    unsafe {
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, PCWSTR(sub.as_ptr()), Some(0), KEY_READ, &mut key).is_err() {
            return out;
        }
        for i in 0..256u32 {
            let mut name = [0u16; 1024];
            let mut len = name.len() as u32;
            if RegEnumValueW(key, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None)
                .is_err()
            {
                break;
            }
            out.push(PathBuf::from(String::from_utf16_lossy(&name[..len as usize])));
        }
        let _ = RegCloseKey(key);
    }
    out
}

fn vk_env() -> Vec<String> {
    ["VK_DRIVER_FILES", "VK_ICD_FILENAMES", "VK_ADD_DRIVER_FILES"]
        .iter()
        .map(|v| format!("{v}={}", std::env::var(v).unwrap_or_default()))
        .collect()
}

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

// --- store -----------------------------------------------------------------------------------

const MAX_RECORD_BYTES: u64 = 64 * 1024;

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
    fn modify(
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

// --- detection -------------------------------------------------------------------------------

/// How the checks run.
#[derive(Debug, Clone)]
pub struct ProbeEnv {
    /// Self-tests run at this preset's size with its encoder settings.
    pub preset: Preset,
    /// Limit for one probe.
    pub timeout: Duration,
    /// Limit for the whole check (listing, round trips and self-tests).
    pub budget: Duration,
}

impl ProbeEnv {
    pub fn new(preset: Preset) -> ProbeEnv {
        ProbeEnv { preset, timeout: Duration::from_secs(60), budget: Duration::from_secs(180) }
    }
}

/// How much checking [`ensure`] may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckMode {
    /// Only reuse a stored record whose fingerprint matches (no self-tests): the desktop start.
    Cached,
    /// Reuse a matching record, otherwise run the self-tests and store the result.
    Ensure,
    /// Always run the self-tests (`doctor --recheck`, Settings › Check again).
    Recheck,
}

/// One probe's result. A probe that timed out or was cancelled proves nothing about the machine,
/// so a check that had one is used for this run but never stored.
#[derive(Debug)]
enum Probe {
    Done { ok: bool, stderr: String },
    Inconclusive,
}

/// Runs ffmpeg through `exec::process` (process-tree kill on cancel and timeout), within
/// `timeout` and before `deadline`.
async fn run_bounded(
    tools: &Tools,
    args: &[String],
    timeout: Duration,
    deadline: Instant,
    cancel: &CancellationToken,
) -> Probe {
    let left = deadline.saturating_duration_since(Instant::now()).min(timeout);
    if left.is_zero() || cancel.is_cancelled() {
        return Probe::Inconclusive;
    }
    let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
    match tokio::time::timeout(left, process::run_capture(&tools.ffmpeg.path, &argv, cancel)).await {
        Err(_) | Ok(Err(crate::Error::Cancelled)) => Probe::Inconclusive,
        Ok(Err(e)) => Probe::Done { ok: false, stderr: e.to_string() },
        Ok(Ok(out)) => Probe::Done { ok: out.status.success(), stderr: out.stderr_tail },
    }
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

/// Most devices kept from a listing (and accepted in a stored record).
const MAX_DEVICES: usize = 16;

/// The listing. Asking for a device index that cannot exist makes ffmpeg print the listing and
/// stop at once, without initialising any device.
async fn listing(
    tools: &Tools,
    env: &ProbeEnv,
    deadline: Instant,
    cancel: &CancellationToken,
) -> Option<Vec<ListedDevice>> {
    let args = strings(&[
        "-hide_banner",
        "-nostdin",
        "-v",
        "verbose",
        "-init_hw_device",
        "vulkan=vk:4096",
        "-f",
        "lavfi",
        "-i",
        "color=s=64x64:d=0.04",
        "-f",
        "null",
        "-",
    ]);
    match run_bounded(tools, &args, env.timeout, deadline, cancel).await {
        Probe::Done { stderr, .. } => Some(parse_listing(&stderr).into_iter().take(MAX_DEVICES).collect()),
        Probe::Inconclusive => None,
    }
}

/// The libplacebo round trip on device `index` (R8): exit 0 means usable.
async fn round_trip(
    tools: &Tools,
    index: u32,
    env: &ProbeEnv,
    deadline: Instant,
    cancel: &CancellationToken,
) -> Probe {
    let mut args = strings(&["-hide_banner", "-nostdin", "-v", "error", "-init_hw_device"]);
    args.push(format!("vulkan=vk:{index}"));
    args.extend(strings(&[
        "-filter_hw_device",
        "vk",
        "-f",
        "lavfi",
        "-i",
        "color=s=64x64:d=0.1",
        "-vf",
        "hwupload,libplacebo=w=128:h=128:format=yuv420p,hwdownload,format=yuv420p",
        "-f",
        "null",
        "-",
    ]));
    run_bounded(tools, &args, env.timeout, deadline, cancel).await
}

/// Self-test result: frames per second, a failure reason, or nothing learnt.
enum SelfTest {
    Fps(f32),
    Failed(String),
    Inconclusive,
}

/// Self-test: 60 frames of a 480x854 test pattern through the tier's graph and the preset's
/// encoder.
async fn self_test(
    tools: &Tools,
    env: &ProbeEnv,
    gpu: Option<u32>,
    deadline: Instant,
    cancel: &CancellationToken,
) -> SelfTest {
    use super::encode::{ColourTags, EncodeParams, video_args};
    use super::graph::{GraphInput, build, setparams_input, zscale_input};
    let preset = &env.preset;
    let tags = ColourTags::default();
    let geometry = super::geometry::plan((480, 854), (preset.width, preset.height), super::Reframe::Center);
    let graph = build(&GraphInput {
        trim: None,
        deinterlace: false,
        square_pixels: false,
        tonemap: false,
        keep_hdr: false,
        colour_in: zscale_input(&tags, 854),
        fps_cap: None,
        geometry,
        reframe: super::Reframe::Center,
        track: None,
        gpu,
        params_in: setparams_input(&tags, 854),
    });
    let mut args = strings(&["-hide_banner", "-nostdin", "-v", "error"]);
    if let Some(n) = gpu {
        args.extend([
            "-init_hw_device".into(),
            format!("vulkan=vk:{n}"),
            "-filter_hw_device".into(),
            "vk".into(),
        ]);
    }
    args.extend(strings(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=480x854:r=30",
        "-frames:v",
        "60",
        "-filter_complex",
    ]));
    args.push(graph);
    args.extend(strings(&["-map", "[v]"]));
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get() as u32).clamp(2, 8);
    args.extend(video_args(&EncodeParams {
        preset,
        crf: None,
        hdr_keep: None,
        fps_out: 30.0,
        threads,
        maxrate_kbps: None,
    }));
    args.extend(strings(&["-f", "null", "-"]));
    let start = Instant::now();
    match run_bounded(tools, &args, env.timeout, deadline, cancel).await {
        Probe::Done { ok: true, .. } => SelfTest::Fps(60.0 / start.elapsed().as_secs_f32().max(0.001)),
        Probe::Done { ok: false, stderr } => SelfTest::Failed(
            stderr
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty() && !l.contains("FINISHME"))
                .map_or_else(|| "failed".to_owned(), |l| clean(l, 200)),
        ),
        Probe::Inconclusive => SelfTest::Inconclusive,
    }
}

/// `s` without control characters, at most `max` characters (text from ffmpeg and drivers is
/// shown in terminals and stored).
fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

/// Bumped when the self-tests or the tier graphs change, so stored speeds are measured again.
const CHECK_REVISION: u32 = 2;

/// The capability record for this machine (research R8):
/// - the stored one when its fingerprint matches (unless `mode` is `Recheck`);
/// - with `Cached`, nothing else: `None` when there is no matching record;
/// - otherwise a fresh check with the self-tests, stored with its demotions cleared.
///
/// Problems become reasons. A check cut short by `cancel` or a timeout is returned for this run
/// but not stored; `None` when not even the listing finished.
pub async fn ensure(
    tools: &Tools,
    store: &CapabilityStore,
    env: &ProbeEnv,
    mode: CheckMode,
    cancel: &CancellationToken,
) -> Option<CapabilityRecord> {
    let deadline = Instant::now() + env.budget;
    let listed = listing(tools, env, deadline, cancel).await?;
    let fp = fingerprint(&FingerprintInputs {
        app_version: format!("{}+check{CHECK_REVISION}", env!("CARGO_PKG_VERSION")),
        ffmpeg_version: tools.version.text().into(),
        listing: listed
            .iter()
            .map(|d| format!("{}: {} ({:?}) ({})", d.index, d.name, d.kind, d.device_id))
            .collect(),
        model_shas: Vec::new(),
        drivers: driver_identity(),
        vk_env: vk_env(),
    });
    if mode != CheckMode::Recheck
        && let Some(r) = store.load().filter(|r| r.fingerprint == fp)
    {
        return Some(r);
    }
    if mode == CheckMode::Cached {
        return None;
    }
    let mut conclusive = true;
    let mut devices = Vec::new();
    for d in &listed {
        let reason = match reject_reason(d) {
            Some(r) => Some(r.to_owned()),
            None => match round_trip(tools, d.index, env, deadline, cancel).await {
                Probe::Done { ok: true, .. } => None,
                Probe::Done { ok: false, .. } => Some("the libplacebo test failed on this device".to_owned()),
                Probe::Inconclusive => {
                    conclusive = false;
                    Some("the check did not finish".to_owned())
                }
            },
        };
        devices.push(GpuDevice {
            index: d.index,
            name: clean(&d.name, 200),
            kind: d.kind,
            device_id: clean(&d.device_id, 32),
            usable: reason.is_none(),
            reason,
        });
    }
    let mut tier = |t: SelfTest, device: Option<u32>| match t {
        SelfTest::Fps(fps) => TierInfo { available: true, device, fps: Some(fps), ..TierInfo::default() },
        SelfTest::Failed(e) => {
            TierInfo { device, reason: Some(format!("self-test failed: {e}")), ..TierInfo::default() }
        }
        SelfTest::Inconclusive => {
            conclusive = false;
            TierInfo { device, reason: Some("the check did not finish".into()), ..TierInfo::default() }
        }
    };
    // The CPU tier always works; a failed self-test only loses the speed figure.
    let cpu = TierInfo { available: true, ..tier(self_test(tools, env, None, deadline, cancel).await, None) };
    let gpu = match preferred(&devices) {
        None if listed.is_empty() => {
            TierInfo { reason: Some("no Vulkan device".into()), ..TierInfo::default() }
        }
        None => TierInfo {
            reason: Some("no usable hardware GPU (only software renderers)".into()),
            ..TierInfo::default()
        },
        Some(index) => tier(self_test(tools, env, Some(index), deadline, cancel).await, Some(index)),
    };
    let record = CapabilityRecord {
        schema: 1,
        fingerprint: fp,
        checked_at: rfc3339(SystemTime::now()),
        devices,
        tiers: TierStatus { cpu, gpu, ai: ai_not_built() },
        demoted: Vec::new(),
    };
    if !conclusive {
        return Some(record);
    }
    let store = store.clone();
    let fresh = record.clone();
    let stored = tokio::task::spawn_blocking(move || {
        // Another process may have finished the same check meanwhile; keep its record (and any
        // demotion made since) unless this is a recheck.
        store.update(|prev| match prev {
            Some(p) if mode != CheckMode::Recheck && p.fingerprint == fresh.fingerprint => p,
            _ => fresh,
        })
    })
    .await;
    match stored {
        Ok(Ok(r)) => Some(r),
        Ok(Err(e)) => {
            tracing::warn!("could not store the capability record: {e}");
            Some(record)
        }
        Err(e) => {
            tracing::warn!("capability store task failed: {e}");
            Some(record)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTEL_LLVMPIPE: &str = "[Vulkan @ 0x560db82ebc80] GPU listing:\n\
        [Vulkan @ 0x560db82ebc80]     0: Intel(R) Graphics (RPL-U) (integrated) (0xa721)\n\
        MESA-INTEL: warning: ../src/intel/vulkan/anv_formats.c:981: FINISHME: support more multi-planar formats\n\
        [Vulkan @ 0x560db82ebc80]     1: llvmpipe (LLVM 20.1.2, 256 bits) (software) (0x0)\n\
        [Vulkan @ 0x560db82ebc80] Device 0 selected: Intel(R) Graphics (RPL-U) (integrated) (0xa721)\n\
        [Vulkan @ 0x560db82ebc80]     0: graphics compute transfer sparse protected (queues: 1)\n";

    fn listed(text: &str) -> Vec<(u32, String, DeviceKind, Option<&'static str>)> {
        parse_listing(text)
            .into_iter()
            .map(|d| (d.index, d.name.clone(), d.kind, reject_reason(&d)))
            .collect()
    }

    #[test]
    fn listing_table() {
        assert_eq!(
            listed(INTEL_LLVMPIPE),
            vec![
                (0, "Intel(R) Graphics (RPL-U)".into(), DeviceKind::Integrated, None),
                (
                    1,
                    "llvmpipe (LLVM 20.1.2, 256 bits)".into(),
                    DeviceKind::Software,
                    Some("software renderer")
                ),
            ]
        );
        let nvidia = "[Vulkan @ 0x1] GPU listing:\n[Vulkan @ 0x1]     0: NVIDIA GeForce RTX 4070 (discrete) (0x2786)\n";
        assert_eq!(listed(nvidia)[0].2, DeviceKind::Discrete);
        let amd_lvp = "[Vulkan @ 0x1] GPU listing:\n[Vulkan @ 0x1]     0: AMD Radeon RX 7600 (RADV NAVI33) (discrete) (0x7480)\n[Vulkan @ 0x1]     1: llvmpipe-ish lavapipe (other) (0x0)\n";
        let l = listed(amd_lvp);
        assert_eq!((l[0].1.as_str(), l[0].3), ("AMD Radeon RX 7600 (RADV NAVI33)", None));
        assert_eq!(l[1].3, Some("software renderer"), "lavapipe by name, whatever the kind");
        let swift = "[Vulkan @ 0x1] GPU listing:\n[Vulkan @ 0x1]     0: SwiftShader Device (Subzero) (cpu) (0xc0de)\n";
        assert_eq!(listed(swift)[0].3, Some("software renderer"));
        assert!(
            parse_listing("Device creation failed: -19.\nFailed to set value 'vulkan=vk:0'\n").is_empty()
        );
        assert!(parse_listing("").is_empty());
    }

    fn device(index: u32, kind: DeviceKind, usable: bool) -> GpuDevice {
        GpuDevice { index, name: "x".into(), kind, device_id: "0x1".into(), usable, reason: None }
    }

    #[test]
    fn discrete_is_preferred_over_integrated() {
        let d = [
            device(0, DeviceKind::Integrated, true),
            device(1, DeviceKind::Discrete, true),
            device(2, DeviceKind::Software, false),
        ];
        assert_eq!(preferred(&d), Some(1));
        assert_eq!(preferred(&d[..1]), Some(0));
        assert_eq!(preferred(&[device(1, DeviceKind::Discrete, false)]), None);
    }

    #[test]
    fn fingerprint_changes_with_drivers_and_versions() {
        let d = tempfile::tempdir().unwrap();
        let lib = d.path().join("libvulkan_x.so");
        std::fs::write(&lib, b"lib").unwrap();
        let manifest = d.path().join("x_icd.json");
        let write = |api: &str| {
            std::fs::write(
                &manifest,
                format!(r#"{{"ICD":{{"library_path":"{}","api_version":"{api}"}}}}"#, lib.display()),
            )
            .unwrap();
        };
        write("1.3.0");
        let inputs = |drivers| FingerprintInputs {
            app_version: "0.1.0".into(),
            ffmpeg_version: "n9".into(),
            drivers,
            ..Default::default()
        };
        let base = fingerprint(&inputs(driver_identity_dirs(&[d.path().to_path_buf()])));
        assert_eq!(base.len(), 64);
        assert_eq!(base, fingerprint(&inputs(driver_identity_dirs(&[d.path().to_path_buf()]))), "stable");
        write("1.3.1");
        let edited = fingerprint(&inputs(driver_identity_dirs(&[d.path().to_path_buf()])));
        assert_ne!(edited, base, "manifest edit");
        let later = SystemTime::now() + Duration::from_secs(60);
        std::fs::File::options().write(true).open(&lib).unwrap().set_modified(later).unwrap();
        assert_ne!(
            fingerprint(&inputs(driver_identity_dirs(&[d.path().to_path_buf()]))),
            edited,
            "driver library updated"
        );
        let mut i = inputs(vec![]);
        let a = fingerprint(&i);
        i.app_version = "0.2.0".into();
        assert_ne!(fingerprint(&i), a, "app version");
        i.ffmpeg_version = "n10".into();
        i.vk_env = vec!["VK_DRIVER_FILES=/x".into()];
        assert_ne!(fingerprint(&i), a);
    }

    #[test]
    fn rfc3339_dates() {
        assert_eq!(rfc3339(SystemTime::UNIX_EPOCH), "1970-01-01T00:00:00Z");
        let t = |s: u64| rfc3339(SystemTime::UNIX_EPOCH + Duration::from_secs(s));
        assert_eq!(t(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(t(1_792_398_645), "2026-10-19T08:30:45Z");
    }

    fn record() -> CapabilityRecord {
        CapabilityRecord {
            fingerprint: "ab".repeat(32),
            devices: vec![device(0, DeviceKind::Integrated, true)],
            tiers: TierStatus {
                cpu: TierInfo { available: true, fps: Some(48.0), ..Default::default() },
                gpu: TierInfo { available: true, device: Some(0), fps: Some(55.0), ..Default::default() },
                ai: ai_not_built(),
            },
            ..CapabilityRecord::default()
        }
    }

    #[test]
    fn record_round_trips_and_matches_the_contract_schema() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../specs/002-mobile-upscale/contracts/capability-record.schema.json");
        let schema: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let v = jsonschema::validator_for(&schema).unwrap();
        let mut r = record();
        r.demoted.push(Demotion { tier: Tier::Gpu, error: "boom".into(), at: rfc3339(SystemTime::now()) });
        let json = serde_json::to_value(&r).unwrap();
        let errors: Vec<String> = v.iter_errors(&json).map(|e| e.to_string()).collect();
        assert!(errors.is_empty(), "{errors:?}");
        assert!(v.is_valid(&serde_json::to_value(CapabilityRecord::default()).unwrap()));
        let d = tempfile::tempdir().unwrap();
        let store = CapabilityStore::in_state_dir(d.path());
        store.update(|_| r.clone()).unwrap();
        assert_eq!(store.load(), Some(r));
    }

    #[test]
    fn tampered_or_foreign_records_are_ignored() {
        let d = tempfile::tempdir().unwrap();
        let store = CapabilityStore::in_state_dir(d.path());
        let write = |v: serde_json::Value| std::fs::write(store.path(), v.to_string()).unwrap();
        let mut v = serde_json::to_value(record()).unwrap();
        v["schema"] = 2.into();
        write(v.clone());
        assert_eq!(store.load(), None, "schema 2");
        v["schema"] = 1.into();
        v["fingerprint"] = "not-hex".into();
        write(v.clone());
        assert_eq!(store.load(), None);
        v["fingerprint"] = "ab".repeat(32).into();
        v["extra"] = 1.into();
        write(v);
        assert_eq!(store.load(), None, "unknown fields");
        std::fs::write(store.path(), " ".repeat(70 * 1024)).unwrap();
        assert_eq!(store.load(), None, "oversized");
        let good = serde_json::to_value(record()).unwrap();
        write(good.clone());
        assert!(store.load().is_some());
        for (path, bad) in [
            ("/tiers/gpu/device", serde_json::json!(7)),
            ("/tiers/cpu/fps", serde_json::json!(-1.0)),
            ("/tiers/cpu/fps", serde_json::json!(1e9)),
            ("/devices/0/name", serde_json::json!("GPU\u{1b}[31m")),
            ("/devices/0/usable", serde_json::json!(false)),
            ("/checked_at", serde_json::json!("x".repeat(600))),
        ] {
            let mut v = good.clone();
            *v.pointer_mut(path).unwrap() = bad.clone();
            write(v);
            assert_eq!(store.load(), None, "{path} = {bad}");
        }
    }

    #[test]
    fn demoting_without_a_record_writes_nothing() {
        let d = tempfile::tempdir().unwrap();
        let store = CapabilityStore::in_state_dir(d.path());
        demote(&store, Tier::Gpu, "x").unwrap();
        assert!(!store.path().exists());
    }

    #[test]
    fn bare_driver_library_names_are_found_in_the_loader_folders() {
        let d = tempfile::tempdir().unwrap();
        let m = d.path().join("x_icd.json");
        // libc is in one of the loader folders on any glibc system.
        let lib = LIBRARY_DIRS.iter().map(|dir| Path::new(dir).join("libc.so.6")).find(|p| p.is_file());
        std::fs::write(&m, r#"{"ICD":{"library_path":"libc.so.6"}}"#).unwrap();
        let id = manifest_identity(std::slice::from_ref(&m));
        if let Some(lib) = lib {
            assert!(id[0].contains(&lib.display().to_string()), "{id:?}");
        }
    }

    #[test]
    fn demotions_persist_and_concurrent_updates_keep_both() {
        let d = tempfile::tempdir().unwrap();
        let store = CapabilityStore::in_state_dir(d.path());
        store.update(|_| record()).unwrap();
        demote(&store, Tier::Gpu, "vk lost").unwrap();
        let r = store.load().unwrap();
        assert_eq!(r.is_demoted(Tier::Gpu).map(|d| d.error.as_str()), Some("vk lost"));
        demote(&store, Tier::Gpu, "again").unwrap();
        assert_eq!(store.load().unwrap().demoted.len(), 1, "one demotion per tier");
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let s = store.clone();
                std::thread::spawn(move || {
                    s.update(|r| {
                        let mut r = r.unwrap();
                        r.devices.push(device(10 + i, DeviceKind::Virtual, false));
                        r
                    })
                    .unwrap();
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(store.load().unwrap().devices.len(), 9, "no lost updates under the lock");
    }
}
