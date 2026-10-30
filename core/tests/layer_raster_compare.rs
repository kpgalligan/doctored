//! Black-box suite for 13B-2c-2b: tile-aware bytes equality
//! (`TilePlane::same_bits` / `equals_contiguous`, `LayerRaster::same_bytes`
//! / `equals_image`) at the five sites that used to compare two
//! `contiguous_image()` views, and the job-polled whole-layer copy
//! `doc_color_policy`'s per-layer conversion now runs.
//!
//! Oracles are independent of the implementation: expected equality is the
//! bytes the test built itself; each site's answer is compared byte for byte
//! with the same op on a CONTIGUOUS twin (every raster re-adopted through
//! `LayerRaster::shared`, the writers suite's `contiguous_twin`); the
//! conversion's checkpoints are counted on the worker's own control. The one
//! wall-clock verdict is `#[ignore]`d and runs under `make cancel-bench`.
//!
//! The view cache is process-wide, so every test holds `serial()`: a count
//! of `contiguous_views` must not see another test's rasters.

use crate::phase4_corpus;
use crate::serial;

use crate::common;

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;
use std::time::Instant;

use common::probe::{
    cancelled_parked_at, checkpoints_line, parked_line, past, probe, verdict, Trigger,
    RETURN_FLOOR_S,
};
use common::take_err_string;
use doctored_core::doc::{DrDocument, LayerKind, LayerRaster};
use doctored_core::doc_lock::LOCK_TRANSPARENCY;
use doctored_core::ffi_doc::{dr_doc_flip_horizontal, dr_doc_free, dr_doc_painting_layer};
use doctored_core::ffi_group::dr_doc_with_layer_locks;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_mask::dr_doc_set_layer_content_preserving_mask;
use doctored_core::ffi_output_color::dr_doc_convert_to_profile_with;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::layer_raster::contiguous_view_bytes;
use doctored_core::tile_store::{TilePlane, TILE_EDGE};
use doctored_core::warp::WarpEdit;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::tempdir;

use crate::resource_report;

// ------------------------------------------------------------- plumbing --

/// An owned document handle an FFI op answered (NULL allowed).
struct Out(*mut DrDocument);

impl Drop for Out {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { dr_doc_free(self.0) };
        }
    }
}

impl Out {
    fn get(&self) -> &DrDocument {
        assert!(!self.0.is_null());
        unsafe { &*self.0 }
    }
}

fn evict() {
    resource_report::evict(&[]).expect("an eviction report");
}

/// The `contiguous_views` allocation count of `document`'s own planes.
fn views_of(document: &DrDocument) -> u64 {
    let handles = [document as *const DrDocument];
    let value = resource_report::report(&handles, &[], &[], &[], 0, 0).expect("a resource report");
    value["categories"]["contiguous_views"]["allocation_count"]
        .as_u64()
        .unwrap_or(0)
}

/// Every raster re-adopted as CONTIGUOUS (`LayerRaster::shared` of an owned
/// copy): the same pixels, compared through the contiguous arms.
fn contiguous_twin(doc: &DrDocument) -> DrDocument {
    let mut out = doc.clone();
    for layer in &mut out.layers {
        if layer.kind == LayerKind::Raster {
            let copy = layer.pixels.to_contiguous().expect("a copy");
            layer.pixels = LayerRaster::shared(Arc::new(copy));
        }
    }
    out
}

fn corpus(width: u32, height: u32) -> RgbaImage {
    phase4_corpus::image(width, height)
}

fn bytes(raster: &LayerRaster) -> Vec<u8> {
    raster.to_contiguous().expect("a copy").into_raw()
}

fn tiles((w, h): (u32, u32)) -> usize {
    (w.div_ceil(TILE_EDGE) * h.div_ceil(TILE_EDGE)) as usize
}

/// `image` with one byte flipped at `at`.
fn flipped(image: &RgbaImage, at: usize) -> RgbaImage {
    let mut raw = image.as_raw().clone();
    raw[at] ^= 0x5a;
    RgbaImage::from_raw(image.width(), image.height(), raw).unwrap()
}

// ------------------------------------------------- TilePlane equality --

fn plane<S: doctored_core::tile_store::Sample>(w: u32, h: u32, samples: &[S]) -> TilePlane<S> {
    TilePlane::from_contiguous(w, h, samples, None).expect("a plane")
}

/// `same_bits` / `equals_contiguous` on a `w×h` plane of `samples`, a
/// separately allocated equal plane, and `changed` (one sample different).
fn check_plane<S: doctored_core::tile_store::Sample>(
    what: &str,
    (w, h): (u32, u32),
    samples: &[S],
    changed: &[S],
) {
    let a = plane(w, h, samples);
    let b = plane(w, h, samples);
    let c = plane(w, h, changed);
    assert_eq!(a.shared_tile_count(&b), 0, "{what}: separate allocations");
    assert!(a.same_bits(&a.clone()), "{what}: a clone");
    assert!(a.same_bits(&b), "{what}: equal without sharing");
    assert!(b.same_bits(&a), "{what}: symmetric");
    assert!(a.equals_contiguous(samples), "{what}: equals its samples");
    assert!(!a.same_bits(&c), "{what}: one sample differs");
    assert!(!c.same_bits(&a), "{what}: one sample differs (symmetric)");
    assert!(!a.equals_contiguous(changed), "{what}: vs changed samples");
    assert!(!c.equals_contiguous(samples), "{what}: changed vs samples");
    assert!(
        !a.equals_contiguous(&samples[..samples.len() - 1]),
        "{what}: a short slice"
    );
}

#[test]
fn tile_plane_equality_is_bitwise_across_sample_types_and_seams() {
    let _serial = serial();
    // u8 gray: 1×1, and 257×257 across the seam (the changed sample in the
    // last clipped tile, then in the first tile's last row).
    check_plane("u8 1x1", (1, 1), &[7_u8], &[8_u8]);
    let gray: Vec<u8> = (0..257 * 257).map(|i| (i * 31 % 251) as u8).collect();
    for at in [257 * 257 - 1, 255 * 257 + 255, 256] {
        let mut changed = gray.clone();
        changed[at] ^= 1;
        check_plane("u8 257x257", (257, 257), &gray, &changed);
    }
    // RGBA8.
    let rgba: Vec<[u8; 4]> = corpus(257, 257).as_raw().as_chunks::<4>().0.to_vec();
    let mut changed = rgba.clone();
    changed[256 * 257 + 256][3] ^= 1;
    check_plane("rgba8 257x257", (257, 257), &rgba, &changed);
    check_plane("rgba8 1x1", (1, 1), &[[1_u8, 2, 3, 4]], &[[1_u8, 2, 3, 5]]);
    // RGBA32F: -0.0 over 0.0 differs; a NaN pattern equals itself.
    let nan = f32::from_bits(0x7fc0_1234);
    let floats: Vec<[f32; 4]> = (0..257 * 257)
        .map(|i| [i as f32 * 0.5, 0.0, nan, 1.0])
        .collect();
    let mut negative = floats.clone();
    negative[256 * 257 + 256][1] = -0.0;
    check_plane("rgba32f -0.0", (257, 257), &floats, &negative);
    let a = plane(257, 257, &floats);
    let b = plane(257, 257, &floats);
    assert!(a.same_bits(&b), "a NaN pattern equals itself");
    let mut other_nan = floats.clone();
    other_nan[3][2] = f32::from_bits(0x7fc0_4321);
    assert!(!a.same_bits(&plane(257, 257, &other_nan)), "another NaN");
    // Dimensions differ.
    let wide = plane(20, 10, &[0_u8; 200]);
    let tall = plane(10, 20, &[0_u8; 200]);
    assert!(!wide.same_bits(&tall), "dimensions differ");
    assert!(!tall.same_bits(&wide), "dimensions differ (symmetric)");
    // A `with_rect` result vs its source, and its `rebuilt_from`-equal plane.
    let source = plane(257, 257, &rgba);
    let edited = source
        .with_rect(250, 250, 7, 7, &[[9, 9, 9, 9]; 49], None)
        .unwrap()
        .unwrap();
    assert!(!edited.same_bits(&source), "with_rect vs its source");
    assert!(!source.same_bits(&edited), "with_rect vs its source (sym)");
    let edited_samples = edited.to_contiguous(None).unwrap();
    let rebuilt = source.rebuilt_from(&edited_samples, None).unwrap().unwrap();
    assert!(rebuilt.shared_tile_count(&edited) < tiles((257, 257)));
    assert!(rebuilt.same_bits(&edited), "rebuilt_from-equal plane");
    assert!(edited.equals_contiguous(&edited_samples));
}

#[test]
fn tile_plane_equality_on_the_24_mp_corpus() {
    let _serial = serial();
    let (w, h) = phase4_corpus::DIMENSIONS[0];
    let samples: Vec<[u8; 4]> = corpus(w, h).as_raw().as_chunks::<4>().0.to_vec();
    let mut changed = samples.clone();
    let last = changed.len() - 1;
    changed[last][0] ^= 1;
    check_plane("rgba8 6000x4000", (w, h), &samples, &changed);
}

// ------------------------------------------------- LayerRaster equality --

/// `a.same_bytes(b) == expected`, symmetric, and no view left behind.
fn same(what: &str, a: &LayerRaster, b: &LayerRaster, expected: bool) {
    evict();
    assert_eq!(a.same_bytes(b), expected, "{what}");
    assert_eq!(b.same_bytes(a), expected, "{what} (symmetric)");
    assert_eq!(contiguous_view_bytes(), 0, "{what}: no view");
}

#[test]
fn same_bytes_in_every_representation_order() {
    let _serial = serial();
    let (w, h) = (600, 400);
    let image = corpus(w, h);
    let last = flipped(&image, (w * h * 4) as usize - 1);
    let first = flipped(&image, 0);
    let tiled = LayerRaster::new(image.clone());
    let arc = Arc::new(image.clone());
    let contiguous = LayerRaster::shared(Arc::clone(&arc));

    // (Tiled, Tiled) sharing every tile.
    let sharing = tiled.rebuilt(image.clone());
    assert_eq!(sharing.shared_tile_count(&tiled), tiles((w, h)));
    same("tiled/tiled shared", &tiled, &sharing, true);
    let sharing_but_one = tiled.rebuilt(last.clone());
    assert_eq!(sharing_but_one.shared_tile_count(&tiled), tiles((w, h)) - 1);
    same("tiled/tiled shared, last", &tiled, &sharing_but_one, false);
    // (Tiled, Tiled) equal without sharing.
    let apart = LayerRaster::new(image.clone());
    assert_eq!(apart.shared_tile_count(&tiled), 0);
    same("tiled/tiled apart", &tiled, &apart, true);
    same(
        "tiled/tiled apart, first",
        &tiled,
        &LayerRaster::new(first.clone()),
        false,
    );
    // (Tiled self, Contiguous other).
    same("tiled/contiguous", &tiled, &contiguous, true);
    let contiguous_last = LayerRaster::shared(Arc::new(last.clone()));
    evict();
    assert!(
        !tiled.same_bytes(&contiguous_last),
        "tiled self, contiguous other: last byte"
    );
    same("tiled/contiguous, last", &tiled, &contiguous_last, false);
    same(
        "tiled/contiguous, first",
        &tiled,
        &LayerRaster::shared(Arc::new(first.clone())),
        false,
    );
    // (Contiguous self, Tiled other).
    same("contiguous/tiled", &contiguous, &tiled, true);
    evict();
    let tiled_first = LayerRaster::new(first.clone());
    evict();
    assert!(
        !contiguous.same_bytes(&tiled_first),
        "contiguous self, tiled other: first byte"
    );
    same("contiguous/tiled, first", &contiguous, &tiled_first, false);
    same(
        "contiguous/tiled, last",
        &contiguous,
        &LayerRaster::new(last.clone()),
        false,
    );
    // (Contiguous, Contiguous) sharing the Arc.
    same(
        "contiguous/contiguous shared",
        &contiguous,
        &LayerRaster::shared(Arc::clone(&arc)),
        true,
    );
    // (Contiguous, Contiguous) equal without sharing.
    same(
        "contiguous/contiguous apart",
        &contiguous,
        &LayerRaster::shared(Arc::new(image.clone())),
        true,
    );
    same(
        "contiguous/contiguous, last",
        &contiguous,
        &contiguous_last,
        false,
    );
    same(
        "contiguous/contiguous, first",
        &contiguous,
        &LayerRaster::shared(Arc::new(first.clone())),
        false,
    );
    // Dimensions differ, every order.
    let small = corpus(w, h - 1);
    for (a, b) in [
        (tiled.clone(), LayerRaster::new(small.clone())),
        (tiled.clone(), LayerRaster::shared(Arc::new(small.clone()))),
        (contiguous.clone(), LayerRaster::new(small.clone())),
        (
            contiguous.clone(),
            LayerRaster::shared(Arc::new(small.clone())),
        ),
    ] {
        same("dimensions differ", &a, &b, false);
    }
}

#[test]
fn equals_image_compares_exactly_the_samples() {
    let _serial = serial();
    let (w, h) = (600, 400);
    let image = corpus(w, h);
    let n = (w * h * 4) as usize;
    let mut longer = image.as_raw().clone();
    longer.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    let longer = RgbaImage::from_raw(w, h, longer).expect("a longer buffer");
    assert!(longer.as_raw().len() > n);
    for (what, raster) in [
        ("tiled", LayerRaster::new(image.clone())),
        ("contiguous", LayerRaster::shared(Arc::new(image.clone()))),
    ] {
        evict();
        assert!(raster.equals_image(&image), "{what}: equal");
        assert!(
            !raster.equals_image(&flipped(&image, n - 1)),
            "{what}: last byte"
        );
        assert!(
            !raster.equals_image(&flipped(&image, 0)),
            "{what}: byte (0, 0)"
        );
        assert!(
            !raster.equals_image(&corpus(w - 1, h)),
            "{what}: dimensions"
        );
        assert!(raster.equals_image(&longer), "{what}: a longer buffer");
        assert_eq!(contiguous_view_bytes(), 0, "{what}: no view");
    }
}

// ---------------------------------------------------------------- sites --

/// A 1024×768 layer whose pixel `(x, y)` is `(W−1−x, y)`'s.
fn mirror_symmetric(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let m = x.min(w - 1 - x);
        Rgba([
            (m * 7 % 256) as u8,
            ((m + y) * 3 % 256) as u8,
            (y * 5 % 256) as u8,
            255,
        ])
    })
}

#[test]
fn flip_horizontal_compares_tiles() {
    let _serial = serial();
    // A centred ruler origin, so the flip moves no chrome either.
    let symmetric = DrDocument::from_pixels(mirror_symmetric(1024, 768))
        .set_ruler_origin(512.0, 384.0)
        .unwrap();
    for (what, doc) in [
        ("tiled", symmetric.clone()),
        ("twin", contiguous_twin(&symmetric)),
    ] {
        evict();
        let out = Out(unsafe { dr_doc_flip_horizontal(&doc as *const _) });
        assert!(out.0.is_null(), "{what}: a mirror-symmetric flip is NULL");
        // The one view is `geom.apply`'s read (a 13B-2c-3 residual): recorded.
        eprintln!(
            "13B-2c-2b flip_horizontal NULL ({what}): {} view bytes",
            contiguous_view_bytes()
        );
    }
    let doc = DrDocument::from_pixels(corpus(1024, 768));
    let twin = contiguous_twin(&doc);
    evict();
    let out = Out(unsafe { dr_doc_flip_horizontal(&doc as *const _) });
    eprintln!(
        "13B-2c-2b flip_horizontal corpus (tiled): {} view bytes",
        contiguous_view_bytes()
    );
    evict();
    let twin_out = Out(unsafe { dr_doc_flip_horizontal(&twin as *const _) });
    eprintln!(
        "13B-2c-2b flip_horizontal corpus (twin): {} view bytes",
        contiguous_view_bytes()
    );
    assert_eq!(
        bytes(&out.get().layers[0].pixels),
        bytes(&twin_out.get().layers[0].pixels),
        "flip equals the twin's"
    );
}

#[test]
fn set_layer_content_preserving_mask_compares_without_a_view() {
    let _serial = serial();
    let (w, h) = (1024, 768);
    let image = corpus(w, h);
    let doc = DrDocument::from_pixels(image.clone());
    let twin = contiguous_twin(&doc);
    let changed = flipped(&image, 5 * 4 + 1);
    let set = |d: &DrDocument, pixels: &RgbaImage| {
        Out(unsafe {
            dr_doc_set_layer_content_preserving_mask(
                d as *const _,
                0,
                pixels.as_raw().as_ptr(),
                w,
                h,
                0,
                0,
            )
        })
    };
    for (what, source) in [("tiled", &doc), ("twin", &twin)] {
        evict();
        let out = set(source, &image);
        assert!(out.0.is_null(), "{what}: same pixels and offset");
        assert_eq!(contiguous_view_bytes(), 0, "{what}: no view");
    }
    evict();
    let out = set(&doc, &changed);
    assert!(!out.0.is_null(), "one changed byte");
    assert_eq!(views_of(&doc), 0, "no view of the source");
    // The only view is `rebuilt`'s seed of the answer.
    assert_eq!(contiguous_view_bytes(), u64::from(w) * u64::from(h) * 4);
    let twin_out = set(&twin, &changed);
    assert_eq!(
        bytes(&out.get().layers[0].pixels),
        bytes(&twin_out.get().layers[0].pixels)
    );
    assert_eq!(bytes(&out.get().layers[0].pixels), changed.into_raw());
}

#[test]
fn a_transparency_locked_paint_restores_alpha_through_row_segments() {
    let _serial = serial();
    let (w, h) = (1024, 768);
    // Columns x < 300 are transparent (their colour kept).
    let mut image = corpus(w, h);
    for y in 0..h {
        for x in 0..300 {
            image.get_pixel_mut(x, y).0[3] = 0;
        }
    }
    let base = DrDocument::from_pixels(image);
    let locked = Out(unsafe { dr_doc_with_layer_locks(&base as *const _, 0, LOCK_TRANSPARENCY) });
    assert!(!locked.0.is_null());
    let doc = locked.get();
    let twin = contiguous_twin(doc);
    let dab = |x0: u32, y0: u32| {
        let mut overlay = vec![0_u8; w as usize * h as usize * 4];
        for y in y0..y0 + 100 {
            for x in x0..x0 + 100 {
                let at = (y as usize * w as usize + x as usize) * 4;
                overlay[at..at + 4].copy_from_slice(&[200, 40, 90, 255]);
            }
        }
        overlay
    };
    let paint = |d: &DrDocument, overlay: &[u8]| {
        Out(unsafe { dr_doc_painting_layer(d as *const _, 0, overlay.as_ptr(), w, h, 0, 1.0) })
    };
    let transparent = dab(50, 50);
    for (what, source) in [("tiled", doc), ("twin", &twin)] {
        let out = paint(source, &transparent);
        assert!(out.0.is_null(), "{what}: a dab over transparent pixels");
    }
    let opaque = dab(500, 300);
    evict();
    let out = paint(doc, &opaque);
    assert!(!out.0.is_null(), "a dab over opaque pixels");
    assert_eq!(
        contiguous_view_bytes(),
        u64::from(w) * u64::from(h) * 4,
        "exactly one view: the rebuilt seed"
    );
    let after = bytes(&out.get().layers[0].pixels);
    let before = bytes(&doc.layers[0].pixels);
    for (a, b) in after.chunks_exact(4).zip(before.chunks_exact(4)) {
        assert_eq!(a[3], b[3], "alpha frozen");
    }
    assert_ne!(after, before, "the colour changed");
    let twin_out = paint(&twin, &opaque);
    assert_eq!(after, bytes(&twin_out.get().layers[0].pixels), "twin");
}

fn warp(request: Value) -> WarpEdit {
    WarpEdit::parse(&request.to_string()).unwrap()
}

#[test]
fn a_reopened_warp_shares_an_equal_original() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let (w, h) = (600, 400);
    let doc = DrDocument::from_pixels(corpus(w, h));
    let original = bytes(&doc.layers[0].pixels);
    // An integral translation keeps the original as the layer's pixels.
    let translate = warp(
        json!({"op":"set","layer":0,"target":"pixels","columns":2,"rows":2,
        "displacements":vec![[7.0,-4.0];4],"filter":"bicubic"}),
    );
    let translated = doc
        .warp_edit(&translate, 2_000_000_000, 0, None)
        .unwrap()
        .unwrap();
    let path = dir.path().join("translated.rz");
    translated.save_native(path.to_str().unwrap()).unwrap();
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    let layer = &reopened.layers[0];
    let source = layer.warp.as_ref().unwrap().source_pixels.as_ref().unwrap();
    assert!(
        LayerRaster::ptr_eq(&layer.pixels, source),
        "an equal original shares the layer"
    );
    assert_eq!(bytes(source), original);
    // A real warp's original differs from the layer.
    let values: Vec<[f64; 2]> = (0..3)
        .flat_map(|y| {
            (0..3).map(move |x| {
                let (fx, fy) = (f64::from(x) / 2.0, f64::from(y) / 2.0);
                [
                    0.35 + 0.58 * fx - 0.31 * fy + 0.12 * fx * fy,
                    -0.27 + 0.33 * fy + 0.19 * fx - 0.17 * fx * fy,
                ]
            })
        })
        .collect();
    let bend = warp(
        json!({"op":"set","layer":0,"target":"pixels","columns":3,"rows":3,
        "displacements":values,"filter":"bicubic"}),
    );
    let warped = doc
        .warp_edit(&bend, 2_000_000_000, 0, None)
        .unwrap()
        .unwrap();
    let path = dir.path().join("warped.rz");
    warped.save_native(path.to_str().unwrap()).unwrap();
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    let layer = &reopened.layers[0];
    let source = layer.warp.as_ref().unwrap().source_pixels.as_ref().unwrap();
    assert!(!LayerRaster::ptr_eq(&layer.pixels, source));
    assert_eq!(bytes(source), original, "the original's bytes");
}

// ------------------------------------------------------- the conversion --

/// The 6000×4000 single-layer corpus document.
fn big() -> DrDocument {
    let (w, h) = phase4_corpus::DIMENSIONS[0];
    DrDocument::from_pixels(corpus(w, h))
}

/// `dr_doc_convert_to_profile_with` sRGB → Display P3 at default options
/// under `control` (a token, 0 = none): the answer and the error message.
fn convert(doc: &DrDocument, token: usize) -> (Out, String) {
    let p3 = IccProfile::display_p3();
    let icc = p3.bytes();
    let mut error: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_convert_to_profile_with(
            doc as *const _,
            icc.as_ptr(),
            icc.len(),
            ptr::null(),
            token as *const DrJobControl,
            ptr::null_mut(),
            &mut error,
        )
    };
    let message = if error.is_null() {
        String::new()
    } else {
        take_err_string(error)
    };
    (Out(out), message)
}

fn render(doc: &DrDocument) -> impl FnOnce(usize) -> (bool, String) + Send + '_ {
    move |token| {
        let (out, message) = convert(doc, token);
        (!out.0.is_null(), message)
    }
}

/// An uncancelled run's answer and its checkpoint count.
fn counted(doc: &DrDocument) -> (Out, u64) {
    let control = unsafe { dr_job_control_create() };
    let (out, message) = convert(doc, control as usize);
    assert!(!out.0.is_null(), "{message}");
    let count = unsafe { &*control }.checkpoints();
    unsafe { dr_job_control_free(control) };
    (out, count)
}

#[test]
fn a_conversion_polls_once_per_tile_and_cancels_inside_the_walk() {
    let _serial = serial();
    let doc = big();
    let twin = contiguous_twin(&doc);
    // Pre-cancelled: `begin`'s checkpoint.
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let (out, message) = convert(&doc, control as usize);
    assert!(out.0.is_null());
    assert_eq!(message, "operation cancelled");
    assert_eq!(unsafe { &*control }.checkpoints(), 1);
    unsafe { dr_job_control_free(control) };
    // Parked inside the tile walk (4…387).
    let parked = cancelled_parked_at(194, &|_, stage| stage == 1, render(&doc));
    eprintln!(
        "13B-2c-2b convert_to_profile tile walk: {}",
        parked_line(&parked)
    );
    // Uncancelled: the same bytes as the twin, 383 more checkpoints.
    let (tiled_out, tiled) = counted(&doc);
    let (twin_out, contiguous) = counted(&twin);
    assert_eq!(
        bytes(&tiled_out.get().layers[0].pixels),
        bytes(&twin_out.get().layers[0].pixels),
        "the tiled conversion equals the twin's"
    );
    assert_eq!(
        tiled - contiguous,
        383,
        "384 tile checks vs the contiguous arm's one ({tiled} vs {contiguous})"
    );
}

// ---------------------------------------------------------------- bench --

fn uptime() -> String {
    std::process::Command::new("uptime")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|e| format!("uptime unavailable: {e}"))
}

#[test]
#[ignore = "make cancel-bench: cancel-to-return of the conversion's tile walk"]
fn a_concurrently_cancelled_conversion_returns_within_one_tile() {
    let _serial = serial();
    let doc = big();
    let raster = &doc.layers[0].pixels;
    let c_tile = (0..3)
        .map(|_| {
            let start = Instant::now();
            raster.to_contiguous().unwrap();
            start.elapsed().as_secs_f64()
        })
        .fold(f64::INFINITY, f64::min)
        / 384.0;
    let p = probe(
        &|_, _, c| c >= 2,
        Trigger::After(50),
        &|_, _| true,
        render(&doc),
    );
    assert!(
        (4..=387).contains(&p.checkpoints),
        "the cancel returned outside the tile walk (checkpoint {})",
        p.checkpoints
    );
    let bound = RETURN_FLOOR_S + c_tile;
    eprintln!(
        "13B-2c-2b convert_to_profile (6000×4000 RGBA8, 384 tiles) cancel inside the tile walk \
         at checkpoint {}: {:.3} ms cancel-to-return (issued at {:.1} ms; {}); c_tile {:.3} µs; \
         bound T_f + c_tile = {:.3} ms (T_f 1 ms); ≈{:.2} tiles past the floor; {}; load: {}",
        p.checkpoints,
        p.latency.as_secs_f64() * 1e3,
        p.at.as_secs_f64() * 1e3,
        checkpoints_line(&p),
        c_tile * 1e6,
        bound * 1e3,
        past(p.latency, c_tile),
        verdict(p.latency, bound),
        uptime(),
    );
}
