//! Core warnings (`tracing`) reach stderr: shown by default, hidden by `-q` or `RMEDIA_LOG=error`,
//! and never mixed into `--json` stdout.

use std::path::Path;
use std::process::{Command, Output};

use assert_cmd::cargo::cargo_bin;
use rmedia_testkit::{Fixture, fixture, require_ffmpeg};
use serde_json::Value;

const WARNING: &str = "warning: crash journal unavailable";

/// A one-second clip whose state "folder" is a file, so the crash journal cannot open and the
/// core logs a warning.
fn clip(dir: &Path, extra: &[&str], env: &[(&str, &str)]) -> Output {
    let state = dir.join("state-is-a-file");
    std::fs::write(&state, b"").unwrap();
    let src = dir.join("in.mp4");
    if !src.exists() {
        std::fs::copy(fixture(Fixture::Gop50Mp4), &src).unwrap();
    }
    let out = dir.join(format!("out{}", extra.len() + env.len()));
    let mut c = Command::new(cargo_bin("rmedia"));
    c.env("RMEDIA_STATE_DIR", &state).env_remove("RMEDIA_LOG");
    for (k, v) in env {
        c.env(k, v);
    }
    let o = c
        .args(extra)
        .args(["clip", "-s", "1", "-d", "1", "--json", "-o"])
        .arg(&out)
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&o.stderr));
    for line in String::from_utf8(o.stdout.clone()).unwrap().lines() {
        let _: Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: stdout line {line:?}"));
    }
    o
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn core_warnings_reach_stderr_unless_quiet() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();

    let shown = clip(d.path(), &[], &[]);
    assert!(stderr(&shown).contains(WARNING), "stderr: {}", stderr(&shown));

    let quiet = clip(d.path(), &["-q"], &[]);
    assert!(!stderr(&quiet).contains("crash journal"), "stderr: {}", stderr(&quiet));

    let filtered = clip(d.path(), &[], &[("RMEDIA_LOG", "error")]);
    assert!(!stderr(&filtered).contains("crash journal"), "stderr: {}", stderr(&filtered));
}
