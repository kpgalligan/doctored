//! 13A-latency: the byte regional render's RUN-MAJOR walk
//! (`doc_regional_run`) against the full projection. The oracle is
//! INDEPENDENT of the regional code: `dr_doc_flattened` cropped and
//! top-left-cell sampled here in plain index arithmetic, byte for byte.
//!
//! The fixtures sit where a run-major walk could go wrong: 256-pixel tile
//! seams in x and y (tiled rasters, and every raster re-adopted contiguous),
//! layers partly outside the window at negative and positive offsets, rows
//! cut mid-row at a multiple of 4096 root samples (any window over 4096
//! samples whose width does not divide 4096), runs longer than 4096 samples,
//! divisors 1, 2, 3 and 8 with window origins off their lattice, a masked
//! raster and a masked adjustment, Dissolve (keyed on the canvas position),
//! linear light, and a long-row stack that interleaves hoisted units with
//! per-sample ones (a clip unit, an isolated and a pass-through group). The
//! cadence case pins the loop's checkpoints row by row from the report log:
//! one per multiple of 4096 root samples plus the row's report, the
//! per-sample loop's count.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument, LayerKind, LayerRaster};
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_regional::*;
use image::{GrayImage, Luma, Rgba, RgbaImage};

use crate::common;
use common::{adjust_meta, img_dims, img_pixels, plasma, take_err_string};

// ------------------------------------------------------------- plumbing --

fn handle(doc: &DrDocument) -> *mut DrDocument {
    Box::into_raw(Box::new(doc.clone()))
}

fn full(doc: &DrDocument) -> Vec<u8> {
    let h = handle(doc);
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_flattened(h, 0, ptr::null(), &mut err) };
    unsafe { dr_doc_free(h) };
    assert!(!img.is_null(), "flattened: {}", take_err_string(err));
    let out = img_pixels(img);
    unsafe { dr_image_free(img) };
    out
}

/// Pixel (x + i·d, y + j·d) of the full projection for every output (i, j).
fn expected(
    full: &[u8],
    canvas_w: u32,
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
) -> (u32, u32, Vec<u8>) {
    let (ow, oh) = (w.div_ceil(d), h.div_ceil(d));
    let mut out = Vec::new();
    for j in 0..oh {
        for i in 0..ow {
            let cx = u64::from(x) + u64::from(i) * u64::from(d);
            let cy = u64::from(y) + u64::from(j) * u64::from(d);
            let at = ((cy * u64::from(canvas_w) + cx) * 4) as usize;
            out.extend_from_slice(&full[at..at + 4]);
        }
    }
    (ow, oh, out)
}

fn region(doc: &DrDocument, (x, y, w, h): (u32, u32, u32, u32), d: u32) -> (u32, u32, Vec<u8>) {
    let hd = handle(doc);
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_render_region(hd, x, y, w, h, d, u64::MAX, ptr::null(), &mut err) };
    unsafe { dr_doc_free(hd) };
    assert!(!img.is_null(), "region: {}", take_err_string(err));
    let (ow, oh) = img_dims(img);
    let px = img_pixels(img);
    unsafe { dr_image_free(img) };
    (ow, oh, px)
}

/// Every window at every divisor equals the oracle.
fn assert_windows(
    doc: &DrDocument,
    what: &str,
    windows: &[(u32, u32, u32, u32)],
    divisors: &[u32],
) {
    let reference = full(doc);
    for &window in windows {
        for &d in divisors {
            assert!(
                region(doc, window, d) == expected(&reference, doc.width, window, d),
                "{what}: window {window:?} divisor {d}"
            );
        }
    }
}

/// Every raster re-adopted CONTIGUOUS (`LayerRaster::shared`): the same
/// pixels through `row_segments`' single-run arm.
fn contiguous(doc: &DrDocument) -> DrDocument {
    let mut out = doc.clone();
    for layer in &mut out.layers {
        if layer.kind == LayerKind::Raster {
            let copy = layer.pixels.to_contiguous().expect("a copy");
            layer.pixels = LayerRaster::shared(Arc::new(copy));
        }
    }
    out
}

fn pattern(w: u32, h: u32, seed: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            (x * 37 + y * 11 + seed * 50) as u8,
            (200 + seed * 13).wrapping_sub(x * 5) as u8,
            (y * 23 + 7 + seed) as u8,
            (40 + (x * 13 + y * 29 + seed * 7) % 216) as u8,
        ])
    })
}

/// A plasma Background with a translucent band, so every mode sees both an
/// opaque and a translucent backdrop.
fn background(w: u32, h: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| {
        let c = plasma(x, y);
        let alpha = if y % 97 < 9 {
            (90 + (x * 20) % 120) as u8
        } else {
            255
        };
        Rgba([c[0], c[1], c[2], alpha])
    }))
}

fn with_raster(
    doc: &DrDocument,
    pixels: RgbaImage,
    offset: (i32, i32),
    blend: BlendMode,
    opacity: f32,
) -> DrDocument {
    let mut out = doc
        .adding_image_layer(doc.layers.len() - 1, pixels, "Raster")
        .expect("add layer");
    let top = out.layers.len() - 1;
    let layer = &mut out.layers[top];
    layer.offset = offset;
    layer.blend = blend;
    layer.opacity = opacity;
    out
}

/// An adjustment layer on top whose (unused) pixels span `pixels` at
/// `offset`: a masked adjustment's coverage is confined to that extent.
fn with_adjustment(
    doc: &DrDocument,
    op: &str,
    params: &str,
    blend: BlendMode,
    pixels: (u32, u32),
    offset: (i32, i32),
) -> DrDocument {
    let mut out = doc
        .adding_image_layer(doc.layers.len() - 1, RgbaImage::new(pixels.0, pixels.1), op)
        .expect("add layer");
    let top = out.layers.len() - 1;
    out.layers[top].meta = Some(adjust_meta(op, params));
    out.layers[top].blend = blend;
    out.layers[top].opacity = 0.8;
    out.layers[top].offset = offset;
    out
}

/// Layer `idx` masked by a gradient plane `w`×`h` at `offset`.
fn masked(doc: &DrDocument, idx: usize, (w, h): (u32, u32), offset: (i32, i32)) -> DrDocument {
    let mut out = doc.clone();
    let mut properties = MaskProperties::at(offset);
    properties.density = 0.8;
    let layer = &mut out.layers[idx];
    layer.mask = Some(Arc::new(GrayImage::from_fn(w, h, |x, y| {
        Luma([((x * 255) / (w - 1)) as u8 ^ (y * 9) as u8])
    })));
    layer.mask_enabled = true;
    layer.mask_properties = properties;
    out
}

// ------------------------------------------------------------- fixtures --

const SW: u32 = 600;
const SH: u32 = 300;

/// 600×300 (tile seams at x = 256, 512 and y = 256): rasters off the canvas
/// at negative and positive offsets, a masked raster, a Dissolve raster, an
/// unmasked Levels and a masked Multiply Hue/Saturation — every unit plain.
fn seams() -> DrDocument {
    let doc = with_raster(
        &background(SW, SH),
        pattern(400, 200, 1),
        (-40, -30),
        BlendMode::Multiply,
        0.8,
    );
    let doc = with_raster(
        &doc,
        pattern(350, 200, 2),
        (300, 150),
        BlendMode::Screen,
        0.6,
    );
    let doc = with_raster(
        &doc,
        pattern(300, 200, 3),
        (130, 40),
        BlendMode::Normal,
        0.9,
    );
    let doc = masked(&doc, 3, (280, 180), (150, 60));
    let doc = with_raster(
        &doc,
        pattern(200, 120, 4),
        (240, 200),
        BlendMode::Dissolve,
        0.5,
    );
    let doc = with_adjustment(
        &doc,
        "levels",
        "{\"black\":0.05,\"white\":0.95,\"gamma\":1.2}",
        BlendMode::Normal,
        (1, 1),
        (0, 0),
    );
    let doc = with_adjustment(
        &doc,
        "hue_saturation",
        "{\"hue\":50,\"saturation\":0.4}",
        BlendMode::Multiply,
        (200, 150),
        (220, 100),
    );
    masked(&doc, 6, (190, 140), (230, 110))
}

const LW: u32 = 5000;
const LH: u32 = 4;

/// 5000×4, every row past a 4096-sample cut: a clip unit (a raster base and a
/// clipped raster member), a plain raster at a negative offset, an isolated
/// group and a pass-through group of rasters, then a masked Invert and an
/// unmasked Levels — hoisted and per-sample units interleaved.
fn long_rows() -> DrDocument {
    let doc = with_raster(
        &background(LW, LH),
        pattern(3000, 3, 5),
        (900, 1),
        BlendMode::Overlay,
        0.9,
    );
    let mut doc = with_raster(
        &doc,
        pattern(2500, 4, 6),
        (2000, 0),
        BlendMode::Multiply,
        0.7,
    );
    doc.layers[2].clipped = true;
    let doc = with_raster(&doc, pattern(4800, 4, 7), (-300, 0), BlendMode::Screen, 0.6);
    let doc = with_raster(&doc, pattern(1800, 4, 8), (3000, 0), BlendMode::Normal, 0.8);
    let doc = with_raster(&doc, pattern(1500, 3, 9), (3900, 1), BlendMode::Darken, 0.9);
    let (mut doc, isolated, ..) = doc.group_layers(&[4, 5], "Isolated").expect("group");
    doc.layers[isolated].blend = BlendMode::Normal;
    let doc = with_raster(
        &doc,
        pattern(4096, 4, 10),
        (500, 0),
        BlendMode::SoftLight,
        0.7,
    );
    let top = doc.layers.len() - 1;
    let (doc, through, ..) = doc.group_layers(&[top], "Through").expect("group");
    assert_eq!(doc.layers[through].blend, BlendMode::PassThrough);
    let doc = with_adjustment(&doc, "invert", "{}", BlendMode::Normal, (3500, 4), (700, 0));
    let top = doc.layers.len() - 1;
    let doc = masked(&doc, top, (3400, 4), (800, 0));
    with_adjustment(
        &doc,
        "levels",
        "{\"black\":0.1,\"gamma\":1.6}",
        BlendMode::Normal,
        (1, 1),
        (0, 0),
    )
}

// ---------------------------------------------------------------- tests --

#[test]
fn tile_seams_offsets_masks_and_divisors_match_the_projection() {
    let windows = [
        (0, 0, SW, SH),
        (250, 250, 20, 20),
        (255, 0, 3, SH),
        (37, 41, 513, 251),
        (511, 1, 89, 299),
        (0, 255, SW, 2),
        (1, 7, 599, 293),
    ];
    let doc = seams();
    for (what, doc) in [("tiled", doc.clone()), ("contiguous", contiguous(&doc))] {
        assert_windows(&doc, what, &windows, &[1, 2, 3, 8]);
    }
    let linear = doc
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .expect("a policy change");
    assert_ne!(full(&linear), full(&doc));
    assert_windows(&linear, "linear light", &windows, &[1, 3]);
}

#[test]
fn a_long_row_mixed_stack_matches_at_every_run_cut() {
    let doc = long_rows();
    let windows = [
        (0, 0, LW, LH),
        (1, 0, LW - 1, LH),
        (3, 1, LW - 4, 3),
        (4095, 0, 905, LH),
    ];
    assert_windows(&doc, "long rows", &windows, &[1, 2, 3]);
    assert_windows(
        &contiguous(&doc),
        "long rows, contiguous",
        &windows,
        &[1, 3],
    );
}

/// The checkpoint count each row's progress report landed at, from the
/// report log of one render of `window` at divisor `d`.
fn row_checkpoints(doc: &DrDocument, (x, y, w, h): (u32, u32, u32, u32), d: u32) -> Vec<u64> {
    let hd = handle(doc);
    let control = unsafe { dr_job_control_create() };
    unsafe { &*control }.record_reports();
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_render_region(hd, x, y, w, h, d, u64::MAX, control, &mut err) };
    assert!(!img.is_null(), "region: {}", take_err_string(err));
    let rows = unsafe { &*control }
        .reports()
        .iter()
        .filter(|report| report.stage == 6)
        .map(|report| report.checkpoints)
        .collect();
    unsafe {
        dr_image_free(img);
        dr_job_control_free(control);
        dr_doc_free(hd);
    }
    rows
}

#[test]
fn the_loop_polls_once_per_4096_root_samples_as_the_per_sample_loop_did() {
    // Between two row reports the loop polls once at every multiple of 4096
    // root samples inside the row, and the second report once more.
    for (doc, window, d) in [
        (seams(), (0, 0, 300, 300), 1),
        (seams(), (5, 3, 595, 297), 2),
        (long_rows(), (0, 0, LW, 3), 1),
        (long_rows(), (2, 0, LW - 2, LH), 1),
    ] {
        let ow = u64::from(window.2.div_ceil(d));
        let rows = row_checkpoints(&doc, window, d);
        assert_eq!(rows.len() as u64, u64::from(window.3.div_ceil(d)));
        for j in 1..rows.len() as u64 {
            let (start, end) = (j * ow, (j + 1) * ow);
            let multiples = (start..end).filter(|k| k % 4096 == 0).count() as u64;
            assert_eq!(
                rows[j as usize] - rows[j as usize - 1],
                multiples + 1,
                "{window:?}/{d}: row {j}"
            );
        }
    }
}
