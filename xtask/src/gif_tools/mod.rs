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

mod gates;
mod pin;

pub use gates::*;
pub use pin::*;

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

/// `gif-tools-YYYY-MM-DD`, or `gif-tools-YYYY-MM-DD.<run-id>` for a second build that day.
fn is_release_tag(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("gif-tools-") else { return false };
    // A second build on the same day: `gif-tools-YYYY-MM-DD.<run-id>`.
    let date = match rest.split_once('.') {
        Some((date, run)) if !run.is_empty() && run.bytes().all(|c| c.is_ascii_digit()) => date,
        Some(_) => return false,
        None => rest,
    };
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
        problems.push(format!("release {:?} must look like gif-tools-YYYY-MM-DD[.<run-id>]", lock.release));
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

// --- pin-gif-tools <run-id> ----------------------------------------------------------------

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
    fn the_bundle_scan_finds_gifski_by_name_and_in_installer_scripts() {
        // A tree with no licence decision; `built` stands for target/release or dist.
        let build = |tag: &str, files: &[(&str, &str)]| {
            let tree = tempfile_dir(&format!("bundle-{tag}"));
            std::fs::write(tree.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
            for (f, text) in files {
                let path = tree.join("built").join(f);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, text).unwrap();
            }
            tree
        };
        let clean = build(
            "clean",
            &[
                ("bundle/deb/data/usr/bin/rmedia-gifsicle", ""),
                ("nsis/x64/installer.nsi", "File rmedia-ffmpeg.exe"),
                ("deps/gifski-cache.rlib", "intermediate"),
            ],
        );
        assert_eq!(bundle_gate(&clean, &[clean.join("built")]).unwrap(), None);
        let _ = std::fs::remove_dir_all(&clean);

        for (n, (file, text)) in [
            ("bundle/appimage/rmedia.AppDir/usr/bin/rmedia-gifski", ""),
            ("licenses/GIFSKI-NOTICE.md", ""),
            ("wix/x64/main.wxs", "<File Source=\"rmedia-gifski.exe\"/>"),
            ("nsis/x64/installer.nsi", "\0F\0i\0l\0e\0 \0G\0I\0F\0S\0K\0I"),
        ]
        .into_iter()
        .enumerate()
        {
            let tree = build(&n.to_string(), &[(file, text)]);
            let why = bundle_gate(&tree, &[tree.join("built")]).unwrap().unwrap();
            assert!(why.contains("is in the build") && why.contains("undecided"), "{file}: {why}");
            let _ = std::fs::remove_dir_all(&tree);
        }
        assert!(bundle_gate(Path::new("."), &[PathBuf::from("/nonexistent-rmedia")]).is_err());
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
        assert!(is_release_tag("gif-tools-2026-10-24.40204764127"));
        for bad in [
            "gif-tools-2026-1-24",
            "gif-tools-",
            "mirror-2026-10-24",
            "gif-tools-2026-10-24x",
            "gif-tools-2026-10-24.",
            "gif-tools-2026-10-24.1a",
        ] {
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
        write("rel", "src/configure", "#!/bin/sh evil");
        let nested = compare_source_trees(&base.join("rel"), &base.join("snap")).unwrap();
        assert_eq!(nested.problems, ["src/configure: only in the release tarball"], "exact paths only");
        std::fs::remove_file(base.join("rel/src/configure")).unwrap();
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
    fn vendored_files_must_match_the_crate() {
        let base = tempfile_dir("vendored");
        let write = |p: &str, text: &str| {
            let p = base.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        write("v/rgb-0.8.0/src/lib.rs", "pub struct Rgb;");
        write("v/rgb-0.8.0/.cargo-checksum.json", "{}");
        write("c/rgb-0.8.0/src/lib.rs", "pub struct Rgb;");
        write("c/rgb-0.8.0/Cargo.toml.orig", "[package]");
        let check = || vendored_differences(&base.join("v/rgb-0.8.0"), &base.join("c/rgb-0.8.0")).unwrap();
        assert_eq!(check(), Vec::<String>::new());
        write("v/rgb-0.8.0/src/lib.rs", "pub struct Rgb; // patched");
        write("v/rgb-0.8.0/build.rs", "fn main() {}");
        assert_eq!(check(), ["rgb-0.8.0/build.rs", "rgb-0.8.0/src/lib.rs"]);
        let lock = "source = \"registry+https://github.com/rust-lang/crates.io-index\"\nsource = \"git+https://x/y#abc\"\n";
        assert_eq!(non_registry_sources(lock), ["\"git+https://x/y#abc\""]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn unsafe_tar_members_are_named() {
        let names = "g-1/\ng-1/src/a.c\n/etc/passwd\ng-1/../x\nC:/w\n";
        let long = "drwxr-xr-x 0/0 0 2026-01-01 g-1/\n-rw-r--r-- 0/0 1 2026-01-01 g-1/src/a.c\n\
                    lrwxrwxrwx 0/0 0 2026-01-01 g-1/l -> /etc\n";
        let bad = unsafe_members(names, long);
        assert_eq!(bad.len(), 4, "{bad:?}");
        assert!(bad[3].starts_with("entry l"), "{bad:?}");
        assert!(unsafe_members("g-1/\ng-1/a\n", "drwx g-1/\n-rw g-1/a\n").is_empty());
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
