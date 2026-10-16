//! T040 (002 US1): CLI contract for `rmedia upscale` — dry-run JSON (contracts/json-output.md),
//! `--list-presets`, exit codes, and dry runs that write nothing.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use assert_cmd::cargo::cargo_bin;
use rmedia_testkit::{Fixture, fixture, require_engine};
use serde_json::Value;

/// `rmedia` with state and config isolated in `dir` (no user presets leak in).
fn rmedia(dir: &Path) -> Command {
    let mut c = Command::new(cargo_bin("rmedia"));
    c.env("RMEDIA_STATE_DIR", dir.join("state"))
        .env("RMEDIA_CONFIG_DIR", dir.join("config"))
        .env("RMEDIA_CACHE_DIR", dir.join("cache"));
    c
}

fn copy(kind: Fixture, dir: &Path, name: &str) -> PathBuf {
    let dst = dir.join(name);
    std::fs::copy(fixture(kind), &dst).unwrap();
    dst
}

fn json(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&o.stdout)))
}

/// Everything under `dir` except the given names.
fn entries(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !matches!(n.as_str(), "state" | "config" | "cache"))
        .collect();
    v.sort();
    v
}

#[test]
fn dry_run_json_matches_the_contract_and_writes_nothing() {
    require_engine!("libx265", "zscale", "cas", "gblur");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::DegradedPortrait, d.path(), "low.mp4");
    let o = rmedia(d.path()).args(["upscale", "--dry-run", "--json"]).arg(&src).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let doc = json(&o);
    assert_eq!((doc["schema"].as_u64(), doc["type"].as_str()), (Some(1), Some("plan")));
    let job = &doc["jobs"][0];
    assert_eq!(job["kind"], "upscale");
    assert!(job.get("mode").is_none() && job.get("predicted").is_none(), "{job}");
    assert_eq!(
        job["output"].as_str().unwrap(),
        d.path().join("upscaled/low_phone-fill.mp4").to_str().unwrap()
    );
    assert_eq!(job["requested"]["start"], 0.0);
    let up = &job["upscale"];
    assert_eq!(up["preset"], "phone-fill");
    assert_eq!(up["size"], serde_json::json!([1080, 2340]));
    assert_eq!(
        (up["reframe"].as_str(), up["tier"].as_str(), up["hdr"].as_str()),
        (Some("smart"), Some("cpu"), Some("sdr"))
    );
    assert!(up["tier_reason"].as_str().is_some_and(|r| r.starts_with("auto")));
    // The test pattern has no faces: the whole clip falls back, reported as blur.
    assert_eq!(up["reframe_applied"], "blur");
    assert!(up["suggest"].is_null());
    assert_eq!(up["no_upscale_needed"], false);
    assert!(up["estimate"]["bytes"].as_u64().unwrap() > 0 && up["estimate"]["secs"].as_f64().unwrap() > 0.0);
    assert_eq!(up["reframe_analysis"]["status"], "ready", "a dry run analyses so the crop path can be shown");
    assert!(job["cwd"].as_str().is_some_and(|c| c.ends_with(".d")), "{job}");
    let argv: Vec<&str> = job["argv"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
    assert!(argv.contains(&"-filter_complex") && argv.contains(&"hvc1"), "{argv:?}");
    assert_eq!(entries(d.path()), ["low.mp4"], "a dry run creates no upscaled/ folder, .part or scratch dir");
}

#[test]
fn list_presets_json_and_user_presets() {
    let d = tempfile::tempdir().unwrap();
    let presets = d.path().join("config/presets");
    std::fs::create_dir_all(&presets).unwrap();
    std::fs::write(presets.join("broken.json"), "{").unwrap();
    let o = rmedia(d.path()).args(["upscale", "--list-presets", "--json"]).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "needs no engine and no inputs");
    let doc = json(&o);
    assert_eq!((doc["type"].as_str(), doc["default"].as_str()), (Some("presets"), Some("phone-fill")));
    let ids: Vec<&str> =
        doc["presets"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        ["phone-fill", "phone-fill-hd", "pixel-tall", "reddit-916", "reddit-feed-45", "fold-inner", "duo"]
    );
    let reddit = &doc["presets"][3];
    assert_eq!((reddit["codec"].as_str(), reddit["source"].as_str()), (Some("h264"), Some("builtin")));
    assert_eq!(reddit["limits"]["max_bytes"], 1_073_741_824u64);
    assert!(doc["problems"][0]["file"].as_str().unwrap().ends_with("broken.json"));
}

#[test]
fn invalid_requests_exit_3_and_unknown_flags_exit_2() {
    require_engine!("libx265", "zscale", "cas");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::DegradedPortrait, d.path(), "low.mp4");
    let run = |args: &[&str]| rmedia(d.path()).args(args).arg(&src).output().unwrap();
    for (args, code) in [
        (&["upscale", "--json", "--preset", "nokia-3310"][..], "PRESET_UNKNOWN"),
        (&["upscale", "--json", "--preset", "reddit-916", "--hdr", "keep"][..], "HDR_UNSUPPORTED"),
        (&["upscale", "--json", "--tier", "gpu"][..], "TIER_UNAVAILABLE"),
    ] {
        let o = run(args);
        assert_eq!(o.status.code(), Some(3), "{args:?}");
        assert_eq!(json(&o)["issues"][0]["code"], code, "{args:?}");
    }
    assert_eq!(
        run(&["upscale", "--mode", "fast"]).status.code(),
        Some(2),
        "--mode does not apply to upscaling"
    );
    assert_eq!(run(&["upscale", "--crf", "60"]).status.code(), Some(2));
    assert_eq!(entries(d.path()), ["low.mp4"]);
}

#[test]
fn smart_reframe_dry_run_block_cache_no_analyze_and_crop_path() {
    require_engine!("libx265", "zscale", "cas", "sendcmd", "scdet");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::MovingMarker, d.path(), "land.mp4");
    let dry = |extra: &[&str]| {
        let o =
            rmedia(d.path()).args(["upscale", "--dry-run", "--json"]).args(extra).arg(&src).output().unwrap();
        assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
        json(&o)["jobs"][0]["upscale"]["reframe_analysis"].clone()
    };
    let pending = dry(&["--no-analyze"]);
    assert_eq!(pending["status"], "pending");
    let first = dry(&[]);
    assert_eq!((first["status"].as_str(), first["cache"].as_str()), (Some("ready"), Some("miss")));
    assert_eq!(first["crop"], serde_json::json!([332, 720]));
    assert_eq!(first["axis"], "x");
    let shots = first["shots"].as_array().unwrap();
    assert_eq!(shots.len(), 2, "{shots:?}");
    assert!((shots[1]["start"].as_f64().unwrap() - 6.0).abs() < 0.05);
    assert_eq!(first["path"]["hz"], 10);
    assert_eq!(first["path"]["keyframes"].as_array().unwrap().len(), 101, "10 s at 10 Hz");
    assert_eq!(dry(&[])["cache"], "hit", "the second dry run reuses the analysis");

    let path = d.path().join("path.json");
    let o =
        rmedia(d.path()).args(["upscale", "--dry-run", "--crop-path"]).arg(&path).arg(&src).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let doc: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!((doc["schema"].as_u64(), doc["fps"].as_f64()), (Some(1), Some(30.0)));
    assert_eq!(doc["x"].as_array().unwrap().len(), 300);
    assert!(doc["x"].as_array().unwrap().iter().all(|x| x.as_i64().unwrap() % 2 == 0));

    let centre = rmedia(d.path())
        .args(["upscale", "--dry-run", "--reframe", "center", "--crop-path"])
        .arg(&path)
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(centre.status.code(), Some(2), "--crop-path needs a smart-reframe job");
    let onto_source =
        rmedia(d.path()).args(["upscale", "--dry-run", "--crop-path"]).arg(&src).arg(&src).output().unwrap();
    assert_eq!(onto_source.status.code(), Some(3), "--crop-path never overwrites a source");
    assert!(src.metadata().unwrap().len() > 10_000, "the source is intact");
}
