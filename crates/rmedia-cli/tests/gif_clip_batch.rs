//! T062 (003 US5): `clip --gif`, `clip -l --gif`, the "needs --gif" usage error, `clip --gif
//! --upscale` as one job, and `batch --gif` (whose dry run never samples).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use assert_cmd::cargo::cargo_bin;
use rmedia_testkit::{Fixture, fixture, require_engine};
use serde_json::Value;

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

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Every file under `dir` with extension `ext`, sorted, relative to `dir`.
fn files(dir: &Path, ext: &str) -> Vec<String> {
    fn walk(d: &Path, root: &Path, ext: &str, out: &mut Vec<String>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, ext, out);
            } else if p.extension().is_some_and(|x| x == ext) {
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, ext, &mut out);
    out.sort();
    out
}

#[test]
fn clip_gif_and_a_clip_list_make_one_gif_per_clip() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::Land6, d.path(), "land.mp4");
    let o = rmedia(d.path())
        .args(["clip", "-s", "1", "-e", "3", "--gif", "--width", "320"])
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(files(&d.path().join("gifs"), "gif").len(), 1, "{:?}", files(d.path(), "gif"));

    let list = d.path().join("clips.txt");
    std::fs::write(&list, "0 1.5 one\n2 3.5 two\n").unwrap();
    let out = d.path().join("list");
    let o = rmedia(d.path())
        .args(["clip", "-l"])
        .arg(&list)
        .args(["--gif", "--width", "320", "-o"])
        .arg(&out)
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let gifs = files(&out, "gif");
    assert_eq!(gifs.len(), 2, "{gifs:?}");
    assert!(gifs.iter().any(|g| g.contains("one")) && gifs.iter().any(|g| g.contains("two")), "{gifs:?}");
}

#[test]
fn gif_flags_without_gif_are_usage_errors() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in.mp4");
    std::fs::write(&src, b"x").unwrap();
    let o =
        rmedia(d.path()).args(["clip", "-s", "1", "--target", "reddit-comment"]).arg(&src).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("--target needs --gif"), "{}", stderr(&o));
}

#[test]
fn clip_gif_upscale_is_one_job_without_an_intermediate() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::Land6, d.path(), "land.mp4");
    let o = rmedia(d.path())
        .args(["clip", "-s", "1", "-e", "3", "--gif", "--upscale", "--dry-run", "--json", "--no-sample"])
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    let jobs = doc["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 1, "one chained job");
    assert_eq!(jobs[0]["kind"], "gif");
    let warnings = jobs[0]["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|w| w.as_str().unwrap().starts_with("upscale skipped")), "{warnings:?}");

    let o = rmedia(d.path())
        .args(["clip", "-s", "1", "-e", "3", "--gif", "--upscale", "--width", "320"])
        .arg(&src)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(files(&d.path().join("gifs"), "gif").len(), 1);
    assert_eq!(files(&d.path().join("gifs"), "mp4"), Vec::<String>::new(), "no intermediate video");
}

#[test]
fn batch_gif_and_its_dry_run_does_not_sample() {
    require_engine!("palettegen", "paletteuse");
    let d = tempfile::tempdir().unwrap();
    let src = copy(Fixture::Land6, d.path(), "land.mp4");
    let manifest = d.path().join("jobs.json");
    let text = format!(
        r#"{{"version":1,"videos":[{{"input":{:?},"clips":[{{"start":0,"end":2}},{{"start":3,"end":5,"options":{{"gif":{{"target":"reddit-comment"}}}}}}]}}]}}"#,
        src.to_string_lossy()
    );
    std::fs::write(&manifest, text).unwrap();
    let o = rmedia(d.path()).args(["batch", "--gif", "--dry-run", "--json"]).arg(&manifest).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    let jobs = doc["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 2);
    assert!(
        jobs.iter().all(|j| j["gif"]["estimate"]["from"] == "pending"),
        "a batch dry run does not sample"
    );
    assert_eq!(jobs[1]["gif"]["target"], "reddit-comment");

    let o = rmedia(d.path()).args(["batch", "--gif", "--width", "320"]).arg(&manifest).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(files(d.path(), "gif").len(), 2, "{:?}", files(d.path(), "gif"));
}
