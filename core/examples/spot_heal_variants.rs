//! Every candidate of the Spot Healing Brush Type take on one
//! picture, through `DrDocument::spot_heal_variant` (strength 1, the
//! automatic ring, seed 0, sampling Current):
//!
//! ```text
//! spot_heal_variants <width> <height> <picture.rgba> <overlay.rgba> <out-dir>
//! ```
//!
//! `picture.rgba` is the straight RGBA8 picture (one layer, canvas-sized);
//! `overlay.rgba` is the stroke as `dr_doc_spot_heal_layer_job` takes it (its
//! ALPHA is the coverage). Writes `<out-dir>/<type>_<candidate>.rgba` — raw
//! RGBA8 of the healed layer — for `content_aware_shipped`, `proximity_H0`
//! (band 4), `proximity_C1` (band 2), `proximity_C2` (band 8), `texture_H0`
//! (tile 7, detrended), `texture_C1` (tile 3, detrended) and `texture_C2`
//! (tile 7, not detrended), and prints one JSON line per candidate: its
//! name and the file, or the refusal.
use std::path::Path;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_pixels_rgba};
use doctored_core::ffi_doc::dr_doc_layer_image;
use doctored_core::spot_heal_type::{SpotHealParams, SpotHealType, SHIPPED};
use image::RgbaImage;

fn params(band: usize, tile: usize, detrend: bool) -> SpotHealParams {
    SpotHealParams {
        band,
        tile,
        detrend,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 6 {
        eprintln!(
            "usage: spot_heal_variants <width> <height> <picture.rgba> <overlay.rgba> <out-dir>"
        );
        std::process::exit(2);
    }
    let w: u32 = args[1].parse().expect("a width");
    let h: u32 = args[2].parse().expect("a height");
    let picture = std::fs::read(&args[3]).expect("the picture");
    let overlay = std::fs::read(&args[4]).expect("the overlay");
    let n = (w * h * 4) as usize;
    assert_eq!(picture.len(), n, "picture is width*height*4 bytes");
    assert_eq!(overlay.len(), n, "overlay is width*height*4 bytes");
    let out_dir = Path::new(&args[5]);
    std::fs::create_dir_all(out_dir).expect("the output folder");
    let doc = DrDocument::from_pixels(RgbaImage::from_raw(w, h, picture).expect("an image"));
    let candidates = [
        ("content_aware_shipped", SpotHealType::ContentAware, SHIPPED),
        (
            "proximity_H0",
            SpotHealType::ProximityMatch,
            params(4, 7, true),
        ),
        (
            "proximity_C1",
            SpotHealType::ProximityMatch,
            params(2, 7, true),
        ),
        (
            "proximity_C2",
            SpotHealType::ProximityMatch,
            params(8, 7, true),
        ),
        (
            "texture_H0",
            SpotHealType::CreateTexture,
            params(4, 7, true),
        ),
        (
            "texture_C1",
            SpotHealType::CreateTexture,
            params(4, 3, true),
        ),
        (
            "texture_C2",
            SpotHealType::CreateTexture,
            params(4, 7, false),
        ),
    ];
    for (name, kind, candidate) in candidates {
        match doc.spot_heal_variant(0, &overlay, kind, candidate) {
            Ok(Some(healed)) => {
                let image = unsafe { dr_doc_layer_image(&healed, 0) };
                assert!(!image.is_null(), "the healed layer");
                let bytes = unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(image), n) };
                let path = out_dir.join(format!("{name}.rgba"));
                std::fs::write(&path, bytes).expect("the output");
                unsafe { dr_image_free(image) };
                println!(
                    "{}",
                    serde_json::json!({"candidate": name, "file": path.to_string_lossy()})
                );
            }
            Ok(None) => println!(
                "{}",
                serde_json::json!({"candidate": name, "refused": "nothing changed"})
            ),
            Err(message) => println!(
                "{}",
                serde_json::json!({"candidate": name, "refused": message})
            ),
        }
    }
}
