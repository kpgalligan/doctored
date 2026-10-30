//! The byte retouch brushes: `dr_doc_retouch_brush_layer` (Blur,
//! Sharpen, Sponge), `dr_doc_history_brush_layer` and `dr_doc_smudge_layer`.
//! Every application runs through the FFI on documents built from the safe
//! constructors; the oracles are naive per-pixel loops over f64 written here
//! from the documented formulas (the full two-dimensional Gaussian sum, the
//! 8×8 coverage sum at every box pixel, the sub-stepped smudge walk).

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::{DrDocument, LayerRaster};
use doctored_core::doc_lock::LOCK_TRANSPARENCY;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_retouch_brush::{
    dr_doc_history_brush_layer, dr_doc_retouch_brush_layer, dr_doc_smudge_layer,
};
use image::{Rgba, RgbaImage};

use crate::common;
use common::*;

const LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];
const HARD: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;
const SOFT: &str =
    r#"{"version":1,"hardness":0.4,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;

fn handle(doc: &DrDocument) -> *mut DrDocument {
    Box::into_raw(Box::new(doc.clone()))
}

fn owned(out: *mut DrDocument) -> Option<DrDocument> {
    (!out.is_null()).then(|| *unsafe { Box::from_raw(out) })
}

fn retouch(
    doc: &DrDocument,
    idx: usize,
    overlay: &[u8],
    kind: u8,
    strength: f32,
) -> Option<DrDocument> {
    let d = handle(doc);
    let out = unsafe {
        dr_doc_retouch_brush_layer(
            d,
            idx,
            overlay.as_ptr(),
            doc.width,
            doc.height,
            kind,
            strength,
            false,
            false,
            ptr::null_mut(),
        )
    };
    unsafe { dr_doc_free(d) };
    owned(out)
}

fn history(
    doc: &DrDocument,
    idx: usize,
    overlay: &[u8],
    source: &DrDocument,
    source_idx: usize,
    opacity: f32,
) -> Option<DrDocument> {
    let (d, s) = (handle(doc), handle(source));
    let out = unsafe {
        dr_doc_history_brush_layer(
            d,
            idx,
            overlay.as_ptr(),
            doc.width,
            doc.height,
            s,
            source_idx,
            opacity,
        )
    };
    unsafe {
        dr_doc_free(d);
        dr_doc_free(s);
    }
    owned(out)
}

fn smudge(
    doc: &DrDocument,
    idx: usize,
    brush: &str,
    dabs: &[[f64; 4]],
    strength: f32,
    selection: Option<&[u8]>,
) -> (Option<DrDocument>, Option<String>) {
    let d = handle(doc);
    let brush = CString::new(brush).unwrap();
    let flat: Vec<f64> = dabs.iter().flatten().copied().collect();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_smudge_layer(
            d,
            idx,
            brush.as_ptr(),
            if flat.is_empty() {
                ptr::null()
            } else {
                flat.as_ptr()
            },
            dabs.len(),
            strength,
            selection.map_or(ptr::null(), <[u8]>::as_ptr),
            selection.map_or(0, <[u8]>::len),
            false,
            &mut err,
        )
    };
    unsafe { dr_doc_free(d) };
    take_doc(out, err)
}

/// Canvas-sized premultiplied overlay whose ALPHA is `f(x, y)`; the colour
/// bytes are junk, so a match with the alpha-only oracle proves they are
/// never read.
fn overlay(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            out.extend_from_slice(&[(x * 31 + 7) as u8, (y * 17 + 3) as u8, 99, f(x, y)]);
        }
    }
    out
}

/// Colour structure with mixed alpha: opaque, partial, and transparent pixels
/// that keep a (latent) colour.
fn plate(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let i = y * w + x;
        let value = |offset: u32| ((x * 23 + y * 41 + offset + (i % 3) * 17) % 256) as u8;
        let alpha = match i % 11 {
            3 => 0,
            7 => 90 + (i % 100) as u8,
            _ => 255,
        };
        Rgba([value(0), value(80), value(160), alpha])
    })
}

/// An opaque red canvas under `image` as layer 1 at `offset`.
fn placed(canvas: (u32, u32), image: &RgbaImage, offset: (i32, i32)) -> DrDocument {
    let mut doc = mask_fixture(canvas, image.dimensions(), offset);
    doc.layers[1].pixels = LayerRaster::new(image.clone());
    doc
}

fn px(bytes: &[u8], w: u32, x: i64, y: i64, h: u32) -> [f64; 4] {
    let cx = x.clamp(0, i64::from(w) - 1) as usize;
    let cy = y.clamp(0, i64::from(h) - 1) as usize;
    let i = (cy * w as usize + cx) * 4;
    [0, 1, 2, 3].map(|c| f64::from(bytes[i + c]) / 255.0)
}

/// The normalized Gaussian taps for `sigma`: `2·ceil(3σ) + 1` weights.
fn taps(sigma: f64) -> Vec<f64> {
    let half = (3.0 * sigma).ceil() as i64;
    let raw: Vec<f64> = (-half..=half)
        .map(|k| (-0.5 * (k as f64 / sigma).powi(2)).exp())
        .collect();
    let sum: f64 = raw.iter().sum();
    raw.iter().map(|v| v / sum).collect()
}

/// The alpha-weighted Gaussian of the straight colour at a pixel, as the full
/// two-dimensional sum with clamped reads; the pixel's own colour when the
/// weight sum is 0.
fn gaussian(bytes: &[u8], w: u32, h: u32, sigma: f64, x: u32, y: u32) -> [f64; 3] {
    let k = taps(sigma);
    let half = (k.len() / 2) as i64;
    let (mut acc, mut total) = ([0.0; 3], 0.0);
    for (j, ky) in k.iter().enumerate() {
        for (i, kx) in k.iter().enumerate() {
            let p = px(
                bytes,
                w,
                i64::from(x) + i as i64 - half,
                i64::from(y) + j as i64 - half,
                h,
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
        let p = px(bytes, w, i64::from(x), i64::from(y), h);
        [p[0], p[1], p[2]]
    }
}

/// The kind's full-strength target at a pixel, from the documented formulas.
fn target(bytes: &[u8], w: u32, h: u32, kind: u8, x: u32, y: u32) -> [f64; 3] {
    let p = px(bytes, w, i64::from(x), i64::from(y), h);
    let luma = LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2];
    match kind {
        0 => gaussian(bytes, w, h, 2.0, x, y),
        1 => {
            let low = gaussian(bytes, w, h, 1.0, x, y);
            std::array::from_fn(|c| (p[c] + (p[c] - low[c])).clamp(0.0, 1.0))
        }
        2 => std::array::from_fn(|c| (luma + 2.0 * (p[c] - luma)).clamp(0.0, 1.0)),
        3 => [luma; 3],
        _ => panic!("bad kind {kind}"),
    }
}

/// Asserts `after` is `before` moved toward each pixel's target by
/// `coverage · strength`, within one code (the kernel mixes in f32), with
/// alpha and every uncovered or transparent pixel exact.
fn assert_retouched(
    before: &[u8],
    after: &[u8],
    (w, h): (u32, u32),
    kind: u8,
    strength: f64,
    coverage: impl Fn(u32, u32) -> u8,
) {
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let cover = coverage(x, y);
            assert_eq!(after[i + 3], before[i + 3], "alpha at {x},{y}");
            if cover == 0 || before[i + 3] == 0 {
                assert_eq!(after[i..i + 4], before[i..i + 4], "untouched {x},{y}");
                continue;
            }
            let wanted = target(before, w, h, kind, x, y);
            let amount = f64::from(cover) / 255.0 * strength;
            for c in 0..3 {
                let v = f64::from(before[i + c]) / 255.0;
                let expect = (v + amount * (wanted[c] - v)) * 255.0;
                assert!(
                    (f64::from(after[i + c]) - expect).abs() <= 1.0,
                    "kind {kind} at {x},{y} channel {c}: {} vs {expect}",
                    after[i + c]
                );
            }
        }
    }
}

// ------------------------------------------------- blur, sharpen, sponge --

#[test]
fn each_kind_matches_its_oracle_at_full_and_partial_coverage() {
    let (w, h) = (22_u32, 17_u32);
    let img = plate(w, h);
    let doc = DrDocument::from_pixels(img.clone());
    let ramp = |x: u32, y: u32| ((x * 11 + y * 7) % 256) as u8;
    for kind in 0..=3_u8 {
        let out = retouch(&doc, 0, &overlay(w, h, |_, _| 255), kind, 1.0)
            .unwrap_or_else(|| panic!("kind {kind} refused"));
        assert_retouched(
            img.as_raw(),
            &layer_bytes(&out, 0),
            (w, h),
            kind,
            1.0,
            |_, _| 255,
        );
        let out = retouch(&doc, 0, &overlay(w, h, ramp), kind, 0.6)
            .unwrap_or_else(|| panic!("kind {kind} refused"));
        assert_retouched(img.as_raw(), &layer_bytes(&out, 0), (w, h), kind, 0.6, ramp);
    }
}

#[test]
fn the_brushes_do_what_their_names_say() {
    // A hard vertical edge: Blur softens it, Sharpen overshoots it.
    let (w, h) = (16_u32, 8_u32);
    let edge = DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, _| {
        let v = if x < 8 { 60 } else { 180 };
        Rgba([v, v, v, 255])
    }));
    let full = overlay(w, h, |_, _| 255);
    let blurred = layer_bytes(&retouch(&edge, 0, &full, 0, 1.0).unwrap(), 0);
    let at = |bytes: &[u8], x: u32| bytes[((3 * w + x) * 4) as usize];
    assert!(at(&blurred, 7) > 60 && at(&blurred, 8) < 180);
    assert_eq!(at(&blurred, 0), 60);
    let sharpened = layer_bytes(&retouch(&edge, 0, &full, 1, 1.0).unwrap(), 0);
    assert!(at(&sharpened, 7) < 60 && at(&sharpened, 8) > 180);
    // A saturated colour: Desaturate takes it to its luma, Saturate away.
    let colour = DrDocument::from_pixels(solid(4, 4, [200, 100, 50, 255]));
    let full = overlay(4, 4, |_, _| 255);
    let grey = layer_bytes(&retouch(&colour, 0, &full, 3, 1.0).unwrap(), 0);
    assert!(grey[0] == grey[1] && grey[1] == grey[2]);
    let vivid = layer_bytes(&retouch(&colour, 0, &full, 2, 1.0).unwrap(), 0);
    assert!(vivid[0] > 200 && vivid[2] < 50);
}

#[test]
fn the_overlay_maps_through_the_layer_offset_and_the_layer_does_not_grow() {
    // A 10×8 layer at (3, −2) over a 12×10 canvas.
    let doc = placed((12, 10), &plate(10, 8), (3, -2));
    let before = layer_bytes(&doc, 1);
    let cover = |x: u32, y: u32| if x >= 5 && y < 4 { 255 } else { 0 };
    let out = retouch(&doc, 1, &overlay(12, 10, cover), 3, 1.0).expect("a change");
    assert_eq!(out.layers[1].pixels.dimensions(), (10, 8));
    assert_eq!(out.layers[1].offset, (3, -2));
    // Layer pixel (lx, ly) sits at canvas (lx + 3, ly − 2).
    assert_retouched(&before, &layer_bytes(&out, 1), (10, 8), 3, 1.0, |lx, ly| {
        let (cx, cy) = (i64::from(lx) + 3, i64::from(ly) - 2);
        if (0..12).contains(&cx) && (0..10).contains(&cy) {
            cover(cx as u32, cy as u32)
        } else {
            0
        }
    });
    assert_eq!(layer_bytes(&out, 0), layer_bytes(&doc, 0));
}

#[test]
fn the_retouch_brush_refuses_what_it_documents() {
    let doc = DrDocument::from_pixels(plate(8, 6));
    let full = overlay(8, 6, |_, _| 255);
    assert!(retouch(&doc, 0, &full, 4, 1.0).is_none(), "kind 4");
    assert!(retouch(&doc, 0, &full, 3, f32::NAN).is_none(), "NaN");
    assert!(retouch(&doc, 0, &full, 3, 0.0).is_none(), "strength 0");
    assert!(retouch(&doc, 9, &full, 3, 1.0).is_none(), "index");
    assert!(
        retouch(&doc, 0, &overlay(8, 6, |_, _| 0), 3, 1.0).is_none(),
        "no coverage"
    );
    // Nothing to do: a flat picture under Blur, a grey one under Sponge.
    let flat = DrDocument::from_pixels(solid(8, 6, [90, 90, 90, 255]));
    for kind in 0..=3 {
        assert!(retouch(&flat, 0, &full, kind, 1.0).is_none(), "flat {kind}");
    }
    // The canvas size is checked before the overlay is read.
    let d = handle(&doc);
    unsafe {
        assert!(dr_doc_retouch_brush_layer(
            d,
            0,
            full.as_ptr(),
            9,
            6,
            3,
            1.0,
            false,
            false,
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_doc_retouch_brush_layer(
            ptr::null(),
            0,
            full.as_ptr(),
            8,
            6,
            3,
            1.0,
            false,
            false,
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_doc_retouch_brush_layer(
            d,
            0,
            ptr::null(),
            8,
            6,
            3,
            1.0,
            false,
            false,
            ptr::null_mut()
        )
        .is_null());
        dr_doc_free(d);
    }
}

// ---------------------------------------------------------- history brush --

/// The premultiplied mix of `a` toward `b` by `t`, as straight bytes.
fn history_oracle(a: [u8; 4], b: [u8; 4], t: f64, frozen: bool) -> [f64; 4] {
    let (a, b) = (
        a.map(|v| f64::from(v) / 255.0),
        b.map(|v| f64::from(v) / 255.0),
    );
    let alpha = (1.0 - t) * a[3] + t * b[3];
    let mut out = [a[0] * 255.0, a[1] * 255.0, a[2] * 255.0, alpha * 255.0];
    if alpha > 0.0 {
        for c in 0..3 {
            out[c] = ((1.0 - t) * a[c] * a[3] + t * b[c] * b[3]) / alpha * 255.0;
        }
    }
    if frozen {
        out[3] = a[3] * 255.0;
    }
    out
}

#[test]
fn the_history_brush_brings_back_a_painted_over_area_exactly() {
    let (w, h) = (20_u32, 14_u32);
    let earlier = DrDocument::from_pixels(plate(w, h));
    let now = DrDocument::from_pixels(solid(w, h, [250, 10, 10, 255]));
    let cover = |x: u32, y: u32| {
        if (4..15).contains(&x) && (3..11).contains(&y) {
            255
        } else {
            0
        }
    };
    let out = history(&now, 0, &overlay(w, h, cover), &earlier, 0, 1.0).expect("a change");
    let (got, old, new) = (
        layer_bytes(&out, 0),
        layer_bytes(&earlier, 0),
        layer_bytes(&now, 0),
    );
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            if cover(x, y) == 0 {
                assert_eq!(got[i..i + 4], new[i..i + 4], "outside {x},{y}");
            } else if old[i + 3] == 0 {
                assert_eq!(got[i..i + 4], [0, 0, 0, 0], "cleared {x},{y}");
            } else {
                assert_eq!(got[i..i + 4], old[i..i + 4], "restored {x},{y}");
            }
        }
    }
    // Painting the same state back onto itself changes nothing.
    assert!(history(&earlier, 0, &overlay(w, h, cover), &earlier, 0, 1.0).is_none());
}

#[test]
fn the_history_brush_mixes_premultiplied_by_coverage_times_opacity() {
    let (w, h) = (18_u32, 12_u32);
    let earlier = DrDocument::from_pixels(plate(w, h));
    let now = DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            (x * 9) as u8,
            200,
            (y * 15) as u8,
            if (x + y) % 5 == 0 {
                0
            } else {
                140 + (x * 5) as u8
            },
        ])
    }));
    let ramp = |x: u32, y: u32| ((x * 13 + y * 29) % 256) as u8;
    let out = history(&now, 0, &overlay(w, h, ramp), &earlier, 0, 0.7).expect("a change");
    let (got, old, new) = (
        layer_bytes(&out, 0),
        layer_bytes(&earlier, 0),
        layer_bytes(&now, 0),
    );
    for i in 0..(w * h) as usize {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let at = i * 4;
        let (a, b) = (
            [new[at], new[at + 1], new[at + 2], new[at + 3]],
            [old[at], old[at + 1], old[at + 2], old[at + 3]],
        );
        if ramp(x, y) == 0 {
            assert_eq!(got[at..at + 4], a, "uncovered {x},{y}");
            continue;
        }
        let expect = history_oracle(a, b, f64::from(ramp(x, y)) / 255.0 * 0.7, false);
        if (expect[3] + 0.5).floor() == 0.0 {
            // Nothing visible is left: the pixel as it was, or transparent black.
            assert!(got[at..at + 4] == a || got[at..at + 4] == [0, 0, 0, 0]);
            continue;
        }
        for c in 0..4 {
            assert!(
                (f64::from(got[at + c]) - expect[c]).abs() <= 1.0,
                "{x},{y} channel {c}: {} vs {}",
                got[at + c],
                expect[c]
            );
        }
    }
}

#[test]
fn the_history_brush_reads_the_source_at_the_same_canvas_position() {
    // Now: a 10×8 layer at (2, 1). Earlier: the same layer, 6×4 at (5, 3).
    let now = mask_fixture((14, 10), (10, 8), (2, 1));
    let earlier = placed((14, 10), &plate(6, 4), (5, 3));
    let full = overlay(14, 10, |_, _| 255);
    let out = history(&now, 1, &full, &earlier, 1, 1.0).expect("a change");
    assert_eq!(out.layers[1].pixels.dimensions(), (10, 8), "no growth");
    let (got, old) = (layer_bytes(&out, 1), layer_bytes(&earlier, 1));
    for ly in 0..8_i64 {
        for lx in 0..10_i64 {
            let at = ((ly * 10 + lx) * 4) as usize;
            // Canvas (lx + 2, ly + 1) is source pixel (lx − 3, ly − 2).
            let (sx, sy) = (lx - 3, ly - 2);
            let wanted = if (0..6).contains(&sx) && (0..4).contains(&sy) {
                let s = ((sy * 6 + sx) * 4) as usize;
                if old[s + 3] == 0 {
                    [0; 4]
                } else {
                    [old[s], old[s + 1], old[s + 2], old[s + 3]]
                }
            } else {
                // The source layer holds nothing there: transparent.
                [0; 4]
            };
            assert_eq!(got[at..at + 4], wanted, "{lx},{ly}");
        }
    }
}

#[test]
fn the_history_brush_keeps_alpha_under_a_transparency_lock() {
    let (w, h) = (12_u32, 9_u32);
    let earlier = DrDocument::from_pixels(solid(w, h, [10, 200, 30, 255]));
    let now = DrDocument::from_pixels(plate(w, h))
        .with_layer_locks(0, LOCK_TRANSPARENCY)
        .expect("locked");
    let out = history(&now, 0, &overlay(w, h, |_, _| 255), &earlier, 0, 1.0).expect("a change");
    let (got, new) = (layer_bytes(&out, 0), layer_bytes(&now, 0));
    for i in (0..got.len()).step_by(4) {
        assert_eq!(got[i + 3], new[i + 3], "alpha kept");
        if new[i + 3] == 0 {
            assert_eq!(got[i..i + 4], new[i..i + 4], "a transparent pixel stays");
        } else {
            assert_eq!(got[i..i + 3], [10, 200, 30]);
        }
    }
}

#[test]
fn the_history_brush_refuses_what_it_documents() {
    let now = DrDocument::from_pixels(solid(8, 6, [250, 10, 10, 255]));
    let earlier = DrDocument::from_pixels(plate(8, 6));
    let full = overlay(8, 6, |_, _| 255);
    assert!(history(&now, 0, &full, &earlier, 0, f32::NAN).is_none());
    assert!(history(&now, 0, &full, &earlier, 0, 0.0).is_none());
    assert!(history(&now, 3, &full, &earlier, 0, 1.0).is_none());
    assert!(history(&now, 0, &full, &earlier, 3, 1.0).is_none());
    // A state of another canvas size is not a state of this document.
    let other = DrDocument::from_pixels(plate(9, 6));
    assert!(history(&now, 0, &full, &other, 0, 1.0).is_none());
    let (d, s) = (handle(&now), handle(&earlier));
    unsafe {
        assert!(
            dr_doc_history_brush_layer(ptr::null(), 0, full.as_ptr(), 8, 6, s, 0, 1.0).is_null()
        );
        assert!(dr_doc_history_brush_layer(d, 0, ptr::null(), 8, 6, s, 0, 1.0).is_null());
        assert!(
            dr_doc_history_brush_layer(d, 0, full.as_ptr(), 8, 6, ptr::null(), 0, 1.0).is_null()
        );
        assert!(dr_doc_history_brush_layer(d, 0, full.as_ptr(), 7, 6, s, 0, 1.0).is_null());
        dr_doc_free(d);
        dr_doc_free(s);
    }
}

// ------------------------------------------------------------------ smudge --

/// A hard round dab's coverage of pixel `(x, y)`: the 8×8 sum of "inside the
/// circle", taken at EVERY pixel (no classification).
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

/// The documented smudge walk over an opaque-or-not straight image, in f64:
/// the footprint field, the sub-steps of at most a pixel, the staged
/// bilinear carry, one quantization at the end.
fn smudge_oracle(
    bytes: &[u8],
    (w, h): (usize, usize),
    dabs: &[[f64; 4]],
    strength: f64,
    weight: impl Fn(usize, usize) -> f64,
) -> Vec<[f64; 4]> {
    let clip = |d: &[f64; 4]| {
        let r = d[2] / 2.0;
        [
            (d[0] - r).floor().clamp(0.0, w as f64) as usize,
            (d[1] - r).floor().clamp(0.0, h as f64) as usize,
            (d[0] + r).ceil().clamp(0.0, w as f64) as usize,
            (d[1] + r).ceil().clamp(0.0, h as f64) as usize,
        ]
    };
    // The footprint: the union of the dabs' clipped boxes.
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
    let mut field: Vec<[f64; 4]> = bytes
        .chunks_exact(4)
        .map(|p| {
            let a = f64::from(p[3]) / 255.0;
            [
                f64::from(p[0]) / 255.0 * a,
                f64::from(p[1]) / 255.0 * a,
                f64::from(p[2]) / 255.0 * a,
                a,
            ]
        })
        .collect();
    for pair in dabs.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let length = (to[0] - from[0]).hypot(to[1] - from[1]);
        let m = (length.ceil() as usize).max(1);
        let step = ((to[0] - from[0]) / m as f64, (to[1] - from[1]) / m as f64);
        for j in 1..=m {
            let dab: [f64; 4] =
                std::array::from_fn(|c| from[c] + (to[c] - from[c]) * j as f64 / m as f64);
            let b = clip(&dab);
            let found = field.clone();
            let read = |x: f64, y: f64| -> [f64; 4] {
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
        .iter()
        .map(|p| {
            if p[3] > 0.0 {
                [
                    p[0] / p[3] * 255.0,
                    p[1] / p[3] * 255.0,
                    p[2] / p[3] * 255.0,
                    p[3] * 255.0,
                ]
            } else {
                [0.0; 4]
            }
        })
        .collect()
}

fn assert_smudged(got: &[u8], expect: &[[f64; 4]], before: &[u8], what: &str) {
    for (i, wanted) in expect.iter().enumerate() {
        let at = i * 4;
        if (wanted[3] + 0.5).floor() == 0.0 {
            assert!(
                got[at + 3] == 0 || got[at..at + 4] == before[at..at + 4],
                "{what}: pixel {i} should hold nothing"
            );
            continue;
        }
        for c in 0..4 {
            assert!(
                (f64::from(got[at + c]) - wanted[c]).abs() <= 1.0,
                "{what}: pixel {i} channel {c}: {} vs {}",
                got[at + c],
                wanted[c]
            );
        }
    }
}

/// White with one black column at `x`.
fn line(w: u32, h: u32, x: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |px, _| {
        let v = if px == x { 0 } else { 255 };
        Rgba([v, v, v, 255])
    })
}

#[test]
fn smudge_leaves_no_gap_between_two_dabs() {
    // (a) A 1 px black line, a hard size-24 stroke of two dabs 6 px apart
    // along the row through it at strength 0.5: every pixel between the line
    // and the second dab's centre is darkened.
    let (w, h) = (64_u32, 40_u32);
    let doc = DrDocument::from_pixels(line(w, h, 20));
    let dabs = [[20.5, 20.5, 24.0, 1.0], [26.5, 20.5, 24.0, 1.0]];
    let (out, message) = smudge(&doc, 0, HARD, &dabs, 0.5, None);
    assert_eq!(message, None);
    let got = layer_bytes(&out.expect("a change"), 0);
    for x in 21..=25_u32 {
        let v = got[((20 * w + x) * 4) as usize];
        assert!(v <= 251, "pixel {x} on the centre row is {v}");
    }
}

#[test]
fn smudge_does_not_depend_on_the_dab_spacing() {
    // (b) The same straight 12 px path as two dabs and as thirteen.
    let (w, h) = (72_u32, 44_u32);
    let doc = DrDocument::from_pixels(plate(w, h));
    let far = [[24.5, 22.5, 24.0, 1.0], [36.5, 22.5, 24.0, 1.0]];
    let near: Vec<[f64; 4]> = (0..13)
        .map(|k| [24.5 + f64::from(k), 22.5, 24.0, 1.0])
        .collect();
    let (a, _) = smudge(&doc, 0, HARD, &far, 0.5, None);
    let (b, _) = smudge(&doc, 0, HARD, &near, 0.5, None);
    let (a, b) = (a.expect("a change"), b.expect("a change"));
    assert_eq!(layer_bytes(&a, 0), layer_bytes(&b, 0));
    assert_ne!(layer_bytes(&a, 0), layer_bytes(&doc, 0));
}

#[test]
fn a_hard_tip_equals_the_oracle_that_sums_8x8_at_every_box_pixel() {
    // (c) Classification is exact: interior 1, exterior 0, the band its sum.
    let (w, h) = (60_u32, 48_u32);
    let img = plate(w, h);
    let doc = DrDocument::from_pixels(img.clone());
    let dabs = [
        [18.3, 20.7, 17.0, 1.0],
        [27.9, 24.2, 21.0, 0.8],
        [31.0, 15.5, 13.0, 1.0],
    ];
    let (out, message) = smudge(&doc, 0, HARD, &dabs, 0.7, None);
    assert_eq!(message, None);
    let got = layer_bytes(&out.expect("a change"), 0);
    let expect = smudge_oracle(
        img.as_raw(),
        (w as usize, h as usize),
        &dabs,
        0.7,
        |_, _| 1.0,
    );
    assert_smudged(&got, &expect, img.as_raw(), "hard tip");
}

/// Smudge's own refusal: 256 million units, not the brush job's
/// 2 billion.
const SMUDGE_WORK: &str =
    "smudge stroke exceeds 256 million work units; use a shorter stroke or a smaller brush";

#[test]
fn smudge_refuses_a_stroke_past_its_own_work_limit_before_walking_it() {
    // (d) A hard size-512 tip charges about 555,000 units a sub-step. Swept
    // 600 px once: 600 sub-steps, 3.3·10⁸, between Smudge's 2.56·10⁸ and the
    // brush's 2·10⁹, so it refuses in Smudge's sentence.
    let doc = DrDocument::from_pixels(line(1200, 600, 300));
    let sweep = |to: f64| [[300.5, 300.5, 512.0, 1.0], [to, 300.5, 512.0, 1.0]];
    let (out, message) = smudge(&doc, 0, HARD, &sweep(900.5), 0.5, None);
    assert!(out.is_none());
    assert_eq!(message.as_deref(), Some(SMUDGE_WORK));
    // Seven such sweeps (2.3·10⁹, past the brush's cap too) refuse the same way.
    let dabs: Vec<[f64; 4]> = (0..8)
        .map(|i| [if i % 2 == 0 { 300.5 } else { 900.5 }, 300.5, 512.0, 1.0])
        .collect();
    let (out, message) = smudge(&doc, 0, HARD, &dabs, 0.5, None);
    assert!(out.is_none());
    assert_eq!(message.as_deref(), Some(SMUDGE_WORK));
    // Swept 300 px: 300 sub-steps, 1.7·10⁸, under the bound, so it runs.
    let (out, message) = smudge(&doc, 0, HARD, &sweep(600.5), 0.5, None);
    assert_eq!(message, None);
    assert!(
        out.is_some(),
        "a stroke under Smudge's bound smears the line"
    );
    // A far move is refused on its sub-step count alone: 2.8·10⁹ sub-steps.
    let far = 1_000_000_000.0;
    let dabs = [[-far, -far, 1.0, 1.0], [far, far, 1.0, 1.0]];
    let (out, message) = smudge(&doc, 0, HARD, &dabs, 0.5, None);
    assert!(out.is_none());
    assert_eq!(message.as_deref(), Some(SMUDGE_WORK));
}

#[test]
fn smudge_drags_the_picture_along_the_stroke_with_a_hard_and_a_soft_tip() {
    let (w, h) = (64_u32, 40_u32);
    let doc = DrDocument::from_pixels(line(w, h, 20));
    let dabs = [[20.5, 20.5, 20.0, 1.0], [34.5, 20.5, 20.0, 1.0]];
    for brush in [HARD, SOFT] {
        let (out, message) = smudge(&doc, 0, brush, &dabs, 0.8, None);
        assert_eq!(message, None);
        let got = layer_bytes(&out.expect("a change"), 0);
        let at = |x: u32, y: u32| got[((y * w + x) * 4) as usize];
        // The line's ink on the centre row moves with the stroke: its
        // centroid ends well to the right of column 20.
        let ink = |x: u32| f64::from(255 - at(x, 20));
        let total: f64 = (0..w).map(ink).sum();
        let centroid = (0..w).map(|x| ink(x) * f64::from(x)).sum::<f64>() / total;
        assert!(total > 100.0 && centroid > 23.0, "{total} at {centroid}");
        // Nothing is dragged against the stroke beyond the tip.
        assert_eq!(at(5, 20), 255);
        // Rows outside the stroke are untouched.
        assert_eq!(at(20, 2), 0);
        assert_eq!(at(24, 2), 255);
    }
}

#[test]
fn a_selection_confines_the_smudge_and_the_layer_offset_is_honoured() {
    // A 40×30 layer at (6, 4) over a 50×40 canvas; the dabs are in canvas
    // coordinates.
    let image = plate(40, 30);
    let doc = placed((50, 40), &image, (6, 4));
    let dabs = [[20.5, 18.5, 14.0, 1.0], [30.5, 22.5, 14.0, 1.0]];
    let local: Vec<[f64; 4]> = dabs
        .iter()
        .map(|d| [d[0] - 6.0, d[1] - 4.0, d[2], d[3]])
        .collect();
    let (out, message) = smudge(&doc, 1, HARD, &dabs, 0.6, None);
    assert_eq!(message, None);
    let out = out.expect("a change");
    assert_eq!(out.layers[1].pixels.dimensions(), (40, 30));
    let expect = smudge_oracle(image.as_raw(), (40, 30), &local, 0.6, |_, _| 1.0);
    assert_smudged(
        &layer_bytes(&out, 1),
        &expect,
        image.as_raw(),
        "offset layer",
    );
    // The selection is canvas-sized: only canvas rows below 21 may change.
    let chosen = selection(50, 40, |_, y| if y < 21 { 255 } else { 0 });
    let (out, _) = smudge(&doc, 1, HARD, &dabs, 0.6, Some(&chosen));
    let got = layer_bytes(&out.expect("a change"), 1);
    let expect = smudge_oracle(image.as_raw(), (40, 30), &local, 0.6, |_, ly| {
        if ly + 4 < 21 {
            1.0
        } else {
            0.0
        }
    });
    assert_smudged(&got, &expect, image.as_raw(), "selected");
    for ly in 17..30_u32 {
        for lx in 0..40_u32 {
            let at = ((ly * 40 + lx) * 4) as usize;
            assert_eq!(got[at..at + 4], image.as_raw()[at..at + 4], "{lx},{ly}");
        }
    }
}

#[test]
fn smudge_keeps_alpha_under_a_transparency_lock() {
    let (w, h) = (48_u32, 36_u32);
    let doc = DrDocument::from_pixels(plate(w, h))
        .with_layer_locks(0, LOCK_TRANSPARENCY)
        .expect("locked");
    let dabs = [[16.5, 18.5, 16.0, 1.0], [30.5, 18.5, 16.0, 1.0]];
    let (out, message) = smudge(&doc, 0, HARD, &dabs, 0.8, None);
    assert_eq!(message, None);
    let (got, before) = (
        layer_bytes(&out.expect("a change"), 0),
        layer_bytes(&doc, 0),
    );
    assert_ne!(got, before);
    for i in (0..got.len()).step_by(4) {
        assert_eq!(got[i + 3], before[i + 3], "alpha kept");
        if before[i + 3] == 0 {
            assert_eq!(got[i..i + 4], before[i..i + 4]);
        }
    }
}

/// A smear that deposits less than half a code of alpha on a fully
/// transparent pixel leaves it transparent, so its latent colour stays and the
/// stroke changes nothing. One sub-step at strength 0.3 from alpha-1 pixels:
/// the transparent side receives at most 0.3/255 (rounds to 0) and the alpha-1
/// side keeps at least 0.7/255 (rounds to 1) with its colour ratio unchanged.
#[test]
fn smudge_keeps_the_latent_colour_of_a_pixel_that_stays_transparent() {
    let image = RgbaImage::from_fn(40, 30, |x, _| {
        if x < 20 {
            Rgba([200, 100, 50, 1])
        } else {
            Rgba([10, 20, 30, 0])
        }
    });
    let doc = DrDocument::from_pixels(image);
    let dabs = [[19.5, 15.5, 12.0, 1.0], [20.5, 15.5, 12.0, 1.0]];
    let (out, message) = smudge(&doc, 0, HARD, &dabs, 0.3, None);
    assert_eq!(message, None);
    assert!(out.is_none(), "no byte changes, so no new document");
}

#[test]
fn smudge_answers_null_for_nothing_to_do_and_names_bad_input() {
    let doc = DrDocument::from_pixels(plate(40, 30));
    let dabs = [[10.5, 10.5, 12.0, 1.0], [20.5, 14.5, 12.0, 1.0]];
    // Nothing to do: one dab, strength 0, a flat picture, a missing index.
    for (what, result) in [
        ("one dab", smudge(&doc, 0, HARD, &dabs[..1], 0.5, None)),
        ("no dabs", smudge(&doc, 0, HARD, &[], 0.5, None)),
        ("strength 0", smudge(&doc, 0, HARD, &dabs, 0.0, None)),
        ("NaN", smudge(&doc, 0, HARD, &dabs, f32::NAN, None)),
        ("index", smudge(&doc, 7, HARD, &dabs, 0.5, None)),
        (
            "flat",
            smudge(
                &DrDocument::from_pixels(solid(40, 30, [9, 9, 9, 255])),
                0,
                HARD,
                &dabs,
                0.5,
                None,
            ),
        ),
    ] {
        assert!(result.0.is_none(), "{what}");
        assert_eq!(result.1, None, "{what}");
    }
    // Named refusals.
    let (out, message) = smudge(&doc, 0, "{", &dabs, 0.5, None);
    assert!(out.is_none());
    assert!(message.unwrap().starts_with("invalid native brush request"));
    let (out, message) = smudge(&doc, 0, HARD, &[[1.0, 1.0, -4.0, 1.0], dabs[1]], 0.5, None);
    assert!(out.is_none());
    assert_eq!(
        message.as_deref(),
        Some("native brush dab coordinates, diameter or opacity are invalid")
    );
    let short = vec![255_u8; 40 * 30 - 1];
    let (out, message) = smudge(&doc, 0, HARD, &dabs, 0.5, Some(&short));
    assert!(out.is_none());
    assert_eq!(
        message.as_deref(),
        Some("smudge selection length differs from the canvas")
    );
    // NULL arguments.
    let d = handle(&doc);
    let brush = CString::new(HARD).unwrap();
    let flat: Vec<f64> = dabs.iter().flatten().copied().collect();
    let mut err: *mut c_char = ptr::null_mut();
    unsafe {
        assert!(dr_doc_smudge_layer(
            ptr::null(),
            0,
            brush.as_ptr(),
            flat.as_ptr(),
            2,
            0.5,
            ptr::null(),
            0,
            false,
            &mut err
        )
        .is_null());
        assert!(err.is_null(), "a NULL document is silent");
        assert!(dr_doc_smudge_layer(
            d,
            0,
            ptr::null(),
            flat.as_ptr(),
            2,
            0.5,
            ptr::null(),
            0,
            false,
            &mut err
        )
        .is_null());
        assert_eq!(take_err_string(err), "native brush request is NULL");
        err = ptr::null_mut();
        assert!(dr_doc_smudge_layer(
            d,
            0,
            brush.as_ptr(),
            ptr::null(),
            2,
            0.5,
            ptr::null(),
            0,
            false,
            &mut err
        )
        .is_null());
        assert_eq!(
            take_err_string(err),
            "native brush requires a valid array of at most 50000 dabs; use a shorter stroke or wider spacing"
        );
        // A NULL err_out is tolerated on the error path.
        assert!(dr_doc_smudge_layer(
            d,
            0,
            ptr::null(),
            flat.as_ptr(),
            2,
            0.5,
            ptr::null(),
            0,
            false,
            ptr::null_mut()
        )
        .is_null());
        dr_doc_free(d);
    }
}
