//! One case of the lifted Content-Aware Fill's admission measurement:
//! a 2600×2600 opaque texture, coverage over the 2000×2000 box at (300, 300)
//! — 4 MP, the most lift 1 takes — ring 0, in place, one fill per process.
//! The caller measures this PROCESS's peak `phys_footprint` from outside
//! (`/usr/bin/time -l`, `MallocLargeCache=0`).
//!
//! ```text
//! inpaint_lift_admission <8|16> <fill|twin>
//! ```
//!
//! `fill` runs the edit (`dr_doc_content_aware_fill_edit` at 8 bits,
//! `dr_precision_doc_edit` at 16, both with a NULL control); `twin` builds
//! the same document, coverage and request, asks for the same plan, and
//! makes no edit call, so the two peaks differ in exactly the edit. The
//! printed JSON holds the admission's figure (`scratch_bytes`): it must stay
//! at or above 1.25 × (fill peak − twin peak).
//!
//! The request carries the hole's `covered` count, so the byte
//! admission prices the call at its lift (`doc_inpaint::LIFTED_WINDOW_BYTES`).
//!
//! Every coverage sample is written before the call, as every app path
//! writes its own (`precision_inpaint`'s `# Admission` holds what an
//! untouched zero page did to an earlier harness).
use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_content_aware::{
    dr_doc_content_aware_fill_edit, dr_doc_content_aware_fill_scratch,
};
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::RgbaImage;

const SIDE: u32 = 2600;
const BOX: (u32, u32, u32, u32) = (300, 300, 2000, 2000);

/// A value that varies with position, so no plane is a run of equal pages.
fn sample(index: usize, channel: usize) -> u32 {
    (index as u32)
        .wrapping_mul(2_654_435_761)
        .rotate_left(channel as u32 * 7)
        >> 16
}

fn covered(index: usize) -> bool {
    let (x, y) = (index as u32 % SIDE, index as u32 / SIDE);
    (BOX.0..BOX.0 + BOX.2).contains(&x) && (BOX.1..BOX.1 + BOX.3).contains(&y)
}

fn taken(pointer: *mut std::ffi::c_char) -> String {
    assert!(!pointer.is_null());
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    text
}

fn refusal(error: *mut std::ffi::c_char) -> String {
    if error.is_null() {
        String::new()
    } else {
        taken(error)
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let [_, depth, mode] = &arguments[..] else {
        eprintln!("usage: inpaint_lift_admission <8|16> <fill|twin>");
        std::process::exit(2);
    };
    let fill = mode == "fill";
    let pixels = SIDE as usize * SIDE as usize;
    let request = CString::new(
        serde_json::json!({
            "op": "content_aware_fill", "layer": 0,
            "x": BOX.0, "y": BOX.1, "width": BOX.2, "height": BOX.3,
            "ring": 0, "seed": 7, "sample": "current", "output": "layer",
            "covered": u64::from(BOX.2) * u64::from(BOX.3),
        })
        .to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let (scratch, changed) = if depth == "16" {
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
        let document = PrecisionDocument::from_image(
            PrecisionImage::from_rgba16(SIDE, SIDE, samples, IccProfile::srgb(), None).unwrap(),
            Metadata::default(),
            Resolution::default(),
        )
        .unwrap();
        let coverage: Vec<u16> = (0..pixels)
            .map(|i| if covered(i) { 65535 } else { 0 })
            .collect();
        let plan = unsafe {
            dr_precision_doc_edit_plan(
                &document,
                request.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                &mut error,
            )
        };
        assert!(!plan.is_null(), "{}", refusal(error));
        let plan: serde_json::Value = serde_json::from_str(&taken(plan)).unwrap();
        let mut changed = false;
        if fill {
            let out = unsafe {
                dr_precision_doc_edit(
                    &document,
                    request.as_ptr(),
                    coverage.as_ptr(),
                    coverage.len(),
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    &mut error,
                )
            };
            assert!(!out.is_null(), "{}", refusal(error));
            drop(unsafe { Box::from_raw(out) });
            changed = true;
        }
        (plan["scratch_bytes"].as_u64().unwrap(), changed)
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
        let document = DrDocument::from_pixels(RgbaImage::from_raw(SIDE, SIDE, raw).unwrap());
        let coverage: Vec<u8> = (0..pixels)
            .map(|i| if covered(i) { 255 } else { 0 })
            .collect();
        let scratch =
            unsafe { dr_doc_content_aware_fill_scratch(&document, request.as_ptr(), &mut error) };
        assert!(scratch > 0, "{}", refusal(error));
        let mut changed = false;
        if fill {
            let out = unsafe {
                dr_doc_content_aware_fill_edit(
                    &document,
                    request.as_ptr(),
                    coverage.as_ptr(),
                    ptr::null(),
                    &mut error,
                )
            };
            assert!(!out.is_null(), "{}", refusal(error));
            unsafe { dr_doc_free(out) };
            changed = true;
        }
        (scratch, changed)
    };
    println!(
        "{}",
        serde_json::json!({
            "depth": depth, "mode": mode, "canvas_pixels": pixels,
            "hole_pixels": u64::from(BOX.2) * u64::from(BOX.3),
            "scratch_bytes": scratch, "changed": changed,
        })
    );
}
