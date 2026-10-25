//! `rmedia gif` (feature 003, contracts/cli.md): make GIFs that fit a Reddit target.

use rmedia_core::gif::target::GifTargetRegistry;
use rmedia_core::preset::PresetSource;
use rmedia_core::request::{BatchRequest, ClipRequest, Mode, Origin, PartialOptions, SourceRequest};
use rmedia_core::upscale::estimate::human_bytes;
use rmedia_core::{EndSpec, Ts, doctor, plan, settings};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::clip::{absolute, execute, options};
use crate::render::{Output, clean, print_json};
use crate::{Cli, GifArgs, exit};

pub async fn run(cli: &Cli, args: &GifArgs) -> u8 {
    let out = Output::new(args.common.json, cli.quiet, cli.verbose);
    if args.list_targets {
        return list_targets(&out);
    }
    if let Some(msg) = usage_error(args) {
        eprintln!("error: {msg}");
        return exit::USAGE;
    }
    let range = match range(args) {
        Ok(r) => r,
        Err(msg) => return out.input_error(&msg),
    };
    let (mut report, mut engine) = doctor::run(&cli.locate());
    let Some(found) = engine.as_mut() else {
        return out.engine_error(&report.problems.join("; "));
    };
    let inputs = match super::scan::expand_inputs(
        &found.0,
        &args.inputs,
        args.inputs_from.as_deref(),
        &args.scan,
        args.common.output_dir.as_deref(),
        "gifs",
        &out,
    )
    .await
    {
        Ok(i) => i,
        Err(code) => return code,
    };
    // GIF export always re-encodes, silently and without subtitles (the planner drops them).
    let mut defaults = options(&args.common, Mode::Accurate, inputs.len() > 1);
    defaults.gif = Some(args.gif.options());
    let req = BatchRequest {
        defaults,
        sources: inputs
            .iter()
            .map(|p| SourceRequest {
                path: absolute(p),
                clips: vec![range.clone()],
                overrides: PartialOptions::default(),
                origin: Origin::Cli,
            })
            .collect(),
    };
    if let Err(code) = super::check_tiers(&mut report, found, &req).await {
        return code;
    }
    let (tools, caps) = engine.expect("checked above");
    let mut plan = plan::plan(&req, &tools, &caps, &CancellationToken::new()).await;
    if let Some(file) = args.gif.crop_path.as_deref()
        && !plan.has_errors()
    {
        // The crop path is written before anything encodes; the job reuses the analysis.
        let ctrl_c = super::ctrl_c_token();
        if rmedia_core::gif::estimate::analyze_plan(&mut plan, &tools, &ctrl_c.token).await.is_err() {
            return exit::CANCELLED;
        }
        if let Err(code) = super::upscale::write_crop_path(&plan, file, &out) {
            return code;
        }
    }
    if args.common.dry_run && !args.gif.no_sample && !plan.has_errors() {
        // The estimate runs short sample encodes in a temporary folder (research R4).
        let ctrl_c = super::ctrl_c_token();
        if rmedia_core::gif::estimate::prepare(&mut plan, &tools, &ctrl_c.token).await.is_err() {
            return exit::CANCELLED;
        }
    }
    execute(plan, tools, args.common.dry_run, args.common.jobs, &out).await
}

/// Shared flags that do not apply to GIF export, and combinations clap cannot check.
fn usage_error(args: &GifArgs) -> Option<String> {
    let c = &args.common;
    let given: Vec<&str> = [
        ("--container (use --format)", c.container.is_some()),
        ("--audio", c.audio.is_some()),
        ("--no-audio", c.no_audio),
        ("--no-subs", c.no_subs),
    ]
    .into_iter()
    .filter_map(|(n, set)| set.then_some(n))
    .collect();
    if !given.is_empty() {
        return Some(format!(
            "{} {} not apply to GIF export (the output is always silent, without subtitles)",
            given.join(", "),
            if given.len() == 1 { "does" } else { "do" }
        ));
    }
    args.gif.usage_error()
}

/// The range from -s/-e/-d (the whole video by default).
fn range(args: &GifArgs) -> Result<ClipRequest, String> {
    let start = args.start.as_deref().map_or(Ok(Ts::ZERO), Ts::parse).map_err(|e| format!("--start: {e}"))?;
    let end = match (&args.end, &args.duration) {
        (Some(e), _) => EndSpec::parse(e),
        (None, Some(d)) => {
            let d = d.trim();
            EndSpec::parse(&if d.starts_with('+') { d.to_owned() } else { format!("+{d}") })
        }
        (None, None) => Ok(EndSpec::EndOfMedia),
    }
    .map_err(|e| format!("--end/--duration: {e}"))?;
    Ok(ClipRequest { start, end, name: None, overrides: PartialOptions::default(), origin: Origin::Cli })
}

/// `--list-targets [--json]`: built-in and user targets, plus any invalid user target files.
fn list_targets(out: &Output) -> u8 {
    let reg = GifTargetRegistry::load(settings::config_dir().as_deref());
    if out.json {
        let targets: Vec<_> = reg
            .list()
            .map(|(t, src)| {
                let mut v = serde_json::to_value(t).expect("a target serialises");
                v["source"] = match src {
                    PresetSource::Builtin => json!("builtin"),
                    PresetSource::User(f) => json!(rmedia_core::pathser::lossy(f)),
                };
                v
            })
            .collect();
        let problems: Vec<_> = reg
            .problems()
            .iter()
            .map(|p| json!({"file": rmedia_core::pathser::lossy(&p.file), "message": p.message}))
            .collect();
        print_json(&json!({"schema": 1, "type": "targets", "targets": targets, "problems": problems}));
    } else {
        for (t, src) in reg.list() {
            let budget = t.budget.map_or_else(
                || "no budget".to_owned(),
                |b| format!("{} / {}", human_bytes(b.soft), human_bytes(b.hard)),
            );
            let verified = if t.verified { "verified" } else { "unverified" };
            let user = if matches!(src, PresetSource::User(_)) { " [user]" } else { "" };
            println!(
                "{:<16} {:<22} {budget:<20} {}/{} px  {} fps  {verified}{user}",
                t.id,
                clean(&t.name),
                t.width.landscape,
                t.width.portrait,
                t.fps
            );
        }
        for p in reg.problems() {
            eprintln!("warning: target file {} ignored: {}", p.file.display(), clean(&p.message));
        }
    }
    exit::OK
}
