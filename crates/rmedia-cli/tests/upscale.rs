//! T040 (002 US1): CLI contract for `rmedia upscale` — dry-run JSON (contracts/json-output.md),
//! `--list-presets`, exit codes, and dry runs that write nothing.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use assert_cmd::cargo::cargo_bin;
use rmedia_testkit::{Fixture, fixture, require_engine};
use serde_json::Value;

/// `rmedia` with state and config isolated in `dir` (no user presets leak in), and the Vulkan
/// loader pointed at a driver file that does not exist, so the tier is the CPU on every machine
/// (the GPU tier is tested in `doctor.rs` and in rmedia-core's `us3_gpu.rs`).
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

fn outputs(doc: &Value) -> Vec<String> {
    doc["jobs"].as_array().unwrap().iter().map(|j| j["output"].as_str().unwrap().to_owned()).collect()
}

fn sources(doc: &Value) -> Vec<String> {
    let mut v: Vec<String> = doc["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| Path::new(j["source"].as_str().unwrap()).file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// T077: `upscale DIR` scans with 001's discovery and never picks up its own outputs.
#[test]
fn upscale_folder_and_inputs_from_skip_output_folders() {
    require_engine!("libx265", "zscale", "cas", "gblur");
    let d = tempfile::tempdir().unwrap();
    let videos = d.path().join("videos");
    std::fs::create_dir_all(videos.join("upscaled")).unwrap();
    std::fs::create_dir_all(videos.join("mine")).unwrap();
    copy(Fixture::TinySource, &videos, "a.mp4");
    copy(Fixture::TinySource, &videos.join("upscaled"), "a_phone-fill.mp4");
    copy(Fixture::TinySource, &videos.join("mine"), "b.mp4");
    std::fs::write(videos.join("notes.txt"), "not a video").unwrap();
    let run = |extra: &[&str]| {
        let o = rmedia(d.path())
            .args(["upscale", "--dry-run", "--json", "--reframe", "center"])
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
        json(&o)
    };
    let dir = videos.to_str().unwrap();
    assert_eq!(sources(&run(&[dir])), ["a.mp4", "b.mp4"], "upscaled/ is skipped");
    // With -o, that folder is skipped too; default-named folders always are (`clips/` as well).
    std::fs::create_dir_all(videos.join("clips")).unwrap();
    copy(Fixture::TinySource, &videos.join("clips"), "a_001_x.mp4");
    let doc = run(&[dir, "-o", videos.join("mine").to_str().unwrap()]);
    assert_eq!(sources(&doc), ["a.mp4"]);
    assert_eq!(sources(&run(&[dir])), ["a.mp4", "b.mp4"]);
    // A list of paths, relative to the list file.
    std::fs::write(d.path().join("list.txt"), "videos/a.mp4\n# comment\nvideos/mine/b.mp4\n").unwrap();
    let doc = run(&["--inputs-from", d.path().join("list.txt").to_str().unwrap()]);
    assert_eq!(sources(&doc), ["a.mp4", "b.mp4"]);
    assert!(outputs(&doc)[0].ends_with("videos/upscaled/a_phone-fill.mp4"), "{:?}", outputs(&doc));
}

/// T077: `clip … --upscale` cuts and upscales in one job; upscale flags need `--upscale`.
#[test]
fn clip_upscale_uses_the_preset_and_clip_names() {
    require_engine!("libx265", "zscale", "cas", "gblur");
    let d = tempfile::tempdir().unwrap();
    let land = copy(Fixture::MovingMarker, d.path(), "land.mp4");
    let o = rmedia(d.path())
        .arg("clip")
        .arg(&land)
        .args(["-s", "0", "-e", "4", "--upscale", "--preset", "reddit-feed-45", "--dry-run", "--json"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let doc = json(&o);
    let job = &doc["jobs"][0];
    assert_eq!(job["kind"], "upscale");
    assert_eq!(job["upscale"]["preset"], "reddit-feed-45");
    assert_eq!(job["upscale"]["size"], serde_json::json!([1080, 1350]));
    let want = d.path().join("clips/land_001_00-00-00.000_00-00-04.000_reddit-feed-45.mp4");
    assert_eq!(Path::new(job["output"].as_str().unwrap()), want);
    assert_eq!(entries(d.path()), ["land.mp4"], "a dry run writes nothing");

    // Upscale flags without --upscale are a usage error.
    let o = rmedia(d.path())
        .arg("clip")
        .arg(&land)
        .args(["-s", "0", "--preset", "duo", "--dry-run"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("--preset needs --upscale"));
    // Many sources into one folder: the clips keep their per-source sub-folder.
    let b = copy(Fixture::TinySource, d.path(), "b.mp4");
    let o = rmedia(d.path())
        .arg("clip")
        .args([&land, &b])
        .args(["-s", "0", "-e", "1", "--upscale", "--reframe", "center", "-o"])
        .arg(d.path().join("out"))
        .args(["--dry-run", "--json"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let outs = outputs(&json(&o));
    assert!(outs[0].contains("/out/land/land_001_") && outs[0].ends_with("_phone-fill.mp4"), "{outs:?}");
}

/// T077: `batch` upscale flags fill what the manifest leaves open and never turn upscaling on;
/// `batch --upscale` turns it on for every video.
#[test]
fn batch_upscale_flags_are_defaults_under_the_manifest() {
    require_engine!("libx265", "zscale", "cas", "gblur");
    let d = tempfile::tempdir().unwrap();
    copy(Fixture::TinySource, d.path(), "a.mp4");
    copy(Fixture::TinySource, d.path(), "b.mp4");
    copy(Fixture::TinySource, d.path(), "c.mp4");
    let m = d.path().join("m.json");
    std::fs::write(
        &m,
        r#"{"version":1,"videos":[
            {"input":"a.mp4","options":{"upscale":{}}},
            {"input":"b.mp4","options":{"upscale":{"preset":"reddit-916"}}},
            {"input":"c.mp4","clips":[{"start":0,"end":1}]}]}"#,
    )
    .unwrap();
    let run = |extra: &[&str]| {
        let o =
            rmedia(d.path()).arg("batch").arg(&m).args(["--dry-run", "--json"]).args(extra).output().unwrap();
        assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
        json(&o)
    };
    let preset = |doc: &Value, i: usize| doc["jobs"][i]["upscale"]["preset"].as_str().map(str::to_owned);
    let doc = run(&["--preset", "duo", "--reframe", "center"]);
    assert_eq!(preset(&doc, 0).as_deref(), Some("duo"), "the flag fills a manifest `{{}}`");
    assert_eq!(preset(&doc, 1).as_deref(), Some("reddit-916"), "the manifest's preset wins");
    assert!(
        doc["jobs"][2].get("upscale").is_none() && doc["jobs"][2]["mode"].is_string(),
        "not turned on: {}",
        doc["jobs"][2]
    );
    let doc = run(&["--upscale", "--preset", "duo", "--reframe", "center"]);
    assert_eq!(preset(&doc, 2).as_deref(), Some("duo"), "--upscale turns it on everywhere");
    assert!(
        doc["jobs"][2]["output"].as_str().unwrap().ends_with("clips/c_001_00-00-00.000_00-00-01.000_duo.mp4"),
        "{}",
        doc["jobs"][2]["output"]
    );
    assert_eq!(entries(d.path()), ["a.mp4", "b.mp4", "c.mp4", "m.json"]);

    // The flags also fill a layer that turns upscaling back on below an `enabled: false`; a named
    // clip of the whole video keeps its name; crf out of range is an input error.
    std::fs::write(
        &m,
        r#"{"version":1,"videos":[
            {"input":"a.mp4","options":{"upscale":{"enabled":false}},
             "clips":[{"start":0,"name":"full","options":{"upscale":{}}}]}]}"#,
    )
    .unwrap();
    let doc = run(&["--upscale", "--preset", "duo", "--reframe", "center"]);
    assert_eq!(preset(&doc, 0).as_deref(), Some("duo"));
    assert!(
        doc["jobs"][0]["output"].as_str().unwrap().ends_with("clips/a_001_full_duo.mp4"),
        "{}",
        doc["jobs"][0]["output"]
    );
    std::fs::write(&m, r#"{"version":1,"videos":[{"input":"a.mp4","options":{"upscale":{"crf":60}}}]}"#)
        .unwrap();
    let o = rmedia(d.path()).arg("batch").arg(&m).args(["--dry-run"]).output().unwrap();
    assert_eq!(o.status.code(), Some(3), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stderr).contains("crf 60 is out of range"));
    // Flags that nothing uses are reported.
    std::fs::write(&m, r#"{"version":1,"videos":[{"input":"a.mp4","clips":[{"start":0,"end":1}]}]}"#)
        .unwrap();
    let o = rmedia(d.path()).arg("batch").arg(&m).args(["--dry-run", "--tier", "cpu"]).output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("--tier had no effect"),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}
