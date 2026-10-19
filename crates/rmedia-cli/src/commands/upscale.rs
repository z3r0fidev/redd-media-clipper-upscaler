//! `rmedia upscale` (002 US1): each input video, whole, made phone-ready for a device preset.

use rmedia_core::preset::{PresetRegistry, PresetSource, VideoCodec};
use rmedia_core::request::{BatchRequest, ClipRequest, Mode, Origin, PartialOptions, SourceRequest};
use rmedia_core::tools::LocateOptions;
use rmedia_core::upscale::prepare_plan;
use rmedia_core::{EndSpec, Ts, doctor, plan, settings};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::clip::{absolute, execute, options};
use crate::render::{Output, print_json};
use crate::{Cli, UpscaleArgs, exit};

pub async fn run(cli: &Cli, args: &UpscaleArgs) -> u8 {
    let out = Output::new(args.common.json, cli.quiet, cli.verbose);
    if args.list_presets {
        return list_presets(&out);
    }
    let opts = LocateOptions { ffmpeg: cli.ffmpeg.clone(), ffprobe: cli.ffprobe.clone(), bundled_dir: None };
    let (mut report, mut engine) = doctor::run(&opts);
    let Some(found) = engine.as_mut() else {
        return out.engine_error(&report.problems.join("; "));
    };
    let inputs = match super::scan::expand_inputs(
        &found.0,
        &args.inputs,
        args.inputs_from.as_deref(),
        &args.scan,
        args.common.output_dir.as_deref(),
        "upscaled",
        &out,
    )
    .await
    {
        Ok(i) => i,
        Err(code) => return code,
    };
    // Upscaling always re-encodes, so the clip mode does not apply; outputs never need a
    // per-source sub-folder (`{source}_{preset}` names are unique per folder).
    let mut defaults = options(&args.common, Mode::Accurate, false);
    defaults.upscale = Some(args.upscale.options());
    let whole = ClipRequest {
        start: Ts::ZERO,
        end: EndSpec::EndOfMedia,
        name: None,
        overrides: PartialOptions::default(),
        origin: Origin::Cli,
    };
    let req = BatchRequest {
        defaults,
        sources: inputs
            .iter()
            .map(|p| SourceRequest {
                path: absolute(p),
                clips: vec![whole.clone()],
                overrides: PartialOptions::default(),
                origin: Origin::Cli,
            })
            .collect(),
    };
    // A forced CPU tier needs no GPU check (FR-008); auto and a forced GPU do.
    if let Err(code) = super::check_tiers(&mut report, found, &req).await {
        return code;
    }
    let (tools, caps) = engine.expect("checked above");
    let mut plan = plan::plan(&req, &tools, &caps, &CancellationToken::new()).await;
    let crop_path = args.crop_path.as_deref();
    if crop_path.is_some() && args.no_analyze {
        eprintln!("error: --crop-path needs the analysis; it cannot be combined with --no-analyze");
        return exit::USAGE;
    }
    let analyse = (args.common.dry_run && !args.no_analyze) || crop_path.is_some();
    if analyse && !plan.has_errors() {
        // The crop path is shown (and written) before anything renders; the render reuses the
        // cached analysis (FR-016).
        let cancel = CancellationToken::new();
        let on_ctrl_c = cancel.clone();
        let watcher = tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                on_ctrl_c.cancel();
            }
        });
        let prepared = prepare_plan(&mut plan, &tools, &cancel).await;
        watcher.abort();
        if prepared.is_err() {
            return exit::CANCELLED;
        }
        if let Some(file) = crop_path
            && let Err(code) = write_crop_path(&plan, file, &out)
        {
            return code;
        }
    }
    execute(plan, tools, args.common.dry_run, args.common.jobs, &out).await
}

/// `--crop-path FILE`: the per-frame crop path of the one smart-reframe job, as
/// `{"schema":1,"fps":…,"crop":[w,h],"x":[…]}` (contracts/json-output.md).
fn write_crop_path(plan: &plan::Plan, file: &std::path::Path, out: &Output) -> Result<(), u8> {
    let analysed: Vec<&rmedia_core::upscale::reframe::ReframeAnalysis> = plan
        .jobs
        .iter()
        .filter_map(|j| match &j.spec {
            plan::JobSpec::Upscale(u) => u.up.analysis(),
            _ => None,
        })
        .collect();
    let [a] = analysed.as_slice() else {
        eprintln!(
            "error: --crop-path needs exactly one video that uses smart reframe (found {})",
            analysed.len()
        );
        return Err(exit::USAGE);
    };
    let doc = json!({"schema": 1, "fps": a.fps, "crop": [a.crop.0, a.crop.1], "x": a.path});
    let file = absolute(file);
    // Never write over a video of this run (Principle V).
    let same = |p: &std::path::Path| {
        p == file
            || std::fs::canonicalize(p)
                .ok()
                .zip(std::fs::canonicalize(&file).ok())
                .is_some_and(|(a, b)| a == b)
    };
    if plan.jobs.iter().any(|j| same(&j.source) || same(&j.output)) {
        return Err(
            out.input_error(&format!("--crop-path {} is one of the videos of this run", file.display()))
        );
    }
    // A fresh, unique temp file beside the target, then an atomic rename.
    let nanos =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let name = file.file_name().map_or_else(|| "crop-path".into(), |n| n.to_string_lossy().into_owned());
    let tmp = file.with_file_name(format!(".{name}.{}-{nanos}.tmp", std::process::id()));
    let written = std::fs::File::options()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut f| {
            std::io::Write::write_all(&mut f, &serde_json::to_vec(&doc).expect("crop path serialises"))
        })
        .and_then(|()| std::fs::rename(&tmp, &file));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(out.input_error(&format!("cannot write {}: {e}", file.display())));
    }
    if !out.quiet && !out.json {
        eprintln!("crop path written to {}", file.display());
    }
    Ok(())
}

/// `--list-presets [--json]`: built-in and user presets, plus any invalid user preset files.
fn list_presets(out: &Output) -> u8 {
    let reg = PresetRegistry::load(settings::config_dir().as_deref());
    if out.json {
        let presets: Vec<_> = reg
            .list()
            .map(|(p, src)| {
                json!({
                    "id": p.id,
                    "name": p.name,
                    "size": [p.width, p.height],
                    "devices": p.devices,
                    "codec": p.video.codec,
                    "max_fps": p.max_fps,
                    "hdr_capable": p.hdr_capable,
                    "limits": p.limits,
                    "source": match src { PresetSource::Builtin => json!("builtin"), PresetSource::User(f) => json!(rmedia_core::pathser::lossy(f)) },
                })
            })
            .collect();
        let problems: Vec<_> = reg
            .problems()
            .iter()
            .map(|p| json!({"file": rmedia_core::pathser::lossy(&p.file), "message": p.message}))
            .collect();
        print_json(
            &json!({"schema": 1, "type": "presets", "default": reg.default_id(), "presets": presets, "problems": problems}),
        );
    } else {
        for (p, src) in reg.list() {
            let codec = match p.video.codec {
                VideoCodec::Hevc => "HEVC",
                VideoCodec::H264 => "H.264",
            };
            let mark = if p.id == reg.default_id() { " (default)" } else { "" };
            let user = if matches!(src, PresetSource::User(_)) { " [user]" } else { "" };
            println!("{:<16} {:>4}x{:<4}  {codec:<5}  {}{mark}{user}", p.id, p.width, p.height, p.name);
            println!("{:<16} for: {}", "", p.devices.join(", "));
        }
        for p in reg.problems() {
            eprintln!("warning: preset file {} ignored: {}", p.file.display(), p.message);
        }
    }
    exit::OK
}
