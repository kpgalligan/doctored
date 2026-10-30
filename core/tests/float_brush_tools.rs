//! The Brush, Eraser and Gradient in a 32-bit float document,
//! black-box through the request API (`dr_precision_doc_edit` and its plan).
//!
//! - Brush: `paint_blend` over dab coverage rasterized by `dr_brush_u16`,
//!   against an f64 W3C oracle over extended-range backdrops, with document
//!   ink and with `linear` ink above white.
//! - Eraser: `erase_native` lowers alpha by `a·(1 − coverage·opacity)` and
//!   keeps every RGB sample bit for bit.
//! - Gradient: `gradient` interpolates in LINEAR light. The oracle writes
//!   out `gradient_field`'s field formulas and the sRGB profile's curve in
//!   f64, interpolates the decoded stops (the Classic ramp for
//!   Linear, Radial, Angle and Reflected, `common::classic_ramp`;
//!   linear for Diamond and for alpha) and takes the same W3C step.
//! - The refusals, cancellation, the capability rows and the operator table.
//! - A mask and an alpha channel are u16 coverage at every depth: the scalar
//!   `paint` and `gradient` on a float document equal the same request on a
//!   16-bit twin sample for sample, and leave the float pixels alone.
//!
//! Float documents are authored as float TIFFs and opened with
//! `open_preserving` (`tests/float_fill_stroke.rs`), so a raster holds
//! exactly the samples the oracle reads.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc_lock::LOCK_TRANSPARENCY;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_brush::{dr_brush_u16, dr_brush_u16_plan};
use doctored_core::ffi_float::dr_float_operators;
use doctored_core::ffi_precision_conversion::dr_precision_capability;
use doctored_core::ffi_precision_edit::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::Metadata;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::tiff_float::FloatTiff;
use serde_json::{json, Value};

use crate::common::classic_ramp;
use crate::common::icc_chain::Chain;
use crate::native_scalar as native;
use native::*;

const W: u32 = 13;
const H: u32 = 9;
/// Normal, Multiply, Darken, Lighten, Difference.
const MODES: [u32; 5] = [0, 1, 6, 7, 8];
const UNAVAILABLE: &str = "is unavailable in a 32-bit float document";
const LINEAR_ONLY: &str = "linear ink is a 32-bit float document's paint and gradient colour";
const LINEAR_RANGE: &str =
    "linear ink must be finite with red, green and blue in [0, 65504] and alpha in [0,1]";
/// The gradient's drag: no pixel centre of the canvas lies on its ray, so
/// the Angle shape's seam falls between pixels.
const FROM: [f64; 2] = [2.3, 3.1];
const TO: [f64; 2] = [9.7, 6.4];
const POSITIONS: [f64; 3] = [0.0, 0.35, 1.0];

// ------------------------------------------------------------- fixtures --

/// Extended-range straight samples: zeros, negatives, above-white RGB,
/// alpha 0 with hidden RGB, partial alpha and alpha 1.
fn extended(width: u32, height: u32) -> Vec<[f32; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let v = ((x * 37 + y * 101) % 1000) as f32 / 1000.0;
            let r = if (x + y) % 9 == 0 { 0.0 } else { v * 6.0 - 1.0 };
            let g = if (x * y) % 7 == 3 { -0.25 } else { 1.0 - v };
            let b = ((x ^ y) % 97) as f32 / 4.0 - 2.0;
            let a = match (x + 2 * y) % 5 {
                0 => 0.0,
                1 | 2 => 1.0,
                k => k as f32 / 5.0,
            };
            [r, g, b, a]
        })
        .collect()
}

/// An sRGB float document holding one canvas-sized raster of `samples`.
fn float_document(width: u32, height: u32, samples: &[[f32; 4]]) -> PrecisionDocument {
    let tiff = FloatTiff {
        width,
        height,
        channels: 4,
        alpha: Some(false),
        samples: samples.iter().flatten().copied().collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("brush.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    assert_eq!(bits(&layer32(&doc, 0)), bits(samples));
    doc
}

fn layer32(d: &PrecisionDocument, i: usize) -> Vec<[f32; 4]> {
    d.layer_source(i).unwrap().to_rgba32().unwrap()
}

fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

fn geometry(d: &PrecisionDocument, i: usize) -> ((u32, u32), (i32, i32)) {
    let layer = d.layer(i).unwrap();
    (layer.source().unwrap().dimensions(), layer.offset())
}

/// Fractional coverage holding 0, 65535 and intermediate weights.
fn fractional(n: u32, salt: u32) -> Vec<u16> {
    (0..n)
        .map(|i| match (i + salt) % 5 {
            0 => 0,
            1 => 65535,
            k => (k * 9001 + i * 7) as u16,
        })
        .collect()
}

/// A stroke's coverage, rasterized by the core's own brush export.
fn dabs(width: u32, height: u32, hardness: f64, dabs: &[f64]) -> Vec<u16> {
    let request = text(
        json!({"version":1,"hardness":hardness,"roundness":1.0,"angle_degrees":0.0,"pixel":false}),
    );
    let mut e = ptr::null_mut();
    let count = dabs.len() / 4;
    let p = unsafe {
        dr_brush_u16_plan(
            width,
            height,
            request.as_ptr(),
            dabs.as_ptr(),
            count,
            LIMIT,
            0,
            &mut e,
        )
    };
    assert!(!p.is_null(), "{}", unsafe { error(e) });
    unsafe { dr_string_free(p) };
    let mut out = vec![777u16; (width * height) as usize];
    let ok = unsafe {
        dr_brush_u16(
            width,
            height,
            request.as_ptr(),
            dabs.as_ptr(),
            count,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(ok, "{}", unsafe { error(e) });
    out
}

/// Two overlapping dabs across the canvas: a stroke with a core and a rim.
fn stroke(hardness: f64) -> Vec<u16> {
    let out = dabs(W, H, hardness, &[4.5, 4.5, 6.0, 1.0, 8.5, 4.5, 6.0, 0.8]);
    assert!(out.contains(&0), "the stroke leaves pixels uncovered");
    assert!(out.contains(&65535), "the stroke has a full core");
    out
}

fn paint_blend(color: [f64; 4], space: &str, opacity: f64, mode: u32, selection: bool) -> Value {
    json!({"op":"paint_blend","layer":0,"color":color,"color_space":space,
        "opacity":opacity,"blend_mode":mode,"selection":selection})
}

fn erase(opacity: f64, selection: bool) -> Value {
    json!({"op":"erase_native","layer":0,"opacity":opacity,"selection":selection})
}

/// The plan's refusal for `v`, which must refuse.
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

/// `d`'s canvas grown by `(8, 6)` with the content at `(4, 3)`: the raster
/// keeps its size, so it is trimmed against the canvas.
fn trimmed(d: &PrecisionDocument) -> PrecisionDocument {
    let edit = GeometryEdit::parse(
        &json!({"op":"canvas_size","width":W + 8,"height":H + 6,"x":4,"y":3}).to_string(),
    )
    .unwrap();
    let out = d.geometry(&edit, LIMIT, 0, None).unwrap().expect("resized");
    assert_eq!(geometry(&out, 0), ((W, H), (4, 3)));
    out
}

// --------------------------------------------------------------- oracle --

/// An authored colour as linear light in the (sRGB) document's primaries:
/// `document` and `srgb` ink decode through the sRGB profile's own curve
/// (`icc_chain::Chain`: its `curv` table, linear between the u16 samples, so
/// up to half a 16-bit step off the closed-form EOTF), `linear` ink is used
/// as it is. Alpha passes through.
fn decoded(color: [f64; 4], space: &str) -> [f64; 4] {
    if space == "linear" {
        return color;
    }
    let srgb = Chain::of(&IccProfile::srgb());
    [
        srgb.decode(color[0]),
        srgb.decode(color[1]),
        srgb.decode(color[2]),
        color[3],
    ]
}

/// The W3C separable blend `B(cb, cs)` for the five modes.
fn blend(mode: u32, cb: f64, cs: f64) -> f64 {
    match mode {
        0 => cs,
        1 => cb * cs,
        6 => cb.min(cs),
        7 => cb.max(cs),
        8 => (cb - cs).abs(),
        _ => unreachable!("not a tested mode"),
    }
}

/// One W3C source-over step of straight `cs` at effective alpha `sa` onto
/// the straight backdrop `bg`, in binary64.
fn composite(mode: u32, bg: [f32; 4], cs: [f64; 4], sa: f64) -> [f64; 4] {
    let ab = f64::from(bg[3]);
    let ao = sa + ab * (1.0 - sa);
    let mut out = [0.0; 4];
    for c in 0..3 {
        let cb = f64::from(bg[c]);
        out[c] =
            (sa * (1.0 - ab) * cs[c] + sa * ab * blend(mode, cb, cs[c]) + (1.0 - sa) * ab * cb)
                / ao;
    }
    out[3] = ao;
    out
}

fn close(actual: f32, expected: f64) -> bool {
    (f64::from(actual) - expected).abs() <= 1e-5 * expected.abs().max(1.0)
}

/// The weight of pixel `i` under `input`'s one or two planes of `n` samples.
fn weight(input: &[u16], n: usize, i: usize) -> f64 {
    let mut weight = f64::from(input[i]) / 65535.0;
    if input.len() > n {
        weight *= f64::from(input[n + i]) / 65535.0;
    }
    weight
}

/// Every pixel of `after` against the oracle: `ink(i)` (straight linear
/// RGBA) composited at `ink alpha × opacity × weight` where that is above 0,
/// and every other pixel bit for bit.
fn check(
    label: &str,
    before: &[[f32; 4]],
    after: &[[f32; 4]],
    mode: u32,
    opacity: f64,
    weight: impl Fn(usize) -> f64,
    ink: impl Fn(usize) -> [f64; 4],
) {
    assert_eq!(before.len(), after.len(), "{label}");
    let mut painted = 0;
    for (i, (bg, got)) in before.iter().zip(after).enumerate() {
        let cs = ink(i);
        let sa = cs[3] * opacity * weight(i);
        if sa <= 0.0 {
            assert_eq!(
                got.map(f32::to_bits),
                bg.map(f32::to_bits),
                "{label} at {i}: an uncovered pixel is bitwise kept"
            );
            continue;
        }
        painted += 1;
        let expected = composite(mode, *bg, cs, sa);
        for c in 0..4 {
            assert!(
                close(got[c], expected[c]),
                "{label} at {i} channel {c}: {got:?} vs {expected:?} over {bg:?}"
            );
        }
    }
    assert!(painted > 0, "{label}: the request paints something");
}

/// The ramp parameter at pixel `(x, y)`'s centre, `gradient_field`'s doc
/// written out: with `L` the drag length, `u` the projection on the drag
/// over `L` and `v` the projection on its perpendicular over `L`.
fn parameter(shape: &str, x: u32, y: u32) -> f64 {
    let point = [f64::from(x) + 0.5, f64::from(y) + 0.5];
    let drag = [TO[0] - FROM[0], TO[1] - FROM[1]];
    let squared = drag[0] * drag[0] + drag[1] * drag[1];
    let offset = [point[0] - FROM[0], point[1] - FROM[1]];
    let u = (offset[0] * drag[0] + offset[1] * drag[1]) / squared;
    let v = (offset[1] * drag[0] - offset[0] * drag[1]) / squared;
    match shape {
        "linear" => u.clamp(0.0, 1.0),
        "radial" => ((offset[0] * offset[0] + offset[1] * offset[1]) / squared)
            .sqrt()
            .clamp(0.0, 1.0),
        "angle" => {
            let turn = v.atan2(u) / std::f64::consts::TAU;
            turn - turn.floor()
        }
        "reflected" => u.abs().clamp(0.0, 1.0),
        "diamond" => (u.abs() + v.abs()).clamp(0.0, 1.0),
        _ => unreachable!("not a gradient shape"),
    }
}

/// The straight RGBA of linear `stops` at `t` under `shape`: the colour
/// the Classic ramp gives for Linear, Radial, Angle and Reflected, else
/// [`linear_ramp`]'s; alpha linear.
fn ramp(stops: &[(f64, [f64; 4])], t: f64, shape: &str) -> [f64; 4] {
    let mut out = linear_ramp(stops, t);
    if matches!(shape, "linear" | "radial" | "angle" | "reflected") {
        for (c, v) in out.iter_mut().take(3).enumerate() {
            let channel: Vec<(f64, f64)> = stops.iter().map(|s| (s.0, s.1[c])).collect();
            *v = classic_ramp(&channel, t);
        }
    }
    out
}

/// The straight RGBA of linear `stops` at `t`: linear between the two
/// stops around it, an end stop's colour at or past an end.
fn linear_ramp(stops: &[(f64, [f64; 4])], t: f64) -> [f64; 4] {
    let (first, last) = (stops[0], stops[stops.len() - 1]);
    if t <= first.0 {
        return first.1;
    }
    if t >= last.0 {
        return last.1;
    }
    for pair in stops.windows(2) {
        let ((a, from), (b, to)) = (pair[0], pair[1]);
        if t >= a && t < b {
            let f = (t - a) / (b - a);
            return std::array::from_fn(|c| from[c] + (to[c] - from[c]) * f);
        }
    }
    last.1
}

/// The five gradient shapes under test: two colours in Linear and Radial,
/// three stops in Angle, Reflected and Diamond.
const SHAPES: [(&str, bool); 5] = [
    ("linear", false),
    ("radial", false),
    ("angle", true),
    ("reflected", true),
    ("diamond", true),
];

/// The authored colours of a gradient in `space`: start, middle, end. The
/// linear end is above white.
fn authored(space: &str) -> [[f64; 4]; 3] {
    if space == "linear" {
        [
            [0.0, 0.25, 0.5, 1.0],
            [2.0, 0.125, 0.0, 0.625],
            [6.0, 3.0, 1.5, 0.875],
        ]
    } else {
        [
            [0.9, 0.2, 0.1, 1.0],
            [0.2, 0.8, 0.4, 0.625],
            [0.1, 0.3, 0.95, 0.875],
        ]
    }
}

/// The `gradient` request of `shape` and the oracle's linear stops for it.
fn gradient(
    shape: (&str, bool),
    space: &str,
    opacity: f64,
    mode: u32,
    selection: bool,
) -> (Value, Vec<(f64, [f64; 4])>) {
    let colors = authored(space);
    let mut request = json!({"op":"gradient","layer":0,"start":colors[0],"end":colors[2],
        "color_space":space,"opacity":opacity,"blend_mode":mode,"from":FROM,"to":TO,
        "radial":shape.0 == "radial","selection":selection});
    let stops = if shape.1 {
        request["style"] = json!(shape.0);
        request["stops"] = Value::Array(
            POSITIONS
                .iter()
                .zip(colors)
                .map(|(position, color)| json!({"position":position,"color":color}))
                .collect(),
        );
        POSITIONS
            .iter()
            .zip(colors)
            .map(|(position, color)| (*position, decoded(color, space)))
            .collect()
    } else {
        vec![
            (0.0, decoded(colors[0], space)),
            (1.0, decoded(colors[2], space)),
        ]
    };
    (request, stops)
}

// ---------------------------------------------------------------- brush --

#[test]
fn a_brush_stroke_matches_the_w3c_oracle_over_extended_range_backdrops() {
    let samples = extended(W, H);
    let d = float_document(W, H, &samples);
    let n = (W * H) as usize;
    let inks = [
        ([0.9, 0.2, 0.1, 0.8], "document"),
        ([4.0, 0.5, 0.25, 0.75], "linear"),
    ];
    for hardness in [0.0, 1.0] {
        let coverage = stroke(hardness);
        for (color, space) in inks {
            let cs = decoded(color, space);
            for mode in MODES {
                for selected in [false, true] {
                    let mut input = coverage.clone();
                    if selected {
                        input.extend(fractional(W * H, 2));
                    }
                    let out = edit(&d, paint_blend(color, space, 0.6, mode, selected), &input)
                        .expect("the stroke changes pixels");
                    assert_eq!(geometry(&out, 0), ((W, H), (0, 0)));
                    check(
                        &format!("hardness {hardness} {space} mode {mode} selection {selected}"),
                        &samples,
                        &layer32(&out, 0),
                        mode,
                        0.6,
                        |i| weight(&input, n, i),
                        |_| cs,
                    );
                }
            }
        }
    }
}

#[test]
fn ink_above_white_paints_as_its_own_value_and_nothing_clamps() {
    let d = float_document(2, 1, &[[8.0, 8.0, 8.0, 1.0], [0.0, 0.0, 0.0, 0.0]]);
    // Half-alpha ink 1.0 over an 8.0 backdrop: 0.5·1 + 0.5·8.
    let out = edit(
        &d,
        paint_blend([1.0, 1.0, 1.0, 1.0], "linear", 0.5, 0, false),
        &[65535, 0],
    )
    .expect("the stroke changes the pixel");
    let pixels = layer32(&out, 0);
    assert_eq!(pixels[0], [4.5, 4.5, 4.5, 1.0]);
    assert_eq!(pixels[1].map(f32::to_bits), [0.0f32; 4].map(f32::to_bits));
    // Ink above white lands as its own numbers, on the backdrop and on
    // transparency alike.
    let out = edit(
        &d,
        paint_blend([4.0, 0.5, 0.25, 1.0], "linear", 1.0, 0, false),
        &[65535, 65535],
    )
    .expect("the stroke changes both pixels");
    assert_eq!(layer32(&out, 0), vec![[4.0, 0.5, 0.25, 1.0]; 2]);
}

// --------------------------------------------------------------- eraser --

#[test]
fn the_eraser_lowers_alpha_and_keeps_every_colour_sample() {
    let samples = extended(W, H);
    let d = float_document(W, H, &samples);
    let n = (W * H) as usize;
    for selected in [false, true] {
        let mut input = stroke(0.0);
        if selected {
            input.extend(fractional(W * H, 2));
        }
        let out = edit(&d, erase(0.7, selected), &input).expect("the eraser lowers alpha");
        assert_eq!(geometry(&out, 0), ((W, H), (0, 0)));
        let after = layer32(&out, 0);
        let mut lowered = 0;
        for (i, (was, got)) in samples.iter().zip(&after).enumerate() {
            for c in 0..3 {
                assert_eq!(
                    got[c].to_bits(),
                    was[c].to_bits(),
                    "selection {selected} at {i}: colour is bitwise kept"
                );
            }
            let c = weight(&input, n, i);
            if c <= 0.0 || was[3] == 0.0 {
                assert_eq!(got[3].to_bits(), was[3].to_bits(), "alpha at {i}");
                continue;
            }
            let expected = f64::from(was[3]) * (1.0 - c * 0.7);
            assert!(
                close(got[3], expected),
                "selection {selected} at {i}: alpha {} vs {expected}",
                got[3]
            );
            lowered += usize::from(got[3] < was[3]);
        }
        assert!(lowered > 0, "the eraser lowered some alpha");
    }
    // Full strength over full coverage leaves alpha 0 and the colour hidden
    // under it.
    let whole = vec![65535u16; n];
    let out = edit(&d, erase(1.0, false), &whole).expect("everything is erased");
    for (was, got) in samples.iter().zip(layer32(&out, 0)) {
        assert_eq!(got[3], 0.0);
        assert_eq!(bits(&[*was])[0][..3], bits(&[got])[0][..3]);
    }
}

#[test]
fn the_eraser_never_grows_a_layer_and_a_stroke_that_erases_nothing_is_none() {
    let samples = extended(W, H);
    let d = trimmed(&float_document(W, H, &samples));
    let (cw, ch) = (W + 8, H + 6);
    let whole = vec![65535u16; (cw * ch) as usize];
    let out = edit(&d, erase(0.7, false), &whole).expect("the layer's alpha is lowered");
    assert_eq!(geometry(&out, 0), ((W, H), (4, 3)), "the extent is kept");
    for (was, got) in samples.iter().zip(layer32(&out, 0)) {
        assert_eq!(bits(&[*was])[0][..3], bits(&[got])[0][..3]);
        assert!(close(got[3], f64::from(was[3]) * (1.0 - 0.7)));
    }
    assert!(
        edit(&d, erase(0.0, false), &whole).is_none(),
        "opacity 0 erases nothing"
    );
    let beside: Vec<u16> = (0..cw * ch)
        .map(|i| if i % cw < 4 { 65535 } else { 0 })
        .collect();
    assert!(
        edit(&d, erase(1.0, false), &beside).is_none(),
        "coverage off the layer erases nothing"
    );
    // Under a transparency lock the Eraser has nothing it may change: NULL
    // with no error (`edit` asserts there is none).
    let locked = d.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    assert!(
        edit(&locked, erase(1.0, false), &whole).is_none(),
        "a transparency-locked layer erases nothing"
    );
}

// ------------------------------------------------------------- gradient --

#[test]
fn a_gradient_interpolates_in_linear_light_in_every_shape() {
    let samples = extended(W, H);
    let d = float_document(W, H, &samples);
    let n = (W * H) as usize;
    for shape in SHAPES {
        for space in ["document", "srgb", "linear"] {
            for mode in [0, 1] {
                for selected in [false, true] {
                    let mut input = vec![65535u16; n];
                    if selected {
                        input.extend(fractional(W * H, 2));
                    }
                    let (request, stops) = gradient(shape, space, 0.8, mode, selected);
                    let out = edit(&d, request, &input).expect("the gradient changes pixels");
                    assert_eq!(geometry(&out, 0), ((W, H), (0, 0)));
                    check(
                        &format!("{} {space} mode {mode} selection {selected}", shape.0),
                        &samples,
                        &layer32(&out, 0),
                        mode,
                        0.8,
                        |i| weight(&input, n, i),
                        |i| {
                            ramp(
                                &stops,
                                parameter(shape.0, i as u32 % W, i as u32 / W),
                                shape.0,
                            )
                        },
                    );
                }
            }
        }
    }
}

#[test]
fn a_gradient_reads_t_between_black_and_an_end_above_white() {
    // Black → (8, 8, 8) along one row: each pixel reads 8 S(t), unclamped,
    // and authored black → white reads S(t) itself (the ramp is in linear
    // light), S the two-stop Classic ramp.
    let d = float_document(8, 1, &[[0.25, 0.25, 0.25, 1.0]; 8]);
    let request = |end: [f64; 4], space: &str| {
        json!({"op":"gradient","layer":0,"start":[0.0,0.0,0.0,1.0],"end":end,
            "color_space":space,"opacity":1.0,"blend_mode":0,"from":[0.5,0.5],"to":[7.5,0.5],
            "radial":false})
    };
    let hdr = edit(&d, request([8.0, 8.0, 8.0, 1.0], "linear"), &[65535; 8]).expect("a ramp");
    let sdr = edit(&d, request([1.0, 1.0, 1.0, 1.0], "document"), &[65535; 8]).expect("a ramp");
    for (x, (high, plain)) in layer32(&hdr, 0).iter().zip(layer32(&sdr, 0)).enumerate() {
        let s = classic_ramp(&[(0.0, 0.0), (1.0, 1.0)], x as f64 / 7.0);
        for c in 0..3 {
            assert!(close(high[c], 8.0 * s), "x {x}: {high:?}");
            assert!(close(plain[c], s), "x {x}: {plain:?}");
        }
        assert_eq!((high[3], plain[3]), (1.0, 1.0));
    }
}

#[test]
fn a_gradient_grows_a_trimmed_layer_and_a_transparency_lock_keeps_every_alpha() {
    let samples = extended(W, H);
    let d = trimmed(&float_document(W, H, &samples));
    let (cw, ch) = (W + 8, H + 6);
    let whole = vec![65535u16; (cw * ch) as usize];
    let (mut request, stops) = gradient(SHAPES[0], "linear", 0.8, 0, false);
    request["end"] = json!([6.0, 3.0, 1.5, 1.0]);
    let stops = vec![stops[0], (1.0, [6.0, 3.0, 1.5, 1.0])];
    let grown = edit(&d, request.clone(), &whole).expect("the gradient grows the layer");
    assert_eq!(geometry(&grown, 0), ((cw, ch), (0, 0)));
    let backdrop: Vec<[f32; 4]> = (0..cw * ch)
        .map(|i| {
            let (x, y) = (i % cw, i / cw);
            if (4..4 + W).contains(&x) && (3..3 + H).contains(&y) {
                samples[((y - 3) * W + x - 4) as usize]
            } else {
                [0.0; 4]
            }
        })
        .collect();
    check(
        "grown",
        &backdrop,
        &layer32(&grown, 0),
        0,
        0.8,
        |_| 1.0,
        |i| {
            ramp(
                &stops,
                parameter("linear", i as u32 % cw, i as u32 / cw),
                "linear",
            )
        },
    );
    let locked = d.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    let out = edit(&locked, request, &whole).expect("the gradient recolours the layer");
    assert_eq!(
        geometry(&out, 0),
        ((W, H), (4, 3)),
        "a locked layer keeps its extent"
    );
    let after = layer32(&out, 0);
    assert_ne!(bits(&after), bits(&samples));
    for (was, got) in samples.iter().zip(&after) {
        assert_eq!(got[3].to_bits(), was[3].to_bits(), "alpha is bitwise kept");
        if was[3] == 0.0 {
            assert_eq!(got.map(f32::to_bits), was.map(f32::to_bits));
        }
    }
}

// ------------------------------------------------------------- refusals --

#[test]
fn the_bucket_and_dodge_burn_plan_and_refused_inks_name_their_rule() {
    let d = float_document(W, H, &extended(W, H));
    // The bucket and Dodge/Burn plan at float (`float_paint_tone.rs`
    // tests their pixels).
    for request in [
        json!({"op":"bucket_fill","layer":0,"color":[0.5,0.5,0.5,1.0],"color_space":"document",
            "opacity":1.0,"x":1,"y":1,"tolerance":0,"contiguous":true}),
        json!({"op":"dodge_burn","layer":0,"exposure":0.5,"range":1,"burn":false}),
    ] {
        plan(&d, request);
    }
    // A mode whose formula inverts about white refuses by name.
    let (screen, _) = gradient(SHAPES[0], "document", 1.0, 2, false);
    let message = plan_error(&d, screen);
    assert!(message.contains(UNAVAILABLE), "{message}");
    assert!(message.to_lowercase().contains("screen"), "{message}");
    // Linear ink is a float document's.
    let typed = doc(W, H, vec![[1000, 2000, 3000, 65535]; (W * H) as usize]);
    let (linear, _) = gradient(SHAPES[0], "linear", 1.0, 0, false);
    plan(&d, linear.clone());
    assert_eq!(plan_error(&typed, linear), LINEAR_ONLY);
    // A linear stop past the stored range.
    let (mut high, _) = gradient(SHAPES[3], "linear", 1.0, 0, false);
    high["stops"][1]["color"] = json!([70000.0, 0.0, 0.0, 1.0]);
    assert_eq!(plan_error(&d, high), LINEAR_RANGE);
    let (mut end, _) = gradient(SHAPES[0], "linear", 1.0, 0, false);
    end["end"] = json!([70000.0, 0.0, 0.0, 1.0]);
    assert_eq!(plan_error(&d, end), LINEAR_RANGE);
}

#[test]
fn a_result_past_the_extended_range_refuses() {
    let mut samples = vec![[0.5f32, 0.5, 0.5, 1.0]; 4];
    samples[0] = [-65504.0, 0.0, 0.0, 1.0];
    let d = float_document(2, 2, &samples);
    // |−65504 − 1| is past the range under Difference, from a brush stroke
    // and from a gradient alike.
    let brush = paint_blend([1.0, 1.0, 1.0, 1.0], "linear", 1.0, 8, false);
    let ramp = json!({"op":"gradient","layer":0,"start":[1.0,1.0,1.0,1.0],"end":[1.0,1.0,1.0,1.0],
        "color_space":"linear","opacity":1.0,"blend_mode":8,"from":[0.0,0.0],"to":[2.0,2.0],
        "radial":false});
    for request in [brush, ramp] {
        let message = edit_error(&d, request.clone(), &[65535; 4], None);
        assert!(message.contains("65504"), "{request}: {message}");
        // The same request without the extreme sample is admitted.
        let tame = float_document(2, 2, &[[0.25, 2.0, -3.0, 1.0]; 4]);
        assert!(edit(&tame, request, &[65535; 4]).is_some());
    }
}

#[test]
fn a_cancelled_control_stops_each_kind_before_it_writes() {
    let d = float_document(W, H, &extended(W, H));
    let whole = vec![65535u16; (W * H) as usize];
    let (ramp, _) = gradient(SHAPES[0], "document", 1.0, 0, false);
    for request in [
        paint_blend([0.9, 0.2, 0.1, 1.0], "document", 1.0, 0, false),
        erase(1.0, false),
        ramp,
    ] {
        let control = DrJobControl::default();
        control.cancel();
        assert_eq!(
            edit_error(&d, request.clone(), &whole, Some(&control)),
            "operation cancelled",
            "{request}"
        );
        // The same request with no control is admitted.
        assert!(edit(&d, request, &whole).is_some());
    }
}

unsafe fn owned_json(value: *mut c_char) -> Value {
    assert!(!value.is_null());
    let parsed = serde_json::from_slice(unsafe { CStr::from_ptr(value) }.to_bytes()).unwrap();
    unsafe { dr_string_free(value) };
    parsed
}

#[test]
fn capability_rows_and_the_operator_table_admit_the_three_tools() {
    for (operation, target, depth, supported) in [
        ("paint", "raster", 32, true),
        ("erase", "raster", 32, true),
        ("paint", "mask", 32, false),
        ("erase", "mask", 32, false),
        ("paint", "raster", 16, true),
        ("erase", "raster", 16, true),
    ] {
        let name = CString::new(operation).unwrap();
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
            "{operation} on {target} at {depth}"
        );
    }
    let table = unsafe { owned_json(dr_float_operators()) };
    let admitted = table["admitted"].as_array().unwrap();
    for row in ["paint", "erase", "gradient", "paint_bucket", "dodge_burn"] {
        assert!(admitted.iter().any(|v| v == row), "{row} is admitted");
    }
    let refused = table["refused"].as_array().unwrap();
    assert!(!refused
        .iter()
        .any(|row| row["operator"].as_str().unwrap().contains("paint bucket")));
    assert!(!refused.iter().any(|row| row["operator"]
        .as_str()
        .unwrap()
        .contains("the Eraser tool")));
}

// ----------------------------------------------------- coverage targets --

/// `d` with a mask on layer 0 and one alpha channel, both canvas-sized.
fn with_coverage(d: &PrecisionDocument) -> PrecisionDocument {
    let n = (W * H) as usize;
    let ramp: Vec<u16> = (0..n).map(|i| (i * 811) as u16).collect();
    let masked = edit(
        d,
        json!({"op":"set_mask","layer":0,"width":W,"height":H,"x":0,"y":0,"linked":true}),
        &ramp,
    )
    .expect("a mask");
    let channel: Vec<u16> = (0..n).map(|i| 65535 - (i * 613) as u16).collect();
    edit(
        &masked,
        json!({"op":"set_channel","index":null,"name":"Coverage"}),
        &channel,
    )
    .expect("a channel")
}

#[test]
fn a_mask_and_a_channel_take_the_16_bit_route_sample_for_sample() {
    let samples = extended(W, H);
    let float = with_coverage(&float_document(W, H, &samples));
    let pixels = (0..W * H).map(|i| [i as u16 * 300, 7, 9, 65535]).collect();
    let typed = with_coverage(&doc(W, H, pixels));
    let n = (W * H) as usize;
    let coverage = stroke(0.0);
    let selection = fractional(W * H, 3);
    let two = json!({"kind":"gradient","from":FROM,"to":TO,"radial":false,
        "start":0,"end":65535,"opacity":0.8});
    let three = json!({"kind":"gradient","from":FROM,"to":TO,"radial":false,
        "start":0,"end":65535,"style":"reflected","stops":[
            {"position":0.0,"color":[0.0,0.0,0.0,1.0]},
            {"position":0.35,"color":[0.7,0.7,0.7,0.625]},
            {"position":1.0,"color":[1.0,1.0,1.0,1.0]}]});
    let mut cases: Vec<(String, Value, bool, Vec<u16>)> = Vec::new();
    for value in [0u16, 65535] {
        cases.push((
            format!("paint {value}"),
            json!({"kind":"paint","value":value,"opacity":0.6,"blend_mode":0}),
            false,
            coverage.clone(),
        ));
    }
    for (name, operation) in [("two-colour linear", two), ("three-stop reflected", three)] {
        // Unselected, the request takes whatever input its plan names.
        let request = json!({"op":"scalar_edit","target":"mask","index":0,
            "operation":operation.clone(),"selection":false});
        let planned = plan(&float, request)["input_samples"].as_u64().unwrap() as usize;
        cases.push((
            format!("{name} gradient"),
            operation.clone(),
            false,
            vec![65535; planned],
        ));
        assert!(planned == 0 || planned == n, "{name}: {planned}");
        cases.push((
            format!("{name} gradient, selected"),
            operation,
            true,
            selection.clone(),
        ));
    }
    for target in ["mask", "channel"] {
        let before = plane(&float, target, false);
        assert_eq!(
            before,
            plane(&typed, target, false),
            "{target}: the twins start equal"
        );
        for (name, operation, selected, input) in &cases {
            let a = scalar(&float, target, operation.clone(), *selected, input)
                .unwrap_or_else(|| panic!("{name} changes the float {target}"));
            let b = scalar(&typed, target, operation.clone(), *selected, input)
                .unwrap_or_else(|| panic!("{name} changes the 16-bit {target}"));
            let after = plane(&a, target, false);
            assert_ne!(after, before, "{name} on the {target}: the plane changes");
            assert_eq!(
                after,
                plane(&b, target, false),
                "{name} on the {target}: every u16 sample equals the 16-bit twin's"
            );
            assert_eq!(
                bits(&layer32(&a, 0)),
                bits(&samples),
                "{name} on the {target}: the float pixels are bitwise kept"
            );
        }
    }
}
