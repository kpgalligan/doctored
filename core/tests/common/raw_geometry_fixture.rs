use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use doctored_core::raw_local_mask::RawLocalMask;
use std::sync::Arc;

pub const LIMIT: u64 = 2 << 30;
pub fn codes(width: u32, height: u32) -> Vec<u16> {
    let special = [0, 1, 256, 257, 258, 32767, 32768, 65534, 65535];
    (0..width * height)
        .map(|i| {
            if (i as usize) < special.len() {
                special[i as usize]
            } else {
                i.wrapping_mul(1747).wrapping_add(139) as u16
            }
        })
        .collect()
}
#[allow(dead_code)]
pub fn mask(width: u32, height: u32) -> RawLocalMask {
    let samples = codes(width, height);
    let mut bytes = Vec::with_capacity(16 + samples.len() * 2);
    bytes.extend_from_slice(b"RZLM");
    for v in [1, width, height] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    for v in samples {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    RawLocalMask::from_blob(Arc::new(bytes)).unwrap()
}
pub fn image(width: u32, height: u32) -> PrecisionDocument {
    let samples = codes(width, height);
    let pixels = samples
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            [
                v,
                v.wrapping_add(257),
                if i % 3 == 0 { 65535 } else { v },
                if i % 5 == 0 { 0 } else { 65535 },
            ]
        })
        .collect();
    let image =
        PrecisionImage::from_rgba16(width, height, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}
