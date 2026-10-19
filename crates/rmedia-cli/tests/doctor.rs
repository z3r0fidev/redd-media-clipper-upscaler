//! T068 (002 US3): `rmedia doctor` reports the tiers, GPUs and models, caches the capability
//! check, runs it again on `--recheck`, and a forced GPU tier with no Vulkan driver exits 3.

use std::path::Path;
use std::process::{Command, Output};

use assert_cmd::cargo::cargo_bin;
use rmedia_testkit::{Fixture, fixture, require_engine};
use serde_json::Value;

/// `rmedia` with its state in `dir`; `no_gpu` hides every Vulkan driver.
fn rmedia(dir: &Path, no_gpu: bool) -> Command {
    let mut c = Command::new(cargo_bin("rmedia"));
    c.env("RMEDIA_STATE_DIR", dir.join("state")).env("RMEDIA_CONFIG_DIR", dir.join("config"));
    if no_gpu {
        let none = dir.join("no-driver.json");
        c.env("VK_DRIVER_FILES", &none).env("VK_ICD_FILENAMES", &none);
    }
    c
}

fn json(o: &Output) -> Value {
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_slice(&o.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&o.stdout)))
}

#[test]
fn doctor_json_reports_tiers_and_caches_the_check() {
    require_engine!("libx265", "zscale", "cas");
    let d = tempfile::tempdir().unwrap();
    let doc = json(&rmedia(d.path(), true).args(["doctor", "--json"]).output().unwrap());
    for key in ["tiers", "gpus", "models", "capability", "preset_problems"] {
        assert!(doc.get(key).is_some(), "doctor --json lacks {key}: {doc}");
    }
    let tiers = &doc["tiers"];
    assert_eq!(tiers["cpu"]["available"], true);
    assert!(tiers["cpu"]["fps"].as_f64().is_some_and(|f| f > 0.0), "{tiers}");
    assert_eq!(tiers["gpu"]["available"], false);
    assert_eq!(tiers["gpu"]["reason"], "no Vulkan device");
    assert_eq!(tiers["ai"]["available"], false);
    assert_eq!(doc["ok"], true, "a missing GPU is not a problem");
    assert!(doc["capability"]["checked_at"].as_str().is_some_and(|c| c.ends_with('Z')), "{doc}");
    // Mark the stored record as old: an unchanged machine reuses it as is, --recheck replaces it.
    let file = d.path().join("state/capability.json");
    let mut stored: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    stored["checked_at"] = "2000-01-01T00:00:00Z".into();
    std::fs::write(&file, serde_json::to_vec(&stored).unwrap()).unwrap();
    let again = json(&rmedia(d.path(), true).args(["doctor", "--json"]).output().unwrap());
    assert_eq!(again["capability"]["checked_at"], "2000-01-01T00:00:00Z", "the stored record is reused");
    let fresh = json(&rmedia(d.path(), true).args(["doctor", "--json", "--recheck"]).output().unwrap());
    assert_ne!(fresh["capability"]["checked_at"], "2000-01-01T00:00:00Z");

    let text = rmedia(d.path(), true).arg("doctor").output().unwrap();
    let text = String::from_utf8_lossy(&text.stdout);
    for line in ["tier:     cpu  yes", "tier:     gpu  no (no Vulkan device)", "checked:"] {
        assert!(text.contains(line), "missing {line:?} in:\n{text}");
    }
}

#[test]
fn a_forced_gpu_with_no_vulkan_driver_exits_3() {
    require_engine!("libx265", "zscale", "cas");
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("low.mp4");
    std::fs::copy(fixture(Fixture::DegradedPortrait), &src).unwrap();
    let o = rmedia(d.path(), true)
        .args(["upscale", "--tier", "gpu", "--dry-run", "--json"])
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(3), "{}", String::from_utf8_lossy(&o.stderr));
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(doc["issues"][0]["code"], "TIER_UNAVAILABLE");
    assert!(doc["issues"][0]["message"].as_str().unwrap().contains("no Vulkan device"), "{doc}");
    let auto =
        json(&rmedia(d.path(), true).args(["upscale", "--dry-run", "--json"]).arg(&src).output().unwrap());
    assert_eq!(auto["jobs"][0]["upscale"]["tier"], "cpu");
    assert!(auto["jobs"][0]["upscale"]["tier_reason"].as_str().unwrap().contains("no Vulkan device"));
}
