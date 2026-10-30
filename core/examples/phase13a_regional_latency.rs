//! Reproducible Phase 13A-1 regional-render measurements. Run in isolation
//! with `cargo run --release --example phase13a_regional_latency`; output is
//! JSON lines — one per run, then one median line per operation. The corpus is
//! `tests/common/phase4_corpus.rs` at 24 and 48 MP, stacked as three rasters
//! (the corpus itself, then two offset copies in Multiply and Screen) under
//! one pointwise Levels adjustment. Every regional output is checked against
//! the full projection before its time is reported. A synthetic timing probe,
//! not a quality or peak-memory qualification.

use std::ffi::{c_char, CStr};
use std::ptr;
use std::time::Instant;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_regional::dr_doc_render_region;
use doctored_core::DrImage;
use serde_json::json;

#[path = "../tests/common/phase4_corpus.rs"]
mod phase4_corpus;

fn message(error: *mut c_char) -> String {
    if error.is_null() {
        return "operation refused without error".into();
    }
    let message = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    message
}

fn stack(width: u32, height: u32) -> DrDocument {
    let pixels = phase4_corpus::image(width, height);
    let mut doc = DrDocument::from_pixels(pixels.clone());
    for (offset, blend, opacity) in [
        ((37, 21), BlendMode::Multiply, 0.6),
        ((-15, 9), BlendMode::Screen, 0.5),
    ] {
        doc = doc
            .adding_image_layer(doc.layers.len() - 1, pixels.clone(), "Copy")
            .expect("a raster layer");
        let top = doc.layers.len() - 1;
        doc.layers[top].offset = offset;
        doc.layers[top].blend = blend;
        doc.layers[top].opacity = opacity;
    }
    doc = doc
        .adding_image_layer(doc.layers.len() - 1, image::RgbaImage::new(1, 1), "Levels")
        .expect("an adjustment layer");
    let top = doc.layers.len() - 1;
    doc.layers[top].meta = Some(
        "{\"type\":\"adjust\",\"op\":\"levels\",\"params\":{\"black\":0.05,\"white\":0.95,\"gamma\":1.2}}"
            .into(),
    );
    doc
}

fn pixels(image: *mut DrImage) -> Vec<u8> {
    let (w, h) = unsafe { (dr_image_width(image), dr_image_height(image)) };
    let data = unsafe { dr_image_pixels_rgba(image) };
    let out = unsafe { std::slice::from_raw_parts(data, (w * h * 4) as usize) }.to_vec();
    unsafe { dr_image_free(image) };
    out
}

fn crop(full: &[u8], canvas_w: u32, (x, y, w, h): (u32, u32, u32, u32), d: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for j in (0..h).step_by(d as usize) {
        for i in (0..w).step_by(d as usize) {
            let at = ((u64::from(y + j) * u64::from(canvas_w) + u64::from(x + i)) * 4) as usize;
            out.extend_from_slice(&full[at..at + 4]);
        }
    }
    out
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn main() {
    for (width, height) in phase4_corpus::DIMENSIONS {
        let doc = stack(width, height);
        let handle = Box::into_raw(Box::new(doc));
        let windows = [
            ("full_projection", None),
            (
                "region_512",
                Some(((width / 2, height / 2, 512, 512), 1u32)),
            ),
            ("preview_divisor_8", Some(((0, 0, width, height), 8u32))),
        ];
        let mut times: Vec<Vec<f64>> = vec![Vec::new(); windows.len()];
        for run in 0..3 {
            let mut error = ptr::null_mut();
            let start = Instant::now();
            let full = unsafe { dr_doc_flattened(handle, 0, std::ptr::null(), &mut error) };
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            assert!(!full.is_null(), "{}", message(error));
            let full = pixels(full);
            times[0].push(elapsed);
            println!(
                "{}",
                json!({"width":width,"height":height,"run":run,"operation":windows[0].0,
                "elapsed_ms":elapsed})
            );
            for (slot, (operation, request)) in windows.iter().enumerate().skip(1) {
                let ((x, y, w, h), d) = request.expect("a regional request");
                let mut error = ptr::null_mut();
                let start = Instant::now();
                let region = unsafe {
                    dr_doc_render_region(handle, x, y, w, h, d, u64::MAX, ptr::null(), &mut error)
                };
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                assert!(!region.is_null(), "{}", message(error));
                assert!(
                    pixels(region) == crop(&full, width, (x, y, w, h), d),
                    "{operation} differs from the full projection"
                );
                times[slot].push(elapsed);
                println!(
                    "{}",
                    json!({"width":width,"height":height,"run":run,"operation":operation,
                    "elapsed_ms":elapsed,"equals_reference":true})
                );
            }
        }
        for ((operation, _), samples) in windows.iter().zip(times) {
            println!(
                "{}",
                json!({"width":width,"height":height,"operation":operation,
                "median_ms":median(samples),"runs":3,"layers":"3 rasters + levels"})
            );
        }
        unsafe { dr_doc_free(handle) };
    }
}
