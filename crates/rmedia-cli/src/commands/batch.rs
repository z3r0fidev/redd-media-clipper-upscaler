//! `rmedia batch MANIFEST` (US3): many videos, many clips, one run.

use rmedia_core::doctor;
use rmedia_core::input::{manifest_csv, manifest_json};
use rmedia_core::plan;
use rmedia_core::request::Options;
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
    // Upscale flags fill the fields of videos the manifest upscales; only --upscale turns
    // upscaling on for every video (a manifest `enabled: false` still wins).
    if args.upscale || args.up.any() {
        // Also the base of a layer that turns upscaling back on below an `enabled: false`.
        defaults.upscale_fill = Some(args.up.options());
    }
    if args.upscale {
        defaults.upscale = Some(args.up.options());
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

    if args.up.any() && !req.resolved().any(|o| o.upscale.is_some()) {
        // stderr even with --json; the flags only fill videos the manifest upscales.
        eprintln!(
            "warning: {} had no effect: the manifest upscales no video (add an \"upscale\" object, or use --upscale)",
            args.up.given().join(", ")
        );
    }

    if args.continue_on_missing {
        let (present, missing): (Vec<_>, Vec<_>) = req.sources.into_iter().partition(|s| s.path.exists());
        for s in &missing {
            // stderr even with --json: stdout stays machine-readable, the skip is never silent.
            eprintln!(
                "warning: {}: {} not found; skipping its {} clip(s)",
                s.origin,
                s.path.display(),
                s.clips.len()
            );
        }
        req.sources = present;
        if req.sources.is_empty() {
            return out.input_error("none of the manifest's videos exist");
        }
    }

    // Same rule as `clip`: many sources into one shared output folder → one sub-folder each.
    if req.sources.len() > 1
        && args.output_dir.is_some()
        && args.name.is_none()
        && req.defaults.name_template == rmedia_core::request::DEFAULT_NAME_TEMPLATE
    {
        req.defaults.name_template = rmedia_core::request::MULTI_SOURCE_NAME_TEMPLATE.to_owned();
    }

    let opts = cli.locate();
    let (mut report, mut engine) = doctor::run(&opts);
    let Some(found) = engine.as_mut() else {
        return out.engine_error(&report.problems.join("; "));
    };
    if let Err(code) = super::check_tiers(&mut report, found, &req).await {
        return code;
    }
    let (tools, caps) = engine.expect("checked above");
    let plan = plan::plan(&req, &tools, &caps, &CancellationToken::new()).await;
    execute(plan, tools, args.dry_run, args.jobs, &out).await
}
