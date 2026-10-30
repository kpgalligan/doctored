//! One case of the layered export's bake admission measurement: build
//! a canvas with one opaque raster and one adjustment layer above it, write
//! it as a layered PSD, and print the preflight's own numbers. The caller
//! measures this PROCESS's peak `phys_footprint` from outside
//! (`/usr/bin/time -l`, `MallocLargeCache=0`), one case per process.
//!
//! ```text
//! psd_bake_admission <8|16> <bake|twin> <width> <height> <out.psd>
//! ```
//!
//! `bake` uses a Gradient Map, which has no Photoshop record and is written
//! as its stamp; `twin` uses Invert, which is written as a record, so the two
//! runs differ in exactly the bake. The printed JSON holds `worst_case_bytes`
//! and `bake_scratch_bytes`; the difference in peak footprint between the two
//! must stay inside the difference in those plans with the 1.25 margin.
use std::ffi::CString;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_psd::{
    dr_doc_psd_export_report, dr_doc_save_psd, dr_precision_doc_psd_export_report,
    dr_precision_doc_save_psd,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use image::RgbaImage;

const GRADIENT: &str = r##"{"gradient":{"stops":[{"position":0.0,"color":"#102030","opacity":1.0},{"position":1.0,"color":"#f0c040","opacity":1.0}]}}"##;

fn meta(bake: bool, typed: bool) -> String {
    let (op, params) = if bake {
        ("gradient_map", GRADIENT)
    } else {
        ("invert", "{}")
    };
    let evaluation = if typed {
        r#","evaluation":"precision_v1""#
    } else {
        ""
    };
    format!(r#"{{"type":"adjust","op":"{op}","params":{params}{evaluation}}}"#)
}

/// A value that varies with position, so PackBits cannot collapse the planes.
fn sample(index: usize, channel: usize) -> u32 {
    (index as u32)
        .wrapping_mul(2_654_435_761)
        .rotate_left(channel as u32 * 7)
        >> 16
}

fn taken(pointer: *mut std::ffi::c_char) -> String {
    assert!(!pointer.is_null());
    let text = unsafe { std::ffi::CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { doctored_core::ffi::dr_string_free(pointer) };
    text
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let [_, depth, mode, width, height, out] = &arguments[..] else {
        eprintln!("usage: psd_bake_admission <8|16> <bake|twin> <width> <height> <out.psd>");
        std::process::exit(2);
    };
    let (width, height): (u32, u32) = (width.parse().unwrap(), height.parse().unwrap());
    let bake = mode == "bake";
    let pixels = width as usize * height as usize;
    let path = CString::new(out.as_str()).unwrap();
    let mut error = ptr::null_mut();
    let report = if depth == "16" {
        let profile = IccProfile::srgb();
        let samples: Vec<[u16; 4]> = (0..pixels)
            .map(|i| {
                [
                    sample(i, 0) as u16,
                    sample(i, 1) as u16,
                    sample(i, 2) as u16,
                    65535,
                ]
            })
            .collect();
        let image = PrecisionImage::from_rgba16(width, height, samples, Arc::clone(&profile), None)
            .unwrap();
        let document = PrecisionDocument::from_parts(
            width,
            height,
            Arc::clone(&profile),
            Metadata::default(),
            Resolution::default(),
            vec![
                PrecisionDocumentLayer::raster("Base", image, (0, 0), 1.0, true).unwrap(),
                PrecisionDocumentLayer::adjustment("Tone", &meta(bake, true), 1.0, true, &profile)
                    .unwrap(),
            ],
        )
        .unwrap();
        let limit = u64::MAX / 4;
        let projection = document.render(limit, 0, None).unwrap();
        let report =
            taken(unsafe { dr_precision_doc_psd_export_report(&document, 0, 0, &mut error) });
        let ok = unsafe {
            dr_precision_doc_save_psd(
                &document,
                &projection,
                path.as_ptr(),
                0,
                0,
                limit,
                0,
                &mut error,
            )
        };
        assert!(
            ok,
            "{}",
            if error.is_null() {
                String::new()
            } else {
                taken(error)
            }
        );
        report
    } else {
        let mut raw = Vec::with_capacity(pixels * 4);
        for i in 0..pixels {
            raw.extend_from_slice(&[
                sample(i, 0) as u8,
                sample(i, 1) as u8,
                sample(i, 2) as u8,
                255,
            ]);
        }
        let mut document =
            DrDocument::from_pixels(RgbaImage::from_raw(width, height, raw).unwrap())
                .adding_image_layer(0, RgbaImage::new(width, height), "Tone")
                .unwrap();
        document.layers[1].meta = Some(meta(bake, false));
        let report = taken(unsafe { dr_doc_psd_export_report(&document, 0, 0, &mut error) });
        let ok =
            unsafe { dr_doc_save_psd(&document, ptr::null(), path.as_ptr(), 0, 0, &mut error) };
        assert!(
            ok,
            "{}",
            if error.is_null() {
                String::new()
            } else {
                taken(error)
            }
        );
        report
    };
    let report: serde_json::Value = serde_json::from_str(&report).unwrap();
    println!(
        "{}",
        serde_json::json!({
            "depth": depth, "mode": mode, "pixels": pixels,
            "worst_case_bytes": report["worst_case_bytes"],
            "bake_scratch_bytes": report["bake_scratch_bytes"],
            "written_bytes": std::fs::metadata(out).map(|m| m.len()).unwrap_or(0),
        })
    );
}
