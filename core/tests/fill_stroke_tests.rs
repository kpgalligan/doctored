//! Edit ▸ Fill… tests: `dr_doc_fill_selection`, through the FFI, against the
//! W3C reference composite in `tests/common` (`ref_composite`) and the
//! op's own contract (the layer never grows; Preserve Transparency keeps
//! the alpha byte-identical; a transparency lock is the same restore; every
//! no-op and malformed request is a refusal).
//!
//! Edit ▸ Stroke… tests: `dr_selection_stroke`'s band against the
//! `dr_selection_grow` / `dr_selection_shrink` contracts (the Euclidean
//! morphology; Layer Style ▸ Stroke's outside band differs from it only at
//! corners, #2), the canvas-edge frame, its
//! refusals, and the band filled through `dr_doc_fill_selection` against
//! `ref_composite`.

use std::ffi::{c_int, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_fill_stroke::*;
use doctored_core::ffi_group::*;
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common;
use common::*;

/// Runs the op through its FFI entry point, returning the new document or
/// `None` when the call refuses.
#[allow(clippy::too_many_arguments)]
fn filled(
    doc: *const DrDocument,
    idx: usize,
    mask: Option<&[u8]>,
    (w, h): (u32, u32),
    rgba: [u8; 4],
    mode: c_int,
    opacity: f32,
    preserve: bool,
) -> Option<*mut DrDocument> {
    let mask = mask.map_or(ptr::null(), |m| m.as_ptr());
    let out = unsafe {
        dr_doc_fill_selection(doc, idx, mask, w, h, rgba.as_ptr(), mode, opacity, preserve)
    };
    (!out.is_null()).then_some(out)
}

/// Asserts every channel of `got` is within `tol` of the unit-float `want`.
fn assert_near(got: [u8; 4], want: [f32; 4], tol: i32, what: &str) {
    for c in 0..4 {
        let w = (want[c] * 255.0).round() as i32;
        assert!(
            (i32::from(got[c]) - w).abs() <= tol,
            "{what}: channel {c} got {got:?}, want {want:?}"
        );
    }
}

/// A varied straight-alpha base with every alpha at least 64, so the
/// quantized colour stays within one code of the reference.
fn varied(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let i = y * w + x;
        Rgba([
            (i * 37 % 256) as u8,
            (i * 11 % 256) as u8,
            (200 + i * 3 % 56) as u8,
            (64 + i * 23 % 192) as u8,
        ])
    })
}

#[test]
fn normal_full_and_half_coverage_match_the_reference() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "white.png", &solid(4, 2, [255, 255, 255, 255]));
    // Left column fully selected, the next half selected, the rest outside.
    let mask = selection(4, 2, |x, _| match x {
        0 => 255,
        1 => 128,
        _ => 0,
    });
    let out = filled(doc, 0, Some(&mask), (4, 2), RED, BLEND_NORMAL, 1.0, false).unwrap();
    let px = layer_pixels(out, 0);
    for y in 0..2 {
        for x in 0..4 {
            let cov = f32::from(mask[(y * 4 + x) as usize]) / 255.0;
            let want = ref_composite([1.0; 4], [1.0, 0.0, 0.0, cov], 1.0, BLEND_NORMAL);
            assert_near(pixel(&px, 4, x, y), want, 1, "normal fill");
        }
    }
    unsafe {
        dr_doc_free(out);
        dr_doc_free(doc);
    }
}

#[test]
fn multiply_and_screen_at_half_opacity_match_the_reference() {
    let dir = TempDir::new().unwrap();
    let base = varied(6, 5);
    let doc = doc_from(&dir, "base.png", &base);
    let color = [200u8, 100, 50, 255];
    for mode in [BLEND_MULTIPLY, BLEND_SCREEN] {
        let out = filled(doc, 0, None, (6, 5), color, mode, 0.5, false).unwrap();
        let px = layer_pixels(out, 0);
        for (x, y, bg) in base.enumerate_pixels() {
            let want = ref_composite(to_unit(bg.0), to_unit(color), 0.5, mode);
            assert_near(pixel(&px, 6, x, y), want, 1, "blend fill");
        }
        unsafe { dr_doc_free(out) };
    }
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_null_mask_fills_the_whole_canvas() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "base.png", &varied(5, 3));
    let everything = vec![255u8; 15];
    let whole = filled(doc, 0, None, (5, 3), RED, BLEND_MULTIPLY, 0.7, false).unwrap();
    let masked = filled(
        doc,
        0,
        Some(&everything),
        (5, 3),
        RED,
        BLEND_MULTIPLY,
        0.7,
        false,
    )
    .unwrap();
    assert_eq!(layer_pixels(whole, 0), layer_pixels(masked, 0));
    unsafe {
        dr_doc_free(whole);
        dr_doc_free(masked);
        dr_doc_free(doc);
    }
}

#[test]
fn an_offset_smaller_layer_maps_and_never_grows() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "white.png", &solid(6, 6, [255, 255, 255, 255]));
    let doc = add_layer(&dir, "blue.png", doc, 0, &solid(2, 2, BLUE), "Small");
    let placed = unsafe { dr_doc_with_layer_offset(doc, 1, 3, 3) };
    assert!(!placed.is_null());
    unsafe { dr_doc_free(doc) };

    // The whole canvas: the layer turns red, and keeps its size and place.
    let out = filled(placed, 1, None, (6, 6), RED, BLEND_NORMAL, 1.0, false).unwrap();
    assert_eq!(layer_dims(out, 1), (2, 2));
    assert_eq!(layer_offset(out, 1), (3, 3));
    assert_eq!(layer_pixels(out, 1), RED.repeat(4));

    // Canvas pixel (3, 3) alone: only layer pixel (0, 0) changes.
    let mask = selection(6, 6, |x, y| if (x, y) == (3, 3) { 255 } else { 0 });
    let one = filled(
        placed,
        1,
        Some(&mask),
        (6, 6),
        RED,
        BLEND_NORMAL,
        1.0,
        false,
    )
    .unwrap();
    let px = layer_pixels(one, 1);
    assert_eq!(pixel(&px, 2, 0, 0), RED);
    assert_eq!(pixel(&px, 2, 1, 0), BLUE);
    assert_eq!(pixel(&px, 2, 0, 1), BLUE);
    assert_eq!(pixel(&px, 2, 1, 1), BLUE);

    // A selection that misses the layer entirely changes nothing.
    let miss = selection(6, 6, |x, y| u8::from(x < 3 && y < 3) * 255);
    assert!(filled(
        placed,
        1,
        Some(&miss),
        (6, 6),
        RED,
        BLEND_NORMAL,
        1.0,
        false
    )
    .is_none());
    unsafe {
        dr_doc_free(out);
        dr_doc_free(one);
        dr_doc_free(placed);
    }
}

/// A base with a fully transparent (but coloured) left column and varied
/// partial alpha elsewhere.
fn with_holes(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        if x == 0 {
            Rgba([10, 20, 30, 0])
        } else {
            varied(w, h).get_pixel(x, y).to_owned()
        }
    })
}

#[test]
fn preserve_transparency_keeps_the_alpha_byte_identical() {
    let dir = TempDir::new().unwrap();
    let base = with_holes(4, 3);
    let doc = doc_from(&dir, "holes.png", &base);
    let out = filled(doc, 0, None, (4, 3), RED, BLEND_NORMAL, 1.0, true).unwrap();
    let px = layer_pixels(out, 0);
    for (x, y, before) in base.enumerate_pixels() {
        let after = pixel(&px, 4, x, y);
        assert_eq!(after[3], before[3], "alpha at ({x}, {y})");
        if before[3] == 0 {
            assert_eq!(after, before.0, "an empty pixel is untouched");
        } else {
            assert_eq!(&after[..3], &RED[..3], "colour at ({x}, {y})");
        }
    }
    unsafe {
        dr_doc_free(out);
        dr_doc_free(doc);
    }
}

#[test]
fn a_transparency_lock_equals_preserve_transparency() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "holes.png", &with_holes(4, 3));
    let locked = unsafe { dr_doc_with_layer_locks(doc, 0, LOCK_TRANSPARENCY) };
    assert!(!locked.is_null());
    let color = [40u8, 160, 220, 255];
    let by_lock = filled(locked, 0, None, (4, 3), color, BLEND_MULTIPLY, 0.5, false).unwrap();
    let by_flag = filled(doc, 0, None, (4, 3), color, BLEND_MULTIPLY, 0.5, true).unwrap();
    assert_eq!(layer_pixels(by_lock, 0), layer_pixels(by_flag, 0));
    unsafe {
        dr_doc_free(by_lock);
        dr_doc_free(by_flag);
        dr_doc_free(locked);
        dr_doc_free(doc);
    }
}

#[test]
fn a_pixel_locked_layer_is_refused() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "white.png", &solid(3, 3, [255, 255, 255, 255]));
    let locked = unsafe { dr_doc_with_layer_locks(doc, 0, LOCK_PIXELS) };
    assert!(!locked.is_null());
    assert!(filled(locked, 0, None, (3, 3), RED, BLEND_NORMAL, 1.0, false).is_none());
    unsafe {
        dr_doc_free(locked);
        dr_doc_free(doc);
    }
}

#[test]
fn no_ops_and_malformed_requests_are_refused() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "base.png", &varied(4, 4));
    let size = (4, 4);

    // An empty selection changes nothing.
    let empty = vec![0u8; 16];
    assert!(filled(doc, 0, Some(&empty), size, RED, BLEND_NORMAL, 1.0, false).is_none());
    assert!(filled(doc, 0, Some(&empty), size, RED, BLEND_MULTIPLY, 1.0, false).is_none());

    // A same-colour Normal fill changes nothing.
    let red = doc_from(&dir, "red.png", &solid(4, 4, RED));
    assert!(filled(red, 0, None, size, RED, BLEND_NORMAL, 1.0, false).is_none());

    // A preserved fill over a fully transparent area changes nothing.
    let holes = doc_from(&dir, "holes.png", &with_holes(4, 4));
    let left = selection(4, 4, |x, _| u8::from(x == 0) * 255);
    assert!(filled(holes, 0, Some(&left), size, RED, BLEND_NORMAL, 1.0, true).is_none());
    assert!(filled(holes, 0, Some(&left), size, RED, BLEND_SCREEN, 1.0, true).is_none());
    // ...while the same fill without the flag paints it.
    let painted = filled(holes, 0, Some(&left), size, RED, BLEND_NORMAL, 1.0, false).unwrap();

    // Malformed: NaN opacity, Pass Through, an unknown mode, a canvas-size
    // mismatch, NULL colour, an out-of-range layer.
    assert!(filled(doc, 0, None, size, RED, BLEND_NORMAL, f32::NAN, false).is_none());
    assert!(filled(doc, 0, None, size, RED, BLEND_PASS_THROUGH, 1.0, false).is_none());
    assert!(filled(doc, 0, None, size, RED, 99, 1.0, false).is_none());
    assert!(filled(doc, 0, None, (4, 3), RED, BLEND_NORMAL, 1.0, false).is_none());
    assert!(filled(doc, 1, None, size, RED, BLEND_NORMAL, 1.0, false).is_none());
    assert!(unsafe {
        dr_doc_fill_selection(
            doc,
            0,
            ptr::null(),
            4,
            4,
            ptr::null(),
            BLEND_NORMAL,
            1.0,
            false,
        )
    }
    .is_null());

    // A group entry is not a raster layer.
    let name = CString::new("Group").unwrap();
    let members = [0usize];
    let mut group = usize::MAX;
    let mut cleared = [0usize; 4];
    let mut cleared_len = 0usize;
    let mut reordered = [0usize; 4];
    let mut reordered_len = 0usize;
    let grouped = unsafe {
        dr_doc_group_layers(
            doc,
            members.as_ptr(),
            1,
            name.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            4,
        )
    };
    assert!(!grouped.is_null());
    assert!(filled(grouped, group, None, size, RED, BLEND_NORMAL, 1.0, false).is_none());

    unsafe {
        dr_doc_free(grouped);
        dr_doc_free(painted);
        dr_doc_free(holes);
        dr_doc_free(red);
        dr_doc_free(doc);
    }
}

// ---------------------------------------------------------------- stroke --

const STROKE_INSIDE: c_int = 0;
const STROKE_CENTER: c_int = 1;
const STROKE_OUTSIDE: c_int = 2;

/// `dr_selection_stroke` on a copy of `mask`: the call's answer and the
/// resulting bytes.
fn stroked(mask: &[u8], (w, h): (u32, u32), px: f32, location: c_int) -> (bool, Vec<u8>) {
    let mut out = mask.to_vec();
    let ok = unsafe { dr_selection_stroke(out.as_mut_ptr(), w, h, px, location) };
    (ok, out)
}

/// The band, asserting the call succeeded.
fn band(mask: &[u8], size: (u32, u32), px: f32, location: c_int) -> Vec<u8> {
    let (ok, out) = stroked(mask, size, px, location);
    assert!(ok, "dr_selection_stroke refused");
    out
}

fn grown(mask: &[u8], (w, h): (u32, u32), px: f32) -> Vec<u8> {
    let mut out = mask.to_vec();
    assert!(unsafe { dr_selection_grow(out.as_mut_ptr(), w, h, px) });
    out
}

fn shrunk(mask: &[u8], (w, h): (u32, u32), px: f32) -> Vec<u8> {
    let mut out = mask.to_vec();
    assert!(unsafe { dr_selection_shrink(out.as_mut_ptr(), w, h, px) });
    out
}

/// `mask − shrink(mask, px)`: the inside oracle away from the canvas edge.
fn inside_oracle(mask: &[u8], size: (u32, u32), px: f32) -> Vec<u8> {
    mask.iter()
        .zip(shrunk(mask, size, px))
        .map(|(&m, s)| m.saturating_sub(s))
        .collect()
}

/// `(grow(mask, px) · (255 − mask) + 127) / 255`: the outside oracle.
fn outside_oracle(mask: &[u8], size: (u32, u32), px: f32) -> Vec<u8> {
    mask.iter()
        .zip(grown(mask, size, px))
        .map(|(&m, g)| ((u32::from(g) * u32::from(255 - m) + 127) / 255) as u8)
        .collect()
}

/// The canvas-edge ring's coverage-to-remove at a pixel `k` whole pixels
/// from the nearest canvas edge: `round(clamp(k + 1 − px, 0, 1) · 255)`.
fn ring_keep(k: u32, px: f32) -> u8 {
    ((k as f32 + 1.0 - px).clamp(0.0, 1.0) * 255.0).round() as u8
}

fn edge_distance(x: u32, y: u32, (w, h): (u32, u32)) -> u32 {
    x.min(w - 1 - x).min(y).min(h - 1 - y)
}

/// Two interior selections, well clear of the canvas edge: a hard rect and
/// an anti-aliased ellipse.
fn interior_shapes(size: (u32, u32)) -> Vec<(&'static str, Vec<u8>)> {
    let (w, h) = size;
    let rect = selection(w, h, |x, y| {
        u8::from((12..36).contains(&x) && (10..30).contains(&y)) * 255
    });
    let ellipse = selection(w, h, |x, y| {
        let dx = (x as f32 + 0.5 - 24.0) / 11.0;
        let dy = (y as f32 + 0.5 - 20.0) / 8.5;
        let d = (dx * dx + dy * dy).sqrt();
        (((1.0 - d) * 9.0 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8
    });
    assert!(
        ellipse.iter().any(|&v| v > 0 && v < 255),
        "the ellipse is anti-aliased"
    );
    vec![("rect", rect), ("ellipse", ellipse)]
}

#[test]
fn stroke_bands_of_interior_shapes_match_grow_and_shrink() {
    let size = (48, 40);
    for (name, mask) in interior_shapes(size) {
        for px in [1.0f32, 2.0, 2.5, 4.0] {
            assert_eq!(
                band(&mask, size, px, STROKE_INSIDE),
                inside_oracle(&mask, size, px),
                "{name}: inside {px}"
            );
            assert_eq!(
                band(&mask, size, px, STROKE_OUTSIDE),
                outside_oracle(&mask, size, px),
                "{name}: outside {px}"
            );
        }
        let center = band(&mask, size, 4.0, STROKE_CENTER);
        let outside = band(&mask, size, 2.0, STROKE_OUTSIDE);
        let inside = band(&mask, size, 2.0, STROKE_INSIDE);
        for i in 0..center.len() {
            assert_eq!(
                u32::from(center[i]),
                u32::from(outside[i]) + u32::from(inside[i]),
                "{name}: center 4 == outside 2 + inside 2 at {i}"
            );
        }
    }
}

#[test]
fn select_all_inside_strokes_a_frame_and_outside_is_empty() {
    let size = (24, 18);
    let all = vec![255u8; 24 * 18];
    for px in [3.0f32, 2.5] {
        let got = band(&all, size, px, STROKE_INSIDE);
        for y in 0..size.1 {
            for x in 0..size.0 {
                let want = 255 - ring_keep(edge_distance(x, y, size), px);
                assert_eq!(
                    got[(y * size.0 + x) as usize],
                    want,
                    "inside {px} at ({x}, {y})"
                );
            }
        }
    }
    let frame = band(&all, size, 3.0, STROKE_INSIDE);
    assert_eq!(frame[0], 255);
    assert_eq!(frame[(9 * 24 + 12) as usize], 0, "the middle is untouched");
    assert!(band(&all, size, 4.0, STROKE_OUTSIDE)
        .iter()
        .all(|&v| v == 0));
    assert_eq!(
        band(&all, size, 4.0, STROKE_CENTER),
        band(&all, size, 2.0, STROKE_INSIDE),
        "center 4 == inside 2 when outside has nothing"
    );
}

#[test]
fn a_selection_touching_the_canvas_edge_is_stroked_along_it() {
    let size = (32, 24);
    let mask = selection(32, 24, |x, y| {
        u8::from(x < 14 && (5..19).contains(&y)) * 255
    });
    let got = band(&mask, size, 3.0, STROKE_INSIDE);
    let shrink = inside_oracle(&mask, size, 3.0);
    for y in 0..size.1 {
        for x in 0..size.0 {
            let i = (y * size.0 + x) as usize;
            let ring = mask[i].saturating_sub(ring_keep(edge_distance(x, y, size), 3.0));
            assert_eq!(got[i], shrink[i].max(ring), "at ({x}, {y})");
        }
    }
    // Interior rows: the left edge carries the full 3 px band.
    for y in 9..15u32 {
        for x in 0..3u32 {
            assert_eq!(
                got[(y * size.0 + x) as usize],
                255,
                "edge band at ({x}, {y})"
            );
        }
        assert_eq!(
            got[(y * size.0 + 6) as usize],
            0,
            "inside the band at (6, {y})"
        );
    }
}

#[test]
fn stroke_edge_cases_and_refusals() {
    let size = (8, 6);
    let empty = vec![0u8; 48];
    for location in [STROKE_INSIDE, STROKE_CENTER, STROKE_OUTSIDE] {
        assert_eq!(stroked(&empty, size, 3.0, location), (true, empty.clone()));
    }
    let mask = selection(8, 6, |x, y| {
        u8::from((2..6).contains(&x) && (2..4).contains(&y)) * 255
    });
    for px in [0.0f32, -1.0] {
        let (ok, out) = stroked(&mask, size, px, STROKE_CENTER);
        assert!(ok, "width {px} is not a refusal");
        assert!(out.iter().all(|&v| v == 0), "width {px} strokes nothing");
    }
    for px in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            stroked(&mask, size, px, STROKE_INSIDE),
            (false, mask.clone())
        );
    }
    for location in [3, -1] {
        assert_eq!(stroked(&mask, size, 2.0, location), (false, mask.clone()));
    }
    let mut copy = mask.clone();
    unsafe {
        assert!(!dr_selection_stroke(
            ptr::null_mut(),
            8,
            6,
            2.0,
            STROKE_INSIDE
        ));
        assert!(!dr_selection_stroke(
            copy.as_mut_ptr(),
            0,
            6,
            2.0,
            STROKE_INSIDE
        ));
        assert!(!dr_selection_stroke(
            copy.as_mut_ptr(),
            8,
            0,
            2.0,
            STROKE_INSIDE
        ));
    }
    assert_eq!(copy, mask, "a refused call leaves the mask untouched");
}

#[test]
fn a_stroke_band_fills_like_any_selection() {
    let dir = TempDir::new().unwrap();
    let size = (48, 40);
    let doc = doc_from(&dir, "white.png", &solid(48, 40, [255, 255, 255, 255]));
    let (_, ellipse) = interior_shapes(size).remove(1);
    let band = band(&ellipse, size, 3.0, STROKE_OUTSIDE);
    let out = filled(doc, 0, Some(&band), size, RED, BLEND_NORMAL, 1.0, false).unwrap();
    let px = layer_pixels(out, 0);
    for y in 0..size.1 {
        for x in 0..size.0 {
            let cov = f32::from(band[(y * size.0 + x) as usize]) / 255.0;
            let want = ref_composite([1.0; 4], [1.0, 0.0, 0.0, cov], 1.0, BLEND_NORMAL);
            assert_near(pixel(&px, size.0, x, y), want, 1, "stroke fill");
        }
    }
    unsafe {
        dr_doc_free(out);
        dr_doc_free(doc);
    }
}
