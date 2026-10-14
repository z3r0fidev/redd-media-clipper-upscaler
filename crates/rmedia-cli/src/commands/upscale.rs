//! `rmedia upscale` (002 US1): each input video, whole, made phone-ready for a device preset.

use rmedia_core::preset::{PresetRegistry, PresetSource, VideoCodec};
use rmedia_core::request::{BatchRequest, ClipRequest, Mode, Origin, PartialOptions, SourceRequest};
use rmedia_core::tools::LocateOptions;
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
    let (report, engine) = doctor::run(&opts);
    let Some((tools, caps)) = engine else {
        return out.engine_error(&report.problems.join("; "));
    };
    let inputs = match super::scan::expand_inputs(
        &tools,
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
    if (args.upscale.crop_path.is_some() || args.upscale.no_analyze) && !out.quiet {
        eprintln!("note: --crop-path and --no-analyze take effect once smart reframe is available");
    }
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
    let plan = plan::plan(&req, &tools, &caps, &CancellationToken::new()).await;
    execute(plan, tools, args.common.dry_run, args.common.jobs, &out).await
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
