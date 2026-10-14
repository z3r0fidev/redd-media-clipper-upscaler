//! T007 (002): pins the 001 clip JSON contract (dry-run job document and run NDJSON) before the
//! job-model refactor, so any change to it is visible in review. Paths, run ids and timings are
//! redacted; everything else is byte-for-byte.

use std::path::Path;
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use rmedia_testkit::{Fixture, fixture, require_ffmpeg};
use serde_json::Value;

/// Keys whose values vary between runs.
const VOLATILE: &[&str] = &["run_id", "eta_ms", "speed", "elapsed_ms", "out_time"];

fn redact(v: &mut Value, dir: &str) {
    match v {
        Value::String(s) => {
            if s.contains(dir) {
                *s = s.replace(dir, "[DIR]");
            }
            // Temp names carry the run id: .<stem>.rmedia-<run>-<job>.part
            if let Some(i) = s.find(".rmedia-") {
                let rest = &s[i + 8..];
                if let Some(j) = rest.find('-') {
                    *s = format!("{}.rmedia-[RUN]{}", &s[..i], &rest[j..]);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| redact(x, dir)),
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                if VOLATILE.contains(&k.as_str()) {
                    *x = Value::String("[REDACTED]".into());
                } else {
                    redact(x, dir);
                }
            }
        }
        _ => {}
    }
}

fn run(dir: &Path, args: &[&str]) -> Vec<Value> {
    let src = dir.join("in.mp4");
    std::fs::copy(fixture(Fixture::Gop50Mp4), &src).unwrap();
    let o = Command::new(cargo_bin("rmedia"))
        .env("RMEDIA_STATE_DIR", dir)
        .args(args)
        .arg("-o")
        .arg(dir.join("out"))
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&o.stderr));
    let d = dir.to_string_lossy().into_owned();
    String::from_utf8(o.stdout)
        .unwrap()
        .lines()
        .map(|l| {
            let mut v: Value = serde_json::from_str(l).unwrap();
            redact(&mut v, &d);
            v
        })
        .collect()
}

#[test]
fn clip_dry_run_job_document() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    // Fast mode at a non-keyframe start, so `predicted` is present.
    let docs = run(d.path(), &["clip", "-s", "3", "-e", "7", "--dry-run", "--json"]);
    insta::assert_json_snapshot!(docs);
}

#[test]
fn clip_run_outcome_events() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    let docs = run(d.path(), &["clip", "-s", "2", "-e", "4", "--mode", "accurate", "--json"]);
    // Progress events vary in count; the contract is in the outcome and the summary.
    let kept: Vec<Value> =
        docs.into_iter().filter(|v| matches!(v["type"].as_str(), Some("job_finished" | "summary"))).collect();
    insta::assert_json_snapshot!(kept);
}
