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
    /// The reframe actually applied (`Smart` follows a crop path from US2; here it is centred).
    pub reframe: Reframe,
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

/// Builds the filtergraph.
pub fn build(g: &GraphInput) -> String {
    let pre = pre_chain(g);
    let (w, h) = g.geometry.target;
    let pix = if g.keep_hdr { "yuv420p10le" } else { "yuv420p" };
    let pad = g.reframe == Reframe::Blur && g.geometry.overflow != Overflow::None;
    if !pad {
        let mut chain = vec![pre];
        if let Some(c) = g.geometry.crop {
            chain.push(format!("crop={}:{}:{}:{}", c.w, c.h, c.x, c.y));
        }
        chain.push(scaler(g, w, h));
        chain.push(format!("setsar=1,format={pix}"));
        return format!("[0:v]{}[v]", chain.join(","));
    }
    let (fw, fh) = g.geometry.fit;
    let (bw, bh) = (even(f64::from(w) / 10.0), even(f64::from(h) / 10.0));
    let (ox, oy) = ((w - fw) / 2 / 2 * 2, (h - fh) / 2 / 2 * 2);
    // The background gets the same colour conversion as the foreground (at 1/10 size, cheap),
    // and overlay keeps 10 bits when HDR is kept (it defaults to 8-bit yuv420).
    let bg_colour = colour_tail(g);
    let overlay_fmt = if g.keep_hdr { ":format=yuv420p10" } else { "" };
    format!(
        "[0:v]{pre},split[bg0][fg0];\
         [bg0]scale={bw}:{bh}:force_original_aspect_ratio=increase,crop={bw}:{bh}{bg_colour},gblur=sigma=3,\
         eq=brightness=-0.08,scale={w}:{h},setsar=1[bg];\
         [fg0]{}[fg];\
         [bg][fg]overlay={ox}:{oy}{overlay_fmt},setsar=1,format={pix}[v]",
        scaler(g, fw, fh)
    )
}

/// Trim, deinterlace, square pixels, tone-map, denoise, frame-rate cap — at source resolution.
fn pre_chain(g: &GraphInput) -> String {
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
    if g.tonemap {
        f.push(TONEMAP.replace("{in}", &g.colour_in));
    }
    f.push(DENOISE.into());
    if let Some(fps) = g.fps_cap {
        f.push(format!("fps={fps}"));
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
    let opts = colour_options(g);
    if opts.is_empty() { String::new() } else { format!(",zscale={}", &opts[1..]) }
}

/// The CPU-tier scaler to `w`x`h`: zscale Lanczos with explicit BT.709 output (or untouched
/// colours when keeping HDR), then sharpening when enlarging. Lanczos with `cas=0.7` measured
/// best within research R3's range (+2.8 VMAF over bicubic on the 480p fixture).
fn scaler(g: &GraphInput, w: u32, h: u32) -> String {
    let mut s = format!("zscale=w={w}:h={h}:f=lanczos");
    s.push_str(&colour_options(g));
    if !g.geometry.no_upscale_needed {
        s.push(',');
        s.push_str(CAS);
    }
    s
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
        }
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
}
