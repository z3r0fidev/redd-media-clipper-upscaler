//! Analysis decode: one ffmpeg process producing low-resolution frames and cut scores
//! (research R6).

/// Long side of the analysis frames (research/09 §4: YuNet at 576 px is about 30 ms a frame).
pub const ANALYSIS_LONG_SIDE: u32 = 576;

/// Analysis frame size for a displayed source size: long side 576, aspect kept, even.
pub fn analysis_size(display: (u32, u32)) -> (u32, u32) {
    let _ = display;
    todo!()
}

/// `size` rounded up to multiples of 32 (YuNet's largest stride); the extra is zero padding.
pub fn padded(size: (u32, u32)) -> (u32, u32) {
    let _ = size;
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(analysis_size((1920, 1080)), (576, 324));
        assert_eq!(analysis_size((1280, 720)), (576, 324));
        assert_eq!(analysis_size((1080, 1920)), (324, 576));
        assert_eq!(analysis_size((1024, 576)), (576, 324));
        assert_eq!(analysis_size((320, 240)), (576, 432), "small sources are scaled up for detection");
        assert_eq!(padded((576, 324)), (576, 352));
        assert_eq!(padded((324, 576)), (352, 576));
    }
}
