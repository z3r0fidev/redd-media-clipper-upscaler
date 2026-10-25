//! T032 (003): output verification on files the engine makes in the test: a looping GIF, a
//! play-once GIF, and a Reddit-style MP4 with and without fast start.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use rmedia_core::Ts;
use rmedia_core::gif::LoopCount;
use rmedia_core::gif::verify::{GifExpect, Mp4Expect, check_gif, check_mp4, gif_facts, mp4_faststart};
use rmedia_testkit::{ffmpeg, require_engine};
use tokio_util::sync::CancellationToken;

fn make(dir: &Path, name: &str, args: &[&str]) -> PathBuf {
    let out = dir.join(name);
    let o = Command::new(ffmpeg())
        .args([
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=320x180:r=15:d=2",
        ])
        .args(args)
        .arg(&out)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn gifs_made_by_the_engine_pass_and_wrong_expectations_fail() {
    require_engine!("palettegen");
    let (tools, _) = common::engine();
    let dir = tempfile::tempdir().unwrap();
    let cancel = CancellationToken::new();
    let forever = make(dir.path(), "f.gif", &["-loop", "0", "-f", "gif"]);
    let facts = gif_facts(&tools, &forever, &cancel).await.unwrap();
    let expect = GifExpect {
        width: 320,
        height: 180,
        frames: 30,
        optimised_cs: None,
        loop_count: LoopCount::Forever,
        max_bytes: None,
    };
    assert_eq!(check_gif(&facts, &expect), Ok(()), "{facts:?}");
    assert!(facts.delays.iter().all(|d| *d == 6 || *d == 7), "{:?}", facts.delays);
    assert!(check_gif(&facts, &GifExpect { loop_count: LoopCount::Plays(1), ..expect.clone() }).is_err());
    assert!(check_gif(&facts, &GifExpect { max_bytes: Some(facts.bytes - 1), ..expect.clone() }).is_err());

    let once = make(dir.path(), "o.gif", &["-loop", "-1", "-f", "gif"]);
    let facts = gif_facts(&tools, &once, &cancel).await.unwrap();
    assert_eq!(facts.netscape, None);
    assert_eq!(check_gif(&facts, &GifExpect { loop_count: LoopCount::Plays(1), ..expect }), Ok(()));
}

#[tokio::test(flavor = "multi_thread")]
async fn mp4s_are_checked_for_streams_faststart_size_and_length() {
    require_engine!("libx264", "aac");
    let (tools, _) = common::engine();
    let dir = tempfile::tempdir().unwrap();
    let cancel = CancellationToken::new();
    let enc = ["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"];
    let fast = make(dir.path(), "fast.mp4", &[&enc[..], &["-movflags", "+faststart"]].concat());
    let slow = make(dir.path(), "slow.mp4", &enc);
    let expect = Mp4Expect {
        width: 320,
        height: 180,
        duration: Ts::from_secs_f64(2.0),
        frame: Ts::from_secs_f64(1.0 / 15.0),
        max_bytes: None,
    };
    let info = rmedia_core::probe::probe(&tools, &fast, &cancel).await.unwrap();
    assert!(mp4_faststart(&fast).unwrap());
    assert_eq!(check_mp4(&info, true, &expect), Ok(()));
    assert!(!mp4_faststart(&slow).unwrap());
    assert!(check_mp4(&info, false, &expect).unwrap_err().contains("fast-start"));
    assert!(
        check_mp4(&info, true, &Mp4Expect { duration: Ts::from_secs_f64(3.0), ..expect.clone() }).is_err()
    );

    // A sound track is not allowed in the silent loop.
    let out = dir.path().join("audio.mp4");
    let o = Command::new(ffmpeg())
        .args([
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=320x180:r=15:d=2",
        ])
        .args([
            "-f",
            "lavfi",
            "-i",
            "sine=d=2",
            "-shortest",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-c:a",
            "aac",
        ])
        .args(["-movflags", "+faststart"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(o.status.success());
    let info = rmedia_core::probe::probe(&tools, &out, &cancel).await.unwrap();
    assert!(check_mp4(&info, true, &expect).unwrap_err().contains("aac"));
}
