use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::warp::WarpEdit;
use image::RgbaImage;
use serde_json::{json, Value};

pub const LIMIT: u64 = 2_000_000_000;
pub const W: u32 = 17;
pub const H: u32 = 13;
pub const OFFSET: (i32, i32) = (-3, 2);

pub fn byte_pixels() -> Vec<u8> {
    let mut raw = Vec::new();
    let ladder = [0, 1, 7, 32, 127, 255];
    for y in 0..H {
        for x in 0..W {
            let a = ladder[((x * 3 + y * 5) as usize) % ladder.len()];
            raw.extend_from_slice(&[
                ((x * 37 + y * 19 + 241) % 256) as u8,
                ((x * 11 + y * 53 + 199) % 256) as u8,
                ((x * 71 + y * 7 + 113) % 256) as u8,
                a,
            ]);
        }
    }
    raw
}

pub fn typed_pixels() -> Vec<[u16; 4]> {
    let ladder = [0, 1, 179, 8191, 32769, 65535];
    (0..H)
        .flat_map(|y| {
            (0..W).map(move |x| {
                [
                    ((x * 3901 + y * 977 + 63101) % 65536) as u16,
                    ((x * 779 + y * 8191 + 17411) % 65536) as u16,
                    ((x * 4999 + y * 127 + 44351) % 65536) as u16,
                    ladder[((x * 3 + y * 5) as usize) % ladder.len()],
                ]
            })
        })
        .collect()
}

pub fn byte_document() -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_raw(W, H, byte_pixels()).unwrap());
    doc.layers[0].offset = OFFSET;
    doc
}

pub fn typed_document() -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let image = PrecisionImage::from_rgba16(W, H, typed_pixels(), profile.clone(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Warp fixture", image, OFFSET, 1.0, true).unwrap();
    PrecisionDocument::from_parts(
        W,
        H,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap()
}

pub fn field(columns: u32, rows: u32, filter: &str) -> Value {
    let values: Vec<[f64; 2]> = (0..rows)
        .flat_map(|y| {
            (0..columns).map(move |x| {
                let fx = x as f64 / (columns - 1) as f64;
                let fy = y as f64 / (rows - 1) as f64;
                [
                    0.35 + 0.58 * fx - 0.31 * fy + 0.12 * fx * fy,
                    -0.27 + 0.33 * fy + 0.19 * fx - 0.17 * fx * fy,
                ]
            })
        })
        .collect();
    json!({"op":"set","layer":0,"target":"pixels","columns":columns,"rows":rows,
        "displacements":values,"filter":filter})
}

pub fn edit(request: Value) -> WarpEdit {
    WarpEdit::parse(&request.to_string()).unwrap()
}
