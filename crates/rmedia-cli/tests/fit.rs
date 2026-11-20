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

/// T026: plan-time refusals exit 3 with the contract's messages, and write nothing; an override
/// above the target is a usage error (2).
#[test]
fn refusals_exit_before_any_encode() {
    require_engine!("libx264", "aac");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::FitBusy, d.path(), "busy.mp4");
    let target = d.path().join("config/video-targets");
    std::fs::create_dir_all(&target).unwrap();
    let short = serde_json::json!({
        "schema": 1, "id": "short", "version": 1, "name": "short", "description": "10 s",
        "verified": false, "max_bytes": 50_000_000, "max_duration": 10,
        "long_side": 1920, "floor_short_side": 480, "max_fps": 60,
        "quality": {"crf": 20, "max_bpp": 0.12, "min_bpp": 0.03},
        "audio": {"kbps": 128, "floor_kbps": 64}
    });
    std::fs::write(target.join("short.json"), short.to_string()).unwrap();

    for (args, code, text) in [
        (
            vec!["--fit", "short"],
            3,
            "the range (0:25) is longer than short allows (0:10); use --fit-allow-trim to keep the first 0:10",
        ),
        (
            vec!["--fit", "reddit-video", "--fit-max-size", "200KB"],
            3,
            "25 s cannot fit 200 kB even at 854x480 (needs at least 1.4 MB); raise --fit-max-size or use --fit-allow-trim",
        ),
        (
            vec!["--fit", "short", "--fit-max-length", "20"],
            2,
            "--fit-max-length 0:20 is above short's limit of 0:10",
        ),
        (
            vec!["--fit", "short", "--fit-max-size", "60MB"],
            2,
            "--fit-max-size 60.0 MB is above short's cap of 50.0 MB",
        ),
    ] {
        let o =
            rmedia(d.path()).args(["clip", "-s", "2", "-e", "27"]).args(&args).arg(&src).output().unwrap();
        assert_eq!(o.status.code(), Some(code), "{args:?}: {}", stderr(&o));
        let all = stdout(&o) + &stderr(&o);
        assert!(all.contains(text), "{args:?}: {all}");
        assert!(!d.path().join("clips").exists(), "{args:?} wrote output");
    }

    // `--json` carries the detail.
    let o = rmedia(d.path())
        .args([
            "clip",
            "--json",
            "--dry-run",
            "-s",
            "2",
            "-e",
            "27",
            "--fit",
            "reddit-video",
            "--fit-max-size",
            "200KB",
        ])
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(3), "{}", stderr(&o));
    let all = stdout(&o) + &stderr(&o);
    let doc: Value = all.lines().find_map(|l| serde_json::from_str(l).ok()).expect("a JSON document");
    let issue =
        doc["issues"].as_array().unwrap().iter().find(|i| i["code"] == "BUDGET_UNREACHABLE").expect("issue");
    assert!(issue["detail"]["min_cap"].as_u64().unwrap() > 1_000_000, "{issue}");
}
