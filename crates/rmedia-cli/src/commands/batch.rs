//! `rmedia batch MANIFEST` (US3): many videos, many clips, one run.

use rmedia_core::doctor;
use rmedia_core::input::{manifest_csv, manifest_json};
use rmedia_core::plan;
use rmedia_core::request::Options;
use rmedia_core::tools::LocateOptions;
use tokio_util::sync::CancellationToken;

use super::clip::{absolute, execute};
use crate::render::Output;
use crate::{BatchArgs, Cli};

pub async fn run(cli: &Cli, args: &BatchArgs) -> u8 {
    let out = Output::new(args.json, cli.quiet, cli.verbose);
    let manifest = absolute(&args.manifest);

    // Only flags given on the command line act as defaults; the manifest's own values win.
    let mut defaults = Options::default();
    if let Some(m) = args.mode {
        defaults.mode = m.into();
    }
    if let Some(o) = &args.output_dir {
        defaults.output_dir = Some(absolute(o));
    }
    if let Some(n) = &args.name {
        defaults.name_template = n.clone();
    }
    if let Some(c) = &args.container {
        defaults.container = Some(c.clone());
    }
    if let Some(e) = args.on_exists {
        defaults.on_exists = e.into();
    }

    let ext = manifest.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let parsed = match ext.as_str() {
        "json" => manifest_json::load(&manifest, &defaults),
        "csv" => manifest_csv::load(&manifest, &defaults),
        _ => {
            return out
                .input_error(&format!("{}: manifest must be a .json or .csv file", manifest.display()));
        }
    };
    let mut req = match parsed {
        Ok(r) => r,
        Err(issues) => return out.issues(&issues),
    };

    if args.continue_on_missing {
        let (present, missing): (Vec<_>, Vec<_>) = req.sources.into_iter().partition(|s| s.path.exists());
        for s in &missing {
            if !out.json {
                eprintln!(
                    "warning: {}: {} not found; skipping its {} clip(s)",
                    s.origin,
                    s.path.display(),
                    s.clips.len()
                );
            }
        }
        req.sources = present;
        if req.sources.is_empty() {
            return out.input_error("none of the manifest's videos exist");
        }
    }

    let opts = LocateOptions { ffmpeg: cli.ffmpeg.clone(), ffprobe: cli.ffprobe.clone(), bundled_dir: None };
    let (report, engine) = doctor::run(&opts);
    let Some((tools, caps)) = engine else {
        return out.engine_error(&report.problems.join("; "));
    };
    let plan = plan::plan(&req, &tools, &caps, &CancellationToken::new()).await;
    execute(plan, tools, args.dry_run, args.jobs, &out).await
}
