//! The magic wand's "Sample: Current layer" through the C FFI:
//! `dr_doc_magic_wand_layer` grows over one layer alone on a transparent
//! canvas. The oracle is this file's own four-connected flood fill over the
//! layer's canvas image, written without the core's region walk.

use std::collections::VecDeque;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::*;
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common::*;

const W: u32 = 12;
const H: u32 = 9;

/// The layer `idx` alone on a transparent canvas, from its pixels and offset.
fn layer_canvas(doc: *const DrDocument, idx: usize) -> Vec<[u8; 4]> {
    let (lw, lh) = layer_dims(doc, idx);
    let (ox, oy) = layer_offset(doc, idx);
    let px = layer_pixels(doc, idx);
    let mut out = vec![[0u8; 4]; (W * H) as usize];
    for y in 0..lh as i32 {
        for x in 0..lw as i32 {
            let (cx, cy) = (x + ox, y + oy);
            if cx < 0 || cy < 0 || cx >= W as i32 || cy >= H as i32 {
                continue;
            }
            let i = (y as u32 * lw + x as u32) as usize * 4;
            out[(cy as u32 * W + cx as u32) as usize] = [px[i], px[i + 1], px[i + 2], px[i + 3]];
        }
    }
    out
}

/// The oracle: every pixel within `tol` of the seed on all four channels,
/// reached four-connected from the seed when `contiguous`.
fn flood(img: &[[u8; 4]], seed: (u32, u32), tol: u8, contiguous: bool) -> Vec<u8> {
    let at = |x: u32, y: u32| img[(y * W + x) as usize];
    let s = at(seed.0, seed.1);
    let near = |p: [u8; 4]| (0..4).all(|c| p[c].abs_diff(s[c]) <= tol);
    let mut out = vec![0u8; (W * H) as usize];
    if !contiguous {
        for (o, p) in out.iter_mut().zip(img) {
            *o = if near(*p) { 255 } else { 0 };
        }
        return out;
    }
    let mut queue = VecDeque::from([seed]);
    out[(seed.1 * W + seed.0) as usize] = 255;
    while let Some((x, y)) = queue.pop_front() {
        let mut visit = |nx: u32, ny: u32| {
            let i = (ny * W + nx) as usize;
            if out[i] == 0 && near(at(nx, ny)) {
                out[i] = 255;
                queue.push_back((nx, ny));
            }
        };
        if x > 0 {
            visit(x - 1, y);
        }
        if x + 1 < W {
            visit(x + 1, y);
        }
        if y > 0 {
            visit(x, y - 1);
        }
        if y + 1 < H {
            visit(x, y + 1);
        }
    }
    out
}

fn wand_layer(doc: *const DrDocument, idx: usize, x: u32, y: u32, tol: u8, c: bool) -> Vec<u8> {
    let mut mask = vec![7u8; (W * H) as usize];
    let ok = unsafe { dr_doc_magic_wand_layer(doc, idx, x, y, tol, c, 1, mask.as_mut_ptr()) };
    assert!(ok, "magic_wand_layer({idx}, {x}, {y}) failed");
    mask
}

/// A red-to-orange background under a smaller layer at (3, 2) holding a blue
/// block with a green notch, transparent around it.
fn fixture(dir: &TempDir) -> *mut DrDocument {
    let base = RgbaImage::from_fn(W, H, |x, _| Rgba([200, (x * 9) as u8, 0, 255]));
    let doc = doc_from(dir, "base.png", &base);
    let top = RgbaImage::from_fn(7, 5, |x, y| {
        if x == 0 || y == 0 {
            Rgba([0, 0, 0, 0])
        } else if x == 3 && y < 4 {
            Rgba([0, 180, 0, 255])
        } else {
            Rgba([10, 20, 220, 255])
        }
    });
    let doc = add_layer(dir, "top.png", doc, 0, &top, "Top");
    apply(doc, |d| unsafe { dr_doc_with_layer_offset(d, 1, 3, 2) })
}

#[test]
fn the_layer_wand_grows_over_the_layer_alone() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let canvas = layer_canvas(doc, 1);
    for (seed, tol, contiguous) in [
        ((5, 5), 0, true),
        ((5, 5), 0, false),
        ((0, 0), 0, true),
        ((4, 3), 40, true),
        ((6, 4), 200, false),
    ] {
        let got = wand_layer(doc, 1, seed.0, seed.1, tol, contiguous);
        let want = flood(&canvas, seed, tol, contiguous);
        assert_eq!(got, want, "seed {seed:?} tol {tol} contiguous {contiguous}");
    }
    // The transparent area of the top layer is one region on the layer even
    // though the composite shows the varying background there.
    let clear = wand_layer(doc, 1, 0, 0, 0, true);
    assert_eq!(clear[(8 * W + 11) as usize], 255);
    let mut composite = vec![0u8; (W * H) as usize];
    assert!(unsafe { dr_doc_magic_wand(doc, 0, 0, 0, true, 1, composite.as_mut_ptr()) });
    assert_ne!(clear, composite, "the composite wand sees the background");
    // The background layer answers its own pixels.
    let base = layer_canvas(doc, 0);
    assert_eq!(
        wand_layer(doc, 0, 2, 2, 20, true),
        flood(&base, (2, 2), 20, true)
    );
    unsafe { dr_doc_free(doc) };
}

#[test]
fn the_layer_wand_refuses_off_canvas_bad_index_adjustments_and_null() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let mut mask = vec![9u8; (W * H) as usize];
    unsafe {
        assert!(!dr_doc_magic_wand_layer(
            doc,
            1,
            W,
            0,
            0,
            true,
            1,
            mask.as_mut_ptr()
        ));
        assert!(!dr_doc_magic_wand_layer(
            doc,
            1,
            0,
            H,
            0,
            true,
            1,
            mask.as_mut_ptr()
        ));
        assert!(!dr_doc_magic_wand_layer(
            doc,
            2,
            0,
            0,
            0,
            true,
            1,
            mask.as_mut_ptr()
        ));
        assert!(!dr_doc_magic_wand_layer(
            ptr::null(),
            0,
            0,
            0,
            0,
            true,
            1,
            mask.as_mut_ptr()
        ));
        assert!(!dr_doc_magic_wand_layer(
            doc,
            0,
            0,
            0,
            0,
            true,
            1,
            ptr::null_mut()
        ));
    }
    assert!(mask.iter().all(|&m| m == 9), "a refusal writes nothing");
    let doc = set_meta(
        doc,
        1,
        &adjust_meta("levels", "{\"black\":0.1,\"gamma\":1.6}"),
    );
    assert!(!unsafe { dr_doc_magic_wand_layer(doc, 1, 0, 0, 0, true, 1, mask.as_mut_ptr()) });
    unsafe { dr_doc_free(doc) };
}
