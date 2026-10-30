//! Edit ▸ Stroke… with nothing selected: `dr_doc_stroke_layer_edge`
//! and `dr_doc_stroke_layer_edge_scratch`, black-box through the FFI.
//!
//! The oracles are independent of the op: the old canvas path (the
//! layer's alpha plane, `dr_selection_stroke`, `dr_doc_expand_retouch_destination`
//! and `dr_doc_fill_selection`) for an in-canvas layer; the same layer at
//! another placement for translation invariance; a canvas band computed on a
//! padded canvas for the growth rect. The frame refactor of the paint path is
//! pinned by digests captured on the unmodified source.

use std::ffi::{c_char, c_int, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_channel::dr_doc_layer_plane;
use doctored_core::ffi_comps::dr_doc_edit_layer_comps;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_fill_stroke::*;
use doctored_core::ffi_group::dr_doc_with_layer_locks;
use doctored_core::ffi_jobs::{
    dr_job_control_create, dr_job_control_free, dr_job_control_set_scratch_limit,
};
use doctored_core::ffi_retained_source::dr_doc_mixed_source_edit;
use doctored_core::ffi_retouch::dr_doc_expand_retouch_destination;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use image::{Rgba, RgbaImage};
use serde_json::json;

use crate::common;
use common::sha256::sha256_hex;
use common::*;

const INSIDE: c_int = 0;
const CENTER: c_int = 1;
const OUTSIDE: c_int = 2;
const PLANE_ALPHA: c_int = 3;
const BLUE: [u8; 4] = [20, 40, 230, 255];
const RETAINED: &str = "Rasterize the retained source before stroking its pixels";

/// A varied straight-alpha image.
fn varied(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let i = y * w + x;
        Rgba([
            (i * 37 % 256) as u8,
            (i * 11 % 256) as u8,
            (200 + i * 3 % 56) as u8,
            (64 + i * 23 % 192) as u8,
        ])
    })
}

/// A 30×24 soft-edged disc of radius 13: clipped by the top and bottom
/// edges of its own rect, so an Outside stroke must grow it.
fn soft_disc() -> RgbaImage {
    RgbaImage::from_fn(30, 24, |x, y| {
        let d = ((x as f32 + 0.5 - 15.0).powi(2) + (y as f32 + 0.5 - 12.0).powi(2)).sqrt();
        let a = ((13.0 - d + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
        Rgba([(x * 8) as u8, (y * 10) as u8, 120, a])
    })
}

/// A `w×h` opaque background with `layer` above it at `offset`.
fn placed(w: u32, h: u32, layer: &RgbaImage, offset: (i32, i32)) -> DrDocument {
    let mut doc = DrDocument::from_pixels(opaque_pattern(w, h))
        .adding_image_layer(0, layer.clone(), "Layer")
        .expect("a layer");
    doc.layers[1].offset = offset;
    doc
}

/// The op through the FFI: the document (NULL = None) and the error, if any.
#[allow(clippy::too_many_arguments)]
fn stroke(
    doc: *const DrDocument,
    idx: usize,
    width: f32,
    location: c_int,
    mode: c_int,
    opacity: f32,
    preserve: bool,
    control: *const DrJobControl,
) -> (Option<DrDocument>, Option<String>) {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_stroke_layer_edge(
            doc,
            idx,
            width,
            location,
            BLUE.as_ptr(),
            mode,
            opacity,
            preserve,
            control,
            &mut err,
        )
    };
    take_doc(out, err)
}

/// The op that must succeed with a change.
fn stroked(doc: &DrDocument, width: f32, location: c_int, mode: c_int, opacity: f32) -> DrDocument {
    let (out, err) = stroke(doc, 1, width, location, mode, opacity, false, ptr::null());
    assert!(err.is_none(), "{err:?}");
    out.expect("the stroke changed the layer")
}

/// The old canvas path: the layer's alpha as canvas coverage, the
/// canvas band, the retouch expansion and the canvas fill.
fn old_path(
    doc: &DrDocument,
    width: f32,
    location: c_int,
    mode: c_int,
    opacity: f32,
) -> DrDocument {
    let (w, h) = (doc.width, doc.height);
    let mut mask = vec![0u8; (w * h) as usize];
    assert!(unsafe { dr_doc_layer_plane(doc, 1, PLANE_ALPHA, mask.as_mut_ptr(), w, h) });
    assert!(unsafe { dr_selection_stroke(mask.as_mut_ptr(), w, h, width, location) });
    let mut err = ptr::null_mut();
    let grown = unsafe { dr_doc_expand_retouch_destination(doc, 1, ptr::null(), &mut err) };
    let (grown, err) = take_doc(grown, err);
    assert!(err.is_none(), "{err:?}");
    let grown = grown.unwrap();
    let out = unsafe {
        dr_doc_fill_selection(
            &grown,
            1,
            mask.as_ptr(),
            w,
            h,
            BLUE.as_ptr(),
            mode,
            opacity,
            false,
        )
    };
    assert!(!out.is_null());
    *unsafe { Box::from_raw(out) }
}

fn flat(doc: &DrDocument) -> Vec<u8> {
    flat_pixels(doc)
}

fn alpha_of(doc: &DrDocument, idx: usize) -> Vec<u8> {
    layer_bytes(doc, idx)
        .chunks_exact(4)
        .map(|p| p[3])
        .collect()
}

/// Nonzero bounds `(x0, y0, x1, y1)` of a `w`-wide plane.
fn nonzero_bounds(plane: &[u8], w: u32) -> (i64, i64, i64, i64) {
    let mut b = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for (i, v) in plane.iter().enumerate() {
        if *v != 0 {
            let (x, y) = ((i as u32 % w) as i64, (i as u32 / w) as i64);
            b = (b.0.min(x), b.1.min(y), b.2.max(x + 1), b.3.max(y + 1));
        }
    }
    b
}

// ------------------------------------------------- the frame refactor pin --

/// The non-Normal blend path on a layer at a NEGATIVE offset, painted with a
/// canvas-sized overlay, plus Normal, Erase and the canvas fill: byte-identical
/// to the pre-frame implementation (digests captured on the unmodified
/// source), except the fill's Multiply and preserved Dissolve rows, re-pinned
/// for the exact fill colour and the preserve-transparency lerp.
#[test]
fn painting_with_a_negative_offset_is_pinned() {
    let (cw, ch) = (120u32, 100u32);
    let mut doc = DrDocument::from_pixels(varied(cw, ch))
        .adding_image_layer(0, varied(90, 50), "Layer")
        .expect("a layer");
    doc.layers[1].offset = (-50, 30);
    let overlay: Vec<u8> = (0..cw * ch)
        .flat_map(|i| {
            let a = (i * 13 % 256) as u8;
            let c = |k: u32| ((u32::from(a) * ((i * k) % 256)) / 255) as u8;
            [c(7), c(19), c(29), a]
        })
        .collect();
    for (mode, want) in [
        (
            BLEND_MULTIPLY,
            "02d9cf73f48effa9872338ebcff573ffde87c94957d4b22c9a729c70205e27a9",
        ),
        (
            BLEND_DISSOLVE,
            "a36942567d727954d9b9386b4f3c71d06999a6a5bf8a3b3285bb289fbbdea094",
        ),
        (
            BLEND_NORMAL,
            "6878a344daf6b1a11e11961887e9c166c431db8cf16fa35e956fa7ed69ecca07",
        ),
    ] {
        let out =
            unsafe { dr_doc_painting_layer_blend(&doc, 1, overlay.as_ptr(), cw, ch, mode, 0.8) };
        assert!(!out.is_null());
        assert_eq!(sha256_hex(&layer_pixels(out, 1)), want, "mode {mode}");
        unsafe { dr_doc_free(out) };
    }
    let out =
        unsafe { dr_doc_painting_layer(&doc, 1, overlay.as_ptr(), cw, ch, COMPOSITE_ERASE, 0.8) };
    assert!(!out.is_null());
    assert_eq!(
        sha256_hex(&layer_pixels(out, 1)),
        "03f768b04e4a8289e44a36845f88614e7e475e03ad6b99d7e231b6ed026cf373"
    );
    unsafe { dr_doc_free(out) };
    let mask: Vec<u8> = (0..cw * ch).map(|i| (i * 7 % 256) as u8).collect();
    for (mode, preserve, want) in [
        (
            BLEND_MULTIPLY,
            false,
            // Re-pinned: the non-Normal fill blends the exact
            // colour at partial mask coverage.
            "7898f516c7517a4b9d8d0a90dc35c192fb5ccab45a004fddb5f42615895fc684",
        ),
        (
            BLEND_DISSOLVE,
            true,
            // Re-pinned: the Dissolve fill shows the exact
            // colour, and preserved semi-transparent pixels keep their alpha
            // with the colour where the dither fires.
            "1d2c516bac83d94a57cf10c6e6bcc71415af25adae6e9c0fa444d49c8742936a",
        ),
    ] {
        let out = unsafe {
            dr_doc_fill_selection(
                &doc,
                1,
                mask.as_ptr(),
                cw,
                ch,
                [200, 60, 30, 230].as_ptr(),
                mode,
                0.7,
                preserve,
            )
        };
        assert!(!out.is_null());
        assert_eq!(sha256_hex(&layer_pixels(out, 1)), want, "fill {mode}");
        unsafe { dr_doc_free(out) };
    }
}

// --------------------------------------------------------------- (i) --

#[test]
fn i_an_in_canvas_layer_flattens_as_the_old_canvas_path() {
    // 7+ px of canvas around the layer at width 4 (≥ width + 3).
    let doc = placed(80, 70, &soft_disc(), (20, 18));
    for width in [4.0f32, 2.5] {
        for location in [INSIDE, CENTER, OUTSIDE] {
            for mode in [BLEND_NORMAL, BLEND_MULTIPLY] {
                let new = stroked(&doc, width, location, mode, 0.75);
                let old = old_path(&doc, width, location, mode, 0.75);
                assert_eq!(
                    flat(&new),
                    flat(&old),
                    "width {width} location {location} mode {mode}"
                );
            }
        }
    }
}

// -------------------------------------------------------------- (ii) --

#[test]
fn ii_the_stroke_is_the_same_wherever_the_layer_sits() {
    let layer = soft_disc();
    let half_off = placed(60, 60, &layer, (-15, 20));
    let inside = placed(200, 200, &layer, (80, 80));
    for location in [INSIDE, CENTER, OUTSIDE] {
        for mode in [BLEND_NORMAL, BLEND_MULTIPLY] {
            let a = stroked(&half_off, 4.0, location, mode, 0.75);
            let b = stroked(&inside, 4.0, location, mode, 0.75);
            let (oa, ob) = (a.layers[1].offset, b.layers[1].offset);
            assert_eq!(
                (ob.0 - oa.0, ob.1 - oa.1),
                (95, 60),
                "offset moves by the placement difference"
            );
            assert_eq!(
                a.layers[1].pixels.dimensions(),
                b.layers[1].pixels.dimensions()
            );
            assert_eq!(
                layer_bytes(&a, 1),
                layer_bytes(&b, 1),
                "location {location}"
            );
        }
    }
    // The canvas edge is not a contour: a tall opaque layer straddling the
    // left edge (its own top and bottom edges off the canvas) keeps canvas
    // column 0 under an Inside stroke.
    let tall = placed(
        60,
        60,
        &RgbaImage::from_pixel(30, 80, Rgba([90, 200, 40, 255])),
        (-15, -10),
    );
    let column = |d: &DrDocument| {
        let f = flat(d);
        (0..60).map(|y| pixel(&f, 60, 0, y)).collect::<Vec<_>>()
    };
    let stroked_tall = stroked(&tall, 4.0, INSIDE, BLEND_NORMAL, 1.0);
    assert_eq!(
        column(&tall),
        column(&stroked_tall),
        "canvas column 0 untouched"
    );
    assert_ne!(
        flat(&tall),
        flat(&stroked_tall),
        "the right edge is stroked"
    );
}

// ------------------------------------------------------------- (iii) --

#[test]
fn iii_an_outside_stroke_grows_the_layer_past_the_canvas() {
    // An opaque 20×16 layer 10 px past the left edge, with a mask.
    let layer = RgbaImage::from_pixel(20, 16, Rgba([90, 200, 40, 255]));
    let doc = placed(60, 60, &layer, (-10, 10));
    let masked = unsafe { dr_doc_adding_layer_mask(&doc, 1, MASK_REVEAL_ALL, ptr::null(), 0, 0) };
    assert!(!masked.is_null());
    let doc = *unsafe { Box::from_raw(masked) };
    let mask_before = (
        doc.layers[1].mask_properties.offset,
        doc.layers[1].mask.as_ref().map(|m| m.dimensions()),
    );
    assert!(mask_before.1.is_some());
    let width = 5.0f32;
    let out = stroked(&doc, width, OUTSIDE, BLEND_NORMAL, 1.0);
    // The oracle: the canvas band of the same rect on a canvas padded by 20.
    let (pw, ph) = (100u32, 100u32);
    let mut plane = vec![0u8; (pw * ph) as usize];
    for y in 0..16 {
        for x in 0..20 {
            plane[((y + 20 + 10) * pw + (x + 20 - 10)) as usize] = 255;
        }
    }
    assert!(unsafe { dr_selection_stroke(plane.as_mut_ptr(), pw, ph, width, OUTSIDE) });
    let (bx0, by0, bx1, by1) = nonzero_bounds(&plane, pw);
    let (x0, y0) = ((bx0 - 20).min(-10), (by0 - 20).min(10));
    let (x1, y1) = ((bx1 - 20).max(10), (by1 - 20).max(26));
    let grown = &out.layers[1];
    assert!(
        grown.offset.0 < -10,
        "the layer grew further past the canvas"
    );
    assert_eq!(
        (i64::from(grown.offset.0), i64::from(grown.offset.1)),
        (x0, y0)
    );
    let (gw, gh) = grown.pixels.dimensions();
    assert_eq!((i64::from(gw), i64::from(gh)), (x1 - x0, y1 - y0));
    // The band's off-canvas column beside the layer's middle row holds the
    // stroke colour.
    let px = layer_bytes(&out, 1);
    let first = pixel(&px, gw, (-11 - x0) as u32, (18 - y0) as u32);
    assert_eq!(first, BLUE, "the off-canvas band is painted");
    let mask_after = (
        grown.mask_properties.offset,
        grown.mask.as_ref().map(|m| m.dimensions()),
    );
    assert_eq!(mask_before, mask_after, "mask placement unchanged");
}

// -------------------------------------------------------------- (iv) --

#[test]
fn iv_kept_alpha_never_grows() {
    let doc = placed(80, 70, &soft_disc(), (-5, 18));
    let before = alpha_of(&doc, 1);
    let locked = unsafe { dr_doc_with_layer_locks(&doc, 1, LOCK_TRANSPARENCY) };
    assert!(!locked.is_null());
    let locked = *unsafe { Box::from_raw(locked) };
    let mut changed = 0;
    for location in [CENTER, OUTSIDE, INSIDE] {
        let (preserved, err) = stroke(&doc, 1, 4.0, location, BLEND_NORMAL, 1.0, true, ptr::null());
        assert!(err.is_none(), "{err:?}");
        let (lock, err) = stroke(
            &locked,
            1,
            4.0,
            location,
            BLEND_NORMAL,
            1.0,
            false,
            ptr::null(),
        );
        assert!(err.is_none(), "{err:?}");
        for out in [preserved, lock].into_iter().flatten() {
            changed += 1;
            let layer = &out.layers[1];
            assert_eq!(layer.offset, (-5, 18), "location {location}");
            assert_eq!(layer.pixels.dimensions(), (30, 24));
            assert_eq!(alpha_of(&out, 1), before, "alpha byte-identical");
        }
    }
    assert!(changed >= 4, "Center and Inside recolour under kept alpha");
}

// --------------------------------------------------------------- (v) --

fn error_of(doc: &DrDocument, idx: usize, width: f32, mode: c_int, opacity: f32) -> String {
    let (out, err) = stroke(doc, idx, width, OUTSIDE, mode, opacity, false, ptr::null());
    assert!(out.is_none());
    err.expect("a refusal names itself")
}

#[test]
fn v_refusals_name_themselves() {
    let doc = placed(60, 60, &soft_disc(), (10, 10));
    assert!(!error_of(&doc, 7, 4.0, BLEND_NORMAL, 1.0).is_empty());
    let (grouped, group, _, _) = doc.group_layers(&[1], "Group").expect("a group");
    assert!(!error_of(&grouped, group, 4.0, BLEND_NORMAL, 1.0).is_empty());
    let pixel_locked = unsafe { dr_doc_with_layer_locks(&doc, 1, LOCK_PIXELS) };
    assert!(!pixel_locked.is_null());
    let pixel_locked = *unsafe { Box::from_raw(pixel_locked) };
    assert!(!error_of(&pixel_locked, 1, 4.0, BLEND_NORMAL, 1.0).is_empty());
    for width in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(!error_of(&doc, 1, width, BLEND_NORMAL, 1.0).is_empty());
    }
    assert!(!error_of(&doc, 1, 4.0, BLEND_NORMAL, f32::NAN).is_empty());
    assert!(!error_of(&doc, 1, 4.0, BLEND_PASS_THROUGH, 1.0).is_empty());
    assert!(!error_of(&doc, 1, 4.0, 99, 1.0).is_empty());
    // An unknown location and a NULL colour are refused at the FFI.
    for (location, rgba) in [(3, BLUE.as_ptr()), (OUTSIDE, ptr::null())] {
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_doc_stroke_layer_edge(
                &doc,
                1,
                4.0,
                location,
                rgba,
                BLEND_NORMAL,
                1.0,
                false,
                ptr::null(),
                &mut err,
            )
        };
        let (out, err) = take_doc(out, err);
        assert!(out.is_none() && err.is_some());
    }
    // A placed (retained-source) layer: the exact sentence.
    let source = DrDocument::from_pixels(soft_disc());
    let request = json!({"op":"place","source_id":"asset","name":"Placed","x":5,"y":5});
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let placed_doc = unsafe {
        dr_doc_mixed_source_edit(
            &doc,
            &source,
            ptr::null(),
            request.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let (placed_doc, err) = take_doc(placed_doc, err);
    assert!(err.is_none(), "{err:?}");
    let placed_doc = placed_doc.unwrap();
    let top = placed_doc.layers.len() - 1;
    assert!(placed_doc.layers[top].retained_source.is_some());
    assert_eq!(error_of(&placed_doc, top, 4.0, BLEND_NORMAL, 1.0), RETAINED);
    // Opacity 0 changes nothing: NULL with no error.
    let (out, err) = stroke(&doc, 1, 4.0, OUTSIDE, BLEND_NORMAL, 0.0, false, ptr::null());
    assert!(out.is_none() && err.is_none());
}

// -------------------------------------------------------------- (vi) --

#[test]
fn vi_an_empty_layer_and_the_scratch_limit() {
    let empty = placed(60, 60, &RgbaImage::new(20, 20), (10, 10));
    assert_eq!(
        unsafe { dr_doc_stroke_layer_edge_scratch(&empty, 1, 4.0) },
        0
    );
    let (out, err) = stroke(
        &empty,
        1,
        4.0,
        OUTSIDE,
        BLEND_NORMAL,
        1.0,
        false,
        ptr::null(),
    );
    assert!(out.is_none() && err.is_none());

    let doc = placed(60, 60, &soft_disc(), (10, 10));
    let scratch = unsafe { dr_doc_stroke_layer_edge_scratch(&doc, 1, 4.0) };
    // F is the disc's 26×24 alpha bounds (x 2..28) grown by ceil(4) + 2 = 6
    // on every side: 38×36; the union with the 30×24 layer is F itself.
    assert_eq!(scratch, 24 * 38 * 36 + 4 * 38 * 36 + 4096);
    for bad in [0.0f32, -2.0, f32::NAN] {
        assert_eq!(unsafe { dr_doc_stroke_layer_edge_scratch(&doc, 1, bad) }, 0);
    }
    assert_eq!(unsafe { dr_doc_stroke_layer_edge_scratch(&doc, 9, 4.0) }, 0);
    unsafe {
        let control = dr_job_control_create();
        assert!(dr_job_control_set_scratch_limit(control, scratch - 1));
        let (out, err) = stroke(&doc, 1, 4.0, OUTSIDE, BLEND_NORMAL, 1.0, false, control);
        assert!(out.is_none());
        assert!(err.is_some(), "a limit below the scratch refuses");
        dr_job_control_free(control);
        let control = dr_job_control_create();
        assert!(dr_job_control_set_scratch_limit(control, scratch));
        let (out, err) = stroke(&doc, 1, 4.0, OUTSIDE, BLEND_NORMAL, 1.0, false, control);
        assert!(out.is_some() && err.is_none(), "{err:?}");
        dr_job_control_free(control);
    }
}

// ------------------------------------------------------------- (vii) --

#[test]
fn vii_null_sweep() {
    let doc = placed(60, 60, &soft_disc(), (10, 10));
    unsafe {
        assert_eq!(dr_doc_stroke_layer_edge_scratch(ptr::null(), 1, 4.0), 0);
        let mut err = ptr::null_mut();
        let out = dr_doc_stroke_layer_edge(
            ptr::null(),
            1,
            4.0,
            OUTSIDE,
            BLUE.as_ptr(),
            BLEND_NORMAL,
            1.0,
            false,
            ptr::null(),
            &mut err,
        );
        assert!(out.is_null() && err.is_null());
        // A NULL err_out still refuses, and still succeeds.
        let out = dr_doc_stroke_layer_edge(
            &doc,
            1,
            4.0,
            OUTSIDE,
            ptr::null(),
            BLEND_NORMAL,
            1.0,
            false,
            ptr::null(),
            ptr::null_mut(),
        );
        assert!(out.is_null());
        let out = dr_doc_stroke_layer_edge(
            &doc,
            1,
            4.0,
            OUTSIDE,
            BLUE.as_ptr(),
            BLEND_NORMAL,
            1.0,
            false,
            ptr::null(),
            ptr::null_mut(),
        );
        assert!(!out.is_null());
        dr_doc_free(out);
    }
}

// ------------------------------------------------- comps follow a growth --

fn comp(doc: &DrDocument, request: serde_json::Value) -> DrDocument {
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_edit_layer_comps(doc, request.as_ptr(), &mut err) };
    let (out, err) = take_doc(out, err);
    assert!(err.is_none(), "{err:?}");
    out.expect("the comp edit changed the document")
}

/// Saves a comp with Position on, grows the layer through `grow`, moves it,
/// applies the comp and asserts the layer's content is back where it was.
fn comp_survives(grow: impl Fn(&DrDocument) -> DrDocument, what: &str) {
    let doc = placed(60, 60, &soft_disc(), (12, 14));
    let doc = comp(
        &doc,
        json!({"op":"layer_comp","action":"save","name":"Here","position":true}),
    );
    let grown = grow(&doc);
    let origin = grown.layers[1].offset;
    assert_ne!(origin, (12, 14), "{what}: the growth moved the origin");
    let moved = grown
        .with_layer_offset(1, origin.0 + 9, origin.1 + 4)
        .unwrap();
    let applied = comp(&moved, json!({"op":"layer_comp","action":"apply","comp":0}));
    assert_eq!(
        applied.layers[1].offset, origin,
        "{what}: content lands where it was"
    );
    assert_eq!(flat(&applied), flat(&grown), "{what}");
}

#[test]
fn comps_follow_a_layer_edge_growth_and_a_retouch_expansion() {
    comp_survives(
        |d| stroked(d, 4.0, OUTSIDE, BLEND_NORMAL, 1.0),
        "outside stroke",
    );
    comp_survives(
        |d| stroked(d, 6.0, CENTER, BLEND_NORMAL, 1.0),
        "center stroke",
    );
    comp_survives(
        |d| {
            let mut err = ptr::null_mut();
            let out = unsafe { dr_doc_expand_retouch_destination(d, 1, ptr::null(), &mut err) };
            let (out, err) = take_doc(out, err);
            assert!(err.is_none(), "{err:?}");
            out.unwrap()
        },
        "retouch expansion",
    );
}

#[test]
fn a_comp_without_position_is_left_alone_by_a_growth() {
    let doc = placed(60, 60, &soft_disc(), (12, 14));
    let doc = comp(
        &doc,
        json!({"op":"layer_comp","action":"save","name":"Look","position":false}),
    );
    let grown = stroked(&doc, 4.0, OUTSIDE, BLEND_NORMAL, 1.0);
    let origin = grown.layers[1].offset;
    assert_ne!(origin, (12, 14), "the growth moved the origin");
    // Applying it changes nothing: the growth recorded no position for it.
    let request =
        CString::new(json!({"op":"layer_comp","action":"apply","comp":0}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_edit_layer_comps(&grown, request.as_ptr(), &mut err) };
    let (out, err) = take_doc(out, err);
    assert!(err.is_none(), "{err:?}");
    assert!(out.is_none(), "no position was recorded");
}

// ------------------------------------------------ the one-scan copy pin --

/// A 320×40 soft-edged ellipse spanning x 20..300: its alpha crosses the
/// 256-px tile edge, so every row of it reads as more than one run.
fn wide_ellipse() -> RgbaImage {
    RgbaImage::from_fn(320, 40, |x, y| {
        let (dx, dy) = (
            (x as f32 + 0.5 - 160.0) / 140.0,
            (y as f32 + 0.5 - 20.0) / 14.0,
        );
        let d = (dx * dx + dy * dy).sqrt();
        let a = (((1.0 - d) * 14.0 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
        Rgba([(x % 256) as u8, (y * 6) as u8, 90, a])
    })
}

/// The layer-edge stroke's output, offset, dimensions and scratch figure,
/// byte-identical across the one-scan / row-segment copy: digests
/// captured on the unmodified source. Row (d) is wider than a tile and
/// partly off canvas.
#[test]
fn layer_edge_stroke_output_is_pinned() {
    let disc = placed(60, 60, &soft_disc(), (-7, 9));
    let wide = placed(60, 60, &wide_ellipse(), (-200, 10));
    #[allow(clippy::type_complexity)]
    let rows: [(
        &DrDocument,
        f32,
        c_int,
        c_int,
        f32,
        bool,
        &str,
        ((i32, i32), (u32, u32)),
        u64,
    ); 4] = [
        (
            &disc,
            4.0,
            OUTSIDE,
            BLEND_NORMAL,
            1.0,
            false,
            "0c4a25f63e5f006bb6b9b50f9144b945c438dbc198760ca5c211e999d0f1ce73",
            ((-9, 5), (34, 32)),
            42400,
        ),
        (
            &disc,
            3.0,
            INSIDE,
            BLEND_MULTIPLY,
            0.6,
            false,
            // Re-pinned: a non-Normal fill blends the exact colour
            // at partial coverage (the soft band edge), not one
            // un-premultiplied out of a low-alpha overlay byte.
            "c00ce6dfddb3b6dc7509b4ce660876115acce5f0a62e6bd81a0db7348d8a0165",
            ((-7, 9), (30, 24)),
            38368,
        ),
        (
            &disc,
            5.5,
            CENTER,
            BLEND_NORMAL,
            0.8,
            true,
            // Re-pinned: preserved semi-transparent pixels on the
            // soft disc take lerp(Cb, Cs, sa) at their own alpha.
            "c9516aa53465f70273b5e49b3ce5b19b23a17ace821fa409790e653bbaae41c6",
            ((-7, 9), (30, 24)),
            51136,
        ),
        (
            &wide,
            3.0,
            OUTSIDE,
            BLEND_SCREEN,
            0.9,
            false,
            // Re-pinned: the Screen fill blends the exact colour at
            // the band's partial coverage.
            "124f9583d68166972d59694f3f4a59bafc11d32280de06e8c8ce7886de66b0ac",
            ((-200, 10), (320, 40)),
            328896,
        ),
    ];
    for (i, (doc, width, location, mode, opacity, preserve, digest, rect, scratch)) in
        rows.into_iter().enumerate()
    {
        let (out, err) = stroke(
            doc,
            1,
            width,
            location,
            mode,
            opacity,
            preserve,
            ptr::null(),
        );
        assert!(err.is_none(), "row {i}: {err:?}");
        let out = out.expect("the stroke changed the layer");
        let got = (
            sha256_hex(&layer_bytes(&out, 1)),
            (out.layers[1].offset, out.layers[1].pixels.dimensions()),
            unsafe { dr_doc_stroke_layer_edge_scratch(doc, 1, width) },
        );
        assert_eq!(got, (digest.to_string(), rect, scratch), "row {i}");
    }
}
