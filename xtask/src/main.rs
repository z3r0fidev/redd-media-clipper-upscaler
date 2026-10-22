//! Developer tasks: `cargo xtask fetch-tools [--target TRIPLE]`, `cargo xtask pin-tools`,
//! `cargo xtask fetch-models`, `cargo xtask fetch-ncnn [--target TRIPLE]`, `cargo xtask archive`,
//! `cargo xtask mirror <tag>`, `cargo xtask check-pins`.
//!
//! Engine binaries are downloaded at build time from a pinned, SHA-256-verified URL and placed in
//! `binaries/` using Tauri's sidecar naming (`rmedia-ffmpeg-<triple>[.exe]`). They are never
//! committed (Constitution, Technical & Licensing Constraints). Model files follow the same rule:
//! `xtask/models.lock.json` pins them by URL and SHA-256, and they land in `models/`.
//!
//! Every pinned archive is kept in `archives/<sha256>/` (gitignored), because upstream builds can
//! disappear (BtbN deletes old FFmpeg autobuilds). A pin may list `mirrors`, our own copies of the
//! same bytes, tried before the upstream `url`; whatever the source, the SHA-256 must match.

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
    /// The BtbN FFmpeg-Builds commit `release` pointed at when pinned: the build scripts of the
    /// shipped binaries, checked by `.github/workflows/ffmpeg-source.yml` (tags can move).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scripts_commit: Option<String>,
    /// The release holding this build's complete corresponding source (GPL v3 §6), published by
    /// `.github/workflows/ffmpeg-source.yml`; `cargo xtask mirror` fills it in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    targets: BTreeMap<String, Pinned>,
}

#[derive(Debug, Deserialize)]
struct ModelLock {
    models: BTreeMap<String, PinnedModel>,
    /// Prebuilt ncnn (static, Vulkan) per target triple, for the `rmedia-sr` worker (US6).
    #[serde(default)]
    ncnn: BTreeMap<String, Pinned>,
}

#[derive(Debug, Deserialize)]
struct PinnedModel {
    /// Direct download; or `archive` when the file only ships inside a release archive.
    #[serde(default)]
    url: Option<String>,
    /// Our own copies of a direct `url` download (same SHA-256), tried first.
    #[serde(default)]
    mirrors: Vec<String>,
    #[serde(default)]
    archive: Option<ArchiveMember>,
    sha256: String,
    /// Repository path of the licence text, copied into the notices.
    license: String,
    description: String,
    /// Fetched for tests only; never shipped, so not listed in the notices.
    #[serde(default)]
    test_only: bool,
}

/// A file inside a pinned release archive: the archive and the file are both SHA-256 verified.
#[derive(Debug, Deserialize)]
struct ArchiveMember {
    url: String,
    #[serde(default)]
    mirrors: Vec<String>,
    sha256: String,
    member: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Pinned {
    url: String,
    /// Our own copies of the archive, tried before `url` (same SHA-256).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mirrors: Vec<String>,
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
        Some("gen-bindings") => gen_bindings(),
        Some("stage-cli") => {
            let target = match args.iter().position(|a| a == "--target") {
                Some(i) => args.get(i + 1).context("--target needs a value")?.clone(),
                None => host_triple().to_owned(),
            };
            stage_cli(&target)
        }
        Some("notices") => notices(),
        Some("fetch-models") => fetch_models(),
        Some("fetch-ncnn") => {
            let target = match args.iter().position(|a| a == "--target") {
                Some(i) => args.get(i + 1).context("--target needs a value")?.clone(),
                None => host_triple().to_owned(),
            };
            fetch_ncnn(&target)
        }
        Some("archive") => archive_all(),
        Some("mirror") => {
            mirror(args.get(1).context("usage: cargo xtask mirror <tag>, e.g. mirror-2026-10-22")?)
        }
        Some("check-pins") => {
            check_pins()?;
            if args.iter().any(|a| a == "--online") { check_mirrors_online() } else { Ok(()) }
        }
        _ => {
            eprintln!(
                "usage: cargo xtask <fetch-tools [--target TRIPLE] | pin-tools | gen-bindings | stage-cli [--target TRIPLE] | notices | fetch-models | fetch-ncnn [--target TRIPLE] | archive | mirror <tag> | check-pins [--online]>"
            );
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

    let archive = pinned_archive(&pin.url, &pin.mirrors, &pin.sha256)?;
    let work = root().join("target/xtask-tools");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let extract = work.join("x");
    std::fs::create_dir_all(&extract)?;
    run(Command::new(tar()).arg("-xf").arg(&archive).arg("-C").arg(&extract))?;

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
fn read_model_lock() -> Result<ModelLock> {
    let path = root().join("xtask/models.lock.json");
    serde_json::from_str(&std::fs::read_to_string(&path)?)
        .with_context(|| format!("parse {}", path.display()))
}

/// Downloads every pinned model into `models/`, verifying SHA-256. Files already present with the
/// right hash are kept; anything else is replaced through a temp file and a rename.
fn fetch_models() -> Result<()> {
    let lock = read_model_lock()?;
    let dir = root().join("models");
    std::fs::create_dir_all(&dir)?;
    for (name, pin) in &lock.models {
        if name.contains(['/', '\\']) || name.starts_with('.') {
            bail!("bad model file name {name:?} in models.lock.json");
        }
        let dest = dir.join(name);
        if file_sha256(&dest).is_ok_and(|h| h == pin.sha256) {
            println!("model {name} already present (verified)");
            continue;
        }
        let tmp = dir.join(format!(".{name}.download"));
        let _ = std::fs::remove_file(&tmp); // never write through a planted file or link
        println!("fetching {name}: {}", pin.description);
        match (&pin.url, &pin.archive) {
            (Some(url), None) => {
                std::fs::copy(pinned_archive(url, &pin.mirrors, &pin.sha256)?, &tmp)?;
            }
            (None, Some(a)) => {
                let extracted = fetch_archive(&a.url, &a.mirrors, &a.sha256)?;
                let member = extracted.join(&a.member);
                if a.member.contains("..") || !member.starts_with(&extracted) {
                    bail!("bad archive member {:?} for {name}", a.member);
                }
                std::fs::copy(&member, &tmp).with_context(|| format!("{} not in {}", a.member, a.url))?;
            }
            _ => bail!("{name}: set exactly one of url and archive in models.lock.json"),
        }
        let actual = file_sha256(&tmp)?;
        if actual != pin.sha256 {
            let _ = std::fs::remove_file(&tmp);
            bail!("checksum mismatch for {name}: expected {}, got {actual}", pin.sha256);
        }
        std::fs::rename(&tmp, &dest)?;
    }
    Ok(())
}

/// Downloads `url` to `dest` over HTTPS only.
fn download(url: &str, dest: &Path) -> Result<()> {
    run(Command::new("curl")
        .args([
            "-fL",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--retry",
            "3",
            "--max-time",
            "900",
            "--max-filesize",
            "500000000",
            "--silent",
            "--show-error",
            "-o",
        ])
        .arg(dest)
        .arg("--url")
        .arg(url))
}

/// The pinned archive, kept as `archives/<sha256>/<file name>`: reused while its hash matches,
/// otherwise downloaded from each mirror and then `url` until one matches the pin.
fn pinned_archive(url: &str, mirrors: &[String], sha256: &str) -> Result<PathBuf> {
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        bail!("bad sha256 {sha256:?} for {url}");
    }
    let name = url
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty() && !n.starts_with('.') && !n.contains(['\\', ':']))
        .with_context(|| format!("no file name in {url}"))?;
    if let Some(bad) = mirrors.iter().map(String::as_str).chain([url]).find(|u| !u.starts_with("https://")) {
        bail!("pinned URLs must be https: {bad:?}");
    }
    let dir = root().join("archives").join(sha256);
    let kept = dir.join(name);
    // A kept copy is reused only as a regular file (never through a link) whose hash matches.
    if std::fs::symlink_metadata(&kept).is_ok_and(|m| m.is_file())
        && file_sha256(&kept).is_ok_and(|h| h == sha256)
    {
        return Ok(kept);
    }
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!(".{name}.download"));
    let mut failures = Vec::new();
    for source in mirrors.iter().map(String::as_str).chain([url]) {
        let _ = std::fs::remove_file(&tmp); // never write through a planted file or link
        println!("downloading {source}");
        match download(source, &tmp).and_then(|()| file_sha256(&tmp)) {
            Ok(h) if h == sha256 => {
                std::fs::rename(&tmp, &kept)?;
                return Ok(kept);
            }
            Ok(h) => failures.push(format!("{source}: checksum {h}, expected {sha256}")),
            Err(e) => failures.push(format!("{source}: {e:#}")),
        }
    }
    let _ = std::fs::remove_file(&tmp);
    bail!("no source of {name} matches the pin:\n  {}", failures.join("\n  "))
}

/// Extracts a pinned archive ([`pinned_archive`]) once into `target/xtask-archives/<sha256>/`.
/// Returns the extraction directory.
fn fetch_archive(url: &str, mirrors: &[String], sha256: &str) -> Result<PathBuf> {
    let base = root().join("target/xtask-archives");
    let done = base.join(format!("{sha256}.ok"));
    let extract = base.join(sha256);
    if done.is_file() && extract.is_dir() {
        return Ok(extract);
    }
    let archive = pinned_archive(url, mirrors, sha256)?;
    let _ = std::fs::remove_dir_all(&extract);
    std::fs::create_dir_all(&extract)?;
    if url.ends_with(".zip") && !cfg!(windows) {
        // GNU tar cannot read .zip.
        run(Command::new("unzip").arg("-q").arg(&archive).arg("-d").arg(&extract))?;
    } else {
        run(Command::new(tar()).arg("-xf").arg(&archive).arg("-C").arg(&extract))?;
    }
    std::fs::write(&done, url)?;
    Ok(extract)
}

/// bsdtar (Windows 10+) reads .zip and .tar.xz; GNU tar only the latter. On Windows use the system
/// bsdtar; a GNU tar earlier on PATH (Git Bash) cannot read .zip.
fn tar() -> String {
    if cfg!(windows) {
        std::env::var("SystemRoot").map(|r| format!(r"{r}\System32\tar.exe")).unwrap_or_else(|_| "tar".into())
    } else {
        "tar".into()
    }
}

/// Installs the pinned prebuilt ncnn for `target` into `third_party/ncnn/<target>/` (include/ and
/// lib/ of the release package), for `crates/rmedia-sr/build.rs`. Never committed.
fn fetch_ncnn(target: &str) -> Result<()> {
    let lock = read_model_lock()?;
    let pin = lock.ncnn.get(target).with_context(|| format!("no pinned ncnn build for {target}"))?;
    let dest = root().join("third_party/ncnn").join(target);
    let stamp = dest.join(".sha256");
    if std::fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == pin.sha256) {
        println!("ncnn for {target} already present");
        return Ok(());
    }
    let extracted = fetch_archive(&pin.url, &pin.mirrors, &pin.sha256)?;
    // The package has one top-level folder (ncnn-<version>-<platform>/).
    let top = std::fs::read_dir(&extracted)?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_dir())
        .context("empty ncnn archive")?;
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::create_dir_all(dest.parent().context("parent")?)?;
    copy_dir(&top, &dest)?;
    std::fs::write(&stamp, &pin.sha256)?;
    println!("installed ncnn for {target} in {}", dest.display());
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type()?.is_dir() {
            copy_dir(&src, &dst)?;
        } else if e.file_type()?.is_file() {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

fn pin_tools() -> Result<()> {
    let mut lock = read_lock()?;
    let line = lock.version.trim_start_matches('n').split('.').take(2).collect::<Vec<_>>().join(".");
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "--proto",
            "=https",
            "https://api.github.com/repos/BtbN/FFmpeg-Builds/releases?per_page=5",
        ])
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
                mirrors: Vec::new(),
                sha256: digest.to_owned(),
            },
        );
    }
    let commit = release["target_commitish"].as_str().unwrap_or_default();
    let tag_ref = Command::new("curl")
        .args(["-fsSL", "--proto", "=https"])
        .arg(format!("https://api.github.com/repos/BtbN/FFmpeg-Builds/commits/{tag}"))
        .output()?;
    let sha: serde_json::Value = serde_json::from_slice(&tag_ref.stdout).unwrap_or_default();
    let sha = sha["sha"]
        .as_str()
        .filter(|s| is_commit(s))
        .with_context(|| format!("could not resolve the commit of {tag} (target_commitish {commit:?})"))?;
    lock.scripts_commit = Some(sha.to_owned());
    lock.release = tag;
    lock.source = None; // a new build needs its own source release
    // Keep the new builds now (the autobuild release will be deleted upstream), and write the
    // lock only once both are downloaded and verified.
    for pin in lock.targets.values() {
        pinned_archive(&pin.url, &pin.mirrors, &pin.sha256)?;
    }
    std::fs::write(root().join("xtask/tools.lock.json"), serde_json::to_string_pretty(&lock)? + "\n")?;
    println!("pinned {} ({})", lock.version, lock.release);
    println!(
        "next, while the autobuild tag still exists: `gh workflow run ffmpeg-source.yml`, then \
         `cargo xtask mirror mirror-<date>`, update licenses/FFMPEG-NOTICE.md, `cargo xtask check-pins`"
    );
    Ok(())
}

/// Downloads every pinned archive (FFmpeg and ncnn for all targets, model release archives) into
/// `archives/`, verified, and lists them: the set to upload as mirrors.
fn archive_all() -> Result<()> {
    let tools = read_lock()?;
    let models = read_model_lock()?;
    let mut pins: BTreeMap<&str, (&str, &[String])> = BTreeMap::new();
    for p in tools.targets.values().chain(models.ncnn.values()) {
        pins.insert(&p.sha256, (&p.url, &p.mirrors));
    }
    for a in models.models.values().filter_map(|m| m.archive.as_ref()) {
        pins.insert(&a.sha256, (&a.url, &a.mirrors));
    }
    for (sha256, (url, mirrors)) in pins {
        let kept = pinned_archive(url, mirrors, sha256)?;
        let size = std::fs::metadata(&kept)?.len();
        println!("{sha256}  {:>5} MB  {}", size >> 20, kept.strip_prefix(root()).unwrap_or(&kept).display());
    }
    println!("upload these with `cargo xtask mirror <tag>`");
    Ok(())
}

/// Every pinned archive as (upstream url, sha256, mirrors), one entry per distinct archive.
fn all_pins() -> Result<Vec<(String, String, Vec<String>)>> {
    Ok(pins_of(&read_lock()?, &read_model_lock()?))
}

/// Every pinned download in the two locks (FFmpeg, ncnn, model archives and direct model files)
/// as (upstream url, sha256, mirrors), one entry per distinct file.
fn pins_of(tools: &Lock, models: &ModelLock) -> Vec<(String, String, Vec<String>)> {
    let mut pins = BTreeMap::new();
    for p in tools.targets.values().chain(models.ncnn.values()) {
        pins.insert(p.sha256.clone(), (p.url.clone(), p.mirrors.clone()));
    }
    for m in models.models.values() {
        match (&m.url, &m.archive) {
            (Some(url), _) => pins.insert(m.sha256.clone(), (url.clone(), m.mirrors.clone())),
            (None, Some(a)) => pins.insert(a.sha256.clone(), (a.url.clone(), a.mirrors.clone())),
            (None, None) => None,
        };
    }
    pins.into_iter().map(|(sha, (url, mirrors))| (url, sha, mirrors)).collect()
}

fn gh(args: &[&str]) -> Result<String> {
    let out = Command::new("gh").args(args).output().context("run gh (GitHub CLI)")?;
    if !out.status.success() {
        bail!("gh {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_owned())
}

/// Uploads every pinned archive that has no mirror yet to the release `tag` of this repository
/// (created as a pre-release when missing) and lists the uploaded URLs under `mirrors` in the lock
/// files. Also records the FFmpeg source release when it exists. Needs `gh` with write access.
fn mirror(tag: &str) -> Result<()> {
    if !tag.starts_with("mirror-") || !tag.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b)) {
        bail!("tag must look like mirror-<date>, got {tag:?}");
    }
    let repo = gh(&["repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"])?;
    let missing: Vec<_> = all_pins()?.into_iter().filter(|(_, _, m)| m.is_empty()).collect();
    if !missing.is_empty() {
        let mut files = Vec::new();
        for (url, sha, _) in &missing {
            files.push(pinned_archive(url, &[], sha)?);
        }
        match gh(&["release", "view", tag, "-R", &repo, "--json", "tagName"]) {
            Ok(_) => {}
            Err(e) if format!("{e:#}").contains("release not found") => {
                gh(&[
                    "release",
                    "create",
                    tag,
                    "-R",
                    &repo,
                    "--prerelease",
                    "--title",
                    &format!("Third-party archive mirror {tag}"),
                    "--notes",
                    "Copies of the archives pinned in `xtask/tools.lock.json` and `xtask/models.lock.json` \
                 (`cargo xtask mirror`). `fetch-*` accept a file only when its SHA-256 matches the pin. \
                 Not an rmedia release. FFmpeg licence and source: licenses/FFMPEG-NOTICE.md.",
                ])?;
            }
            Err(e) => return Err(e),
        }
        // name -> "sha256:<hex>" of the assets already there; a same-name asset must be these bytes.
        let present = gh(&[
            "release",
            "view",
            tag,
            "-R",
            &repo,
            "--json",
            "assets",
            "-q",
            r#".assets[] | "\(.name) \(.digest)""#,
        ])?;
        let mut upload = vec!["release", "upload", tag, "-R", &repo];
        let names: Vec<String> = files.iter().map(|f| f.to_string_lossy().into_owned()).collect();
        for ((f, path), (_, sha, _)) in files.iter().zip(&names).zip(&missing) {
            let name = f.file_name().and_then(|n| n.to_str()).context("file name")?;
            let already = present.lines().any(|l| l.ends_with(&format!(" sha256:{sha}")));
            match present.lines().find_map(|l| l.strip_prefix(name)?.strip_prefix(' ')) {
                None if already => {}
                None => upload.push(path),
                Some(d) if d == format!("sha256:{sha}") => {}
                Some(d) => bail!("{tag} already has {name} with digest {d}, not sha256:{sha}; use a new tag"),
            }
        }
        if upload.len() > 5 {
            println!("uploading {} archive(s) to {repo} {tag}", upload.len() - 5);
            gh(&upload)?;
        }
        // GitHub may rename an asset on upload (for example `~` becomes `.`), so each mirror URL
        // uses the name GitHub stored for the file with that digest.
        let stored = gh(&[
            "release",
            "view",
            tag,
            "-R",
            &repo,
            "--json",
            "assets",
            "-q",
            r#".assets[] | "\(.digest) \(.name)""#,
        ])?;
        for (url, sha, _) in &missing {
            let name = stored
                .lines()
                .find_map(|l| l.strip_prefix(&format!("sha256:{sha} ")))
                .with_context(|| format!("{tag} has no asset with sha256:{sha} after the upload"))?;
            add_mirror(url, sha, &format!("https://github.com/{repo}/releases/download/{tag}/{name}"))?;
        }
    }
    let mut lock = read_lock()?;
    if lock.source.is_none() {
        let source_tag = format!("ffmpeg-source-{}", lock.version);
        match gh(&["release", "view", &source_tag, "-R", &repo, "--json", "url", "-q", ".url"]) {
            Ok(url) => {
                lock.source = Some(url);
                std::fs::write(
                    root().join("xtask/tools.lock.json"),
                    serde_json::to_string_pretty(&lock)? + "\n",
                )?;
            }
            Err(_) => println!("no release {source_tag} yet: run `gh workflow run ffmpeg-source.yml` first"),
        }
    }
    check_pins()
}

/// Lists `mirror` under `mirrors` for every pin of `url` with `sha256` in both lock files. The
/// models lock is edited as text, keeping its layout; both are re-read to confirm the result.
fn add_mirror(url: &str, sha256: &str, mirror: &str) -> Result<()> {
    let tools_path = root().join("xtask/tools.lock.json");
    let models_path = root().join("xtask/models.lock.json");
    let mut lock = read_lock()?;
    for p in lock.targets.values_mut().filter(|p| p.url == url && p.sha256 == sha256) {
        p.mirrors.push(mirror.to_owned());
    }
    let tools_text = serde_json::to_string_pretty(&lock)? + "\n";
    let models_text = with_mirror_text(&std::fs::read_to_string(&models_path)?, url, mirror)?;
    // Check the result before writing anything, so a failed edit leaves both files as they were.
    let (tools, models): (Lock, ModelLock) =
        (serde_json::from_str(&tools_text)?, serde_json::from_str(&models_text)?);
    if pins_of(&tools, &models)
        .iter()
        .any(|(u, s, m)| u == url && s == sha256 && !m.iter().any(|x| x == mirror))
    {
        bail!(
            "could not add the mirror for {url} (is xtask/models.lock.json pretty-printed with LF line ends?)"
        );
    }
    for (path, text) in [(&tools_path, tools_text), (&models_path, models_text)] {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)?;
    }
    Ok(())
}

/// `check-pins --online`: every mirror URL answers (HEAD over HTTPS, redirects followed), so a
/// renamed or deleted mirror asset is noticed instead of silently falling back to upstream.
fn check_mirrors_online() -> Result<()> {
    let mut broken = Vec::new();
    for (_, _, mirrors) in all_pins()? {
        for m in mirrors {
            let ok = Command::new("curl")
                .args(["-fsSIL", "--proto", "=https", "--proto-redir", "=https", "--max-time", "60", "-o"])
                .arg(if cfg!(windows) { "NUL" } else { "/dev/null" })
                .arg("--url")
                .arg(&m)
                .status()
                .is_ok_and(|s| s.success());
            if !ok {
                broken.push(m);
            }
        }
    }
    if broken.is_empty() {
        println!("mirrors: every URL answers");
        Ok(())
    } else {
        bail!("mirror URLs that do not answer:\n  {}", broken.join("\n  "))
    }
}

/// A full git commit id (40 lowercase hex).
fn is_commit(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `text` (pretty-printed JSON) with `"mirrors": [mirror]` after each `"url": <url>,` line that
/// has no mirrors yet, at the same indentation.
fn with_mirror_text(text: &str, url: &str, mirror: &str) -> Result<String> {
    let needle = format!("\"url\": {},\n", serde_json::to_string(url)?);
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(&needle) {
        let line_start = rest[..i].rfind('\n').map_or(0, |n| n + 1);
        let indent = &rest[line_start..i];
        let (head, tail) = rest.split_at(i + needle.len());
        out.push_str(head);
        if !tail.trim_start().starts_with("\"mirrors\"") {
            out.push_str(&format!(
                "{indent}\"mirrors\": [\n{indent}  {}\n{indent}],\n",
                serde_json::to_string(mirror)?
            ));
        }
        rest = tail;
    }
    out.push_str(rest);
    Ok(out)
}

/// CI check (offline): every pinned download (archives and direct model files) lists a mirror, the FFmpeg pin names its source
/// release, and `licenses/FFMPEG-NOTICE.md` names the pinned build and that release.
fn check_pins() -> Result<()> {
    let mut problems = Vec::new();
    for (url, _, mirrors) in all_pins()? {
        if mirrors.is_empty() {
            problems.push(format!("no mirror for {url} (cargo xtask mirror <tag>)"));
        }
    }
    let lock = read_lock()?;
    if !lock.scripts_commit.as_deref().is_some_and(is_commit) {
        problems.push(format!("no scripts_commit (40 hex) for {} in tools.lock.json", lock.release));
    }
    let notice = std::fs::read_to_string(root().join("licenses/FFMPEG-NOTICE.md"))?;
    match &lock.source {
        None => problems.push(format!(
            "no source release for FFmpeg {} (gh workflow run ffmpeg-source.yml, then cargo xtask mirror <tag>)",
            lock.version
        )),
        Some(s) if !notice.contains(s.as_str()) => {
            problems.push(format!("licenses/FFMPEG-NOTICE.md does not link the source release {s}"))
        }
        Some(_) => {}
    }
    for v in [&lock.version, &lock.release] {
        if !notice.contains(v.as_str()) {
            problems.push(format!("licenses/FFMPEG-NOTICE.md does not name {v}"));
        }
    }
    if problems.is_empty() {
        println!(
            "pins: every pinned download mirrored; FFmpeg {} source release and notice in place",
            lock.version
        );
        Ok(())
    } else {
        bail!("pin check failed:\n  {}", problems.join("\n  "))
    }
}

/// Regenerates the desktop app's TypeScript types from rmedia-core (ts-rs). CI fails if the
/// committed bindings differ from the generated ones.
fn gen_bindings() -> Result<()> {
    let out = root().join("apps/desktop/src/lib/bindings");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out)?;
    run(Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["test", "-q", "-p", "rmedia-core", "--features", "ts", "--lib", "export_bindings"])
        .env("TS_RS_EXPORT_DIR", &out)
        .env("TS_RS_LARGE_INT", "number")
        .current_dir(root()))?;
    println!("bindings written to {}", out.display());
    Ok(())
}

/// Builds the release CLI and the AI worker and copies them to `binaries/rmedia-<triple>[.exe]`
/// and `binaries/rmedia-sr-<triple>[.exe]`, so installers ship them next to the bundled engine
/// (Tauri sidecar naming). The worker needs `cargo xtask fetch-ncnn --target <triple>` first.
fn stage_cli(target: &str) -> Result<()> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let host = target == host_triple();
    let mut cmd = Command::new(&cargo);
    cmd.args(["build", "--release", "-p", "rmedia-cli", "-p", "rmedia-sr"]).current_dir(root());
    if !host {
        cmd.args(["--target", target]);
    }
    run(&mut cmd)?;
    let exe = if target.contains("windows") { ".exe" } else { "" };
    std::fs::create_dir_all(root().join("binaries"))?;
    for name in ["rmedia", "rmedia-sr"] {
        let built = if host {
            root().join(format!("target/release/{name}{exe}"))
        } else {
            root().join(format!("target/{target}/release/{name}{exe}"))
        };
        let dest = root().join(format!("binaries/{name}-{target}{exe}"));
        std::fs::copy(&built, &dest).with_context(|| format!("copy {}", built.display()))?;
        println!("staged {}", dest.display());
    }
    Ok(())
}

/// Writes THIRD_PARTY_NOTICES.md: every Rust crate compiled into rmedia (CLI + desktop) and
/// every npm package bundled into the desktop UI, with their license texts. FFmpeg is covered
/// separately in licenses/FFMPEG-NOTICE.md (it ships as a separate program).
fn notices() -> Result<()> {
    use std::collections::BTreeMap;
    #[derive(Default)]
    struct Entry {
        license: String,
        texts: Vec<String>,
    }
    let mut crates: BTreeMap<String, Entry> = BTreeMap::new();
    // Union over every release platform, so the file is identical wherever it is generated and
    // the Windows installers list their Windows-only crates too.
    let platforms = ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"];
    for (manifest, platform) in ["Cargo.toml", "apps/desktop/src-tauri/Cargo.toml"]
        .iter()
        .flat_map(|m| platforms.iter().map(move |p| (*m, *p)))
    {
        let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args([
                "metadata",
                "--format-version",
                "1",
                "--manifest-path",
                manifest,
                "--filter-platform",
                platform,
            ])
            .current_dir(root())
            .output()?;
        if !out.status.success() {
            bail!("cargo metadata failed for {manifest}");
        }
        let meta: serde_json::Value = serde_json::from_slice(&out.stdout)?;
        // Only packages actually reachable through normal (non-dev, non-build) dependencies.
        let resolve = &meta["resolve"]["nodes"];
        let members: Vec<String> = meta["workspace_members"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m.as_str().map(String::from))
            .collect();
        let mut keep = std::collections::HashSet::new();
        let mut stack: Vec<String> = members.clone();
        while let Some(id) = stack.pop() {
            if !keep.insert(id.clone()) {
                continue;
            }
            if let Some(node) = resolve.as_array().into_iter().flatten().find(|n| n["id"] == id.as_str()) {
                for dep in node["deps"].as_array().into_iter().flatten() {
                    let normal =
                        dep["dep_kinds"].as_array().into_iter().flatten().any(|k| k["kind"].is_null());
                    if normal && let Some(d) = dep["pkg"].as_str() {
                        stack.push(d.to_owned());
                    }
                }
            }
        }
        for p in meta["packages"].as_array().into_iter().flatten() {
            let id = p["id"].as_str().unwrap_or_default();
            if !keep.contains(id) || members.iter().any(|m| m == id) || p["source"].is_null() {
                continue;
            }
            let key =
                format!("{} {}", p["name"].as_str().unwrap_or("?"), p["version"].as_str().unwrap_or("?"));
            let dir = Path::new(p["manifest_path"].as_str().unwrap_or(""))
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            let entry = crates.entry(key).or_default();
            entry.license = p["license"].as_str().unwrap_or("see license file").to_owned();
            entry.texts = license_texts(&dir);
        }
    }
    let mut npm: BTreeMap<String, Entry> = BTreeMap::new();
    let nm = root().join("apps/desktop/node_modules");
    // The UI bundle contains @tauri-apps/api and the Svelte runtime (and their dependencies).
    let mut stack = vec!["@tauri-apps/api".to_owned(), "svelte".to_owned()];
    let mut seen = std::collections::HashSet::new();
    while let Some(name) = stack.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let dir = nm.join(&name);
        let Ok(text) = std::fs::read_to_string(dir.join("package.json")) else { continue };
        let pkg: serde_json::Value = serde_json::from_str(&text)?;
        for dep in pkg["dependencies"].as_object().into_iter().flatten().map(|(k, _)| k.clone()) {
            stack.push(dep);
        }
        let key = format!("{} {}", name, pkg["version"].as_str().unwrap_or("?"));
        npm.insert(
            key,
            Entry {
                license: pkg["license"].as_str().unwrap_or("see license file").to_owned(),
                texts: license_texts(&dir),
            },
        );
    }
    let mut out = String::from(
        "# Third-party notices\n\nrmedia includes the following third-party software. FFmpeg ships as a \
         separate program; see `licenses/FFMPEG-NOTICE.md`. The AI worker `rmedia-sr` links ncnn and \
         glslang statically; see `licenses/NCNN-NOTICE.md`.\n\nGenerated by `cargo xtask notices`.\n",
    );
    // Packages that ship no license file get the canonical text of their SPDX license(s) in an
    // appendix, taken from another package that ships exactly that license.
    let mut needed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (title, list) in [("Rust crates", &crates), ("JavaScript packages (desktop UI)", &npm)] {
        out.push_str(&format!("\n## {title}\n\n"));
        for (name, e) in list.iter() {
            out.push_str(&format!("### {name}\n\nLicense: {}\n\n", e.license));
            if e.texts.is_empty() {
                let ids: Vec<String> = e
                    .license
                    .split([' ', '(', ')', '/'])
                    .filter(|t| !t.is_empty() && !matches!(*t, "OR" | "AND" | "WITH"))
                    .map(String::from)
                    .collect();
                out.push_str(&format!(
                    "No license file is shipped; see the appendix for {}.\n\n",
                    ids.join(", ")
                ));
                needed.extend(ids);
            }
            for t in &e.texts {
                out.push_str("```text\n");
                out.push_str(t.trim());
                out.push_str("\n```\n\n");
            }
        }
    }
    if !needed.is_empty() {
        out.push_str("\n## Appendix: license texts\n\nCanonical texts; copyright belongs to each package's authors.\n\n");
        for id in needed {
            let text = crates
                .values()
                .chain(npm.values())
                .find(|e| e.license == id && e.texts.len() == 1)
                .map(|e| e.texts[0].clone());
            match text {
                Some(t) => out.push_str(&format!("### {id}\n\n```text\n{}\n```\n\n", t.trim())),
                None => out.push_str(&format!("### {id}\n\nSee https://spdx.org/licenses/{id}.html\n\n")),
            }
        }
    }
    // Model weights are distributed with the app, so their licences are listed too.
    let models = read_model_lock()?;
    if !models.models.is_empty() {
        out.push_str("\n## Model files\n\n");
        for (name, pin) in models.models.iter().filter(|(_, p)| !p.test_only) {
            let text = std::fs::read_to_string(root().join(&pin.license))
                .with_context(|| format!("licence file {} for {name}", pin.license))?;
            out.push_str(&format!("### {name}\n\n{}\n\n```text\n{}\n```\n\n", pin.description, text.trim()));
        }
    }
    let path = root().join("THIRD_PARTY_NOTICES.md");
    std::fs::write(&path, out)?;
    println!("wrote {} ({} crates, {} npm packages)", path.display(), crates.len(), npm.len());
    Ok(())
}

/// LICENSE*/COPYING*/NOTICE* files next to a package manifest.
fn license_texts(dir: &Path) -> Vec<String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().to_ascii_uppercase()).unwrap_or_default();
            p.is_file()
                && (n.starts_with("LICENSE")
                    || n.starts_with("LICENCE")
                    || n.starts_with("COPYING")
                    || n.starts_with("NOTICE"))
        })
        .collect();
    files.sort();
    files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).collect()
}

fn file_sha256(path: &Path) -> Result<String> {
    Ok(Sha256::digest(std::fs::read(path)?).iter().map(|b| format!("{b:02x}")).collect())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_text_is_added_once_per_pin_in_the_pretty_layout() {
        let pin = |m: &str| {
            format!(
                "{{\n  \"a\": {{\n    \"archive\": {{\n      \"url\": \"https://x/f.zip\",\n{m}      \"sha256\": \"00\"\n    }}\n  }}\n}}\n"
            )
        };
        let mirrored = "      \"mirrors\": [\n        \"https://m/f.zip\"\n      ],\n";
        let added = with_mirror_text(&pin(""), "https://x/f.zip", "https://m/f.zip").unwrap();
        assert_eq!(added, pin(mirrored));
        let v: serde_json::Value = serde_json::from_str(&added).unwrap();
        assert_eq!(v["a"]["archive"]["mirrors"][0], "https://m/f.zip");
        // Already mirrored: unchanged. Another URL: unchanged.
        assert_eq!(with_mirror_text(&added, "https://x/f.zip", "https://m2/f.zip").unwrap(), added);
        assert_eq!(with_mirror_text(&pin(""), "https://x/g.zip", "https://m/g.zip").unwrap(), pin(""));
    }

    /// Stripping the mirrors from the committed models lock and adding them back gives the same
    /// file, so `cargo xtask mirror` writes exactly the reviewed layout.
    #[test]
    fn mirror_text_round_trips_the_models_lock() {
        let real = std::fs::read_to_string(root().join("xtask/models.lock.json")).unwrap();
        let lines: Vec<&str> = real.lines().collect();
        let (mut stripped, mut mirrors) = (String::new(), Vec::new());
        let mut i = 0;
        while i < lines.len() {
            if lines[i].trim_start().starts_with("\"mirrors\": [") {
                assert!(lines[i + 2].trim() == "],", "one mirror per pin");
                mirrors.push(lines[i + 1].trim().trim_matches('"').to_owned());
                i += 3;
                continue;
            }
            stripped.push_str(lines[i]);
            stripped.push('\n');
            i += 1;
        }
        let pins: ModelLock = serde_json::from_str(&real).unwrap();
        let mut rebuilt = stripped;
        for p in pins.ncnn.values() {
            rebuilt = with_mirror_text(&rebuilt, &p.url, &p.mirrors[0]).unwrap();
        }
        for m in pins.models.values() {
            match (&m.url, &m.archive) {
                (Some(url), _) => rebuilt = with_mirror_text(&rebuilt, url, &m.mirrors[0]).unwrap(),
                (None, Some(a)) => rebuilt = with_mirror_text(&rebuilt, &a.url, &a.mirrors[0]).unwrap(),
                (None, None) => {}
            }
        }
        assert!(!mirrors.is_empty());
        assert_eq!(rebuilt, real);
    }
}
