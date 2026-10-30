//! The 16-bit retouch brushes: the `retouch_brush` op of
//! `dr_precision_doc_edit` (Blur, Sharpen, Sponge),
//! `dr_precision_doc_history_brush` and `dr_precision_doc_smudge`, with their
//! plans. The oracles are naive per-pixel loops over f64 written here from the
//! documented formulas, on the layer's own u16 codes.
use crate::native_scalar as native;
use doctored_core::doc_lock::{LOCK_PIXELS, LOCK_TRANSPARENCY};
use doctored_core::ffi_precision_edit::*;
use doctored_core::ffi_retouch_brush::*;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use native::*;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

const LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];
const HARD: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;

fn samples(d: &PrecisionDocument) -> Vec<[u16; 4]> {
    d.layer_source(0).unwrap().to_rgba16().unwrap()
}

/// A `canvas` holding one raster layer of `size` at `offset`, with `locks`.
fn placed(
    canvas: (u32, u32),
    size: (u32, u32),
    offset: (i32, i32),
    pixels: Vec<[u16; 4]>,
    locks: u32,
) -> PrecisionDocument {
    let image =
        PrecisionImage::from_rgba16(size.0, size.1, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_parts(
        canvas.0,
        canvas.1,
        IccProfile::srgb(),
        Default::default(),
        Default::default(),
        vec![
            PrecisionDocumentLayer::raster("raw", image, offset, 1., true)
                .unwrap()
                .with_locks(locks),
        ],
    )
    .unwrap()
}

/// Colour structure in full 16-bit codes with mixed alpha: opaque, partial,
/// and transparent pixels that keep a (latent) colour.
fn plate(w: u32, h: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let value =
                |offset: u32| ((x * 2311 + y * 4099 + offset + (i % 3) * 1723) % 65536) as u16;
            let alpha = match i % 11 {
                3 => 0,
                7 => 23000 + (i % 100) as u16 * 211,
                _ => 65535,
            };
            [value(0), value(20000), value(41000), alpha]
        })
        .collect()
}

fn px(pixels: &[[u16; 4]], (w, h): (u32, u32), x: i64, y: i64) -> [f64; 4] {
    let cx = x.clamp(0, i64::from(w) - 1) as usize;
    let cy = y.clamp(0, i64::from(h) - 1) as usize;
    pixels[cy * w as usize + cx].map(|v| f64::from(v) / 65535.0)
}

fn taps(sigma: f64) -> Vec<f64> {
    let half = (3.0 * sigma).ceil() as i64;
    let raw: Vec<f64> = (-half..=half)
        .map(|k| (-0.5 * (k as f64 / sigma).powi(2)).exp())
        .collect();
    let sum: f64 = raw.iter().sum();
    raw.iter().map(|v| v / sum).collect()
}

/// The alpha-weighted Gaussian of the straight colour, as the full
/// two-dimensional sum with clamped reads.
fn gaussian(pixels: &[[u16; 4]], size: (u32, u32), sigma: f64, x: u32, y: u32) -> [f64; 3] {
    let k = taps(sigma);
    let half = (k.len() / 2) as i64;
    let (mut acc, mut total) = ([0.0; 3], 0.0);
    for (j, ky) in k.iter().enumerate() {
        for (i, kx) in k.iter().enumerate() {
            let p = px(
                pixels,
                size,
                i64::from(x) + i as i64 - half,
                i64::from(y) + j as i64 - half,
            );
            let weight = kx * ky * p[3];
            for c in 0..3 {
                acc[c] += weight * p[c];
            }
            total += weight;
        }
    }
    if total > 0.0 {
        acc.map(|v| v / total)
    } else {
        let p = px(pixels, size, i64::from(x), i64::from(y));
        [p[0], p[1], p[2]]
    }
}

fn target(pixels: &[[u16; 4]], size: (u32, u32), kind: &str, x: u32, y: u32) -> [f64; 3] {
    let p = px(pixels, size, i64::from(x), i64::from(y));
    let luma = LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2];
    match kind {
        "blur" => gaussian(pixels, size, 2.0, x, y),
        "sharpen" => {
            let low = gaussian(pixels, size, 1.0, x, y);
            std::array::from_fn(|c| (p[c] + (p[c] - low[c])).clamp(0.0, 1.0))
        }
        "saturate" => std::array::from_fn(|c| (luma + 2.0 * (p[c] - luma)).clamp(0.0, 1.0)),
        "desaturate" => [luma; 3],
        _ => panic!("bad kind {kind}"),
    }
}

fn request(kind: &str, strength: f64, selection: bool) -> Value {
    json!({"op":"retouch_brush","layer":0,"kind":kind,"strength":strength,"selection":selection})
}

/// The edit's error message (the plan must succeed first).
fn edit_error(d: &PrecisionDocument, v: Value, input: &[u16]) -> String {
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
    assert!(out.is_null());
    unsafe { error(e) }
}

fn plan_error(d: &PrecisionDocument, v: Value) -> String {
    let t = text(v);
    let mut e = ptr::null_mut();
    let p = unsafe { dr_precision_doc_edit_plan(d, t.as_ptr(), LIMIT, 0, &mut e) };
    assert!(p.is_null());
    unsafe { error(e) }
}

// ------------------------------------------------- blur, sharpen, sponge --

#[test]
fn each_kind_matches_its_oracle_with_coverage_and_selection_multiplied() {
    let size = (21_u32, 16_u32);
    let n = (size.0 * size.1) as usize;
    let pixels = plate(size.0, size.1);
    let d = doc(size.0, size.1, pixels.clone());
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
    for kind in ["blur", "sharpen", "saturate", "desaturate"] {
        let out = samples(&edit(&d, request(kind, 0.8, true), &input).expect(kind));
        for i in 0..n {
            let (x, y) = (i as u32 % size.0, i as u32 / size.0);
            let coverage = f64::from(stroke[i]) / 65535.0 * f64::from(chosen[i]) / 65535.0;
            assert_eq!(out[i][3], pixels[i][3], "{kind}: alpha at {i}");
            if coverage == 0.0 || pixels[i][3] == 0 {
                assert_eq!(out[i], pixels[i], "{kind}: untouched {i}");
                continue;
            }
            let wanted = target(&pixels, size, kind, x, y);
            for c in 0..3 {
                let v = f64::from(pixels[i][c]) / 65535.0;
                let expect = (v + coverage * 0.8 * (wanted[c] - v)) * 65535.0;
                // The kernel mixes in f32: a few codes of 65535.
                assert!(
                    (f64::from(out[i][c]) - expect).abs() <= 3.0,
                    "{kind} pixel {i} channel {c}: {} vs {expect}",
                    out[i][c]
                );
            }
        }
    }
}

#[test]
fn the_coverage_maps_through_the_layer_offset_and_the_plan_names_its_input() {
    // A 9×7 layer at (4, −2) over a 12×8 canvas.
    let pixels = plate(9, 7);
    let d = placed((12, 8), (9, 7), (4, -2), pixels.clone(), 0);
    let p = plan(&d, request("desaturate", 1.0, false));
    assert_eq!(p["input_samples"], 96);
    assert_eq!(p["input_width"], 12);
    assert_eq!(p["input_height"], 8);
    assert_eq!(p["input_planes"], 1);
    assert_eq!(plan(&d, request("blur", 1.0, true))["input_samples"], 192);
    assert_eq!(plan(&d, request("blur", 1.0, true))["input_planes"], 2);
    assert!(p["scratch_bytes"].as_u64().unwrap() > 0);
    let cover = |cx: i64, cy: i64| cx >= 6 && cy < 3;
    let input: Vec<u16> = (0..96)
        .map(|i| if cover(i % 12, i / 12) { 65535 } else { 0 })
        .collect();
    let out = edit(&d, request("desaturate", 1.0, false), &input).expect("a change");
    assert_eq!(out.layer(0).unwrap().offset(), (4, -2));
    let got = samples(&out);
    assert_eq!(got.len(), 63, "the layer does not grow");
    for (i, (after, before)) in got.iter().zip(&pixels).enumerate() {
        // Layer pixel (lx, ly) sits at canvas (lx + 4, ly − 2).
        let (cx, cy) = (i as i64 % 9 + 4, i as i64 / 9 - 2);
        let on = (0..12).contains(&cx) && (0..8).contains(&cy) && cover(cx, cy);
        if !on || before[3] == 0 {
            assert_eq!(after, before, "pixel {i}");
        } else {
            let p = before.map(|v| f64::from(v) / 65535.0);
            let luma = (LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2]) * 65535.0;
            for value in after.iter().take(3) {
                assert!((f64::from(*value) - luma).abs() <= 3.0, "pixel {i}");
            }
            assert_eq!(after[3], before[3]);
        }
    }
}

#[test]
fn the_retouch_brush_op_refuses_by_name_and_answers_null_for_nothing_to_do() {
    let pixels = plate(8, 6);
    let d = doc(8, 6, pixels.clone());
    let full = vec![65535_u16; 48];
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
    assert!(plan_error(
        &d,
        json!({"op":"retouch_brush","layer":0,"kind":"smear","strength":1.0})
    )
    .starts_with("invalid retouch brush request"));
    assert!(plan_error(
        &d,
        json!({"op":"retouch_brush","layer":0,"kind":"blur","strength":1.0,"range":1})
    )
    .starts_with("invalid retouch brush request"));
    assert_eq!(
        edit_error(&d, request("blur", 1.0, false), &full[..47]),
        "high-depth input sample length does not match the core plan"
    );
    // Nothing to do.
    assert!(edit(&d, request("blur", 0.0, false), &full).is_none());
    assert!(edit(&d, request("blur", 1.0, false), &[0; 48]).is_none());
    let flat = doc(8, 6, vec![[30000, 30000, 30000, 65535]; 48]);
    for kind in ["blur", "sharpen", "saturate", "desaturate"] {
        assert!(
            edit(&flat, request(kind, 1.0, false), &full).is_none(),
            "{kind}"
        );
    }
    // A pixel lock refuses by name; a transparency lock does not stop a
    // brush that never writes alpha.
    let locked = placed((8, 6), (8, 6), (0, 0), pixels.clone(), LOCK_PIXELS);
    assert!(edit_error(&locked, request("desaturate", 1.0, false), &full).contains("lock"));
    let frozen = placed((8, 6), (8, 6), (0, 0), pixels, LOCK_TRANSPARENCY);
    assert!(edit(&frozen, request("desaturate", 1.0, false), &full).is_some());
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
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { doctored_core::ffi::dr_string_free(p) };
    Ok(serde_json::from_str(&s).unwrap())
}

fn history(
    d: &PrecisionDocument,
    source: &PrecisionDocument,
    opacity: f64,
    input: &[u16],
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
            ptr::null(),
            &mut e,
        )
    };
    if !e.is_null() {
        assert!(out.is_null());
        return Err(unsafe { error(e) });
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

#[test]
fn the_history_brush_restores_exactly_and_mixes_premultiplied() {
    let size = (17_u32, 12_u32);
    let n = (size.0 * size.1) as usize;
    let old = plate(size.0, size.1);
    let earlier = doc(size.0, size.1, old.clone());
    let new: Vec<[u16; 4]> = (0..n as u32)
        .map(|i| {
            [
                (i * 301) as u16,
                51000,
                (i * 97) as u16,
                if i % 6 == 0 {
                    0
                } else {
                    30000 + (i * 151) as u16 % 30000
                },
            ]
        })
        .collect();
    let now = doc(size.0, size.1, new.clone());
    assert_eq!(
        history_plan(&now, &earlier, false).unwrap()["input_samples"],
        n
    );
    assert_eq!(
        history_plan(&now, &earlier, true).unwrap()["input_samples"],
        2 * n
    );
    // Opacity 1 under full coverage: the earlier pixels, exactly.
    let inside = |i: usize| (3..14).contains(&(i % 17)) && (2..9).contains(&(i / 17));
    let input: Vec<u16> = (0..n).map(|i| if inside(i) { 65535 } else { 0 }).collect();
    let got = samples(
        &history(&now, &earlier, 1.0, &input)
            .unwrap()
            .expect("a change"),
    );
    for i in 0..n {
        let wanted = if !inside(i) {
            new[i]
        } else if old[i][3] == 0 {
            if new[i][3] == 0 {
                new[i]
            } else {
                [0; 4]
            }
        } else {
            old[i]
        };
        assert_eq!(got[i], wanted, "pixel {i}");
    }
    // Partial coverage times a selection plane times opacity.
    let stroke: Vec<u16> = (0..n).map(|i| ((i * 5231) % 65536) as u16).collect();
    let chosen: Vec<u16> = (0..n).map(|i| if i % 4 == 0 { 0 } else { 50000 }).collect();
    let input: Vec<u16> = stroke.iter().chain(&chosen).copied().collect();
    let got = samples(
        &history(&now, &earlier, 0.6, &input)
            .unwrap()
            .expect("a change"),
    );
    for i in 0..n {
        let t = f64::from(stroke[i]) / 65535.0 * f64::from(chosen[i]) / 65535.0 * 0.6;
        if t == 0.0 {
            assert_eq!(got[i], new[i], "uncovered {i}");
            continue;
        }
        let (a, b) = (
            new[i].map(|v| f64::from(v) / 65535.0),
            old[i].map(|v| f64::from(v) / 65535.0),
        );
        let alpha = (1.0 - t) * a[3] + t * b[3];
        if (alpha * 65535.0).round() == 0.0 {
            assert!(got[i] == new[i] || got[i] == [0; 4], "pixel {i}");
            continue;
        }
        assert_eq!(got[i][3], q(alpha * 65535.0), "alpha {i}");
        for c in 0..3 {
            let expect = ((1.0 - t) * a[c] * a[3] + t * b[c] * b[3]) / alpha * 65535.0;
            assert!(
                (f64::from(got[i][c]) - expect).abs() <= 1.0,
                "pixel {i} channel {c}: {} vs {expect}",
                got[i][c]
            );
        }
    }
    // The same state painted onto itself changes nothing.
    assert!(history(&earlier, &earlier, 1.0, &vec![65535; n])
        .unwrap()
        .is_none());
    assert!(history(&now, &earlier, 0.0, &vec![65535; n])
        .unwrap()
        .is_none());
}

#[test]
fn the_history_brush_reads_the_source_at_the_same_canvas_position_and_keeps_locked_alpha() {
    // Now: 10×8 at (2, 1). Earlier: 6×4 at (5, 3). Canvas 14×10.
    let old = plate(6, 4);
    let earlier = placed((14, 10), (6, 4), (5, 3), old.clone(), 0);
    let new = vec![[60000_u16, 100, 100, 40000]; 80];
    let full = vec![65535_u16; 140];
    let now = placed((14, 10), (10, 8), (2, 1), new.clone(), 0);
    let out = history(&now, &earlier, 1.0, &full)
        .unwrap()
        .expect("a change");
    assert_eq!(out.layer(0).unwrap().offset(), (2, 1));
    let got = samples(&out);
    assert_eq!(got.len(), 80, "no growth");
    let source_at = |i: usize| {
        // Canvas (lx + 2, ly + 1) is source pixel (lx − 3, ly − 2).
        let (sx, sy) = (i as i64 % 10 - 3, i as i64 / 10 - 2);
        ((0..6).contains(&sx) && (0..4).contains(&sy)).then(|| old[(sy * 6 + sx) as usize])
    };
    for (i, after) in got.iter().enumerate() {
        let wanted = match source_at(i) {
            Some(p) if p[3] > 0 => p,
            // Transparent in the source, or the source layer holds nothing.
            _ => [0; 4],
        };
        assert_eq!(*after, wanted, "pixel {i}");
    }
    let frozen = placed((14, 10), (10, 8), (2, 1), new.clone(), LOCK_TRANSPARENCY);
    let got = samples(
        &history(&frozen, &earlier, 1.0, &full)
            .unwrap()
            .expect("a change"),
    );
    for (i, after) in got.iter().enumerate() {
        assert_eq!(after[3], 40000, "alpha kept at {i}");
        match source_at(i) {
            Some(p) if p[3] > 0 => assert_eq!(after[..3], p[..3], "colour {i}"),
            _ => assert_eq!(*after, new[i], "no source colour at {i}"),
        }
    }
}

#[test]
fn the_history_brush_refuses_by_name_and_tolerates_null() {
    let now = doc(8, 6, vec![[9000, 100, 100, 65535]; 48]);
    let earlier = doc(8, 6, plate(8, 6));
    let full = vec![65535_u16; 48];
    assert_eq!(
        history_plan(&now, &doc(9, 6, plate(9, 6)), false)
            .err()
            .unwrap(),
        "history brush source has a different canvas size"
    );
    assert_eq!(
        history(&now, &earlier, 1.5, &full).err().unwrap(),
        "history brush opacity is invalid"
    );
    assert_eq!(
        history(&now, &earlier, 1.0, &full[..40]).err().unwrap(),
        "high-depth input sample length does not match the core plan"
    );
    let locked = placed((8, 6), (8, 6), (0, 0), plate(8, 6), LOCK_PIXELS);
    assert!(history(&locked, &now, 1.0, &full)
        .err()
        .unwrap()
        .contains("lock"));
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
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { doctored_core::ffi::dr_string_free(p) };
    Ok(serde_json::from_str(&s).unwrap())
}

fn smudge(
    d: &PrecisionDocument,
    dabs: &[[f64; 4]],
    strength: f64,
    selection: &[u16],
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
            ptr::null(),
            &mut e,
        )
    };
    if !e.is_null() {
        assert!(out.is_null());
        return Err(unsafe { error(e) });
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
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
/// bilinear carry. Returns unit colours.
fn smudge_oracle(
    pixels: &[[u16; 4]],
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
        .map(|p| [0, 1, 2].map(|c| f64::from(p[c]) / 65535.0))
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

fn opaque(w: u32, h: u32) -> Vec<[u16; 4]> {
    plate(w, h)
        .into_iter()
        .map(|p| [p[0], p[1], p[2], 65535])
        .collect()
}

#[test]
fn smudge_matches_the_sub_stepped_oracle_and_a_selection_confines_it() {
    let (w, h) = (56_u32, 44_u32);
    let n = (w * h) as usize;
    let pixels = opaque(w, h);
    let d = doc(w, h, pixels.clone());
    let dabs = [
        [17.3, 19.7, 15.0, 1.0],
        [26.9, 23.2, 19.0, 0.8],
        [30.0, 14.5, 11.0, 1.0],
    ];
    let p = smudge_plan(&d, &dabs, false).unwrap();
    assert_eq!(p["input_samples"], 0);
    assert!(p["scratch_bytes"].as_u64().unwrap() > 0);
    assert_eq!(smudge_plan(&d, &dabs, true).unwrap()["input_samples"], n);
    let check = |got: &[[u16; 4]], expect: &[[f64; 3]], what: &str| {
        for i in 0..n {
            assert_eq!(got[i][3], 65535, "{what}: alpha {i}");
            for c in 0..3 {
                // The field is f32: a few codes of 65535 over the walk.
                assert!(
                    (f64::from(got[i][c]) - expect[i][c] * 65535.0).abs() <= 4.0,
                    "{what}: pixel {i} channel {c}: {} vs {}",
                    got[i][c],
                    expect[i][c] * 65535.0
                );
            }
        }
    };
    let out = smudge(&d, &dabs, 0.7, &[]).unwrap().expect("a change");
    let got = samples(&out);
    assert_ne!(got, pixels);
    check(
        &got,
        &smudge_oracle(&pixels, (w as usize, h as usize), &dabs, 0.7, |_, _| 1.0),
        "whole",
    );
    // Half coverage on the left half of the canvas, none on the right.
    let chosen: Vec<u16> = (0..n)
        .map(|i| if i % 56 < 24 { 32768 } else { 0 })
        .collect();
    let got = samples(&smudge(&d, &dabs, 0.7, &chosen).unwrap().expect("a change"));
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
            assert_eq!(got[i], pixels[i], "outside the selection {i}");
        }
    }
}

#[test]
fn smudge_does_not_depend_on_the_dab_spacing_and_honours_the_layer_offset() {
    let pixels = opaque(40, 30);
    let d = placed((50, 40), (40, 30), (6, 4), pixels.clone(), 0);
    let far = [[22.5, 18.5, 14.0, 1.0], [34.5, 18.5, 14.0, 1.0]];
    let near: Vec<[f64; 4]> = (0..13)
        .map(|k| [22.5 + f64::from(k), 18.5, 14.0, 1.0])
        .collect();
    let a = smudge(&d, &far, 0.5, &[]).unwrap().expect("a change");
    let b = smudge(&d, &near, 0.5, &[]).unwrap().expect("a change");
    assert_eq!(samples(&a), samples(&b));
    assert_eq!(a.layer(0).unwrap().offset(), (6, 4));
    // The dabs are canvas coordinates: the oracle runs in the layer's.
    let local: Vec<[f64; 4]> = far
        .iter()
        .map(|d| [d[0] - 6.0, d[1] - 4.0, d[2], d[3]])
        .collect();
    let expect = smudge_oracle(&pixels, (40, 30), &local, 0.5, |_, _| 1.0);
    for (i, got) in samples(&a).iter().enumerate() {
        for c in 0..3 {
            assert!(
                (f64::from(got[c]) - expect[i][c] * 65535.0).abs() <= 4.0,
                "pixel {i} channel {c}"
            );
        }
    }
}

#[test]
fn smudge_keeps_alpha_under_a_transparency_lock_and_refuses_a_pixel_lock() {
    let pixels = plate(48, 36);
    let dabs = [[16.5, 18.5, 16.0, 1.0], [30.5, 18.5, 16.0, 1.0]];
    let frozen = placed(
        (48, 36),
        (48, 36),
        (0, 0),
        pixels.clone(),
        LOCK_TRANSPARENCY,
    );
    let got = samples(&smudge(&frozen, &dabs, 0.8, &[]).unwrap().expect("a change"));
    assert_ne!(got, pixels);
    for (after, before) in got.iter().zip(&pixels) {
        assert_eq!(after[3], before[3], "alpha kept");
        if before[3] == 0 {
            assert_eq!(after, before);
        }
    }
    let locked = placed((48, 36), (48, 36), (0, 0), pixels, LOCK_PIXELS);
    assert!(smudge(&locked, &dabs, 0.8, &[])
        .err()
        .unwrap()
        .contains("lock"));
}

/// The byte rule at u16 (`retouch_brush.rs`'s twin): one sub-step at strength
/// 0.3 from alpha-1 codes deposits at most 0.3 of a code on the transparent
/// side, which stays transparent and keeps its latent colour, so nothing
/// changes.
#[test]
fn smudge_keeps_the_latent_colour_of_a_pixel_that_stays_transparent() {
    let pixels: Vec<[u16; 4]> = (0..40 * 30)
        .map(|i| {
            if i % 40 < 20 {
                [51400, 25700, 12850, 1]
            } else {
                [2570, 5140, 7710, 0]
            }
        })
        .collect();
    let d = placed((40, 30), (40, 30), (0, 0), pixels, 0);
    let dabs = [[19.5, 15.5, 12.0, 1.0], [20.5, 15.5, 12.0, 1.0]];
    assert!(
        smudge(&d, &dabs, 0.3, &[]).unwrap().is_none(),
        "no code changes, so no new document"
    );
}

#[test]
fn smudge_refuses_by_name_answers_null_for_nothing_to_do_and_tolerates_null() {
    let d = doc(40, 30, plate(40, 30));
    let dabs = [[10.5, 10.5, 12.0, 1.0], [20.5, 14.5, 12.0, 1.0]];
    assert!(
        smudge(&d, &dabs[..1], 0.5, &[]).unwrap().is_none(),
        "one dab"
    );
    assert!(smudge(&d, &[], 0.5, &[]).unwrap().is_none(), "no dabs");
    assert!(smudge(&d, &dabs, 0.0, &[]).unwrap().is_none(), "strength 0");
    let flat = doc(40, 30, vec![[7000, 7000, 7000, 65535]; 1200]);
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
    // A hard size-512 tip charges about 555,000 units a sub-step. Swept
    // 600 px once (600 sub-steps, 3.3·10⁸) it is between Smudge's own
    // 2.56·10⁸ bound and the brush's 2·10⁹, and the plan says so in
    // Smudge's sentence before anything is allocated; swept 300 px
    // (1.7·10⁸) it plans.
    let wide = doc(1200, 600, vec![[1, 2, 3, 65535]; 720_000]);
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
        assert!(dr_precision_doc_smudge_plan(
            &d,
            0,
            brush.as_ptr(),
            ptr::null(),
            2,
            false,
            false,
            LIMIT,
            0,
            &mut e
        )
        .is_null());
        assert_eq!(
            error(e),
            "native brush requires a valid array of at most 50000 dabs; use a shorter stroke or wider spacing"
        );
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
            ptr::null(),
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
        assert_eq!(error(e), "native brush request is NULL");
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
