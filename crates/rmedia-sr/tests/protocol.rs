//! The worker's protocol (contracts/rmedia-sr-protocol.md): argv checks, exit codes, JSON stderr
//! and exact frame output. The `builtin:nearest --gpu none` mode needs no GPU; tests with the real
//! model are `#[ignore]` (they need a Vulkan GPU and `cargo xtask fetch-models`).

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn worker(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rmedia-sr"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rmedia-sr");
    let mut input = child.stdin.take().expect("stdin");
    let data = stdin.to_vec();
    // Write from another thread so a worker that stops reading cannot deadlock the test.
    let feeder = std::thread::spawn(move || {
        let _ = input.write_all(&data);
    });
    let out = child.wait_with_output().expect("wait");
    feeder.join().expect("feeder");
    out
}

/// Every stderr line is one JSON object; returns them.
fn json_lines(out: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("stderr line is not JSON ({e}): {l:?}")))
        .collect()
}

fn error_code(out: &Output) -> String {
    let lines = json_lines(out);
    let err = lines.iter().find(|v| v["type"] == "error").expect("an error line");
    err["code"].as_str().expect("code").to_owned()
}

fn nearest(scale: u32, w: u32, h: u32) -> Vec<String> {
    ["--model", "builtin:nearest", "--gpu", "none", "--scale", &scale.to_string()]
        .into_iter()
        .map(str::to_owned)
        .chain(["--width".into(), w.to_string(), "--height".into(), h.to_string()])
        .collect()
}

fn refs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

/// A deterministic test frame: every pixel distinct-ish.
fn frame(w: u32, h: u32, seed: u8) -> Vec<u8> {
    (0..w * h * 3).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
}

fn nearest_reference(f: &[u8], w: usize, h: usize, s: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(f.len() * s * s);
    for y in 0..h * s {
        for x in 0..w * s {
            let p = ((y / s) * w + x / s) * 3;
            out.extend_from_slice(&f[p..p + 3]);
        }
    }
    out
}

#[test]
fn version_names_the_worker_and_ncnn() {
    let out = worker(&["--version"], &[]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("rmedia-sr ") && text.contains("ncnn"), "{text}");
}

#[test]
fn usage_errors_exit_2_with_a_json_line() {
    for args in [
        vec![],
        vec!["--model", "builtin:nearest", "--gpu", "none", "--scale", "5", "--width", "2", "--height", "2"],
        vec!["--model", "/no/such/model", "--gpu", "none", "--scale", "2", "--width", "2", "--height", "2"],
        vec!["--model", "builtin:nearest", "--gpu", "none", "--scale", "2", "--width", "2"],
        vec!["--bogus"],
    ] {
        let out = worker(&args, &[]);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert_eq!(error_code(&out), "USAGE", "{args:?}");
    }
}

#[test]
fn nearest_outputs_exact_frames_in_order() {
    let (w, h) = (5, 3);
    for s in [2, 3, 4] {
        let frames: Vec<Vec<u8>> = (0..3).map(|i| frame(w, h, i * 50)).collect();
        let out = worker(&refs(&nearest(s, w, h)), &frames.concat());
        assert!(out.status.success(), "×{s}: {}", String::from_utf8_lossy(&out.stderr));
        let expected: Vec<u8> =
            frames.iter().flat_map(|f| nearest_reference(f, w as usize, h as usize, s as usize)).collect();
        assert_eq!(out.stdout.len(), (w * s * h * s * 3 * 3) as usize, "×{s}: three frames of (W·s)×(H·s)×3");
        assert_eq!(out.stdout, expected, "×{s}");
        assert!(json_lines(&out).iter().all(|v| v["type"] != "error"));
    }
}

#[test]
fn empty_input_is_zero_frames_and_a_short_frame_is_exit_4() {
    let args = nearest(2, 4, 4);
    let out = worker(&refs(&args), &[]);
    assert!(out.status.success());
    assert!(out.stdout.is_empty());

    let mut data = frame(4, 4, 1);
    data.extend_from_slice(&[1, 2, 3]);
    let out = worker(&refs(&args), &data);
    assert_eq!(out.status.code(), Some(4));
    assert_eq!(error_code(&out), "SHORT_FRAME");
    // The whole frame before the short one was still written.
    assert_eq!(out.stdout.len(), 8 * 8 * 3);
}

#[test]
fn a_missing_model_is_exit_5_or_no_gpu_exit_3() {
    // Model files are checked after the device; without any Vulkan GPU the device check fails.
    let out = worker(
        &["--model", "/no/such/model", "--gpu", "0", "--scale", "2", "--width", "4", "--height", "4"],
        &[],
    );
    match out.status.code() {
        Some(5) => assert_eq!(error_code(&out), "MODEL_LOAD"),
        Some(3) => {
            assert!(["NO_VULKAN", "NO_DEVICE", "SOFTWARE_DEVICE"].contains(&error_code(&out).as_str()))
        }
        other => panic!("exit {other:?}: {}", String::from_utf8_lossy(&out.stderr)),
    }
}

#[test]
fn selftest_reports_timings() {
    let mut args = nearest(2, 64, 36);
    args.extend(["--selftest".into(), "--frames".into(), "2".into()]);
    let out = worker(&refs(&args), &[]);
    assert!(out.status.success());
    let lines = json_lines(&out);
    let st = lines.iter().find(|v| v["type"] == "selftest").expect("selftest line");
    assert_eq!(st["frames"], 2);
    assert!(st["sec_per_frame"].as_f64().unwrap() >= 0.0);
    assert!(st["sec_per_mpx"].as_f64().is_some());
    assert!(out.stdout.is_empty(), "the self-test writes no frames");
}

fn model(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models").join(name)
}

fn first_gpu() -> Option<u32> {
    let out = worker(&["--list-gpus"], &[]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    v["devices"]
        .as_array()?
        .iter()
        .find(|d| d["software"] == false)
        .and_then(|d| d["index"].as_u64())
        .map(|i| i as u32)
}

#[test]
#[ignore = "needs a Vulkan GPU and `cargo xtask fetch-models`"]
fn the_real_model_upscales_on_a_gpu_and_refuses_software_devices() {
    let out = worker(&["--list-gpus"], &[]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON listing");
    for d in v["devices"].as_array().expect("devices") {
        let name = d["name"].as_str().unwrap().to_ascii_lowercase();
        if name.contains("llvmpipe") || name.contains("lavapipe") || d["kind"] == "cpu" {
            assert_eq!(d["software"], true, "{d}");
            let out = worker(
                &[
                    "--model",
                    model("realesr-animevideov3-x2").to_str().unwrap(),
                    "--gpu",
                    &d["index"].to_string(),
                    "--scale",
                    "2",
                    "--width",
                    "8",
                    "--height",
                    "8",
                ],
                &[],
            );
            assert_eq!(out.status.code(), Some(3));
            assert_eq!(error_code(&out), "SOFTWARE_DEVICE");
        }
    }
    let gpu = first_gpu().expect("a hardware Vulkan GPU").to_string();
    let (w, h) = (96u32, 54u32);
    let frames = [frame(w, h, 3), frame(w, h, 90)].concat();
    let m = model("realesr-animevideov3-x2");
    let args =
        ["--model", m.to_str().unwrap(), "--gpu", &gpu, "--scale", "2", "--width", "96", "--height", "54"];
    let a = worker(&args, &frames);
    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stderr));
    assert_eq!(a.stdout.len(), (w * 2 * h * 2 * 3 * 2) as usize);
    let lines = json_lines(&a);
    assert!(lines.iter().any(|v| v["type"] == "device"));
    // Same device, same output (contract › Determinism), and tiling does not change it much.
    let b = worker(&args, &frames);
    assert_eq!(a.stdout, b.stdout, "deterministic on one device");
    let tiled = worker(&[&args[..], &["--tile", "32"]].concat(), &frames);
    let diff = a.stdout.iter().zip(&tiled.stdout).map(|(x, y)| x.abs_diff(*y) as u64).sum::<u64>() as f64
        / a.stdout.len() as f64;
    assert!(diff < 2.0, "tiled output differs by {diff:.2} per byte on average");

    // A truncated weights file is a model error, not a crash.
    let dir = tempfile_dir();
    std::fs::copy(model("realesr-animevideov3-x2.param"), dir.join("t.param")).unwrap();
    let bin = std::fs::read(model("realesr-animevideov3-x2.bin")).unwrap();
    std::fs::write(dir.join("t.bin"), &bin[..bin.len() / 2]).unwrap();
    let t = dir.join("t");
    let out = worker(
        &["--model", t.to_str().unwrap(), "--gpu", &gpu, "--scale", "2", "--width", "8", "--height", "8"],
        &[],
    );
    assert_eq!(out.status.code(), Some(5), "{}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(dir);
}

fn tempfile_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("rmedia-sr-test-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}
