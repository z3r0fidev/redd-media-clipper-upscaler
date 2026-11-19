//! `pin-gif-tools`: checks a gif-tools workflow run, its sources and attestations, and writes the lock.

use super::*;

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
pub(super) const GIFSICLE_VERSION: &str = "1.98";

pub(super) const GIFSICLE_SOURCE: &str = "https://www.lcdf.org/gifsicle/gifsicle-1.98.tar.gz";

pub(super) const GIFSICLE_SHA256: &str = "e9d4cc0816db95c64d32e79f778836db0f1f7eb15868490bd330104e5146b66e";

pub(super) const GIFSKI_VERSION: &str = "1.34.0";

pub(super) const GIFSKI_SOURCE: &str = "https://static.crates.io/crates/gifski/gifski-1.34.0.crate";

pub(super) const GIFSKI_SHA256: &str = "c246c795a61d4a2476fb1c8ab70bedfaa825c734882adc40e117fc837df81190";

/// Security updates applied to gifski's `Cargo.lock` before vendoring (name, from, to, advisory);
/// the same list as `GIFSKI_LOCK_UPDATES` in gif-tools.yml, checked against the shipped lock.
pub(super) const GIFSKI_LOCK_UPDATES: &[(&str, &str, &str, &str)] =
    &[("crossbeam-epoch", "0.9.18", "0.9.20", "RUSTSEC-2026-0204")];

pub(super) fn curl_text(url: &str) -> Result<Option<String>> {
    let out = Command::new("curl")
        .args(["-fsSL", "--proto", "=https", "--proto-redir", "=https", "--max-time", "120", "--url", url])
        .output()?;
    Ok(out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned()))
}

/// `cargo xtask pin-gif-tools <run-id>`: checks the run and every asset, then writes the lock.
/// `without_debian`: pin even when Debian has no copy of this gifsicle to compare with.
pub fn pin(run_id: u64, without_debian: bool) -> Result<()> {
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
    // A fresh, randomly named 0700 folder (never one another user pre-created), removed on drop.
    let tmp = tempfile::Builder::new().prefix("rmedia-pin-gif-tools-").tempdir()?;
    let work = tmp.path().to_path_buf();
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
            // Debian's copy is not hash-pinned: only plain files and folders, no escaping paths.
            let unsafe_members = unsafe_members(&tar_list(archive, "-tzf")?, &tar_list(archive, "-tvzf")?);
            if !unsafe_members.is_empty() {
                bail!("{} has unsafe members:\n  {}", archive.display(), unsafe_members.join("\n  "));
            }
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
    } else if without_debian {
        println!(
            "no Debian orig tarball for gifsicle {GIFSICLE_VERSION}: not compared (say so in the commit)"
        );
    } else {
        bail!(
            "cannot fetch Debian's gifsicle {GIFSICLE_VERSION} ({debian}) to compare with; pass \
             --without-debian to pin without that check"
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
    let foreign = non_registry_sources(&shipped);
    if !foreign.is_empty() {
        bail!("the shipped Cargo.lock has non-registry sources:\n  {}", foreign.join("\n  "));
    }
    let locked = lock_checksums(&shipped);
    let mismatched = vendor_mismatches(&vendor_dir.join("vendor"), &locked)?;
    if !mismatched.is_empty() {
        bail!("vendored packages that are not the locked ones:\n  {}", mismatched.join("\n  "));
    }
    // `.cargo-checksum.json` is written by whoever made the tarball, so each vendored package is
    // compared with its `.crate` from crates.io, checked against the lock's checksum.
    let crates_dir = work.join("crates");
    let mut differ = Vec::new();
    for (key, checksum) in &locked {
        let (name, version) = key.split_once(' ').context("a lock key without a version")?;
        let file = crates_dir.join(format!("{name}-{version}.crate"));
        std::fs::create_dir_all(&crates_dir)?;
        crate::download(&format!("https://static.crates.io/crates/{name}/{name}-{version}.crate"), &file)?;
        if file_sha256(&file)? != *checksum {
            bail!("crates.io's {name} {version} is not the locked checksum {checksum}");
        }
        let unpacked = crates_dir.join(format!("{name}-{version}.d"));
        let members = unsafe_members(&tar_list(&file, "-tzf")?, &tar_list(&file, "-tvzf")?);
        if !members.is_empty() {
            bail!("{name} {version} has unsafe members:\n  {}", members.join("\n  "));
        }
        std::fs::create_dir_all(&unpacked)?;
        crate::run(Command::new(crate::tar()).arg("-xzf").arg(&file).arg("-C").arg(&unpacked))?;
        let vendored = vendor_dir.join("vendor").join(format!("{name}-{version}"));
        differ.extend(vendored_differences(&vendored, &unpacked.join(format!("{name}-{version}")))?);
    }
    if !differ.is_empty() {
        bail!("vendored sources that differ from crates.io:\n  {}", differ.join("\n  "));
    }
    println!("vendored sources: {} packages identical to their crates.io .crate files", locked.len());
    crate::run(
        Command::new("cargo")
            .args(["deny", "--manifest-path"])
            .arg(&manifest)
            .args(["--locked", "--config"])
            .arg(root().join("deny-gifski.toml"))
            .args(["check", "licenses", "bans", "advisories", "sources"]),
    )?;
    // `cargo` itself starts without cargo-audit and exits non-zero, so ask for its version first.
    let audit_installed =
        Command::new("cargo").args(["audit", "--version"]).output().is_ok_and(|o| o.status.success());
    if !audit_installed {
        println!("cargo audit is not installed: advisories were checked by cargo deny only");
    } else if Command::new("cargo").args(["audit", "--file"]).arg(&cargo_lock).status()?.success() {
        println!("cargo audit: no advisories");
    } else {
        bail!("cargo audit found advisories in gifski's Cargo.lock");
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
    drop(tmp);
    println!("pinned gifsicle {GIFSICLE_VERSION} and gifski {GIFSKI_VERSION} from {release} (run {run_id})");
    println!(
        "notices updated with the mirrored sources; next: `cargo xtask check-pins --online`, then commit"
    );
    Ok(())
}

/// `gh attestation verify` for `file`: signed by gif-tools.yml, from `main` at `commit`, on a
/// GitHub-hosted runner.
pub(super) fn attest_args(file: &str, repo: &str, commit: &str) -> Vec<String> {
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
pub(super) fn names_run(body: &str, run: u64) -> bool {
    body.lines().any(|l| l.trim() == format!("build_run: {run}"))
}

/// How `after` moves packages off `before` (both from [`lock_checksums`]): `name old -> new` for
/// each name whose single version changed, and `+ name version` / `- name version` for anything
/// else, sorted. A changed checksum for the same version is reported as `~ name version`.
pub(super) fn lock_changes(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Vec<String> {
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

/// Packages in a Cargo.lock whose `source` is not crates.io (git, other registries).
pub(super) fn non_registry_sources(lock: &str) -> Vec<String> {
    lock.lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("source = "))
        .filter(|v| *v != "\"registry+https://github.com/rust-lang/crates.io-index\"")
        .map(str::to_owned)
        .collect()
}

/// Files of a vendored package that are not byte-identical in the unpacked `.crate` (`cargo
/// vendor` may leave files out, and adds `.cargo-checksum.json`).
pub(super) fn vendored_differences(vendored: &Path, unpacked: &Path) -> Result<Vec<String>> {
    let (v, c) = (tree_hashes(vendored)?, tree_hashes(unpacked)?);
    let name = vendored.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(v.iter()
        .filter(|(path, _)| path.as_str() != ".cargo-checksum.json")
        .filter(|(path, sha)| c.get(*path) != Some(*sha))
        .map(|(path, _)| format!("{name}/{path}"))
        .collect())
}

/// `name version → checksum` of every registry package in a Cargo.lock.
pub(super) fn lock_checksums(lock: &str) -> BTreeMap<String, String> {
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
/// Files the gifsicle release tarball adds to the repository: autotools output, by exact path.
/// The Debian comparison does not cover them (the Linux build runs `configure`); the release's
/// sha256 pin is their only check (validation.md).
pub(super) const AUTOTOOLS_OUTPUT: &[&str] = &[
    "Makefile.in",
    "aclocal.m4",
    "compile",
    "config.h.in",
    "configure",
    "depcomp",
    "install-sh",
    "missing",
    "src/Makefile.in",
];

/// The member list of a gzip tarball (`-tzf` names, `-tvzf` long listing).
pub(super) fn tar_list(archive: &Path, flag: &str) -> Result<String> {
    let out = Command::new(crate::tar()).arg(flag).arg(archive).output()?;
    if !out.status.success() {
        bail!("tar {flag} {} failed", archive.display());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Members that are links or special files, or whose paths are absolute or contain `..`.
pub(super) fn unsafe_members(names: &str, long: &str) -> Vec<String> {
    let bad_path = |n: &&str| n.starts_with('/') || n.split('/').any(|c| c == "..") || n.contains(':');
    let mut out: Vec<String> = names.lines().filter(bad_path).map(|n| format!("path {n}")).collect();
    out.extend(long.lines().filter(|l| !l.starts_with(['-', 'd'])).map(|l| format!("entry {l}")));
    out
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct TreeComparison {
    pub(super) same: usize,
    pub(super) release_only: Vec<String>,
    pub(super) problems: Vec<String>,
}

/// The one folder a source tarball unpacks to.
pub(super) fn single_dir(dir: &Path) -> Result<PathBuf> {
    let entries: Vec<PathBuf> =
        std::fs::read_dir(dir)?.map(|e| e.map(|e| e.path())).collect::<Result<_, _>>()?;
    match entries.as_slice() {
        [one] if one.is_dir() => Ok(one.clone()),
        _ => bail!("{} does not hold exactly one folder", dir.display()),
    }
}

/// sha256 of every regular file under `dir`, keyed by its `/`-separated relative path.
pub(super) fn tree_hashes(dir: &Path) -> Result<BTreeMap<String, String>> {
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
pub(super) fn compare_source_trees(release: &Path, snapshot: &Path) -> Result<TreeComparison> {
    let (r, s) = (tree_hashes(release)?, tree_hashes(snapshot)?);
    let mut cmp = TreeComparison::default();
    for (path, sha) in &r {
        match s.get(path) {
            Some(other) if other == sha => cmp.same += 1,
            Some(_) => cmp.problems.push(format!("{path}: contents differ")),
            None if AUTOTOOLS_OUTPUT.contains(&path.as_str()) => cmp.release_only.push(path.clone()),
            None => cmp.problems.push(format!("{path}: only in the release tarball")),
        }
    }
    if cmp.same == 0 {
        cmp.problems.push("no files in common".into());
    }
    Ok(cmp)
}

pub(super) fn vendor_mismatches(vendor: &Path, locked: &BTreeMap<String, String>) -> Result<Vec<String>> {
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
pub(super) fn with_mirrors(notice: &str, urls: &[&String]) -> String {
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
pub(super) fn dependency_notice(manifest: &Path) -> Result<String> {
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
