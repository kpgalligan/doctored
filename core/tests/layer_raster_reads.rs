//! Black-box suite for 13B-2c-1: the allocation-free in-place reads
//! (`TilePlane::row_segments` / `sample_at` and their `LayerRaster`
//! wrappers) and the compositor, regional tree and content-box scan that now
//! read tiles through them. Oracles are independent of the implementation:
//! row runs are checked against hand slices of the source samples, the
//! compositor against a CONTIGUOUS twin of the same document (every raster
//! re-adopted through `LayerRaster::shared`, so the pre-13B-2c-1 single-run
//! reads), and regional windows against the full-frame projection cropped in
//! plain index arithmetic (`regional_document.rs`'s oracle, re-implemented).
//!
//! The view cache is process-wide, so every test holds `serial()`: a count
//! of `contiguous_views` must not see another test's rasters.

use crate::phase4_corpus;
use crate::serial;

use crate::common;

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;

use common::{img_dims, img_pixels, styled, take_err_string};
use doctored_core::doc::{BlendMode, DrDocument, LayerKind, LayerRaster, MaskKind};
use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_regional::dr_doc_render_region;
use doctored_core::layer_raster::contiguous_view_bytes;
use doctored_core::tile_store::TilePlane;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::Value;

use crate::resource_report;

// ------------------------------------------------------------- plumbing --

fn report(document: &DrDocument) -> Value {
    let handles = [document as *const DrDocument];
    resource_report::report(&handles, &[], &[], &[], 0, 0).expect("a resource report")
}

fn evict() {
    resource_report::evict(&[]).expect("an eviction report");
}

fn category(report: &Value, category: &str, field: &str) -> u64 {
    report["categories"][category][field].as_u64().unwrap_or(0)
}

fn handle(doc: &DrDocument) -> *mut DrDocument {
    Box::into_raw(Box::new(doc.clone()))
}

/// The full projection through `dr_doc_flattened`.
fn full(doc: &DrDocument) -> Vec<u8> {
    let h = handle(doc);
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_flattened(h, 0, std::ptr::null(), &mut err) };
    assert!(!img.is_null(), "flattened: {}", take_err_string(err));
    let out = img_pixels(img);
    unsafe {
        dr_image_free(img);
        dr_doc_free(h);
    }
    out
}

/// The independent oracle: pixel (x + i·d, y + j·d) of the full projection
/// for every output (i, j), row-major.
fn expected(
    full: &[u8],
    canvas_w: u32,
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
) -> (u32, u32, Vec<u8>) {
    let ow = w.div_ceil(d);
    let oh = h.div_ceil(d);
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
    assert!(!img.is_null(), "render region: {}", take_err_string(err));
    let (ow, oh) = img_dims(img);
    let px = img_pixels(img);
    unsafe { dr_image_free(img) };
    (ow, oh, px)
}

// ------------------------------------------------------------- fixtures --

fn samples(image: &RgbaImage) -> Vec<[u8; 4]> {
    image
        .as_raw()
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

fn corpus(width: u32, height: u32) -> RgbaImage {
    phase4_corpus::image(width, height)
}

/// Translucent, varying in every channel.
fn translucent(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            (x * 7 + y * 3) as u8,
            (x * 13 + y * 29) as u8,
            ((x ^ y) * 5) as u8,
            (40 + (x * 11 + y * 17) % 200) as u8,
        ])
    })
}

/// A ROW-VARYING mask: a mask index that collapses to row 0 or to a run
/// offset reads a different byte.
fn row_varying(width: u32, height: u32) -> GrayImage {
    GrayImage::from_fn(width, height, |x, y| Luma([(y * 7 + x) as u8]))
}

fn with_raster(doc: &DrDocument, pixels: RgbaImage, offset: (i32, i32)) -> DrDocument {
    let mut out = doc
        .adding_image_layer(doc.layers.len() - 1, pixels, "Raster")
        .expect("add layer");
    let top = out.layers.len() - 1;
    out.layers[top].offset = offset;
    out
}

/// `add_mask` on the top layer, then its plane replaced by `row_varying`.
fn masked_top(doc: &DrDocument) -> DrDocument {
    let top = doc.layers.len() - 1;
    let mut out = doc.add_mask(top, MaskKind::RevealAll).expect("add mask");
    let (w, h) = out.layers[top].mask.as_ref().expect("a mask").dimensions();
    out.layers[top].mask = Some(Arc::new(row_varying(w, h)));
    out.layers[top].mask_enabled = true;
    out
}

fn clipped_top(mut doc: DrDocument) -> DrDocument {
    let top = doc.layers.len() - 1;
    doc.layers[top].clipped = true;
    doc
}

/// Document A (600×300): an opaque base; a 300×200 layer at (−100, −50); a
/// 1×1 layer at (599, 299); a 300×100 layer entirely RIGHT of the canvas at
/// (700, 0); a 300×100 layer entirely LEFT at (−300, 100); a masked 400×250
/// layer at (150, 40).
fn document_a() -> DrDocument {
    let doc = DrDocument::from_pixels(corpus(600, 300));
    let doc = with_raster(&doc, translucent(300, 200), (-100, -50));
    let doc = with_raster(&doc, translucent(1, 1), (599, 299));
    let doc = with_raster(&doc, translucent(300, 100), (700, 0));
    let doc = with_raster(&doc, translucent(300, 100), (-300, 100));
    let doc = with_raster(&doc, translucent(400, 250), (150, 40));
    masked_top(&doc)
}

const STYLED: &str = "{\"fill_opacity\":0.8,\
    \"blend_if\":{\"channel\":\"gray\",\"this_layer\":[10,60,180,240],\"underlying\":[0,40,200,255]},\
    \"effects\":[{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2},{\"type\":\"stroke\",\"size\":3}]}";

const KNOCKOUT: &str = "{\"fill_opacity\":0.5,\"knockout\":\"deep\"}";

const GROUP_STYLE: &str = "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":5,\"size\":2},\
    {\"type\":\"stroke\",\"size\":3},\
    {\"type\":\"color_overlay\",\"blend\":\"multiply\",\"color\":\"#80c0ff\"}]}";

/// Document B (600×300): an opaque base (0); a styled raster with effects
/// and Blend If carrying a mask (1) with a clip member entirely right of the
/// canvas (2); a knockout-styled raster carrying a mask (3) with a clip
/// member entirely right of the canvas (4); two rasters (5, 6) grouped into
/// an isolated styled group (7).
fn document_b() -> DrDocument {
    let doc = DrDocument::from_pixels(corpus(600, 300));
    let doc = masked_top(&with_raster(&doc, translucent(330, 180), (40, 30)));
    let doc = styled(&doc, 1, STYLED);
    let doc = clipped_top(with_raster(&doc, translucent(300, 100), (700, 0)));
    let doc = masked_top(&with_raster(&doc, translucent(280, 200), (230, 60)));
    let doc = styled(&doc, 3, KNOCKOUT);
    let doc = clipped_top(with_raster(&doc, translucent(300, 100), (700, 0)));
    let doc = with_raster(&doc, translucent(270, 120), (300, 150));
    let doc = with_raster(&doc, translucent(200, 90), (-20, 220));
    let (mut doc, g, ..) = doc.group_layers(&[5, 6], "Group").expect("group");
    doc.layers[g].blend = BlendMode::Normal;
    styled(&doc, g, GROUP_STYLE)
}

/// Every raster re-adopted as CONTIGUOUS (`LayerRaster::shared` of an owned
/// copy): the same pixels, read through the single-run arm.
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

/// Every raster of `doc` is tiled (a tiled raster shares all its tiles with
/// itself; a contiguous one answers 0) and none of `twin`'s is.
fn assert_representations(doc: &DrDocument, twin: &DrDocument) {
    for (i, (a, b)) in doc.layers.iter().zip(&twin.layers).enumerate() {
        if a.kind == LayerKind::Raster {
            assert!(a.pixels.shared_tile_count(&a.pixels) > 0, "layer {i} tiled");
            assert_eq!(b.pixels.shared_tile_count(&b.pixels), 0, "twin {i}");
        }
    }
}

// ------------------------------------------------------- row_segments --

const ROWS: [u32; 5] = [0, 255, 256, 3999, 4000];
const SPANS: [(u32, u32); 8] = [
    (0, 1),
    (255, 257),
    (511, 513),
    (0, 6000),
    (5999, 6000),
    (300, 300),
    (0, 7000),
    (5990, 7000),
];

/// The hand slice `row_segments` must answer, by `TilePlane`'s three rules.
fn hand<S: Copy>(source: &[S], (width, height): (u32, u32), y: u32, x0: u32, x1: u32) -> Vec<S> {
    let x1 = x1.min(width);
    if y >= height || x0 >= x1 {
        return Vec::new();
    }
    let row = y as usize * width as usize;
    source[row + x0 as usize..row + x1 as usize].to_vec()
}

/// Runs adjoin from `x0`, each inside one tile column when `tiled`, and
/// concatenate to the hand slice; answers the concatenation.
fn concatenated<'a, S: Copy + PartialEq + std::fmt::Debug + 'a>(
    runs: impl Iterator<Item = (u32, &'a [S])>,
    x0: u32,
    tiled: bool,
) -> Vec<S> {
    let mut next = x0;
    let mut out = Vec::new();
    for (x, run) in runs {
        assert_eq!(x, next, "runs adjoin");
        assert!(!run.is_empty(), "no empty run");
        if tiled {
            let last = x + run.len() as u32 - 1;
            assert_eq!(x / 256, last / 256, "one run per tile column");
        }
        next = x + run.len() as u32;
        out.extend_from_slice(run);
    }
    out
}

#[test]
fn row_segments_concatenate_to_the_hand_slice_on_every_plane_and_arm() {
    let _serial = serial();
    let (big_w, big_h) = phase4_corpus::DIMENSIONS[0];
    for (width, height) in [(big_w, big_h), (257, 257), (1, 1)] {
        let image = corpus(width, height);
        let source = samples(&image);
        let plane = TilePlane::from_contiguous(width, height, &source, None).unwrap();
        let gray: Vec<u16> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x * 31 + y * 7) as u16))
            .collect();
        let gray_plane = TilePlane::from_contiguous(width, height, &gray, None).unwrap();
        let tiled = LayerRaster::new(image.clone());
        let contiguous = LayerRaster::shared(Arc::new(image));
        assert!(tiled.shared_tile_count(&tiled) > 0);
        assert_eq!(contiguous.shared_tile_count(&contiguous), 0);
        for y in ROWS {
            for (x0, x1) in SPANS {
                let want = hand(&source, (width, height), y, x0, x1);
                let what = format!("{width}×{height} row {y} [{x0},{x1})");
                let store = concatenated(plane.row_segments(y, x0, x1), x0, true);
                assert_eq!(store, want, "store {what}");
                let a = concatenated(tiled.row_segments(y, x0, x1), x0, true);
                assert_eq!(a, want, "tiled arm {what}");
                let b = concatenated(contiguous.row_segments(y, x0, x1), x0, false);
                assert_eq!(b, want, "contiguous arm {what}");
                assert!(contiguous.row_segments(y, x0, x1).count() <= 1);
                let want16 = hand(&gray, (width, height), y, x0, x1);
                let got16 = concatenated(gray_plane.row_segments(y, x0, x1), x0, true);
                assert_eq!(got16, want16, "u16 {what}");
            }
        }
    }
}

#[test]
fn sample_at_answers_the_source_sample_or_none_outside() {
    let _serial = serial();
    let (big_w, big_h) = phase4_corpus::DIMENSIONS[0];
    let points = [
        (0, 0),
        (255, 255),
        (256, 256),
        (5999, 3999),
        (6000, 0),
        (0, 4000),
        (u32::MAX, 0),
        (0, u32::MAX),
    ];
    for (width, height) in [(big_w, big_h), (257, 257), (1, 1)] {
        let image = corpus(width, height);
        let source = samples(&image);
        let plane = TilePlane::from_contiguous(width, height, &source, None).unwrap();
        let tiled = LayerRaster::new(image.clone());
        let contiguous = LayerRaster::shared(Arc::new(image));
        for (x, y) in points {
            let want =
                (x < width && y < height).then(|| source[y as usize * width as usize + x as usize]);
            let what = format!("{width}×{height} at ({x}, {y})");
            assert_eq!(plane.sample_at(x, y), want, "store {what}");
            assert_eq!(tiled.sample_at(x, y), want, "tiled arm {what}");
            assert_eq!(contiguous.sample_at(x, y), want, "contiguous arm {what}");
        }
    }
}

// ------------------------------------------------------- compositor --

#[test]
fn the_tiled_projection_equals_its_contiguous_twin_byte_for_byte() {
    let _serial = serial();
    for (what, doc) in [("A", document_a()), ("B", document_b())] {
        let twin = contiguous_twin(&doc);
        assert_representations(&doc, &twin);
        let tiled = doc.flattened().expect("a projection");
        let contiguous = twin.flattened().expect("a projection");
        assert_eq!(tiled.dimensions(), (600, 300), "{what}");
        assert!(
            tiled == contiguous,
            "document {what}: tiled != contiguous twin"
        );
    }
}

#[test]
fn regional_windows_equal_the_cropped_projection_and_the_twin() {
    let _serial = serial();
    let windows = [
        ((200, 160, 120, 120), 1),
        ((0, 0, 600, 300), 1),
        ((599, 299, 1, 1), 1),
        ((100, 100, 300, 150), 2),
    ];
    for (what, doc) in [("A", document_a()), ("B", document_b())] {
        let twin = contiguous_twin(&doc);
        let reference = full(&doc);
        for (window, d) in windows {
            let got = region(&doc, window, d);
            assert_eq!(
                got,
                expected(&reference, doc.width, window, d),
                "{what}: window {window:?} divisor {d}"
            );
            assert_eq!(got, region(&twin, window, d), "{what}: twin {window:?}/{d}");
        }
    }
}

#[test]
fn a_projection_and_a_window_leave_no_view_behind() {
    let _serial = serial();
    let doc = document_b();
    evict();
    assert_eq!(contiguous_view_bytes(), 0);
    let first = doc.flattened().expect("a projection");
    let window = doc
        .render_region(
            doctored_core::RegionRequest {
                x: 100,
                y: 50,
                width: 300,
                height: 200,
                preview_divisor: 1,
            },
            u64::MAX,
            None,
        )
        .expect("a window");
    assert_eq!(window.dimensions(), (300, 200));
    assert_eq!(first.get_pixel(100, 50), window.get_pixel(0, 0));
    assert_eq!(contiguous_view_bytes(), 0);
    let measured = report(&doc);
    assert_eq!(
        category(&measured, "contiguous_views", "allocation_count"),
        0
    );
    assert_eq!(measured["contiguous_view_bytes"].as_u64(), Some(0));
}

#[test]
fn layer_bounds_reads_tiles_and_leaves_no_view() {
    let _serial = serial();
    let image = RgbaImage::from_fn(700, 500, |x, y| {
        let inside = (250..560).contains(&x) && (200..330).contains(&y);
        Rgba([
            (x * 3) as u8,
            (y * 5) as u8,
            90,
            if inside { 180 } else { 0 },
        ])
    });
    let doc = DrDocument::from_pixels(image);
    assert!(
        doc.layers[0]
            .pixels
            .shared_tile_count(&doc.layers[0].pixels)
            > 0
    );
    evict();
    assert_eq!(doc.layer_bounds(0), Some((250, 200, 310, 130)));
    assert_eq!(contiguous_view_bytes(), 0);
    let measured = report(&doc);
    assert_eq!(
        category(&measured, "contiguous_views", "allocation_count"),
        0
    );
}
