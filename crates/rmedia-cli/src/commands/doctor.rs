//! `rmedia doctor` (FR-026).

use rmedia_core::doctor;
use rmedia_core::tools::LocateOptions;
use rmedia_core::upscale::capability::CheckMode;

use crate::{Cli, DoctorArgs, exit};

pub async fn run(cli: &Cli, args: &DoctorArgs) -> u8 {
    let opts = LocateOptions { ffmpeg: cli.ffmpeg.clone(), ffprobe: cli.ffprobe.clone(), bundled_dir: None };
    let (mut report, mut engine) = doctor::run(&opts);
    let mode = if args.recheck { CheckMode::Recheck } else { CheckMode::Ensure };
    let ctrl_c = super::ctrl_c_token();
    doctor::check_capability(&mut report, engine.as_mut(), mode, &ctrl_c.token).await;
    if ctrl_c.token.is_cancelled() {
        return exit::CANCELLED;
    }
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report).expect("serialize doctor report"));
    } else {
        let tool = |t: &Option<rmedia_core::tools::Tool>| match t {
            Some(t) => format!("{} ({:?})", t.path.display(), t.source).to_lowercase(),
            None => "not found".into(),
        };
        println!("ffmpeg:   {}", tool(&report.ffmpeg));
        println!("ffprobe:  {}", tool(&report.ffprobe));
        if let Some(v) = &report.version {
            println!("version:  {v}");
        }
        for (enc, ok) in &report.encoders {
            println!("encoder:  {enc:<11} {}", if *ok { "yes" } else { "missing" });
        }
        let missing: Vec<&str> =
            report.filters.iter().filter(|(_, ok)| !**ok).map(|(f, _)| f.as_str()).collect();
        if missing.is_empty() {
            println!("filters:  all upscaling filters present");
        } else {
            println!("filters:  missing {} (upscaling needs some of these)", missing.join(", "));
        }
        if let Some(t) = &report.tiers {
            let line = |name: &str, info: &rmedia_core::upscale::capability::TierInfo| {
                let state = if info.available { "yes" } else { "no" };
                let fps = info.fps.map(|f| format!(", {f:.0} fps")).unwrap_or_default();
                let why = info.reason.as_deref().map(|r| format!(" ({r})")).unwrap_or_default();
                println!("tier:     {name:<4} {state}{fps}{why}");
            };
            line("cpu", &t.cpu);
            line("gpu", &t.gpu);
            line("ai", &t.ai);
        }
        for g in &report.gpus {
            let state = if g.usable {
                "usable".to_owned()
            } else {
                format!("not used: {}", g.reason.as_deref().unwrap_or("unusable"))
            };
            let kind = format!("{:?}", g.kind).to_lowercase();
            println!("gpu:      {} {} ({kind}) — {state}", g.index, g.name);
        }
        for (model, state) in &report.models {
            println!("model:    {model} {state}");
        }
        if let Some(c) = &report.capability {
            println!("checked:  {}", c.checked_at);
            for d in &c.demoted {
                println!("demoted:  {:?} since {} ({})", d.tier, d.at, d.error);
            }
        }
        for p in &report.preset_problems {
            eprintln!("problem:  preset file {p}");
        }
        if !report.accurate_containers.is_empty() {
            println!("accurate mode can write: {}", report.accurate_containers.join(", "));
        }
        for p in &report.problems {
            eprintln!("problem:  {p}");
        }
        println!("status:   {}", if report.ok { "ready" } else { "NOT READY" });
    }
    if report.ok { exit::OK } else { exit::ENGINE }
}
