//! Feature 005: the CLI contract for `--fit` (contracts/cli.md, contracts/json-output.md):
//! results, usage errors and exit codes (T016; later stories add to it).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use assert_cmd::cargo::cargo_bin;
use rmedia_testkit::{Fixture, fixture, require_engine};
use serde_json::Value;

/// `rmedia` with state and config isolated in `dir` (no user targets leak in).
fn rmedia(dir: &Path) -> Command {
    let mut c = Command::new(cargo_bin("rmedia"));
    let none = dir.join("no-driver.json");
    c.env("RMEDIA_STATE_DIR", dir.join("state"))
        .env("RMEDIA_CONFIG_DIR", dir.join("config"))
        .env("RMEDIA_CACHE_DIR", dir.join("cache"))
        .env("VK_DRIVER_FILES", &none)
        .env("VK_ICD_FILENAMES", &none);
    c
}

fn copy(kind: Fixture, dir: &Path, name: &str) -> PathBuf {
    let dst = dir.join(name);
    std::fs::copy(fixture(kind), &dst).unwrap();
    dst
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// The `--json` lines of a run.
fn events(o: &Output) -> Vec<Value> {
    stdout(o).lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// T016: a fit run prints its size against the cap; `--json` carries the fit block.
#[test]
fn a_fit_run_reports_its_size_against_the_cap() {
    require_engine!("libx264", "aac");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::FitBusy, d.path(), "busy.mp4");
    let o = rmedia(d.path())
        .args(["clip", "-s", "2", "-e", "12", "--fit", "reddit-video", "--fit-max-size", "4MB"])
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let text = stdout(&o) + &stderr(&o);
    assert!(text.contains(" MB of 4.0 MB ("), "{text}");
    assert!(text.contains("2 passes"), "{text}");

    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::FitBusy, d.path(), "busy.mp4");
    let o = rmedia(d.path())
        .args(["clip", "--json", "-s", "2", "-e", "12", "--fit", "reddit-video", "--fit-max-size", "4MB"])
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let done = events(&o).into_iter().find(|e| e["type"] == "job_finished").expect("a job_finished event");
    let fit = &done["outcome"]["fit"];
    assert_eq!(fit["cap"], 4_000_000, "{done}");
    assert!(fit["bytes"].as_u64().unwrap() <= 4_000_000);
    assert_eq!(fit["mode"], "budget");
    assert_eq!(fit["attempts"].as_array().unwrap().len(), 1);
}

/// T016: misused flags are usage errors (2); an unknown target is invalid input (3).
#[test]
fn fit_flag_misuse_and_unknown_targets_exit_with_their_codes() {
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::FitShort, d.path(), "short.mp4");
    for (args, code, text) in [
        (vec!["--fit-max-size", "4MB"], 2, "--fit"),
        (vec!["--fit-allow-trim"], 2, "--fit"),
        (vec!["--fit", "reddit-video", "--gif"], 2, "--gif"),
        (vec!["--fit", "nope"], 3, "TARGET_UNKNOWN"),
        (vec!["--fit", "reddit-video", "--container", "mkv"], 3, "mp4"),
    ] {
        let o = rmedia(d.path())
            .args(["clip", "--dry-run", "--json", "-s", "0", "-e", "1"])
            .args(&args)
            .arg(&src)
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(code), "{args:?}: {}", stderr(&o));
        let all = stdout(&o) + &stderr(&o);
        assert!(all.contains(text), "{args:?}: {all}");
    }
}
