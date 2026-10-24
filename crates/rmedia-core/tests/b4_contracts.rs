//! T014 (003): refactor B4 keeps the 001/002 job contract. `PlannedJob::commands()` lists every
//! process of a job (one for clips and CPU/GPU upscales; the AI tier's three come from
//! `pipeline()`), and `args()` stays the final encode. Clip and upscale jobs carry no notices.

mod common;

use rmedia_core::EndSpec;
use rmedia_core::plan::Program;
use rmedia_core::request::Mode;
use rmedia_core::upscale::{TierChoice, UpscaleOptions};
use rmedia_testkit::{Fixture, fixture, require_engine};

#[tokio::test]
async fn commands_of_clip_and_upscale_jobs_are_their_single_encode() {
    require_engine!("libx264", "zscale", "cas", "hqdn3d", "aac");
    let (tools, caps) = common::engine();
    let out = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::Gop50Mp4);
    let mut defaults = common::opts(out.path(), Mode::Accurate);
    let req =
        common::request(&src, vec![common::clip(1.0, EndSpec::At(common::secs(3.0)))], defaults.clone());
    let plan = common::make_plan(&req, &tools, &caps).await;
    assert!(!plan.has_errors(), "{:?}", plan.issues);
    let job = &plan.jobs[0];
    let cmds = job.commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].program, Program::Ffmpeg);
    assert_eq!(cmds[0].args, job.args());
    assert!(job.notices.is_empty());

    defaults.upscale = Some(UpscaleOptions { tier: TierChoice::Cpu, ..UpscaleOptions::default() });
    let req = common::request(&src, vec![common::clip(1.0, EndSpec::At(common::secs(3.0)))], defaults);
    let plan = common::make_plan(&req, &tools, &caps).await;
    assert!(!plan.has_errors(), "{:?}", plan.issues);
    let job = &plan.jobs[0];
    assert_eq!(job.kind(), "upscale");
    assert!(job.pipeline().is_none());
    let cmds = job.commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].args, job.args(), "args() is the final encode");
    assert!(job.notices.is_empty());
}
