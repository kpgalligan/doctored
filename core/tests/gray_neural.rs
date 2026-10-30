//! #1 phase 6b: the neural, generative and Merge to HDR results on a
//! Grayscale document through the FFI. Each brings in colour from outside
//! the document (a model, a provider, the merge), and the core folds it to
//! its gray by the Mode rule before it lands.
//!
//! # The oracle
//!
//! The RGB TWIN (`gray_ops::twin`) given the same coloured input, its result
//! then taken through Image ▸ Mode ▸ Grayscale (`dr_precision_doc_convert_to_gray`,
//! the same Gray profile and depth): the fold IS that conversion of the new
//! pixels, so Super Zoom's document, JPEG Artifacts Removal's layer (in place
//! and as a new layer), a Generative Fill layer and its second variation, and
//! a 32-bit Merge to HDR equal the twin's converted, with R = G = B. The
//! generative plan is the twin's plus the fold's 8 bytes a pixel.
//!
//! # Bounds, declared before the first run
//!
//! Every comparison is exact.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_gen_fill::{
    dr_precision_doc_gen_fill_apply, dr_precision_doc_gen_fill_plan,
};
use doctored_core::ffi_gen_variations::dr_precision_doc_gen_fill_select_variation;
use doctored_core::ffi_gray::dr_precision_doc_convert_to_gray;
use doctored_core::ffi_merge_hdr::dr_precision_doc_merge_hdr;
use doctored_core::ffi_neural::{
    dr_precision_doc_neural_layer_apply, dr_precision_doc_super_zoom_document,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::json;

use crate::common::take_err_string;
use crate::gray_ops::{assert_neutral, edit, free, gray_doc, layer, result, twin, LIMIT};

const W: u32 = 9;
const H: u32 = 6;

/// A coloured image: R and G always differ, every seventh pixel at partial
/// alpha, so no pixel is neutral and the fold re-encodes every one.
fn coloured(w: u32, h: u32) -> PrecisionImage {
    let pixels = (0..w * h)
        .map(|i| {
            let r = (i * 7919 + 211) % 60_000 + 100;
            let g = (r + 4000) % 60_000 + 100;
            let b = (i * 1299 + 501) % 61_000 + 300;
            let a = if i % 7 == 3 { 41_000 } else { 65_535 };
            [r as u16, g as u16, b as u16, a]
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap()
}

/// `doc` through Image ▸ Mode ▸ Grayscale into Gray Gamma 2.2 at `depth`
/// (the profile `gray_ops::gray_doc` uses).
fn to_gray(doc: *const PrecisionDocument, depth: u32) -> *mut PrecisionDocument {
    let options = if depth == 32 {
        r#"{"space":"gray_gamma_22"}"#.to_string()
    } else {
        format!(r#"{{"space":"gray_gamma_22","depth":{depth}}}"#)
    };
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let gray = unsafe {
        dr_precision_doc_convert_to_gray(
            doc,
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!gray.is_null(), "to gray: {}", take_err_string(err));
    gray
}

/// Layer `index` of `ours` against layer `their_index` of `theirs`
/// converted to Gray at `depth`; both R = G = B.
fn assert_twin_layer(
    ours: *const PrecisionDocument,
    theirs: *const PrecisionDocument,
    index: usize,
    depth: u32,
    what: &str,
    their_index: usize,
) {
    assert!(unsafe { &*ours }.is_gray(), "{what}: not Gray");
    let oracle = to_gray(theirs, depth);
    let a = layer(ours, index, depth);
    let b = layer(oracle, their_index, depth);
    assert_neutral(&a, what);
    assert_eq!(a, b, "{what}");
    free(oracle);
}

#[test]
fn super_zoom_makes_a_gray_document_of_the_folded_result() {
    for depth in [8, 16] {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        let zoomed = coloured(2 * W, 2 * H);
        let zoom = |doc| {
            let mut err = ptr::null_mut();
            let out = unsafe {
                dr_precision_doc_super_zoom_document(doc, &zoomed, LIMIT, 0, ptr::null(), &mut err)
            };
            result(out, err).unwrap().unwrap()
        };
        let ours = zoom(gray);
        let theirs = zoom(rgb);
        assert_eq!(unsafe { &*ours }.dimensions(), (2 * W, 2 * H));
        assert_twin_layer(
            ours,
            theirs,
            0,
            depth,
            &format!("{depth}-bit super zoom"),
            0,
        );
        free(theirs);
        free(ours);
        free(rgb);
        free(gray);
    }
}

#[test]
fn a_neural_layer_lands_folded_in_place_and_as_a_new_layer() {
    let name = CString::new("JPEG Artifacts Removal").unwrap();
    for depth in [8, 16] {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        let pixels = coloured(W, H);
        for new_layer in [false, true] {
            let apply = |doc| {
                let mut err = ptr::null_mut();
                let out = unsafe {
                    dr_precision_doc_neural_layer_apply(
                        doc,
                        0,
                        &pixels,
                        new_layer,
                        if new_layer {
                            name.as_ptr()
                        } else {
                            ptr::null()
                        },
                        LIMIT,
                        0,
                        ptr::null(),
                        &mut err,
                    )
                };
                result(out, err).unwrap().unwrap()
            };
            let ours = apply(gray);
            let theirs = apply(rgb);
            let index = usize::from(new_layer);
            let what = format!("{depth}-bit neural apply, new layer {new_layer}");
            assert_twin_layer(ours, theirs, index, depth, &what, index);
            free(theirs);
            free(ours);
        }
        free(rgb);
        free(gray);
    }
}

type Picture = (Vec<u8>, (u32, u32));

fn solid(size: (u32, u32), rgb: [u8; 3]) -> Picture {
    let pixels = (0..size.0 * size.1)
        .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
        .collect();
    (pixels, size)
}

const NATIVE: (u32, u32) = (64, 64);

/// A Generative Fill layer over the left two thirds of the canvas from two
/// solid pictures; answers the document and the new layer's index.
fn gen_fill(doc: *const PrecisionDocument) -> (*mut PrecisionDocument, usize) {
    let coverage: Vec<u16> = (0..W * H)
        .map(|i| if i % W < 2 * W / 3 { 65_535 } else { 0 })
        .collect();
    let pictures = [solid(NATIVE, [200, 30, 40]), solid(NATIVE, [30, 60, 220])];
    let mut bytes = Vec::new();
    let mut sizes = Vec::new();
    for (pixels, size) in &pictures {
        bytes.extend_from_slice(pixels);
        sizes.extend([size.0, size.1]);
    }
    let name = CString::new("a red boat").unwrap();
    let tag = CString::new(
        r#"{"type":"generated","version":1,"feature":"generative_fill","provider":"gemini","model":"gemini-nano-banana-2.1","prompt":"a red boat"}"#,
    )
    .unwrap();
    let mut index = usize::MAX;
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_gen_fill_apply(
            doc,
            0,
            coverage.as_ptr(),
            coverage.len(),
            NATIVE.0,
            NATIVE.1,
            bytes.as_ptr(),
            bytes.len(),
            sizes.as_ptr(),
            sizes.len(),
            name.as_ptr(),
            tag.as_ptr(),
            &mut index,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    (result(out, err).unwrap().unwrap(), index)
}

fn select_variation(doc: *const PrecisionDocument, layer: usize, k: u32) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_gen_fill_select_variation(doc, layer, k, LIMIT, 0, ptr::null(), &mut err)
    };
    result(out, err).unwrap().unwrap()
}

/// Layer `index` of `doc` alone as a one-layer document: the oracle side of
/// a Generative Layer, its pixels converted with no binding to carry.
fn layer_alone(doc: *const PrecisionDocument, index: usize) -> *mut PrecisionDocument {
    let image = unsafe { &*doc }.layer_source(index).unwrap().clone();
    let alone = PrecisionDocument::from_image(image, Metadata::default(), Resolution::default());
    Box::into_raw(Box::new(alone.unwrap()))
}

/// [`assert_twin_layer`] for a Generative Layer: its raster through the
/// conversion alone.
fn assert_twin_generated(
    ours: *const PrecisionDocument,
    theirs: *const PrecisionDocument,
    index: usize,
    depth: u32,
    what: &str,
) {
    let theirs = layer_alone(theirs, index);
    let ours_alone = layer_alone(ours, index);
    assert_eq!(layer(ours_alone, 0, depth), layer(ours, index, depth));
    free(ours_alone);
    assert_twin_layer(ours, theirs, index, depth, what, 0);
    free(theirs);
}

#[test]
fn a_generative_fill_and_its_second_variation_land_folded() {
    for depth in [8, 16] {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        let (ours, index) = gen_fill(gray);
        let (theirs, twin_index) = gen_fill(rgb);
        assert_eq!(index, twin_index);
        assert_twin_generated(ours, theirs, index, depth, &format!("{depth}-bit fill"));
        let ours2 = select_variation(ours, index, 1);
        let theirs2 = select_variation(theirs, index, 1);
        assert_twin_generated(
            ours2,
            theirs2,
            index,
            depth,
            &format!("{depth}-bit variation"),
        );
        // Back to the first picture: the record's own rendered raster.
        let ours3 = select_variation(ours2, index, 0);
        assert_eq!(layer(ours3, index, depth), layer(ours, index, depth));
        for doc in [ours3, theirs2, ours2, theirs, ours, rgb, gray] {
            free(doc);
        }
    }
}

#[test]
fn the_generative_plan_carries_the_fold() {
    for depth in [8, 16] {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        let plan = |doc| {
            let mut err = ptr::null_mut();
            let bytes = unsafe { dr_precision_doc_gen_fill_plan(doc, 64, 64, 2, &mut err) };
            assert!(err.is_null(), "{}", take_err_string(err));
            bytes
        };
        assert_eq!(plan(gray), plan(rgb) + 8 * u64::from(W * H), "{depth}");
        free(rgb);
        free(gray);
    }
}

#[test]
fn merge_to_hdr_of_gray_frames_is_a_32_bit_gray_document() {
    let request =
        CString::new(r#"{"version":1,"layers":[0,1],"evs":[0,1],"reference":0,"deghost":false}"#)
            .unwrap();
    for depth in [8, 16] {
        // Two R = G = B frames: the layer and Stamp Visible's copy of it.
        let one = gray_doc(W, H, depth);
        let gray = edit(
            one,
            &json!({"op":"stamp_visible","name":"Stamp","anchor":0}),
            &[],
        )
        .unwrap()
        .unwrap();
        free(one);
        let rgb = twin(gray);
        let merge = |doc| {
            let (mut err, mut report) = (ptr::null_mut(), ptr::null_mut());
            let out = unsafe {
                dr_precision_doc_merge_hdr(
                    doc,
                    request.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut report,
                    &mut err,
                )
            };
            if !report.is_null() {
                unsafe { dr_string_free(report) };
            }
            result(out, err).unwrap().unwrap()
        };
        let ours = merge(gray);
        let theirs = merge(rgb);
        assert!(unsafe { &*ours }.is_float());
        assert_twin_layer(
            ours,
            theirs,
            0,
            32,
            &format!("{depth}-bit frames merged"),
            0,
        );
        free(theirs);
        free(ours);
        free(rgb);
        free(gray);
    }
}
