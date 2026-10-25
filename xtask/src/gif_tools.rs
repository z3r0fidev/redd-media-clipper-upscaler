//! The bundled GIF engines gifsicle and gifski (feature 003, contracts/gif-tools-lock.md):
//! `xtask/gif-tools.lock.json`, its `check-pins` rules, staging into `binaries/`,
//! `cargo xtask pin-gif-tools <run-id>` and `cargo xtask check-release-gate`.
//!
//! Both engines are built from pinned sources by `.github/workflows/gif-tools.yml`, which
//! publishes them to a `gif-tools-<date>` release of this repository. Until the first pin there
//! is no lock file: every command here then skips the GIF engines.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{Pinned, file_sha256, gh, is_commit, pinned_archive, root};

/// The triples both engines are built for.
pub const TRIPLES: [&str; 2] = ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"];
/// The workflow that builds them.
pub const WORKFLOW: &str = ".github/workflows/gif-tools.yml";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GifToolsLock {
    pub comment: String,
    /// The release of this repository holding the built binaries (`gif-tools-YYYY-MM-DD`).
    pub release: String,
    /// The `gif-tools.yml` run that built them, and its commit on `main`.
    pub build_run: u64,
    pub build_commit: String,
    pub gifsicle: GifTool,
    pub gifski: GifTool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GifTool {
    pub version: String,
    pub license: String,
    /// The upstream source archive (gifsicle's tarball, gifski's `.crate`) and its mirrors.
    pub source: Pinned,
    /// gifski: sha256 of the `Cargo.lock` shipped in the crate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cargo_lock_sha256: Option<String>,
    /// gifski: packages the build moved off the crate's `Cargo.lock` (security updates), as
    /// `name old -> new (advisory)`. The lock the build used ships in the vendor tarball.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lock_updates: Vec<String>,
    /// gifski: the vendored dependency sources the build used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<Pinned>,
    pub targets: BTreeMap<String, Built>,
}

/// A binary built by the workflow, on this repository's release.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Built {
    pub url: String,
    pub sha256: String,
}

fn lock_path() -> PathBuf {
    root().join("xtask/gif-tools.lock.json")
}

/// The lock, or `None` before the first pin.
pub fn read_lock() -> Result<Option<GifToolsLock>> {
    let path = lock_path();
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)?;
    serde_json::from_str(&text).map(Some).with_context(|| format!("parse {}", path.display()))
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `gif-tools-YYYY-MM-DD`.
fn is_release_tag(s: &str) -> bool {
    let Some(date) = s.strip_prefix("gif-tools-") else { return false };
    let b = date.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

/// The third-party archives of the lock (sources and the vendor tarball) as (url, sha256,
/// mirrors), for `all_pins`: `archive`, `mirror` and `check-pins` treat them like FFmpeg's.
pub fn archive_pins(lock: &GifToolsLock) -> Vec<(String, String, Vec<String>)> {
    [&lock.gifsicle, &lock.gifski]
        .into_iter()
        .flat_map(|t| std::iter::once(&t.source).chain(t.vendor.as_ref()))
        .map(|p| (p.url.clone(), p.sha256.clone(), p.mirrors.clone()))
        .collect()
}

/// Every URL of the lock, built binaries included (`check-pins --online`).
pub fn all_urls(lock: &GifToolsLock) -> Vec<String> {
    let mut urls: Vec<String> =
        archive_pins(lock).into_iter().flat_map(|(u, _, m)| std::iter::once(u).chain(m)).collect();
    for t in [&lock.gifsicle, &lock.gifski] {
        urls.extend(t.targets.values().map(|b| b.url.clone()));
    }
    urls
}

/// The lock's rules (contracts/gif-tools-lock.md › Rules). `repo` is `owner/name`.
pub fn check_lock(lock: &GifToolsLock, repo: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if !is_release_tag(&lock.release) {
        problems.push(format!("release {:?} must look like gif-tools-YYYY-MM-DD", lock.release));
    }
    if lock.build_run == 0 {
        problems.push("build_run must be the gif-tools.yml run id".into());
    }
    if !is_commit(&lock.build_commit) {
        problems.push(format!("build_commit {:?} must be a 40-hex commit", lock.build_commit));
    }
    let downloads = format!("https://github.com/{repo}/releases/download/{}/", lock.release);
    for (name, t) in [("gifsicle", &lock.gifsicle), ("gifski", &lock.gifski)] {
        let mut pins: Vec<&Pinned> = vec![&t.source];
        pins.extend(t.vendor.as_ref());
        for p in pins {
            if !is_sha256(&p.sha256) {
                problems.push(format!("{name}: bad sha256 {:?} for {}", p.sha256, p.url));
            }
            if let Some(bad) = std::iter::once(&p.url).chain(&p.mirrors).find(|u| !u.starts_with("https://"))
            {
                problems.push(format!("{name}: {bad:?} is not https"));
            }
            if p.mirrors.is_empty() {
                problems.push(format!("{name}: no mirror for {}", p.url));
            }
        }
        for triple in TRIPLES {
            match t.targets.get(triple) {
                None => problems.push(format!("{name}: no build for {triple}")),
                Some(b) => {
                    if !is_sha256(&b.sha256) {
                        problems.push(format!("{name}: bad sha256 {:?} for {triple}", b.sha256));
                    }
                    if !b.url.starts_with(&downloads) {
                        problems.push(format!(
                            "{name}: the {triple} build must be on {downloads}, not {}",
                            b.url
                        ));
                    }
                }
            }
        }
    }
    if lock.gifski.vendor.is_none() {
        problems.push("gifski: no vendor archive".into());
    }
    if !lock.gifski.cargo_lock_sha256.as_deref().is_some_and(is_sha256) {
        problems.push("gifski: no cargo_lock_sha256".into());
    }
    problems
}

/// The notice rules: each notice names the pinned version and every mirrored source URL, and
/// gifski's dependency list exists.
pub fn check_notices(lock: &GifToolsLock, licenses: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for (file, t) in [("GIFSICLE-NOTICE.md", &lock.gifsicle), ("GIFSKI-NOTICE.md", &lock.gifski)] {
        let Ok(text) = std::fs::read_to_string(licenses.join(file)) else {
            problems.push(format!("licenses/{file} is missing"));
            continue;
        };
        if !text.contains(&t.version) {
            problems.push(format!("licenses/{file} does not name version {}", t.version));
        }
        let mirrors = t.source.mirrors.iter().chain(t.vendor.iter().flat_map(|v| &v.mirrors));
        for m in mirrors {
            if !text.contains(m.as_str()) {
                problems.push(format!("licenses/{file} does not link the mirrored source {m}"));
            }
        }
    }
    for file in ["GIFSKI-DEPS.md", "GIFSICLE-COPYING.txt", "GIFSKI-LICENSE.txt"] {
        if !licenses.join(file).is_file() {
            problems.push(format!("licenses/{file} is missing"));
        }
    }
    problems
}

/// `check-pins` for the GIF engines: the lock rules and the notices. Nothing before the first
/// pin.
pub fn check_pins(repo: &str) -> Result<Vec<String>> {
    let Some(lock) = read_lock()? else { return Ok(Vec::new()) };
    let mut problems = check_lock(&lock, repo);
    problems.extend(check_notices(&lock, &root().join("licenses")));
    Ok(problems)
}

/// Stages `binaries/rmedia-{gifsicle,gifski}-<target>[.exe]` from the pinned builds (verified,
/// cached under `archives/<sha256>/`, stamped). `with_gifski: false` stages only gifsicle and
/// removes a staged gifski (interim release artifacts, research R10).
pub fn stage(target: &str, with_gifski: bool) -> Result<()> {
    let bin_dir = root().join("binaries");
    let exe = if target.contains("windows") { ".exe" } else { "" };
    let gifski_dest = bin_dir.join(format!("rmedia-gifski-{target}{exe}"));
    if !with_gifski {
        let _ = std::fs::remove_file(&gifski_dest);
        let _ = std::fs::remove_file(bin_dir.join(format!(".gifski-{target}.sha256")));
    }
    let Some(lock) = read_lock()? else {
        println!("no xtask/gif-tools.lock.json yet: GIF engines not staged (built-in GIF engine only)");
        return Ok(());
    };
    std::fs::create_dir_all(&bin_dir)?;
    let tools: Vec<(&str, &GifTool)> = if with_gifski {
        vec![("gifsicle", &lock.gifsicle), ("gifski", &lock.gifski)]
    } else {
        vec![("gifsicle", &lock.gifsicle)]
    };
    for (name, t) in tools {
        let built = t.targets.get(target).with_context(|| format!("no pinned {name} build for {target}"))?;
        let dest = bin_dir.join(format!("rmedia-{name}-{target}{exe}"));
        let stamp = bin_dir.join(format!(".{name}-{target}.sha256"));
        // Re-verified on every reuse, so a tampered binary in binaries/ is replaced.
        if std::fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == built.sha256)
            && file_sha256(&dest).is_ok_and(|h| h == built.sha256)
        {
            println!("{name} {} for {target} already present (verified)", t.version);
            continue;
        }
        let kept = pinned_archive(&built.url, &[], &built.sha256)?;
        let tmp = bin_dir.join(format!(".rmedia-{name}-{target}{exe}.tmp"));
        let _ = std::fs::remove_file(&tmp);
        std::fs::copy(&kept, &tmp)?;
        if file_sha256(&tmp)? != built.sha256 {
            let _ = std::fs::remove_file(&tmp);
            bail!("the staged copy of {name} does not match its pin");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(&tmp, &dest)?;
        std::fs::write(&stamp, format!("{}\n", built.sha256))?;
        println!("installed {} ({name} {})", dest.display(), t.version);
    }
    Ok(())
}

// --- the release gate (FR-025, research R10) ------------------------------------------------

/// Application licences under which gifski (AGPL-3.0-or-later) may ship inside rmedia, or the
/// marker for a bought gifski licence. The owner edits this list only together with the
/// licence decision.
/// `commercial-gifski-imagequant`: bought licences for both gifski and imagequant (GPL-3.0-or-later,
/// compiled into gifski); a gifski licence alone does not cover imagequant.
pub const GATE_ALLOWLIST: [&str; 5] = [
    "AGPL-3.0-only",
    "AGPL-3.0-or-later",
    "GPL-3.0-only",
    "GPL-3.0-or-later",
    "commercial-gifski-imagequant",
];

/// Why a release may not ship from `tree` (`None`: it may). It may when no gifski is staged in
/// `tree/binaries/`, or when `tree` has a root `LICENSE` and `[workspace.metadata.rmedia]
/// app_licence` in its `Cargo.toml` names an allowlisted id.
pub fn release_gate(tree: &Path) -> Result<Option<String>> {
    let staged = std::fs::read_dir(tree.join("binaries"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("rmedia-gifski"));
    if !staged {
        return Ok(None);
    }
    let manifest = std::fs::read_to_string(tree.join("Cargo.toml")).context("read the root Cargo.toml")?;
    let licence = app_licence(&manifest);
    let has_license_file = tree.join("LICENSE").is_file();
    match (licence, has_license_file) {
        (Some(l), true) if GATE_ALLOWLIST.contains(&l.as_str()) => Ok(None),
        (Some(l), true) => Ok(Some(format!(
            "gifski (AGPL-3.0) is staged but app_licence {l:?} is not one it may ship under ({})",
            GATE_ALLOWLIST.join(", ")
        ))),
        _ => Ok(Some(
            "gifski (AGPL-3.0) is staged but the application licence is undecided (docs/roadmap.md): \
             add a root LICENSE and [workspace.metadata.rmedia] app_licence, or stage without gifski \
             (cargo xtask fetch-tools --no-gifski)"
                .into(),
        )),
    }
}

/// `app_licence = "…"` in the `[workspace.metadata.rmedia]` table of a Cargo.toml (a minimal
/// reader: the table header, then `key = "value"` lines until the next table).
fn app_licence(manifest: &str) -> Option<String> {
    let mut in_table = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_table = line == "[workspace.metadata.rmedia]";
            continue;
        }
        if in_table
            && let Some(value) = line.strip_prefix("app_licence").map(str::trim_start)
            && let Some(value) = value.strip_prefix('=')
        {
            let value = value.split('#').next().unwrap_or_default().trim();
            return value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).map(str::to_owned);
        }
    }
    None
}

pub fn check_release_gate(tree: Option<&Path>) -> Result<()> {
    let tree = tree.map(Path::to_path_buf).unwrap_or_else(root);
    match release_gate(&tree)? {
        None => {
            println!("release gate: ok");
            Ok(())
        }
        Some(reason) => bail!("release gate: {reason}"),
    }
}

// --- pin-gif-tools <run-id> ----------------------------------------------------------------

/// The facts of a workflow run that matter for pinning (from `GET /repos/{repo}/actions/runs/{id}`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RunInfo {
    pub id: u64,
    pub path: String,
    pub conclusion: Option<String>,
    pub head_sha: String,
    pub head_branch: String,
    pub event: String,
}

/// Checks a run before anything is downloaded: the right workflow, finished successfully, on
/// `main`, from a manual dispatch.
pub fn check_run(run: &RunInfo, id: u64) -> Result<()> {
    if run.id != id {
        bail!("asked for run {id}, got {}", run.id);
    }
    if run.path.split('@').next() != Some(WORKFLOW) {
        bail!("run {id} is {}, not {WORKFLOW}", run.path);
    }
    if run.conclusion.as_deref() != Some("success") {
        bail!("run {id} did not succeed ({:?})", run.conclusion);
    }
    if run.head_branch != "main" || run.event != "workflow_dispatch" {
        bail!("run {id} ran on {} ({}), not a manual run on main", run.head_branch, run.event);
    }
    if !is_commit(&run.head_sha) {
        bail!("run {id} has no commit id");
    }
    Ok(())
}

/// The `cksum` of `version` in a crates.io sparse-index file (one JSON object per line).
pub fn index_checksum(index: &str, version: &str) -> Option<String> {
    index.lines().find_map(|l| {
        let v: serde_json::Value = serde_json::from_str(l).ok()?;
        (v["vers"] == version).then(|| v["cksum"].as_str().map(str::to_owned)).flatten()
    })
}

/// `SHA256SUMS` lines (`<hex>  <name>`) as name → hash; the publish job writes the file from the
/// build job's outputs.
pub fn parse_sums(text: &str) -> Result<BTreeMap<String, String>> {
    let mut sums = BTreeMap::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let (hash, name) = line.split_once("  ").with_context(|| format!("bad SHA256SUMS line {line:?}"))?;
        if !is_sha256(hash) || name.contains(['/', '\\']) || name.starts_with('.') {
            bail!("bad SHA256SUMS line {line:?}");
        }
        sums.insert(name.to_owned(), hash.to_owned());
    }
    Ok(sums)
}

/// The asset names of a build, as the workflow publishes them.
pub fn asset_names(gifsicle: &str, gifski: &str) -> Vec<(String, &'static str, &'static str)> {
    let mut v = Vec::new();
    for triple in TRIPLES {
        let exe = if triple.contains("windows") { ".exe" } else { "" };
        v.push((format!("gifsicle-{gifsicle}-{triple}{exe}"), "gifsicle", triple));
        v.push((format!("gifski-{gifski}-{triple}{exe}"), "gifski", triple));
    }
    v
}

/// The versions and source pins the workflow builds (kept in step with gif-tools.yml).
const GIFSICLE_VERSION: &str = "1.98";
const GIFSICLE_SOURCE: &str = "https://www.lcdf.org/gifsicle/gifsicle-1.98.tar.gz";
const GIFSICLE_SHA256: &str = "e9d4cc0816db95c64d32e79f778836db0f1f7eb15868490bd330104e5146b66e";
const GIFSKI_VERSION: &str = "1.34.0";
const GIFSKI_SOURCE: &str = "https://static.crates.io/crates/gifski/gifski-1.34.0.crate";
const GIFSKI_SHA256: &str = "c246c795a61d4a2476fb1c8ab70bedfaa825c734882adc40e117fc837df81190";
/// Security updates applied to gifski's `Cargo.lock` before vendoring (name, from, to, advisory);
/// the same list as `GIFSKI_LOCK_UPDATES` in gif-tools.yml, checked against the shipped lock.
const GIFSKI_LOCK_UPDATES: &[(&str, &str, &str, &str)] =
    &[("crossbeam-epoch", "0.9.18", "0.9.20", "RUSTSEC-2026-0204")];

fn curl_text(url: &str) -> Result<Option<String>> {
    let out = Command::new("curl")
        .args(["-fsSL", "--proto", "=https", "--proto-redir", "=https", "--max-time", "120", "--url", url])
        .output()?;
    Ok(out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned()))
}

/// `cargo xtask pin-gif-tools <run-id>`: checks the run and every asset, then writes the lock.
pub fn pin(run_id: u64) -> Result<()> {
    let repo = gh(&["repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"])?;
    let run: RunInfo = serde_json::from_str(&gh(&["api", &format!("repos/{repo}/actions/runs/{run_id}")])?)?;
    check_run(&run, run_id)?;
    let on_main = Command::new("git")
        .args(["merge-base", "--is-ancestor", &run.head_sha, "origin/main"])
        .status()
        .is_ok_and(|s| s.success());
    if !on_main {
        bail!("commit {} of run {run_id} is not on origin/main (git fetch first?)", run.head_sha);
    }
    // The release the run published: its notes name the run on a line of their own, and its tag
    // is at the run's commit. (Notes and tags are writable by anyone who can write releases;
    // the attestations below are what authenticates.)
    let tags =
        gh(&["release", "list", "-R", &repo, "--limit", "50", "--json", "tagName", "-q", ".[].tagName"])?;
    let release = tags
        .lines()
        .filter(|t| is_release_tag(t))
        .find(|t| {
            gh(&["release", "view", t, "-R", &repo, "--json", "body", "-q", ".body"])
                .is_ok_and(|b| names_run(&b, run_id))
        })
        .with_context(|| format!("no gif-tools-<date> release names build_run: {run_id}"))?
        .to_owned();
    let target = gh(&[
        "release",
        "view",
        &release,
        "-R",
        &repo,
        "--json",
        "targetCommitish",
        "-q",
        ".targetCommitish",
    ])?;
    if target != run.head_sha {
        bail!("{release} targets {target}, not run {run_id}'s commit {}", run.head_sha);
    }

    // Downloads are untrusted until verified: an empty folder of their own, outside the
    // repository (so no parent .cargo/config or rust-toolchain applies to the crate).
    let work = std::env::temp_dir().join(format!("rmedia-pin-gif-tools-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    gh(&["release", "download", &release, "-R", &repo, "--dir", &work.to_string_lossy()])?;
    let sums = parse_sums(
        &std::fs::read_to_string(work.join("SHA256SUMS")).context("the release has no SHA256SUMS")?,
    )?;
    let digests = gh(&[
        "release",
        "view",
        &release,
        "-R",
        &repo,
        "--json",
        "assets",
        "-q",
        r#".assets[] | "\(.name) \(.digest)""#,
    ])?;
    let digest_of = |name: &str| {
        digests.lines().find_map(|l| l.strip_prefix(name)?.strip_prefix(" sha256:")).map(str::to_owned)
    };
    let download = |name: &str| format!("https://github.com/{repo}/releases/download/{release}/{name}");

    let source_file = |url: &str| url.rsplit('/').next().unwrap_or_default().to_owned();
    let vendor_name = format!("gifski-{GIFSKI_VERSION}-vendor.tar.gz");
    let attested: Vec<String> = asset_names(GIFSICLE_VERSION, GIFSKI_VERSION)
        .into_iter()
        .map(|(n, _, _)| n)
        .chain([source_file(GIFSICLE_SOURCE), source_file(GIFSKI_SOURCE), vendor_name.clone()])
        .collect();
    let mut verified = BTreeMap::new();
    for name in attested {
        let file = work.join(&name);
        let actual = file_sha256(&file).with_context(|| format!("{release} has no {name}"))?;
        let listed = sums.get(&name).with_context(|| format!("SHA256SUMS does not list {name}"))?;
        if &actual != listed || digest_of(&name).as_deref() != Some(actual.as_str()) {
            bail!("{name}: downloaded {actual}, SHA256SUMS {listed}, GitHub digest {:?}", digest_of(&name));
        }
        // Made by gif-tools.yml from main at the run's commit, on a GitHub-hosted runner.
        let args = attest_args(&file.to_string_lossy(), &repo, &run.head_sha);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        gh(&refs).with_context(|| {
            format!("{name}: no valid attestation from {WORKFLOW} on main at {}", run.head_sha)
        })?;
        verified.insert(name, actual);
    }

    // Sources: the pinned hashes, the crates.io index, and Debian's copy of gifsicle.
    for (url, sha) in [(GIFSICLE_SOURCE, GIFSICLE_SHA256), (GIFSKI_SOURCE, GIFSKI_SHA256)] {
        let name = source_file(url);
        if file_sha256(&work.join(&name)).ok().as_deref() != Some(sha) {
            bail!("{release}'s {name} is not the pinned source ({sha})");
        }
    }
    let index =
        curl_text("https://index.crates.io/gi/fs/gifski")?.context("fetch the crates.io index entry")?;
    if index_checksum(&index, GIFSKI_VERSION).as_deref() != Some(GIFSKI_SHA256) {
        bail!("the crates.io index checksum of gifski {GIFSKI_VERSION} is not the pinned {GIFSKI_SHA256}");
    }
    // Debian's orig tarball is a snapshot of the upstream repository, not the release tarball:
    // the hashes never match, so the trees are compared. Every file in both must be identical,
    // and the release may add only autotools output (which this check does not cover).
    let debian =
        format!("https://deb.debian.org/debian/pool/main/g/gifsicle/gifsicle_{GIFSICLE_VERSION}.orig.tar.gz");
    let debian_dir = work.join("debian");
    std::fs::create_dir_all(&debian_dir)?;
    let deb_file = debian_dir.join("orig.tar.gz");
    if crate::download(&debian, &deb_file).is_ok() {
        let (release_tree, debian_tree) = (work.join("gifsicle-release"), debian_dir.join("tree"));
        for (archive, dir) in
            [(&work.join(source_file(GIFSICLE_SOURCE)), &release_tree), (&deb_file, &debian_tree)]
        {
            std::fs::create_dir_all(dir)?;
            crate::run(Command::new(crate::tar()).arg("-xzf").arg(archive).arg("-C").arg(dir))?;
        }
        let cmp = compare_source_trees(&single_dir(&release_tree)?, &single_dir(&debian_tree)?)?;
        if !cmp.problems.is_empty() {
            bail!(
                "gifsicle {GIFSICLE_VERSION} differs from Debian's {debian}:\n  {}",
                cmp.problems.join("\n  ")
            );
        }
        println!(
            "gifsicle source matches Debian's {debian}: {} files identical; release-only autotools output \
             not covered: {}",
            cmp.same,
            cmp.release_only.join(", ")
        );
    } else {
        println!(
            "no Debian orig tarball for gifsicle {GIFSICLE_VERSION}: not compared (say so in the commit)"
        );
    }

    // gifski's Cargo.lock: the vendored tree's, checked with cargo deny (and cargo audit when
    // installed), then its dependency list for the notices.
    let vendor_sha = verified[&vendor_name].clone();
    let crate_dir = work.join("crate");
    std::fs::create_dir_all(&crate_dir)?;
    crate::run(
        Command::new(crate::tar())
            .arg("-xzf")
            .arg(work.join(source_file(GIFSKI_SOURCE)))
            .arg("-C")
            .arg(&crate_dir),
    )?;
    let manifest = crate_dir.join(format!("gifski-{GIFSKI_VERSION}/Cargo.toml"));
    let cargo_lock = manifest.with_file_name("Cargo.lock");
    let cargo_lock_sha256 = file_sha256(&cargo_lock).context("the gifski crate has no Cargo.lock")?;
    // Each vendored package must be the one Cargo.lock pins (its crates.io checksum).
    let vendor_dir = work.join("vendor");
    std::fs::create_dir_all(&vendor_dir)?;
    crate::run(
        Command::new(crate::tar()).arg("-xzf").arg(work.join(&vendor_name)).arg("-C").arg(&vendor_dir),
    )?;
    // The lock the build used ships in the vendor tarball: it may differ from the crate's only by
    // the listed updates. It then replaces the crate's for every check below.
    let shipped_lock = vendor_dir.join("Cargo.lock");
    let shipped = std::fs::read_to_string(&shipped_lock).context("the vendor tarball has no Cargo.lock")?;
    let changes =
        lock_changes(&lock_checksums(&std::fs::read_to_string(&cargo_lock)?), &lock_checksums(&shipped));
    let expected: Vec<String> =
        GIFSKI_LOCK_UPDATES.iter().map(|u| format!("{} {} -> {}", u.0, u.1, u.2)).collect();
    if changes != expected {
        bail!("the shipped Cargo.lock changes {changes:?}, not the listed updates {expected:?}");
    }
    std::fs::copy(&shipped_lock, &cargo_lock)?;
    let locked = lock_checksums(&shipped);
    let mismatched = vendor_mismatches(&vendor_dir.join("vendor"), &locked)?;
    if !mismatched.is_empty() {
        bail!("vendored packages that are not the locked ones:\n  {}", mismatched.join("\n  "));
    }
    crate::run(
        Command::new("cargo")
            .args(["deny", "--manifest-path"])
            .arg(&manifest)
            .args(["--locked", "--config"])
            .arg(root().join("deny-gifski.toml"))
            .args(["check", "licenses", "bans", "advisories", "sources"]),
    )?;
    match Command::new("cargo").args(["audit", "--file"]).arg(&cargo_lock).status() {
        Ok(s) if s.success() => println!("cargo audit: no advisories"),
        Ok(_) => bail!("cargo audit found advisories in gifski's Cargo.lock"),
        Err(_) => println!("cargo audit is not installed: advisories were checked by cargo deny only"),
    }
    std::fs::write(root().join("licenses/GIFSKI-DEPS.md"), dependency_notice(&manifest)?)?;

    let built = |tool: &str, version: &str| -> BTreeMap<String, Built> {
        TRIPLES
            .iter()
            .map(|triple| {
                let exe = if triple.contains("windows") { ".exe" } else { "" };
                let name = format!("{tool}-{version}-{triple}{exe}");
                (triple.to_string(), Built { url: download(&name), sha256: verified[&name].clone() })
            })
            .collect()
    };
    let lock = GifToolsLock {
        comment:
            "Pinned GIF engines bundled with rmedia, built from source by .github/workflows/gif-tools.yml. \
                  Refresh with `cargo xtask pin-gif-tools <run-id>`."
                .into(),
        release: release.clone(),
        build_run: run_id,
        build_commit: run.head_sha.clone(),
        gifsicle: GifTool {
            version: GIFSICLE_VERSION.into(),
            license: "GPL-2.0-only".into(),
            source: Pinned {
                url: GIFSICLE_SOURCE.into(),
                mirrors: vec![download(&source_file(GIFSICLE_SOURCE))],
                sha256: GIFSICLE_SHA256.into(),
            },
            cargo_lock_sha256: None,
            lock_updates: Vec::new(),
            vendor: None,
            targets: built("gifsicle", GIFSICLE_VERSION),
        },
        gifski: GifTool {
            version: GIFSKI_VERSION.into(),
            license: "AGPL-3.0-or-later".into(),
            source: Pinned {
                url: GIFSKI_SOURCE.into(),
                mirrors: vec![download(&source_file(GIFSKI_SOURCE))],
                sha256: GIFSKI_SHA256.into(),
            },
            cargo_lock_sha256: Some(cargo_lock_sha256),
            lock_updates: GIFSKI_LOCK_UPDATES
                .iter()
                .map(|u| format!("{} {} -> {} ({})", u.0, u.1, u.2, u.3))
                .collect(),
            vendor: Some(Pinned {
                url: download(&vendor_name),
                mirrors: vec![download(&vendor_name)],
                sha256: vendor_sha,
            }),
            targets: built("gifski", GIFSKI_VERSION),
        },
    };
    let problems = check_lock(&lock, &repo);
    if !problems.is_empty() {
        bail!("the new lock breaks its own rules:\n  {}", problems.join("\n  "));
    }
    std::fs::write(lock_path(), serde_json::to_string_pretty(&lock)? + "\n")?;
    for (file, t) in [("GIFSICLE-NOTICE.md", &lock.gifsicle), ("GIFSKI-NOTICE.md", &lock.gifski)] {
        let path = root().join("licenses").join(file);
        let urls: Vec<&String> =
            t.source.mirrors.iter().chain(t.vendor.iter().flat_map(|v| &v.mirrors)).collect();
        std::fs::write(&path, with_mirrors(&std::fs::read_to_string(&path)?, &urls))?;
    }
    let _ = std::fs::remove_dir_all(&work);
    println!("pinned gifsicle {GIFSICLE_VERSION} and gifski {GIFSKI_VERSION} from {release} (run {run_id})");
    println!(
        "notices updated with the mirrored sources; next: `cargo xtask check-pins --online`, then commit"
    );
    Ok(())
}

/// `gh attestation verify` for `file`: signed by gif-tools.yml, from `main` at `commit`, on a
/// GitHub-hosted runner.
fn attest_args(file: &str, repo: &str, commit: &str) -> Vec<String> {
    let signer = format!("{repo}/{WORKFLOW}");
    [
        "attestation",
        "verify",
        file,
        "--repo",
        repo,
        "--signer-workflow",
        &signer,
        "--source-ref",
        "refs/heads/main",
        "--source-digest",
        commit,
        "--signer-digest",
        commit,
        "--deny-self-hosted-runners",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Whether release notes name `run` on a line of their own (`build_run: 12` is not run 123).
fn names_run(body: &str, run: u64) -> bool {
    body.lines().any(|l| l.trim() == format!("build_run: {run}"))
}

/// How `after` moves packages off `before` (both from [`lock_checksums`]): `name old -> new` for
/// each name whose single version changed, and `+ name version` / `- name version` for anything
/// else, sorted. A changed checksum for the same version is reported as `~ name version`.
fn lock_changes(before: &BTreeMap<String, String>, after: &BTreeMap<String, String>) -> Vec<String> {
    let names = |m: &BTreeMap<String, String>, k: &str| -> Vec<String> {
        m.keys().filter(|p| p.split(' ').next() == Some(k)).cloned().collect()
    };
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for key in before.keys().chain(after.keys()) {
        let name = key.split(' ').next().unwrap_or(key).to_owned();
        if !seen.insert(name.clone()) {
            continue;
        }
        let (b, a) = (names(before, &name), names(after, &name));
        let removed: Vec<&String> = b.iter().filter(|k| !after.contains_key(*k)).collect();
        let added: Vec<&String> = a.iter().filter(|k| !before.contains_key(*k)).collect();
        if let ([r], [n]) = (removed.as_slice(), added.as_slice()) {
            let v = |k: &str| k.split(' ').nth(1).unwrap_or_default().to_owned();
            out.push(format!("{name} {} -> {}", v(r), v(n)));
        } else {
            out.extend(removed.iter().map(|k| format!("- {k}")));
            out.extend(added.iter().map(|k| format!("+ {k}")));
        }
        for k in b.iter().filter(|k| after.get(*k).is_some_and(|c| Some(c) != before.get(*k))) {
            out.push(format!("~ {k}"));
        }
    }
    out.sort();
    out
}

/// `name version → checksum` of every registry package in a Cargo.lock.
fn lock_checksums(lock: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let (mut name, mut version) = (None::<String>, None::<String>);
    let value = |l: &str, key: &str| {
        l.strip_prefix(key)?
            .trim_start()
            .strip_prefix('=')?
            .trim()
            .strip_prefix('"')?
            .strip_suffix('"')
            .map(str::to_owned)
    };
    for line in lock.lines().map(str::trim) {
        if line == "[[package]]" {
            (name, version) = (None, None);
        } else if let Some(v) = value(line, "name") {
            name = Some(v);
        } else if let Some(v) = value(line, "version") {
            version = Some(v);
        } else if let (Some(c), Some(n), Some(v)) = (value(line, "checksum"), &name, &version) {
            out.insert(format!("{n} {v}"), c);
        }
    }
    out
}

/// Vendored packages (`<name>-<version>/.cargo-checksum.json`) whose `package` checksum is not
/// the one `locked` has for them, or that the lock does not list.
/// Files the gifsicle release tarball adds to the repository: autotools output.
const AUTOTOOLS_OUTPUT: &[&str] =
    &["aclocal.m4", "compile", "config.h.in", "configure", "depcomp", "install-sh", "missing", "Makefile.in"];

#[derive(Debug, Default, PartialEq)]
struct TreeComparison {
    same: usize,
    release_only: Vec<String>,
    problems: Vec<String>,
}

/// The one folder a source tarball unpacks to.
fn single_dir(dir: &Path) -> Result<PathBuf> {
    let entries: Vec<PathBuf> =
        std::fs::read_dir(dir)?.map(|e| e.map(|e| e.path())).collect::<Result<_, _>>()?;
    match entries.as_slice() {
        [one] if one.is_dir() => Ok(one.clone()),
        _ => bail!("{} does not hold exactly one folder", dir.display()),
    }
}

/// sha256 of every regular file under `dir`, keyed by its `/`-separated relative path.
fn tree_hashes(dir: &Path) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            let entry = entry?;
            let (path, kind) = (entry.path(), entry.file_type()?);
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() {
                let rel = path.strip_prefix(dir)?.components();
                let rel: Vec<String> = rel.map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                out.insert(rel.join("/"), file_sha256(&path)?);
            } else {
                bail!("{} is a link or special file", path.display());
            }
        }
    }
    Ok(out)
}

/// The release tree against a repository snapshot: shared files must be identical, and the
/// release may add only autotools output. Files only in the snapshot (CI, fuzzers) are fine.
fn compare_source_trees(release: &Path, snapshot: &Path) -> Result<TreeComparison> {
    let (r, s) = (tree_hashes(release)?, tree_hashes(snapshot)?);
    let mut cmp = TreeComparison::default();
    for (path, sha) in &r {
        match s.get(path) {
            Some(other) if other == sha => cmp.same += 1,
            Some(_) => cmp.problems.push(format!("{path}: contents differ")),
            None if AUTOTOOLS_OUTPUT.contains(&path.rsplit('/').next().unwrap_or(path)) => {
                cmp.release_only.push(path.clone())
            }
            None => cmp.problems.push(format!("{path}: only in the release tarball")),
        }
    }
    if cmp.same == 0 {
        cmp.problems.push("no files in common".into());
    }
    Ok(cmp)
}

fn vendor_mismatches(vendor: &Path, locked: &BTreeMap<String, String>) -> Result<Vec<String>> {
    let mut bad = Vec::new();
    for entry in std::fs::read_dir(vendor).with_context(|| format!("read {}", vendor.display()))? {
        let dir = entry?.path();
        let Some(dir_name) = dir.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        let text = std::fs::read_to_string(dir.join(".cargo-checksum.json")).unwrap_or_default();
        let sums: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let package = sums["package"].as_str().unwrap_or_default();
        // `--versioned-dirs`: the folder is `<name>-<version>`.
        match locked.iter().find(|(k, _)| k.replacen(' ', "-", 1) == dir_name) {
            Some((_, c)) if c == package => {}
            Some((k, c)) => bad.push(format!("{k}: vendored {package:?}, locked {c}")),
            None => bad.push(format!("{dir_name}: not in Cargo.lock")),
        }
    }
    Ok(bad)
}

/// The notice text with the mirrored source URLs listed after its marker line (replacing a
/// list written by an earlier pin).
fn with_mirrors(notice: &str, urls: &[&String]) -> String {
    const MARKER: &str = "<!-- mirrored sources: cargo xtask pin-gif-tools -->";
    let Some(at) = notice.find(MARKER) else { return notice.to_owned() };
    let head = &notice[..at + MARKER.len()];
    let rest = &notice[at + MARKER.len()..];
    // The earlier list: the lines starting with "- https://" right after the marker.
    let rest = rest.trim_start_matches('\n');
    let kept: Vec<&str> = rest.lines().skip_while(|l| l.starts_with("- https://")).collect();
    let list: String = urls.iter().map(|u| format!("- {u}\n")).collect();
    format!("{head}\n{list}\n{}\n", kept.join("\n").trim_start_matches('\n'))
}

/// `licenses/GIFSKI-DEPS.md`: every package compiled into gifski on the shipped triples, from
/// `cargo metadata` of the crate (its own lock, default features).
fn dependency_notice(manifest: &Path) -> Result<String> {
    let mut packages: BTreeMap<(String, String), String> = BTreeMap::new();
    for triple in TRIPLES {
        let out = Command::new("cargo")
            .args([
                "metadata",
                "--format-version",
                "1",
                "--locked",
                "--filter-platform",
                triple,
                "--manifest-path",
            ])
            .arg(manifest)
            .output()?;
        if !out.status.success() {
            bail!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        let meta: serde_json::Value = serde_json::from_slice(&out.stdout)?;
        let resolved: Vec<&str> = meta["resolve"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n["id"].as_str())
            .collect();
        for p in meta["packages"].as_array().into_iter().flatten() {
            if resolved.contains(&p["id"].as_str().unwrap_or_default()) {
                let key = (
                    p["name"].as_str().unwrap_or_default().to_owned(),
                    p["version"].as_str().unwrap_or_default().to_owned(),
                );
                packages.insert(key, p["license"].as_str().unwrap_or("(see the crate)").to_owned());
            }
        }
    }
    let mut text = format!(
        "# Crates compiled into the bundled gifski {GIFSKI_VERSION}\n\nGenerated by `cargo xtask pin-gif-tools` from \
         the crate's own Cargo.lock (default features, {}). Their licence texts are in the vendored \
         sources linked from GIFSKI-NOTICE.md.\n\n| Crate | Version | Licence |\n|---|---|---|\n",
        TRIPLES.join(", ")
    );
    for ((name, version), licence) in packages {
        text.push_str(&format!("| {name} | {version} | {licence} |\n"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: &str = "z3r0fidev/redd-media-clipper-upscaler";

    fn hex(c: char) -> String {
        std::iter::repeat_n(c, 64).collect()
    }

    fn lock() -> GifToolsLock {
        let rel = format!("https://github.com/{REPO}/releases/download/gif-tools-2026-10-24/");
        let built = |tool: &str| {
            TRIPLES
                .iter()
                .map(|t| (t.to_string(), Built { url: format!("{rel}{tool}-{t}"), sha256: hex('b') }))
                .collect()
        };
        let pinned = |url: &str, mirror: &str| Pinned {
            url: url.into(),
            mirrors: vec![format!("{rel}{mirror}")],
            sha256: hex('a'),
        };
        GifToolsLock {
            comment: "c".into(),
            release: "gif-tools-2026-10-24".into(),
            build_run: 42,
            build_commit: "0123456789abcdef0123456789abcdef01234567".into(),
            gifsicle: GifTool {
                version: "1.98".into(),
                license: "GPL-2.0-only".into(),
                source: pinned(GIFSICLE_SOURCE, "gifsicle-1.98.tar.gz"),
                cargo_lock_sha256: None,
                lock_updates: Vec::new(),
                vendor: None,
                targets: built("gifsicle"),
            },
            gifski: GifTool {
                version: "1.34.0".into(),
                license: "AGPL-3.0-or-later".into(),
                source: pinned(GIFSKI_SOURCE, "gifski-1.34.0.crate"),
                cargo_lock_sha256: Some(hex('c')),
                lock_updates: vec!["crossbeam-epoch 0.9.18 -> 0.9.20 (RUSTSEC-2026-0204)".into()],
                vendor: Some(pinned(
                    &format!("{rel}gifski-1.34.0-vendor.tar.gz"),
                    "gifski-1.34.0-vendor.tar.gz",
                )),
                targets: built("gifski"),
            },
        }
    }

    #[test]
    fn a_complete_lock_passes_and_round_trips() {
        assert_eq!(check_lock(&lock(), REPO), Vec::<String>::new());
        let text = serde_json::to_string_pretty(&lock()).unwrap();
        assert_eq!(serde_json::from_str::<GifToolsLock>(&text).unwrap(), lock());
        assert!(
            serde_json::from_str::<GifToolsLock>(&text.replace("\"comment\"", "\"bogus\": 1, \"comment\""))
                .is_err()
        );
        assert_eq!(archive_pins(&lock()).len(), 3, "two sources and the vendor tarball");
        assert_eq!(all_urls(&lock()).len(), 3 * 2 + 4, "each archive and mirror, and four builds");
    }

    #[test]
    fn each_rule_is_enforced() {
        let bad = |edit: &dyn Fn(&mut GifToolsLock), needle: &str| {
            let mut l = lock();
            edit(&mut l);
            let problems = check_lock(&l, REPO);
            assert!(problems.iter().any(|p| p.contains(needle)), "{problems:?} (wanted {needle})");
        };
        bad(&|l| l.release = "gif-tools-latest".into(), "gif-tools-YYYY-MM-DD");
        bad(&|l| l.build_run = 0, "build_run");
        bad(&|l| l.build_commit = "abc".into(), "build_commit");
        bad(&|l| l.gifsicle.source.sha256 = "ABC".into(), "bad sha256");
        bad(&|l| l.gifsicle.source.mirrors[0] = "http://example.com/g.tar.gz".into(), "not https");
        bad(&|l| l.gifski.source.mirrors.clear(), "no mirror");
        bad(&|l| l.gifski.vendor.as_mut().unwrap().mirrors.clear(), "no mirror");
        bad(&|l| l.gifski.vendor = None, "no vendor archive");
        bad(&|l| l.gifski.cargo_lock_sha256 = None, "cargo_lock_sha256");
        bad(
            &|l| {
                l.gifsicle.targets.remove(TRIPLES[1]);
            },
            "no build for x86_64-pc-windows-msvc",
        );
        bad(
            &|l| l.gifski.targets.get_mut(TRIPLES[0]).unwrap().url = "https://example.com/gifski".into(),
            "must be on https://github.com/",
        );
    }

    #[test]
    fn notices_must_name_the_version_and_mirrored_sources() {
        let dir = tempfile_dir("notices");
        let l = lock();
        let problems = check_notices(&l, &dir);
        assert!(problems.iter().any(|p| p.contains("GIFSICLE-NOTICE.md is missing")), "{problems:?}");
        let gifski_notice = format!(
            "gifski 1.34.0\n{}\n{}\n",
            l.gifski.source.mirrors[0],
            l.gifski.vendor.as_ref().unwrap().mirrors[0]
        );
        std::fs::write(
            dir.join("GIFSICLE-NOTICE.md"),
            format!("gifsicle 1.98\n{}\n", l.gifsicle.source.mirrors[0]),
        )
        .unwrap();
        std::fs::write(dir.join("GIFSKI-NOTICE.md"), &gifski_notice).unwrap();
        for f in ["GIFSKI-DEPS.md", "GIFSICLE-COPYING.txt", "GIFSKI-LICENSE.txt"] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        assert_eq!(check_notices(&l, &dir), Vec::<String>::new());
        std::fs::write(dir.join("GIFSKI-NOTICE.md"), "gifski 1.34.0\n").unwrap();
        let problems = check_notices(&l, &dir);
        assert!(problems.iter().any(|p| p.contains("does not link the mirrored source")), "{problems:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tempfile_dir(tag: &str) -> PathBuf {
        let dir = root().join(format!("target/xtask-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn tree(staged: bool, licence: Option<&str>, license_file: bool) -> PathBuf {
        let dir = tempfile_dir(
            &format!("gate-{staged}-{licence:?}-{license_file}").replace(['"', ' ', '(', ')'], ""),
        );
        std::fs::create_dir_all(dir.join("binaries")).unwrap();
        std::fs::write(dir.join("binaries/rmedia-ffmpeg-x86_64-unknown-linux-gnu"), "").unwrap();
        if staged {
            std::fs::write(dir.join("binaries/rmedia-gifski-x86_64-unknown-linux-gnu"), "").unwrap();
        }
        let meta = licence
            .map(|l| format!("\n[workspace.metadata.rmedia]\napp_licence = \"{l}\" # decided\n"))
            .unwrap_or_default();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!("[workspace]\nmembers = []\n{meta}\n[profile.release]\nlto = true\n"),
        )
        .unwrap();
        if license_file {
            std::fs::write(dir.join("LICENSE"), "licence text").unwrap();
        }
        dir
    }

    #[test]
    fn the_release_gate_blocks_gifski_until_the_licence_is_decided() {
        assert_eq!(
            release_gate(&tree(false, None, false)).unwrap(),
            None,
            "no gifski staged: nothing to decide"
        );
        let undecided = release_gate(&tree(true, None, false)).unwrap().unwrap();
        assert!(undecided.contains("application licence is undecided"), "{undecided}");
        assert!(
            release_gate(&tree(true, Some("AGPL-3.0-or-later"), false)).unwrap().is_some(),
            "no LICENSE file"
        );
        assert!(release_gate(&tree(true, None, true)).unwrap().is_some(), "no app_licence");
        let wrong = release_gate(&tree(true, Some("MIT OR Apache-2.0"), true)).unwrap().unwrap();
        assert!(wrong.contains("is not one it may ship under"), "{wrong}");
        assert_eq!(release_gate(&tree(true, Some("AGPL-3.0-or-later"), true)).unwrap(), None);
        assert_eq!(release_gate(&tree(true, Some("commercial-gifski-imagequant"), true)).unwrap(), None);
        assert!(
            release_gate(&tree(true, Some("commercial-gifski"), true)).unwrap().is_some(),
            "imagequant too"
        );
    }

    #[test]
    fn app_licence_is_read_from_its_table_only() {
        assert_eq!(
            app_licence("[workspace.metadata.rmedia]\napp_licence = \"GPL-3.0-only\"\n").as_deref(),
            Some("GPL-3.0-only")
        );
        assert_eq!(app_licence("[package]\napp_licence = \"GPL-3.0-only\"\n"), None);
        assert_eq!(app_licence("[workspace.metadata.rmedia]\nother = 1\n[x]\napp_licence = \"A\"\n"), None);
    }

    /// A trimmed `GET /repos/{repo}/actions/runs/{id}` response (recorded JSON text).
    const RUN: &str = r#"{"id": 42, "name": "GIF tools", "path": ".github/workflows/gif-tools.yml",
        "conclusion": "success", "head_sha": "0123456789abcdef0123456789abcdef01234567",
        "head_branch": "main", "event": "workflow_dispatch", "status": "completed"}"#;

    #[test]
    fn runs_are_checked_before_anything_is_downloaded() {
        let run: RunInfo = serde_json::from_str(RUN).unwrap();
        check_run(&run, 42).unwrap();
        let fails = |edit: &dyn Fn(&mut RunInfo), id: u64, needle: &str| {
            let mut r = run.clone();
            edit(&mut r);
            let e = check_run(&r, id).unwrap_err().to_string();
            assert!(e.contains(needle), "{e} (wanted {needle})");
        };
        fails(&|_| {}, 43, "asked for run 43");
        fails(&|r| r.path = ".github/workflows/ci.yml".into(), 42, "not .github/workflows/gif-tools.yml");
        fails(&|r| r.conclusion = Some("failure".into()), 42, "did not succeed");
        fails(&|r| r.conclusion = None, 42, "did not succeed");
        fails(&|r| r.head_branch = "feature".into(), 42, "not a manual run on main");
        fails(&|r| r.event = "push".into(), 42, "not a manual run on main");
    }

    #[test]
    fn the_crates_io_index_checksum_is_found_by_version() {
        let index = concat!(
            r#"{"name":"gifski","vers":"1.33.0","cksum":"aaaa","yanked":false}"#,
            "\n",
            r#"{"name":"gifski","vers":"1.34.0","cksum":"c246c795a61d4a2476fb1c8ab70bedfaa825c734882adc40e117fc837df81190","yanked":false}"#,
            "\n"
        );
        assert_eq!(index_checksum(index, "1.34.0").as_deref(), Some(GIFSKI_SHA256));
        assert_eq!(index_checksum(index, "9.9.9"), None);
        assert_eq!(index_checksum("not json\n", "1.34.0"), None);
    }

    #[test]
    fn sha256sums_are_parsed_strictly() {
        let sums = parse_sums(&format!("{}  gifsicle-1.98-x86_64-unknown-linux-gnu\n\n", hex('d'))).unwrap();
        assert_eq!(sums["gifsicle-1.98-x86_64-unknown-linux-gnu"], hex('d'));
        assert!(parse_sums("abc  file\n").is_err(), "short hash");
        assert!(parse_sums(&format!("{}  ../escape\n", hex('d'))).is_err(), "path in a name");
        assert!(parse_sums(&format!("{} one-space\n", hex('d'))).is_err());
        assert_eq!(asset_names("1.98", "1.34.0").len(), 4);
        assert!(
            asset_names("1.98", "1.34.0")
                .iter()
                .any(|(n, _, _)| n == "gifski-1.34.0-x86_64-pc-windows-msvc.exe")
        );
    }

    #[test]
    fn pinning_lists_the_mirrors_after_the_marker() {
        let notice = "# n\n\n- The project mirrors it:\n\n<!-- mirrored sources: cargo xtask pin-gif-tools -->\n\nOn request …\n";
        let a = "https://example.com/a".to_owned();
        let b = "https://example.com/b".to_owned();
        let once = with_mirrors(notice, &[&a]);
        assert!(once.contains("-->\n- https://example.com/a\n\nOn request"), "{once}");
        let twice = with_mirrors(&once, &[&b]);
        assert!(!twice.contains("/a") && twice.contains("- https://example.com/b\n\nOn request"), "{twice}");
        assert_eq!(with_mirrors("no marker", &[&a]), "no marker");
    }

    #[test]
    fn the_release_must_name_the_run_exactly_and_attestations_are_bound_to_main() {
        assert!(names_run("notes\nbuild_run: 12\nbuild_commit: x", 12));
        assert!(!names_run("build_run: 123", 12), "no substring matches");
        assert!(!names_run("see build_run: 12 above", 12));
        let args = attest_args("f", REPO, "abc").join(" ");
        for need in [
            "--source-ref refs/heads/main",
            "--source-digest abc",
            "--signer-digest abc",
            "--deny-self-hosted-runners",
        ] {
            assert!(args.contains(need), "{args}");
        }
    }

    #[test]
    fn vendored_packages_must_match_the_lock() {
        let lock = "[[package]]\nname = \"gifski\"\nversion = \"1.34.0\"\n\n[[package]]\nname = \"loop9\"\nversion = \"0.1.5\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"aa\"\n\n[[package]]\nname = \"rgb-x\"\nversion = \"0.8.0\"\nchecksum = \"bb\"\n";
        let locked = lock_checksums(lock);
        assert_eq!(locked.len(), 2, "{locked:?}");
        assert_eq!(locked["loop9 0.1.5"], "aa");
        let dir = tempfile_dir("vendor");
        let pkg = |name: &str, sum: &str| {
            std::fs::create_dir_all(dir.join(name)).unwrap();
            let json = format!("{{\"files\":{{}},\"package\":\"{sum}\"}}");
            std::fs::write(dir.join(name).join(".cargo-checksum.json"), json).unwrap();
        };
        pkg("loop9-0.1.5", "aa");
        pkg("rgb-x-0.8.0", "bb");
        assert_eq!(vendor_mismatches(&dir, &locked).unwrap(), Vec::<String>::new());
        pkg("rgb-x-0.8.0", "cc");
        pkg("evil-1.0.0", "dd");
        assert_eq!(vendor_mismatches(&dir, &locked).unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn release_tags_have_a_date() {
        assert!(is_release_tag("gif-tools-2026-10-24"));
        for bad in ["gif-tools-2026-1-24", "gif-tools-", "mirror-2026-10-24", "gif-tools-2026-10-24x"] {
            assert!(!is_release_tag(bad), "{bad}");
        }
    }

    #[test]
    fn source_trees_compare_by_contents() {
        let base = tempfile_dir("trees");
        let write = |dir: &str, file: &str, text: &str| {
            let p = base.join(dir).join(file);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        for d in ["rel", "snap"] {
            write(d, "src/gifsicle.c", "int main;");
        }
        write("rel", "configure", "#!/bin/sh");
        write("rel", "src/Makefile.in", "all:");
        write("snap", ".github/ci.yml", "on: push");
        let ok = compare_source_trees(&base.join("rel"), &base.join("snap")).unwrap();
        assert_eq!((ok.same, ok.problems.len()), (1, 0), "{ok:?}");
        assert_eq!(ok.release_only, ["configure", "src/Makefile.in"]);

        write("rel", "src/extra.c", "evil");
        write("snap", "src/gifsicle.c", "int main; /* changed */");
        let bad = compare_source_trees(&base.join("rel"), &base.join("snap")).unwrap();
        assert_eq!(
            bad.problems,
            [
                "src/extra.c: only in the release tarball",
                "src/gifsicle.c: contents differ",
                "no files in common"
            ]
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn lock_changes_name_each_move() {
        let m = |v: &[(&str, &str)]| v.iter().map(|(k, c)| (k.to_string(), c.to_string())).collect();
        let before = m(&[("crossbeam-epoch 0.9.18", "a"), ("rayon 1.10.0", "b"), ("rgb 0.8.50", "c")]);
        let bumped = m(&[("crossbeam-epoch 0.9.20", "d"), ("rayon 1.10.0", "b"), ("rgb 0.8.50", "c")]);
        assert_eq!(lock_changes(&before, &bumped), ["crossbeam-epoch 0.9.18 -> 0.9.20"]);
        assert_eq!(lock_changes(&before, &before), Vec::<String>::new());
        let sneaky = m(&[("crossbeam-epoch 0.9.18", "a"), ("evil 1.0.0", "e"), ("rayon 1.10.0", "x")]);
        assert_eq!(lock_changes(&before, &sneaky), ["+ evil 1.0.0", "- rgb 0.8.50", "~ rayon 1.10.0"]);
    }
}
