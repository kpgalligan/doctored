//! The byte brush and Edit ▸ Fill… composite through ONE per-pixel ink
//! walk, black-box through the FFI.
//!
//! (a) Byte identity: Normal and Erase through `dr_doc_painting_layer`, its
//! blend and colour entries and a Normal fill land exactly what today's
//! premultiplied overlay plus `paint_pixel` math — copied into this file as
//! the oracle — lands. (b) Quantization: a non-Normal fill and the colour
//! brush blend the EXACT straight colour (an independent f64 W3C oracle),
//! while the overlay entry still un-premultiplies its overlay. (c) Parity and
//! refusals of the new colour entry. The two ignored probes measure the
//! fill's peak memory (run under `/usr/bin/time -l` after a `--no-run`
//! build).

use std::ffi::c_int;
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi_doc::*;
use doctored_core::ffi_fill_stroke::dr_doc_fill_selection;
use doctored_core::ffi_group::dr_doc_with_layer_locks;
use image::{Rgba, RgbaImage};

use crate::common;
use common::*;

const INK: [u8; 3] = [200, 100, 50];
const W: u32 = 16;
const H: u32 = 12;

/// A straight-alpha layer whose alphas include 0, semi-transparent values
/// and 255.
fn varied(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let i = y * w + x;
        let a = match i % 6 {
            0 => 0,
            1 => 255,
            k => (k * 47 + i * 3) as u8,
        };
        Rgba([
            (i * 37 % 256) as u8,
            (i * 11 % 256) as u8,
            (90 + i * 7) as u8,
            a,
        ])
    })
}

/// The fill's premultiply at coverage `a`: `(c·a + 127) / 255`.
fn premul(rgb: [u8; 3], a: u8) -> [u8; 4] {
    let c = |v: u8| ((u32::from(v) * u32::from(a) + 127) / 255) as u8;
    [c(rgb[0]), c(rgb[1]), c(rgb[2]), a]
}

/// A canvas-frame overlay of `premul(INK, a)` everywhere.
fn overlay(w: u32, h: u32, a: u8) -> Vec<u8> {
    (0..w * h).flat_map(|_| premul(INK, a)).collect()
}

/// Today's Over math, copied (blend::paint_pixel's Over arm and
/// source_over_rgba8, before the one walk): the identity oracle.
fn old_over(mut dp: [u8; 4], sp: [u8; 4], a: f32) -> [u8; 4] {
    if sp == [0; 4] {
        return dp;
    }
    let sa = (f32::from(sp[3]) / 255.0) * a;
    let da = f32::from(dp[3]) / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a < 1e-6 {
        dp[3] = 0;
        return dp;
    }
    for c in 0..3 {
        let scp = (f32::from(sp[c]) / 255.0) * a;
        let dc = f32::from(dp[c]) / 255.0;
        let v = (scp + dc * da * (1.0 - sa)) / out_a;
        dp[c] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    dp[3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
    dp
}

/// Today's Erase math, copied (blend::paint_pixel's Erase arm).
fn old_erase(mut dp: [u8; 4], sp: [u8; 4], a: f32) -> [u8; 4] {
    if sp[3] == 0 {
        return dp;
    }
    let sa = (f32::from(sp[3]) / 255.0) * a;
    let out_a = f32::from(dp[3]) / 255.0 * (1.0 - sa);
    dp[3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
    dp
}

/// The independent f64 separable `B(cb, cs)` for the modes tested here.
fn blend(mode: c_int, cb: f64, cs: f64) -> f64 {
    match mode {
        BLEND_MULTIPLY => cb * cs,
        BLEND_SCREEN => cb + cs - cb * cs,
        BLEND_DARKEN => cb.min(cs),
        BLEND_DIFFERENCE => (cb - cs).abs(),
        _ => unreachable!("not a tested mode"),
    }
}

/// One W3C compositing step in f64 of straight `cs` at alpha `sa` onto the
/// straight byte pixel `dp`, quantized.
fn w3c(dp: [u8; 4], cs: [f64; 3], sa: f64, mode: c_int) -> [u8; 4] {
    let ab = f64::from(dp[3]) / 255.0;
    let ao = sa + ab * (1.0 - sa);
    let mut out = [0u8; 4];
    for c in 0..3 {
        let cb = f64::from(dp[c]) / 255.0;
        let v = (sa * (1.0 - ab) * cs[c] + sa * ab * blend(mode, cb, cs[c]) + (1.0 - sa) * ab * cb)
            / ao;
        out[c] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    out[3] = (ao * 255.0).round() as u8;
    out
}

fn near(got: [u8; 4], want: [u8; 4], what: &str) {
    for c in 0..4 {
        assert!(
            (i32::from(got[c]) - i32::from(want[c])).abs() <= 1,
            "{what}: got {got:?}, want {want:?}"
        );
    }
}

/// The FFI result's layer `idx` pixels, freeing the result; `None` on NULL.
fn take(out: *mut DrDocument, idx: usize) -> Option<Vec<u8>> {
    if out.is_null() {
        return None;
    }
    let pixels = layer_pixels(out, idx);
    unsafe { dr_doc_free(out) };
    Some(pixels)
}

fn painted(doc: &DrDocument, src: &[u8], mode: c_int, alpha: f32) -> Option<Vec<u8>> {
    let (w, h) = (doc.width, doc.height);
    take(
        unsafe { dr_doc_painting_layer(doc, 0, src.as_ptr(), w, h, mode, alpha) },
        0,
    )
}

fn blended(doc: &DrDocument, src: &[u8], mode: c_int, alpha: f32) -> Option<Vec<u8>> {
    let (w, h) = (doc.width, doc.height);
    take(
        unsafe { dr_doc_painting_layer_blend(doc, 0, src.as_ptr(), w, h, mode, alpha) },
        0,
    )
}

fn coloured(
    doc: &DrDocument,
    src: &[u8],
    rgb: [u8; 3],
    mode: c_int,
    alpha: f32,
) -> Option<Vec<u8>> {
    let (w, h) = (doc.width, doc.height);
    take(
        unsafe {
            dr_doc_painting_layer_blend_color(doc, 0, src.as_ptr(), w, h, rgb.as_ptr(), mode, alpha)
        },
        0,
    )
}

fn filled(doc: &DrDocument, mask: &[u8], rgba: [u8; 4], mode: c_int, opacity: f32) -> Vec<u8> {
    let (w, h) = (doc.width, doc.height);
    let out = unsafe {
        dr_doc_fill_selection(
            doc,
            0,
            mask.as_ptr(),
            w,
            h,
            rgba.as_ptr(),
            mode,
            opacity,
            false,
        )
    };
    take(out, 0).expect("the fill changes the layer")
}

fn px(buf: &[u8], i: usize) -> [u8; 4] {
    [buf[4 * i], buf[4 * i + 1], buf[4 * i + 2], buf[4 * i + 3]]
}

// ------------------------------------------------------------------ (a) --

#[test]
fn a_normal_and_erase_are_byte_identical_to_the_overlay_math() {
    let base = varied(W, H);
    let doc = DrDocument::from_pixels(base.clone());
    let before = base.as_raw();
    for coverage in [255u8, 128, 3] {
        let src = overlay(W, H, coverage);
        let mask = vec![coverage; (W * H) as usize];
        for alpha in [1.0f32, 0.8] {
            let want: Vec<u8> = (0..(W * H) as usize)
                .flat_map(|i| old_over(px(before, i), px(&src, i), alpha))
                .collect();
            let what = format!("coverage {coverage} alpha {alpha}");
            let over = painted(&doc, &src, COMPOSITE_OVER, alpha).unwrap();
            assert_eq!(over, want, "dr_doc_painting_layer {what}");
            let normal = blended(&doc, &src, BLEND_NORMAL, alpha).unwrap();
            assert_eq!(normal, want, "dr_doc_painting_layer_blend {what}");
            let colour = coloured(&doc, &src, INK, BLEND_NORMAL, alpha).unwrap();
            assert_eq!(colour, want, "dr_doc_painting_layer_blend_color {what}");
            let rgba = [INK[0], INK[1], INK[2], 255];
            assert_eq!(
                filled(&doc, &mask, rgba, BLEND_NORMAL, alpha),
                want,
                "fill {what}"
            );
            let erase: Vec<u8> = (0..(W * H) as usize)
                .flat_map(|i| old_erase(px(before, i), px(&src, i), alpha))
                .collect();
            let erased = painted(&doc, &src, COMPOSITE_ERASE, alpha).unwrap();
            assert_eq!(erased, erase, "erase {what}");
        }
    }
}

// ------------------------------------------------------------------ (b) --

#[test]
fn b_low_coverage_blends_the_exact_colour() {
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 0])));
    let over = premul(INK, 3);
    assert_eq!(over, [2, 1, 1, 3]);
    let rgba = [INK[0], INK[1], INK[2], 255];
    // Was (170, 85, 85, 3): the colour un-premultiplied out of [2, 1, 1, 3].
    assert_eq!(
        filled(&doc, &[3], rgba, BLEND_MULTIPLY, 1.0),
        [200, 100, 50, 3]
    );
    assert_eq!(
        coloured(&doc, &over, INK, BLEND_MULTIPLY, 1.0).unwrap(),
        [200, 100, 50, 3]
    );
    // The overlay entry still reads its overlay's colour (Clone Stamp's ink).
    assert_eq!(
        blended(&doc, &over, BLEND_MULTIPLY, 1.0).unwrap(),
        [170, 85, 85, 3]
    );
}

#[test]
fn b_fill_and_colour_brush_match_the_exact_colour_oracle_at_coverages_1_to_16() {
    let base = RgbaImage::from_fn(16, 1, |x, _| {
        Rgba([
            90 + x as u8 * 3,
            160,
            220 - x as u8 * 5,
            [0, 64, 200, 255][x as usize % 4],
        ])
    });
    let doc = DrDocument::from_pixels(base.clone());
    let mask: Vec<u8> = (1..=16).collect();
    let src: Vec<u8> = (1..=16u8).flat_map(|a| premul(INK, a)).collect();
    let cs = INK.map(|v| f64::from(v) / 255.0);
    let rgba = [INK[0], INK[1], INK[2], 255];
    for mode in [BLEND_MULTIPLY, BLEND_SCREEN, BLEND_DARKEN, BLEND_DIFFERENCE] {
        for opacity in [1.0f32, 0.7] {
            let fill = filled(&doc, &mask, rgba, mode, opacity);
            let brush = coloured(&doc, &src, INK, mode, opacity).unwrap();
            for (i, coverage) in mask.iter().enumerate() {
                let sa = f64::from(*coverage) / 255.0 * f64::from(opacity);
                let want = w3c(px(base.as_raw(), i), cs, sa, mode);
                let what = format!("mode {mode} opacity {opacity} coverage {coverage}");
                near(px(&fill, i), want, &format!("fill {what}"));
                near(px(&brush, i), want, &format!("blend_color {what}"));
            }
        }
    }
}

// ------------------------------------------------------------------ (c) --

#[test]
fn c_the_overlay_entry_still_un_premultiplies_its_overlay() {
    let base = varied(W, H);
    let doc = DrDocument::from_pixels(base.clone());
    let src: Vec<u8> = (0..W * H)
        .flat_map(|i| premul(INK, [0u8, 3, 9, 40, 128, 255][i as usize % 6]))
        .collect();
    for mode in [BLEND_MULTIPLY, BLEND_SCREEN] {
        let got = blended(&doc, &src, mode, 0.9).unwrap();
        for i in 0..(W * H) as usize {
            let sp = px(&src, i);
            let dp = px(base.as_raw(), i);
            let want = if sp[3] == 0 {
                dp
            } else {
                let cs = [0, 1, 2].map(|c| (f64::from(sp[c]) / f64::from(sp[3])).min(1.0));
                w3c(dp, cs, f64::from(sp[3]) / 255.0 * 0.9, mode)
            };
            near(px(&got, i), want, &format!("mode {mode} pixel {i}"));
        }
    }
}

#[test]
fn c_the_colour_entry_at_full_overlay_alpha_is_the_overlay_entry() {
    let doc = DrDocument::from_pixels(varied(W, H));
    let src = overlay(W, H, 255);
    for mode in [
        BLEND_MULTIPLY,
        BLEND_SCREEN,
        BLEND_DISSOLVE,
        BLEND_HUE,
        BLEND_DIFFERENCE,
    ] {
        for alpha in [1.0f32, 0.55] {
            assert_eq!(
                coloured(&doc, &src, INK, mode, alpha),
                blended(&doc, &src, mode, alpha),
                "mode {mode} alpha {alpha}"
            );
        }
    }
}

#[test]
fn c_the_colour_entry_refuses() {
    let doc = DrDocument::from_pixels(varied(W, H));
    let src = overlay(W, H, 200);
    let short = [0u8; 4];
    unsafe {
        let null_doc: *const DrDocument = ptr::null();
        let f = dr_doc_painting_layer_blend_color;
        let ink = INK.as_ptr();
        assert!(f(null_doc, 0, src.as_ptr(), W, H, ink, BLEND_MULTIPLY, 1.0).is_null());
        assert!(f(&doc, 0, ptr::null(), W, H, ink, BLEND_MULTIPLY, 1.0).is_null());
        assert!(f(
            &doc,
            0,
            src.as_ptr(),
            W,
            H,
            ptr::null(),
            BLEND_MULTIPLY,
            1.0
        )
        .is_null());
        assert!(f(
            null_doc,
            0,
            ptr::null(),
            W,
            H,
            ptr::null(),
            BLEND_MULTIPLY,
            1.0
        )
        .is_null());
        assert!(f(&doc, 0, src.as_ptr(), W, H, ink, BLEND_PASS_THROUGH, 1.0).is_null());
        assert!(f(&doc, 0, src.as_ptr(), W, H, ink, 99, 1.0).is_null());
        assert!(f(&doc, 0, src.as_ptr(), W, H, ink, BLEND_MULTIPLY, f32::NAN).is_null());
        assert!(f(&doc, 1, src.as_ptr(), W, H, ink, BLEND_MULTIPLY, 1.0).is_null());
        assert!(f(&doc, 0, short.as_ptr(), 1, 1, ink, BLEND_MULTIPLY, 1.0).is_null());
        // Multiply by white changes no opaque pixel: a refusal, not an
        // identical copy.
        let opaque = DrDocument::from_pixels(opaque_pattern(W, H));
        let white = [255u8; 3];
        assert!(f(
            &opaque,
            0,
            src.as_ptr(),
            W,
            H,
            white.as_ptr(),
            BLEND_MULTIPLY,
            1.0
        )
        .is_null());
        // A pixel-locked layer refuses, through the one lock gate.
        let locked = dr_doc_with_layer_locks(&doc, 0, LOCK_PIXELS);
        assert!(!locked.is_null());
        assert!(f(locked, 0, src.as_ptr(), W, H, ink, BLEND_MULTIPLY, 1.0).is_null());
        assert!(f(locked, 0, src.as_ptr(), W, H, ink, BLEND_NORMAL, 1.0).is_null());
        dr_doc_free(locked);
    }
}

// --------------------------------------------------------------- probes --

/// The 60-megapixel peak-memory probes' document and full mask.
fn probe_60mp() -> (DrDocument, Vec<u8>) {
    let (w, h) = (10_000u32, 6_000u32);
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([10, 20, 30, 255])));
    (doc, vec![255u8; w as usize * h as usize])
}

/// Baseline for the fill probe: the same document and mask, no fill. Run
/// both under `/usr/bin/time -l` after a `--no-run` build; the fill's peak
/// cost is the difference of the two maximum resident set sizes.
#[test]
#[ignore = "manual: peak memory probe"]
fn peak_60mp_baseline() {
    let (doc, mask) = probe_60mp();
    std::hint::black_box((&doc, &mask));
}

/// A whole-canvas Normal fill of the 60-megapixel document.
#[test]
#[ignore = "manual: peak memory probe"]
fn peak_60mp_fill() {
    let (doc, mask) = probe_60mp();
    let out = doc.fill_selection(
        0,
        Some(&mask),
        [200, 100, 50, 255],
        BlendMode::Normal,
        1.0,
        false,
    );
    assert!(std::hint::black_box(out).is_some());
}
