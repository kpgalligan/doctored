//! Filter-menu filters on a layer mask, a saved alpha channel
//! and one Red/Green/Blue/Alpha plane of a 32-bit float layer, black-box
//! through `scalar_edit` (`dr_precision_doc_edit` and its plan), under
//! `float_plane_filter`'s plane rule. The layer holds values below 0, above
//! 1 and alpha 0, 0.5 and 1; the mask and channel hold codes 0 and 65535.
//!
//! Every oracle is written here, and each bound is declared before the first
//! run and never widened:
//!
//! - GRAY TWIN (Red/Green/Blue). The layer route (`dr_precision_doc_selected_edit`)
//!   on a twin float layer of `[v, v, v, 1]`, its result `r` when `r == g == b`
//!   bit for bit, else the Rec. 709 luma in f64 rounded once: the plane's
//!   bits EXACTLY; every other component bit for bit.
//! - COVERAGE (linear ops but Add Noise). Mask and channel codes equal
//!   `round(65535·clamp(twin, 0, 1))` exactly, the twin read from `code/65535`;
//!   the float Alpha equals `clamp(twin, 0, 1)` bit for bit. High Pass and
//!   Unsharp Mask reach the clamp.
//! - COVERAGE ADD NOISE. On an all-0 mask and channel the codes equal the
//!   same request on the same mask of a 16-bit document bit for bit, and
//!   change; an all-0 float Alpha changes and stays inside 0…1.
//! - COVERAGE DISPLAY TWIN. The display-domain filters on an SDR mask and
//!   channel lie within 4 codes of the 16-bit document's, wherever that
//!   result is not 0 or 65535.
//! - PIXELATE on Red: the f64 cell mean, `≤ 1e-6` relative; OFFSET on Red:
//!   an exact move. A value above white stays above white.
//! - GATE. Weight 0 keeps the bits, 65535 takes the full result's, between is
//!   `v0·(1−t) + v1·t` in f64 rounded once (exact).
//! - REFUSALS. Liquify, Depth Blur and the Develop Filter refuse on every
//!   plane with their own sentence, in the plan and the edit, with
//!   non-identity and identity parameters and with Develop Filter masks; an
//!   identity Offset is NULL with no error; Lock Pixels
//!   refuses Red and admits the mask.
//! - PLAN. A float Red plane's scratch is at most the whole layer's plan for
//!   the same filter.
use std::ffi::CStr;
use std::ptr;

use doctored_core::doc_lock::LOCK_PIXELS;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::*;
use doctored_core::metadata::Metadata;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::tiff_float::FloatTiff;
use serde_json::{json, Value};

use crate::float_filters::filter as layer_filter;
use crate::float_filters_display::plate_codes;
use crate::native_scalar as native;
use native::LIMIT;

const W: u32 = 24;
const H: u32 = 18;
const COLORS: [&str; 3] = ["red", "green", "blue"];
/// `blend.rs`'s f32 constants, widened to f64.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];
const LIQUIFY: &str = "liquify edits layer pixels only";
const DEPTH_BLUR: &str = "depth blur edits layer pixels only: its depth guide, haze, warmth and saturation read the layer's colour";
const DEVELOP_FILTER: &str = "the develop filter edits layer pixels only at 32 bits: its temperature, tint, vibrance and saturation act on colour, which one plane does not hold";

// ------------------------------------------------------------- fixtures --

/// Extended-range straight samples: below 0, above 1 and alpha 0, 0.5, 1.
fn extended(w: u32, h: u32) -> Vec<[f32; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let r = -0.4 + 0.11 * x as f32 + 0.3 * ((x * 7 + y * 3) % 5) as f32;
            let g = 2.5 * ((x * 13 + y * 29) % 11) as f32 / 11.0 - 0.3;
            let b = 0.004 * (y * y) as f32 - 0.5 + 0.2 * ((x + 2 * y) % 3) as f32;
            let a = [0.0, 0.5, 1.0][((x + 2 * y) % 3) as usize];
            [r, g, b, a]
        })
        .collect()
}

/// A float document holding one canvas-sized raster of `samples`, opened
/// from a float TIFF so the raster holds exactly these samples.
fn float_document(w: u32, h: u32, samples: &[[f32; 4]]) -> PrecisionDocument {
    let tiff = FloatTiff {
        width: w,
        height: h,
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
    let path = directory.path().join("plane.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    assert_eq!(bits(&layer32(&doc)), bits(samples));
    doc
}

fn layer32(d: &PrecisionDocument) -> Vec<[f32; 4]> {
    d.layer_source(0).unwrap().to_rgba32().unwrap()
}

fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

/// Mask codes holding 0, 65535 and a spread between.
fn mask_codes(n: u32) -> Vec<u16> {
    (0..n)
        .map(|i| match i % 7 {
            0 => 0,
            1 => 65535,
            _ => ((i * 2741) % 65536) as u16,
        })
        .collect()
}

fn channel_codes(n: u32) -> Vec<u16> {
    (0..n)
        .map(|i| match i % 5 {
            0 => 65535,
            3 => 0,
            _ => (65535 - (i * 1913) % 65536) as u16,
        })
        .collect()
}

/// `d` with a canvas-sized mask on layer 0 and one alpha channel.
fn with_planes(d: &PrecisionDocument, mask: &[u16], channel: &[u16]) -> PrecisionDocument {
    let (w, h) = d.dimensions();
    let masked = native::edit(
        d,
        json!({"op":"set_mask","layer":0,"width":w,"height":h,"x":0,"y":0,"linked":true}),
        mask,
    )
    .expect("a mask");
    native::edit(
        &masked,
        json!({"op":"set_channel","index":null,"name":"Coverage"}),
        channel,
    )
    .expect("a channel")
}

/// The float layer with its mask and channel.
fn subject() -> PrecisionDocument {
    let n = W * H;
    with_planes(
        &float_document(W, H, &extended(W, H)),
        &mask_codes(n),
        &channel_codes(n),
    )
}

fn request(target: &str, op: &str, params: &Value, selection: bool) -> Value {
    json!({"op":"scalar_edit","target":target,"index":0,"selection":selection,
        "operation":{"kind":"filter","edit":{"layer":0,"op":op,"params":params}}})
}

fn take(e: *mut std::ffi::c_char) -> String {
    unsafe { native::error(e) }
}

fn try_plan(d: &PrecisionDocument, v: &Value) -> Result<Value, String> {
    let t = native::text(v.clone());
    let mut e = ptr::null_mut();
    let p = unsafe { dr_precision_doc_edit_plan(d, t.as_ptr(), LIMIT, 0, &mut e) };
    if p.is_null() {
        return Err(take(e));
    }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Ok(serde_json::from_str(&s).unwrap())
}

/// `Ok(None)` is the exact no-op answer (NULL with no error).
fn try_edit(
    d: &PrecisionDocument,
    v: &Value,
    input: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
    let t = native::text(v.clone());
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            d,
            t.as_ptr(),
            input.as_ptr(),
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    let error = take(e);
    if out.is_null() {
        return if error.is_empty() {
            Ok(None)
        } else {
            Err(error)
        };
    }
    assert!(error.is_empty(), "{error}");
    Ok(Some(*unsafe { Box::from_raw(out) }))
}

fn run(d: &PrecisionDocument, target: &str, op: &str, params: &Value) -> Option<PrecisionDocument> {
    let v = request(target, op, params, false);
    let p = try_plan(d, &v).unwrap_or_else(|e| panic!("{target} {op} plan: {e}"));
    assert_eq!(p["input_samples"], 0, "{target} {op}");
    try_edit(d, &v, &[]).unwrap_or_else(|e| panic!("{target} {op} {params}: {e}"))
}

/// The plane rule's Result, restated: `r` when the three agree bit for bit,
/// else the Rec. 709 luma in f64 rounded once.
fn rule(p: [f32; 4]) -> f32 {
    if p[0].to_bits() == p[1].to_bits() && p[1].to_bits() == p[2].to_bits() {
        return p[0];
    }
    (f64::from(LUMA[0]) * f64::from(p[0])
        + f64::from(LUMA[1]) * f64::from(p[1])
        + f64::from(LUMA[2]) * f64::from(p[2])) as f32
}

/// The layer route on a twin float layer of `[v, v, v, 1]`, through the rule;
/// `None` when the route leaves the twin unchanged.
fn twin(values: &[f32], op: &str, params: &Value) -> Option<Vec<f32>> {
    let pixels: Vec<[f32; 4]> = values.iter().map(|&v| [v, v, v, 1.0]).collect();
    let doc = float_document(W, H, &pixels);
    layer_filter(&doc, op, params, &[])
        .unwrap_or_else(|e| panic!("twin {op} {params}: {e}"))
        .map(|out| layer32(&out).into_iter().map(rule).collect())
}

fn component(pixels: &[[f32; 4]], c: usize) -> Vec<f32> {
    pixels.iter().map(|p| p[c]).collect()
}

fn index_of(target: &str) -> usize {
    ["red", "green", "blue", "alpha"]
        .iter()
        .position(|t| *t == target)
        .unwrap()
}

// --------------------------------------------------------------- the ops --

/// The linear-domain filters: the Gaussian family (both Gaussian routes),
/// the five means, Offset, Lens Correction and Pixelate.
fn linear_requests() -> Vec<(&'static str, Value)> {
    vec![
        ("blur", json!({"sigma": 2.5})),
        ("blur", json!({"sigma": 8})),
        ("unsharp_mask", json!({"radius": 2, "amount": 1.5})),
        ("high_pass", json!({"radius": 3})),
        ("box_blur", json!({"radius": 2})),
        ("motion_blur", json!({"angle": 30, "distance": 7})),
        ("radial_blur", json!({"amount": 30})),
        (
            "lens_blur",
            json!({"radius": 3, "brightness": 50, "threshold": 128}),
        ),
        (
            "tilt_shift",
            json!({"radius": 4, "focus": 0.05, "transition": 0.3}),
        ),
        ("offset", json!({"horizontal": 5, "vertical": 3})),
        ("lens_correction", json!({"distortion": 40})),
        ("pixelate", json!({"block": 3})),
    ]
}

/// `float_filters_display.rs`'s requests plus Presence and Smart Sharpen:
/// the display-domain filters and Add Noise.
fn display_requests() -> Vec<(&'static str, Value)> {
    vec![
        ("surface_blur", json!({"radius": 2, "threshold": 30})),
        (
            "dust_and_scratches",
            json!({"radius": 2, "threshold": 0.02}),
        ),
        ("reduce_noise", json!({"radius": 2, "luminance": 0.05})),
        ("add_noise", json!({"amount": 0.1, "seed": 2})),
        ("add_grain", json!({"amount": 40, "seed": 2})),
        ("clouds", json!({"seed": 3})),
        ("difference_clouds", json!({"seed": 3})),
        ("emboss", json!({})),
        ("edge_detect", json!({})),
        ("sharpen", json!({"amount": 1})),
        ("presence", json!({"texture": 50, "clarity": 50})),
        (
            "smart_sharpen",
            json!({"amount": 150, "radius": 1.5, "noise": 30, "remove": "gaussian",
                "shadow_fade": 50, "shadow_tonal_width": 40, "shadow_radius": 2,
                "highlight_fade": 40, "highlight_tonal_width": 60, "highlight_radius": 3}),
        ),
    ]
}

// ------------------------------------------------------------------ tests --

#[test]
fn a_colour_plane_is_its_gray_twin_through_the_rule() {
    let d = subject();
    let before = layer32(&d);
    let mut checked = 0;
    for (op, params) in linear_requests().into_iter().chain(display_requests()) {
        for target in COLORS {
            let c = index_of(target);
            let values = component(&before, c);
            let want = twin(&values, op, &params);
            let got = run(&d, target, op, &params);
            let (Some(want), Some(got)) = (want, got.as_ref()) else {
                panic!("{target} {op}: the twin and the plane disagree on a change");
            };
            let after = layer32(got);
            for i in 0..before.len() {
                assert_eq!(
                    after[i][c].to_bits(),
                    want[i].to_bits(),
                    "{target} {op} {params} pixel {i}: {} vs {}",
                    after[i][c],
                    want[i]
                );
                for k in (0..4).filter(|k| *k != c) {
                    assert_eq!(
                        after[i][k].to_bits(),
                        before[i][k].to_bits(),
                        "{target} {op} {k}"
                    );
                }
            }
            assert_eq!(
                native::plane(got, "mask", false),
                native::plane(&d, "mask", false)
            );
            checked += 1;
        }
    }
    assert!(checked >= 3 * 24, "{checked}");
}

#[test]
fn coverage_planes_are_the_clamped_twin() {
    let d = subject();
    let before = layer32(&d);
    for (op, params) in linear_requests() {
        let mut clamped = false;
        for target in ["mask", "channel"] {
            let codes = native::plane(&d, target, false);
            let values: Vec<f32> = codes.iter().map(|&c| f32::from(c) / 65535.0).collect();
            let want = twin(&values, op, &params).expect("the twin changes");
            clamped |= want.iter().any(|v| !(0.0..=1.0).contains(v));
            let got = run(&d, target, op, &params).expect("the plane changes");
            let expected: Vec<u16> = want
                .iter()
                .map(|v| (f64::from(v.clamp(0.0, 1.0)) * 65535.0).round() as u16)
                .collect();
            assert_eq!(
                native::plane(&got, target, false),
                expected,
                "{target} {op}"
            );
            assert_eq!(bits(&layer32(&got)), bits(&before), "{target} {op}");
        }
        let values = component(&before, 3);
        let want = twin(&values, op, &params).expect("the twin changes");
        clamped |= want.iter().any(|v| !(0.0..=1.0).contains(v));
        let got = run(&d, "alpha", op, &params).expect("the alpha changes");
        let after = layer32(&got);
        for i in 0..before.len() {
            assert_eq!(
                after[i][3].to_bits(),
                want[i].clamp(0.0, 1.0).to_bits(),
                "alpha {op} pixel {i}"
            );
            for k in 0..3 {
                assert_eq!(after[i][k].to_bits(), before[i][k].to_bits(), "alpha {op}");
            }
        }
        if matches!(op, "high_pass" | "unsharp_mask") {
            assert!(clamped, "{op} must reach the clamp");
        }
    }
}

#[test]
fn add_noise_on_coverage_is_the_16_bit_additive_draw() {
    let n = W * H;
    let zeros = vec![0u16; n as usize];
    let float = with_planes(&float_document(W, H, &extended(W, H)), &zeros, &zeros);
    let typed = with_planes(
        &native::doc(W, H, vec![[1000, 2000, 3000, 65535]; n as usize]),
        &zeros,
        &zeros,
    );
    let params = json!({"amount": 0.2, "seed": 11});
    for target in ["mask", "channel"] {
        let a = run(&float, target, "add_noise", &params).expect("noise lifts a black mask");
        let b = run(&typed, target, "add_noise", &params).expect("the 16-bit twin changes");
        let codes = native::plane(&a, target, false);
        assert!(codes.iter().any(|&c| c > 0), "{target}");
        assert_eq!(codes, native::plane(&b, target, false), "{target}");
    }
    let clear: Vec<[f32; 4]> = extended(W, H)
        .into_iter()
        .map(|p| [p[0], p[1], p[2], 0.0])
        .collect();
    let d = float_document(W, H, &clear);
    let got = run(&d, "alpha", "add_noise", &params).expect("noise lifts a zero alpha");
    let alpha = component(&layer32(&got), 3);
    assert!(alpha.iter().any(|&a| a > 0.0));
    assert!(alpha.iter().all(|a| (0.0..=1.0).contains(a)));
}

#[test]
fn display_filters_on_coverage_match_the_16_bit_mask() {
    let n = (W * H) as usize;
    let plate = plate_codes(W, H);
    let mask: Vec<u16> = plate.iter().map(|p| p[0]).collect();
    let channel: Vec<u16> = plate.iter().map(|p| p[1]).collect();
    let float = with_planes(&float_document(W, H, &extended(W, H)), &mask, &channel);
    let typed = with_planes(
        &native::doc(W, H, vec![[1000, 2000, 3000, 65535]; n]),
        &mask,
        &channel,
    );
    for (op, params) in [
        ("surface_blur", json!({"radius": 3, "threshold": 40})),
        ("surface_blur", json!({"radius": 1, "threshold": 6})),
        ("dust_and_scratches", json!({"radius": 2, "threshold": 0})),
        (
            "reduce_noise",
            json!({"radius": 2, "luminance": 0.05, "detail": 0.4, "color": 0.6}),
        ),
        (
            "add_grain",
            json!({"amount": 60, "size": 1.5, "roughness": 30, "seed": 7}),
        ),
        ("clouds", json!({"seed": 5})),
        (
            "clouds",
            json!({"seed": 9, "foreground_color": "#204080", "background_color": "#f0e0a0",
                "high_contrast": true}),
        ),
        ("difference_clouds", json!({"seed": 5})),
        ("emboss", json!({})),
        ("edge_detect", json!({})),
    ] {
        for target in ["mask", "channel"] {
            let a = run(&float, target, op, &params).map(|d| native::plane(&d, target, false));
            let b = run(&typed, target, op, &params).map(|d| native::plane(&d, target, false));
            let (Some(a), Some(b)) = (a, b) else {
                panic!("{target} {op}: one side left the plane unchanged");
            };
            let mut compared = 0;
            for (i, (&got, &want)) in a.iter().zip(&b).enumerate() {
                if want == 0 || want == 65535 {
                    continue;
                }
                assert!(
                    (i32::from(got) - i32::from(want)).abs() <= 4,
                    "{target} {op} {params} sample {i}: {got} vs {want}"
                );
                compared += 1;
            }
            assert!(compared > n / 5, "{target} {op}: only {compared} compared");
        }
    }
}

#[test]
fn pixelate_and_offset_on_red_against_their_oracles() {
    let d = subject();
    let before = layer32(&d);
    let red = component(&before, 0);
    let block = 4u32;
    let got = run(&d, "red", "pixelate", &json!({"block": block})).expect("pixelates");
    let after = layer32(&got);
    let mut above = false;
    for y in 0..H {
        for x in 0..W {
            let (cx, cy) = (x / block * block, y / block * block);
            let (cw, ch) = (block.min(W - cx), block.min(H - cy));
            let mut sum = 0.0f64;
            for yy in cy..cy + ch {
                for xx in cx..cx + cw {
                    sum += f64::from(red[(yy * W + xx) as usize]);
                }
            }
            let mean = sum / f64::from(cw * ch);
            let v = f64::from(after[(y * W + x) as usize][0]);
            assert!(
                (v - mean).abs() <= 1e-6 * mean.abs().max(1.0),
                "pixelate ({x}, {y}): {v} vs {mean}"
            );
            if mean > 1.0 {
                assert!(v > 1.0);
                above = true;
            }
        }
    }
    assert!(above, "a cell above white");
    let (dx, dy) = (5i64, 3i64);
    let got = run(
        &d,
        "red",
        "offset",
        &json!({"horizontal": dx, "vertical": dy}),
    )
    .expect("moves");
    let after = layer32(&got);
    for y in 0..i64::from(H) {
        for x in 0..i64::from(W) {
            let sx = (x - dx).rem_euclid(i64::from(W));
            let sy = (y - dy).rem_euclid(i64::from(H));
            let i = (y * i64::from(W) + x) as usize;
            assert_eq!(
                after[i][0].to_bits(),
                red[(sy * i64::from(W) + sx) as usize].to_bits(),
                "offset ({x}, {y})"
            );
            for k in 1..4 {
                assert_eq!(after[i][k].to_bits(), before[i][k].to_bits());
            }
        }
    }
}

#[test]
fn the_selection_gates_the_write_at_its_ends_and_lerps_between() {
    let d = subject();
    let before = layer32(&d);
    let n = (W * H) as usize;
    let weights: Vec<u16> = (0..n).map(|i| [0, 65535, 20000, 47000][i % 4]).collect();
    let params = json!({"sigma": 2.5});
    for target in ["red", "alpha"] {
        let c = index_of(target);
        let full = layer32(&run(&d, target, "blur", &params).unwrap());
        let v = request(target, "blur", &params, true);
        assert_eq!(try_plan(&d, &v).unwrap()["input_samples"], n);
        let got = layer32(&try_edit(&d, &v, &weights).unwrap().unwrap());
        for i in 0..n {
            let want = match weights[i] {
                0 => before[i][c],
                65535 => full[i][c],
                w => {
                    let t = f64::from(w) / 65535.0;
                    (f64::from(before[i][c]) * (1.0 - t) + f64::from(full[i][c]) * t) as f32
                }
            };
            assert_eq!(got[i][c].to_bits(), want.to_bits(), "{target} {i}");
        }
    }
    let codes = native::plane(&d, "mask", false);
    let full = native::plane(&run(&d, "mask", "blur", &params).unwrap(), "mask", false);
    let v = request("mask", "blur", &params, true);
    let got = native::plane(&try_edit(&d, &v, &weights).unwrap().unwrap(), "mask", false);
    for i in 0..n {
        match weights[i] {
            0 => assert_eq!(got[i], codes[i]),
            65535 => assert_eq!(got[i], full[i]),
            // The u16 walk lerps the f64 result, which `full` holds rounded.
            w => {
                let t = f64::from(w) / 65535.0;
                let want = f64::from(codes[i]) * (1.0 - t) + f64::from(full[i]) * t;
                assert!((f64::from(got[i]) - want).abs() <= 1.0, "mask {i}");
            }
        }
    }
}

#[test]
fn the_layer_only_filters_refuse_by_name_on_every_plane() {
    let d = subject();
    let stroke = json!({"tool": "forward_warp", "size": 8.0, "strength": 1.0, "density": 1,
        "points": [[6, 6], [12, 9]]});
    let samples: Vec<f64> = (0..W * H).map(|i| f64::from(i % 97) / 96.0).collect();
    let depth = json!({"width": W, "height": H, "samples": samples, "frame": [0, 0, W, H]});
    let cases = [
        ("liquify", json!({"strokes": [stroke]}), LIQUIFY),
        // A zero-strength stroke: the identity field.
        (
            "liquify",
            json!({"strokes": [{"tool": "forward_warp", "size": 8.0, "strength": 0.0,
                "density": 1, "points": [[6, 6], [12, 9]]}]}),
            LIQUIFY,
        ),
        (
            "depth_blur",
            json!({"depth": depth.clone(), "blur_strength": 60, "focal_distance": 40}),
            DEPTH_BLUR,
        ),
        (
            "depth_blur",
            json!({"depth": depth, "blur_strength": 0}),
            DEPTH_BLUR,
        ),
        (
            "develop_filter",
            json!({"exposure": 0.5, "temperature": 20}),
            DEVELOP_FILTER,
        ),
        ("develop_filter", json!({}), DEVELOP_FILTER),
        (
            "develop_filter",
            json!({"masks": [{"shape": {"op": "full"}, "exposure": 0.5}]}),
            DEVELOP_FILTER,
        ),
    ];
    for target in ["mask", "channel", "red", "green", "blue", "alpha"] {
        for (op, params, sentence) in &cases {
            let v = request(target, op, params, false);
            assert_eq!(
                try_plan(&d, &v).err().as_deref(),
                Some(*sentence),
                "{target} {op} {params} plan"
            );
            assert_eq!(
                try_edit(&d, &v, &[]).err().as_deref(),
                Some(*sentence),
                "{target} {op} {params} edit"
            );
        }
    }
}

#[test]
fn an_identity_filter_is_null_and_lock_pixels_follows_the_target() {
    let d = subject();
    let still = json!({"horizontal": 0, "vertical": 0});
    for target in ["mask", "channel", "red", "alpha"] {
        assert!(
            try_edit(&d, &request(target, "offset", &still, false), &[])
                .unwrap()
                .is_none(),
            "{target}"
        );
    }
    let locked = d.with_layer_locks(0, LOCK_PIXELS).unwrap().unwrap();
    let params = json!({"sigma": 2.0});
    let message = try_edit(&locked, &request("red", "blur", &params, false), &[])
        .err()
        .unwrap_or_default();
    assert!(message.contains("locked"), "{message}");
    assert!(run(&locked, "mask", "blur", &params).is_some());
}

#[test]
fn a_red_plane_plans_no_more_than_the_whole_layer() {
    let (w, h) = (1600u32, 1200u32);
    let pixels: Vec<[f32; 4]> = (0..w * h)
        .map(|i| [(i % 977) as f32 / 300.0 - 0.5, 0.25, 1.5, 1.0])
        .collect();
    let d = crate::float_filters::solid_doc(&crate::float_filters::Picture { w, h, pixels });
    for (op, params) in [
        ("blur", json!({"sigma": 2})),
        ("surface_blur", json!({"radius": 5, "threshold": 15})),
    ] {
        let plane = try_plan(&d, &request("red", op, &params, false)).unwrap()["scratch_bytes"]
            .as_u64()
            .unwrap();
        let layer = crate::float_filters::plan(&d, op, &params).unwrap()["scratch_bytes"]
            .as_u64()
            .unwrap();
        assert!(plane <= layer, "{op}: plane {plane} > layer {layer}");
    }
}
