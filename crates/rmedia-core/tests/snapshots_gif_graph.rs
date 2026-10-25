//! T016 (003): the GIF frame chain of refactor B1, pinned as insta snapshots. Each chain is also
//! run through the engine into `palettegen`, so a snapshot can only hold a graph FFmpeg accepts.
//! The 001/002 `build()` snapshots (here and in `upscale::graph`) are unchanged by B1.

use std::path::Path;
use std::process::Command;

use rmedia_core::Ts;
use rmedia_core::upscale::Reframe;
use rmedia_core::upscale::encode::ColourTags;
use rmedia_core::upscale::geometry::plan;
use rmedia_core::upscale::graph::{GifChain, GraphInput, Track, frame_chain, setparams_input, zscale_input};
use rmedia_testkit::{Fixture, ffmpeg, fixture, require_engine};

fn input(display: (u32, u32), target: (u32, u32), reframe: Reframe) -> GraphInput {
    GraphInput {
        trim: None,
        deinterlace: false,
        square_pixels: false,
        tonemap: false,
        keep_hdr: false,
        colour_in: zscale_input(&ColourTags::bt709(), 720),
        fps_cap: None,
        geometry: plan(display, target, reframe),
        reframe,
        track: None,
        gpu: None,
        params_in: setparams_input(&ColourTags::bt709(), 720),
        flatten_alpha: false,
    }
}

const AT_15: GifChain<'static> = GifChain { rate: "15/1", sharpen: None, mp4: false };

/// Runs `chain` on the first second of `src` into a palette, in a folder holding `reframe.cmd`.
fn accepted(chain: &str, src: &Path) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("reframe.cmd"), "0.0 [enter] crop@trk x 100;\n").unwrap();
    let o = Command::new(ffmpeg())
        .current_dir(dir.path())
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-t", "1", "-i"])
        .arg(src)
        .args(["-filter_complex", &format!("{chain},palettegen"), "-frames:v", "1", "-update", "1", "p.png"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{chain}\n{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn source_shape_is_one_resample_to_rgb() {
    let g = input((1280, 720), (720, 405), Reframe::Smart);
    let chain = frame_chain(&g, &AT_15);
    insta::assert_snapshot!(chain, @"[0:v]fps=15/1,scale=720:405:flags=lanczos+accurate_rnd+full_chroma_int:in_color_matrix=bt709:in_range=tv:out_range=pc,format=rgb24,setsar=1");
    require_engine!("palettegen");
    accepted(&chain, &fixture(Fixture::Land6));
}

#[test]
fn centre_crop_to_square() {
    let g = input((1280, 720), (480, 480), Reframe::Center);
    let chain = frame_chain(&g, &AT_15);
    insta::assert_snapshot!(chain, @"[0:v]fps=15/1,crop=720:720:280:0,scale=480:480:flags=lanczos+accurate_rnd+full_chroma_int:in_color_matrix=bt709:in_range=tv:out_range=pc,format=rgb24,setsar=1");
    require_engine!("palettegen");
    accepted(&chain, &fixture(Fixture::Land6));
}

#[test]
fn blur_pad_converts_both_layers_together() {
    let g = input((1280, 720), (480, 600), Reframe::Blur);
    let chain = frame_chain(&g, &AT_15);
    insta::assert_snapshot!(chain, @"[0:v]fps=15/1,split[bg0][fg0];[bg0]scale=48:60:force_original_aspect_ratio=increase,crop=48:60,gblur=sigma=3,eq=brightness=-0.08,scale=480:600,setsar=1[bg];[fg0]scale=480:270:flags=lanczos+accurate_rnd+full_chroma_int[fg];[bg][fg]overlay=0:164,scale=flags=accurate_rnd+full_chroma_int:in_color_matrix=bt709:in_range=tv:out_range=pc,format=rgb24,setsar=1");
    assert!(!chain.contains("zscale"), "no video colour conversion: {chain}");
    require_engine!("palettegen", "gblur");
    accepted(&chain, &fixture(Fixture::Land6));
}

#[test]
fn smart_crop_moves_with_sendcmd_and_can_fall_back() {
    let mut g = input((1280, 720), (480, 600), Reframe::Smart);
    g.track = Some(Track { x0: 300, fallback: false });
    let chain = frame_chain(&g, &AT_15);
    insta::assert_snapshot!(chain, @"[0:v]fps=15/1,sendcmd=f=reframe.cmd,crop@trk=w=576:h=720:x=300:y=0,scale=480:600:flags=lanczos+accurate_rnd+full_chroma_int,scale=flags=accurate_rnd+full_chroma_int:in_color_matrix=bt709:in_range=tv:out_range=pc,format=rgb24,setsar=1");
    g.track = Some(Track { x0: 300, fallback: true });
    let fallback = frame_chain(&g, &AT_15);
    insta::assert_snapshot!(fallback, @"[0:v]fps=15/1,split[a][b];[a]sendcmd=f=reframe.cmd,crop@trk=w=576:h=720:x=300:y=0,scale=480:600:flags=lanczos+accurate_rnd+full_chroma_int[crop];[b]split[bg0][fg0];[bg0]scale=48:60:force_original_aspect_ratio=increase,crop=48:60,gblur=sigma=3,eq=brightness=-0.08,scale=480:600,setsar=1[bg];[fg0]scale=480:270:flags=lanczos+accurate_rnd+full_chroma_int[fg];[bg][fg]overlay=0:164[pad];[crop][pad]overlay@fb=0:0:enable=0,scale=flags=accurate_rnd+full_chroma_int:in_color_matrix=bt709:in_range=tv:out_range=pc,format=rgb24,setsar=1");
    require_engine!("palettegen", "sendcmd", "gblur");
    accepted(&chain, &fixture(Fixture::Land6));
    accepted(&fallback, &fixture(Fixture::Land6));
}

#[test]
fn hdr_is_tone_mapped_before_the_resample() {
    let mut g = input((640, 360), (480, 270), Reframe::Smart);
    g.tonemap = true;
    g.colour_in = "min=2020_ncl:tin=smpte2084:pin=2020:rin=limited".into();
    let chain = frame_chain(&g, &AT_15);
    insta::assert_snapshot!(chain, @"[0:v]zscale=min=2020_ncl:tin=smpte2084:pin=2020:rin=limited:t=linear:npl=100,format=gbrpf32le,zscale=p=709,tonemap=tonemap=hable:desat=0,zscale=t=709:m=709:r=limited,format=yuv420p,fps=15/1,scale=480:270:flags=lanczos+accurate_rnd+full_chroma_int:in_color_matrix=bt709:in_range=tv:out_range=pc,format=rgb24,setsar=1");
    require_engine!("palettegen", "zscale", "tonemap");
    accepted(&chain, &fixture(Fixture::HdrTagged));
}

#[test]
fn alpha_is_flattened_onto_black_first() {
    let mut g = input((640, 360), (480, 270), Reframe::Smart);
    g.flatten_alpha = true;
    g.trim = Some((Ts::from_secs_f64(0.5), Some(Ts::from_secs_f64(2.0))));
    let chain = frame_chain(&g, &AT_15);
    insta::assert_snapshot!(chain, @"[0:v]trim=start=0.500000:end=2.000000,setpts=PTS-STARTPTS,split[fa][fb];[fa]format=yuv420p,drawbox=c=black:t=fill[fk];[fk][fb]overlay=format=auto,fps=15/1,scale=480:270:flags=lanczos+accurate_rnd+full_chroma_int:in_color_matrix=bt709:in_range=tv:out_range=pc,format=rgb24,setsar=1");
    require_engine!("palettegen", "drawbox", "overlay");
    accepted(&chain, &fixture(Fixture::Alpha));
}

#[test]
fn a_narrower_source_keeps_its_width() {
    // The planner sets W = min(target width, crop width): a 640 px source stays 640 wide.
    let g = input((640, 360), (640, 360), Reframe::Smart);
    let chain = frame_chain(&g, &AT_15);
    insta::assert_snapshot!(chain, @"[0:v]fps=15/1,scale=640:360:flags=lanczos+accurate_rnd+full_chroma_int:in_color_matrix=bt709:in_range=tv:out_range=pc,format=rgb24,setsar=1");
    assert!(g.geometry.no_upscale_needed);
}

#[test]
fn the_rate_is_whatever_the_planner_capped_it_to_and_sd_uses_bt601() {
    let mut g = input((720, 576), (480, 384), Reframe::Smart);
    g.colour_in = zscale_input(&ColourTags::default(), 576);
    g.square_pixels = true;
    let chain = frame_chain(&g, &GifChain { rate: "25/1", sharpen: None, mp4: false });
    insta::assert_snapshot!(chain, @"[0:v]scale=720:576,setsar=1,fps=25/1,scale=480:384:flags=lanczos+accurate_rnd+full_chroma_int:in_color_matrix=bt601:in_range=tv:out_range=pc,format=rgb24,setsar=1");
    let ntsc = frame_chain(&g, &GifChain { rate: "30000/1001", sharpen: None, mp4: false });
    assert!(ntsc.contains(",fps=30000/1001,"), "{ntsc}");
}

#[test]
fn flat_ui_sharpens_between_the_resample_and_rgb() {
    let g = input((1280, 720), (720, 405), Reframe::Smart);
    let chain = frame_chain(&g, &GifChain { rate: "15/1", sharpen: Some(0.5), mp4: false });
    insta::assert_snapshot!(chain, @"[0:v]fps=15/1,scale=720:405:flags=lanczos+accurate_rnd+full_chroma_int,unsharp=5:5:0.5:5:5:0,scale=flags=accurate_rnd+full_chroma_int:in_color_matrix=bt709:in_range=tv:out_range=pc,format=rgb24,setsar=1");
    require_engine!("palettegen", "unsharp");
    accepted(&chain, &fixture(Fixture::FlatUi));
}
