//! T040 (003): the argv of a planned GIF job — palette pass, encode pass — pinned as insta
//! snapshots (the source path and the palette key are redacted). `args()` is the encode, and
//! `commands()` lists both passes (plus gifsicle when it is found).

mod common;

use common::*;
use rmedia_core::EndSpec;
use rmedia_core::gif::GifOptions;
use rmedia_core::plan::ffargs::display_args;
use rmedia_core::plan::{JobSpec, Program};
use rmedia_core::request::Options;
use rmedia_testkit::{Fixture, fixture, require_engine};

#[tokio::test(flavor = "multi_thread")]
async fn planned_gif_argv() {
    require_engine!("palettegen", "paletteuse");
    let (tools, mut caps) = engine();
    caps.gif_engines = rmedia_core::gif::engines::Engines::default(); // no gifsicle: two passes
    let out = tempfile::tempdir().unwrap();
    let src = fixture(Fixture::Land6);
    let opts = Options {
        gif: Some(GifOptions::default()),
        output_dir: Some(out.path().to_path_buf()),
        ..Options::default()
    };
    let p = make_plan(&request(&src, vec![clip(1.0, EndSpec::At(secs(3.0)))], opts), &tools, &caps).await;
    assert!(!p.has_errors(), "{:?}", p.issues);
    let job = &p.jobs[0];
    assert!(matches!(job.spec, JobSpec::Gif(_)));
    let cmds = job.commands();
    assert_eq!(cmds.len(), 2);
    assert!(cmds.iter().all(|c| c.program == Program::Ffmpeg));
    assert_eq!(cmds[1].args, job.args(), "args() is the encode");
    // The planner canonicalises the source, so its spelling differs by platform: redact the token.
    let show = |a: &[std::ffi::OsString]| {
        let v: Vec<String> = display_args(a)
            .into_iter()
            .map(|t| if t.starts_with("file:") { "file:[SRC]".to_owned() } else { t })
            .collect();
        v.join(" ")
    };
    let palette = show(&cmds[0].args);
    let key = palette.split_whitespace().last().unwrap().to_owned();
    assert!(key.starts_with("p-") && key.ends_with(".png"), "{key}");
    insta::assert_snapshot!("palette", palette.replace(&key, "p-[KEY].png"));
    insta::assert_snapshot!("encode", show(&cmds[1].args).replace(&key, "p-[KEY].png"));
}
