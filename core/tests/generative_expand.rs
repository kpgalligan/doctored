//! Generative Expand (M3b phase 2) through the FFI:
//! `dr_doc_gen_expand_coverage` and `dr_precision_doc_gen_expand_coverage`
//! at 8 and 16 bits (and the coverage alone on a 32-bit float document), then Generative Fill's frame, input and apply over that
//! coverage, with STUB answers in place of the cloud provider.
//!
//! # The oracles
//!
//! Written from the header's contract: the coverage is `65535 −` each
//! pixel's full-composite alpha (u16; a byte alpha × 257), so after Canvas
//! Size grows a 40 × 30 opaque picture to 60 × 50 at (10, 10) it is 65535
//! outside the old rect and 0 inside; the input mask is the hole on exactly
//! the cells whose footprint touches a covered pixel (phase 1's brute-force
//! cell oracle); the composite inside the old rect is the pre-expand
//! composite and the new area is the answer's colour.
//!
//! # Bounds, declared before the first run
//!
//! * The coverage, the input mask and every pixel inside the old rect: exact.
//! * The answer on the new area: exact at 8 bits, ±1 code at 16.

use std::ffi::CString;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::dr_doc_canvas_resize;
use doctored_core::ffi_gen_expand::*;
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use serde_json::json;

use crate::common::take_err_string;
use crate::float_fixture::float_doc;
use crate::generative_fill::{cells_oracle, frame, gradient, input_with, projection};
use crate::generative_variations::{apply, info, select, solid, Picture, COLORS, NATIVE};
use crate::sky_replacement::{build, Depth, Doc, LIMIT};

const DEPTHS: [Depth; 2] = [Depth::Byte, Depth::Sixteen];
const OLD: (u32, u32) = (40, 30);
const CANVAS: (u32, u32) = (60, 50);
const ORIGIN: (u32, u32) = (10, 10);
const TAG: &str = r#"{"type":"generated","version":1,"feature":"generative_expand","provider":"openai","model":"gpt-image-2.5-sunburst","prompt":"more beach"}"#;

// ---------------------------------------------------------------- calls --

fn expand_with(
    doc: &Doc,
    layer: usize,
    count: usize,
    limit: u64,
    control: *const DrJobControl,
) -> Result<Vec<u16>, String> {
    let mut out = vec![7u16; count];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        match doc {
            Doc::Byte(d) => dr_doc_gen_expand_coverage(
                d,
                layer,
                out.as_mut_ptr(),
                out.len(),
                limit,
                0,
                control,
                &mut err,
            ),
            Doc::Typed(d) => dr_precision_doc_gen_expand_coverage(
                d,
                layer,
                out.as_mut_ptr(),
                out.len(),
                limit,
                0,
                control,
                &mut err,
            ),
        }
    };
    if ok {
        assert!(err.is_null());
        Ok(out)
    } else {
        Err(take_err_string(err))
    }
}

fn expand(doc: &Doc) -> Result<Vec<u16>, String> {
    expand_with(doc, 0, (CANVAS.0 * CANVAS.1) as usize, LIMIT, ptr::null())
}

/// `doc` with its canvas grown to `CANVAS`, the picture at `ORIGIN`.
fn grown(doc: &Doc) -> Doc {
    let (w, h) = CANVAS;
    let (x, y) = (ORIGIN.0 as i32, ORIGIN.1 as i32);
    match doc {
        Doc::Byte(d) => {
            let out = unsafe { dr_doc_canvas_resize(d, w, h, x, y) };
            assert!(!out.is_null());
            Doc::Byte(*unsafe { Box::from_raw(out) })
        }
        Doc::Typed(d) => {
            let text = CString::new(
                json!({"op":"canvas_size","width":w,"height":h,"x":x,"y":y}).to_string(),
            )
            .unwrap();
            let mut err = ptr::null_mut();
            let out = unsafe {
                dr_precision_doc_edit(
                    d,
                    text.as_ptr(),
                    ptr::null(),
                    0,
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                )
            };
            assert!(!out.is_null(), "{}", take_err_string(err));
            Doc::Typed(*unsafe { Box::from_raw(out) })
        }
    }
}

// -------------------------------------------------------------- helpers --

fn inside_old(x: u32, y: u32) -> bool {
    (ORIGIN.0..ORIGIN.0 + OLD.0).contains(&x) && (ORIGIN.1..ORIGIN.1 + OLD.1).contains(&y)
}

/// Whether native cell `k`'s footprint on `crop` (every pixel it overlaps,
/// the cell oracle's span) touches the old rect.
fn footprint_meets_old(k: usize, crop: [u32; 4]) -> bool {
    let (nw, nh) = NATIVE;
    let (i, j) = (k as u32 % nw, k as u32 / nw);
    let [x, y, cw, ch] = crop.map(f64::from);
    let span = |n: u32, size: f64, total: u32| {
        let a = f64::from(n) * size / f64::from(total);
        let b = f64::from(n + 1) * size / f64::from(total);
        a.floor() as u32..b.ceil() as u32
    };
    span(j, ch, nh).any(|py| span(i, cw, nw).any(|px| inside_old(x as u32 + px, y as u32 + py)))
}

fn max(depth: Depth) -> f64 {
    depth.max()
}

/// The opaque gradient picture at `OLD`, grown to `CANVAS`.
fn padded(depth: Depth) -> Doc {
    grown(&build(
        depth,
        OLD.0,
        OLD.1,
        &[gradient(depth, OLD.0, OLD.1)],
    ))
}

/// Every pixel of the new area is `rgb` (exact at 8 bits, ±1 code at 16)
/// and opaque, and every pixel of the old rect is `before`'s.
fn assert_expanded(doc: &Doc, depth: Depth, before: &[f64], rgb: [u8; 3], what: &str) {
    let after = projection(doc);
    let tolerance = match depth {
        Depth::Byte => 0.0,
        Depth::Sixteen => 1.0,
    };
    for y in 0..CANVAS.1 {
        for x in 0..CANVAS.0 {
            let at = 4 * (y * CANVAS.0 + x) as usize;
            if inside_old(x, y) {
                assert_eq!(
                    after[at..at + 4],
                    before[at..at + 4],
                    "{what} at {depth:?}: ({x}, {y}) inside the old rect"
                );
                continue;
            }
            for c in 0..3 {
                let want = f64::from(rgb[c]) / 255.0;
                let diff = (after[at + c] - want).abs() * max(depth);
                assert!(
                    diff <= tolerance + 1e-6,
                    "{what} at {depth:?}: ({x}, {y}) channel {c} {} vs {want}",
                    after[at + c]
                );
            }
            assert_eq!(after[at + 3], 1.0, "{what} at {depth:?}: ({x}, {y}) opaque");
        }
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn the_coverage_is_one_minus_the_composite_alpha() {
    for depth in DEPTHS {
        let mut pixels = gradient(depth, OLD.0, OLD.1);
        let soft = match depth {
            Depth::Byte => 128.0 / 255.0,
            Depth::Sixteen => 30000.0 / 65535.0,
        };
        pixels[(4 * OLD.0 + 3) as usize][3] = soft;
        let doc = grown(&build(depth, OLD.0, OLD.1, &[pixels]));
        let coverage = expand(&doc).unwrap();
        let soft_coverage = match depth {
            Depth::Byte => 65535 - 128 * 257,
            Depth::Sixteen => 35535,
        };
        for y in 0..CANVAS.1 {
            for x in 0..CANVAS.0 {
                let got = coverage[(y * CANVAS.0 + x) as usize];
                let want = if (x, y) == (ORIGIN.0 + 3, ORIGIN.1 + 4) {
                    soft_coverage
                } else if inside_old(x, y) {
                    0
                } else {
                    65535
                };
                assert_eq!(got, want, "{depth:?}: ({x}, {y})");
            }
        }
    }
}

#[test]
fn the_new_area_is_filled_and_the_picture_stays_bit_exact() {
    for depth in DEPTHS {
        let doc = padded(depth);
        let before = projection(&doc);
        let coverage = expand(&doc).unwrap();
        let framed = frame(&coverage, CANVAS.0, CANVAS.1, &[NATIVE]).unwrap();
        assert_eq!((framed[4], framed[5]), NATIVE);
        // The covered rect is the whole canvas, so the crop is the canvas.
        assert_eq!(framed[..4], [0, 0, CANVAS.0, CANVAS.1]);
        let input = input_with(&doc, 0, &coverage, NATIVE, LIMIT, ptr::null()).unwrap();
        assert_eq!(input.crop[..], framed[..4]);
        let picture: Vec<[f64; 4]> = before.chunks(4).map(|p| [p[0], p[1], p[2], p[3]]).collect();
        let (_, holes) = cells_oracle(&picture, &coverage, CANVAS.0, input.crop, NATIVE);
        for (k, hole) in holes.iter().enumerate() {
            let want: [u8; 4] = if *hole {
                [255, 255, 255, 0]
            } else {
                [0, 0, 0, 255]
            };
            assert_eq!(input.mask[4 * k..4 * k + 4], want, "{depth:?}: cell {k}");
            // The image is transparent on the hole, so a provider that reads
            // its edit area from the image's alpha generates there; a hole
            // cell with no picture under its footprint has no mean either.
            let pixel = &input.image[4 * k..4 * k + 4];
            if *hole && !footprint_meets_old(k, input.crop) {
                assert_eq!(pixel, [0, 0, 0, 0], "{depth:?}: image cell {k}");
            } else {
                let alpha = if *hole { 0 } else { 255 };
                assert_eq!(pixel[3], alpha, "{depth:?}: image alpha {k}");
            }
        }
        assert!(holes.iter().any(|h| !h), "{depth:?}: some cells are kept");
        let results: Vec<Picture> = COLORS[..3].iter().map(|&c| solid(NATIVE, c)).collect();
        let (out, index) = apply(&doc, &coverage, &results, TAG).unwrap();
        assert_eq!(index, 1);
        assert_eq!(info(&out, index).unwrap().unwrap()[0], 3);
        assert_expanded(&out, depth, &before, COLORS[0], "applied");
        let picked = select(&out, index, 1).unwrap().expect("a change");
        assert_expanded(&picked, depth, &before, COLORS[1], "select 1");
    }
}

#[test]
fn every_expand_refusal_names_itself() {
    let n = (CANVAS.0 * CANVAS.1) as usize;
    for depth in DEPTHS {
        let doc = padded(depth);
        assert_eq!(
            expand_with(&doc, 5, n, LIMIT, ptr::null()).unwrap_err(),
            "generative expand layer 5 is out of range"
        );
        let buffer = "invalid generative expand coverage buffer";
        assert_eq!(
            expand_with(&doc, 0, n - 1, LIMIT, ptr::null()).unwrap_err(),
            buffer
        );
        let mut err = ptr::null_mut();
        let null_buffer = unsafe {
            match &doc {
                Doc::Byte(d) => dr_doc_gen_expand_coverage(
                    d,
                    0,
                    ptr::null_mut(),
                    n,
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                ),
                Doc::Typed(d) => dr_precision_doc_gen_expand_coverage(
                    d,
                    0,
                    ptr::null_mut(),
                    n,
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                ),
            }
        };
        assert!(!null_buffer);
        assert_eq!(take_err_string(err), buffer);
        // No canvas change: nothing is transparent.
        let opaque = build(depth, OLD.0, OLD.1, &[gradient(depth, OLD.0, OLD.1)]);
        assert_eq!(
            expand_with(&opaque, 0, (OLD.0 * OLD.1) as usize, LIMIT, ptr::null()).unwrap_err(),
            "the picture has no transparent area to expand into: enlarge the canvas first"
        );
        assert!(expand_with(&doc, 0, n, 4096, ptr::null())
            .unwrap_err()
            .contains("working limit"));
        let control = unsafe { dr_job_control_create() };
        unsafe { dr_job_control_cancel(control) };
        let cancelled = expand_with(&doc, 0, n, LIMIT, control);
        unsafe { dr_job_control_free(control) };
        assert_eq!(cancelled.unwrap_err(), "operation cancelled");
    }
    // A float document's coverage (Auto-Blend's Content-Aware Fill
    // Transparent Areas reads it): 65535 where α = 0, 0 where α = 1.
    let opaque_left = |i: usize| (i as u32 % CANVAS.0) < CANVAS.0 / 2;
    let float = Doc::Typed(float_doc(
        CANVAS.0,
        CANVAS.1,
        &IccProfile::srgb(),
        &[(0..n)
            .map(|i| {
                let alpha = if opaque_left(i) { 1.0 } else { 0.0 };
                [0.5, 0.5, 0.5, alpha]
            })
            .collect()],
    ));
    let coverage = expand(&float).unwrap();
    for (i, &c) in coverage.iter().enumerate() {
        assert_eq!(c, if opaque_left(i) { 0 } else { 65535 }, "float pixel {i}");
    }
    let mut err = ptr::null_mut();
    let mut out = vec![0u16; n];
    let null = unsafe {
        dr_doc_gen_expand_coverage(
            ptr::null::<DrDocument>(),
            0,
            out.as_mut_ptr(),
            n,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!null);
    assert_eq!(take_err_string(err), "required handle is NULL");
}
