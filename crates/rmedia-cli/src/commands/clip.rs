//! `rmedia clip` (US1): one clip per input from -s/-e/-d.

use std::path::{Path, PathBuf};

use rmedia_core::exec::{self, RunConfig};
use rmedia_core::input::cliplist;
use rmedia_core::plan::{self, Plan};
use rmedia_core::request::{
    AudioSel, BatchRequest, ClipRequest, MULTI_SOURCE_NAME_TEMPLATE, Options, Origin, PartialOptions,
    SourceRequest, StreamSelection, SubSel,
};
use rmedia_core::tools::LocateOptions;
use rmedia_core::{EndSpec, Ts, doctor};
use tokio_util::sync::CancellationToken;

use crate::render::{self, Output};
use crate::{Cli, ClipArgs, CommonArgs, exit};

pub async fn run(cli: &Cli, args: &ClipArgs) -> u8 {
    let out = Output::new(args.common.json, cli.quiet, cli.verbose);

    // Parse clips first: input errors never need the engine.
    let clips = match &args.list {
        Some(list) => match cliplist::load(&absolute(list)) {
            Ok(clips) => clips,
            Err(issues) => return out.issues(&issues),
        },
        None => match single_clip(args) {
            Ok(c) => vec![c],
            Err(msg) => return out.input_error(&msg),
        },
    };
    if !args.upscale && args.up.any() {
        let given = args.up.given();
        let verb = if given.len() == 1 { "needs" } else { "need" };
        eprintln!("error: {} {verb} --upscale", given.join(", "));
        return exit::USAGE;
    }
    let opts = LocateOptions {
        ffmpeg: cli.ffmpeg.clone(),
        ffprobe: cli.ffprobe.clone(),
        ..LocateOptions::default()
    };
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
        "clips",
        &out,
    )
    .await
    {
        Ok(i) => i,
        Err(code) => return code,
    };

    let mut defaults = options(&args.common, args.mode.into(), inputs.len() > 1);
    if args.upscale {
        defaults.upscale = Some(args.up.options());
    }
    let req = BatchRequest {
        defaults,
        sources: inputs
            .iter()
            .map(|p| SourceRequest {
                path: absolute(p),
                clips: clips.clone(),
                overrides: PartialOptions::default(),
                origin: Origin::Cli,
            })
            .collect(),
    };
    if let Err(code) = super::check_tiers(&mut report, found, &req).await {
        return code;
    }
    let (tools, caps) = engine.expect("checked above");

    let cancel = CancellationToken::new();
    let plan = plan::plan(&req, &tools, &caps, &cancel).await;
    execute(plan, tools, args.common.dry_run, args.common.jobs, &out).await
}

/// Shared by clip/batch: report issues, dry-run, or run with live rendering and Ctrl-C.
pub async fn execute(
    plan: Plan,
    tools: rmedia_core::tools::Tools,
    dry_run: bool,
    jobs: Option<u16>,
    out: &Output,
) -> u8 {
    if plan.cancelled {
        return exit::CANCELLED;
    }
    if plan.has_errors() {
        out.plan_issues(&plan);
        return if plan.engine_errors_only() { exit::ENGINE } else { exit::INVALID_INPUT };
    }
    if dry_run {
        out.plan(&plan);
        return exit::OK;
    }
    out.plan_warnings(&plan);

    let temps: Vec<std::path::PathBuf> =
        plan.jobs.iter().flat_map(|j| std::iter::once(j.temp.clone()).chain(j.scratch.clone())).collect();
    let sink = render::sink(out, &plan);
    let cfg = RunConfig { jobs: jobs.map_or_else(exec::default_jobs, usize::from), ..RunConfig::default() };
    let handle = exec::start(plan, tools, cfg, sink.clone());
    let cancel = handle.cancel_token();
    let ctrl_c = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            eprintln!("\ncancelling… (press Ctrl-C again to quit immediately)");
            cancel.cancel();
            if tokio::signal::ctrl_c().await.is_ok() {
                // Children die with us: process group + PDEATHSIG (Linux) / Job Object (Windows).
                // Best-effort removal of partial outputs first; anything still locked (Windows)
                // is purged by the next run via the crash journal.
                for t in &temps {
                    // Only our own temp outputs (`.<stem>.rmedia-<run>-<job>.part|.d`).
                    if t.file_name().is_some_and(|n| n.to_string_lossy().contains(".rmedia-")) {
                        let _ = rmedia_core::exec::commit::remove_now(t);
                    }
                }
                std::process::exit(i32::from(exit::CANCELLED));
            }
        }
    });
    let summary = handle.wait().await;
    ctrl_c.abort();
    sink.finish();
    out.summary(&summary);
    u8::try_from(summary.exit_code).unwrap_or(exit::JOB_FAILED)
}

fn single_clip(args: &ClipArgs) -> Result<ClipRequest, String> {
    let start = args.start.as_deref().unwrap_or("0");
    let start = Ts::parse(start).map_err(|e| format!("--start: {e}"))?;
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

pub fn options(c: &CommonArgs, mode: rmedia_core::request::Mode, multi_source: bool) -> Options {
    let mut o = Options {
        mode,
        output_dir: c.output_dir.as_deref().map(absolute),
        container: c.container.clone(),
        on_exists: c.on_exists.into(),
        streams: StreamSelection {
            audio: if c.no_audio {
                AudioSel::None
            } else {
                c.audio.clone().map_or(AudioSel::All, AudioSel::Tracks)
            },
            subtitles: if c.no_subs { SubSel::None } else { SubSel::All },
        },
        ..Options::default()
    };
    if let Some(n) = &c.name {
        o.name_template = n.clone();
    } else if multi_source && c.output_dir.is_some() {
        // Many sources into one folder: one sub-folder per source.
        o.name_template = MULTI_SOURCE_NAME_TEMPLATE.to_owned();
    }
    o
}

pub fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}
