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
    /// The source has an alpha plane: the GIF chain flattens it onto opaque black first
    /// (feature 003, research R15). The video flavours ignore it.
    pub flatten_alpha: bool,
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
    /// The AI tier's source-side chain: no denoise (the model removes compression noise
    /// itself) and a constant frame rate, so ffmpeg #2 can time the worker's raw frames.
    Ai,
    /// The GIF frame chain (feature 003, refactor B1): the same trim, pre-processing and
    /// framing, a constant frame rate, one swscale Lanczos resample straight to the GIF size,
    /// and full-range RGB out for the palette. No denoise, sharpening or video tail.
    Gif,
}

/// swscale flags of the GIF resample (research R1).
const GIF_SCALER: &str = "lanczos+accurate_rnd+full_chroma_int";
/// Flattens a source with alpha onto opaque black: the base is a copy of the source (so it has
/// its timing) without the alpha plane, painted black (research R15, T005 e).
const FLATTEN_ALPHA: &str =
    "split[fa][fb];[fa]format=yuv420p,drawbox=c=black:t=fill[fk];[fk][fb]overlay=format=auto";

/// What the GIF frame chain needs besides [`GraphInput`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GifChain<'a> {
    /// The constant output frame rate (`n/d`): the target's, capped at the source's.
    pub rate: &'a str,
    /// `unsharp` luma amount after the resample (flat-ui).
    pub sharpen: Option<f32>,
    /// The Reddit MP4: end in BT.709 limited-range `yuv420p` instead of full-range RGB.
    pub mp4: bool,
}

/// The GIF frame chain (refactor B1): `[0:v]` → frames at the GIF size in full-range RGB. It
/// ends in an open chain, so callers append `,palettegen=…` or `[x];[x][1:v]paletteuse=…`.
/// `geometry.target` is the GIF size, planned with `W = min(target width, crop width)`.
pub fn frame_chain(g: &GraphInput, c: &GifChain<'_>) -> String {
    let pre = pre_chain_at(g, Flavor::Gif, Some(c.rate));
    let (w, h) = g.geometry.target;
    let sharpen = c.sharpen.map(|a| format!(",unsharp=5:5:{a}:5:5:0")).unwrap_or_default();
    let (rgb, pix) = if c.mp4 {
        // The frames carry the tags too: the encoder takes their (unset) primaries and transfer
        // over its own -color_* options.
        (
            gif_rgb(g).replace(":out_range=pc", ":out_color_matrix=bt709:out_range=tv"),
            "yuv420p,setparams=range=tv:color_primaries=bt709:color_trc=bt709:colorspace=bt709",
        )
    } else {
        (gif_rgb(g), "rgb24")
    };
    let to_rgb = format!("{sharpen},scale=flags=accurate_rnd+full_chroma_int:{rgb},format={pix}");
    let overflow = g.geometry.overflow != Overflow::None;
    match (g.reframe, &g.track, g.geometry.crop) {
        (Reframe::Smart, Some(t), Some(cr)) if overflow => {
            let crop = format!(
                "sendcmd=f=reframe.cmd,crop@trk=w={}:h={}:x={}:y={},{}",
                cr.w,
                cr.h,
                t.x0,
                cr.y,
                scaler(g, w, h, Flavor::Gif)
            );
            if !t.fallback {
                return format!("[0:v]{pre},{crop}{to_rgb}");
            }
            format!(
                "[0:v]{pre},split[a][b];[a]{crop}[crop];{};[crop][pad]overlay@fb=0:0:enable=0{to_rgb}",
                blur_pad(g, "[b]", "[pad]", Flavor::Gif)
            )
        }
        (Reframe::Blur, _, _) if overflow => blur_pad(g, &format!("[0:v]{pre},"), &to_rgb, Flavor::Gif),
        _ => {
            let mut chain = vec![pre];
            if let Some(cr) = g.geometry.crop {
                chain.push(format!("crop={}:{}:{}:{}", cr.w, cr.h, cr.x, cr.y));
            }
            // One swscale pass resamples and converts to RGB, unless sharpening sits between.
            if c.sharpen.is_some() {
                chain.push(format!("{}{to_rgb}", scaler(g, w, h, Flavor::Gif)));
            } else {
                chain.push(format!("scale={w}:{h}:flags={GIF_SCALER}:{rgb},format={pix}"));
            }
            format!("[0:v]{}", chain.join(","))
        }
    }
}

/// swscale's input colour options for the GIF's RGB conversion: the source matrix and range
/// (from `colour_in`, which already has the SD/HD defaults), or BT.709 limited after
/// tone-mapping.
fn gif_rgb(g: &GraphInput) -> String {
    let field = |key: &str| g.colour_in.split(':').find_map(|kv| kv.strip_prefix(key)).unwrap_or_default();
    let (matrix, range) = if g.tonemap {
        ("bt709", "tv")
    } else {
        let matrix = match field("min=") {
            "170m" | "470bg" => "bt601",
            "2020_ncl" | "2020_cl" => "bt2020",
            _ => "bt709",
        };
        (matrix, if field("rin=") == "full" { "pc" } else { "tv" })
    };
    format!("in_color_matrix={matrix}:in_range={range}:out_range=pc")
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
    let (fw, fh) = g.geometry.fit;
    let (ox, oy) = fit_offset(g);
    // The background gets the same colour conversion as the foreground (at 1/10 size, cheap),
    // and overlay keeps 10 bits when HDR is kept (it defaults to 8-bit yuv420).
    let overlay_fmt = if g.keep_hdr { ":format=yuv420p10" } else { "" };
    format!(
        "{head}split[bg0][fg0];[bg0]{}[bg];[fg0]{}[fg];[bg][fg]overlay={ox}:{oy}{overlay_fmt}{end}",
        blur_background(g, flavor),
        scaler(g, fw, fh, flavor)
    )
}

/// The blurred, darkened copy filling the target: scaled to 1/10 size (cheap), converted like
/// the foreground, blurred, darkened and scaled back up. The GIF flavour keeps the source's
/// colours (both layers are converted to RGB together after the overlay).
fn blur_background(g: &GraphInput, flavor: Flavor) -> String {
    let (w, h) = g.geometry.target;
    let (bw, bh) = (even(f64::from(w) / 10.0), even(f64::from(h) / 10.0));
    let colour = if flavor == Flavor::Gif { String::new() } else { colour_tail(g) };
    format!(
        "scale={bw}:{bh}:force_original_aspect_ratio=increase,crop={bw}:{bh}{colour},gblur=sigma=3,\
         eq=brightness=-0.08,scale={w}:{h},setsar=1"
    )
}

/// Where the foreground sits on the blurred background.
fn fit_offset(g: &GraphInput) -> (u32, u32) {
    let (w, h) = g.geometry.target;
    let (fw, fh) = g.geometry.fit;
    ((w - fw) / 2 / 2 * 2, (h - fh) / 2 / 2 * 2)
}

/// `sendcmd` files of the AI tier: the crop moves in ffmpeg #1, the fallback overlay is
/// switched in ffmpeg #2 (each ffmpeg gets only its own filter's commands).
pub const AI_CROP_CMD: &str = "reframe-crop.cmd";
pub const AI_FALLBACK_CMD: &str = "reframe-fallback.cmd";

/// Temporal smoothing after the AI scale (FR-011), chosen by measurement (T104, research R13):
/// temporal-only `hqdn3d`, which backs off where pixels change a lot. On a static, noisy source
/// it cut the frame-to-frame difference by 35 % (1.10 → 0.72) and cost 0.07 VMAF on moving
/// content; a 3-frame `tmix` cut it by 45 % but cost 17 VMAF (ghosting), and a motion-masked
/// `tmix` drops the first frame (`tblend`).
const AI_DEFLICKER: Option<&str> = Some("hqdn3d=0:0:3:3");

/// AI tier, ffmpeg #1 (research R13): `[0:v]` → `[v]`, the framed picture at source scale as
/// full-range BT.709 RGB at the constant frame `rate` (`n/d`), for the worker. Smart reframe
/// moves the crop with `reframe-crop.cmd`; the blurred background is added by ffmpeg #2, so
/// the worker only upscales the picture itself.
pub fn ai_decode(g: &GraphInput, rate: &str) -> String {
    let pre = pre_chain_at(g, Flavor::Ai, Some(rate));
    let overflow = g.geometry.overflow != Overflow::None;
    let framing = match (g.reframe, &g.track, g.geometry.crop) {
        (Reframe::Smart, Some(t), Some(c)) if overflow => {
            Some(format!("sendcmd=f={AI_CROP_CMD},crop@trk=w={}:h={}:x={}:y={}", c.w, c.h, t.x0, c.y))
        }
        (Reframe::Blur, _, _) if overflow => None,
        (_, _, Some(c)) => Some(format!("crop={}:{}:{}:{}", c.w, c.h, c.x, c.y)),
        _ => None,
    };
    // After tone-mapping the frames are BT.709 already.
    let input = if g.tonemap { "min=709:tin=709:pin=709:rin=limited" } else { g.colour_in.as_str() };
    let st = super::ai::stage(g);
    let size =
        if st.shrunk { format!("w={}:h={}:f=lanczos:", st.input.0, st.input.1) } else { String::new() };
    let rgb = format!("zscale={size}{input}:p=709:t=709:r=full,format=gbrp,format=rgb24");
    let chain: Vec<String> = std::iter::once(pre).chain(framing).chain(std::iter::once(rgb)).collect();
    format!("[0:v]{}[v]", chain.join(","))
}

/// AI tier, ffmpeg #2: the worker's frames `[1:v]` (RGB at `stage.output()`) scaled to the
/// target (or the fitted foreground) as BT.709 limited-range video, over the blurred background
/// made from the source `[0:v]` where the reframe needs one. Smart reframe's fallback ranges
/// show the whole frame classically scaled over the background (`reframe-fallback.cmd`).
pub fn ai_finish(g: &GraphInput, stage: &super::ai::AiStage, rate: &str) -> String {
    let tail = "setsar=1,format=yuv420p";
    let to = |(w, h): (u32, u32)| {
        let mut s =
            format!("zscale=w={w}:h={h}:f=lanczos:tin=709:pin=709:rin=full:m=709:t=709:p=709:r=limited");
        if let Some(d) = AI_DEFLICKER {
            s.push(',');
            s.push_str(d);
        }
        s
    };
    let overflow = g.geometry.overflow != Overflow::None;
    let source = pre_chain_at(g, Flavor::Ai, Some(rate));
    match (g.reframe, &g.track) {
        (Reframe::Smart, Some(t)) if overflow && t.fallback && g.geometry.crop.is_some() => format!(
            "[1:v]sendcmd=f={AI_FALLBACK_CMD},{}[crop];{};[crop][pad]overlay@fb=0:0:enable=0,{tail}[v]",
            to(g.geometry.target),
            blur_pad(g, &format!("[0:v]{source},"), "[pad]", Flavor::Render)
        ),
        (Reframe::Blur, _) if overflow => {
            let (ox, oy) = fit_offset(g);
            format!(
                "[1:v]{}[fg];[0:v]{source},{}[bg];[bg][fg]overlay={ox}:{oy},{tail}[v]",
                to(stage.fit),
                blur_background(g, Flavor::Render)
            )
        }
        _ => format!("[1:v]{},{tail}[v]", to(g.geometry.target)),
    }
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
    pre_chain_at(g, flavor, None)
}

/// [`pre_chain`]; the AI flavour forces the constant frame `rate` (`n/d`).
fn pre_chain_at(g: &GraphInput, flavor: Flavor, rate: Option<&str>) -> String {
    let mut f: Vec<String> = Vec::new();
    if let Some((start, end)) = g.trim {
        let end = end.map(|e| format!(":end={}", e.ffmpeg())).unwrap_or_default();
        f.push(format!("trim=start={}{end},setpts=PTS-STARTPTS", start.ffmpeg()));
    }
    if flavor == Flavor::Gif && g.flatten_alpha {
        f.push(FLATTEN_ALPHA.into());
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
    match (flavor, rate) {
        (Flavor::Ai | Flavor::Gif, Some(r)) => f.push(format!("fps={r}")),
        _ => {
            if let Some(fps) = g.fps_cap {
                f.push(format!("fps={fps}"));
            }
        }
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
    if flavor == Flavor::Gif {
        return format!("scale={w}:{h}:flags={GIF_SCALER}");
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
            flatten_alpha: false,
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

    #[test]
    fn ai_graphs_frame_at_source_scale_and_compose_after_the_worker() {
        use crate::upscale::ai::stage;
        let rate = "30000/1001";
        let center = input((480, 854), (1080, 2340), Reframe::Center);
        insta::assert_snapshot!(ai_decode(&center, rate), @"[0:v]fps=30000/1001,crop=394:854:44:0,zscale=min=709:tin=709:pin=709:rin=limited:p=709:t=709:r=full,format=gbrp,format=rgb24[v]");
        insta::assert_snapshot!(ai_finish(&center, &stage(&center), rate), @"[1:v]zscale=w=1080:h=2340:f=lanczos:tin=709:pin=709:rin=full:m=709:t=709:p=709:r=limited,hqdn3d=0:0:3:3,setsar=1,format=yuv420p[v]");
        // Smart reframe: the crop moves in ffmpeg #1, the fallback switches in ffmpeg #2 (whole
        // frame, classically scaled, over the blurred background).
        let s = smart(true);
        insta::assert_snapshot!(ai_decode(&s, rate), @"[0:v]fps=30000/1001,sendcmd=f=reframe-crop.cmd,crop@trk=w=332:h=720:x=470:y=0,zscale=min=709:tin=709:pin=709:rin=limited:p=709:t=709:r=full,format=gbrp,format=rgb24[v]");
        insta::assert_snapshot!(ai_finish(&s, &stage(&s), rate), @"[1:v]sendcmd=f=reframe-fallback.cmd,zscale=w=1080:h=2340:f=lanczos:tin=709:pin=709:rin=full:m=709:t=709:p=709:r=limited,hqdn3d=0:0:3:3[crop];[0:v]fps=30000/1001,split[bg0][fg0];[bg0]scale=108:234:force_original_aspect_ratio=increase,crop=108:234,zscale=min=709:tin=709:pin=709:rin=limited:m=709:t=709:p=709:r=limited,gblur=sigma=3,eq=brightness=-0.08,scale=1080:2340,setsar=1[bg];[fg0]zscale=w=1080:h=608:f=lanczos:min=709:tin=709:pin=709:rin=limited:m=709:t=709:p=709:r=limited,cas=0.7[fg];[bg][fg]overlay=0:866[pad];[crop][pad]overlay@fb=0:0:enable=0,setsar=1,format=yuv420p[v]");
        // Blurred background: only the picture goes through the worker (shrunk to half the
        // foreground: the 1080p source is bigger than it); the background comes from the source.
        let b = input((1920, 1080), (1080, 2340), Reframe::Blur);
        insta::assert_snapshot!(ai_decode(&b, rate), @"[0:v]fps=30000/1001,zscale=w=540:h=304:f=lanczos:min=709:tin=709:pin=709:rin=limited:p=709:t=709:r=full,format=gbrp,format=rgb24[v]");
        insta::assert_snapshot!(ai_finish(&b, &stage(&b), rate), @"[1:v]zscale=w=1080:h=608:f=lanczos:tin=709:pin=709:rin=full:m=709:t=709:p=709:r=limited,hqdn3d=0:0:3:3[fg];[0:v]fps=30000/1001,scale=108:234:force_original_aspect_ratio=increase,crop=108:234,zscale=min=709:tin=709:pin=709:rin=limited:m=709:t=709:p=709:r=limited,gblur=sigma=3,eq=brightness=-0.08,scale=1080:2340,setsar=1[bg];[bg][fg]overlay=0:866,setsar=1,format=yuv420p[v]");
    }
}
