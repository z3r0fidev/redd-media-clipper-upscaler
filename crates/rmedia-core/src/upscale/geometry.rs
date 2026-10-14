//! Displayed size, target fit, crop window and the no-enlarge rule (FR-005, research R3).
//!
//! All sizes are in displayed pixels (after rotation and pixel-aspect correction) and even, as
//! 4:2:0 video needs.

use serde::Serialize;

use super::Reframe;

/// Aspect ratios closer than this count as equal: the difference is cropped away centrally
/// (a handful of pixels) instead of reframing. 480x854 and 1080x1920 are both "9:16".
const SAME_ASPECT: f64 = 0.01;

/// Which axis of the source is too long for the target's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Overflow {
    /// Same shape (within 1 %).
    None,
    /// Wider than the target (e.g. landscape into portrait): crop or pad horizontally.
    Horizontal,
    /// Taller than the target (e.g. 9:21 into 9:19.5): crop or pad vertically.
    Vertical,
}

/// A crop window in displayed source pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Crop {
    pub w: u32,
    pub h: u32,
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Geometry {
    /// Displayed source size.
    pub display: (u32, u32),
    /// Output size.
    pub target: (u32, u32),
    pub overflow: Overflow,
    /// The window with the target's shape, centred (smart reframe moves `x` along the path).
    /// `None` when the whole frame already has the target's shape.
    pub crop: Option<Crop>,
    /// The whole frame scaled to fit inside the target (the foreground of `blur`).
    pub fit: (u32, u32),
    /// The picture that gets scaled (crop window, or the whole frame for `blur`) is already at
    /// least as large as the output: scale down only, never enlarge (FR-005).
    pub no_upscale_needed: bool,
}

/// Rounds a size to the nearest even number, at least 2.
pub fn even(v: f64) -> u32 {
    ((v / 2.0).round().max(1.0) as u32) * 2
}

/// Rounds an offset to the nearest even number (0 allowed).
fn even_offset(v: f64) -> u32 {
    ((v / 2.0).round().max(0.0) as u32) * 2
}

/// Plans how a `display`-sized source fills a `target`-sized output with `reframe`.
pub fn plan(display: (u32, u32), target: (u32, u32), reframe: Reframe) -> Geometry {
    let (dw, dh) = (f64::from(display.0), f64::from(display.1));
    let (tw, th) = (f64::from(target.0), f64::from(target.1));
    let (src_aspect, dst_aspect) = (dw / dh, tw / th);
    let ratio = src_aspect / dst_aspect;
    let overflow = if (ratio - 1.0).abs() < SAME_ASPECT {
        Overflow::None
    } else if ratio > 1.0 {
        Overflow::Horizontal
    } else {
        Overflow::Vertical
    };
    let wider = src_aspect > dst_aspect;
    // The largest window of the target's shape inside the frame, centred on even offsets.
    let (cw, ch) = if wider {
        (even(dh * dst_aspect).min(display.0), display.1)
    } else {
        (display.0, even(dw / dst_aspect).min(display.1))
    };
    let crop = (cw != display.0 || ch != display.1).then(|| Crop {
        w: cw,
        h: ch,
        x: even_offset(f64::from(display.0 - cw) / 2.0).min(display.0 - cw),
        y: even_offset(f64::from(display.1 - ch) / 2.0).min(display.1 - ch),
    });
    // The whole frame scaled to fit inside the target.
    let fit = if wider {
        (target.0, even(tw / src_aspect).min(target.1))
    } else {
        (even(th * src_aspect).min(target.0), target.1)
    };
    let no_upscale_needed = match (reframe, overflow) {
        // `blur` scales the whole frame to `fit`.
        (Reframe::Blur, Overflow::Horizontal | Overflow::Vertical) => {
            display.0 >= fit.0 && display.1 >= fit.1
        }
        // Otherwise the crop window (or whole frame) is scaled to the target.
        // The window has the target's shape (within 1 %), so its width decides.
        _ => cw >= target.0,
    };
    Geometry { display, target, overflow, crop, fit, no_upscale_needed }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHONE: (u32, u32) = (1080, 2340);
    const REDDIT: (u32, u32) = (1080, 1920);

    fn crop(g: &Geometry) -> (u32, u32, u32, u32) {
        g.crop.map_or((0, 0, 0, 0), |c| (c.w, c.h, c.x, c.y))
    }

    #[test]
    fn geometry_table() {
        // (display, target, reframe, overflow, crop w,h,x,y, fit, no_upscale_needed)
        type Row = ((u32, u32), (u32, u32), Reframe, Overflow, (u32, u32, u32, u32), (u32, u32), bool);
        let rows: &[Row] = &[
            // 9:16 portrait into 9:19.5: a little too wide, cropped at the sides.
            (
                (480, 854),
                PHONE,
                Reframe::Center,
                Overflow::Horizontal,
                (394, 854, 44, 0),
                (1080, 1922),
                false,
            ),
            // Same shape (within 1 %): only rounding pixels go, no reframing.
            ((480, 854), REDDIT, Reframe::Center, Overflow::None, (0, 0, 0, 0), (1080, 1920), false),
            // Landscape into portrait.
            (
                (1920, 1080),
                PHONE,
                Reframe::Center,
                Overflow::Horizontal,
                (498, 1080, 712, 0),
                (1080, 608),
                false,
            ),
            (
                (1920, 1080),
                PHONE,
                Reframe::Blur,
                Overflow::Horizontal,
                (498, 1080, 712, 0),
                (1080, 608),
                true,
            ),
            // 9:21 is taller than 9:19.5: cropped top and bottom.
            (
                (720, 1680),
                PHONE,
                Reframe::Center,
                Overflow::Vertical,
                (720, 1560, 0, 60),
                (1002, 2340),
                false,
            ),
            // Square.
            (
                (1080, 1080),
                PHONE,
                Reframe::Center,
                Overflow::Horizontal,
                (498, 1080, 292, 0),
                (1080, 1080),
                false,
            ),
            // Rotated phone video (already the displayed size) at 4K: downscale only.
            ((2160, 3840), REDDIT, Reframe::Center, Overflow::None, (0, 0, 0, 0), (1080, 1920), true),
            (
                (2160, 3840),
                PHONE,
                Reframe::Smart,
                Overflow::Horizontal,
                (1772, 3840, 194, 0),
                (1080, 1920),
                true,
            ),
            // Anamorphic 720x576 at 64:45 is displayed 1024x576.
            (
                (1024, 576),
                REDDIT,
                Reframe::Center,
                Overflow::Horizontal,
                (324, 576, 350, 0),
                (1080, 608),
                false,
            ),
        ];
        for (i, (display, target, reframe, overflow, c, fit, no_up)) in rows.iter().enumerate() {
            let g = plan(*display, *target, *reframe);
            assert_eq!(g.overflow, *overflow, "row {i} overflow");
            assert_eq!(crop(&g), *c, "row {i} crop");
            assert_eq!(g.fit, *fit, "row {i} fit");
            assert_eq!(g.no_upscale_needed, *no_up, "row {i} no_upscale_needed");
        }
    }

    #[test]
    fn everything_is_even_and_inside_the_frame() {
        for display in
            [(480, 854), (1920, 1080), (1080, 1080), (640, 360), (1282, 722), (2160, 3840), (720, 1680)]
        {
            for target in [PHONE, REDDIT, (1080, 1350), (1080, 1200), (1440, 3120), (1080, 1566)] {
                for reframe in [Reframe::Center, Reframe::Blur, Reframe::Smart] {
                    let g = plan(display, target, reframe);
                    if let Some(c) = g.crop {
                        assert!(
                            c.w.is_multiple_of(2)
                                && c.h.is_multiple_of(2)
                                && c.x.is_multiple_of(2)
                                && c.y.is_multiple_of(2),
                            "{g:?}"
                        );
                        assert!(c.x + c.w <= display.0 && c.y + c.h <= display.1, "{g:?}");
                    }
                    assert!(
                        g.fit.0.is_multiple_of(2)
                            && g.fit.1.is_multiple_of(2)
                            && g.fit.0 <= target.0
                            && g.fit.1 <= target.1,
                        "{g:?}"
                    );
                }
            }
        }
    }
}
