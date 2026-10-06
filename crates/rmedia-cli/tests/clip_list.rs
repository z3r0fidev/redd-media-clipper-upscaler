//! T041: `rmedia clip -l` contract.

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
fn malformed_list_reports_lines_and_writes_nothing() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("v.mp4");
    std::fs::copy(fixture(Fixture::Gop50Mp4), &src).unwrap();
    let list = d.path().join("clips.txt");
    std::fs::write(&list, "0:01 0:02 ok\n0:05\n0:01 0:1x\n").unwrap();
    let o = rmedia(d.path()).arg("clip").arg(&src).arg("-l").arg(&list).arg("--json").output().unwrap();
    assert_eq!(o.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(stderr.contains("clips.txt:2:") && stderr.contains("clips.txt:3:"), "{stderr}");
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    let lines: Vec<u64> =
        doc["issues"].as_array().unwrap().iter().map(|i| i["line"].as_u64().unwrap()).collect();
    assert_eq!(lines, [2, 3]);
    assert!(!d.path().join("clips").exists());
}

#[test]
fn list_applies_to_every_input() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("a.mp4");
    let b = d.path().join("b.mp4");
    std::fs::copy(fixture(Fixture::Gop50Mp4), &a).unwrap();
    std::fs::copy(fixture(Fixture::Gop50Mp4), &b).unwrap();
    let list = d.path().join("clips.txt");
    std::fs::write(&list, "0:01 0:02 x\n0:03 +1 y\n").unwrap();
    let out = d.path().join("out");
    let o = rmedia(d.path())
        .args(["clip", "-j", "3", "-q", "-o"])
        .arg(&out)
        .arg(&a)
        .arg(&b)
        .arg("-l")
        .arg(&list)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("4 succeeded"));
    for f in ["a/a_001_x.mp4", "a/a_002_y.mp4", "b/b_001_x.mp4", "b/b_002_y.mp4"] {
        assert!(out.join(f).is_file(), "missing {f}");
    }
}

#[test]
fn list_and_start_conflict() {
    let d = tempfile::tempdir().unwrap();
    let o = rmedia(d.path()).args(["clip", "x.mp4", "-l", "c.txt", "-s", "1"]).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    let o = rmedia(d.path()).args(["clip", "x.mp4"]).output().unwrap();
    assert_eq!(o.status.code(), Some(2), "either -s or -l is required");
}
