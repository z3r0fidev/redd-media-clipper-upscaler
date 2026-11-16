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

/// Feature 004, T040 (US3): a dry run of five smart clips, three eligible and two falling back,
/// shows the parts, the shares and the reasons, and writes nothing (contracts/cli.md).
#[test]
fn smart_dry_run() {
    if !rmedia_testkit::require_engine(&["libx264", "aac"]) {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    for (kind, name) in
        [(Fixture::SmartMp4, "a.mp4"), (Fixture::OpenGop, "b.mp4"), (Fixture::VfrMp4, "c.mp4")]
    {
        std::fs::copy(fixture(kind), d.path().join(name)).unwrap();
    }
    let m = d.path().join("jobs.json");
    std::fs::write(
        &m,
        r#"{"version":1,"videos":[
            {"input":"a.mp4","clips":[{"start":3.32,"end":20.72},{"start":4,"end":20},{"start":3.32,"end":8.72}]},
            {"input":"b.mp4","clips":[{"start":1.32,"end":6.72}]},
            {"input":"c.mp4","clips":[{"start":2,"end":12}]}]}"#,
    )
    .unwrap();
    let batch = |extra: &[&str]| {
        rmedia(d.path())
            .arg("batch")
            .arg(&m)
            .args(["--mode", "smart", "--dry-run"])
            .args(extra)
            .output()
            .unwrap()
    };

    let o = batch(&[]);
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let stdout = String::from_utf8_lossy(&o.stdout);
    for line in [
        "smart: head 0.680 s (17 fr) re-encoded · copy 16.000 s (400 fr) · tail 0.720 s (18 fr) re-encoded · 92% copied",
        "smart: copy 16.000 s (400 fr) · 100% copied",
        "smart: head 0.680 s (17 fr) re-encoded · copy 4.000 s (100 fr) · tail 0.720 s (18 fr) re-encoded · 74% copied",
        "smart → accurate: the video has open GOPs (frames that depend on the previous GOP)",
        "smart → accurate: the video has a variable frame rate",
        "smart: 3 jobs; fallback: 2 (open_gop 1, vfr 1)",
    ] {
        assert!(stdout.lines().any(|l| l.trim() == line), "missing {line:?} in:\n{stdout}");
    }

    let o = batch(&["--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        doc["smart"],
        serde_json::json!({"smart": 3, "fallback": {"open_gop": 1, "vfr": 1}, "not_applied": 0})
    );
    let job = |input: &str| {
        doc["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["source"].as_str().unwrap().ends_with(input))
            .unwrap()
    };
    assert_eq!(job("a.mp4")["smart"]["mode"], "smart");
    assert_eq!(job("a.mp4")["pipeline"].as_array().map(Vec::len), Some(5), "head, copy, tail, audio, join");
    assert_eq!(
        job("b.mp4")["smart"],
        serde_json::json!({"requested": true, "mode": "accurate", "strict": false, "head": null,
            "copy": null, "tail": null, "copied_share": null, "fallback": "open_gop"})
    );
    assert_eq!(job("b.mp4")["mode"], "accurate");
    assert_eq!(job("c.mp4")["smart"]["fallback"], "vfr");

    // Strict mode: the two fallbacks are planning errors, each with its reason.
    let o = batch(&["--json", "--no-smart-fallback"]);
    assert_eq!(o.status.code(), Some(3), "{}", String::from_utf8_lossy(&o.stderr));
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    let reasons: Vec<&str> = doc["issues"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["code"] == "SMART_UNAVAILABLE")
        .map(|i| i["detail"]["reason"].as_str().unwrap())
        .collect();
    assert_eq!(reasons, ["open_gop", "vfr"]);

    // Nothing written: no clips folder and no scratch folders.
    let names: Vec<String> = walk(d.path()).iter().map(|p| p.to_string_lossy().into_owned()).collect();
    assert!(!d.path().join("clips").exists() && names.iter().all(|n| !n.contains(".rmedia-")), "{names:?}");
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        }
        out.push(p);
    }
    out
}
