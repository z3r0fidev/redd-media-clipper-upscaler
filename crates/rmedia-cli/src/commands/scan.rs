//! `rmedia scan` (US4) and folder/`--inputs-from` expansion for `clip`.

use std::io::Read;
use std::path::{Path, PathBuf};

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use rmedia_core::discover::{self, ScanEvent, ScanResult};
use rmedia_core::input::pathlist;
use rmedia_core::probe::{MediaInfo, StreamKind};
use rmedia_core::tools::{LocateOptions, Tools};
use rmedia_core::{Error, doctor};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::render::{Output, clean};
use crate::{Cli, ScanArgs, ScanFlags, exit};

pub async fn run(cli: &Cli, args: &ScanArgs) -> u8 {
    let out = Output::new(args.json, cli.quiet || args.paths, cli.verbose);
    for r in &args.roots {
        if !r.exists() {
            return out.input_error(&format!("{} does not exist", r.display()));
        }
    }
    let opts = LocateOptions { ffmpeg: cli.ffmpeg.clone(), ffprobe: cli.ffprobe.clone(), bundled_dir: None };
    let (report, engine) = doctor::run(&opts);
    let Some((tools, _)) = engine else {
        return out.engine_error(&report.problems.join("; "));
    };
    let roots: Vec<PathBuf> = args.roots.iter().map(|r| absolute(r)).collect();
    let result = match scan_with_progress(&tools, &roots, &args.flags, &out).await {
        Ok(r) => r,
        Err(code) => return code,
    };

    if args.json {
        println!("{}", scan_doc(&result));
    } else if args.paths {
        for v in &result.videos {
            println!("{}", v.path.display());
        }
    } else {
        for v in &result.videos {
            println!("{}", row(v));
        }
        if cli.verbose > 0 {
            for r in &result.rejected {
                eprintln!(
                    "rejected ({:?}): {} — {}",
                    r.reason,
                    clean(&r.path.display().to_string()),
                    clean(&r.detail)
                );
            }
        }
        let s = &result.skipped;
        eprintln!(
            "{} video(s), {} rejected{}; skipped {} hidden, {} temp, {} symlink, {} excluded ({:.1} s)",
            result.videos.len(),
            result.rejected.len(),
            if result.rejected.is_empty() || cli.verbose > 0 { "" } else { " (-v lists them)" },
            s.hidden,
            s.temp,
            s.symlink,
            s.excluded,
            result.elapsed_ms as f64 / 1000.0
        );
    }
    exit::OK
}

/// Runs a scan with a spinner on stderr and Ctrl-C cancellation.
async fn scan_with_progress(
    tools: &Tools,
    roots: &[PathBuf],
    flags: &ScanFlags,
    out: &Output,
) -> Result<ScanResult, u8> {
    let pb = if out.json || out.quiet {
        ProgressBar::with_draw_target(None, ProgressDrawTarget::hidden())
    } else {
        ProgressBar::new_spinner()
            .with_style(ProgressStyle::with_template("{spinner} {msg}").expect("template"))
    };
    pb.enable_steady_tick(std::time::Duration::from_millis(120));
    pb.set_message("scanning…");
    let cancel = CancellationToken::new();
    let c = cancel.clone();
    let ctrl_c = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            c.cancel();
        }
    });
    let progress = |e: ScanEvent| {
        if let ScanEvent::Progress { visited, candidates, probed } = e {
            pb.set_message(format!("{visited} entries, checking {probed}/{candidates} candidate videos"));
        }
    };
    let result = discover::scan(tools, roots, &flags.options(), &progress, &cancel).await;
    ctrl_c.abort();
    pb.finish_and_clear();
    match result {
        Ok(r) => Ok(r),
        Err(Error::Cancelled) => Err(exit::CANCELLED),
        Err(Error::InvalidInput(m)) => Err(out.input_error(&m)),
        Err(e) => Err(out.input_error(&e.to_string())),
    }
}

/// Expands `clip` inputs: files as given, folders scanned, plus `--inputs-from` (file or stdin).
pub async fn expand_inputs(
    tools: &Tools,
    inputs: &[PathBuf],
    inputs_from: Option<&str>,
    flags: &ScanFlags,
    output_dir: Option<&Path>,
    out: &Output,
) -> Result<Vec<PathBuf>, u8> {
    let mut all: Vec<PathBuf> = inputs.iter().map(|p| absolute(p)).collect();
    if let Some(src) = inputs_from {
        let (text, base) = if src == "-" {
            let mut s = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut s) {
                return Err(out.input_error(&format!("cannot read paths from stdin: {e}")));
            }
            (s, std::env::current_dir().unwrap_or_default())
        } else {
            let p = absolute(Path::new(src));
            match std::fs::read_to_string(&p) {
                Ok(s) => (s, p.parent().map(Path::to_path_buf).unwrap_or_default()),
                Err(e) => return Err(out.input_error(&format!("cannot read {}: {e}", p.display()))),
            }
        };
        all.extend(pathlist::parse(&base, &text));
    }
    let (dirs, files): (Vec<PathBuf>, Vec<PathBuf>) = all.into_iter().partition(|p| p.is_dir());
    let mut sources = files;
    if !dirs.is_empty() {
        // Never treat our own outputs as new sources on a re-run.
        let mut flags = flags.clone();
        match output_dir.map(absolute) {
            Some(o) => {
                for d in &dirs {
                    if let Ok(rel) = o.strip_prefix(d) {
                        flags.exclude.push(format!("{}/**", rel.display()));
                    }
                }
            }
            None => flags.exclude.push("**/clips/**".into()),
        }
        let found = scan_with_progress(tools, &dirs, &flags, out).await?;
        if !out.quiet && !out.json {
            eprintln!(
                "found {} video(s) in {} folder(s){}",
                found.videos.len(),
                dirs.len(),
                if found.rejected.is_empty() {
                    String::new()
                } else {
                    format!(", {} file(s) rejected", found.rejected.len())
                }
            );
        }
        sources.extend(found.videos.into_iter().map(|v| v.path));
    }
    if sources.is_empty() {
        return Err(out.input_error("no input videos (folders contained no videos)"));
    }
    Ok(sources)
}

fn row(v: &MediaInfo) -> String {
    let video = v.first_video();
    let res = video.and_then(|s| Some(format!("{}x{}", s.width?, s.height?))).unwrap_or_default();
    let fps = video
        .and_then(|s| s.frame_rate)
        .map(|(n, d)| format!("{:.2}", n as f64 / d as f64))
        .unwrap_or_default();
    let codec = video.map(|s| s.codec.clone()).unwrap_or_default();
    let dur = v.duration.map(|d| d.display()).unwrap_or_else(|| "?".into());
    format!(
        "{dur:>12}  {res:>9}  {fps:>6}  {codec:<6}  {:>9}  {}",
        human_size(v.size),
        clean(&v.path.display().to_string())
    )
}

fn human_size(b: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{b} B") } else { format!("{v:.1} {}", units[i]) }
}

fn scan_doc(r: &ScanResult) -> Value {
    let videos: Vec<Value> = r
        .videos
        .iter()
        .map(|v| {
            let vs = v.first_video();
            json!({
                "path": v.path,
                "size": v.size,
                "duration": v.duration,
                "width": vs.and_then(|s| s.width),
                "height": vs.and_then(|s| s.height),
                "fps": vs.and_then(|s| s.frame_rate).map(|(n, d)| ((n as f64 / d as f64) * 1000.0).round() / 1000.0),
                "video_codec": vs.map(|s| s.codec.clone()),
                "container": v.container,
                "audio_streams": v.streams.iter().filter(|s| s.kind == StreamKind::Audio).count(),
                "subtitle_streams": v.streams.iter().filter(|s| s.kind == StreamKind::Subtitle).count(),
            })
        })
        .collect();
    json!({
        "schema": 1,
        "type": "scan",
        "roots": r.roots,
        "videos": videos,
        "rejected": r.rejected.iter().map(|x| json!({"path": x.path, "reason": x.reason})).collect::<Vec<_>>(),
        "skipped": r.skipped,
        "elapsed_ms": r.elapsed_ms,
    })
}

fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}
