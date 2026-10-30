//! Fixed synthetic capacity corpus shared by resource and latency probes. It
//! measures storage/work rather than photographic quality. Dimensions are exact
//! 24 and 48 megapixels; each channel varies deterministically across both axes.

use image::{Rgba, RgbaImage};

pub const DIMENSIONS: [(u32, u32); 2] = [(6000, 4000), (8000, 6000)];

pub fn image(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            ((x * 17 + y * 3) % 256) as u8,
            ((x * 5 + y * 11) % 256) as u8,
            ((x ^ y) % 256) as u8,
            255,
        ])
    })
}
