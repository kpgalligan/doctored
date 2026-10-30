//! Edit ▸ Fade (v1): `dr_doc_fade` on the byte document and
//! `dr_precision_doc_fade` / `dr_precision_doc_fade_plan` at 16 bits and in a
//! 32-bit float document, black-box through the C exports.
//!
//! The oracle is written here from the documented rule, in binary64, over
//! the W3C reference compositor `common::ref_composite` for the modes and the
//! premultiplied lerp for the two interpolations: E is `after` for Normal,
//! `before` where `after` is transparent, else `ref_composite(before, after, 1,
//! mode)`; E′ = lerp(after → E, weight); out = lerp(before → E′, opacity).
//! Where both sides are transparent the result's pixel stays verbatim.

use std::ffi::c_int;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_lock::LOCK_TRANSPARENCY;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_fade::{dr_doc_fade, dr_precision_doc_fade, dr_precision_doc_fade_plan};
use doctored_core::ffi_group::dr_doc_with_layer_locks;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use image::RgbaImage;
use serde_json::Value;

use crate::common::{
    ref_composite, BLEND_MULTIPLY, BLEND_NORMAL, BLEND_PASS_THROUGH, BLEND_SCREEN,
};
use crate::float_fixture::float_doc;
use crate::native_scalar as native;
use native::{error, LIMIT};

// ------------------------------------------------------------- the oracle --

/// The premultiplied lerp from `from` toward `to` by `t`.
fn lerp(from: [f64; 4], to: [f64; 4], t: f64) -> [f64; 4] {
    if t <= 0.0 {
        return from;
    }
    if t >= 1.0 {
        return to;
    }
    let a = from[3] + (to[3] - from[3]) * t;
    if a <= 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    for c in 0..3 {
        out[c] = (from[c] * from[3] + (to[c] * to[3] - from[c] * from[3]) * t) / a;
    }
    out[3] = a;
    out
}

fn expected(before: [f64; 4], after: [f64; 4], mode: c_int, weight: f64, opacity: f64) -> [f64; 4] {
    if before[3] <= 0.0 && after[3] <= 0.0 {
        return after;
    }
    let target = if mode == BLEND_NORMAL {
        after
    } else if after[3] <= 0.0 {
        before
    } else {
        let f = |p: [f64; 4]| p.map(|v| v as f32);
        ref_composite(f(before), f(after), 1.0, mode).map(f64::from)
    };
    lerp(before, lerp(after, target, weight), opacity)
}

fn unit8(p: [u8; 4]) -> [f64; 4] {
    p.map(|v| f64::from(v) / 255.0)
}

fn unit16(p: [u16; 4]) -> [f64; 4] {
    p.map(|v| f64::from(v) / 65535.0)
}

// --------------------------------------------------------------- fixtures --

/// Colour structure with mixed alpha: opaque, partial, and transparent
/// pixels that keep a latent colour.
fn plate8(w: u32, h: u32, seed: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let i = y * w + x;
        let v = |o: u32| ((x * 37 + y * 91 + o + seed * 13) % 256) as u8;
        let a = match (i + seed) % 7 {
            2 => 0,
            5 => 120,
            _ => 255,
        };
        image::Rgba([v(0), v(80), v(170), a])
    })
}

fn handle(doc: &DrDocument) -> *mut DrDocument {
    Box::into_raw(Box::new(doc.clone()))
}

fn layer8(doc: &DrDocument) -> Vec<[u8; 4]> {
    let raster = &doc.layers[0].pixels;
    let (w, h) = raster.dimensions();
    raster.read_rect(0, 0, w, h).expect("layer pixels")
}

fn fade8(
    after: &DrDocument,
    before: &DrDocument,
    mode: c_int,
    opacity: f32,
    coverage: Option<&[u8]>,
    changed_only: bool,
) -> Option<DrDocument> {
    let (d, b) = (handle(after), handle(before));
    let (cp, cl) = coverage.map_or((ptr::null(), 0), |c| (c.as_ptr(), c.len()));
    let out = unsafe { dr_doc_fade(d, b, 0, mode, opacity, cp, cl, changed_only) };
    unsafe {
        dr_doc_free(d);
        dr_doc_free(b);
    }
    (!out.is_null()).then(|| *unsafe { Box::from_raw(out) })
}

#[allow(clippy::too_many_arguments)]
fn fade_typed(
    after: &PrecisionDocument,
    before: &PrecisionDocument,
    mode: c_int,
    opacity: f64,
    coverage: &[u16],
    changed_only: bool,
) -> Result<Option<PrecisionDocument>, String> {
    let mut e = ptr::null_mut();
    let (cp, cl) = if coverage.is_empty() {
        (ptr::null(), 0)
    } else {
        (coverage.as_ptr(), coverage.len())
    };
    let out = unsafe {
        dr_precision_doc_fade(
            after,
            0,
            before,
            mode,
            opacity,
            cp,
            cl,
            changed_only,
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

/// The outcome as comparable text: "none", "changed" or the error.
fn outcome(result: Result<Option<PrecisionDocument>, String>) -> String {
    match result {
        Ok(None) => "none".into(),
        Ok(Some(_)) => "changed".into(),
        Err(e) => e,
    }
}

fn plan(after: &PrecisionDocument, before: &PrecisionDocument, coverage: bool) -> Value {
    let mut e = ptr::null_mut();
    let p = unsafe { dr_precision_doc_fade_plan(after, 0, before, coverage, LIMIT, 0, &mut e) };
    assert!(!p.is_null(), "{}", unsafe { error(e) });
    let text = unsafe { std::ffi::CStr::from_ptr(p) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(p) };
    serde_json::from_str(&text).unwrap()
}

fn samples16(d: &PrecisionDocument) -> Vec<[u16; 4]> {
    d.layer_source(0).unwrap().to_rgba16().unwrap()
}

fn samples32(d: &PrecisionDocument) -> Vec<[f32; 4]> {
    d.layer_source(0).unwrap().to_rgba32().unwrap()
}

fn plate16(w: u32, h: u32, seed: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let v = |o: u32| ((x * 2311 + y * 4099 + o + seed * 977) % 65536) as u16;
            let a = match (i + seed) % 7 {
                2 => 0,
                5 => 30000,
                _ => 65535,
            };
            [v(0), v(20000), v(41000), a]
        })
        .collect()
}

// ------------------------------------------------------------------ 8 bits --

#[test]
fn byte_fade_matches_the_reference_in_normal_and_multiply() {
    let (w, h) = (9, 7);
    let before = DrDocument::from_pixels(plate8(w, h, 0));
    let after = DrDocument::from_pixels(plate8(w, h, 3));
    let (b, a) = (layer8(&before), layer8(&after));
    for (mode, opacity) in [
        (BLEND_NORMAL, 0.5f32),
        (BLEND_NORMAL, 0.25),
        (BLEND_MULTIPLY, 1.0),
        (BLEND_MULTIPLY, 0.5),
    ] {
        let out = fade8(&after, &before, mode, opacity, None, false).expect("a change");
        let got = layer8(&out);
        for i in 0..got.len() {
            let want = expected(unit8(b[i]), unit8(a[i]), mode, 1.0, f64::from(opacity));
            if b[i][3] == 0 && a[i][3] == 0 {
                assert_eq!(got[i], a[i], "transparent pixel {i} stays the result's");
                continue;
            }
            for c in 0..4 {
                let code = (want[c].clamp(0.0, 1.0) * 255.0).round();
                assert!(
                    (f64::from(got[i][c]) - code).abs() <= 1.0,
                    "mode {mode} opacity {opacity} pixel {i} channel {c}: {} vs {code}",
                    got[i][c]
                );
            }
        }
    }
    // Normal at 1 is the result itself: nothing changes.
    assert!(fade8(&after, &before, BLEND_NORMAL, 1.0, None, false).is_none());
    // Opacity 0 is the state before, wherever the result's layer holds it.
    let back = fade8(&after, &before, BLEND_NORMAL, 0.0, None, false).expect("a change");
    for (i, (g, w)) in layer8(&back).iter().zip(&b).enumerate() {
        if w[3] == 0 && a[i][3] == 0 {
            continue;
        }
        assert_eq!(g, w, "pixel {i} at opacity 0");
    }
}

#[test]
fn byte_fade_weights_by_coverage_or_by_changed_pixels() {
    let (w, h) = (8, 4);
    let before = DrDocument::from_pixels(RgbaImage::from_pixel(
        w,
        h,
        image::Rgba([200, 100, 50, 255]),
    ));
    // The step changed only the left half.
    let after = DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, _| {
        if x < 4 {
            image::Rgba([40, 220, 128, 255])
        } else {
            image::Rgba([200, 100, 50, 255])
        }
    }));
    let (b, a) = (layer8(&before), layer8(&after));
    // Coverage: the top row fully, the rest not at all.
    let coverage: Vec<u8> = (0..w * h).map(|i| if i < w { 255 } else { 0 }).collect();
    let out =
        fade8(&after, &before, BLEND_MULTIPLY, 1.0, Some(&coverage), false).expect("a change");
    for (i, got) in layer8(&out).iter().enumerate() {
        let weight = f64::from(coverage[i]) / 255.0;
        let want = expected(unit8(b[i]), unit8(a[i]), BLEND_MULTIPLY, weight, 1.0);
        for c in 0..4 {
            let code = (want[c] * 255.0).round();
            assert!(
                (f64::from(got[c]) - code).abs() <= 1.0,
                "coverage pixel {i} channel {c}"
            );
        }
        if coverage[i] == 0 {
            assert_eq!(*got, a[i], "weight 0 leaves the result at opacity 1");
        }
    }
    // A stroke: Multiply only where the step changed the pixel.
    let out = fade8(&after, &before, BLEND_MULTIPLY, 1.0, None, true).expect("a change");
    for (i, got) in layer8(&out).iter().enumerate() {
        if a[i] == b[i] {
            assert_eq!(*got, a[i], "an untouched pixel {i} stays");
        } else {
            let want = expected(unit8(b[i]), unit8(a[i]), BLEND_MULTIPLY, 1.0, 1.0);
            for c in 0..4 {
                assert!((f64::from(got[c]) - (want[c] * 255.0).round()).abs() <= 1.0);
            }
        }
    }
    // The wrong coverage length refuses.
    assert!(fade8(
        &after,
        &before,
        BLEND_MULTIPLY,
        1.0,
        Some(&coverage[1..]),
        false
    )
    .is_none());
}

#[test]
fn byte_fade_over_transparent_pixels_keeps_the_result_and_no_ops_are_null() {
    let (w, h) = (4, 3);
    // Every pixel transparent, carrying colour.
    let clear = |seed: u8| {
        DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([seed.wrapping_add(x as u8 * 40), 90 + y as u8, 200, 0])
        }))
    };
    let (before, after) = (clear(10), clear(70));
    for mode in [BLEND_MULTIPLY, BLEND_NORMAL] {
        assert!(
            fade8(&after, &before, mode, 0.5, None, false).is_none(),
            "an all-transparent layer has nothing to fade (mode {mode})"
        );
    }
    // A transparent pixel carrying colour inside an otherwise changed layer.
    let before = DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, _| {
        image::Rgba([10, 20, 30, if x == 1 { 0 } else { 255 }])
    }));
    let after = DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, _| {
        image::Rgba([250, 120, 60, if x == 1 { 0 } else { 255 }])
    }));
    let a = layer8(&after);
    for mode in [BLEND_MULTIPLY, BLEND_NORMAL] {
        let out = fade8(&after, &before, mode, 0.5, None, false).expect("a change");
        for (i, got) in layer8(&out).iter().enumerate() {
            if i as u32 % w == 1 {
                assert_eq!(
                    *got, a[i],
                    "mode {mode}: the transparent pixel stays the result's"
                );
            }
        }
    }
    // The same document on both sides, Normal: unchanged.
    assert!(fade8(&after, &after, BLEND_NORMAL, 0.5, None, false).is_none());
}

#[test]
fn byte_fade_refusals_and_the_transparency_lock() {
    let before = DrDocument::from_pixels(plate8(5, 4, 1));
    let after = DrDocument::from_pixels(plate8(5, 4, 4));
    assert!(fade8(&after, &before, BLEND_PASS_THROUGH, 0.5, None, false).is_none());
    assert!(fade8(&after, &before, 99, 0.5, None, false).is_none());
    assert!(fade8(&after, &before, BLEND_NORMAL, 1.5, None, false).is_none());
    assert!(fade8(&after, &before, BLEND_NORMAL, f32::NAN, None, false).is_none());
    let other = DrDocument::from_pixels(plate8(6, 4, 1));
    assert!(fade8(&after, &other, BLEND_NORMAL, 0.5, None, false).is_none());
    // Index out of range.
    let (d, b) = (handle(&after), handle(&before));
    assert!(unsafe { dr_doc_fade(d, b, 3, BLEND_NORMAL, 0.5, ptr::null(), 0, false) }.is_null());
    // Under a transparency lock every pixel keeps the result's alpha.
    let locked = unsafe { dr_doc_with_layer_locks(d, 0, LOCK_TRANSPARENCY) };
    assert!(!locked.is_null());
    let out = unsafe { dr_doc_fade(locked, b, 0, BLEND_NORMAL, 0.5, ptr::null(), 0, false) };
    assert!(!out.is_null());
    let faded = *unsafe { Box::from_raw(out) };
    for (got, want) in layer8(&faded).iter().zip(layer8(&after)) {
        assert_eq!(got[3], want[3], "alpha is frozen");
    }
    unsafe {
        dr_doc_free(d);
        dr_doc_free(b);
        dr_doc_free(locked);
    }
}

// ----------------------------------------------------------------- 16 bits --

#[test]
fn u16_fade_matches_the_reference_and_plans_like_the_history_brush() {
    let (w, h) = (9, 6);
    let before = native::doc(w, h, plate16(w, h, 0));
    let after = native::doc(w, h, plate16(w, h, 2));
    let (b, a) = (samples16(&before), samples16(&after));
    let n = (w * h) as usize;
    let coverage: Vec<u16> = (0..n).map(|i| ((i * 4099) % 65536) as u16).collect();
    for (mode, opacity, weighted) in [
        (BLEND_NORMAL, 0.5, false),
        (BLEND_MULTIPLY, 1.0, false),
        (BLEND_MULTIPLY, 0.5, true),
    ] {
        let cover: &[u16] = if weighted { &coverage } else { &[] };
        let out = fade_typed(&after, &before, mode, opacity, cover, false)
            .unwrap()
            .expect("a change");
        let got = samples16(&out);
        for i in 0..n {
            if b[i][3] == 0 && a[i][3] == 0 {
                assert_eq!(
                    got[i], a[i],
                    "transparent pixel {i} stays the result's, bit for bit"
                );
                continue;
            }
            let weight = if weighted {
                f64::from(coverage[i]) / 65535.0
            } else {
                1.0
            };
            let want = expected(unit16(b[i]), unit16(a[i]), mode, weight, opacity);
            for c in 0..4 {
                let code = (want[c].clamp(0.0, 1.0) * 65535.0).round();
                assert!(
                    (f64::from(got[i][c]) - code).abs() <= 2.0,
                    "mode {mode} pixel {i} channel {c}: {} vs {code}",
                    got[i][c]
                );
            }
        }
    }
    assert_eq!(
        outcome(fade_typed(&after, &before, BLEND_NORMAL, 1.0, &[], false)),
        "none"
    );
    assert_eq!(plan(&after, &before, true)["input_samples"], n as u64);
    assert_eq!(plan(&after, &before, false)["input_samples"], 0);
}

#[test]
fn u16_fade_over_transparent_pixels_and_refusals_by_name() {
    let (w, h) = (4, 3);
    let clear = |seed: u16| -> Vec<[u16; 4]> {
        (0..w * h)
            .map(|i| [seed + i as u16 * 300, 40000, 1234, 0])
            .collect()
    };
    let (before, after) = (
        native::doc(w, h, clear(100)),
        native::doc(w, h, clear(9000)),
    );
    for mode in [BLEND_MULTIPLY, BLEND_NORMAL] {
        assert_eq!(
            outcome(fade_typed(&after, &before, mode, 0.5, &[], false)),
            "none"
        );
    }
    let mixed = |c: u16| -> Vec<[u16; 4]> {
        (0..w * h)
            .map(|i| [c, c / 2, 777, if i % 4 == 1 { 0 } else { 65535 }])
            .collect()
    };
    let (before, after) = (
        native::doc(w, h, mixed(5000)),
        native::doc(w, h, mixed(60000)),
    );
    let a = samples16(&after);
    for mode in [BLEND_MULTIPLY, BLEND_NORMAL] {
        let out = fade_typed(&after, &before, mode, 0.5, &[], false)
            .unwrap()
            .expect("a change");
        for (i, got) in samples16(&out).iter().enumerate() {
            if i % 4 == 1 {
                assert_eq!(*got, a[i], "mode {mode}: the transparent pixel stays");
            }
        }
    }
    let pass = outcome(fade_typed(
        &after,
        &before,
        BLEND_PASS_THROUGH,
        0.5,
        &[],
        false,
    ));
    assert!(pass.contains("Pass Through"), "{pass}");
    let other = native::doc(5, 3, plate16(5, 3, 1));
    let size = fade_typed(&after, &other, BLEND_NORMAL, 0.5, &[], false);
    assert_eq!(outcome(size), "fade source has a different canvas size");
    assert_eq!(
        outcome(fade_typed(&after, &before, BLEND_NORMAL, 1.5, &[], false)),
        "fade opacity is invalid"
    );
    assert_eq!(
        outcome(fade_typed(
            &after,
            &before,
            BLEND_NORMAL,
            0.5,
            &[1, 2, 3],
            false
        )),
        "fade coverage length differs from the canvas"
    );
}

// --------------------------------------------------------- 32-bit float --

#[test]
fn float_fade_keeps_the_range_refuses_screen_and_leaves_transparent_pixels() {
    let (w, h) = (4, 3);
    let srgb = IccProfile::srgb();
    // The step's result is 0.5 over a backdrop of 2.0 (above white); one
    // interior pixel is transparent on both sides.
    let record = |v: f32| -> Vec<[f32; 4]> {
        (0..w * h)
            .map(|i| {
                if i == 5 {
                    [v, -0.25, 3.0, 0.0]
                } else {
                    [v, v * 0.5, 0.25, 1.0]
                }
            })
            .collect()
    };
    let before = float_doc(w, h, &srgb, &[record(2.0)]);
    let after = float_doc(w, h, &srgb, &[record(0.5)]);
    let (b, a) = (samples32(&before), samples32(&after));
    for (mode, opacity) in [
        (BLEND_MULTIPLY, 1.0),
        (BLEND_NORMAL, 0.5),
        (BLEND_MULTIPLY, 0.5),
    ] {
        let out = fade_typed(&after, &before, mode, opacity, &[], false)
            .unwrap()
            .expect("a change");
        let got = samples32(&out);
        for i in 0..got.len() {
            assert!(got[i].iter().all(|v| v.is_finite()), "no NaN at pixel {i}");
            if b[i][3] == 0.0 && a[i][3] == 0.0 {
                assert_eq!(
                    got[i].map(f32::to_bits),
                    a[i].map(f32::to_bits),
                    "the transparent pixel stays the result's"
                );
                continue;
            }
            let want = expected(b[i].map(f64::from), a[i].map(f64::from), mode, 1.0, opacity);
            for c in 0..4 {
                assert!(
                    (f64::from(got[i][c]) - want[c]).abs() <= 1e-5 * want[c].abs().max(1.0),
                    "mode {mode} pixel {i} channel {c}: {} vs {}",
                    got[i][c],
                    want[c]
                );
            }
        }
        if mode == BLEND_MULTIPLY && opacity == 1.0 {
            // 2.0 × 0.5 = 1.0, unclamped arithmetic above white.
            assert!((got[0][0] - 1.0).abs() < 1e-6);
        }
    }
    let screen = outcome(fade_typed(&after, &before, BLEND_SCREEN, 0.5, &[], false));
    assert!(
        screen == "the screen blend mode is unavailable in a 32-bit float document",
        "{screen}"
    );
    // A u16 `before` under a float result refuses by name.
    let u16 = native::doc(w, h, plate16(w, h, 1));
    assert_eq!(
        outcome(fade_typed(&after, &u16, BLEND_NORMAL, 0.5, &[], false)),
        "fade source has a different bit depth"
    );
    // The same document on both sides, Normal: an exact no-op.
    assert_eq!(
        outcome(fade_typed(&after, &after, BLEND_NORMAL, 0.5, &[], false)),
        "none"
    );
    // A stroke weighed by changed samples: unchanged layers stay unchanged in
    // every mode the float document admits.
    assert_eq!(
        outcome(fade_typed(&after, &after, BLEND_MULTIPLY, 0.5, &[], true)),
        "none"
    );
}

// ------------------------------------------------------------ null safety --

#[test]
fn null_safety_sweep() {
    let before = DrDocument::from_pixels(plate8(3, 3, 0));
    let b = handle(&before);
    unsafe {
        assert!(dr_doc_fade(ptr::null(), b, 0, BLEND_NORMAL, 0.5, ptr::null(), 0, false).is_null());
        assert!(dr_doc_fade(b, ptr::null(), 0, BLEND_NORMAL, 0.5, ptr::null(), 0, false).is_null());
        assert!(dr_doc_fade(b, b, 0, BLEND_NORMAL, 0.5, ptr::null(), 9, false).is_null());
        dr_doc_free(b);
    }
    let doc = native::doc(3, 3, plate16(3, 3, 0));
    let mut e = ptr::null_mut();
    unsafe {
        assert!(
            dr_precision_doc_fade_plan(ptr::null(), 0, &doc, false, LIMIT, 0, &mut e).is_null()
        );
        assert_eq!(error(e), "required handle is NULL");
        e = ptr::null_mut();
        assert!(
            dr_precision_doc_fade_plan(&doc, 0, ptr::null(), false, LIMIT, 0, &mut e).is_null()
        );
        assert_eq!(error(e), "required handle is NULL");
        e = ptr::null_mut();
        assert!(dr_precision_doc_fade(
            ptr::null(),
            0,
            &doc,
            BLEND_NORMAL,
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
        assert!(dr_precision_doc_fade(
            &doc,
            0,
            ptr::null(),
            BLEND_NORMAL,
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
        // NULL err_out is tolerated.
        assert!(dr_precision_doc_fade(
            &doc,
            0,
            &doc,
            BLEND_PASS_THROUGH,
            0.5,
            ptr::null(),
            0,
            false,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
    }
}
