//! `rmedia batch` CLI contract.

use std::path::Path;
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use rmedia_testkit::{Fixture, fixture, require_ffmpeg};
use serde_json::Value;

fn rmedia(state: &Path) -> Command {
    let mut c = Command::new(cargo_bin("rmedia"));
    c.env("RMEDIA_STATE_DIR", state);
    c
}

#[test]
fn batch_csv_runs_and_continue_on_missing() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    std::fs::copy(fixture(Fixture::Gop50Mp4), d.path().join("a.mp4")).unwrap();
    let m = d.path().join("jobs.csv");
    std::fs::write(&m, "input,start,end,name\na.mp4,1,2,x\nmissing.mp4,1,2,y\n").unwrap();

    let o = rmedia(d.path()).arg("batch").arg(&m).output().unwrap();
    assert_eq!(o.status.code(), Some(3), "missing input stops the batch by default");
    assert!(String::from_utf8_lossy(&o.stderr).contains("jobs.csv:3"));

    let o = rmedia(d.path()).arg("batch").arg(&m).args(["--continue-on-missing", "--json"]).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let last: Value =
        serde_json::from_str(String::from_utf8_lossy(&o.stdout).lines().last().unwrap()).unwrap();
    assert_eq!((last["type"].as_str(), last["succeeded"].as_u64()), (Some("summary"), Some(1)));
    assert!(d.path().join("clips/a_001_x.mp4").is_file());
}

#[test]
fn batch_dry_run_and_cli_defaults() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    std::fs::copy(fixture(Fixture::Gop50Mp4), d.path().join("a.mp4")).unwrap();
    let m = d.path().join("jobs.json");
    std::fs::write(
        &m,
        r#"{"version":1,"videos":[{"input":"a.mp4","clips":[{"start":1,"end":2},{"start":3,"end":4,"options":{"mode":"fast"}}]}]}"#,
    )
    .unwrap();
    let o = rmedia(d.path())
        .arg("batch")
        .arg(&m)
        .args(["--mode", "accurate", "--dry-run", "--json"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0));
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(doc["jobs"][0]["mode"], "accurate", "CLI flag fills unspecified mode");
    assert_eq!(doc["jobs"][1]["mode"], "fast", "manifest value wins");
    assert!(!d.path().join("clips").exists());

    let bad = d.path().join("bad.json");
    std::fs::write(
        &bad,
        "{\"version\":1,\n\"videos\":[{\"input\":\"a.mp4\",\"clips\":[{\"start\":\"x\"}]}]}",
    )
    .unwrap();
    let o = rmedia(d.path()).arg("batch").arg(&bad).output().unwrap();
    assert_eq!(o.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&o.stderr).contains("videos[0].clips[0].start"));
    let o = rmedia(d.path()).args(["batch", "jobs.yaml"]).output().unwrap();
    assert_eq!(o.status.code(), Some(3));
}
