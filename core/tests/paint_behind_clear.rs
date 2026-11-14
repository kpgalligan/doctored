//! Behind and Clear at 8 bits (`paint_mode`'s rule), black-box through the
//! FFI: the colour brush (`dr_doc_painting_layer_blend_color`), Edit ▸ Fill…
//! (`dr_doc_fill_selection`), its Pattern contents
//! (`dr_doc_fill_selection_pattern`) and Edit ▸ Stroke… with nothing selected
//! (`dr_doc_stroke_layer_edge`).
//!
//! The oracle is the rule's two formulas written here in f64 from the rule
//! alone: Behind `ao = ab + sa(1 − ab)`, `Co = (Cb·ab + Cs·sa(1 − ab)) / ao`;
//! Clear `ao = ab(1 − sa)` with colour untouched; `sa` = ink alpha × opacity ×
//! coverage. Exact where the rule leaves a pixel untouched (opaque under
//! Behind, every colour byte under Clear), within one code elsewhere. A frozen
//! alpha (Preserve Transparency, a transparency lock) answers NULL, beside the
//! same call on the free layer answering a change.

use std::ffi::{c_char, c_int};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_lock::LOCK_TRANSPARENCY;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_fill_stroke::*;
use doctored_core::ffi_group::dr_doc_with_layer_locks;
use image::{Rgba, RgbaImage};

use crate::common;
use common::*;

const BEHIND: c_int = 28;
const CLEAR: c_int = 29;
const CENTER: c_int = 1;
const W: u32 = 18;
const H: u32 = 12;
const INK: [u8; 3] = [200, 100, 50];
const OPACITY: f32 = 0.7;

/// Alphas 0, 255 and partial, with colour under alpha 0 too.
fn layer_image() -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, y| {
        let i = y * W + x;
        let a = match i % 5 {
            0 => 0,
            1 => 255,
            k => (k * 53 + i * 3) as u8,
        };
        Rgba([
            (i * 37 % 256) as u8,
            (i * 11 % 256) as u8,
            (90 + i * 7) as u8,
            a,
        ])
    })
}

/// Soft coverage holding 0, 255 and partial weights.
fn coverage() -> Vec<u8> {
    (0..W * H)
        .map(|i| match i % 7 {
            0 => 0,
            1 => 255,
            k => (k * 31 + i) as u8,
        })
        .collect()
}

fn px(buf: &[u8], i: usize) -> [u8; 4] {
    [buf[4 * i], buf[4 * i + 1], buf[4 * i + 2], buf[4 * i + 3]]
}

/// The rule's Behind in f64, quantized.
fn behind(dp: [u8; 4], cs: [u8; 3], sa: f64) -> [u8; 4] {
    let ab = f64::from(dp[3]) / 255.0;
    let ao = ab + sa * (1.0 - ab);
    if ao <= 0.0 {
        return dp;
    }
    let mut out = [0u8; 4];
    for c in 0..3 {
        let cb = f64::from(dp[c]) / 255.0;
        let v = (cb * ab + f64::from(cs[c]) / 255.0 * sa * (1.0 - ab)) / ao;
        out[c] = (v * 255.0).round() as u8;
    }
    out[3] = (ao * 255.0).round() as u8;
    out
}

/// The rule's Clear in f64, quantized: alpha only.
fn clear(dp: [u8; 4], sa: f64) -> [u8; 4] {
    let ab = f64::from(dp[3]) / 255.0;
    [dp[0], dp[1], dp[2], (ab * (1.0 - sa) * 255.0).round() as u8]
}

/// Checks `got` against the rule for `mode` at per-pixel `sa`, with `ink`
/// per pixel: exact where the rule leaves bytes untouched, else within one.
fn assert_rule(
    mode: c_int,
    before: &[u8],
    got: &[u8],
    sa: impl Fn(usize) -> f64,
    ink: impl Fn(usize) -> [u8; 3],
    what: &str,
) {
    let mut moved = 0;
    for i in 0..before.len() / 4 {
        let (b, g, s) = (px(before, i), px(got, i), sa(i));
        if s <= 0.0 {
            assert_eq!(g, b, "{what}: pixel {i} has no paint");
            continue;
        }
        if mode == BEHIND && b[3] == 255 {
            assert_eq!(g, b, "{what}: opaque pixel {i} is never written");
            continue;
        }
        let want = if mode == BEHIND {
            behind(b, ink(i), s)
        } else {
            clear(b, s)
        };
        if mode == CLEAR {
            assert_eq!(g[..3], b[..3], "{what}: Clear writes no colour at {i}");
        }
        for c in 0..4 {
            assert!(
                (i32::from(g[c]) - i32::from(want[c])).abs() <= 1,
                "{what}: pixel {i}: got {g:?}, want {want:?}"
            );
        }
        moved += usize::from(g != b);
    }
    assert!(moved > 20, "{what}: the paint moved {moved} pixels");
}

fn take(out: *mut DrDocument) -> Option<Vec<u8>> {
    if out.is_null() {
        return None;
    }
    let pixels = layer_pixels(out, 0);
    unsafe { dr_doc_free(out) };
    Some(pixels)
}

fn fill(doc: &DrDocument, mask: &[u8], rgba: [u8; 4], mode: c_int, keep: bool) -> Option<Vec<u8>> {
    take(unsafe {
        dr_doc_fill_selection(
            doc,
            0,
            mask.as_ptr(),
            W,
            H,
            rgba.as_ptr(),
            mode,
            OPACITY,
            keep,
        )
    })
}

fn brush(doc: &DrDocument, overlay: &[u8], mode: c_int) -> Option<Vec<u8>> {
    take(unsafe {
        dr_doc_painting_layer_blend_color(
            doc,
            0,
            overlay.as_ptr(),
            W,
            H,
            INK.as_ptr(),
            mode,
            OPACITY,
        )
    })
}

/// A 3×2 tile of opaque colours and one transparent texel.
const TILE: [[u8; 4]; 6] = [
    [250, 10, 10, 255],
    [10, 240, 30, 255],
    [0, 0, 0, 0],
    [40, 40, 220, 255],
    [130, 130, 0, 255],
    [255, 255, 255, 255],
];

fn texel(i: usize) -> [u8; 4] {
    let (x, y) = (i as u32 % W, i as u32 / W);
    TILE[((y % 2) * 3 + x % 3) as usize]
}

fn pattern(doc: &DrDocument, mask: &[u8], mode: c_int, keep: bool) -> Option<Vec<u8>> {
    let tile: Vec<u8> = TILE.iter().flatten().copied().collect();
    take(unsafe {
        dr_doc_fill_selection_pattern(
            doc,
            0,
            mask.as_ptr(),
            W,
            H,
            tile.as_ptr(),
            3,
            2,
            mode,
            OPACITY,
            keep,
        )
    })
}

/// The canvas-frame overlay the brush reads: only its alpha is coverage.
fn overlay(mask: &[u8]) -> Vec<u8> {
    mask.iter().flat_map(|&a| [0, 0, 0, a]).collect()
}

fn locked(doc: &DrDocument) -> DrDocument {
    let out = unsafe { dr_doc_with_layer_locks(doc, 0, LOCK_TRANSPARENCY) };
    assert!(!out.is_null());
    *unsafe { Box::from_raw(out) }
}

#[test]
fn fill_brush_and_pattern_follow_the_rule() {
    let image = layer_image();
    let doc = DrDocument::from_pixels(image.clone());
    let before = image.as_raw();
    let mask = coverage();
    let sa = |i: usize| f64::from(mask[i]) / 255.0 * f64::from(OPACITY);
    for (mode, name) in [(BEHIND, "Behind"), (CLEAR, "Clear")] {
        let rgba = [INK[0], INK[1], INK[2], 255];
        let filled = fill(&doc, &mask, rgba, mode, false).expect("the fill changes the layer");
        assert_rule(mode, before, &filled, sa, |_| INK, &format!("{name} fill"));
        let brushed = brush(&doc, &overlay(&mask), mode).expect("the stroke changes the layer");
        assert_rule(
            mode,
            before,
            &brushed,
            sa,
            |_| INK,
            &format!("{name} brush"),
        );
        let tiled = pattern(&doc, &mask, mode, false).expect("the pattern changes the layer");
        let tile_sa = |i: usize| sa(i) * f64::from(texel(i)[3]) / 255.0;
        let tile_ink = |i: usize| {
            let t = texel(i);
            [t[0], t[1], t[2]]
        };
        assert_rule(
            mode,
            before,
            &tiled,
            tile_sa,
            tile_ink,
            &format!("{name} pattern"),
        );
    }
    // Clear's ink colour is inert: another colour clears exactly the same.
    let a = fill(&doc, &mask, [200, 100, 50, 255], CLEAR, false);
    let b = fill(&doc, &mask, [0, 255, 7, 255], CLEAR, false);
    assert_eq!(a, b, "Clear reads only the ink's alpha");
    // Clear scales by the ink's alpha: half-alpha ink clears half as much.
    let half = fill(&doc, &mask, [9, 9, 9, 128], CLEAR, false).unwrap();
    let half_sa = |i: usize| sa(i) * 128.0 / 255.0;
    assert_rule(
        CLEAR,
        before,
        &half,
        half_sa,
        |_| INK,
        "Clear at ink alpha 128",
    );
}

#[test]
fn a_frozen_alpha_changes_nothing() {
    let doc = DrDocument::from_pixels(layer_image());
    let frozen = locked(&doc);
    let mask = coverage();
    let rgba = [INK[0], INK[1], INK[2], 255];
    for (mode, name) in [(BEHIND, "Behind"), (CLEAR, "Clear")] {
        // The free layer changes, so the NULLs below are the freeze's.
        assert!(
            fill(&doc, &mask, rgba, mode, false).is_some(),
            "{name} fill"
        );
        assert!(
            fill(&doc, &mask, rgba, mode, true).is_none(),
            "{name} fill, preserved"
        );
        assert!(
            fill(&frozen, &mask, rgba, mode, false).is_none(),
            "{name} fill, locked"
        );
        assert!(
            pattern(&doc, &mask, mode, false).is_some(),
            "{name} pattern"
        );
        assert!(
            pattern(&doc, &mask, mode, true).is_none(),
            "{name} pattern, preserved"
        );
        assert!(
            pattern(&frozen, &mask, mode, false).is_none(),
            "{name} pattern, locked"
        );
        assert!(brush(&doc, &overlay(&mask), mode).is_some(), "{name} brush");
        assert!(
            brush(&frozen, &overlay(&mask), mode).is_none(),
            "{name} brush, locked"
        );
    }
}

#[test]
fn behind_over_an_opaque_layer_and_clear_over_a_clear_one_change_nothing() {
    let opaque = DrDocument::from_pixels(opaque_pattern(W, H));
    let empty = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([9, 8, 7, 0])));
    let all = vec![255u8; (W * H) as usize];
    let rgba = [INK[0], INK[1], INK[2], 255];
    assert!(fill(&opaque, &all, rgba, BEHIND, false).is_none());
    assert!(brush(&opaque, &overlay(&all), BEHIND).is_none());
    assert!(fill(&empty, &all, rgba, CLEAR, false).is_none());
    assert!(brush(&empty, &overlay(&all), CLEAR).is_none());
}

/// A `CW×CH` opaque background with a soft disc above it at `offset`.
fn disc_doc(offset: (i32, i32)) -> DrDocument {
    let disc = RgbaImage::from_fn(30, 24, |x, y| {
        let d = ((x as f32 + 0.5 - 15.0).powi(2) + (y as f32 + 0.5 - 12.0).powi(2)).sqrt();
        let a = ((10.0 - d + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
        Rgba([(x * 8) as u8, (y * 10) as u8, 120, a])
    });
    let mut doc = DrDocument::from_pixels(opaque_pattern(40, 32))
        .adding_image_layer(0, disc, "Disc")
        .expect("a layer");
    doc.layers[1].offset = offset;
    doc
}

fn edge(doc: &DrDocument, mode: c_int, width: f32) -> Option<DrDocument> {
    let mut err: *mut c_char = ptr::null_mut();
    let rgba = [INK[0], INK[1], INK[2], 255];
    let out = unsafe {
        dr_doc_stroke_layer_edge(
            doc,
            1,
            width,
            CENTER,
            rgba.as_ptr(),
            mode,
            1.0,
            false,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "{}", take_err_string(err));
    (!out.is_null()).then(|| *unsafe { Box::from_raw(out) })
}

#[test]
fn the_layer_edge_stroke_follows_the_rule_and_clear_never_grows() {
    // In the canvas, away from its edges: the band is `dr_selection_stroke`
    // of the layer's alpha, and the stroke is the rule over that band.
    let doc = disc_doc((5, 4));
    let before = layer_pixels(&doc, 1);
    let alpha: Vec<u8> = (0..30 * 24).map(|i| before[4 * i + 3]).collect();
    let mut band = vec![0u8; 40 * 32];
    for y in 0..24 {
        for x in 0..30 {
            band[(y + 4) * 40 + x + 5] = alpha[y * 30 + x];
        }
    }
    assert!(unsafe { dr_selection_stroke(band.as_mut_ptr(), 40, 32, 2.0, CENTER) });
    let band_at = |i: usize| {
        let (x, y) = (i % 30 + 5, i / 30 + 4);
        f64::from(band[y * 40 + x]) / 255.0
    };
    let cleared = edge(&doc, CLEAR, 2.0).expect("Clear takes the band's alpha off");
    assert_eq!(
        layer_dims(&cleared, 1),
        (30, 24),
        "Clear never grows the layer"
    );
    assert_eq!(layer_offset(&cleared, 1), (5, 4));
    assert_rule(
        CLEAR,
        &before,
        &layer_pixels(&cleared, 1),
        band_at,
        |_| INK,
        "Clear edge",
    );
    let behind_doc = edge(&doc, BEHIND, 2.0).expect("Behind paints under the band");
    assert_eq!(
        layer_dims(&behind_doc, 1),
        (30, 24),
        "the band stays inside the layer"
    );
    assert_rule(
        BEHIND,
        &before,
        &layer_pixels(&behind_doc, 1),
        band_at,
        |_| INK,
        "Behind edge",
    );
    // A disc clipped by its own rect: Behind grows the layer to the band,
    // Clear keeps its storage exactly.
    let tight = DrDocument::from_pixels(opaque_pattern(40, 32))
        .adding_image_layer(0, solid(10, 8, [30, 60, 90, 200]), "Rect")
        .expect("a layer");
    let grown = edge(&tight, BEHIND, 3.0).expect("Behind paints outside the rect");
    assert_ne!(layer_dims(&grown, 1), (10, 8), "Behind grows the layer");
    let kept = edge(&tight, CLEAR, 3.0).expect("Clear takes alpha off the rect's edge");
    assert_eq!(layer_dims(&kept, 1), (10, 8), "Clear never grows the layer");
    assert_eq!(layer_offset(&kept, 1), (0, 0));
}

#[test]
fn every_other_mode_entry_refuses_the_paint_only_codes() {
    let doc = DrDocument::from_pixels(layer_image());
    let src = overlay(&coverage());
    for mode in [BEHIND, CLEAR] {
        let blended =
            unsafe { dr_doc_painting_layer_blend(&doc, 0, src.as_ptr(), W, H, mode, 1.0) };
        assert!(
            blended.is_null(),
            "the overlay-ink entry (Clone Stamp) refuses {mode}"
        );
        let set = unsafe { dr_doc_with_layer_blend_mode(&doc, 0, mode) };
        assert!(set.is_null(), "no layer carries {mode}");
    }
    assert!(brush(&doc, &src, 30).is_none(), "30 names no mode");
}
