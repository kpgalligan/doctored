//! #547 phase 2a-ii, painting a Grayscale document: a Brush stroke
//! (`paint_blend`), the Paint Bucket (`bucket_fill`), the Gradient (two
//! colours and stops), Fill's mode-less `paint` and the Eraser
//! (`erase_native`) on Gray Gamma 2.2 at 8, 16 and 32 bits. The oracles are
//! independent: a covered pixel's gray at full coverage is the colour's
//! hand-computed luminance (sRGB's published D50 Y row over the IEC decode,
//! then `Y^(1/2.2)`; linear `Y` at 32 bits) within ≤ 4/65535 at 16 bits, ≤ 1
//! code on the ×257 grid at 8 and ≤ 1e-4 at 32; and a half-covered stroke at
//! opacity 0.5 equals the same stroke on the RGB TWIN (the Gray document
//! converted to RGB through its own profile bytes, which keeps every sample)
//! painted with the greyed colour, exactly. Bounds declared before measuring.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_gray::*;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_precision_inspect::dr_precision_doc_profile;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_fixture::{srgb_decode, SRGB_Y_ROW};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 8;
const H: u32 = 2;
const RED: [f64; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f64; 4] = [0.0, 1.0, 0.0, 1.0];
const BLUE: [f64; 4] = [0.0, 0.0, 1.0, 1.0];

fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

/// A Gray Gamma 2.2 document of `depth`, one layer of mid gray.
fn new_gray(depth: u32) -> *mut PrecisionDocument {
    let request = CString::new(format!(
        r#"{{"depth":{depth},"space":"gray_gamma_22","fill_srgb":[0.5,0.5,0.5,1]}}"#
    ))
    .unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_new_gray(W, H, ptr::null(), 0, request.as_ptr(), LIMIT, 0, &mut err)
    };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

/// The RGB twin: `gray` converted to RGB through its own profile's bytes.
fn twin(gray: *const PrecisionDocument) -> *mut PrecisionDocument {
    let mut length = 0;
    let bytes = unsafe { dr_precision_doc_profile(gray, &mut length) };
    assert!(!bytes.is_null());
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_gray_to_rgb(
            gray,
            bytes,
            length,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "twin: {}", take_err_string(err));
    out
}

fn plan_samples(doc: *const PrecisionDocument, request: &Value) -> Result<usize, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    if plan.is_null() {
        return Err(take_err_string(err));
    }
    let value: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(plan) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(plan) };
    Ok(value["input_samples"].as_u64().unwrap() as usize)
}

/// `request` with every input sample at `cover`.
fn paint(
    doc: *const PrecisionDocument,
    request: &Value,
    cover: u16,
) -> Result<*mut PrecisionDocument, String> {
    let n = plan_samples(doc, request)?;
    let input = vec![cover; n];
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            input.as_ptr(),
            input.len(),
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

/// Layer 0's samples: u16 codes, or the f32 bits at 32 bits.
fn raw(doc: *const PrecisionDocument, depth: u32) -> Vec<[u32; 4]> {
    let image = unsafe { &*doc }.layer_source(0).unwrap();
    if depth == 32 {
        image
            .to_rgba32()
            .unwrap()
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect()
    } else {
        image
            .to_rgba16()
            .unwrap()
            .iter()
            .map(|p| p.map(u32::from))
            .collect()
    }
}

/// Layer 0's samples as numbers: codes at 8 and 16 bits, linear at 32.
fn values(doc: *const PrecisionDocument, depth: u32) -> Vec<[f64; 4]> {
    raw(doc, depth)
        .iter()
        .map(|p| {
            if depth == 32 {
                p.map(|b| f64::from(f32::from_bits(b)))
            } else {
                p.map(f64::from)
            }
        })
        .collect()
}

fn assert_neutral(pixels: &[[f64; 4]], what: &str) {
    for (i, p) in pixels.iter().enumerate() {
        assert!(p[0] == p[1] && p[1] == p[2], "{what} pixel {i}: {p:?}");
    }
}

/// The colour's relative luminance, written from the standards.
fn luminance(rgb: [f64; 4]) -> f64 {
    (0..3).map(|c| SRGB_Y_ROW[c] * srgb_decode(rgb[c])).sum()
}

/// A full-coverage sample against the luminance of `rgb` at `depth`.
fn assert_luminance(got: f64, rgb: [f64; 4], depth: u32, what: &str) {
    let y = luminance(rgb);
    match depth {
        32 => assert!((got - y).abs() <= 1e-4, "{what}: {got} against {y}"),
        16 => {
            let want = y.powf(1.0 / 2.2) * 65535.0;
            assert!((got - want).abs() <= 4.0, "{what}: {got} against {want}");
        }
        _ => {
            let want = y.powf(1.0 / 2.2) * 255.0;
            assert_eq!(got as u32 % 257, 0, "{what}: {got} is off the 8-bit grid");
            assert!(
                (got / 257.0 - want).abs() <= 1.0,
                "{what}: {got} against {want}"
            );
        }
    }
}

fn brush(color: [f64; 4], space: &str, opacity: f64, mode: u32) -> Value {
    json!({"op":"paint_blend","layer":0,"color":color,"color_space":space,"opacity":opacity,
        "blend_mode":mode})
}

fn bucket(color: [f64; 4], space: &str, opacity: f64, mode: u32) -> Value {
    json!({"op":"bucket_fill","layer":0,"color":color,"color_space":space,"opacity":opacity,
        "blend_mode":mode,"x":0,"y":0,"tolerance":0,"contiguous":true})
}

/// Pixel 0 lies before `from` and pixel `W - 1` past `to`, so each takes an
/// end colour whole.
fn ramp(start: [f64; 4], end: [f64; 4], space: &str, opacity: f64, mode: u32) -> Value {
    json!({"op":"gradient","layer":0,"start":start,"end":end,"color_space":space,
        "opacity":opacity,"blend_mode":mode,"from":[1.0,0.0],"to":[f64::from(W) - 1.0,0.0],
        "radial":false})
}

fn stops(colors: [[f64; 4]; 3], space: &str, opacity: f64, mode: u32) -> Value {
    let mut request = ramp(colors[0], colors[2], space, opacity, mode);
    request["style"] = json!("linear");
    request["stops"] = json!([
        {"position":0.0,"color":colors[0]},
        {"position":0.5,"color":colors[1]},
        {"position":1.0,"color":colors[2]},
    ]);
    request
}

/// The greyed ink of `rgb` on `gray`, in the twin's terms: (space, colour).
/// Read back from a full-coverage Normal stroke, which stores the ink.
fn greyed(gray: *const PrecisionDocument, rgb: [f64; 4], depth: u32) -> [f64; 4] {
    let out = paint(gray, &brush(rgb, "srgb", 1.0, 0), 65535).unwrap();
    let p = values(out, depth)[0];
    free(out);
    let g = if depth == 32 { p[0] } else { p[0] / 65535.0 };
    [g, g, g, rgb[3]]
}

#[test]
fn every_paint_tool_paints_the_colours_luminance_on_r_equal_g_equal_b() {
    for depth in [8, 16, 32] {
        let gray = new_gray(depth);
        let full = |request: &Value, what: &str| {
            let out = paint(gray, request, 65535).unwrap_or_else(|e| panic!("{what}: {e}"));
            let v = values(out, depth);
            free(out);
            assert_neutral(&v, what);
            v
        };
        for (what, request) in [
            ("brush", brush(RED, "srgb", 1.0, 0)),
            ("bucket", bucket(RED, "srgb", 1.0, 0)),
        ] {
            let what = format!("{what} at {depth}");
            for p in full(&request, &what) {
                assert_luminance(p[0], RED, depth, &what);
            }
        }
        let what = format!("gradient at {depth}");
        let v = full(&ramp(RED, BLUE, "srgb", 1.0, 0), &what);
        assert_luminance(v[0][0], RED, depth, &what);
        assert_luminance(v[W as usize - 1][0], BLUE, depth, &what);
        let what = format!("stops gradient at {depth}");
        let v = full(&stops([RED, GREEN, BLUE], "srgb", 1.0, 0), &what);
        assert_luminance(v[0][0], RED, depth, &what);
        assert_luminance(v[W as usize - 1][0], BLUE, depth, &what);
        assert!(unsafe { &*gray }.is_gray(), "the document stays Grayscale");
        free(gray);
    }
}

#[test]
fn half_covered_strokes_equal_the_rgb_twin_painted_with_the_greyed_colour() {
    for depth in [8, 16, 32] {
        let gray = new_gray(depth);
        let rgb = twin(gray);
        assert_eq!(
            raw(rgb, depth),
            raw(gray, depth),
            "the twin keeps every sample"
        );
        let [red, green, blue] = [RED, GREEN, BLUE].map(|c| greyed(gray, c, depth));
        // The twin's ink: the greyed colour as the document's numbers, or
        // linear light at 32 bits (a float document's Brush and Gradient).
        let space = if depth == 32 { "linear" } else { "document" };
        let mut cases = vec![
            (
                "brush multiply",
                brush(RED, "srgb", 0.5, 1),
                brush(red, space, 0.5, 1),
            ),
            (
                "brush normal",
                brush(BLUE, "srgb", 0.5, 0),
                brush(blue, space, 0.5, 0),
            ),
            (
                "gradient",
                ramp(RED, BLUE, "srgb", 0.5, 0),
                ramp(red, blue, space, 0.5, 0),
            ),
            (
                "stops gradient",
                stops([RED, GREEN, BLUE], "srgb", 0.5, 0),
                stops([red, green, blue], space, 0.5, 0),
            ),
            (
                "eraser",
                json!({"op":"erase_native","layer":0,"opacity":0.5}),
                json!({"op":"erase_native","layer":0,"opacity":0.5}),
            ),
        ];
        // A float document's bucket takes no linear ink; its pixels are
        // checked against the luminance above.
        if depth != 32 {
            cases.push((
                "bucket",
                bucket(GREEN, "srgb", 0.5, 0),
                bucket(green, space, 0.5, 0),
            ));
        }
        for (what, on_gray, on_twin) in cases {
            let what = format!("{what} at {depth}");
            let a = paint(gray, &on_gray, 32768).unwrap_or_else(|e| panic!("{what}: {e}"));
            let b = paint(rgb, &on_twin, 32768).unwrap_or_else(|e| panic!("{what} twin: {e}"));
            assert_neutral(&values(a, depth), &what);
            assert_eq!(raw(a, depth), raw(b, depth), "{what}");
            free(a);
            free(b);
        }
        free(rgb);
        free(gray);
    }
}

#[test]
fn fills_mode_less_paint_lands_on_the_luminance() {
    // MCP `fill` with `contiguous: false` sends op `paint`, a u16 edit.
    for depth in [8, 16] {
        let gray = new_gray(depth);
        let request =
            json!({"op":"paint","layer":0,"color":RED,"color_space":"srgb","opacity":1.0});
        let out = paint(gray, &request, 65535).unwrap();
        let v = values(out, depth);
        assert_neutral(&v, "paint");
        for p in v {
            assert_luminance(p[0], RED, depth, &format!("paint at {depth}"));
        }
        free(out);
        free(gray);
    }
}

#[test]
fn the_bucket_and_gradient_refuse_hue_and_admit_multiply() {
    let gray = new_gray(16);
    let hue = "The Hue blend mode is unavailable in a Grayscale document.";
    for request in [
        bucket(RED, "srgb", 1.0, 23),
        ramp(RED, BLUE, "srgb", 1.0, 23),
    ] {
        assert_eq!(plan_samples(gray, &request).unwrap_err(), hue);
    }
    for request in [bucket(RED, "srgb", 1.0, 1), ramp(RED, BLUE, "srgb", 1.0, 1)] {
        let out = paint(gray, &request, 65535).unwrap();
        assert_neutral(&values(out, 16), "multiply");
        free(out);
    }
    free(gray);
}
