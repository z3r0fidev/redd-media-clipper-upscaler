//! The Vulkan driver files and loader environment that go into the fingerprint (a driver update re-runs the self-tests).

use super::*;

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
            Some(format!(
                "{} {} {}",
                m.display(),
                crate::hex(&Sha256::digest(&text)),
                lib.unwrap_or_default()
            ))
        })
        .collect();
    out.sort();
    out
}

/// Where the dynamic loader finds a driver named without a path (Linux; harmless elsewhere).
pub(super) const LIBRARY_DIRS: &[&str] = &[
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
pub(super) fn windows_manifests() -> Vec<PathBuf> {
    use windows::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegEnumValueW, RegOpenKeyExW,
    };
    use windows::core::{PCWSTR, PWSTR};
    let sub: Vec<u16> = "SOFTWARE\\Khronos\\Vulkan\\Drivers\0".encode_utf16().collect();
    let mut key = HKEY::default();
    let mut out = Vec::new();
    // One of the listed `unsafe` exceptions (docs/security/security-model.md).
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

pub(super) fn vk_env() -> Vec<String> {
    ["VK_DRIVER_FILES", "VK_ICD_FILENAMES", "VK_ADD_DRIVER_FILES"]
        .iter()
        .map(|v| format!("{v}={}", std::env::var(v).unwrap_or_default()))
        .collect()
}
