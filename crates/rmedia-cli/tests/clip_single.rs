//! T025: CLI contract for `rmedia clip` (exit codes, --json, --dry-run, odd paths, crash cleanup).

use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use rmedia_testkit::{Fixture, duration, fixture, require_ffmpeg};
use serde_json::Value;

fn rmedia(state: &Path) -> Command {
    let mut c = Command::new(cargo_bin("rmedia"));
    c.env("RMEDIA_STATE_DIR", state);
    c
}

fn copy_fixture(kind: Fixture, dir: &Path, name: &str) -> PathBuf {
    let dst = dir.join(name);
    std::fs::copy(fixture(kind), &dst).unwrap();
    dst
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

#[test]
fn clips_with_odd_paths_and_reports_json() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    let src = copy_fixture(Fixture::Gop50Mp4, d.path(), "-dash ü b.mp4");
    let out = d.path().join("out dir");
    let o = rmedia(d.path())
        .args(["clip", "-s", "2", "-e", "0:04", "--mode", "accurate", "--json", "-o"])
        .arg(&out)
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&o.stderr));
    let lines: Vec<Value> = String::from_utf8(o.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("every stdout line is JSON"))
        .collect();
    assert!(lines.iter().all(|l| l["schema"] == 1));
    let last = lines.last().unwrap();
    assert_eq!(last["type"], "summary");
    assert_eq!(last["succeeded"], 1);
    let output = PathBuf::from(last["jobs"][0]["output"].as_str().unwrap());
    assert_eq!(output.parent().unwrap(), out);
    assert!((duration(&output) - 2.0).abs() < 0.05);
    assert_eq!(files_in(&out), ["-dash ü b_001_00-00-02.000_00-00-04.000.mp4"]);
}

#[test]
fn dry_run_writes_nothing() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    let src = copy_fixture(Fixture::Gop50Mp4, d.path(), "a.mp4");
    let o = rmedia(d.path())
        .args(["clip", "-s", "10.5", "-d", "+5", "--dry-run", "--json"])
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0));
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(doc["type"], "plan");
    assert_eq!(doc["jobs"][0]["status"], "planned");
    assert_eq!(doc["jobs"][0]["predicted"]["start"], 10.0);
    assert!(doc["jobs"][0]["argv"].as_array().unwrap().iter().any(|a| a == "copy"));
    assert!(!d.path().join("clips").exists(), "dry run must not create anything");
}

#[test]
fn exit_codes() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    let src = copy_fixture(Fixture::Gop50Mp4, d.path(), "a.mp4");
    let code = |args: &[&str]| rmedia(d.path()).args(args).output().unwrap().status.code();

    assert_eq!(code(&["clip", src.to_str().unwrap(), "-s", "1:7x"]), Some(3), "bad time");
    assert_eq!(code(&["clip", src.to_str().unwrap(), "-s", "20", "-e", "10"]), Some(3), "end before start");
    assert_eq!(
        code(&["clip", d.path().join("nope.mp4").to_str().unwrap(), "-s", "1"]),
        Some(3),
        "missing source"
    );
    assert_eq!(code(&["clip", src.to_str().unwrap(), "-s", "1", "--bogus"]), Some(2), "usage");
    assert_eq!(
        code(&["clip", src.to_str().unwrap(), "-s", "1", "--ffmpeg", "/no/ffmpeg"]),
        Some(4),
        "engine"
    );
    assert_eq!(code(&["doctor", "--ffmpeg", "/no/ffmpeg"]), Some(4));
    assert_eq!(code(&["doctor"]), Some(0));
    assert!(!d.path().join("clips").exists(), "failed validation writes nothing");
}

#[test]
fn second_run_skips_existing_output() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    let src = copy_fixture(Fixture::Gop50Mp4, d.path(), "a.mp4");
    let run = || rmedia(d.path()).args(["clip", "-s", "1", "-e", "3"]).arg(&src).output().unwrap();
    assert_eq!(run().status.code(), Some(0));
    let o = run();
    assert_eq!(o.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&o.stdout).contains("skipped"));
    assert_eq!(files_in(&d.path().join("clips")).len(), 1);
}

/// Killing rmedia with SIGKILL must take ffmpeg down too (PDEATHSIG), and the next run must
/// delete the partial file the dead run left behind (journal).
#[cfg(target_os = "linux")]
#[test]
fn sigkill_leaves_no_orphans_and_next_run_cleans_up() {
    use std::time::{Duration, Instant};
    use sysinfo_free::*;
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    let src = copy_fixture(Fixture::Hd60Mp4, d.path(), "hd.mp4");
    let mut child =
        rmedia(d.path()).args(["clip", "-s", "0", "--mode", "accurate", "-q"]).arg(&src).spawn().unwrap();
    let pid = child.id();
    // Wait for ffmpeg to start writing its .part file.
    let clips = d.path().join("clips");
    let t0 = Instant::now();
    while !files_in(&clips).iter().any(|f| f.ends_with(".part")) {
        assert!(t0.elapsed() < Duration::from_secs(20), "ffmpeg never started");
        std::thread::sleep(Duration::from_millis(50));
    }
    let engines = children_of(pid);
    assert!(!engines.is_empty(), "ffmpeg should be a child of rmedia");
    Command::new("kill").args(["-9", &pid.to_string()]).status().unwrap();
    child.wait().unwrap();
    let t0 = Instant::now();
    while engines.iter().any(|p| alive(*p)) {
        assert!(t0.elapsed() < Duration::from_secs(2), "ffmpeg outlived rmedia");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(files_in(&clips).iter().any(|f| f.ends_with(".part")), "the killed run leaves its temp file");
    // Any next run purges stale temp files first.
    let o = rmedia(d.path()).args(["clip", "-s", "1", "-e", "2", "-q"]).arg(&src).output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    assert!(
        !files_in(&clips).iter().any(|f| f.ends_with(".part")),
        "stale temp not purged: {:?}",
        files_in(&clips)
    );
}

#[cfg(target_os = "linux")]
mod sysinfo_free {
    /// PIDs whose parent is `pid` (from /proc).
    pub fn children_of(pid: u32) -> Vec<u32> {
        std::fs::read_dir("/proc")
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
            .filter(|p| {
                std::fs::read_to_string(format!("/proc/{p}/stat")).ok().and_then(|s| {
                    let after = s.rsplit_once(')')?.1;
                    after.split_whitespace().nth(1)?.parse::<u32>().ok()
                }) == Some(pid)
            })
            .collect()
    }

    pub fn alive(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|s| s.rsplit_once(')').map(|(_, rest)| !rest.trim_start().starts_with('Z')))
            .unwrap_or(false)
    }
}

/// Windows: paths longer than MAX_PATH (260) must work end to end (runs in CI on windows-latest).
#[cfg(windows)]
#[test]
fn windows_long_paths() {
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    let mut deep = d.path().to_path_buf();
    while deep.as_os_str().len() < 300 {
        deep.push("a-rather-long-folder-name-for-testing");
    }
    std::fs::create_dir_all(&deep).unwrap();
    let src = copy_fixture(Fixture::Gop50Mp4, &deep, "long path source.mp4");
    let o = rmedia(d.path()).args(["clip", "-s", "1", "-e", "3"]).arg(&src).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(files_in(&deep.join("clips")).len(), 1);
}

/// First Ctrl-C (SIGINT): graceful cancel → summary, exit 130, no partial files, no orphans.
#[cfg(target_os = "linux")]
#[test]
fn sigint_cancels_gracefully() {
    use std::time::{Duration, Instant};
    use sysinfo_free::*;
    require_ffmpeg!();
    let d = tempfile::tempdir().unwrap();
    let src = copy_fixture(Fixture::Hd60Mp4, d.path(), "hd.mp4");
    let child = rmedia(d.path())
        .args(["clip", "-s", "0", "--mode", "accurate"])
        .arg(&src)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let clips = d.path().join("clips");
    let t0 = Instant::now();
    while !files_in(&clips).iter().any(|f| f.ends_with(".part")) {
        assert!(t0.elapsed() < Duration::from_secs(20), "ffmpeg never started");
        std::thread::sleep(Duration::from_millis(50));
    }
    let engines = children_of(pid);
    Command::new("kill").args(["-INT", &pid.to_string()]).status().unwrap();
    let t0 = Instant::now();
    let out = child.wait_with_output().unwrap();
    assert!(t0.elapsed() < Duration::from_secs(3), "graceful cancel took {:?}", t0.elapsed());
    assert_eq!(out.status.code(), Some(130));
    assert!(String::from_utf8_lossy(&out.stdout).contains("1 cancelled"));
    assert!(files_in(&clips).is_empty(), "left behind: {:?}", files_in(&clips));
    assert!(engines.iter().all(|p| !alive(*p)), "ffmpeg survived");
}

/// Feature 004, T030: `--mode smart` makes a frame-exact clip and says how much was copied.
#[test]
fn smart_mode_clip() {
    if !rmedia_testkit::require_engine(&["libx264", "aac"]) {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let src = copy_fixture(Fixture::Gop50Mp4, d.path(), "in.mp4");
    let out = d.path().join("out");
    let o = rmedia(d.path())
        .args(["clip", "-s", "3.32", "-e", "28.72", "--mode", "smart", "-o"])
        .arg(&out)
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&o.stderr));
    let stdout = String::from_utf8_lossy(&o.stdout);
    // 600 of 635 frames are whole GOPs (4 s to 28 s).
    assert!(stdout.contains("(smart, 94% copied)"), "{stdout}");
    let clip = out.join("in_001_00-00-03.320_00-00-28.720.mp4");
    assert_eq!(rmedia_testkit::frame_count(&clip), 635);
}

/// Feature 004, T033 (contracts/cli.md › Exit codes): a fallback is a success; with
/// `--no-smart-fallback` a reason found while planning is invalid input (3), and one found while
/// running fails the job (1).
#[test]
fn no_smart_fallback_exit_codes() {
    if !rmedia_testkit::require_engine(&["libx264", "aac"]) {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let open_gop = copy_fixture(Fixture::OpenGop, d.path(), "open.mp4");
    let res_change = d.path().join("res.mp4");
    std::fs::copy(rmedia_testkit::res_change(), &res_change).unwrap();
    for (src, end, strict, code, says) in [
        (&open_gop, "6.72", false, 0, "(accurate (smart fallback: open_gop))"),
        (&open_gop, "6.72", true, 3, "smart cut not possible: the video has open GOPs"),
        (&res_change, "6.72", false, 0, "(accurate (smart fallback: params_changed))"),
        (&res_change, "6.72", true, 1, "smart cut not possible: the video's size or pixel format changes"),
    ] {
        let out = d.path().join(format!("out-{code}-{strict}"));
        let mut cmd = rmedia(d.path());
        cmd.args(["clip", "-s", "1.32", "-e", end, "--mode", "smart", "-o"]).arg(&out).arg(src);
        if strict {
            cmd.arg("--no-smart-fallback");
        }
        let o = cmd.output().unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        assert_eq!(o.status.code(), Some(code), "{}: {text}", src.display());
        assert!(text.contains(says), "{}: {text}", src.display());
        if code != 0 {
            assert!(files_in(&out).is_empty(), "{:?}", files_in(&out));
        }
    }
}
