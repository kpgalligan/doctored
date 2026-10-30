//! #1 Lab phase 6b-ii: Quick Selection, Select and Mask's edge stage and
//! Decontaminate Colors on a Lab Color document, through the FFI
//! (`dr_precision_doc_select`, `dr_precision_doc_estimate_foreground(_plan)`).
//! Quick Selection's ΔE76 reads the codes' own L\*a\*b\*; the edge stage
//! guides on the codes per channel and the foreground solve runs per channel
//! on them, so their oracle is the same request on an sRGB-tagged RGB16
//! document holding the same samples. Bounds, declared before measuring:
//!
//! * (e) `quick` on an sRGB16 document of flat patches and on its
//!   `convert_to_lab` at 16 and 8 bits, with the same seeds, `radius` 4 and
//!   `smooth` 1, gives equal coverage, exactly;
//! * (f) `refine_edge` (smart radius on and off, with and without `area`)
//!   equals the same request on the RGB16 document, exactly, at 8 and 16 bits;
//! * (g) `dr_precision_doc_estimate_foreground` equals the RGB16 document's
//!   estimate, exactly, and its plan does not refuse, at 8 and 16 bits.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_foreground::{
    dr_precision_doc_estimate_foreground, dr_precision_doc_estimate_foreground_plan,
};
use doctored_core::ffi_lab::dr_precision_doc_convert_to_lab;
use doctored_core::ffi_precision::dr_precision_image_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit_plan, dr_precision_doc_select};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_ops::{free, plan_json, LIMIT};
use crate::lab_geometry::{document, ramp, twin_profile, DEPTHS, H, W};
use crate::lab_selection::select;

/// `dr_precision_doc_select` of `operation` over the composite with `input`.
fn select_with(doc: *const PrecisionDocument, operation: &Value, input: &[u16]) -> Vec<u16> {
    let request = json!({"op":"selection","operation":operation}).to_string();
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    let planned = plan_json(plan, err).unwrap_or_else(|e| panic!("{operation}: {e}"));
    assert_eq!(planned["input_samples"].as_u64(), Some(input.len() as u64));
    let mut out = vec![0u16; planned["output_samples"].as_u64().unwrap() as usize];
    let ok = unsafe {
        dr_precision_doc_select(
            doc,
            text.as_ptr(),
            input.as_ptr(),
            input.len(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{operation}: {}", take_err_string(err));
    out
}

/// The four flat sRGB patches of (e), 12 × 8 each over a 24 × 16 canvas.
const PATCHES: [[f64; 3]; 4] = [
    [0.80, 0.20, 0.20],
    [0.20, 0.70, 0.30],
    [0.20, 0.30, 0.85],
    [0.90, 0.85, 0.30],
];

fn patch_document() -> *mut PrecisionDocument {
    let (w, h) = (24u32, 16u32);
    let pixels = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let rgb = PATCHES[((y / 8) * 2 + x / 12) as usize];
            let c = |v: f64| (v * 65535.0).round() as u16;
            [c(rgb[0]), c(rgb[1]), c(rgb[2]), 65535]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ))
}

fn to_lab(doc: *const PrecisionDocument, bits: u32) -> *mut PrecisionDocument {
    let options = CString::new(json!({"depth": bits}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_lab(doc, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!out.is_null(), "to Lab: {}", take_err_string(err));
    out
}

#[test]
fn quick_selection_reads_the_codes_lab() {
    let rgb = patch_document();
    let quick =
        |seeds: Value| json!({"kind":"quick","params":{"seeds":seeds,"radius":4,"smooth":1}});
    for seeds in [json!([[5, 4]]), json!([[18, 11], [6, 12]])] {
        let operation = quick(seeds);
        let want = select(rgb, &operation).unwrap();
        assert!(want.contains(&65535), "{operation} selects");
        assert!(want.contains(&0), "{operation} leaves something out");
        for bits in DEPTHS {
            let lab = to_lab(rgb, bits);
            let got = select(lab, &operation).unwrap_or_else(|e| panic!("{operation}: {e}"));
            assert_eq!(got, want, "{operation} at {bits} bits");
            free(lab);
        }
    }
    free(rgb);
}

/// `pixels` in an sRGB-tagged RGB16 document of `W × H` (no Lab policy).
fn srgb_document(pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    let record = Layer::raster("Background", image, (0, 0), 1.0, true).unwrap();
    document(&[record], 16, false)
}

fn lab_document(pixels: Vec<[u16; 4]>, bits: u32) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(W, H, pixels, twin_profile(), None).unwrap();
    let record = Layer::raster("Background", image, (0, 0), 1.0, true).unwrap();
    document(&[record], bits, true)
}

/// A soft rectangle of coverage: full inside x 6..18, y 4..13, a ramp two
/// pixels wide outside it.
fn coverage() -> Vec<u16> {
    (0..W * H)
        .map(|i| {
            let (x, y) = ((i % W) as i32, (i / W) as i32);
            let dx = (6 - x).max(x - 17).max(0);
            let dy = (4 - y).max(y - 12).max(0);
            let d = dx.max(dy);
            (65535 - (d * 65535 / 3).min(65535)) as u16
        })
        .collect()
}

#[test]
fn the_edge_stage_guides_on_the_codes() {
    let mut area_input = coverage();
    area_input.extend((0..W * H).map(|i| match i % W {
        0..=3 => 0,
        20.. => 255,
        _ => 128,
    }));
    for bits in DEPTHS {
        let pixels = ramp(W, H, bits, 3);
        let lab = lab_document(pixels.clone(), bits);
        let rgb = srgb_document(pixels);
        for smart in [false, true] {
            for area in [false, true] {
                let operation = json!({"kind":"refine_edge","edge_radius":3.0,
                    "smart_radius":smart,"area":area});
                let input = if area { &area_input } else { &coverage() };
                let want = select_with(rgb, &operation, input);
                let got = select_with(lab, &operation, input);
                assert_ne!(
                    got[..],
                    input[..got.len()],
                    "{operation} at {bits} bits: no change"
                );
                assert_eq!(got, want, "{operation} at {bits} bits");
            }
        }
        free(lab);
        free(rgb);
    }
}

/// The estimate's plan, or its refusal.
fn foreground_plan(
    doc: *const PrecisionDocument,
    source: &PrecisionImage,
) -> Result<Value, String> {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_estimate_foreground_plan(doc, source, &mut err) };
    plan_json(out, err)
}

/// The estimate's samples; it must change the source.
fn foreground(
    doc: *const PrecisionDocument,
    source: &PrecisionImage,
    matte: &[u16],
) -> Vec<[u16; 4]> {
    let params = CString::new(json!({"color_space":"encoded","strength":1}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_estimate_foreground(
            doc,
            source,
            matte.as_ptr(),
            matte.len(),
            params.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "estimate: {}", take_err_string(err));
    let samples = unsafe { &*out }.to_rgba16().unwrap();
    unsafe { dr_precision_image_free(out) };
    samples
}

#[test]
fn decontaminate_colors_solves_on_the_codes() {
    // The matte: 0 left, full right, a ramp between.
    let matte: Vec<u16> = (0..W * H)
        .map(|i| match i % W {
            0..=5 => 0,
            18.. => 65535,
            x => ((x - 5) * 65535 / 13) as u16,
        })
        .collect();
    for bits in DEPTHS {
        let pixels: Vec<[u16; 4]> = ramp(W, H, bits, 9)
            .into_iter()
            .map(|p| [p[0], p[1], p[2], 65535])
            .collect();
        let lab = lab_document(pixels.clone(), bits);
        let rgb = srgb_document(pixels.clone());
        let lab_source = PrecisionImage::from_rgba16(
            W,
            H,
            pixels.clone(),
            unsafe { &*lab }.profile().clone(),
            None,
        )
        .unwrap();
        let rgb_source =
            PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
        let plan = foreground_plan(lab, &lab_source).unwrap_or_else(|e| panic!("{bits} bits: {e}"));
        let rgb_plan = foreground_plan(rgb, &rgb_source).unwrap();
        assert_eq!(plan["scratch_bytes"], rgb_plan["scratch_bytes"]);
        let solved = foreground(lab, &lab_source, &matte);
        assert_ne!(
            solved,
            lab_source.to_rgba16().unwrap(),
            "{bits} bits: no change"
        );
        assert_eq!(solved, foreground(rgb, &rgb_source, &matte), "{bits} bits");
        free(lab);
        free(rgb);
    }
}
