//! Black-box suite for 13B-2c-3a: the byte persistence, export, preservation,
//! thumbnail, canvas-image, sampling-plate and promotion reads go through the
//! tiles (`runs` / `row_segments` / a transient `to_contiguous`), so none
//! leaves a contiguous view behind, and a reopened placed layer whose bytes
//! equal its asset shares the asset's tiles again.
//!
//! Oracles are independent of the implementation: every stream, report,
//! image and sample is compared byte for byte with the same call on a
//! CONTIGUOUS twin (every raster re-adopted through `LayerRaster::shared`,
//! with the retained-source and WARP bindings re-pointed so neither writer
//! drops them); promoted samples are the test's own `257·byte`; expected
//! sharing is the tile count of the asset's dimensions and expected report
//! figures are dimension products.
//!
//! Order in every test: build the fixture and its twin (fixtures seed views),
//! `evict()`, assert no view, run the measured call, then check no view is
//! left. The post-call view and sharing checks are COLLECTED (`Checks`) and
//! reported together at the end of each test, so a run against a library
//! without these reads names every failing check rather than the first.
//!
//! The view cache is process-wide, so every test holds `serial()`.

use crate::phase4_corpus;
use crate::serial;

use crate::warp_fixture;

use crate::common;

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use common::{cpath, fill_request, take_err_string};
use doctored_core::doc::{DrDocument, LayerKind, LayerRaster};
use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_content_aware::dr_doc_content_aware_fill_edit;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_conversion::dr_doc_promote_structure;
use doctored_core::ffi_preservation::dr_doc_flat_save_losses;
use doctored_core::ffi_psd::dr_doc_save_psd;
use doctored_core::ffi_retained_source::dr_doc_mixed_source_edit;
use doctored_core::layer_raster::contiguous_view_bytes;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::retained_source::{SourceAsset, SourcePixels};
use doctored_core::warp::WarpEdit;
use doctored_core::DrImage;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::tempdir;

use crate::resource_report;

const EDGE: u32 = 256;
const DAB: [u8; 4] = [200, 40, 90, 255];
const PSD_LAYERS: i32 = 0;
const FORMAT_PNG: i32 = 0;
const FORMAT_JPEG: i32 = 1;
const FORMAT_GIF: i32 = 4;

// ------------------------------------------------------------- plumbing --

struct Doc(*mut DrDocument);

impl Drop for Doc {
    fn drop(&mut self) {
        unsafe { dr_doc_free(self.0) };
    }
}

impl Doc {
    fn checked(value: *mut DrDocument) -> Self {
        assert!(!value.is_null());
        Self(value)
    }

    fn owned(document: DrDocument) -> Self {
        Self(Box::into_raw(Box::new(document)))
    }

    fn from_image(image: &RgbaImage) -> Self {
        Self::owned(DrDocument::from_pixels(image.clone()))
    }

    fn get(&self) -> &DrDocument {
        unsafe { &*self.0 }
    }

    fn raster(&self, idx: usize) -> &LayerRaster {
        &self.get().layers[idx].pixels
    }

    /// The byte asset of layer `idx`'s retained source.
    fn asset(&self, idx: usize) -> &LayerRaster {
        self.get().layers[idx]
            .retained_source
            .as_ref()
            .expect("a retained source")
            .asset
            .rgba8()
            .expect("a byte asset")
    }

    fn twin(&self) -> Self {
        Self::owned(contiguous_twin(self.get()))
    }

    fn paint(&self, idx: usize, overlay: &[u8]) -> Self {
        let d = self.get();
        Self::checked(unsafe {
            dr_doc_painting_layer(self.0, idx, overlay.as_ptr(), d.width, d.height, 0, 1.0)
        })
    }

    /// `dr_doc_mixed_source_edit` (`source16` NULL) with an optional input document.
    fn source_edit(&self, input: Option<&Doc>, request: Value) -> Self {
        let request = CString::new(request.to_string()).unwrap();
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_doc_mixed_source_edit(
                self.0,
                input.map_or(ptr::null(), |d| d.0 as *const _),
                ptr::null(),
                request.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        };
        assert!(error.is_null(), "{}", take_err_string(error));
        Self::checked(out)
    }

    fn place(&self, input: &Doc) -> Self {
        self.source_edit(
            Some(input),
            json!({"op":"place","source_id":"asset","name":"Placed","x":0,"y":0}),
        )
    }

    /// `dr_doc_save_native` to `path`, answering the stream.
    fn save(&self, path: &Path) -> Vec<u8> {
        let c = cpath(path);
        let mut error: *mut c_char = ptr::null_mut();
        let saved = unsafe { dr_doc_save_native(self.0, c.as_ptr(), &mut error) };
        assert!(saved, "save: {}", take_err_string(error));
        std::fs::read(path).expect("the saved file")
    }

    /// `dr_doc_save_psd` (layers mode, the document composited here).
    fn save_psd(&self, path: &Path) -> Vec<u8> {
        let c = cpath(path);
        let mut error: *mut c_char = ptr::null_mut();
        let saved =
            unsafe { dr_doc_save_psd(self.0, ptr::null(), c.as_ptr(), 0, PSD_LAYERS, &mut error) };
        assert!(saved, "save_psd: {}", take_err_string(error));
        std::fs::read(path).expect("the saved PSD")
    }

    fn open(path: &Path) -> Self {
        let c = cpath(path);
        let mut error: *mut c_char = ptr::null_mut();
        let out = unsafe { dr_doc_open(c.as_ptr(), &mut error) };
        assert!(!out.is_null(), "open: {}", take_err_string(error));
        Self(out)
    }
}

unsafe fn json_result(result: *mut c_char, error: *mut c_char) -> Value {
    assert!(error.is_null(), "{}", take_err_string(error));
    assert!(!result.is_null());
    let value = serde_json::from_slice(unsafe { CStr::from_ptr(result) }.to_bytes()).unwrap();
    unsafe { dr_string_free(result) };
    value
}

fn report(documents: &[&Doc]) -> Value {
    let handles: Vec<*const DrDocument> = documents.iter().map(|d| d.0 as *const _).collect();
    resource_report::report(&handles, &[], &[], &[], 0, 0).expect("a resource report")
}

fn evict() {
    resource_report::evict(&[]).expect("an eviction report");
}

fn category(report: &Value, category: &str, field: &str) -> u64 {
    report["categories"][category][field].as_u64().unwrap_or(0)
}

/// The `contiguous_views` allocation count of `document`'s own planes.
fn views_of(document: &Doc) -> u64 {
    category(&report(&[document]), "contiguous_views", "allocation_count")
}

fn tiles(width: u32, height: u32) -> usize {
    (width.div_ceil(EDGE) * height.div_ceil(EDGE)) as usize
}

/// Evicts every view and asserts none is left: the measured call starts
/// from an empty cache.
fn cold() {
    evict();
    assert_eq!(contiguous_view_bytes(), 0, "evicted before the call");
}

/// The post-call checks, collected so every failing one is named.
#[derive(Default)]
struct Checks(Vec<String>);

impl Checks {
    /// No view left by `what`, process-wide and on every held document.
    fn no_view(&mut self, what: &str, documents: &[&Doc]) {
        let bytes = contiguous_view_bytes();
        let held: Vec<u64> = documents.iter().map(|d| views_of(d)).collect();
        if bytes != 0 || held.iter().any(|&n| n != 0) {
            self.0.push(format!(
                "{what}: contiguous_view_bytes {bytes}, views_of {held:?}"
            ));
        }
    }

    fn check(&mut self, what: &str, holds: bool) {
        if !holds {
            self.0.push(what.to_string());
        }
    }

    fn finish(self) {
        assert!(
            self.0.is_empty(),
            "{} failing check(s):\n{}",
            self.0.len(),
            self.0.join("\n")
        );
    }
}

/// Every raster re-adopted as CONTIGUOUS (`LayerRaster::shared` of an owned
/// copy), binding-aware: a retained source's `rendered` and a WARP binding's
/// `rendered_pixels` re-point at the twin raster (so `effective_source` /
/// `effective_warp` still hold), each byte asset Arc is re-adopted ONCE (a
/// second pointer under the same identity would be refused by the writer's
/// table) and each WARP original is re-adopted contiguously.
fn contiguous_twin(doc: &DrDocument) -> DrDocument {
    let contiguous = |raster: &LayerRaster| {
        LayerRaster::shared(Arc::new(raster.to_contiguous().expect("a copy")))
    };
    let mut out = doc.clone();
    let mut memo: Vec<(*const SourceAsset, Arc<SourceAsset>)> = Vec::new();
    for layer in &mut out.layers {
        if layer.kind != LayerKind::Raster {
            continue;
        }
        layer.pixels = contiguous(&layer.pixels);
        if let Some(source) = layer.retained_source.as_mut() {
            source.rendered = layer.pixels.clone();
            let key = Arc::as_ptr(&source.asset);
            let adopted = match memo.iter().find(|(k, _)| *k == key) {
                Some((_, adopted)) => adopted.clone(),
                None => {
                    let mut asset = (*source.asset).clone();
                    let replaced = match &asset.pixels {
                        SourcePixels::Rgba8 { pixels, profile } => Some(SourcePixels::Rgba8 {
                            pixels: contiguous(pixels),
                            profile: profile.clone(),
                        }),
                        SourcePixels::Rgba16(_) | SourcePixels::Rgba32F(_) => None,
                    };
                    if let Some(pixels) = replaced {
                        asset.pixels = pixels;
                    }
                    let adopted = Arc::new(asset);
                    memo.push((key, adopted.clone()));
                    adopted
                }
            };
            source.asset = adopted;
        }
        if let Some(warp) = layer.warp.as_mut() {
            if warp.rendered_pixels.is_some() {
                warp.rendered_pixels = Some(layer.pixels.clone());
            }
            if let Some(original) = &warp.source_pixels {
                warp.source_pixels = Some(contiguous(original));
            }
        }
    }
    out
}

fn bytes(raster: &LayerRaster) -> Vec<u8> {
    raster.to_contiguous().expect("a copy").into_raw()
}

fn has_tag(stream: &[u8], tag: &[u8; 4]) -> bool {
    stream.windows(4).any(|w| w == tag)
}

/// An owned `DrImage` answer's pixels (freed here); `None` for NULL.
fn image_bytes(image: *mut DrImage) -> Option<(u32, u32, Vec<u8>)> {
    if image.is_null() {
        return None;
    }
    let out = unsafe {
        let (w, h) = (dr_image_width(image), dr_image_height(image));
        let raw =
            std::slice::from_raw_parts(dr_image_pixels_rgba(image), w as usize * h as usize * 4);
        (w, h, raw.to_vec())
    };
    unsafe { dr_image_free(image) };
    Some(out)
}

fn corpus() -> RgbaImage {
    let (width, height) = phase4_corpus::DIMENSIONS[0];
    phase4_corpus::image(width, height)
}

fn patterned(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            (x * 7 + y) as u8,
            (y * 13 + x * 3) as u8,
            (x ^ (y * 5)) as u8,
            (x + y * 2) as u8 | 1,
        ])
    })
}

/// A canvas-frame premultiplied overlay holding `value` over `(x, y, w, h)`
/// (inside the canvas).
fn overlay(canvas: (u32, u32), (x, y, w, h): (u32, u32, u32, u32), value: [u8; 4]) -> Vec<u8> {
    let mut out = vec![0_u8; canvas.0 as usize * canvas.1 as usize * 4];
    for cy in y..y + h {
        for cx in x..x + w {
            let at = (cy as usize * canvas.0 as usize + cx as usize) * 4;
            out[at..at + 4].copy_from_slice(&value);
        }
    }
    out
}

/// A 300×200 source placed at (0, 0) over a 640×480 host.
fn placed_case() -> (Doc, RgbaImage) {
    let host = Doc::from_image(&patterned(640, 480));
    let source = patterned(300, 200);
    let input = Doc::from_image(&source);
    evict();
    let placed = host.place(&input);
    (placed, source)
}

/// The warp fixture under a 3×3 bend.
fn bend() -> Doc {
    let request = warp_fixture::edit(warp_fixture::field(3, 3, "bicubic"));
    let bent = warp_fixture::byte_document()
        .warp_edit(&request, warp_fixture::LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert!(bent.layers[0]
        .warp
        .as_ref()
        .is_some_and(|w| w.source_pixels.is_some()));
    Doc::owned(bent)
}

/// `with` added above the top layer at `offset`.
fn with_layer(doc: DrDocument, image: RgbaImage, offset: (i32, i32), name: &str) -> DrDocument {
    let top = doc.layers.len() - 1;
    let mut out = doc.adding_image_layer(top, image, name).expect("a layer");
    out.layers[top + 1].offset = offset;
    out
}

// ---------------------------------------------------------------- (a) --

#[test]
fn a_native_save_reads_tiles_and_leaves_no_view() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let mut checks = Checks::default();
    let corpus_doc = Doc::from_image(&corpus());
    let (placed, _) = placed_case();
    let bent = bend();
    for (name, doc, tag) in [
        ("corpus", &corpus_doc, None),
        ("placed", &placed, Some(b"RSRC")),
        ("bend", &bent, Some(b"WARP")),
    ] {
        let twin = doc.twin();
        cold();
        let saved = doc.save(&dir.path().join(format!("{name}.rz")));
        checks.no_view(&format!("{name} save"), &[doc, &twin]);
        let expected = twin.save(&dir.path().join(format!("{name}-twin.rz")));
        if let Some(tag) = tag {
            assert!(
                has_tag(&expected, tag),
                "{name}: the twin keeps its binding"
            );
        }
        assert!(saved == expected, "{name}: tiled stream == contiguous twin");
    }
    checks.finish();
}

// ---------------------------------------------------------------- (b) --

/// A 320×240 base, a 200×150 raster at (−30, 40), a masked 120×90 raster at
/// (50, 60) and a group holding a 64×48 raster at (10, 10).
fn psd_case() -> Doc {
    let doc = DrDocument::from_pixels(patterned(320, 240));
    let doc = with_layer(doc, patterned(200, 150), (-30, 40), "Offset");
    let mut doc = with_layer(doc, patterned(120, 90), (50, 60), "Masked");
    let masked = &mut doc.layers[2];
    masked.mask = Some(Arc::new(GrayImage::from_fn(120, 90, |x, y| {
        Luma([((x * 2 + y) % 256) as u8])
    })));
    masked.mask_enabled = true;
    masked.mask_properties.offset = (50, 60);
    let doc = with_layer(doc, patterned(64, 48), (10, 10), "Inner");
    let (doc, ..) = doc.group_layers(&[3], "Group").expect("a group");
    assert!(doc.layers.iter().any(|l| l.kind == LayerKind::Group));
    Doc::owned(doc)
}

#[test]
fn a_psd_save_reads_tiles_and_leaves_no_view() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let mut checks = Checks::default();
    let doc = psd_case();
    let twin = doc.twin();
    cold();
    let saved = doc.save_psd(&dir.path().join("case.psd"));
    checks.no_view("psd save", &[&doc, &twin]);
    let expected = twin.save_psd(&dir.path().join("twin.psd"));
    assert!(saved == expected, "tiled PSD == contiguous twin");
    checks.finish();
}

// ---------------------------------------------------------------- (c) --

fn losses(doc: &Doc, format: i32) -> Value {
    let mut error = ptr::null_mut();
    let result = unsafe { dr_doc_flat_save_losses(doc.0, format, true, false, &mut error) };
    unsafe { json_result(result, error) }
}

#[test]
fn flat_save_loss_scans_read_tiles_and_leave_no_view() {
    let _serial = serial();
    let mut checks = Checks::default();
    let partial = RgbaImage::from_fn(300, 280, |x, y| {
        Rgba([
            x as u8,
            y as u8,
            9,
            if x == 299 && y == 279 { 128 } else { 255 },
        ])
    });
    let hidden = RgbaImage::from_fn(300, 280, |x, y| {
        let a = if (x + y) % 3 == 0 { 0 } else { 255 };
        Rgba([x as u8 | 1, y as u8, 7, a])
    });
    let opaque = RgbaImage::from_fn(300, 280, |x, y| Rgba([x as u8, y as u8, 3, 255]));
    for (name, image) in [
        ("partial-alpha", partial),
        ("hidden-colour", hidden),
        ("opaque", opaque),
    ] {
        let doc = Doc::from_image(&image);
        let twin = doc.twin();
        for (format_name, format) in [
            ("jpeg", FORMAT_JPEG),
            ("gif", FORMAT_GIF),
            ("png", FORMAT_PNG),
        ] {
            cold();
            let answer = losses(&doc, format);
            checks.no_view(&format!("{name} {format_name} losses"), &[&doc, &twin]);
            assert_eq!(answer, losses(&twin, format), "{name} {format_name}");
        }
        if name == "hidden-colour" {
            // PNG still runs the hidden-colour scan.
            assert!(losses(&doc, FORMAT_PNG)
                .as_array()
                .unwrap()
                .iter()
                .any(|l| l == "hidden pixel colors"));
        }
    }
    checks.finish();
}

// ---------------------------------------------------------------- (d) --

fn thumbnail(doc: &Doc, idx: usize) -> Option<(u32, u32, Vec<u8>)> {
    image_bytes(unsafe { dr_doc_layer_thumbnail(doc.0, idx, 256) })
}

#[test]
fn a_layer_thumbnail_reads_its_tiles_and_leaves_no_view() {
    let _serial = serial();
    let mut checks = Checks::default();
    let corpus_doc = Doc::from_image(&corpus());
    let offset = Doc::owned(with_layer(
        DrDocument::from_pixels(patterned(640, 480)),
        patterned(700, 500),
        (-50, 30),
        "Offset",
    ));
    // 16x the 256-pixel thumbnail per axis: the box arm, which reads
    // the tiles' rows instead of a transient copy.
    let large = Doc::from_image(&patterned(4096, 3072));
    for (name, doc, idx) in [
        ("corpus", &corpus_doc, 0),
        ("offset", &offset, 1),
        ("large", &large, 0),
    ] {
        let twin = doc.twin();
        cold();
        let thumb = thumbnail(doc, idx);
        checks.no_view(&format!("{name} thumbnail"), &[doc, &twin]);
        let expected = thumbnail(&twin, idx);
        assert!(thumb.is_some(), "{name}: a thumbnail");
        assert!(thumb == expected, "{name}: tiled thumbnail == twin");
    }
    checks.finish();
}

// ---------------------------------------------------------------- (e) --

fn canvas_image(doc: &Doc, idx: usize) -> Option<(u32, u32, Vec<u8>)> {
    image_bytes(unsafe { dr_doc_layer_canvas_image(doc.0, idx) })
}

#[test]
fn a_layer_canvas_image_copies_rows_and_leaves_no_view() {
    let _serial = serial();
    let mut checks = Checks::default();
    let doc = DrDocument::from_pixels(patterned(640, 480));
    let doc = with_layer(doc, patterned(700, 500), (-50, 30), "Offset");
    let doc = with_layer(doc, patterned(40, 40), (2000, -900), "Off canvas");
    let doc = with_layer(doc, patterned(1, 1), (5, 7), "One");
    let doc = with_layer(doc, patterned(90, 70), (600, 440), "Inner");
    let (doc, group, ..) = doc.group_layers(&[4], "Group").expect("a group");
    assert_eq!(doc.layers[group].kind, LayerKind::Group);
    let doc = Doc::owned(doc);
    let twin = doc.twin();
    for (name, idx) in [
        ("canvas-sized", 0),
        ("offset", 1),
        ("off-canvas", 2),
        ("1x1", 3),
        ("group", group),
    ] {
        cold();
        let image = canvas_image(&doc, idx);
        checks.no_view(&format!("{name} canvas image"), &[&doc, &twin]);
        let expected = canvas_image(&twin, idx);
        assert!(image.is_some(), "{name}: an image");
        assert!(image == expected, "{name}: tiled canvas image == twin");
    }
    // The off-canvas layer answers the transparent canvas.
    let (w, h, raw) = canvas_image(&doc, 2).unwrap();
    assert_eq!((w, h), (640, 480));
    assert!(raw.iter().all(|&b| b == 0));
    checks.finish();
}

// ---------------------------------------------------------------- (f) --

#[test]
fn a_structure_promotion_widens_tiles_and_leaves_no_view() {
    let _serial = serial();
    let mut checks = Checks::default();
    let bent = bend();
    let second = patterned(warp_fixture::W, warp_fixture::H);
    let doc = Doc::owned(
        bent.get()
            .adding_image_layer(0, second.clone(), "Second")
            .expect("a second layer"),
    );
    assert!(doc.get().layers[0].warp.is_some());
    cold();
    let mut error = ptr::null_mut();
    let typed = unsafe {
        dr_doc_promote_structure(doc.0, REFERENCE_WORKING_BYTES, 0, ptr::null(), &mut error)
    };
    assert!(!typed.is_null(), "promote: {}", take_err_string(error));
    // 13B-3b-2: the promotion builds its planes `from_rgba16_unseeded`, so
    // it leaves no view at all — neither a byte view nor a typed one.
    let typed_views = {
        let handles = [typed as *const _];
        let measured =
            resource_report::report(&[], &[], &handles, &[], 0, 0).expect("a resource report");
        measured["contiguous_view_bytes"].as_u64().unwrap_or(0)
    };
    checks.check(
        "promotion leaves no view",
        contiguous_view_bytes() == 0 && typed_views == 0 && views_of(&doc) == 0,
    );
    let widen = |raw: &[u8]| -> Vec<[u16; 4]> {
        raw.chunks_exact(4)
            .map(|p| [p[0], p[1], p[2], p[3]].map(|v| u16::from(v) * 257))
            .collect()
    };
    let promoted = unsafe { &*typed };
    for (i, layer) in doc.get().layers.iter().enumerate() {
        let source = promoted.layer_source(i).expect("a promoted raster");
        assert_eq!(
            source.to_rgba16().ok().unwrap(),
            widen(&bytes(&layer.pixels)).as_slice(),
            "raster {i}"
        );
    }
    // The WARP original: clearing restores it, the byte original ×257.
    let cleared = promoted
        .warp_edit(
            &WarpEdit::parse(r#"{"op":"clear","layer":0}"#).unwrap(),
            warp_fixture::LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        cleared.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        widen(&warp_fixture::byte_pixels()).as_slice()
    );
    unsafe { dr_precision_doc_free(typed) };
    checks.finish();
}

// ---------------------------------------------------------------- (g) --

fn fill(doc: &Doc, hole: &[u8]) -> Doc {
    let d = doc.get();
    let request = fill_request(0, hole, (d.width, d.height), (16, 1), false);
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_doc_content_aware_fill_edit(
            doc.0,
            request.as_ptr(),
            hole.as_ptr(),
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "fill: {}", take_err_string(error));
    Doc::checked(out)
}

#[test]
fn content_aware_fill_copies_the_plate_rows_and_leaves_no_view() {
    let _serial = serial();
    let mut checks = Checks::default();
    let base = Doc::from_image(&corpus());
    let twin = base.twin();
    let (width, height) = phase4_corpus::DIMENSIONS[0];
    let mut hole = vec![0_u8; width as usize * height as usize];
    for y in 100..164 {
        let at = y * width as usize + 100;
        hole[at..at + 64].fill(255);
    }
    cold();
    let filled = fill(&base, &hole);
    checks.no_view("content-aware fill", &[&base, &filled, &twin]);
    let expected = fill(&twin, &hole);
    assert!(
        bytes(filled.raster(0)) == bytes(expected.raster(0)),
        "tiled fill == contiguous twin"
    );
    checks.finish();
}

// ---------------------------------------------------------------- (h) --

#[test]
fn a_reopened_placed_layer_shares_its_asset_tiles() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let mut checks = Checks::default();
    let (placed, source) = placed_case();
    let path = dir.path().join("placed.rz");
    let saved = placed.save(&path);
    drop(placed);
    cold();
    let reopened = Doc::open(&path);
    checks.no_view("reopen", &[&reopened]);
    assert!(reopened.raster(1).equals_image(&source));
    checks.check(
        "reopen: the placed layer shares every asset tile",
        reopened.asset(1).shared_tile_count(reopened.raster(1)) == tiles(300, 200),
    );
    let measured = report(&[&reopened]);
    checks.check(
        "reopen: layer_pixels.unique_bytes == 640·480·4",
        category(&measured, "layer_pixels", "unique_bytes") == 640 * 480 * 4,
    );
    assert_eq!(
        category(&measured, "embedded_source_pixels", "unique_bytes"),
        300 * 200 * 4
    );
    let resaved = reopened.save(&dir.path().join("resaved.rz"));
    checks.no_view("resave", &[&reopened]);
    assert!(saved == resaved, "reopen then resave is byte-identical");
    // Through 13B-2c-3b's resample read: no view assertion.
    let transformed = reopened.source_edit(
        None,
        json!({"op":"transform","layer":1,"matrix":[0.5,0,0,0.5,0,0],"filter":"bilinear"}),
    );
    assert_eq!(transformed.get().layers.len(), 2);
    checks.finish();
}

// ---------------------------------------------------------------- (i) --

#[test]
fn a_reopened_dabbed_placed_layer_keeps_its_own_plane() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let mut checks = Checks::default();
    let (placed, source) = placed_case();
    let rect = (100, 100, 100, 100);
    let dabbed = placed.paint(1, &overlay((640, 480), rect, DAB));
    let mut expected = source;
    for y in 100..200 {
        for x in 100..200 {
            expected.put_pixel(x, y, Rgba(DAB));
        }
    }
    assert!(dabbed.raster(1).equals_image(&expected));
    let path = dir.path().join("dabbed.rz");
    dabbed.save(&path);
    drop((placed, dabbed));
    cold();
    let reopened = Doc::open(&path);
    checks.no_view("dabbed reopen", &[&reopened]);
    // The dab broke `effective_source`'s `ptr_eq`, so the writer dropped the
    // binding and its asset: nothing is left to share with.
    assert!(reopened.get().layers[1].retained_source.is_none());
    assert!(reopened.raster(1).equals_image(&expected));
    checks.finish();
}

// ---------------------------------------------------------------- (j) --

#[test]
fn plain_and_bent_reopens_leave_no_view() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let mut checks = Checks::default();
    let plain = Doc::owned(with_layer(
        DrDocument::from_pixels(patterned(640, 480)),
        patterned(300, 280),
        (20, -10),
        "Second",
    ));
    let bent = bend();
    for (name, doc) in [("plain", &plain), ("bend", &bent)] {
        let path = dir.path().join(format!("{name}.rz"));
        doc.save(&path);
        cold();
        let reopened = Doc::open(&path);
        checks.no_view(&format!("{name} reopen"), &[&reopened]);
        for (a, b) in doc.get().layers.iter().zip(&reopened.get().layers) {
            assert!(a.pixels.same_bytes(&b.pixels), "{name}: layer bytes");
        }
        if name == "bend" {
            let warp = reopened.get().layers[0].warp.as_ref().expect("a bend");
            assert!(warp.source_pixels.is_some(), "the WARP original");
        }
    }
    checks.finish();
}
