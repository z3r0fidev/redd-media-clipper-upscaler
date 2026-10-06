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
        Some("gen-bindings") => gen_bindings(),
        Some("stage-cli") => {
            let target = match args.iter().position(|a| a == "--target") {
                Some(i) => args.get(i + 1).context("--target needs a value")?.clone(),
                None => host_triple().to_owned(),
            };
            stage_cli(&target)
        }
        Some("notices") => notices(),
        _ => {
            eprintln!(
                "usage: cargo xtask <fetch-tools [--target TRIPLE] | pin-tools | gen-bindings | stage-cli [--target TRIPLE] | notices>"
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

/// Builds the release CLI and copies it to `binaries/rmedia-<triple>[.exe]`, so installers ship it
/// next to the bundled engine (Tauri sidecar naming).
fn stage_cli(target: &str) -> Result<()> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let host = target == host_triple();
    let mut cmd = Command::new(&cargo);
    cmd.args(["build", "--release", "-p", "rmedia-cli"]).current_dir(root());
    if !host {
        cmd.args(["--target", target]);
    }
    run(&mut cmd)?;
    let exe = if target.contains("windows") { ".exe" } else { "" };
    let built = if host {
        root().join(format!("target/release/rmedia{exe}"))
    } else {
        root().join(format!("target/{target}/release/rmedia{exe}"))
    };
    let dest = root().join(format!("binaries/rmedia-{target}{exe}"));
    std::fs::create_dir_all(root().join("binaries"))?;
    std::fs::copy(&built, &dest).with_context(|| format!("copy {}", built.display()))?;
    println!("staged {}", dest.display());
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
    for manifest in ["Cargo.toml", "apps/desktop/src-tauri/Cargo.toml"] {
        let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args(["metadata", "--format-version", "1", "--manifest-path", manifest, "--filter-platform"])
            .arg(host_triple())
            .current_dir(root())
            .output()?;
        if !out.status.success() {
            bail!("cargo metadata failed for {manifest}");
        }
        let meta: serde_json::Value = serde_json::from_slice(&out.stdout)?;
        // Only packages actually reachable through normal (non-dev, non-build) dependencies.
        let resolve = &meta["resolve"]["nodes"];
        let members: Vec<String> =
            meta["workspace_members"].as_array().into_iter().flatten().filter_map(|m| m.as_str().map(String::from)).collect();
        let mut keep = std::collections::HashSet::new();
        let mut stack: Vec<String> = members.clone();
        while let Some(id) = stack.pop() {
            if !keep.insert(id.clone()) {
                continue;
            }
            if let Some(node) = resolve.as_array().into_iter().flatten().find(|n| n["id"] == id.as_str()) {
                for dep in node["deps"].as_array().into_iter().flatten() {
                    let normal = dep["dep_kinds"].as_array().into_iter().flatten().any(|k| k["kind"].is_null());
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
            let key = format!("{} {}", p["name"].as_str().unwrap_or("?"), p["version"].as_str().unwrap_or("?"));
            let dir = Path::new(p["manifest_path"].as_str().unwrap_or("")).parent().map(Path::to_path_buf).unwrap_or_default();
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
            Entry { license: pkg["license"].as_str().unwrap_or("see license file").to_owned(), texts: license_texts(&dir) },
        );
    }
    let mut out = String::from(
        "# Third-party notices\n\nrmedia includes the following third-party software. FFmpeg ships as a \
         separate program; see `licenses/FFMPEG-NOTICE.md`.\n\nGenerated by `cargo xtask notices`.\n",
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
                out.push_str(&format!("No license file is shipped; see the appendix for {}.\n\n", ids.join(", ")));
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
            p.is_file() && (n.starts_with("LICENSE") || n.starts_with("LICENCE") || n.starts_with("COPYING") || n.starts_with("NOTICE"))
        })
        .collect();
    files.sort();
    files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).collect()
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
