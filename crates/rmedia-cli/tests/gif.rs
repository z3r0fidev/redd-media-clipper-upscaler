//! T035 (003 US1): CLI contract for `rmedia gif` (contracts/cli.md, contracts/json-output.md):
//! dry runs (human and JSON, sampled or not) that write nothing, `--list-targets`, usage errors,
//! exit codes and the `doctor` lines.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use assert_cmd::cargo::cargo_bin;
use rmedia_testkit::{Fixture, fixture, require_engine, require_tool};
use serde_json::Value;

/// `rmedia` with state and config isolated in `dir` (no user targets leak in) and no GIF engine
/// found unless the test passes one.
fn rmedia(dir: &Path) -> Command {
    let mut c = Command::new(cargo_bin("rmedia"));
    let none = dir.join("no-driver.json");
    c.env("RMEDIA_STATE_DIR", dir.join("state"))
        .env("RMEDIA_CONFIG_DIR", dir.join("config"))
        .env("RMEDIA_CACHE_DIR", dir.join("cache"))
        .env("VK_DRIVER_FILES", &none)
        .env("VK_ICD_FILENAMES", &none)
        .env("RMEDIA_GIFSICLE", dir.join("no-gifsicle"))
        .env("RMEDIA_GIFSKI", dir.join("no-gifski"));
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

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Everything under `dir` except the isolated state folders.
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
fn a_sampled_json_dry_run_has_the_gif_block_and_writes_nothing() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::Land6, d.path(), "land.mp4");
    let o = rmedia(d.path()).args(["gif", "--dry-run", "--json"]).arg(&src).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc = json(&o);
    assert_eq!((doc["schema"].as_u64(), doc["type"].as_str()), (Some(1), Some("plan")));
    let job = &doc["jobs"][0];
    assert_eq!(job["kind"], "gif");
    assert!(Path::new(job["output"].as_str().unwrap()).ends_with("gifs/land_reddit-post.gif"), "{job}");
    let g = &job["gif"];
    assert_eq!((g["target"].as_str(), g["target_verified"].as_bool()), (Some("reddit-post"), Some(false)));
    assert_eq!((g["format"].as_str(), g["engine"].as_str()), (Some("gif"), Some("builtin")));
    assert_eq!((g["settings"]["width"].as_u64(), g["settings"]["height"].as_u64()), (Some(720), Some(405)));
    assert_eq!(g["settings"]["timing"]["exact"], false);
    assert_eq!(g["budget"]["hard"], 20_000_000);
    // reddit-post has a budget: the estimate is the search's prediction (it fits at once).
    assert_eq!(g["estimate"]["from"], "search");
    assert_eq!(g["estimate"]["lowered"], serde_json::json!([]), "{g}");
    assert!(g["estimate"]["bytes"].as_u64().is_some_and(|b| b > 0), "{g}");
    assert_eq!(job["notices"][0]["code"], "TIMING_APPROXIMATE");
    assert!(job["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("6–7 cs")));
    let pipeline = job["pipeline"].as_array().unwrap();
    assert_eq!(pipeline.len(), 2, "palette and encode, no gifsicle: {pipeline:?}");
    assert_eq!(pipeline[1], job["argv"], "argv is the final encode");
    assert_eq!(entries(d.path()), ["land.mp4"], "a dry run writes nothing beside the source");
}

#[test]
fn no_sample_leaves_the_estimate_pending_and_the_human_dry_run_says_so() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::Land6, d.path(), "land.mp4");
    let o = rmedia(d.path()).args(["gif", "--dry-run", "--json", "--no-sample"]).arg(&src).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let e = &json(&o)["jobs"][0]["gif"]["estimate"];
    assert_eq!((e["from"].as_str(), e["bytes"].is_null()), (Some("pending"), true), "{e}");

    let o = rmedia(d.path()).args(["gif", "--dry-run", "--no-sample"]).arg(&src).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let text =
        String::from_utf8_lossy(&o.stdout).replace(&*d.path().to_string_lossy(), "<dir>").replace('\\', "/");
    insta::assert_snapshot!(text, @r"
    #1   would write: <dir>/gifs/land_reddit-post.gif
         from <dir>/land.mp4  00:00:00.000 → 00:00:06.000  [gif]
         target reddit-post (Reddit post, limits unverified), gif, engine builtin (target engine: builtin)
         720x405, 15 fps (≈ 6.67 cs, approximate), 256 colours, dither bayer:4
         budget 8.0 MB soft / 20.0 MB hard
         estimate pending (not sampled)
         warning: 15 fps is shown as 6–7 cs frame delays (≈ 15.0 fps)
    1 job(s) planned, 0 would be skipped. Nothing was written (dry run).
    ");
    assert_eq!(entries(d.path()), ["land.mp4"]);
}

#[test]
fn list_targets_human_and_json() {
    let d = tempfile::tempdir().unwrap();
    let o = rmedia(d.path()).args(["gif", "--list-targets"]).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let text = String::from_utf8_lossy(&o.stdout);
    for id in ["reddit-post", "reddit-comment", "max-quality", "flat-ui"] {
        assert!(text.lines().any(|l| l.starts_with(id)), "{id} missing:\n{text}");
    }
    assert!(text.lines().any(|l| l.starts_with("reddit-post") && l.contains("unverified")), "{text}");

    let o = rmedia(d.path()).args(["gif", "--list-targets", "--json"]).output().unwrap();
    let doc = json(&o);
    assert_eq!((doc["schema"].as_u64(), doc["type"].as_str()), (Some(1), Some("targets")));
    let targets = doc["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 4, "{doc}");
    assert!(targets.iter().all(|t| t["source"] == "builtin"), "{doc}");
    assert_eq!(doc["problems"].as_array().map(Vec::len), Some(0));
}

#[test]
fn shared_flags_that_do_not_apply_and_bad_values_are_usage_errors() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in.mp4");
    for args in [
        &["--mode", "fast"][..],
        &["--container", "mkv"],
        &["--audio", "1"],
        &["--no-audio"],
        &["--no-subs"],
        &["--fps", "51"],
        &["--fps", "0"],
        &["--width", "2000"],
        &["--width", "32"],
        &["--colors", "1"],
        &["--budget", "0"],
        &["--budget", "3XB"],
        &["--budget", "2GiB"],
        &["--dither", "bayer:6"],
        &["--loop", "0"],
        &["--budget", "none", "--hard-limit", "8MB"],
    ] {
        let o = rmedia(d.path()).arg("gif").arg(&src).args(args).output().unwrap();
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", stderr(&o));
    }
    assert!(!d.path().join("gifs").exists());
}

#[test]
fn input_and_engine_errors_exit_3_and_4_and_write_nothing() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::Land6, d.path(), "land.mp4");
    let run = |args: &[&str]| rmedia(d.path()).arg("gif").arg(&src).args(args).output().unwrap();

    let o = run(&["--target", "nope", "--json"]);
    assert_eq!(o.status.code(), Some(3), "{}", stderr(&o));
    assert_eq!(json(&o)["issues"][0]["code"], "TARGET_UNKNOWN");

    // The static floor is checked at plan time, before any sample (C4).
    let o = run(&["--budget", "10k", "--no-sample", "--dry-run", "--json"]);
    assert_eq!(o.status.code(), Some(3), "{}", stderr(&o));
    let issue = &json(&o)["issues"][0];
    assert_eq!(issue["code"], "BUDGET_UNREACHABLE");
    assert_eq!(issue["detail"], serde_json::json!({"min_bytes": 65536}));

    let o = run(&["--engine", "gifski", "--strict-engine", "--json"]);
    assert_eq!(o.status.code(), Some(4), "{}", stderr(&o));
    assert_eq!(json(&o)["issues"][0]["code"], "ENGINE_MISSING");
    assert_eq!(entries(d.path()), ["land.mp4"]);
}

#[test]
fn a_run_writes_the_gif_and_exits_0() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::Land6, d.path(), "land.mp4");
    let o = rmedia(d.path())
        .args(["gif", "-s", "1", "-d", "2", "--width", "320", "--fps", "10", "--loop", "3"])
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let out = d.path().join("gifs/land_001_00-00-01.000_00-00-03.000_reddit-post.gif");
    assert_eq!(rmedia_testkit::display_size(&out), (320, 180));
    assert_eq!(rmedia_testkit::gif_delays(&out).len(), 20);
    assert_eq!(rmedia_testkit::gif_loop(&out), Some(2), "3 plays = 2 repeats");
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(text.contains("320x180, 10.000 fps"), "{text}");
    assert!(text.contains("1 succeeded"), "{text}");
}

/// Ctrl-C before the run (here while ffprobe, slowed by a wrapper, is still answering): exit
/// 130, nothing written, and the NDJSON still ends with a summary line.
#[cfg(unix)]
#[test]
fn ctrl_c_before_the_run_still_ends_with_a_summary() {
    use std::os::unix::fs::PermissionsExt;
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::Land6, d.path(), "land.mp4");
    let slow = d.path().join("slow-ffprobe");
    std::fs::write(
        &slow,
        format!("#!/bin/sh\nsleep 2\nexec '{}' \"$@\"\n", rmedia_testkit::ffprobe().display()),
    )
    .unwrap();
    std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
    let child = rmedia(d.path())
        .env("RMEDIA_FFPROBE", &slow)
        .args(["gif", "--json", "--width", "320"])
        .arg(&src)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1000));
    Command::new("kill").args(["-INT", &child.id().to_string()]).status().unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(130), "{}", stderr(&out));
    let text = String::from_utf8_lossy(&out.stdout);
    let last: Value = serde_json::from_str(text.lines().last().expect("a summary line")).unwrap();
    assert_eq!(last["type"], "summary", "{text}");
    assert_eq!(last["exit_code"], 130, "{text}");
    assert_eq!(text.matches(r#""type":"summary""#).count(), 1, "{text}");
    assert_eq!(entries(d.path()), ["land.mp4", "slow-ffprobe"]);
}

#[cfg(unix)]
#[test]
fn ctrl_c_cancels_with_exit_130_and_no_partial_files() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::Busy10, d.path(), "busy.mp4");
    let mut child = rmedia(d.path())
        .args(["gif", "--json", "--budget", "none", "--width", "1280"])
        .arg(&src)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Signal once the job runs (its first stage), not after a fixed wait: planning can outlast
    // one on a slow runner, and before the run the CLI does not catch Ctrl-C yet.
    let stdout = child.stdout.take().unwrap();
    let (started, on_start) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        use std::io::BufRead;
        let mut lines = Vec::new();
        for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
            if line.contains(r#""type":"job_stage""#) {
                let _ = started.send(());
            }
            lines.push(line);
        }
        lines
    });
    on_start.recv_timeout(std::time::Duration::from_secs(60)).expect("the job never started");
    std::thread::sleep(std::time::Duration::from_millis(200));
    Command::new("kill").args(["-INT", &child.id().to_string()]).status().unwrap();
    let out = child.wait_with_output().unwrap();
    let lines = reader.join().unwrap();
    assert_eq!(out.status.code(), Some(130), "{}", stderr(&out));
    let last: Value = serde_json::from_str(lines.last().expect("a summary line")).unwrap();
    assert_eq!((last["type"].as_str(), last["cancelled"].as_u64()), (Some("summary"), Some(1)), "{last}");
    let gifs = d.path().join("gifs");
    let left: Vec<_> =
        std::fs::read_dir(&gifs).map(|r| r.flatten().map(|e| e.file_name()).collect()).unwrap_or_default();
    assert!(left.is_empty(), "nothing is left behind: {left:?}");
}

#[test]
fn doctor_reports_gif_export_and_missing_engines_without_changing_the_status() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let o = rmedia(d.path()).arg("doctor").output().unwrap();
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(text.contains("gif:      yes (palettegen, paletteuse, gif muxer)"), "{text}");
    assert!(text.lines().any(|l| l.starts_with("gifsicle: no (") && l.contains("RMEDIA_GIFSICLE")), "{text}");
    assert!(text.lines().any(|l| l.starts_with("gifski: no (") && l.contains("RMEDIA_GIFSKI")), "{text}");
    assert!(!text.contains("NOT READY") || o.status.code() == Some(4), "{text}");

    let o = rmedia(d.path()).args(["doctor", "--json"]).output().unwrap();
    let gif = &json(&o)["gif"];
    assert_eq!(gif["gif"], true);
    assert_eq!(gif["engines"].as_array().map(Vec::len), Some(2), "{gif}");
}

#[test]
fn doctor_shows_a_found_gifsicle_with_its_version() {
    require_tool!(gifsicle);
    let d = tempfile::tempdir().unwrap();
    let path = rmedia_testkit::gif_tool(rmedia_testkit::GifTool::Gifsicle).unwrap();
    let o = rmedia(d.path()).arg("doctor").env("RMEDIA_GIFSICLE", &path).output().unwrap();
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(text.lines().any(|l| l.starts_with("gifsicle: 1.")), "{text}");
}

/// T051: a budgeted run lists the search and what it lowered, in the summary and in JSON.
#[test]
fn a_budgeted_run_reports_its_search() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::Land6, d.path(), "land.mp4");
    let o = rmedia(d.path()).args(["gif", "--budget", "1MB"]).arg(&src).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let text = String::from_utf8_lossy(&o.stdout);
    let line = text.lines().find(|l| l.starts_with("  budget search: ")).unwrap_or_else(|| panic!("{text}"));
    assert!(line.contains("; lowered "), "{line}");
    let out = d.path().join("gifs/land_reddit-post.gif");
    assert!(std::fs::metadata(&out).unwrap().len() <= 1_000_000);

    let o = rmedia(d.path())
        .args(["gif", "--budget", "1MB", "--json", "--on-exists", "overwrite"])
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc: Value = String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v["type"] == "summary")
        .unwrap_or_else(|| panic!("{}", String::from_utf8_lossy(&o.stdout)));
    // The measure is flattened into the job (`"measure":"gif"`, contracts/json-output.md).
    let m = &doc["jobs"][0];
    assert_eq!(m["measure"], "gif", "{m}");
    assert!(m["search"]["encodes"].as_u64().is_some_and(|n| n <= 10), "{m}");
    assert!(m["search"]["lowered"].as_array().is_some_and(|l| !l.is_empty()), "{m}");
    assert_eq!(m["search"]["fit"], "soft", "{m}");
    assert!(m["trimmed"].is_null(), "{m}");
}

/// T056: `--format mp4` writes the Reddit MP4, and a Reddit-post GIF run mentions the MP4 once.
#[test]
fn the_mp4_format_and_the_mp4_note_once_per_run() {
    require_engine!("palettegen", "paletteuse", "libx264");
    let d = tempfile::tempdir().unwrap();
    let a = copy(Fixture::Land6, d.path(), "a.mp4");
    let b = copy(Fixture::Land6, d.path(), "b.mp4");
    let o = rmedia(d.path()).args(["gif", "--dry-run"]).arg(&a).arg(&b).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let text = String::from_utf8_lossy(&o.stdout);
    assert_eq!(text.matches("note: as a Reddit MP4").count(), 1, "{text}");

    let o = rmedia(d.path()).args(["gif", "--format", "mp4"]).arg(&a).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let out = d.path().join("gifs/a_reddit-post.mp4");
    assert!(rmedia_core::gif::verify::mp4_faststart(&out).unwrap());
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(text.contains("libx264") && !text.contains("note: as a Reddit MP4"), "{text}");
}

/// T080 review: the MP4 accepts up to 60 fps, and its dry run shows the one H.264 encode it runs.
#[test]
fn an_mp4_dry_run_shows_its_own_encode_and_takes_60_fps() {
    require_engine!("palettegen", "paletteuse", "libx264");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::Land6, d.path(), "land.mp4");
    let args = ["gif", "--format", "mp4", "--fps", "60", "--dry-run", "--no-sample"];
    let o = rmedia(d.path()).args(args).arg("--json").arg(&src).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let job = &json(&o)["jobs"][0];
    let argv = job["argv"].to_string();
    assert!(argv.contains("libx264") && !argv.contains("palette"), "{argv}");
    assert_eq!(job["pipeline"].as_array().unwrap().len(), 1, "{}", job["pipeline"]);
    let gif =
        rmedia(d.path()).args(["gif", "--fps", "60", "--dry-run", "--no-sample"]).arg(&src).output().unwrap();
    assert_eq!(gif.status.code(), Some(2), "a GIF still stops at 50 fps: {}", stderr(&gif));

    let o = rmedia(d.path()).args(args).arg(&src).output().unwrap();
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(text.contains("H.264, silent loop") && !text.contains("colours"), "{text}");
}

/// T058: with smart framing, `--crop-path` writes the crop path before anything encodes.
#[test]
fn crop_path_writes_the_gifs_smart_framing_path() {
    require_engine!("palettegen", "paletteuse", "sendcmd", "scdet");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::MovingMarker, d.path(), "land.mp4");
    let path = d.path().join("path.json");
    let o = rmedia(d.path())
        .args(["gif", "--shape", "4:5", "--dry-run", "--no-sample", "--crop-path"])
        .arg(&path)
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!((doc["schema"].as_u64(), doc["fps"].as_f64()), (Some(1), Some(15.0)), "the GIF's rate");
    assert_eq!(doc["x"].as_array().map(Vec::len), Some(150), "10 s at 15 fps");
    let o = rmedia(d.path())
        .args(["gif", "--shape", "4:5", "--framing", "center", "--dry-run", "--no-sample", "--crop-path"])
        .arg(d.path().join("p2.json"))
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2), "--crop-path needs a smart-framing job: {}", stderr(&o));
}
