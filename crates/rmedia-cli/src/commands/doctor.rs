//! `rmedia doctor` (FR-026).

use rmedia_core::doctor;
use rmedia_core::upscale::capability::CheckMode;

use crate::{Cli, DoctorArgs, exit};

pub async fn run(cli: &Cli, args: &DoctorArgs) -> u8 {
    let opts = cli.locate();
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
                let fps = info
                    .fps
                    .map(|f| if f < 10.0 { format!(", {f:.1} fps") } else { format!(", {f:.0} fps") })
                    .unwrap_or_default();
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
        gif_lines(&report.gif);
        for p in &report.problems {
            eprintln!("problem:  {p}");
        }
        println!("status:   {}", if report.ok { "ready" } else { "NOT READY" });
    }
    if report.ok { exit::OK } else { exit::ENGINE }
}

/// GIF export: the built-in engine, the Reddit MP4 and the optional engines (contracts/cli.md
/// › doctor). None of these change the status.
fn gif_lines(g: &doctor::GifReport) {
    let mp4_needs = |m: &String| m == "libx264" || m == "mp4 muxer";
    let state = |ok: bool, have: &str, missing: Vec<&str>| {
        if ok { format!("yes ({have})") } else { format!("no (missing {})", missing.join(", ")) }
    };
    let gif_missing = g.missing.iter().filter(|m| !mp4_needs(m)).map(String::as_str).collect();
    let mp4_missing = g.missing.iter().filter(|m| mp4_needs(m)).map(String::as_str).collect();
    println!("gif:      {}", state(g.gif, "palettegen, paletteuse, gif muxer", gif_missing));
    println!("reddit mp4: {}", state(g.mp4, "libx264", mp4_missing));
    for e in &g.engines {
        let name = e.name.name();
        match (&e.version, &e.path) {
            (Some(v), Some(p)) if e.ok => {
                let source = e.source.map(|s| format!("{s:?} ").to_lowercase()).unwrap_or_default();
                println!("{name}: {v} ({source}{})", p.display());
            }
            _ => println!(
                "{name}: no ({}; set {name}_path or {})",
                crate::render::clean(e.reason.as_deref().unwrap_or("not found")),
                e.name.env_var()
            ),
        }
    }
    for p in &g.target_problems {
        eprintln!("problem:  GIF target file {}", crate::render::clean(p));
    }
}
