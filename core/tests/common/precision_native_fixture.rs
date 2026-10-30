//! Independent old typed framing; never derive compatibility by relabeling a
//! current-version file whose record layout may have changed.
use doctored_core::icc::IccProfile;
use image::{DynamicImage, ImageBuffer, Rgba};
use std::io::Cursor;
fn blob(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend((bytes.len() as u32).to_le_bytes());
    out.extend(bytes);
}
fn record(out: &mut Vec<u8>, kind: u8) {
    out.extend([kind, 1, 0, 0]);
    out.extend(1.0_f32.to_le_bytes());
    blob(out, b"L");
    out.extend([0_u8; 8]);
}
pub fn image_file(version: u32, pixels: &[[u16; 4]], adjustment: Option<&str>) -> Vec<u8> {
    let count = 1 + u32::from(adjustment.is_some());
    let width = pixels.len() as u32;
    let mut out = b"RZDC".to_vec();
    for v in [version, 1, 1, width, 1, count, count] {
        out.extend(v.to_le_bytes());
    }
    blob(&mut out, IccProfile::srgb().bytes());
    out.extend(72.0_f32.to_le_bytes());
    out.extend(72.0_f32.to_le_bytes());
    out.extend([0, 0, 0]);
    record(&mut out, 0);
    out.extend(width.to_le_bytes());
    out.extend(1_u32.to_le_bytes());
    let image = ImageBuffer::<Rgba<u16>, Vec<u16>>::from_raw(
        width,
        1,
        pixels.iter().flatten().copied().collect(),
    )
    .unwrap();
    let mut png = Cursor::new(Vec::new());
    DynamicImage::ImageRgba16(image)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let png = png.into_inner();
    out.extend((png.len() as u64).to_le_bytes());
    out.extend(png);
    if let Some(meta) = adjustment {
        record(&mut out, 3);
        blob(&mut out, meta.as_bytes());
    }
    out
}
