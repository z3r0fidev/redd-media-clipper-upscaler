//! `rmedia doctor` (FR-026).

use rmedia_core::doctor;
use rmedia_core::tools::LocateOptions;

use crate::{Cli, DoctorArgs, exit};

pub fn run(cli: &Cli, args: &DoctorArgs) -> u8 {
    let opts = LocateOptions { ffmpeg: cli.ffmpeg.clone(), ffprobe: cli.ffprobe.clone(), bundled_dir: None };
    let (report, _) = doctor::run(&opts);
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
