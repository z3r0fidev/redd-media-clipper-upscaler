//! T046: `rmedia scan` and folder / --inputs-from inputs for `rmedia clip`.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use assert_cmd::cargo::cargo_bin;
use rmedia_testkit::{Fixture, fixture, require_ffmpeg};
use serde_json::Value;

fn rmedia(state: &Path) -> Command {
    let mut c = Command::new(cargo_bin("rmedia"));
    c.env("RMEDIA_STATE_DIR", state);
    c
}

fn library() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("lib/season 1")).unwrap();
    std::fs::copy(fixture(Fixture::Gop50Mp4), d.path().join("lib/ep2.mp4")).unwrap();
    std::fs::copy(fixture(Fixture::Gop50Mp4), d.path().join("lib/season 1/ep10.mp4")).unwrap();
    std::fs::copy(fixture(Fixture::FakeMp4), d.path().join("lib/broken.mp4")).unwrap();
    std::fs::write(d.path().join("lib/readme.txt"), "x").unwrap();
    d
}

#[test]
fn scan_json_matches_contract() {
    require_ffmpeg!();
    let d = library();
    let o = rmedia(d.path()).args(["scan", "--json"]).arg(d.path().join("lib")).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!((doc["schema"].as_u64(), doc["type"].as_str()), (Some(1), Some("scan")));
    let videos = doc["videos"].as_array().unwrap();
    assert_eq!(videos.len(), 2);
    let v = &videos[0];
    for key in [
        "path",
        "size",
        "duration",
        "width",
        "height",
        "fps",
        "video_codec",
        "container",
        "audio_streams",
        "subtitle_streams",
    ] {
        assert!(!v[key].is_null(), "missing {key}");
    }
    assert_eq!(v["fps"], 25.0);
    assert_eq!(doc["rejected"][0]["reason"], "unreadable");
    assert!(doc["skipped"].is_object() && doc["elapsed_ms"].is_u64());
}

#[test]
fn scan_paths_feed_clip_inputs_from_stdin() {
    require_ffmpeg!();
    let d = library();
    let o = rmedia(d.path()).args(["scan", "--paths"]).arg(d.path().join("lib")).output().unwrap();
    let paths = String::from_utf8(o.stdout).unwrap();
    assert_eq!(paths.lines().count(), 2);
    assert!(paths.lines().next().unwrap().ends_with("ep2.mp4"), "natural order: ep2 before season 1/ep10");

    let mut child = rmedia(d.path())
        .args(["clip", "--inputs-from", "-", "-s", "1", "-e", "2", "-q", "-o"])
        .arg(d.path().join("out"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(paths.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(d.path().join("out/ep2/ep2_001_00-00-01.000_00-00-02.000.mp4").is_file());
    assert!(d.path().join("out/ep10/ep10_001_00-00-01.000_00-00-02.000.mp4").is_file());
}

#[test]
fn clip_a_folder_twice_never_clips_its_own_outputs() {
    require_ffmpeg!();
    let d = library();
    let list = d.path().join("l.txt");
    std::fs::write(&list, "0:01 0:02 x\n").unwrap();
    let run =
        || rmedia(d.path()).arg("clip").arg(d.path().join("lib")).arg("-l").arg(&list).output().unwrap();
    let first = run();
    assert_eq!(first.status.code(), Some(0), "{}", String::from_utf8_lossy(&first.stderr));
    assert!(d.path().join("lib/clips/ep2_001_x.mp4").is_file());
    assert!(d.path().join("lib/season 1/clips/ep10_001_x.mp4").is_file());
    let second = run();
    assert_eq!(second.status.code(), Some(0), "{}", String::from_utf8_lossy(&second.stderr));
    assert!(String::from_utf8_lossy(&second.stdout).contains("0 succeeded, 2 skipped"));
}

#[test]
fn scan_errors() {
    let d = tempfile::tempdir().unwrap();
    let code = |args: &[&str]| rmedia(d.path()).args(args).output().unwrap().status.code();
    assert_eq!(code(&["scan", "/definitely/not/here"]), Some(3));
    assert_eq!(code(&["scan", d.path().to_str().unwrap(), "--include", "a["]), Some(3));
    assert_eq!(code(&["scan", d.path().to_str().unwrap(), "--paths", "--json"]), Some(2));
}

#[test]
fn output_folder_with_glob_characters_is_still_excluded() {
    require_ffmpeg!();
    let d = library();
    let out = d.path().join("lib/out [*]");
    let list = d.path().join("l.txt");
    std::fs::write(&list, "0:01 0:02 x\n").unwrap();
    let run = || {
        rmedia(d.path())
            .arg("clip")
            .arg(d.path().join("lib"))
            .arg("-l")
            .arg(&list)
            .arg("-o")
            .arg(&out)
            .output()
            .unwrap()
    };
    assert_eq!(run().status.code(), Some(0));
    let second = run();
    assert!(
        String::from_utf8_lossy(&second.stdout).contains("0 succeeded, 2 skipped"),
        "{}",
        String::from_utf8_lossy(&second.stdout)
    );
}

#[test]
fn output_folder_cannot_be_the_scanned_folder() {
    require_ffmpeg!();
    let d = library();
    let lib = d.path().join("lib");
    let o =
        rmedia(d.path()).arg("clip").arg(&lib).args(["-s", "1", "-e", "2", "-o"]).arg(&lib).output().unwrap();
    assert_eq!(o.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&o.stderr).contains("also a scanned input folder"));
}

/// Hostile file names: non-UTF-8 bytes must not crash --json; newlines must not inject entries
/// into --paths output.
#[cfg(target_os = "linux")]
#[test]
fn hostile_file_names() {
    use std::os::unix::ffi::OsStrExt;
    require_ffmpeg!();
    let d = library();
    let lib = d.path().join("lib");
    let bad_utf8 = lib.join(std::ffi::OsStr::from_bytes(b"caf\xe9.mp4"));
    let newline = lib.join("evil\ninjected.mp4");
    std::fs::copy(fixture(Fixture::Gop50Mp4), &bad_utf8).unwrap();
    std::fs::copy(fixture(Fixture::Gop50Mp4), &newline).unwrap();

    let o = rmedia(d.path()).args(["scan", "--json"]).arg(&lib).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(doc["videos"].as_array().unwrap().len(), 4);

    let o = rmedia(d.path()).args(["scan", "--paths"]).arg(&lib).output().unwrap();
    let stdout = String::from_utf8(o.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 2, "only the two safe names are listed: {stdout}");
    assert!(!stdout.contains("injected"));
    assert!(String::from_utf8_lossy(&o.stderr).contains("skipping a path"));
}

#[test]
fn the_same_video_reached_twice_is_clipped_once() {
    require_ffmpeg!();
    let d = library();
    let lib = d.path().join("lib");
    let o = rmedia(d.path())
        .arg("clip")
        .arg(lib.join("ep2.mp4"))
        .arg(&lib)
        .args(["-s", "1", "-e", "2", "-q"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("2 succeeded"),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
}
