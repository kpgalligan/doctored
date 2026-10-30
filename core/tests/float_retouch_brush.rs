//! The Blur, Sharpen and Smudge brushes and the History Brush in a 32-bit
//! float document, black-box through the typed exports: the
//! `retouch_brush` op of `dr_precision_doc_edit`, `dr_precision_doc_smudge`
//! and `dr_precision_doc_history_brush`, with their plans.
//!
//! Every expected value is computed here in binary64 from the documented
//! rule, on the samples the layer stores:
//!
//! - Blur and Sharpen against the alpha-weighted Gaussian of the straight
//!   linear colour summed out to `ceil(8σ)` (the kernel under test stops near
//!   5.33σ), with no clamp on colour.
//! - Smudge against the sub-stepped walk of `retouch_smudge`'s module doc.
//! - The History Brush against the source's own bits at full coverage and the
//!   premultiplied mix below it.
//!
//! The highlight check declares its bound before measuring: beside a 5000
//! highlight on a 0.1 surround, Blur stays within 2 % of the reference at
//! every pixel and Sharpen within 2 % of `max(|reference|, 0.1)` (its result
//! crosses zero). The test prints the worst error of each.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc_lock::{LOCK_PIXELS, LOCK_TRANSPARENCY};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_float::dr_float_operators;
use doctored_core::ffi_precision_conversion::dr_precision_capability;
use doctored_core::ffi_precision_edit::*;
use doctored_core::ffi_retouch_brush::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use serde_json::{json, Value};

use crate::float_fixture::float_doc;
use crate::native_scalar as native;
use native::*;

/// The largest stored magnitude (`float_document::FLOAT_RANGE_MAX`).
const RANGE: f64 = 65504.0;
/// `sampling::EXTENDED_ALPHA_FLOOR`, 2^-24.
const FLOOR: f64 = 1.0 / 16_777_216.0;
const HARD: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;

// ------------------------------------------------------------- fixtures --

fn layer32(d: &PrecisionDocument) -> Vec<[f32; 4]> {
    d.layer_source(0).unwrap().to_rgba32().unwrap()
}

fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

fn geometry(d: &PrecisionDocument) -> ((u32, u32), (i32, i32)) {
    let layer = d.layer(0).unwrap();
    (layer.source().unwrap().dimensions(), layer.offset())
}

/// An sRGB float document holding one canvas-sized raster of `pixels`. The
/// oracles read the samples the layer stores (`layer32`), so a fixture's
/// hidden colour under alpha 0 is whatever the document kept of it.
fn document(w: u32, h: u32, pixels: &[[f32; 4]]) -> PrecisionDocument {
    let d = float_doc(w, h, &IccProfile::srgb(), &[pixels.to_vec()]);
    assert_eq!(geometry(&d), ((w, h), (0, 0)), "the fixture is not trimmed");
    d
}

/// `d`'s canvas resized to `canvas` with the content at `at`: the raster
/// keeps its size and sits at that offset.
fn placed(d: &PrecisionDocument, canvas: (u32, u32), at: (i32, i32)) -> PrecisionDocument {
    let size = geometry(d).0;
    let edit = GeometryEdit::parse(
        &json!({"op":"canvas_size","width":canvas.0,"height":canvas.1,"x":at.0,"y":at.1})
            .to_string(),
    )
    .unwrap();
    let out = d.geometry(&edit, LIMIT, 0, None).unwrap().expect("resized");
    assert_eq!(geometry(&out), (size, at));
    out
}

/// Extended-range straight samples: (8, 4, 2) above white, negatives, alpha
/// 0, half alpha and alpha 1. The four corners are opaque.
fn extended(w: u32, h: u32) -> Vec<[f32; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let corner = (x == 0 || x == w - 1) && (y == 0 || y == h - 1);
            let a = match (x + 2 * y + 1) % 5 {
                _ if corner => 1.0,
                0 => 0.0,
                1 | 2 => 1.0,
                3 => 0.5,
                _ => 0.75,
            };
            if (x + y) % 6 == 0 {
                return [8.0, 4.0, 2.0, a];
            }
            let v = ((x * 37 + y * 101) % 1000) as f32 / 1000.0;
            let g = if (x * y) % 7 == 3 { -0.25 } else { 1.0 - v };
            [v * 6.0 - 1.0, g, ((x ^ y) % 97) as f32 / 4.0 - 2.0, a]
        })
        .collect()
}

/// The largest colour magnitude of `pixels`, at least 1: what a tolerance is
/// a fraction of.
fn magnitude(pixels: &[[f32; 4]]) -> f64 {
    pixels
        .iter()
        .flat_map(|p| p[..3].iter())
        .fold(1.0_f64, |m, v| m.max(f64::from(v.abs())))
}

/// Every sample is finite and inside the stored range.
fn assert_stored(label: &str, pixels: &[[f32; 4]]) {
    for (i, p) in pixels.iter().enumerate() {
        for v in &p[..3] {
            assert!(
                v.is_finite() && f64::from(v.abs()) <= RANGE,
                "{label} at {i}: {p:?}"
            );
        }
        assert!(
            p[3].is_finite() && (0.0..=1.0).contains(&p[3]),
            "{label} at {i}: {p:?}"
        );
    }
}

fn plan_error(d: &PrecisionDocument, v: Value) -> String {
    let t = text(v);
    let mut e = ptr::null_mut();
    let p = unsafe { dr_precision_doc_edit_plan(d, t.as_ptr(), LIMIT, 0, &mut e) };
    assert!(p.is_null(), "the plan was expected to refuse");
    unsafe { error(e) }
}

/// The edit's refusal for `v` under `control`, after a successful plan.
fn edit_error(
    d: &PrecisionDocument,
    v: Value,
    input: &[u16],
    control: Option<&DrJobControl>,
) -> String {
    plan(d, v.clone());
    let t = text(v);
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            d,
            t.as_ptr(),
            input.as_ptr(),
            input.len(),
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c as *const DrJobControl),
            &mut e,
        )
    };
    assert!(out.is_null(), "the edit was expected to refuse");
    unsafe { error(e) }
}

unsafe fn owned_json(value: *mut c_char) -> Value {
    assert!(!value.is_null());
    let parsed = serde_json::from_slice(unsafe { CStr::from_ptr(value) }.to_bytes()).unwrap();
    unsafe { dr_string_free(value) };
    parsed
}

// ------------------------------------------------- blur and sharpen oracle --

fn at(pixels: &[[f32; 4]], (w, h): (u32, u32), x: i64, y: i64) -> [f64; 4] {
    let cx = x.clamp(0, i64::from(w) - 1) as usize;
    let cy = y.clamp(0, i64::from(h) - 1) as usize;
    pixels[cy * w as usize + cx].map(f64::from)
}

/// The alpha-weighted Gaussian of the straight colour, `Σ k·a·rgb / Σ k·a`,
/// as the full two-dimensional sum out to `ceil(8σ)` with reads clamped to
/// the layer; the pixel's own colour where the weight sum is 0.
fn gaussian(pixels: &[[f32; 4]], size: (u32, u32), sigma: f64, x: u32, y: u32) -> [f64; 3] {
    let half = (8.0 * sigma).ceil() as i64;
    let k = |d: i64| (-0.5 * (d as f64 / sigma).powi(2)).exp();
    let (mut acc, mut total) = ([0.0; 3], 0.0);
    for dy in -half..=half {
        for dx in -half..=half {
            let p = at(pixels, size, i64::from(x) + dx, i64::from(y) + dy);
            let weight = k(dx) * k(dy) * p[3];
            for c in 0..3 {
                acc[c] += weight * p[c];
            }
            total += weight;
        }
    }
    if total > 0.0 {
        acc.map(|v| v / total)
    } else {
        let p = at(pixels, size, i64::from(x), i64::from(y));
        [p[0], p[1], p[2]]
    }
}

/// What a pixel becomes at full strength under full coverage: the Gaussian
/// at sigma 2 (Blur), or `rgb + (rgb − Gaussian at sigma 1)` (Sharpen), with
/// no clamp.
fn target(pixels: &[[f32; 4]], size: (u32, u32), kind: &str, x: u32, y: u32) -> [f64; 3] {
    match kind {
        "blur" => gaussian(pixels, size, 2.0, x, y),
        "sharpen" => {
            let p = at(pixels, size, i64::from(x), i64::from(y));
            let low = gaussian(pixels, size, 1.0, x, y);
            std::array::from_fn(|c| p[c] + (p[c] - low[c]))
        }
        _ => panic!("bad kind {kind}"),
    }
}

fn request(kind: &str, strength: f64, selection: bool) -> Value {
    json!({"op":"retouch_brush","layer":0,"kind":kind,"strength":strength,"selection":selection})
}

// ------------------------------------------------------ blur and sharpen --

#[test]
fn blur_and_sharpen_match_the_f64_gaussian_with_coverage_and_selection_multiplied() {
    let size = (21_u32, 16_u32);
    let n = (size.0 * size.1) as usize;
    let d = document(size.0, size.1, &extended(size.0, size.1));
    let pixels = layer32(&d);
    assert!(pixels.iter().any(|p| p[0] > 1.0), "above white");
    assert!(pixels.iter().any(|p| p[1] < 0.0), "below zero");
    assert!(pixels.iter().any(|p| p[3] > 0.0 && p[3] < 1.0), "partial");
    assert!(pixels.iter().any(|p| p[3] == 0.0), "transparent");
    let stroke: Vec<u16> = (0..n).map(|i| ((i * 7919) % 65536) as u16).collect();
    let chosen: Vec<u16> = (0..n)
        .map(|i| {
            if i % 5 == 0 {
                0
            } else {
                65535 - (i % 9) as u16 * 4001
            }
        })
        .collect();
    let input: Vec<u16> = stroke.iter().chain(&chosen).copied().collect();
    let tolerance = 1e-5 * magnitude(&pixels);
    for kind in ["blur", "sharpen"] {
        let out = layer32(&edit(&d, request(kind, 0.8, true), &input).expect(kind));
        assert_stored(kind, &out);
        let mut moved = 0;
        for i in 0..n {
            let (x, y) = (i as u32 % size.0, i as u32 / size.0);
            let coverage = f64::from(stroke[i]) / 65535.0 * f64::from(chosen[i]) / 65535.0;
            assert_eq!(
                out[i][3].to_bits(),
                pixels[i][3].to_bits(),
                "{kind}: alpha at {i}"
            );
            if coverage == 0.0 || pixels[i][3] == 0.0 {
                assert_eq!(
                    out[i].map(f32::to_bits),
                    pixels[i].map(f32::to_bits),
                    "{kind}: untouched {i}"
                );
                continue;
            }
            let wanted = target(&pixels, size, kind, x, y);
            for c in 0..3 {
                let v = f64::from(pixels[i][c]);
                let expect = v + coverage * 0.8 * (wanted[c] - v);
                assert!(
                    (f64::from(out[i][c]) - expect).abs() <= tolerance,
                    "{kind} pixel {i} channel {c}: {} vs {expect}",
                    out[i][c]
                );
                moved += usize::from(out[i][c].to_bits() != pixels[i][c].to_bits());
            }
        }
        assert!(moved > n / 2, "{kind} moved {moved} samples");
    }
}

/// A vertical edge: `left` in columns below `w / 2`, `right` from it.
fn edge(w: u32, h: u32, left: f32, right: f32) -> Vec<[f32; 4]> {
    (0..w * h)
        .map(|i| {
            let v = if i % w < w / 2 { left } else { right };
            [v, v * 0.5, v * 0.25, 1.0]
        })
        .collect()
}

#[test]
fn a_sharpened_value_below_zero_is_stored_and_an_overshoot_lands_at_the_bound() {
    let size = (24_u32, 12_u32);
    let n = (size.0 * size.1) as usize;
    let full = vec![65535_u16; n];
    // 0.1 beside 8: the dark side of the edge is pushed below zero.
    let d = document(size.0, size.1, &edge(size.0, size.1, 0.1, 8.0));
    let pixels = layer32(&d);
    let out = layer32(&edit(&d, request("sharpen", 1.0, false), &full).expect("sharpen"));
    assert_stored("sharpen", &out);
    let tolerance = 1e-5 * magnitude(&pixels);
    let mut negative = 0;
    for (i, got) in out.iter().enumerate() {
        let wanted = target(
            &pixels,
            size,
            "sharpen",
            i as u32 % size.0,
            i as u32 / size.0,
        );
        for c in 0..3 {
            assert!(
                (f64::from(got[c]) - wanted[c]).abs() <= tolerance,
                "pixel {i} channel {c}: {} vs {}",
                got[c],
                wanted[c]
            );
        }
        negative += usize::from(got[0] < 0.0);
    }
    assert!(negative >= size.1 as usize, "a column went below zero");
    // 60000 beside −60000: each side's target is past the stored bound, and
    // the edit lands there instead of refusing.
    let d = document(size.0, size.1, &edge(size.0, size.1, 60000.0, -60000.0));
    let pixels = layer32(&d);
    let out = layer32(&edit(&d, request("sharpen", 1.0, false), &full).expect("at the bound"));
    assert_stored("bound", &out);
    let row = 5 * size.0 as usize;
    let (bright, dark) = (row + 11, row + 12);
    assert!(target(&pixels, size, "sharpen", 11, 5)[0] > RANGE);
    assert!(target(&pixels, size, "sharpen", 12, 5)[0] < -RANGE);
    assert_eq!(out[bright][0], 65504.0);
    assert_eq!(out[dark][0], -65504.0);
    assert_eq!(out[bright][3].to_bits(), 1.0_f32.to_bits());
    // Far from the edge the layer is flat and keeps its bits.
    assert_eq!(out[row].map(f32::to_bits), pixels[row].map(f32::to_bits));
}

/// A 0.1 surround with 5000 where `lit` says so.
fn highlight(w: u32, h: u32, lit: impl Fn(i64, i64) -> bool) -> Vec<[f32; 4]> {
    (0..w * h)
        .map(|i| {
            let v = if lit(i64::from(i % w), i64::from(i / w)) {
                5000.0
            } else {
                0.1
            };
            [v, v, v, 1.0]
        })
        .collect()
}

#[test]
fn blur_and_sharpen_stay_within_two_percent_beside_a_5000_highlight() {
    // The bound, declared before measuring: 2 % of the reference at every
    // pixel for Blur, 2 % of max(|reference|, 0.1) for Sharpen.
    const BOUND: f64 = 0.02;
    let size = (64_u32, 64_u32);
    let n = (size.0 * size.1) as usize;
    let full = vec![65535_u16; n];
    let scenes = [
        (
            "one 5000 pixel",
            highlight(64, 64, |x, y| (x, y) == (32, 32)),
        ),
        (
            "a 5000 disc of radius 6",
            highlight(64, 64, |x, y| (x - 32).pow(2) + (y - 32).pow(2) <= 36),
        ),
    ];
    for (name, scene) in scenes {
        let d = document(size.0, size.1, &scene);
        let pixels = layer32(&d);
        for kind in ["blur", "sharpen"] {
            let out = layer32(&edit(&d, request(kind, 1.0, false), &full).expect(kind));
            assert_stored(kind, &out);
            let mut worst = 0.0_f64;
            for (i, got) in out.iter().enumerate() {
                let wanted = target(&pixels, size, kind, i as u32 % size.0, i as u32 / size.0);
                let floor = if kind == "sharpen" { 0.1 } else { 0.0 };
                let error = (f64::from(got[0]) - wanted[0]).abs() / wanted[0].abs().max(floor);
                worst = worst.max(error);
            }
            println!(
                "float {kind} brush, {name}: worst error {:.3e} of the reference (bound {BOUND})",
                worst
            );
            assert!(worst <= BOUND, "{kind}, {name}: {worst}");
            if kind == "blur" {
                // The highlight spreads: a pixel beside it is lifted above 1.
                assert!(out[32 * 64 + 34][0] > 1.0);
            } else {
                assert!(out.iter().any(|p| p[0] < 0.0), "{name}: below zero");
            }
        }
    }
}

#[test]
fn the_retouch_brush_op_refuses_by_name_and_answers_null_for_nothing_to_do() {
    let d = document(8, 6, &extended(8, 6));
    let full = vec![65535_u16; 48];
    // The Sponge is admitted (R5; `float_paint_tone.rs` tests its pixels).
    for kind in ["saturate", "desaturate"] {
        plan(&d, request(kind, 1.0, false));
    }
    assert_eq!(
        plan_error(&d, request("blur", 1.5, false)),
        "retouch brush strength is invalid"
    );
    assert_eq!(
        plan_error(
            &d,
            json!({"op":"retouch_brush","layer":4,"kind":"blur","strength":1.0})
        ),
        "native paint target is out of range"
    );
    assert_eq!(
        edit_error(&d, request("blur", 1.0, false), &full[..47], None),
        "high-depth input sample length does not match the core plan"
    );
    let p = plan(&d, request("sharpen", 1.0, true));
    assert_eq!(p["input_samples"], 96);
    assert_eq!(p["input_width"], 8);
    assert_eq!(p["input_height"], 6);
    assert_eq!(p["input_planes"], 2);
    assert!(p["scratch_bytes"].as_u64().unwrap() > 0);
    // Nothing to do.
    assert!(edit(&d, request("blur", 0.0, false), &full).is_none());
    assert!(edit(&d, request("blur", 1.0, false), &[0; 48]).is_none());
    let flat = document(8, 6, &[[5000.0, -3.0, 0.25, 1.0]; 48]);
    for kind in ["blur", "sharpen"] {
        assert!(
            edit(&flat, request(kind, 1.0, false), &full).is_none(),
            "{kind}"
        );
    }
    // A pixel lock refuses by name; a transparency lock does not stop a
    // brush that never writes alpha.
    let locked = d.with_layer_locks(0, LOCK_PIXELS).unwrap().unwrap();
    assert!(edit_error(&locked, request("blur", 1.0, false), &full, None).contains("lock"));
    let frozen = d.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    assert!(edit(&frozen, request("blur", 1.0, false), &full).is_some());
    // A cancelled control stops the edit; the same request with none runs.
    let control = DrJobControl::default();
    control.cancel();
    assert_eq!(
        edit_error(&d, request("sharpen", 1.0, false), &full, Some(&control)),
        "operation cancelled"
    );
    assert!(edit(&d, request("sharpen", 1.0, false), &full).is_some());
}

#[test]
fn the_retouch_brush_op_tolerates_null() {
    let d = document(8, 6, &extended(8, 6));
    let full = [65535_u16; 48];
    let blur = text(request("blur", 1.0, false));
    let (doc, req, input): (*const PrecisionDocument, _, _) = (&d, blur.as_ptr(), full.as_ptr());
    type Out = *mut *mut c_char;
    let planned = |d: *const PrecisionDocument, t: *const c_char, e: Out| {
        unsafe { dr_precision_doc_edit_plan(d, t, LIMIT, 0, e) }.is_null()
    };
    let edited = |d: *const PrecisionDocument, t: *const c_char, i: *const u16, e: Out| {
        unsafe { dr_precision_doc_edit(d, t, i, 48, LIMIT, 0, ptr::null(), e) }.is_null()
    };
    let length = "high-depth input sample length does not match the core plan";
    let mut e: *mut c_char = ptr::null_mut();
    assert!(planned(ptr::null(), req, &mut e));
    assert_eq!(unsafe { error(e) }, "required handle is NULL");
    e = ptr::null_mut();
    assert!(planned(doc, ptr::null(), &mut e));
    assert_eq!(unsafe { error(e) }, "request is NULL");
    e = ptr::null_mut();
    assert!(edited(ptr::null(), req, input, &mut e));
    assert_eq!(unsafe { error(e) }, "required handle is NULL");
    e = ptr::null_mut();
    assert!(edited(doc, ptr::null(), input, &mut e));
    assert_eq!(unsafe { error(e) }, "request is NULL");
    // A NULL input with a length.
    e = ptr::null_mut();
    assert!(edited(doc, req, ptr::null(), &mut e));
    assert_eq!(unsafe { error(e) }, length);
    // A NULL err_out is tolerated on both error paths.
    assert!(planned(ptr::null(), req, ptr::null_mut()));
    assert!(edited(doc, req, ptr::null(), ptr::null_mut()));
}

// ------------------------------------------------------------------ smudge --

fn smudge_plan(d: &PrecisionDocument, dabs: &[[f64; 4]], selection: bool) -> Result<Value, String> {
    let brush = CString::new(HARD).unwrap();
    let flat: Vec<f64> = dabs.iter().flatten().copied().collect();
    let mut e = ptr::null_mut();
    let p = unsafe {
        dr_precision_doc_smudge_plan(
            d,
            0,
            brush.as_ptr(),
            flat.as_ptr(),
            dabs.len(),
            selection,
            false,
            LIMIT,
            0,
            &mut e,
        )
    };
    if p.is_null() {
        return Err(unsafe { error(e) });
    }
    Ok(unsafe { owned_json(p) })
}

fn smudge_under(
    d: &PrecisionDocument,
    dabs: &[[f64; 4]],
    strength: f64,
    selection: &[u16],
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let brush = CString::new(HARD).unwrap();
    let flat: Vec<f64> = dabs.iter().flatten().copied().collect();
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_smudge(
            d,
            0,
            brush.as_ptr(),
            if flat.is_empty() {
                ptr::null()
            } else {
                flat.as_ptr()
            },
            dabs.len(),
            strength,
            if selection.is_empty() {
                ptr::null()
            } else {
                selection.as_ptr()
            },
            selection.len(),
            false,
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c as *const DrJobControl),
            &mut e,
        )
    };
    if !e.is_null() {
        assert!(out.is_null());
        return Err(unsafe { error(e) });
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

fn smudge(
    d: &PrecisionDocument,
    dabs: &[[f64; 4]],
    strength: f64,
    selection: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
    smudge_under(d, dabs, strength, selection, None)
}

/// A hard round dab's coverage of a pixel: the 8×8 sum at every pixel.
fn hard_coverage(dab: [f64; 4], x: usize, y: usize) -> f64 {
    let radius = dab[2] / 2.0;
    let mut inside = 0.0;
    for sy in 0..8 {
        for sx in 0..8 {
            let px = x as f64 + (f64::from(sx) + 0.5) / 8.0;
            let py = y as f64 + (f64::from(sy) + 0.5) / 8.0;
            if (px - dab[0]).hypot(py - dab[1]) / radius <= 1.0 {
                inside += 1.0;
            }
        }
    }
    inside / 64.0
}

/// The documented smudge walk over an OPAQUE layer in f64 (so the field is
/// the colour itself): footprint field, sub-steps of at most a pixel, staged
/// bilinear carry. Returns linear colours.
fn smudge_oracle(
    pixels: &[[f32; 4]],
    (w, h): (usize, usize),
    dabs: &[[f64; 4]],
    strength: f64,
    weight: impl Fn(usize, usize) -> f64,
) -> Vec<[f64; 3]> {
    let clip = |d: &[f64; 4]| {
        let r = d[2] / 2.0;
        [
            (d[0] - r).floor().clamp(0.0, w as f64) as usize,
            (d[1] - r).floor().clamp(0.0, h as f64) as usize,
            (d[0] + r).ceil().clamp(0.0, w as f64) as usize,
            (d[1] + r).ceil().clamp(0.0, h as f64) as usize,
        ]
    };
    let mut f = [usize::MAX, usize::MAX, 0, 0];
    for d in dabs {
        let b = clip(d);
        f = [
            f[0].min(b[0]),
            f[1].min(b[1]),
            f[2].max(b[2]),
            f[3].max(b[3]),
        ];
    }
    let mut field: Vec<[f64; 3]> = pixels
        .iter()
        .map(|p| [0, 1, 2].map(|c| f64::from(p[c])))
        .collect();
    for pair in dabs.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let m = ((to[0] - from[0]).hypot(to[1] - from[1]).ceil() as usize).max(1);
        let step = ((to[0] - from[0]) / m as f64, (to[1] - from[1]) / m as f64);
        for j in 1..=m {
            let dab: [f64; 4] =
                std::array::from_fn(|c| from[c] + (to[c] - from[c]) * j as f64 / m as f64);
            let b = clip(&dab);
            let found = field.clone();
            let read = |x: f64, y: f64| -> [f64; 3] {
                let (x0, y0) = (x.floor(), y.floor());
                let (fx, fy) = (x - x0, y - y0);
                let at = |cx: f64, cy: f64| {
                    let cx = (cx as i64).clamp(f[0] as i64, f[2] as i64 - 1) as usize;
                    let cy = (cy as i64).clamp(f[1] as i64, f[3] as i64 - 1) as usize;
                    found[cy * w + cx]
                };
                let (p00, p01, p10, p11) = (
                    at(x0, y0),
                    at(x0 + 1.0, y0),
                    at(x0, y0 + 1.0),
                    at(x0 + 1.0, y0 + 1.0),
                );
                std::array::from_fn(|c| {
                    (p00[c] * (1.0 - fx) + p01[c] * fx) * (1.0 - fy)
                        + (p10[c] * (1.0 - fx) + p11[c] * fx) * fy
                })
            };
            for y in b[1].max(f[1])..b[3] {
                for x in b[0].max(f[0])..b[2] {
                    let a = (strength * hard_coverage(dab, x, y) * dab[3] * weight(x, y)).min(1.0);
                    if a > 0.0 {
                        let carried = read(x as f64 - step.0, y as f64 - step.1);
                        let old = found[y * w + x];
                        field[y * w + x] =
                            std::array::from_fn(|c| old[c] + a * (carried[c] - old[c]));
                    }
                }
            }
        }
    }
    field
}

/// An opaque layer: a region near 8 left of column `split`, a dark one from
/// it, both with structure.
fn bright_and_dark(w: u32, h: u32, split: u32) -> Vec<[f32; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if x < split {
                [
                    8.0 + 0.01 * (y % 7) as f32,
                    4.0,
                    2.0 - 0.02 * (x % 5) as f32,
                    1.0,
                ]
            } else {
                [
                    0.05 + 0.001 * (x % 9) as f32,
                    0.02,
                    0.01 * (y % 4) as f32,
                    1.0,
                ]
            }
        })
        .collect()
}

#[test]
fn smudge_carries_values_above_white_like_the_sub_stepped_oracle() {
    let (w, h) = (56_u32, 44_u32);
    let n = (w * h) as usize;
    let d = document(w, h, &bright_and_dark(w, h, 28));
    let pixels = layer32(&d);
    let dabs = [
        [17.3, 19.7, 15.0, 1.0],
        [26.9, 23.2, 19.0, 0.8],
        [30.0, 14.5, 11.0, 1.0],
    ];
    let p = smudge_plan(&d, &dabs, false).unwrap();
    assert_eq!(p["input_samples"], 0);
    assert!(p["scratch_bytes"].as_u64().unwrap() > 0);
    assert_eq!(smudge_plan(&d, &dabs, true).unwrap()["input_samples"], n);
    let tolerance = 1e-4 * magnitude(&pixels);
    let check = |got: &[[f32; 4]], expect: &[[f64; 3]], what: &str| {
        for i in 0..n {
            assert_eq!(got[i][3].to_bits(), 1.0_f32.to_bits(), "{what}: alpha {i}");
            for c in 0..3 {
                assert!(
                    (f64::from(got[i][c]) - expect[i][c]).abs() <= tolerance,
                    "{what}: pixel {i} channel {c}: {} vs {}",
                    got[i][c],
                    expect[i][c]
                );
            }
        }
    };
    let got = layer32(&smudge(&d, &dabs, 0.7, &[]).unwrap().expect("a change"));
    assert_stored("smudge", &got);
    check(
        &got,
        &smudge_oracle(&pixels, (w as usize, h as usize), &dabs, 0.7, |_, _| 1.0),
        "whole",
    );
    // The bright region was carried into the dark one: a pixel right of the
    // split, dark before, now reads above white.
    let carried = (0..n)
        .filter(|i| (*i as u32 % w) >= 30 && pixels[*i][0] < 0.1 && got[*i][0] > 1.0)
        .count();
    assert!(carried > 20, "{carried} dark pixels now read above white");
    // Half coverage on the left of the canvas, none on the right.
    let chosen: Vec<u16> = (0..n)
        .map(|i| if i % 56 < 24 { 32768 } else { 0 })
        .collect();
    let got = layer32(&smudge(&d, &dabs, 0.7, &chosen).unwrap().expect("a change"));
    check(
        &got,
        &smudge_oracle(&pixels, (w as usize, h as usize), &dabs, 0.7, |x, _| {
            if x < 24 {
                32768.0 / 65535.0
            } else {
                0.0
            }
        }),
        "selected",
    );
    for i in 0..n {
        if i % 56 >= 24 {
            assert_eq!(
                got[i].map(f32::to_bits),
                pixels[i].map(f32::to_bits),
                "outside the selection {i}"
            );
        }
    }
}

#[test]
fn a_hard_integer_smudge_at_full_strength_moves_the_start_centre_to_the_end_centre() {
    // (8.25, 3.5, 1.125) left of column 20, dark from it. The tip starts
    // wholly inside the bright region and moves 20 px right in 20 sub-steps
    // of one pixel, each an exact read.
    let (w, h) = (48_u32, 24_u32);
    let scene: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            if i % w < 20 {
                [8.25, 3.5, 1.125, 1.0]
            } else {
                [0.05, 0.02, 0.01, 1.0]
            }
        })
        .collect();
    let d = document(w, h, &scene);
    let pixels = layer32(&d);
    let dabs = [[10.5, 12.5, 12.0, 1.0], [30.5, 12.5, 12.0, 1.0]];
    let got = layer32(&smudge(&d, &dabs, 1.0, &[]).unwrap().expect("a change"));
    assert_stored("hard move", &got);
    let (start, end) = (12 * w as usize + 10, 12 * w as usize + 30);
    assert_eq!(pixels[end][0], 0.05);
    assert_eq!(
        got[end].map(f32::to_bits),
        pixels[start].map(f32::to_bits),
        "the end centre holds the start centre's bits"
    );
    assert!(got[end][0] > 1.0);
}

#[test]
fn smudge_does_not_depend_on_the_dab_spacing_and_honours_the_layer_offset() {
    let d = placed(
        &document(40, 30, &bright_and_dark(40, 30, 22)),
        (50, 40),
        (6, 4),
    );
    let pixels = layer32(&d);
    let far = [[22.5, 18.5, 14.0, 1.0], [34.5, 18.5, 14.0, 1.0]];
    let near: Vec<[f64; 4]> = (0..13)
        .map(|k| [22.5 + f64::from(k), 18.5, 14.0, 1.0])
        .collect();
    let a = smudge(&d, &far, 0.5, &[]).unwrap().expect("a change");
    let b = smudge(&d, &near, 0.5, &[]).unwrap().expect("a change");
    assert_eq!(bits(&layer32(&a)), bits(&layer32(&b)));
    assert_eq!(geometry(&a), ((40, 30), (6, 4)));
    // The dabs are canvas coordinates: the oracle runs in the layer's.
    let local: Vec<[f64; 4]> = far
        .iter()
        .map(|d| [d[0] - 6.0, d[1] - 4.0, d[2], d[3]])
        .collect();
    let expect = smudge_oracle(&pixels, (40, 30), &local, 0.5, |_, _| 1.0);
    let tolerance = 1e-4 * magnitude(&pixels);
    for (i, got) in layer32(&a).iter().enumerate() {
        for c in 0..3 {
            assert!(
                (f64::from(got[c]) - expect[i][c]).abs() <= tolerance,
                "pixel {i} channel {c}"
            );
        }
    }
}

#[test]
fn smudge_keeps_alpha_under_a_transparency_lock_and_refuses_a_pixel_lock() {
    let d = document(48, 36, &extended(48, 36));
    let pixels = layer32(&d);
    let dabs = [[16.5, 18.5, 16.0, 1.0], [30.5, 18.5, 16.0, 1.0]];
    // Unlocked, alpha moves with the colour.
    let free = layer32(&smudge(&d, &dabs, 0.8, &[]).unwrap().expect("a change"));
    assert_stored("free", &free);
    assert!(free
        .iter()
        .zip(&pixels)
        .any(|(a, b)| a[3].to_bits() != b[3].to_bits()));
    let frozen = d.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    let got = layer32(&smudge(&frozen, &dabs, 0.8, &[]).unwrap().expect("a change"));
    assert_stored("frozen", &got);
    assert_ne!(bits(&got), bits(&pixels));
    for (after, before) in got.iter().zip(&pixels) {
        assert_eq!(after[3].to_bits(), before[3].to_bits(), "alpha kept");
        if before[3] == 0.0 {
            assert_eq!(after.map(f32::to_bits), before.map(f32::to_bits));
        }
    }
    let locked = d.with_layer_locks(0, LOCK_PIXELS).unwrap().unwrap();
    assert!(smudge(&locked, &dabs, 0.8, &[])
        .err()
        .unwrap()
        .contains("lock"));
}

/// `w × h` samples: alpha 0 left of column `split` and 2^-23 from it, with
/// opaque corners so the fixture is not trimmed.
fn faint_from(w: u32, h: u32, split: u32) -> Vec<[f32; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if (x == 0 || x == w - 1) && (y == 0 || y == h - 1) {
                [1.0, 1.0, 1.0, 1.0]
            } else if x < split {
                [5.0, 5.0, 5.0, 0.0]
            } else {
                [3.0, -0.5, 2.0, (2.0 * FLOOR) as f32]
            }
        })
        .collect()
}

#[test]
fn a_smudge_that_leaves_alpha_under_the_floor_writes_transparent_black() {
    // Alpha 0 is smeared into alpha 2^-23 at strength 0.8, so the first
    // column it reaches holds 0.2 · 2^-23, strictly between 0 and 2^-24.
    let (w, h) = (40_u32, 24_u32);
    let d = document(w, h, &faint_from(w, h, 20));
    let pixels = layer32(&d);
    let faint = (2.0 * FLOOR) as f32;
    assert_eq!(pixels[12 * 40 + 25][3].to_bits(), faint.to_bits());
    let dabs = [[12.5, 12.5, 10.0, 1.0], [28.5, 12.5, 10.0, 1.0]];
    let got = layer32(&smudge(&d, &dabs, 0.8, &[]).unwrap().expect("a change"));
    assert_stored("faint", &got);
    // The walk is linear in each premultiplied sample, so the oracle run on
    // alpha alone is the smeared alpha.
    let alphas: Vec<[f32; 4]> = pixels.iter().map(|p| [p[3], 0.0, 0.0, 1.0]).collect();
    let expect = smudge_oracle(&alphas, (w as usize, h as usize), &dabs, 0.8, |_, _| 1.0);
    let mut cleared = 0;
    for (i, after) in got.iter().enumerate() {
        let (before, alpha) = (pixels[i], expect[i][0]);
        assert!(
            after[3] == 0.0 || f64::from(after[3]) >= FLOOR,
            "pixel {i} stores an alpha under the floor: {after:?}"
        );
        if alpha < 0.9 * FLOOR {
            if before[3] == 0.0 {
                assert_eq!(bits(&[*after]), bits(&[before]), "nothing stays at {i}");
            } else {
                assert_eq!(
                    bits(&[*after]),
                    bits(&[[0.0; 4]]),
                    "transparent black at {i}"
                );
                if alpha > 0.0 {
                    cleared += 1;
                }
            }
        } else if alpha > 1.1 * FLOOR {
            assert!(
                (f64::from(after[3]) - alpha).abs() <= 1e-3 * alpha,
                "alpha at {i}: {} vs {alpha}",
                after[3]
            );
        }
    }
    assert!(
        cleared > 0,
        "{cleared} pixels fell strictly under the floor"
    );
}

#[test]
fn smudge_refuses_by_name_answers_null_for_nothing_to_do_and_tolerates_null() {
    let d = document(40, 30, &extended(40, 30));
    let dabs = [[10.5, 10.5, 12.0, 1.0], [20.5, 14.5, 12.0, 1.0]];
    assert!(
        smudge(&d, &dabs[..1], 0.5, &[]).unwrap().is_none(),
        "one dab"
    );
    assert!(smudge(&d, &[], 0.5, &[]).unwrap().is_none(), "no dabs");
    assert!(smudge(&d, &dabs, 0.0, &[]).unwrap().is_none(), "strength 0");
    let flat = document(40, 30, &[[7000.0, -2.0, 0.5, 1.0]; 1200]);
    assert!(smudge(&flat, &dabs, 0.5, &[]).unwrap().is_none(), "flat");
    assert_eq!(
        smudge(&d, &dabs, 1.5, &[]).err().unwrap(),
        "smudge strength is invalid"
    );
    assert_eq!(
        smudge(&d, &dabs, 0.5, &[65535; 1199]).err().unwrap(),
        "smudge selection length differs from the canvas"
    );
    assert_eq!(
        smudge(&d, &[[1.0, 1.0, -4.0, 1.0], dabs[1]], 0.5, &[])
            .err()
            .unwrap(),
        "native brush dab coordinates, diameter or opacity are invalid"
    );
    // A cancelled control stops the walk; the same stroke with none runs.
    let control = DrJobControl::default();
    control.cancel();
    assert_eq!(
        smudge_under(&d, &dabs, 0.5, &[], Some(&control))
            .err()
            .unwrap(),
        "operation cancelled"
    );
    assert!(smudge(&d, &dabs, 0.5, &[]).unwrap().is_some());
    // Smudge's own 256 million bound, as at 16 bits: a hard size-512 tip
    // swept 600 px is past it and refuses in the plan; swept 300 px it plans.
    let wide = document(1200, 600, &vec![[1.5, 2.0, 3.0, 1.0]; 720_000]);
    let sweep = |to: f64| [[300.5, 300.5, 512.0, 1.0], [to, 300.5, 512.0, 1.0]];
    let long = sweep(900.5);
    let sentence =
        "smudge stroke exceeds 256 million work units; use a shorter stroke or a smaller brush";
    assert_eq!(smudge_plan(&wide, &long, false).err().unwrap(), sentence);
    assert_eq!(smudge(&wide, &long, 0.5, &[]).err().unwrap(), sentence);
    assert!(smudge_plan(&wide, &sweep(600.5), false).is_ok());
    let brush = CString::new(HARD).unwrap();
    let flat_dabs: Vec<f64> = dabs.iter().flatten().copied().collect();
    let mut e: *mut c_char = ptr::null_mut();
    unsafe {
        assert!(dr_precision_doc_smudge_plan(
            ptr::null(),
            0,
            brush.as_ptr(),
            flat_dabs.as_ptr(),
            2,
            false,
            false,
            LIMIT,
            0,
            &mut e
        )
        .is_null());
        assert_eq!(error(e), "required handle is NULL");
        e = ptr::null_mut();
        assert!(dr_precision_doc_smudge_plan(
            &d,
            0,
            ptr::null(),
            flat_dabs.as_ptr(),
            2,
            false,
            false,
            LIMIT,
            0,
            &mut e
        )
        .is_null());
        assert_eq!(error(e), "native brush request is NULL");
        e = ptr::null_mut();
        assert!(dr_precision_doc_smudge(
            ptr::null(),
            0,
            brush.as_ptr(),
            flat_dabs.as_ptr(),
            2,
            0.5,
            ptr::null(),
            0,
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut e
        )
        .is_null());
        assert_eq!(error(e), "required handle is NULL");
        e = ptr::null_mut();
        assert!(dr_precision_doc_smudge(
            &d,
            0,
            brush.as_ptr(),
            ptr::null(),
            2,
            0.5,
            ptr::null(),
            0,
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut e
        )
        .is_null());
        assert_eq!(
            error(e),
            "native brush requires a valid array of at most 50000 dabs; use a shorter stroke or wider spacing"
        );
        // A NULL selection with a length, and a NULL err_out.
        assert!(dr_precision_doc_smudge(
            &d,
            0,
            brush.as_ptr(),
            flat_dabs.as_ptr(),
            2,
            0.5,
            ptr::null(),
            1200,
            false,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
    }
}

// ---------------------------------------------------------- history brush --

fn history_plan(
    d: &PrecisionDocument,
    source: &PrecisionDocument,
    selection: bool,
) -> Result<Value, String> {
    let mut e = ptr::null_mut();
    let p = unsafe {
        dr_precision_doc_history_brush_plan(d, 0, source, 0, selection, LIMIT, 0, &mut e)
    };
    if p.is_null() {
        return Err(unsafe { error(e) });
    }
    Ok(unsafe { owned_json(p) })
}

fn history_under(
    d: &PrecisionDocument,
    source: &PrecisionDocument,
    opacity: f64,
    input: &[u16],
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_history_brush(
            d,
            0,
            source,
            0,
            opacity,
            input.as_ptr(),
            input.len(),
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c as *const DrJobControl),
            &mut e,
        )
    };
    if !e.is_null() {
        assert!(out.is_null());
        return Err(unsafe { error(e) });
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

fn history(
    d: &PrecisionDocument,
    source: &PrecisionDocument,
    opacity: f64,
    input: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
    history_under(d, source, opacity, input, None)
}

/// The edited state: above white, a negative channel, alpha 0 and partial
/// alpha, with opaque-enough corners.
fn edited(n: u32) -> Vec<[f32; 4]> {
    (0..n)
        .map(|i| {
            let alpha = if i % 6 == 3 {
                0.0
            } else {
                0.3 + (i % 5) as f32 * 0.15
            };
            [
                3.0 + (i % 7) as f32 * 0.5,
                -0.5,
                12.0 - (i % 11) as f32,
                alpha,
            ]
        })
        .collect()
}

#[test]
fn the_history_brush_restores_the_source_bits_and_mixes_premultiplied() {
    let size = (17_u32, 12_u32);
    let n = (size.0 * size.1) as usize;
    let earlier = document(size.0, size.1, &extended(size.0, size.1));
    let old = layer32(&earlier);
    let now = document(size.0, size.1, &edited(n as u32));
    let new = layer32(&now);
    assert_eq!(
        history_plan(&now, &earlier, false).unwrap()["input_samples"],
        n
    );
    assert_eq!(
        history_plan(&now, &earlier, true).unwrap()["input_samples"],
        2 * n
    );
    // Opacity 1 under full coverage: the earlier samples, bit for bit —
    // above white, negative, partly transparent and transparent alike.
    let inside = |i: usize| (3..14).contains(&(i % 17)) && (2..9).contains(&(i / 17));
    let input: Vec<u16> = (0..n).map(|i| if inside(i) { 65535 } else { 0 }).collect();
    let got = layer32(
        &history(&now, &earlier, 1.0, &input)
            .unwrap()
            .expect("a change"),
    );
    assert_stored("restored", &got);
    for i in 0..n {
        let wanted = if inside(i) { old[i] } else { new[i] };
        assert_eq!(
            got[i].map(f32::to_bits),
            wanted.map(f32::to_bits),
            "pixel {i}"
        );
    }
    assert!((0..n).any(|i| inside(i) && old[i][0] > 1.0));
    assert!((0..n).any(|i| inside(i) && old[i][1] < 0.0));
    assert!((0..n).any(|i| inside(i) && old[i][3] > 0.0 && old[i][3] < 1.0));
    // Partial coverage times a selection plane times opacity.
    let stroke: Vec<u16> = (0..n).map(|i| ((i * 5231) % 65536) as u16).collect();
    let chosen: Vec<u16> = (0..n).map(|i| if i % 4 == 0 { 0 } else { 50000 }).collect();
    let input: Vec<u16> = stroke.iter().chain(&chosen).copied().collect();
    let got = layer32(
        &history(&now, &earlier, 0.6, &input)
            .unwrap()
            .expect("a change"),
    );
    assert_stored("mixed", &got);
    let mut mixed = 0;
    for i in 0..n {
        let t = f64::from(stroke[i]) / 65535.0 * f64::from(chosen[i]) / 65535.0 * 0.6;
        if t == 0.0 {
            assert_eq!(
                got[i].map(f32::to_bits),
                new[i].map(f32::to_bits),
                "uncovered {i}"
            );
            continue;
        }
        let (a, b) = (new[i].map(f64::from), old[i].map(f64::from));
        let alpha = (1.0 - t) * a[3] + t * b[3];
        if alpha < FLOOR {
            assert!(
                bits(&[got[i]]) == bits(&[new[i]]) || got[i] == [0.0; 4],
                "pixel {i}"
            );
            continue;
        }
        assert!((f64::from(got[i][3]) - alpha).abs() <= 1e-6, "alpha {i}");
        for c in 0..3 {
            let expect = ((1.0 - t) * a[c] * a[3] + t * b[c] * b[3]) / alpha;
            assert!(
                (f64::from(got[i][c]) - expect).abs() <= 1e-6 * expect.abs().max(1.0),
                "pixel {i} channel {c}: {} vs {expect}",
                got[i][c]
            );
        }
        mixed += 1;
    }
    assert!(mixed > n / 2, "{mixed} pixels mixed");
    // The same state painted onto itself changes nothing.
    assert!(history(&earlier, &earlier, 1.0, &vec![65535; n])
        .unwrap()
        .is_none());
    assert!(history(&now, &earlier, 0.0, &vec![65535; n])
        .unwrap()
        .is_none());
}

#[test]
fn a_history_mix_that_leaves_alpha_under_the_floor_writes_transparent_black() {
    // Stored alpha 2^-23 mixed with a transparent source at t = 0.75 is
    // 2^-25: strictly between 0 and the 2^-24 floor.
    let (w, h) = (12_u32, 8_u32);
    let n = (w * h) as usize;
    let now = document(w, h, &faint_from(w, h, 0));
    let earlier = document(w, h, &faint_from(w, h, w));
    let (new, old) = (layer32(&now), layer32(&earlier));
    let faint = (2.0 * FLOOR) as f32;
    let got = layer32(
        &history(&now, &earlier, 0.75, &vec![65535; n])
            .unwrap()
            .expect("a change"),
    );
    assert_stored("faint history", &got);
    let mut cleared = 0;
    for i in 0..n {
        if new[i][3].to_bits() == faint.to_bits() && old[i][3] == 0.0 {
            let mixed = 0.25 * f64::from(new[i][3]);
            assert!(mixed > 0.0 && mixed < FLOOR);
            assert_eq!(bits(&[got[i]]), bits(&[[0.0; 4]]), "pixel {i}");
            cleared += 1;
        }
    }
    assert_eq!(cleared, n - 4, "every pixel but the opaque corners");
}

#[test]
fn the_history_brush_reads_the_source_at_the_same_canvas_position_and_keeps_locked_alpha() {
    // Now: 10×8 at (2, 1). Earlier: 6×4 at (5, 3). Canvas 14×10.
    let earlier = placed(&document(6, 4, &extended(6, 4)), (14, 10), (5, 3));
    let old = layer32(&earlier);
    let now = placed(
        &document(10, 8, &[[9.0, 0.125, -0.5, 0.625]; 80]),
        (14, 10),
        (2, 1),
    );
    let new = layer32(&now);
    let full = vec![65535_u16; 140];
    let out = history(&now, &earlier, 1.0, &full)
        .unwrap()
        .expect("a change");
    assert_eq!(geometry(&out), ((10, 8), (2, 1)), "no growth");
    let got = layer32(&out);
    assert_stored("offset", &got);
    let source_at = |i: usize| {
        // Canvas (lx + 2, ly + 1) is source pixel (lx − 3, ly − 2).
        let (sx, sy) = (i as i64 % 10 - 3, i as i64 / 10 - 2);
        ((0..6).contains(&sx) && (0..4).contains(&sy)).then(|| old[(sy * 6 + sx) as usize])
    };
    for (i, after) in got.iter().enumerate() {
        // The source's own bits, or transparent black where its layer holds
        // nothing.
        let wanted = source_at(i).unwrap_or([0.0; 4]);
        assert_eq!(
            after.map(f32::to_bits),
            wanted.map(f32::to_bits),
            "pixel {i}"
        );
    }
    let frozen = now.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    let got = layer32(
        &history(&frozen, &earlier, 1.0, &full)
            .unwrap()
            .expect("a change"),
    );
    for (i, after) in got.iter().enumerate() {
        assert_eq!(after[3].to_bits(), 0.625_f32.to_bits(), "alpha kept at {i}");
        match source_at(i) {
            Some(p) if p[3] > 0.0 => assert_eq!(
                bits(&[[after[0], after[1], after[2], 0.0]]),
                bits(&[[p[0], p[1], p[2], 0.0]]),
                "colour {i}"
            ),
            _ => assert_eq!(
                after.map(f32::to_bits),
                new[i].map(f32::to_bits),
                "no source colour at {i}"
            ),
        }
    }
    // Under the lock a mix keeps alpha too.
    let got = layer32(
        &history(&frozen, &earlier, 0.5, &full)
            .unwrap()
            .expect("a change"),
    );
    for (i, after) in got.iter().enumerate() {
        assert_eq!(after[3].to_bits(), 0.625_f32.to_bits(), "alpha kept at {i}");
        if let Some(p) = source_at(i).filter(|p| p[3] > 0.0) {
            let (a, b) = (new[i].map(f64::from), p.map(f64::from));
            let alpha = 0.5 * a[3] + 0.5 * b[3];
            for c in 0..3 {
                let expect = (0.5 * a[c] * a[3] + 0.5 * b[c] * b[3]) / alpha;
                assert!(
                    (f64::from(after[c]) - expect).abs() <= 1e-6 * expect.abs().max(1.0),
                    "pixel {i} channel {c}"
                );
            }
        }
    }
}

#[test]
fn the_history_brush_refuses_by_name_and_tolerates_null() {
    let now = document(8, 6, &[[9.0, 0.5, -0.25, 1.0]; 48]);
    let earlier = document(8, 6, &extended(8, 6));
    let full = vec![65535_u16; 48];
    assert_eq!(
        history_plan(&now, &document(9, 6, &extended(9, 6)), false)
            .err()
            .unwrap(),
        "history brush source has a different canvas size"
    );
    // A float target paints from a float state, and a 16-bit one from a
    // 16-bit state.
    let sixteen = doc(8, 6, vec![[9000, 100, 100, 65535]; 48]);
    assert_eq!(
        history_plan(&now, &sixteen, false).err().unwrap(),
        "history brush source has a different bit depth"
    );
    assert_eq!(
        history(&now, &sixteen, 1.0, &full).err().unwrap(),
        "history brush source has a different bit depth"
    );
    assert_eq!(
        history_plan(&sixteen, &earlier, false).err().unwrap(),
        "native paint requires u16 storage"
    );
    assert_eq!(
        history(&now, &earlier, 1.5, &full).err().unwrap(),
        "history brush opacity is invalid"
    );
    assert_eq!(
        history(&now, &earlier, 1.0, &full[..40]).err().unwrap(),
        "high-depth input sample length does not match the core plan"
    );
    let locked = now.with_layer_locks(0, LOCK_PIXELS).unwrap().unwrap();
    assert!(history(&locked, &earlier, 1.0, &full)
        .err()
        .unwrap()
        .contains("lock"));
    let control = DrJobControl::default();
    control.cancel();
    assert_eq!(
        history_under(&now, &earlier, 1.0, &full, Some(&control))
            .err()
            .unwrap(),
        "operation cancelled"
    );
    assert!(history(&now, &earlier, 1.0, &full).unwrap().is_some());
    let mut e: *mut c_char = ptr::null_mut();
    unsafe {
        assert!(dr_precision_doc_history_brush_plan(
            ptr::null(),
            0,
            &earlier,
            0,
            false,
            LIMIT,
            0,
            &mut e
        )
        .is_null());
        assert_eq!(error(e), "required handle is NULL");
        e = ptr::null_mut();
        assert!(dr_precision_doc_history_brush_plan(
            &now,
            0,
            ptr::null(),
            0,
            false,
            LIMIT,
            0,
            &mut e
        )
        .is_null());
        assert_eq!(error(e), "required handle is NULL");
        e = ptr::null_mut();
        assert!(dr_precision_doc_history_brush(
            ptr::null(),
            0,
            &earlier,
            0,
            1.0,
            full.as_ptr(),
            48,
            LIMIT,
            0,
            ptr::null(),
            &mut e
        )
        .is_null());
        assert_eq!(error(e), "required handle is NULL");
        e = ptr::null_mut();
        assert!(dr_precision_doc_history_brush(
            &now,
            0,
            ptr::null(),
            0,
            1.0,
            full.as_ptr(),
            48,
            LIMIT,
            0,
            ptr::null(),
            &mut e
        )
        .is_null());
        assert_eq!(error(e), "required handle is NULL");
        e = ptr::null_mut();
        assert!(dr_precision_doc_history_brush(
            &now,
            0,
            &earlier,
            0,
            1.0,
            ptr::null(),
            48,
            LIMIT,
            0,
            ptr::null(),
            &mut e
        )
        .is_null());
        assert_eq!(
            error(e),
            "high-depth input sample length does not match the core plan"
        );
        // A NULL err_out is tolerated on the error path.
        assert!(dr_precision_doc_history_brush(
            &now,
            0,
            ptr::null(),
            0,
            1.0,
            full.as_ptr(),
            48,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
    }
}

// ------------------------------------------------- capability and table --

#[test]
fn capability_rows_and_the_operator_table_admit_the_four_brushes() {
    for (target, depth, supported) in [("raster", 32, true), ("mask", 32, false)] {
        let name = CString::new("retouch_brush").unwrap();
        let plane = CString::new(target).unwrap();
        let mut err = ptr::null_mut();
        let answer = unsafe {
            owned_json(dr_precision_capability(
                name.as_ptr(),
                plane.as_ptr(),
                depth,
                &mut err,
            ))
        };
        assert!(err.is_null());
        assert_eq!(
            answer["supported"], supported,
            "retouch_brush on {target} at {depth}"
        );
    }
    let table = unsafe { owned_json(dr_float_operators()) };
    let admitted = table["admitted"].as_array().unwrap();
    for row in [
        "blur_brush",
        "sharpen_brush",
        "smudge",
        "history_brush",
        "sponge",
    ] {
        assert!(admitted.iter().any(|v| v == row), "{row} is admitted");
    }
    let refused = table["refused"].as_array().unwrap();
    // The Sponge runs at float (R5), so no row refuses it.
    assert!(!refused.iter().any(|row| row["operator"] == "the Sponge"));
    assert!(!refused.iter().any(|row| {
        let operator = row["operator"].as_str().unwrap();
        operator.contains("brushes") || operator.contains("Smudge") || operator.contains("History")
    }));
}
