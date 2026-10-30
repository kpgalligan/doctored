//! #1 Lab phase 6b-iii: the neural and generative tools on a Lab Color
//! document through the FFI. The model or provider reads the document's
//! Display P3 view, and its result, tagged Display P3, returns to L\*a\*b\*
//! codes by Image ▸ Mode ▸ Lab Color's rule (`lab_neural`).
//!
//! # The oracle
//!
//! An RGB document in the built-in Display P3 holding the same model output,
//! its result then taken through `dr_precision_doc_convert_to_lab` at the
//! Lab document's depth: the way back IS that conversion of the new pixels.
//!
//! * (a) Super Zoom's document is Lab at the source's depth and equals the
//!   oracle; at 16 bits within 0.2 L\* and 0.3 a\*/b\* of the hand-computed
//!   L\*a\*b\* of the picture (`lab_fixture::p3_to_lab`), at 8 bits on the
//!   ×257 grid.
//! * (b) `neural_layer_apply`, in place and as a new layer, equals the
//!   oracle; a twin-tagged result refuses (`lab_neural::NOT_VIEW`).
//! * (c) Colorize of the layer's view (`dr_precision_image_lab_display`)
//!   with the Display P3 bytes, through `neural_layer_apply`, equals the
//!   oracle; the twin's bytes refuse naming matrix/TRC.
//! * (d) `gen_fill_input`'s picture is within 1 code of the same call on the
//!   document converted to Display P3 (relative, no BPC).
//! * (e) a Generative Fill layer and its second variation equal Mode ▸ Lab
//!   Color of the oracle's raster, and the mask is the coverage.
//! * (f) the Lab plan is at least the Display P3 plan plus 8 B/px;
//!   `dr_lab_neural_scratch_bytes` covers the view and the Lab result.
//! * (g) a 16-bit Lab Generative Layer round-trips `.rz` with its variations
//!   live.
//! * (h) Sky Replacement refuses by name.
//!
//! # Bounds, declared before the first run
//!
//! Exact, unless stated above.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_gen_fill::{
    dr_precision_doc_gen_fill_apply, dr_precision_doc_gen_fill_input,
    dr_precision_doc_gen_fill_plan,
};
use doctored_core::ffi_gen_variations::{
    dr_precision_doc_gen_fill_select_variation, dr_precision_doc_gen_fill_variations,
};
use doctored_core::ffi_lab::{
    dr_lab_neural_scratch_bytes, dr_precision_doc_convert_lab_to_rgb,
    dr_precision_image_lab_display,
};
use doctored_core::ffi_neural::{
    dr_precision_doc_neural_layer_apply, dr_precision_doc_super_zoom_document,
    dr_precision_image_neural_colorize_apply,
};
use doctored_core::ffi_precision::{
    dr_precision_doc_open, dr_precision_doc_save_native, dr_precision_image_free,
};
use doctored_core::ffi_sky::dr_precision_doc_sky_replace;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use tempfile::TempDir;

use crate::common::take_err_string;
use crate::gray_ops::{free, result, LIMIT};
use crate::lab_fixture::{decode, p3_to_lab};
use crate::lab_targets::{codes, pattern, srgb_doc, to_lab, H, W};

const NOT_VIEW: &str = "a Lab Color document takes a model's result in its Display P3 view";

/// The pattern as a Lab document at `depth`.
fn lab_doc(depth: u32) -> *mut PrecisionDocument {
    let rgb = srgb_doc(pattern());
    let lab = to_lab(rgb, &format!(r#"{{"depth":{depth}}}"#)).unwrap();
    free(rgb);
    lab
}

/// An RGB document in the built-in Display P3 of one W × H layer holding
/// exactly `pixels`: the oracle's document.
fn p3_doc(pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    let p3 = IccProfile::display_p3();
    let image = PrecisionImage::from_rgba16(W, H, pixels, p3.clone(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Layer", image, (0, 0), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        p3,
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap();
    Box::into_raw(Box::new(doc))
}

/// A coloured Display P3 picture: the channels all differ, every seventh
/// pixel at partial alpha.
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
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::display_p3(), None).unwrap()
}

/// Layer `index` of `ours` (Lab) against layer `their_index` of `theirs`
/// (Display P3) converted to Lab at `depth`.
fn assert_oracle(
    ours: *const PrecisionDocument,
    theirs: *const PrecisionDocument,
    index: usize,
    their_index: usize,
    depth: u32,
    what: &str,
) {
    assert!(unsafe { &*ours }.is_lab(), "{what}: not Lab");
    let oracle = to_lab(theirs, &format!(r#"{{"depth":{depth}}}"#)).unwrap();
    assert_eq!(
        codes(ours, index),
        codes(oracle, their_index),
        "{what}: the oracle"
    );
    free(oracle);
}

fn zoom(
    doc: *const PrecisionDocument,
    zoomed: &PrecisionImage,
) -> Result<*mut PrecisionDocument, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_super_zoom_document(doc, zoomed, LIMIT, 0, ptr::null(), &mut err)
    };
    result(out, err).map(|d| d.expect("super zoom answered nothing"))
}

#[test]
fn super_zoom_makes_a_lab_document_of_the_view_result() {
    for depth in [8, 16] {
        let lab = lab_doc(depth);
        let rgb = p3_doc(pattern());
        let zoomed = coloured(2 * W, 2 * H);
        let ours = zoom(lab, &zoomed).unwrap();
        let theirs = zoom(rgb, &zoomed).unwrap();
        let d = unsafe { &*ours };
        assert!(d.is_lab(), "{depth}");
        assert_eq!(d.dimensions(), (2 * W, 2 * H));
        let what = format!("{depth}-bit super zoom");
        assert_oracle(ours, theirs, 0, 0, depth, &what);
        let got = codes(ours, 0);
        let source = zoomed.to_rgba16().unwrap();
        for (g, s) in got.iter().zip(&source) {
            if depth == 8 {
                assert!(
                    g.iter().all(|&c| c % 257 == 0),
                    "{what}: off the grid {g:?}"
                );
            } else {
                let want = p3_to_lab([s[0], s[1], s[2]].map(|c| f64::from(c) / 65535.0));
                let have = decode(*g);
                assert!(
                    (have[0] - want[0]).abs() <= 0.2,
                    "{what}: L* {have:?} vs {want:?}"
                );
                for c in 1..3 {
                    assert!(
                        (have[c] - want[c]).abs() <= 0.3,
                        "{what}: {have:?} vs {want:?}"
                    );
                }
                assert_eq!(g[3], s[3], "{what}: alpha");
            }
        }
        for doc in [theirs, ours, rgb, lab] {
            free(doc);
        }
    }
}

fn layer_apply(
    doc: *const PrecisionDocument,
    pixels: &PrecisionImage,
    new_layer: bool,
) -> Result<*mut PrecisionDocument, String> {
    let name = CString::new("JPEG Artifacts Removal").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_neural_layer_apply(
            doc,
            0,
            pixels,
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
    result(out, err).map(|d| d.expect("the apply answered nothing"))
}

#[test]
fn a_neural_layer_lands_as_lab_codes_in_place_and_as_a_new_layer() {
    for depth in [8, 16] {
        let lab = lab_doc(depth);
        let rgb = p3_doc(pattern());
        let pixels = coloured(W, H);
        for new_layer in [false, true] {
            let ours = layer_apply(lab, &pixels, new_layer).unwrap();
            let theirs = layer_apply(rgb, &pixels, new_layer).unwrap();
            let index = usize::from(new_layer);
            let what = format!("{depth}-bit neural apply, new layer {new_layer}");
            assert_oracle(ours, theirs, index, index, depth, &what);
            free(theirs);
            free(ours);
        }
        // A result in the twin is not the view's: it refuses.
        let twin = unsafe { &*lab }.layer_source(0).unwrap().clone();
        assert_eq!(layer_apply(lab, &twin, false).unwrap_err(), NOT_VIEW);
        free(rgb);
        free(lab);
    }
}

const SIDE: u32 = 16;

fn colorize(image: *const PrecisionImage, profile: &[u8]) -> Result<*mut PrecisionImage, String> {
    let chroma: Vec<f32> = (0..2 * SIDE * SIDE)
        .map(|i| if i < SIDE * SIDE { 20.0 } else { -15.0 })
        .collect();
    let adjust = CString::new("{}").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_image_neural_colorize_apply(
            image,
            profile.as_ptr(),
            profile.len(),
            SIDE,
            chroma.as_ptr(),
            chroma.len(),
            adjust.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

#[test]
fn colorize_runs_on_the_view_and_lands_as_lab_codes() {
    for depth in [8, 16] {
        let lab = lab_doc(depth);
        let source = unsafe { &*lab }.layer_source(0).unwrap().clone();
        let mut err = ptr::null_mut();
        let view =
            unsafe { dr_precision_image_lab_display(&source, LIMIT, 0, ptr::null(), &mut err) };
        assert!(!view.is_null(), "{}", take_err_string(err));
        let p3 = IccProfile::display_p3();
        let colorized = colorize(view, p3.bytes()).unwrap();
        let output = unsafe { &*colorized };
        let ours = layer_apply(lab, output, false).unwrap();
        let rgb = p3_doc(unsafe { &*view }.to_rgba16().unwrap());
        let theirs = layer_apply(rgb, output, false).unwrap();
        assert_oracle(ours, theirs, 0, 0, depth, &format!("{depth}-bit colorize"));
        // The twin is no matrix/TRC profile: Colorize refuses its bytes.
        let twin = unsafe { &*lab }.profile().bytes().to_vec();
        let refusal = colorize(&source, &twin).unwrap_err();
        assert!(refusal.contains("matrix/TRC"), "{refusal}");
        for doc in [theirs, rgb, ours, lab] {
            free(doc);
        }
        unsafe {
            dr_precision_image_free(colorized);
            dr_precision_image_free(view);
        }
    }
}

type Picture = (Vec<u8>, (u32, u32));

const NATIVE: (u32, u32) = (64, 64);

fn solid(rgb: [u8; 3]) -> Picture {
    let pixels = (0..NATIVE.0 * NATIVE.1)
        .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
        .collect();
    (pixels, NATIVE)
}

/// The left two thirds of the canvas.
fn coverage() -> Vec<u16> {
    (0..W * H)
        .map(|i| if i % W < 2 * W / 3 { 65_535 } else { 0 })
        .collect()
}

fn gen_input(doc: *const PrecisionDocument) -> Vec<u8> {
    let bytes = (4 * NATIVE.0 * NATIVE.1) as usize;
    let cover = coverage();
    let mut crop = [0u32; 4];
    let mut image = vec![7u8; bytes];
    let mut mask = vec![7u8; bytes];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_gen_fill_input(
            doc,
            0,
            cover.as_ptr(),
            cover.len(),
            NATIVE.0,
            NATIVE.1,
            crop.as_mut_ptr(),
            image.as_mut_ptr(),
            image.len(),
            mask.as_mut_ptr(),
            mask.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
    image
}

#[test]
fn the_generative_input_is_the_display_p3_view() {
    let options =
        CString::new(r#"{"intent":"relative","black_point_compensation":false}"#).unwrap();
    for depth in [8, 16] {
        let lab = lab_doc(depth);
        let p3 = IccProfile::display_p3();
        let mut err = ptr::null_mut();
        let rgb = unsafe {
            dr_precision_doc_convert_lab_to_rgb(
                lab,
                p3.bytes().as_ptr(),
                p3.bytes().len(),
                options.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!rgb.is_null(), "{}", take_err_string(err));
        let ours = gen_input(lab);
        let theirs = gen_input(rgb);
        for (i, (a, b)) in ours.iter().zip(&theirs).enumerate() {
            assert!(a.abs_diff(*b) <= 1, "{depth}-bit sample {i}: {a} vs {b}");
        }
        free(rgb);
        free(lab);
    }
}

/// A Generative Fill layer over [`coverage`] from two solid pictures;
/// answers the document and the new layer's index.
fn gen_fill(doc: *const PrecisionDocument) -> (*mut PrecisionDocument, usize) {
    let cover = coverage();
    let pictures = [solid([200, 30, 40]), solid([30, 60, 220])];
    let mut bytes = Vec::new();
    let mut sizes = Vec::new();
    for (pixels, size) in &pictures {
        bytes.extend_from_slice(pixels);
        sizes.extend([size.0, size.1]);
    }
    let name = CString::new("a stone").unwrap();
    let tag = CString::new(
        r#"{"type":"generated","version":1,"feature":"generative_fill","provider":"gemini","model":"gemini-nano-banana-2.1","prompt":"a stone"}"#,
    )
    .unwrap();
    let mut index = usize::MAX;
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_gen_fill_apply(
            doc,
            0,
            cover.as_ptr(),
            cover.len(),
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
/// a Generative Layer (its binding would detach in the conversion), its
/// pixels converted with nothing to carry.
fn layer_alone(doc: *const PrecisionDocument, index: usize) -> *mut PrecisionDocument {
    let image = unsafe { &*doc }.layer_source(index).unwrap().clone();
    let alone = PrecisionDocument::from_image(image, Metadata::default(), Resolution::default());
    Box::into_raw(Box::new(alone.unwrap()))
}

/// [`assert_oracle`] for a Generative Layer, plus its mask: the coverage.
fn assert_generated(
    ours: *const PrecisionDocument,
    theirs: *const PrecisionDocument,
    index: usize,
    depth: u32,
    what: &str,
) {
    let alone = layer_alone(theirs, index);
    assert_oracle(ours, alone, index, 0, depth, what);
    free(alone);
    let mask = unsafe { &*ours }
        .layer(index)
        .unwrap()
        .mask()
        .expect("a mask");
    assert_eq!(mask.plane().samples(), &coverage()[..], "{what}: the mask");
}

fn variations(doc: *const PrecisionDocument, layer: usize) -> Option<[u32; 8]> {
    let mut out = [0u32; 8];
    let mut err = ptr::null_mut();
    let found =
        unsafe { dr_precision_doc_gen_fill_variations(doc, layer, out.as_mut_ptr(), &mut err) };
    assert!(err.is_null(), "{}", take_err_string(err));
    found.then_some(out)
}

#[test]
fn a_generative_fill_and_its_second_variation_land_as_lab_codes() {
    for depth in [8, 16] {
        let lab = lab_doc(depth);
        let rgb = p3_doc(pattern());
        let (ours, index) = gen_fill(lab);
        let (theirs, twin_index) = gen_fill(rgb);
        assert_eq!(index, twin_index);
        assert_generated(ours, theirs, index, depth, &format!("{depth}-bit fill"));
        assert_eq!(variations(ours, index).unwrap()[0], 2);
        let ours2 = select_variation(ours, index, 1);
        let theirs2 = select_variation(theirs, index, 1);
        assert_generated(
            ours2,
            theirs2,
            index,
            depth,
            &format!("{depth}-bit variation"),
        );
        // Back to the first picture: the record's own rendered raster.
        let ours3 = select_variation(ours2, index, 0);
        assert_eq!(codes(ours3, index), codes(ours, index));
        for doc in [ours3, theirs2, ours2, theirs, ours, rgb, lab] {
            free(doc);
        }
    }
}

#[test]
fn the_plans_carry_the_view_and_the_lab_result() {
    for depth in [8, 16] {
        let lab = lab_doc(depth);
        let rgb = p3_doc(pattern());
        let plan = |doc| {
            let mut err = ptr::null_mut();
            let bytes = unsafe { dr_precision_doc_gen_fill_plan(doc, 64, 64, 2, &mut err) };
            assert!(err.is_null(), "{}", take_err_string(err));
            bytes
        };
        assert!(
            plan(lab) >= plan(rgb) + 8 * u64::from(W * H),
            "{depth}: {} vs {}",
            plan(lab),
            plan(rgb)
        );
        free(rgb);
        free(lab);
    }
    let (w, h) = (u64::from(W), u64::from(H));
    let bytes = unsafe { dr_lab_neural_scratch_bytes(W, H, 2 * W, 2 * H) };
    assert_ne!(bytes, u64::MAX);
    assert!(bytes >= 8 * w * h + 8 * 4 * w * h, "{bytes}");
}

#[test]
fn a_lab_generative_layer_round_trips_rz_with_live_variations() {
    let lab = lab_doc(16);
    let (doc, index) = gen_fill(lab);
    let dir = TempDir::new().unwrap();
    let path = CString::new(dir.path().join("lab-gen.rz").to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(doc, path.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        take_err_string(err)
    );
    let back = unsafe { dr_precision_doc_open(path.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!back.is_null(), "{}", take_err_string(err));
    assert!(unsafe { &*back }.is_lab());
    let live = variations(doc, index).expect("variations");
    assert_eq!(variations(back, index), Some(live));
    assert_eq!(live[0], 2);
    assert_eq!(codes(back, index), codes(doc, index));
    for d in [back, doc, lab] {
        free(d);
    }
}

#[test]
fn sky_replacement_refuses_a_lab_document() {
    let lab = lab_doc(16);
    let sky = vec![120u8; (4 * W * H) as usize];
    let sky = unsafe { dr_image_from_rgba(sky.as_ptr(), W, H) };
    assert!(!sky.is_null());
    let cover = coverage();
    let params = CString::new("{}").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_sky_replace(
            lab,
            0,
            sky,
            cover.as_ptr(),
            cover.len(),
            params.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(
        take_err_string(err),
        "sky replacement is not available in a Lab Color document"
    );
    unsafe { dr_image_free(sky) };
    free(lab);
}
