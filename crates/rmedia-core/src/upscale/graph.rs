//! Pure `-filter_complex` builder for upscale jobs (research R3–R5).
//!
//! One pass: trim → pre-processing at source resolution → reframe → scale → sharpen → tag.
//! The graph reads input `0:v` and writes the label `[v]`.

use super::Reframe;
use super::encode::ColourTags;
use super::geometry::{Geometry, Overflow, even};
use crate::time::Ts;

/// Everything the graph depends on, resolved by the planner.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphInput {
    /// Cut `[start, end)` from the source first, so later timings are clip-relative.
    pub trim: Option<(Ts, Option<Ts>)>,
    /// Interlaced source: deinterlace first (`bwdif`).
    pub deinterlace: bool,
    /// Non-square pixels: resample to square pixels at the displayed size (the same size the
    /// geometry was planned for, so the crop always fits).
    pub square_pixels: bool,
    /// HDR source converted to SDR (research R4).
    pub tonemap: bool,
    /// HDR kept: 10-bit output, no colour conversion.
    pub keep_hdr: bool,
    /// The source's colour properties in zscale's names (`min=…:tin=…:pin=…:rin=…`), from its
    /// tags or, where untagged, BT.601 (≤ 576 lines) / BT.709 defaults. zscale needs every one
    /// of them to convert (research R2).
    pub colour_in: String,
    /// Frame-rate cap, only when the source is faster than the preset allows.
    pub fps_cap: Option<u32>,
    pub geometry: Geometry,
    /// The reframe actually applied.
    pub reframe: Reframe,
    /// Smart reframe: the crop's starting x and whether any fallback range needs the blurred
    /// background branch. `None` for other reframes (and smart with nothing to pan).
    pub track: Option<Track>,
    /// GPU tier: the Vulkan device index the scaler runs on (`None` = CPU tier).
    pub gpu: Option<u32>,
    /// The source's colour tags in ffmpeg's names (`setparams` options), with the same SD/HD
    /// defaults as `colour_in`, so the GPU scaler converts from the right colours.
    pub params_in: String,
}

/// Smart-reframe parameters of the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Track {
    pub x0: u32,
    pub fallback: bool,
}

/// Restrained sharpening after upscaling: the strongest setting in research R3's range
/// (0.3–0.7). Stronger values raise VMAF but ring on flat areas (measured 2026-10-14).
const CAS: &str = "cas=0.7";
const DENOISE: &str = "hqdn3d=1.5:1.5:6:6";
/// Tone-maps an HDR (PQ/HLG) source to BT.709 SDR; `{in}` is the source's colour properties.
const TONEMAP: &str = "zscale={in}:t=linear:npl=100,format=gbrpf32le,zscale=p=709,tonemap=tonemap=hable:desat=0,\
                       zscale=t=709:m=709:r=limited,format=yuv420p";

/// zscale's input options for a source with these colour tags (ffprobe names), filling in
/// defaults for anything untagged: BT.601 for SD (≤ 576 lines), BT.709 otherwise.
pub fn zscale_input(tags: &ColourTags, coded_height: u32) -> String {
    let sd = coded_height <= 576;
    let matrix = match tags.space.as_deref() {
        Some("bt709") => "709",
        Some("smpte170m") => "170m",
        Some("bt470bg") => "470bg",
        Some("bt2020nc") => "2020_ncl",
        Some("bt2020c") => "2020_cl",
        _ if sd => "470bg",
        _ => "709",
    };
    let transfer = match tags.transfer.as_deref() {
        Some("bt709") => "709",
        Some("smpte170m" | "bt470bg" | "bt470m") => "601",
        Some("smpte2084") => "smpte2084",
        Some("arib-std-b67") => "arib-std-b67",
        Some("iec61966-2-1") => "iec61966-2-1",
        Some("bt2020-10") => "2020_10",
        Some("bt2020-12") => "2020_12",
        Some("linear") => "linear",
        _ if sd => "601",
        _ => "709",
    };
    let primaries = match tags.primaries.as_deref() {
        Some("bt709") => "709",
        Some("smpte170m" | "bt470bg") => "170m",
        Some("bt2020") => "2020",
        _ if sd => "170m",
        _ => "709",
    };
    let range = if tags.range.as_deref() == Some("pc") { "full" } else { "limited" };
    format!("min={matrix}:tin={transfer}:pin={primaries}:rin={range}")
}

/// What a graph is for: the render, or the plain-bicubic baseline the preview compares with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flavor {
    Render,
    /// Same framing and colours, bicubic scaling, no denoise or sharpening (what a plain
    /// upscaler gives; the quality bar of SC-002 is measured against it).
    Baseline,
}

/// Builds the filtergraph.
pub fn build(g: &GraphInput) -> String {
    build_with(g, Flavor::Render)
}

/// The inputs for one still frame at a fixed crop: smart reframe at crop x `x` (centred when
/// `None`), or the blurred background when `blurred`. No trim and no frame-rate cap (the
/// still is input-seeked); the CPU scaler when `cpu`.
pub fn still_input(g: &GraphInput, x: Option<u32>, blurred: bool) -> GraphInput {
    let mut s = g.clone();
    s.trim = None;
    s.fps_cap = None;
    s.track = None;
    if blurred {
        s.geometry = super::geometry::plan(g.geometry.display, g.geometry.target, Reframe::Blur);
        s.reframe = Reframe::Blur;
    } else if let (Some(x), Some(c)) = (x, s.geometry.crop.as_mut()) {
        c.x = x.min(g.geometry.display.0.saturating_sub(c.w));
    }
    s
}

/// One graph giving both preview stills from `[0:v]`: `[after]` (the render's processing) and
/// `[before]` (the bicubic baseline, always on the CPU), each converted to full-range RGB with
/// the output's matrix for PNG.
pub fn preview_pair(g: &GraphInput) -> String {
    let mut base = g.clone();
    base.gpu = None;
    let matrix = if g.keep_hdr { "bt2020" } else { "bt709" };
    let rgb = format!(",scale=in_color_matrix={matrix}:in_range=tv:out_range=pc,format=rgb24");
    // Both graphs use the same internal labels: each gets its own prefix.
    let relabel = |graph: String, prefix: &str, to: &str| {
        let graph = prefix_labels(&graph, prefix);
        let cut = graph.rfind(&format!("[{prefix}v]")).expect("graph ends in [v]");
        format!("{}{rgb}[{to}]", &graph[..cut])
    };
    format!(
        "[0:v]split[a_in][b_in];{};{}",
        relabel(build_with(g, Flavor::Render), "a_", "after"),
        relabel(build_with(&base, Flavor::Baseline), "b_", "before")
    )
}

/// Prefixes every link label (`[x]`) of `graph` with `prefix`; `[0:v]` becomes `[{prefix}in]`.
fn prefix_labels(graph: &str, prefix: &str) -> String {
    let mut out = String::with_capacity(graph.len() + 32);
    let mut rest = graph;
    while let Some(open) = rest.find('[') {
        let close = open + rest[open..].find(']').expect("labels are closed");
        let name = &rest[open + 1..close];
        out.push_str(&rest[..open]);
        if name == "0:v" {
            out.push_str(&format!("[{prefix}in]"));
        } else {
            out.push_str(&format!("[{prefix}{name}]"));
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

fn build_with(g: &GraphInput, flavor: Flavor) -> String {
    let pre = pre_chain(g, flavor);
    let (w, h) = g.geometry.target;
    let pix = if g.keep_hdr { "yuv420p10le" } else { "yuv420p" };
    let overlay_fmt = if g.keep_hdr { ":format=yuv420p10" } else { "" };
    let tail = format!("setsar=1,format={pix}");
    let overflow = g.geometry.overflow != Overflow::None;
    match (g.reframe, &g.track, g.geometry.crop) {
        // Smart reframe: a crop that sendcmd moves along the path; where no subject fits, the
        // blurred-background picture is switched on over it (research R5).
        (Reframe::Smart, Some(t), Some(c)) if overflow => {
            let crop = format!(
                "sendcmd=f=reframe.cmd,crop@trk=w={}:h={}:x={}:y={},{}",
                c.w,
                c.h,
                t.x0,
                c.y,
                scaler(g, w, h, flavor)
            );
            if !t.fallback {
                return format!("[0:v]{pre},{crop},{tail}[v]");
            }
            format!(
                "[0:v]{pre},split[a][b];[a]{crop}[crop];{};[crop][pad]overlay@fb=0:0:enable=0{overlay_fmt},{tail}[v]",
                blur_pad(g, "[b]", "[pad]", flavor)
            )
        }
        (Reframe::Blur, _, _) if overflow => {
            blur_pad(g, &format!("[0:v]{pre},"), &format!(",{tail}[v]"), flavor)
        }
        _ => {
            let mut chain = vec![pre];
            if let Some(c) = g.geometry.crop {
                chain.push(format!("crop={}:{}:{}:{}", c.w, c.h, c.x, c.y));
            }
            chain.push(scaler(g, w, h, flavor));
            chain.push(tail);
            format!("[0:v]{}[v]", chain.join(","))
        }
    }
}

/// The whole frame fitted inside the target over a blurred, darkened copy that fills it.
/// `head` feeds the picture in (a label or a filter chain ending in `,`); `end` follows the
/// final overlay (an output label, or more filters and a label).
fn blur_pad(g: &GraphInput, head: &str, end: &str, flavor: Flavor) -> String {
    let (w, h) = g.geometry.target;
    let (fw, fh) = g.geometry.fit;
    let (bw, bh) = (even(f64::from(w) / 10.0), even(f64::from(h) / 10.0));
    let (ox, oy) = ((w - fw) / 2 / 2 * 2, (h - fh) / 2 / 2 * 2);
    // The background gets the same colour conversion as the foreground (at 1/10 size, cheap),
    // and overlay keeps 10 bits when HDR is kept (it defaults to 8-bit yuv420).
    let bg_colour = colour_tail(g);
    let overlay_fmt = if g.keep_hdr { ":format=yuv420p10" } else { "" };
    format!(
        "{head}split[bg0][fg0];\
         [bg0]scale={bw}:{bh}:force_original_aspect_ratio=increase,crop={bw}:{bh}{bg_colour},gblur=sigma=3,\
         eq=brightness=-0.08,scale={w}:{h},setsar=1[bg];\
         [fg0]{}[fg];\
         [bg][fg]overlay={ox}:{oy}{overlay_fmt}{end}",
        scaler(g, fw, fh, flavor)
    )
}

/// The analysis decode graph (research R6): the clip at analysis size, scene-cut scores
/// printed to `cuts.txt` (relative to the working directory) and 3 fps bgr24 frames on `[raw]`.
/// No denoise or frame-rate cap: they do not change where things are.
pub fn analysis_graph(g: &GraphInput, size: (u32, u32)) -> String {
    let mut f: Vec<String> = Vec::new();
    match g.trim {
        Some((start, end)) => {
            let end = end.map(|e| format!(":end={}", e.ffmpeg())).unwrap_or_default();
            f.push(format!("trim=start={}{end},setpts=PTS-STARTPTS", start.ffmpeg()));
        }
        None => f.push("setpts=PTS-STARTPTS".into()),
    }
    if g.deinterlace {
        f.push("bwdif=mode=send_frame".into());
    }
    f.push(format!("scale={}:{}:flags=area,setsar=1", size.0, size.1));
    if g.tonemap || g.keep_hdr {
        f.push(TONEMAP.replace("{in}", &g.colour_in));
    }
    format!(
        "[0:v]{},split[s][d];[s]scdet=t=10,metadata=mode=select:key=lavfi.scd.score:value=2.999:function=greater,metadata=mode=print:key=lavfi.scd.score:file=cuts.txt,nullsink;\
         [d]fps=3,format=bgr24[raw]",
        f.join(",")
    )
}

/// Trim, deinterlace, square pixels, tone-map, denoise, frame-rate cap — at source resolution.
fn pre_chain(g: &GraphInput, flavor: Flavor) -> String {
    let mut f: Vec<String> = Vec::new();
    if let Some((start, end)) = g.trim {
        let end = end.map(|e| format!(":end={}", e.ffmpeg())).unwrap_or_default();
        f.push(format!("trim=start={}{end},setpts=PTS-STARTPTS", start.ffmpeg()));
    }
    if g.deinterlace {
        f.push("bwdif=mode=send_frame".into());
    }
    if g.square_pixels {
        let (w, h) = g.geometry.display;
        f.push(format!("scale={w}:{h},setsar=1"));
    }
    // On the GPU tier, libplacebo tone-maps while it scales.
    if g.tonemap && g.gpu.is_none() {
        f.push(TONEMAP.replace("{in}", &g.colour_in));
    }
    if flavor == Flavor::Render {
        f.push(DENOISE.into());
    }
    if let Some(fps) = g.fps_cap {
        f.push(format!("fps={fps}"));
    }
    // A filter chain cannot be empty (the baseline of a plain source has nothing to do here).
    if f.is_empty() {
        f.push("null".into());
    }
    f.join(",")
}

/// zscale colour options converting to BT.709 limited (empty when keeping HDR).
fn colour_options(g: &GraphInput) -> String {
    if g.keep_hdr {
        return String::new();
    }
    // After tone-mapping the frames are BT.709 already.
    let input = if g.tonemap { "min=709:tin=709:pin=709:rin=limited" } else { g.colour_in.as_str() };
    format!(":{input}:m=709:t=709:p=709:r=limited")
}

/// A colour-only zscale step (for the blurred background), or nothing when keeping HDR.
fn colour_tail(g: &GraphInput) -> String {
    if g.tonemap && g.gpu.is_some() {
        // The pre chain did not tone-map (libplacebo does it for the picture): the background
        // is tone-mapped here, at 1/10 size.
        return format!(",{}", TONEMAP.replace("{in}", &g.colour_in));
    }
    let opts = colour_options(g);
    if opts.is_empty() { String::new() } else { format!(",zscale={}", &opts[1..]) }
}

/// The CPU-tier scaler to `w`x`h`: zscale Lanczos with explicit BT.709 output (or untouched
/// colours when keeping HDR), then sharpening when enlarging. Lanczos with `cas=0.7` measured
/// best within research R3's range (+2.8 VMAF over bicubic on the 480p fixture).
fn scaler(g: &GraphInput, w: u32, h: u32, flavor: Flavor) -> String {
    if flavor == Flavor::Baseline {
        return format!("zscale=w={w}:h={h}:f=bicubic{}", colour_options(g));
    }
    if let Some(_device) = g.gpu {
        return gpu_scaler(g, w, h);
    }
    let mut s = format!("zscale=w={w}:h={h}:f=lanczos");
    s.push_str(&colour_options(g));
    if !g.geometry.no_upscale_needed {
        s.push(',');
        s.push_str(CAS);
    }
    s
}

/// The GPU-tier scaler (research R9): the frame goes to the Vulkan device (`-filter_hw_device`),
/// libplacebo scales it with `ewa_lanczossharp` in gamma light (`sigmoid=0`), converts to BT.709
/// (tone-mapping HDR with BT.2390) or keeps HDR, and it comes back for the same `cas` as the CPU
/// tier when enlarging. Measured on the 480p fixture: 57.65 VMAF against the CPU tier's 57.63;
/// the libplacebo defaults (sigmoid upscaling, no `cas`) scored 53.9, under bicubic.
fn gpu_scaler(g: &GraphInput, w: u32, h: u32) -> String {
    let hdr_in = g.tonemap || g.keep_hdr;
    let upload = if hdr_in { "p010le" } else { "yuv420p" };
    let out = if g.keep_hdr { "yuv420p10le" } else { "yuv420p" };
    let colour = match (g.keep_hdr, g.tonemap) {
        (true, _) => String::new(),
        (false, tonemap) => format!(
            ":colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv{}",
            if tonemap { ":tonemapping=bt.2390:peak_detect=0" } else { "" }
        ),
    };
    format!(
        "setparams={},format={upload},hwupload,libplacebo=w={w}:h={h}:upscaler=ewa_lanczossharp:sigmoid=0:\
         format={out}{colour},hwdownload,format={out}{}",
        g.params_in,
        if g.geometry.no_upscale_needed { String::new() } else { format!(",{CAS}") }
    )
}

/// `setparams` options for a source with these colour tags (ffprobe names), filling in defaults
/// for anything untagged: BT.601 for SD (≤ 576 lines), BT.709 otherwise.
pub fn setparams_input(tags: &ColourTags, coded_height: u32) -> String {
    let sd = coded_height <= 576;
    let space = tags.space.clone().unwrap_or_else(|| if sd { "bt470bg" } else { "bt709" }.into());
    let primaries = tags.primaries.clone().unwrap_or_else(|| if sd { "smpte170m" } else { "bt709" }.into());
    let trc = tags.transfer.clone().unwrap_or_else(|| if sd { "smpte170m" } else { "bt709" }.into());
    let range = tags.range.clone().unwrap_or_else(|| "tv".into());
    format!("colorspace={space}:color_primaries={primaries}:color_trc={trc}:range={range}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upscale::geometry::plan;

    fn input(display: (u32, u32), target: (u32, u32), reframe: Reframe) -> GraphInput {
        GraphInput {
            trim: None,
            deinterlace: false,
            square_pixels: false,
            tonemap: false,
            keep_hdr: false,
            colour_in: zscale_input(&ColourTags::bt709(), 1080),
            fps_cap: None,
            geometry: plan(display, target, reframe),
            reframe,
            track: None,
            gpu: None,
            params_in: setparams_input(&ColourTags::bt709(), 1080),
        }
    }

    fn gpu(mut g: GraphInput) -> GraphInput {
        g.gpu = Some(0);
        g
    }

    #[test]
    fn gpu_tier_scales_with_libplacebo_on_the_vulkan_device() {
        insta::assert_snapshot!(build(&gpu(input((480, 854), (1080, 2340), Reframe::Center))), @"[0:v]hqdn3d=1.5:1.5:6:6,crop=394:854:44:0,setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv,format=yuv420p,hwupload,libplacebo=w=1080:h=2340:upscaler=ewa_lanczossharp:sigmoid=0:format=yuv420p:colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv,hwdownload,format=yuv420p,cas=0.7,setsar=1,format=yuv420p[v]");
    }

    #[test]
    fn gpu_tier_hdr_tone_maps_in_libplacebo_and_keeps_10_bit() {
        let mut g = gpu(input((640, 360), (1080, 2340), Reframe::Blur));
        g.tonemap = true;
        let sdr = build(&g);
        assert!(
            sdr.contains("format=p010le,hwupload,libplacebo=")
                && sdr.contains(":tonemapping=bt.2390:peak_detect=0,"),
            "{sdr}"
        );
        assert_eq!(
            sdr.matches("tonemap=tonemap=hable").count(),
            1,
            "only the background is tone-mapped on the CPU: {sdr}"
        );
        let mut g = gpu(input((640, 360), (1080, 2340), Reframe::Center));
        g.keep_hdr = true;
        let keep = build(&g);
        assert!(keep.contains("format=yuv420p10le,hwdownload,format=yuv420p10le"), "{keep}");
        assert!(
            !keep.contains("upscaler=ewa_lanczossharp:format=yuv420p10le:colorspace"),
            "colours pass through: {keep}"
        );
    }

    #[test]
    fn untagged_sources_get_sd_or_hd_defaults() {
        assert_eq!(
            setparams_input(&ColourTags::default(), 480),
            "colorspace=bt470bg:color_primaries=smpte170m:color_trc=smpte170m:range=tv"
        );
        assert_eq!(
            setparams_input(&ColourTags::default(), 1080),
            "colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv"
        );
    }

    fn smart(fallback: bool) -> GraphInput {
        let mut g = input((1280, 720), (1080, 2340), Reframe::Smart);
        g.track = Some(Track { x0: 470, fallback });
        g
    }

    #[test]
    fn smart_without_fallback_is_one_moving_crop() {
        insta::assert_snapshot!(build(&smart(false)), @"[0:v]hqdn3d=1.5:1.5:6:6,sendcmd=f=reframe.cmd,crop@trk=w=332:h=720:x=470:y=0,zscale=w=1080:h=2340:f=lanczos:min=709:tin=709:pin=709:rin=limited:m=709:t=709:p=709:r=limited,cas=0.7,setsar=1,format=yuv420p[v]");
    }

    #[test]
    fn smart_with_fallback_switches_the_blurred_background_on() {
        insta::assert_snapshot!(build(&smart(true)), @"[0:v]hqdn3d=1.5:1.5:6:6,split[a][b];[a]sendcmd=f=reframe.cmd,crop@trk=w=332:h=720:x=470:y=0,zscale=w=1080:h=2340:f=lanczos:min=709:tin=709:pin=709:rin=limited:m=709:t=709:p=709:r=limited,cas=0.7[crop];[b]split[bg0][fg0];[bg0]scale=108:234:force_original_aspect_ratio=increase,crop=108:234,zscale=min=709:tin=709:pin=709:rin=limited:m=709:t=709:p=709:r=limited,gblur=sigma=3,eq=brightness=-0.08,scale=1080:2340,setsar=1[bg];[fg0]zscale=w=1080:h=608:f=lanczos:min=709:tin=709:pin=709:rin=limited:m=709:t=709:p=709:r=limited,cas=0.7[fg];[bg][fg]overlay=0:866[pad];[crop][pad]overlay@fb=0:0:enable=0,setsar=1,format=yuv420p[v]");
        let mut keep = smart(true);
        keep.keep_hdr = true;
        let k = build(&keep);
        assert!(
            k.contains("overlay=0:866:format=yuv420p10[pad]")
                && k.contains("overlay@fb=0:0:enable=0:format=yuv420p10,"),
            "{k}"
        );
    }

    #[test]
    fn smart_with_nothing_to_pan_is_a_plain_scale() {
        let g = input((480, 854), (1080, 1920), Reframe::Smart);
        assert!(!build(&g).contains("sendcmd"));
    }

    #[test]
    fn analysis_graph_prints_cuts_and_samples_three_frames_a_second() {
        let mut g = input((1280, 720), (1080, 2340), Reframe::Smart);
        g.trim = Some((Ts::from_secs_f64(2.0), Some(Ts::from_secs_f64(5.0))));
        insta::assert_snapshot!(analysis_graph(&g, (576, 324)), @"[0:v]trim=start=2.000000:end=5.000000,setpts=PTS-STARTPTS,scale=576:324:flags=area,setsar=1,split[s][d];[s]scdet=t=10,metadata=mode=select:key=lavfi.scd.score:value=2.999:function=greater,metadata=mode=print:key=lavfi.scd.score:file=cuts.txt,nullsink;[d]fps=3,format=bgr24[raw]");
    }

    #[test]
    fn center_crop_portrait_into_phone_fill() {
        insta::assert_snapshot!(build(&input((480, 854), (1080, 2340), Reframe::Center)), @"[0:v]hqdn3d=1.5:1.5:6:6,crop=394:854:44:0,zscale=w=1080:h=2340:f=lanczos:min=709:tin=709:pin=709:rin=limited:m=709:t=709:p=709:r=limited,cas=0.7,setsar=1,format=yuv420p[v]");
    }

    #[test]
    fn same_shape_needs_no_crop() {
        insta::assert_snapshot!(build(&input((480, 854), (1080, 1920), Reframe::Smart)), @"[0:v]hqdn3d=1.5:1.5:6:6,zscale=w=1080:h=1920:f=lanczos:min=709:tin=709:pin=709:rin=limited:m=709:t=709:p=709:r=limited,cas=0.7,setsar=1,format=yuv420p[v]");
    }

    #[test]
    fn whole_video_blur_pad() {
        insta::assert_snapshot!(build(&input((1920, 1080), (1080, 2340), Reframe::Blur)), @"[0:v]hqdn3d=1.5:1.5:6:6,split[bg0][fg0];[bg0]scale=108:234:force_original_aspect_ratio=increase,crop=108:234,zscale=min=709:tin=709:pin=709:rin=limited:m=709:t=709:p=709:r=limited,gblur=sigma=3,eq=brightness=-0.08,scale=1080:2340,setsar=1[bg];[fg0]zscale=w=1080:h=608:f=lanczos:min=709:tin=709:pin=709:rin=limited:m=709:t=709:p=709:r=limited[fg];[bg][fg]overlay=0:866,setsar=1,format=yuv420p[v]");
    }

    #[test]
    fn no_upscale_is_a_plain_downscale_without_sharpening() {
        let g = build(&input((2160, 3840), (1080, 1920), Reframe::Center));
        assert!(g.contains("zscale=w=1080:h=1920:f=lanczos") && !g.contains("cas="), "{g}");
    }

    #[test]
    fn pre_processing_order_and_untagged_sd_matrix() {
        let mut g = input((1024, 576), (1080, 1350), Reframe::Center);
        g.trim = Some((Ts::from_secs_f64(1.5), Some(Ts::from_secs_f64(4.0))));
        g.deinterlace = true;
        g.square_pixels = true;
        g.fps_cap = Some(60);
        g.colour_in = zscale_input(&ColourTags::default(), 576);
        insta::assert_snapshot!(build(&g), @"[0:v]trim=start=1.500000:end=4.000000,setpts=PTS-STARTPTS,bwdif=mode=send_frame,scale=1024:576,setsar=1,hqdn3d=1.5:1.5:6:6,fps=60,crop=460:576:282:0,zscale=w=1080:h=1350:f=lanczos:min=470bg:tin=601:pin=170m:rin=limited:m=709:t=709:p=709:r=limited,cas=0.7,setsar=1,format=yuv420p[v]");
    }

    #[test]
    fn colour_input_from_tags_with_sd_and_hd_defaults() {
        assert_eq!(zscale_input(&ColourTags::bt709(), 1080), "min=709:tin=709:pin=709:rin=limited");
        assert_eq!(zscale_input(&ColourTags::default(), 480), "min=470bg:tin=601:pin=170m:rin=limited");
        assert_eq!(zscale_input(&ColourTags::default(), 720), "min=709:tin=709:pin=709:rin=limited");
        let hdr = ColourTags {
            space: Some("bt2020nc".into()),
            primaries: Some("bt2020".into()),
            transfer: Some("smpte2084".into()),
            range: Some("pc".into()),
        };
        assert_eq!(zscale_input(&hdr, 2160), "min=2020_ncl:tin=smpte2084:pin=2020:rin=full");
        let partial = ColourTags { space: Some("smpte170m".into()), ..ColourTags::default() };
        assert_eq!(zscale_input(&partial, 1080), "min=170m:tin=709:pin=709:rin=limited");
    }

    #[test]
    fn blur_pad_keeps_10_bits_and_source_colours_when_keeping_hdr() {
        let mut g = input((1920, 1080), (1080, 2340), Reframe::Blur);
        g.keep_hdr = true;
        let blur = build(&g);
        assert!(blur.contains("overlay=0:866:format=yuv420p10,"), "overlay must stay 10-bit: {blur}");
        assert!(!blur.contains("zscale=min"), "no colour conversion when keeping HDR: {blur}");
    }

    #[test]
    fn hdr_to_sdr_tone_maps_and_keep_stays_10_bit() {
        let mut g = input((640, 360), (1080, 2340), Reframe::Center);
        g.tonemap = true;
        let sdr = build(&g);
        assert!(sdr.contains("tonemap=tonemap=hable") && sdr.ends_with("format=yuv420p[v]"), "{sdr}");
        let mut g = input((640, 360), (1080, 2340), Reframe::Center);
        g.keep_hdr = true;
        let keep = build(&g);
        assert!(
            !keep.contains("tonemap") && !keep.contains(":m=709") && keep.ends_with("format=yuv420p10le[v]"),
            "{keep}"
        );
    }

    #[test]
    fn still_input_fixes_the_crop_or_uses_the_background() {
        let mut g = smart(true);
        g.trim = Some((Ts::from_secs_f64(2.0), None));
        g.fps_cap = Some(30);
        let at = still_input(&g, Some(1100), false);
        assert_eq!((at.trim, at.fps_cap, at.track), (None, None, None));
        let c = at.geometry.crop.unwrap();
        assert_eq!(c.x, 1280 - c.w, "x is clamped to the frame");
        assert!(build(&at).contains(&format!("crop={}:{}:{}:0", c.w, c.h, c.x)), "{}", build(&at));
        let centred = still_input(&g, None, false);
        assert_eq!(centred.geometry.crop, g.geometry.crop);
        let blurred = still_input(&g, Some(10), true);
        assert_eq!(blurred.reframe, Reframe::Blur);
        assert!(build(&blurred).contains("gblur"));
    }

    #[test]
    fn preview_pair_gives_the_render_and_a_bicubic_baseline() {
        let cropped = preview_pair(&gpu(still_input(&smart(false), Some(300), false)));
        let after = &cropped[..cropped.find(";[b_in]").unwrap()];
        assert!(after.contains("crop=332:720:300:0") && after.contains("libplacebo"), "{after}");
        assert!(after.contains("cas=0.7"), "enlarging is sharpened: {after}");
        let before = &cropped[cropped.rfind("[b_in]").unwrap()..];
        assert!(before.contains("crop=332:720:300:0,zscale=w=1080:h=2340:f=bicubic"), "{before}");

        let g = gpu(still_input(&smart(false), Some(300), true));
        let pair = preview_pair(&g);
        assert!(pair.starts_with("[0:v]split[a_in][b_in];[a_in]hqdn3d"), "{pair}");
        // The baseline: no denoise, no sharpening, zscale bicubic on the CPU, same framing.
        let before = &pair[pair.rfind("[b_in]").unwrap()..];
        assert!(
            !before.contains("hqdn3d") && !before.contains("cas=") && !before.contains("libplacebo"),
            "{before}"
        );
        assert!(before.contains("zscale=w=1080:h=608:f=bicubic:min=709"), "{before}");
        assert!(before.starts_with("[b_in]null,split[b_bg0][b_fg0];"), "{before}");
        assert!(before.contains("[b_bg][b_fg]overlay"), "labels are prefixed: {before}");
        for out in ["[after]", "[before]"] {
            assert_eq!(pair.matches(out).count(), 1, "{pair}");
            let end = pair.find(out).unwrap();
            assert!(
                pair[..end].ends_with("scale=in_color_matrix=bt709:in_range=tv:out_range=pc,format=rgb24"),
                "{pair}"
            );
        }
        assert!(!pair.contains("[v]") && !pair.contains("[0:v]["), "{pair}");
    }
}
