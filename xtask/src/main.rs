//! Developer tasks: `cargo xtask fetch-tools [--target TRIPLE]`, `cargo xtask pin-tools`.
//!
//! Engine binaries are downloaded at build time from a pinned, SHA-256-verified URL and placed in
//! `binaries/` using Tauri's sidecar naming (`rmedia-ffmpeg-<triple>[.exe]`). They are never
//! committed (Constitution, Technical & Licensing Constraints).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize, Deserialize)]
struct Lock {
    comment: String,
    release: String,
    version: String,
    targets: BTreeMap<String, Pinned>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Pinned {
    url: String,
    sha256: String,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("workspace root").to_path_buf()
}

fn host_triple() -> &'static str {
    if cfg!(windows) { "x86_64-pc-windows-msvc" } else { "x86_64-unknown-linux-gnu" }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("fetch-tools") => {
            let target = match args.iter().position(|a| a == "--target") {
                Some(i) => args.get(i + 1).context("--target needs a value")?.clone(),
                None => host_triple().to_owned(),
            };
            fetch_tools(&target)
        }
        Some("pin-tools") => pin_tools(),
        _ => {
            eprintln!("usage: cargo xtask <fetch-tools [--target TRIPLE] | pin-tools>");
            std::process::exit(2);
        }
    }
}

fn read_lock() -> Result<Lock> {
    let path = root().join("xtask/tools.lock.json");
    serde_json::from_str(&std::fs::read_to_string(&path)?)
        .with_context(|| format!("parse {}", path.display()))
}

fn fetch_tools(target: &str) -> Result<()> {
    let lock = read_lock()?;
    let pin = lock.targets.get(target).with_context(|| format!("no pinned build for {target}"))?;
    let exe = if target.contains("windows") { ".exe" } else { "" };
    let bin_dir = root().join("binaries");
    std::fs::create_dir_all(&bin_dir)?;
    let stamp = bin_dir.join(format!(".{target}.sha256"));
    let dest_ffmpeg = bin_dir.join(format!("rmedia-ffmpeg-{target}{exe}"));
    let dest_ffprobe = bin_dir.join(format!("rmedia-ffprobe-{target}{exe}"));
    // Stamp: archive hash + hashes of the installed binaries, re-verified on every reuse so a
    // tampered or corrupted binary in binaries/ is replaced rather than trusted.
    let expected_stamp = |ff: &Path, fp: &Path| -> Option<String> {
        Some(format!("{}\n{}\n{}\n", pin.sha256, file_sha256(ff).ok()?, file_sha256(fp).ok()?))
    };
    if let (Ok(stamped), Some(actual)) =
        (std::fs::read_to_string(&stamp), expected_stamp(&dest_ffmpeg, &dest_ffprobe))
        && stamped == actual
    {
        println!("engine {} for {target} already present (verified)", lock.version);
        return Ok(());
    }

    let work = root().join("target/xtask-tools");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let archive = work.join(pin.url.rsplit('/').next().context("url file name")?);
    println!("downloading {}", pin.url);
    run(Command::new("curl")
        .args(["-fL", "--retry", "3", "--silent", "--show-error", "-o"])
        .arg(&archive)
        .arg(&pin.url))?;

    let actual = format!("{:x}", Sha256::digest(std::fs::read(&archive)?));
    if actual != pin.sha256 {
        bail!("checksum mismatch for {}: expected {}, got {actual}", pin.url, pin.sha256);
    }
    let extract = work.join("x");
    std::fs::create_dir_all(&extract)?;
    // bsdtar (Windows 10+) and GNU tar both handle these formats.
    run(Command::new("tar").arg("-xf").arg(&archive).arg("-C").arg(&extract))?;

    let find = |name: &str| -> Result<PathBuf> {
        walk(&extract)
            .into_iter()
            .find(|p| {
                p.file_name().is_some_and(|n| n == name) && p.parent().is_some_and(|d| d.ends_with("bin"))
            })
            .with_context(|| format!("{name} not found in archive"))
    };
    std::fs::copy(find(&format!("ffmpeg{exe}"))?, &dest_ffmpeg)?;
    std::fs::copy(find(&format!("ffprobe{exe}"))?, &dest_ffprobe)?;
    if let Some(license) =
        walk(&extract).into_iter().find(|p| p.file_name().is_some_and(|n| n == "LICENSE.txt"))
    {
        std::fs::copy(license, bin_dir.join("FFMPEG-LICENSE.txt"))?;
    }
    #[cfg(unix)]
    for p in [&dest_ffmpeg, &dest_ffprobe] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::write(&stamp, expected_stamp(&dest_ffmpeg, &dest_ffprobe).context("hash installed binaries")?)?;
    let _ = std::fs::remove_dir_all(&work);
    println!("installed {} and {}", dest_ffmpeg.display(), dest_ffprobe.display());
    println!("use them for tests with RMEDIA_FFMPEG / RMEDIA_FFPROBE");
    Ok(())
}

/// Pins the newest BtbN autobuild of the current release line, using GitHub's asset digests.
fn pin_tools() -> Result<()> {
    let mut lock = read_lock()?;
    let line = lock.version.trim_start_matches('n').split('.').take(2).collect::<Vec<_>>().join(".");
    let out = Command::new("curl")
        .args(["-fsSL", "https://api.github.com/repos/BtbN/FFmpeg-Builds/releases?per_page=5"])
        .output()?;
    if !out.status.success() {
        bail!("GitHub API request failed");
    }
    let releases: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let release = releases
        .as_array()
        .context("releases array")?
        .iter()
        .find(|r| r["tag_name"].as_str().is_some_and(|t| t.starts_with("autobuild-")))
        .context("no autobuild release")?;
    let tag = release["tag_name"].as_str().unwrap_or_default().to_owned();
    for (triple, flavour, ext) in [
        ("x86_64-unknown-linux-gnu", "linux64-gpl", ".tar.xz"),
        ("x86_64-pc-windows-msvc", "win64-gpl", ".zip"),
    ] {
        let suffix = format!("-{flavour}-{line}{ext}");
        let asset = release["assets"]
            .as_array()
            .context("assets")?
            .iter()
            .find(|a| {
                a["name"]
                    .as_str()
                    .is_some_and(|n| n.starts_with(&format!("ffmpeg-n{line}")) && n.ends_with(&suffix))
            })
            .with_context(|| format!("no {suffix} asset in {tag}"))?;
        let name = asset["name"].as_str().unwrap_or_default();
        let digest =
            asset["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).context("asset digest")?;
        lock.version = name.trim_start_matches("ffmpeg-").trim_end_matches(&suffix).to_owned();
        lock.targets.insert(
            triple.to_owned(),
            Pinned {
                url: asset["browser_download_url"].as_str().unwrap_or_default().to_owned(),
                sha256: digest.to_owned(),
            },
        );
    }
    lock.release = tag;
    std::fs::write(root().join("xtask/tools.lock.json"), serde_json::to_string_pretty(&lock)? + "\n")?;
    println!("pinned {} ({})", lock.version, lock.release);
    Ok(())
}

fn file_sha256(path: &Path) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(std::fs::read(path)?)))
}

fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("failed to run {cmd:?}"))?;
    if !status.success() {
        bail!("{cmd:?} exited with {status}");
    }
    Ok(())
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}
