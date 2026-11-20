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

/// The dry run's JSON document (`--dry-run --json` prints one).
fn plan_doc(o: &Output) -> Value {
    stdout(o).lines().find_map(|l| serde_json::from_str::<Value>(l).ok()).expect("a plan document")
}

/// T030: the dry run shows the fit (text and JSON) and writes nothing; its prediction is close
/// to the real size (SC-006).
#[test]
fn the_dry_run_shows_the_fit_and_its_prediction_holds() {
    require_engine!("libx264", "aac");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::FitBusy, d.path(), "busy.mp4");
    let dry = |extra: &[&str]| {
        rmedia(d.path())
            .args(["clip", "--dry-run", "-s", "2", "-e", "27"])
            .args(extra)
            .arg(&src)
            .output()
            .unwrap()
    };

    // Budget mode.
    let o = dry(&["--fit", "reddit-video", "--fit-max-size", "8MB"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let text = stdout(&o);
    assert!(
        text.contains("fit reddit-video (unverified): 1920x1080 30 fps · video ")
            && text.contains(" kb/s + audio 128 kb/s (budget) · ")
            && text.contains(" of 8.0 MB"),
        "{text}"
    );
    // Quality mode: a generous cap.
    let o = dry(&["--fit", "reddit-video"]);
    let text = stdout(&o);
    assert!(
        text.contains("1920x1080 30 fps quality · crf 20 ≤ ") && text.contains("(a 25 s range)"),
        "{text}"
    );
    // A lowered size.
    let o = dry(&["--fit", "reddit-video", "--fit-max-size", "5MB"]);
    let text = stdout(&o);
    assert!(text.contains("1440x810") && text.contains("size lowered 1920x1080 → 1440x810"), "{text}");
    assert!(!d.path().join("clips").exists(), "a dry run wrote output");

    // JSON: exactly the contract's keys.
    let o = dry(&["--json", "--fit", "reddit-video", "--fit-max-size", "8MB"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc = plan_doc(&o);
    let fit = &doc["jobs"][0]["fit"];
    let mut keys: Vec<&str> = fit.as_object().expect("a fit block").keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "audio_kbps",
            "cap",
            "crf",
            "fps",
            "len",
            "lowered",
            "max_length",
            "mode",
            "passes",
            "predicted_bytes",
            "size",
            "target",
            "target_verified",
            "trimmed_from",
            "video_kbps"
        ]
    );
    assert_eq!(
        (fit["mode"].as_str(), fit["passes"].as_u64(), fit["crf"].is_null()),
        (Some("budget"), Some(2), true)
    );
    assert_eq!(
        (fit["size"][0].as_u64(), fit["fps"].as_f64(), fit["len"].as_f64()),
        (Some(1920), Some(30.0), Some(25.0))
    );
    let predicted = fit["predicted_bytes"].as_u64().unwrap();

    // SC-006: the real size is within 15% of the prediction.
    let o = rmedia(d.path())
        .args(["clip", "--json", "-s", "2", "-e", "27", "--fit", "reddit-video", "--fit-max-size", "8MB"])
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let done = events(&o).into_iter().find(|e| e["type"] == "job_finished").expect("job_finished");
    let bytes = done["outcome"]["fit"]["bytes"].as_u64().unwrap();
    let off = (bytes as f64 - predicted as f64).abs() / predicted as f64;
    assert!(off <= 0.15, "predicted {predicted}, got {bytes} ({:.1}% off)", off * 100.0);
}

/// T031: `clip --list-fit-targets` lists built-in and user targets and reports broken files;
/// `doctor --json` has the `fit` block, and target problems leave its status alone.
#[test]
fn fit_targets_are_listed_and_reported_by_doctor() {
    let d = tempfile::tempdir().unwrap();
    let folder = d.path().join("config/video-targets");
    std::fs::create_dir_all(&folder).unwrap();
    let user = serde_json::json!({
        "schema": 1, "id": "sub-50mb", "version": 1, "name": "r/example rule", "description": "50 MB",
        "verified": false, "max_bytes": 50_000_000, "max_duration": 300,
        "long_side": 1920, "floor_short_side": 480, "max_fps": 60,
        "quality": {"crf": 20, "max_bpp": 0.12, "min_bpp": 0.03},
        "audio": {"kbps": 128, "floor_kbps": 64}
    });
    std::fs::write(folder.join("sub-50mb.json"), user.to_string()).unwrap();
    std::fs::write(folder.join("broken.json"), r#"{"schema": 1, "id": "broken", "max_bytes": 0}"#).unwrap();

    let o = rmedia(d.path()).args(["clip", "--list-fit-targets"]).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let text = stdout(&o);
    assert!(
        text.contains("reddit-video") && text.contains("15:00 · 1920 px · unverified   built-in"),
        "{text}"
    );
    assert!(text.contains("sub-50mb") && text.contains("05:00") && text.contains("user ("), "{text}");
    assert!(stderr(&o).contains("broken.json ignored"), "{}", stderr(&o));

    let o = rmedia(d.path()).args(["clip", "--list-fit-targets", "--json"]).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc: Value = serde_json::from_str(stdout(&o).trim()).unwrap();
    let targets = doc["targets"].as_array().unwrap();
    let ids: Vec<&str> = targets.iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["reddit-video", "sub-50mb"]);
    assert_eq!(
        (targets[0]["source"].as_str(), targets[1]["source"].as_str()),
        (Some("builtin"), Some("user"))
    );
    assert!(targets[1]["file"].as_str().unwrap().ends_with("sub-50mb.json"));
    assert_eq!(targets[0]["max_bytes"], 1_073_741_824);
    assert!(doc["problems"][0]["file"].as_str().unwrap().ends_with("broken.json"));

    // doctor: the fit block; a broken target file never changes the status.
    let o = rmedia(d.path()).args(["doctor", "--json"]).output().unwrap();
    let doc: Value = serde_json::from_str(stdout(&o).trim()).expect("one JSON document");
    let fit = &doc["fit"];
    assert_eq!(fit["targets"], serde_json::json!(["reddit-video", "sub-50mb"]), "{fit}");
    assert!(fit["problems"][0]["file"].as_str().unwrap().ends_with("broken.json"), "{fit}");
    let clean = tempfile::tempdir().unwrap();
    let o2 = rmedia(clean.path()).args(["doctor", "--json"]).output().unwrap();
    let doc2: Value = serde_json::from_str(stdout(&o2).trim()).unwrap();
    assert_eq!((o.status.code(), &doc["ok"]), (o2.status.code(), &doc2["ok"]));
}

/// T037: `upscale --fit` and `clip --upscale --fit` plan a fit from the preset's shape.
#[test]
fn upscales_can_be_fitted_from_the_cli() {
    require_engine!("libx264", "aac", "zscale");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::FitShort, d.path(), "short.mp4");
    let up = ["--preset", "reddit-916", "--reframe", "blur", "--tier", "cpu"];
    for cmd in [vec!["upscale"], vec!["clip", "-s", "0", "-e", "2", "--upscale"]] {
        let o = rmedia(d.path())
            .args(&cmd)
            .args(["--dry-run", "--json", "--fit", "reddit-video", "--fit-max-size", "1MB"])
            .args(up)
            .arg(&src)
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(0), "{cmd:?}: {}", stderr(&o));
        let doc = plan_doc(&o);
        let fit = &doc["jobs"][0]["fit"];
        assert_eq!(
            (fit["cap"].as_u64(), fit["size"][1].as_u64()),
            (Some(1_000_000), Some(1920)),
            "{cmd:?}: {doc}"
        );
    }
    // A real run: the result line shows the size against the cap.
    let o = rmedia(d.path())
        .args(["upscale", "--fit", "reddit-video", "--fit-max-size", "1MB"])
        .args(up)
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let text = stdout(&o) + &stderr(&o);
    assert!(text.contains(" of 1.0 MB (") && text.contains("2 passes"), "{text}");
    // --fit-max-size without --fit, and an override above the target.
    let o = rmedia(d.path()).args(["upscale", "--fit-max-size", "1MB"]).arg(&src).output().unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
    let o = rmedia(d.path())
        .args(["upscale", "--fit", "reddit-video", "--fit-max-size", "2GB"])
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
}
