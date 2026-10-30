//! Edit ▸ Fill… / Stroke… in a 32-bit float document, black-box
//! through the request API: `paint_blend` over extended-range backdrops
//! (negatives, values above white, hidden RGB under alpha 0) for the five
//! admitted modes against an independent f64 W3C oracle, the linear ink
//! decode, the refusals, Preserve Transparency, the transparency lock and
//! growth of a trimmed layer, `stroke_layer_edge` on an f32 layer, the
//! extended-range output refusal, coverage targets (mask, alpha channel)
//! against the same request on a u16 document, and the capability rows.
//!
//! Float documents are authored as float TIFFs and opened with
//! `open_preserving` (`tests/float_scoped_statistics.rs`), so a raster
//! holds exactly the samples the oracle reads.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc_lock::LOCK_TRANSPARENCY;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_float::dr_float_operators;
use doctored_core::ffi_precision_conversion::dr_precision_capability;
use doctored_core::ffi_precision_edit::*;
use doctored_core::metadata::Metadata;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::tiff_float::FloatTiff;
use serde_json::{json, Value};

use crate::native_scalar as native;
use native::*;

const W: u32 = 11;
const H: u32 = 7;
/// Normal, Multiply, Darken, Lighten, Difference.
const MODES: [u32; 5] = [0, 1, 6, 7, 8];
const UNAVAILABLE: &str = "is unavailable in a 32-bit float document";
const INVERTS_ABOUT_WHITE: &str = "each formula inverts about white";

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

/// A float document holding one canvas-sized raster of `samples`.
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
    let path = directory.path().join("fill.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    let held: Vec<[u32; 4]> = layer32(&doc, 0)
        .into_iter()
        .map(|p| p.map(f32::to_bits))
        .collect();
    let wanted: Vec<[u32; 4]> = samples.iter().map(|p| p.map(f32::to_bits)).collect();
    assert_eq!(held, wanted, "the raster holds the TIFF samples");
    doc
}

fn layer32(d: &PrecisionDocument, i: usize) -> Vec<[f32; 4]> {
    d.layer_source(i).unwrap().to_rgba32().unwrap()
}

fn geometry(d: &PrecisionDocument, i: usize) -> ((u32, u32), (i32, i32)) {
    let layer = d.layer(i).unwrap();
    (layer.source().unwrap().dimensions(), layer.offset())
}

/// Fractional coverage holding 0, 65535 and intermediate weights.
fn coverage(n: u32, salt: u32) -> Vec<u16> {
    (0..n)
        .map(|i| match (i + salt) % 5 {
            0 => 0,
            1 => 65535,
            k => (k * 9001 + i * 7) as u16,
        })
        .collect()
}

fn paint_blend(color: [f64; 4], space: &str, opacity: f64, mode: u32, selection: bool) -> Value {
    json!({"op":"paint_blend","layer":0,"color":color,"color_space":space,
        "opacity":opacity,"blend_mode":mode,"selection":selection})
}

/// The plan's refusal for `v`, which must refuse.
fn plan_error(d: &PrecisionDocument, v: Value) -> String {
    let t = text(v);
    let mut e = ptr::null_mut();
    let p = unsafe { dr_precision_doc_edit_plan(d, t.as_ptr(), LIMIT, 0, &mut e) };
    assert!(p.is_null(), "the plan was expected to refuse");
    unsafe { error(e) }
}

/// The edit's refusal for `v`, which must refuse after a successful plan.
fn edit_error(d: &PrecisionDocument, v: Value, input: &[u16]) -> String {
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
            ptr::null(),
            &mut e,
        )
    };
    assert!(out.is_null(), "the edit was expected to refuse");
    unsafe { error(e) }
}

// --------------------------------------------------------------- oracle --

/// The sRGB EOTF, written out.
fn srgb_eotf(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// The W3C separable blend `B(cb, cs)` for the five admitted modes.
fn blend(mode: u32, cb: f64, cs: f64) -> f64 {
    match mode {
        0 => cs,
        1 => cb * cs,
        6 => cb.min(cs),
        7 => cb.max(cs),
        8 => (cb - cs).abs(),
        _ => unreachable!("not an admitted mode"),
    }
}

/// One W3C source-over step of straight `cs` at effective alpha `sa` onto
/// the straight backdrop `bg`, in binary64.
fn composite(mode: u32, bg: [f32; 4], cs: [f64; 3], sa: f64) -> [f64; 4] {
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

/// The ink a float document decodes `color` to: a full-coverage opaque
/// Normal fill shows the ink itself.
fn decoded_ink(color: [f64; 3], space: &str) -> [f32; 3] {
    let d = float_document(2, 1, &[[3.0, -1.0, 0.5, 1.0], [0.0, 0.0, 0.0, 0.0]]);
    let out = edit(
        &d,
        paint_blend([color[0], color[1], color[2], 1.0], space, 1.0, 0, false),
        &[65535; 2],
    )
    .expect("the fill changes the layer");
    let pixels = layer32(&out, 0);
    for p in &pixels {
        assert_eq!(p[3], 1.0);
        assert_eq!(&p[..3], &pixels[0][..3], "every pixel shows the ink");
    }
    [pixels[0][0], pixels[0][1], pixels[0][2]]
}

// ---------------------------------------------------------------- tests --

#[test]
fn paint_blend_matches_the_w3c_oracle_over_extended_range_backdrops() {
    let samples = extended(W, H);
    let d = float_document(W, H, &samples);
    let color = [0.9, 0.2, 0.1, 0.8];
    let ink = decoded_ink([color[0], color[1], color[2]], "document");
    let cs = ink.map(f64::from);
    let n = W * H;
    for mode in MODES {
        for selected in [false, true] {
            let mut input = coverage(n, 0);
            if selected {
                input.extend(coverage(n, 2));
            }
            let out = edit(
                &d,
                paint_blend(color, "document", 0.7, mode, selected),
                &input,
            )
            .expect("the fill changes pixels");
            assert_eq!(geometry(&out, 0), ((W, H), (0, 0)));
            let after = layer32(&out, 0);
            for (i, (bg, got)) in samples.iter().zip(&after).enumerate() {
                let mut weight = f64::from(input[i]) / 65535.0;
                if selected {
                    weight *= f64::from(input[n as usize + i]) / 65535.0;
                }
                let sa = color[3] * 0.7 * weight;
                if sa <= 0.0 {
                    assert_eq!(
                        got.map(f32::to_bits),
                        bg.map(f32::to_bits),
                        "mode {mode} at {i}: uncovered pixels are bitwise kept"
                    );
                    continue;
                }
                let expected = composite(mode, *bg, cs, sa);
                for c in 0..4 {
                    assert!(
                        close(got[c], expected[c]),
                        "mode {mode} selection {selected} at {i} channel {c}: {got:?} vs {expected:?} over {bg:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn ink_decodes_to_linear_light_through_the_profile_curves() {
    let document = decoded_ink([0.5, 0.25, 1.0], "document");
    let expected = [srgb_eotf(0.5), srgb_eotf(0.25), 1.0];
    assert!((expected[0] - 0.214041).abs() < 1e-6);
    for c in 0..3 {
        assert!(
            (f64::from(document[c]) - expected[c]).abs() < 2e-5,
            "{document:?} vs {expected:?}"
        );
    }
    // The document profile is sRGB: sRGB ink decodes the same way.
    let srgb = decoded_ink([0.5, 0.25, 1.0], "srgb");
    for c in 0..3 {
        assert!(
            (f64::from(srgb[c]) - expected[c]).abs() < 2e-5,
            "{srgb:?} vs {expected:?}"
        );
    }
}

#[test]
fn refused_modes_name_the_float_rule_and_the_bucket_and_dodge_burn_plan() {
    let d = float_document(W, H, &extended(W, H));
    // The nine modes whose formula inverts about white refuse by name (the
    // operator table gives the reason, below; decision B); Divide is admitted.
    for (mode, name) in [(2, "Screen"), (3, "Overlay"), (18, "vivid_light")] {
        let message = plan_error(&d, paint_blend([0.5; 4], "document", 1.0, mode, false));
        assert!(message.contains(UNAVAILABLE), "{message}");
        assert!(
            message.to_lowercase().contains(&name.to_lowercase()),
            "{message}"
        );
    }
    plan(&d, paint_blend([0.5; 4], "document", 1.0, 22, false));
    for request in [
        json!({"op":"bucket_fill","layer":0,"color":[0.5,0.5,0.5,1.0],"color_space":"document",
            "opacity":1.0,"x":1,"y":1,"tolerance":0,"contiguous":true}),
        json!({"op":"dodge_burn","layer":0,"exposure":0.5,"range":1,"burn":false}),
    ] {
        plan(&d, request);
    }
}

#[test]
fn preserve_alpha_keeps_every_alpha_and_transparent_pixel() {
    let samples = extended(W, H);
    let d = float_document(W, H, &samples);
    let mut request = paint_blend([0.9, 0.2, 0.1, 1.0], "document", 0.75, 0, false);
    request["preserve_alpha"] = json!(true);
    let out = edit(&d, request, &vec![65535; (W * H) as usize]).expect("colour changes");
    assert_eq!(geometry(&out, 0), ((W, H), (0, 0)));
    let after = layer32(&out, 0);
    assert_ne!(after, samples);
    for (b, a) in samples.iter().zip(&after) {
        assert_eq!(a[3].to_bits(), b[3].to_bits(), "alpha is bitwise kept");
        if b[3] == 0.0 {
            assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
        }
    }
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

#[test]
fn a_transparency_lock_clips_and_a_trimmed_layer_grows() {
    let samples = extended(W, H);
    let d = trimmed(&float_document(W, H, &samples));
    let (cw, ch) = (W + 8, H + 6);
    let request = paint_blend([0.9, 0.2, 0.1, 1.0], "document", 1.0, 0, false);
    let locked = d.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    let whole = vec![65535u16; (cw * ch) as usize];
    let out = edit(&locked, request.clone(), &whole).expect("clipped paint");
    assert_eq!(geometry(&out, 0), ((W, H), (4, 3)));
    for (b, a) in samples.iter().zip(layer32(&out, 0)) {
        assert_eq!(a[3].to_bits(), b[3].to_bits());
    }
    // Unlocked, a rect reaching past the layer's top-left grows it to the
    // union, and every untouched sample is bitwise kept.
    let rect: Vec<u16> = (0..cw * ch)
        .map(|i| {
            let (x, y) = (i % cw, i / cw);
            if x < 6 && (1..5).contains(&y) {
                65535
            } else {
                0
            }
        })
        .collect();
    let grown = edit(&d, request, &rect).expect("the fill grows the layer");
    assert_eq!(geometry(&grown, 0), ((W + 4, H + 2), (0, 1)));
    let after = layer32(&grown, 0);
    let ink = decoded_ink([0.9, 0.2, 0.1], "document");
    for y in 0..H + 2 {
        for x in 0..W + 4 {
            let got = after[(y * (W + 4) + x) as usize];
            let (cx, cy) = (x, y + 1);
            if cx < 6 && (1..5).contains(&cy) {
                assert_eq!(got, [ink[0], ink[1], ink[2], 1.0], "painted ({cx}, {cy})");
            } else if (4..4 + W).contains(&cx) && (3..3 + H).contains(&cy) {
                let held = samples[((cy - 3) * W + cx - 4) as usize];
                assert_eq!(got.map(f32::to_bits), held.map(f32::to_bits));
            } else {
                assert_eq!(got, [0.0; 4], "growth is transparent at ({cx}, {cy})");
            }
        }
    }
}

/// A hard-alpha rect `(6..16, 5..13)` on a 24×20 canvas, as f32 samples.
fn hard_rect() -> Vec<[f32; 4]> {
    (0..24 * 20)
        .map(|i| {
            let (x, y) = (i % 24, i / 24);
            if (6..16).contains(&x) && (5..13).contains(&y) {
                [x as f32 / 2.0 - 4.0, -0.5, 7.0, 1.0]
            } else {
                [0.0; 4]
            }
        })
        .collect()
}

fn edge_request(width: f64, location: &str) -> Value {
    json!({"op":"stroke_layer_edge","layer":0,"width":width,"location":location,
        "color":[0.1,0.6,0.9,1.0],"color_space":"document","opacity":1.0,"blend_mode":0})
}

#[test]
fn layer_edge_stroke_on_a_float_layer() {
    let samples = hard_rect();
    let d = float_document(24, 20, &samples);
    let u16_twin = doc(
        24,
        20,
        samples
            .iter()
            .map(|p| {
                if p[3] == 1.0 {
                    [30000, 20000, 10000, 65535]
                } else {
                    [0; 4]
                }
            })
            .collect(),
    );
    let ink = decoded_ink([0.1, 0.6, 0.9], "document").map(f64::from);
    // The frame: the alpha bounds grown by ceil(2.5) + 2 = 5 on every side.
    let (fx, fy, fw, fh) = (1u32, 0u32, 20u32, 18u32);
    let plane: Vec<u16> = (0..fw * fh)
        .map(|i| {
            let (x, y) = (i % fw + fx, i / fw + fy);
            if (6..16).contains(&x) && (5..13).contains(&y) {
                65535
            } else {
                0
            }
        })
        .collect();
    for location in ["inside", "center", "outside"] {
        assert_eq!(plan(&d, edge_request(2.5, location))["outline"], true);
        let out = edit(&d, edge_request(2.5, location), &[]).expect("a stroke");
        let twin = edit(&u16_twin, edge_request(2.5, location), &[]).expect("a stroke");
        assert_eq!(geometry(&out, 0), geometry(&twin, 0), "{location}");
        let band = selection(
            fw,
            fh,
            json!({"kind":"stroke","width":2.5,"location":location}),
            &plane,
        );
        let after = layer32(&out, 0);
        let twin_pixels = twin.layer_source(0).unwrap().to_rgba16().unwrap();
        for (i, (bg, got)) in samples.iter().zip(&after).enumerate() {
            let (x, y) = (i as u32 % 24, i as u32 / 24);
            let alpha = f64::from(twin_pixels[i][3]) / 65535.0;
            assert!(
                (f64::from(got[3]) - alpha).abs() <= 1.0 / 65535.0,
                "{location} band alpha at ({x}, {y}): {got:?} vs {alpha}"
            );
            let inside = (fx..fx + fw).contains(&x) && (fy..fy + fh).contains(&y);
            let weight = if inside {
                f64::from(band[((y - fy) * fw + x - fx) as usize]) / 65535.0
            } else {
                0.0
            };
            if weight == 0.0 {
                assert_eq!(got.map(f32::to_bits), bg.map(f32::to_bits));
                continue;
            }
            let expected = composite(0, *bg, ink, weight);
            for c in 0..4 {
                assert!(
                    close(got[c], expected[c]),
                    "{location} at ({x}, {y}) channel {c}: {got:?} vs {expected:?}"
                );
            }
        }
    }
    let empty = float_document(6, 5, &[[2.0, -1.0, 0.5, 0.0]; 30]);
    assert_eq!(plan(&empty, edge_request(2.0, "outside"))["outline"], false);
    assert!(edit(&empty, edge_request(2.0, "outside"), &[]).is_none());
}

#[test]
fn a_result_past_the_extended_range_refuses() {
    let mut samples = vec![[0.5f32, 0.5, 0.5, 1.0]; 4];
    samples[0] = [-65504.0, 0.0, 0.0, 1.0];
    let d = float_document(2, 2, &samples);
    let message = edit_error(
        &d,
        paint_blend([1.0, 1.0, 1.0, 1.0], "document", 1.0, 8, false),
        &[65535; 4],
    );
    assert!(message.contains("65504"), "{message}");
    // The same fill without the extreme sample is admitted.
    let tame = float_document(2, 2, &[[0.25, 2.0, -3.0, 1.0]; 4]);
    assert!(edit(
        &tame,
        paint_blend([1.0, 1.0, 1.0, 1.0], "document", 1.0, 8, false),
        &[65535; 4],
    )
    .is_some());
}

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
fn coverage_targets_match_the_u16_document() {
    let float = with_coverage(&float_document(W, H, &extended(W, H)));
    let pixels = (0..W * H).map(|i| [i as u16 * 300, 7, 9, 65535]).collect();
    let typed = with_coverage(&doc(W, H, pixels));
    let input = coverage(W * H, 1);
    for target in ["mask", "channel"] {
        for mode in [0, 1, 6] {
            let operation = json!({"kind":"paint","value":40000,"opacity":0.6,"blend_mode":mode});
            let a = scalar(&float, target, operation.clone(), false, &input).expect("paints");
            let b = scalar(&typed, target, operation, false, &input).expect("paints");
            assert_eq!(
                plane(&a, target, false),
                plane(&b, target, false),
                "{target} {mode}"
            );
        }
    }
    let red = json!({"op":"scalar_edit","target":"red","index":0,"selection":false,
        "operation":{"kind":"fill","value":40000}});
    let t = text(red);
    let mut e = ptr::null_mut();
    let p = unsafe { dr_precision_doc_edit_plan(&float, t.as_ptr(), LIMIT, 0, &mut e) };
    if p.is_null() {
        assert!(!unsafe { error(e) }.is_empty());
    } else {
        unsafe { dr_string_free(p) };
        let out = unsafe {
            dr_precision_doc_edit(
                &float,
                t.as_ptr(),
                ptr::null(),
                0,
                LIMIT,
                0,
                ptr::null(),
                &mut e,
            )
        };
        assert!(out.is_null(), "a colour-plane edit at float must refuse");
        assert!(!unsafe { error(e) }.is_empty());
    }
}

unsafe fn owned_json(value: *mut c_char) -> Value {
    assert!(!value.is_null());
    let parsed = serde_json::from_slice(unsafe { CStr::from_ptr(value) }.to_bytes()).unwrap();
    unsafe { dr_string_free(value) };
    parsed
}

#[test]
fn capability_rows_admit_layer_mask_and_channel_at_32_bits() {
    let operation = CString::new("fill_stroke").unwrap();
    for (target, depth, supported) in [
        ("raster", 32, true),
        ("mask", 32, true),
        ("channel", 32, true),
        ("red", 32, true),
        ("alpha", 32, true),
        ("raster", 16, true),
        ("red", 16, true),
    ] {
        let target = CString::new(target).unwrap();
        let mut err = ptr::null_mut();
        let answer = unsafe {
            owned_json(dr_precision_capability(
                operation.as_ptr(),
                target.as_ptr(),
                depth,
                &mut err,
            ))
        };
        assert!(err.is_null());
        assert_eq!(answer["supported"], supported, "{target:?} at {depth}");
    }
    let table = unsafe { owned_json(dr_float_operators()) };
    assert!(table["admitted"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "fill_stroke"));
    let refused = table["refused"].as_array().unwrap();
    assert!(refused.iter().any(|row| row["reason"]
        .as_str()
        .unwrap()
        .starts_with(INVERTS_ABOUT_WHITE)));
}
