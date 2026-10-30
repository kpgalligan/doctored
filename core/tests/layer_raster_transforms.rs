//! Black-box suite for 13B-2c-3b: the resample inputs (transform, perspective,
//! geometry set, rotate/flip, resize, warp and the source render), the
//! retouch expansion, the scoped analysis and the retouch temp documents read the tiles (a transient
//! `to_contiguous[_with_job]`, `runs` / `row_segments` / `sample_at` /
//! `read_rect`), so none leaves a contiguous view of its INPUT behind, and the
//! eight geometry producers answer `unseeded` outputs.
//!
//! Oracles are independent of the implementation: every output is compared
//! byte for byte (`same_bytes`, plus offset and dimensions) with the same call
//! on a CONTIGUOUS twin (every raster re-adopted through
//! `LayerRaster::shared`, with the retained-source and WARP bindings
//! re-pointed so neither writer drops them); the expansion is also compared
//! with a hand-built placement of the source rows, and expected seeded view
//! bytes are dimension products.
//!
//! Order in every test: build the fixture and its twin (fixtures seed views),
//! `cold()`, run the measured call on both, check the views, compare. The
//! post-call view checks are COLLECTED (`Checks`) and reported together at the
//! end of each test, so a run against a library without these reads names
//! every failing check rather than the first.
//!
//! The view cache is process-wide, so every test holds `serial()`.

use crate::phase4_corpus;
use crate::serial;

use crate::warp_fixture;

use crate::common;

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use common::take_err_string;
use doctored_core::doc::{DrDocument, LayerKind, LayerRaster};
use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_content_aware::dr_doc_content_aware_fill_edit;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_geometry::dr_doc_geometry_edit;
use doctored_core::ffi_heal::{dr_doc_heal_to_new_layer, dr_doc_retouch_source};
use doctored_core::ffi_retained_source::dr_doc_mixed_source_edit;
use doctored_core::ffi_retouch::dr_doc_expand_retouch_destination;
use doctored_core::ffi_scoped_statistics::dr_doc_scoped_analysis;
use doctored_core::layer_raster::contiguous_view_bytes;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::retained_source::{SourceAsset, SourcePixels};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::resource_report;

const DAB: [u8; 4] = [200, 40, 90, 255];
const NEAREST: i32 = 0;
const BILINEAR: i32 = 1;
const LANCZOS3: i32 = 3;
const CURRENT_AND_BELOW: u32 = 1;

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

    fn twin(&self) -> Self {
        Self::owned(contiguous_twin(self.get()))
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

    fn geometry_edit(&self, request: &Value) -> Self {
        let request = CString::new(request.to_string()).unwrap();
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_doc_geometry_edit(
                self.0,
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
/// `effective_warp` still hold), each byte asset Arc is re-adopted ONCE and
/// each WARP original is re-adopted contiguously.
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

/// Every layer's pixels, offset and dimensions equal between two answers.
fn same_layers(what: &str, a: &Doc, b: &Doc) {
    let (a, b) = (a.get(), b.get());
    assert_eq!(
        (a.width, a.height, a.layers.len()),
        (b.width, b.height, b.layers.len()),
        "{what}: canvas and layer count"
    );
    for (i, (la, lb)) in a.layers.iter().zip(&b.layers).enumerate() {
        assert_eq!(la.offset, lb.offset, "{what}: layer {i} offset");
        assert_eq!(
            la.pixels.dimensions(),
            lb.pixels.dimensions(),
            "{what}: layer {i} dimensions"
        );
        assert!(
            la.pixels.same_bytes(&lb.pixels),
            "{what}: layer {i} bytes == contiguous twin"
        );
    }
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

/// A canvas-frame buffer of `value` over `(x, y, w, h)` (inside the canvas).
fn boxed(canvas: (u32, u32), (x, y, w, h): (u32, u32, u32, u32), value: &[u8]) -> Vec<u8> {
    let n = value.len();
    let mut out = vec![0_u8; canvas.0 as usize * canvas.1 as usize * n];
    for cy in y..y + h {
        for cx in x..x + w {
            let at = (cy as usize * canvas.0 as usize + cx as usize) * n;
            out[at..at + n].copy_from_slice(value);
        }
    }
    out
}

/// The 640×480 patterned base plus the 300×200 source at (−30, 40) as
/// layer 1, the edge-overlap layer.
fn offset_case() -> (Doc, RgbaImage) {
    let source = patterned(300, 200);
    let doc = DrDocument::from_pixels(patterned(640, 480))
        .adding_image_layer(0, source.clone(), "Offset")
        .expect("a layer");
    let mut doc = doc;
    doc.layers[1].offset = (-30, 40);
    (Doc::owned(doc), source)
}

/// A 300×200 source placed at (0, 0) over a 640×480 host.
fn placed_case() -> Doc {
    let host = Doc::from_image(&patterned(640, 480));
    let input = Doc::from_image(&patterned(300, 200));
    host.place(&input)
}

fn transform(doc: &Doc, idx: usize, affine: [f64; 6], filter: i32) -> Doc {
    Doc::checked(unsafe { dr_doc_transform_layer(doc.0, idx, affine.as_ptr(), filter) })
}

// ---------------------------------------------------------------- (a) --

#[test]
fn a_transform_layer_reads_a_transient_copy_and_leaves_no_view() {
    let _serial = serial();
    let mut checks = Checks::default();
    let (doc, _) = offset_case();
    let twin = doc.twin();
    let (s, c) = 30_f64.to_radians().sin_cos();
    for (what, affine, filter) in [
        ("rotate 30", [c, s, -s, c, 40.0, -20.0], BILINEAR),
        ("rotate 90", [0.0, 1.0, -1.0, 0.0, 400.0, 50.0], BILINEAR),
        ("translate", [1.0, 0.0, 0.0, 1.0, 12.0, -7.0], BILINEAR),
    ] {
        cold();
        let out = transform(&doc, 1, affine, filter);
        let twin_out = transform(&twin, 1, affine, filter);
        checks.no_view(what, &[&doc, &twin, &out, &twin_out]);
        if what == "translate" {
            checks.check(
                "translate: identity kept (no copy)",
                LayerRaster::ptr_eq(out.raster(1), doc.raster(1)),
            );
        }
        same_layers(what, &out, &twin_out);
    }
    checks.finish();
}

// ---------------------------------------------------------------- (b) --

#[test]
fn b_perspective_layer_reads_a_transient_copy_and_leaves_no_view() {
    let _serial = serial();
    let mut checks = Checks::default();
    let (doc, _) = offset_case();
    let twin = doc.twin();
    let quad = [-20.0, 45.0, 265.0, 40.0, 275.0, 250.0, -30.0, 235.0];
    cold();
    let out = Doc::checked(unsafe { dr_doc_perspective_layer(doc.0, 1, quad.as_ptr(), BILINEAR) });
    let twin_out =
        Doc::checked(unsafe { dr_doc_perspective_layer(twin.0, 1, quad.as_ptr(), BILINEAR) });
    checks.no_view("perspective", &[&doc, &twin, &out, &twin_out]);
    same_layers("perspective", &out, &twin_out);
    checks.finish();
}

// ---------------------------------------------------------------- (c) --

#[test]
fn c_perspective_set_on_a_plain_layer_and_a_placed_source_leaves_no_view() {
    let _serial = serial();
    let mut checks = Checks::default();
    let request = json!({"op":"perspective_set","layers":[1],
        "source_rect":[0,0,640,480],
        "quad":[0,0,630,5,640,470,10,480],"filter":"bilinear"});
    let (plain, _) = offset_case();
    let placed = placed_case();
    assert!(placed.get().layers[1].retained_source.is_some());
    for (what, doc) in [("plain", &plain), ("placed", &placed)] {
        let twin = doc.twin();
        cold();
        let out = doc.geometry_edit(&request);
        let twin_out = twin.geometry_edit(&request);
        checks.no_view(
            &format!("perspective_set {what}"),
            &[doc, &twin, &out, &twin_out],
        );
        same_layers(&format!("perspective_set {what}"), &out, &twin_out);
        if what == "placed" {
            assert!(out.get().layers[1].retained_source.is_some());
        }
    }
    checks.finish();
}

// ---------------------------------------------------------------- (d) --

#[test]
fn d_rotate90_and_flip_horizontal_leave_no_view() {
    let _serial = serial();
    let mut checks = Checks::default();
    let (width, height) = phase4_corpus::DIMENSIONS[0];
    let corpus = Doc::from_image(&phase4_corpus::image(width, height));
    let corpus_twin = corpus.twin();
    cold();
    let out = Doc::checked(unsafe { dr_doc_rotate90(corpus.0) });
    let twin_out = Doc::checked(unsafe { dr_doc_rotate90(corpus_twin.0) });
    checks.no_view("rotate90 corpus", &[&corpus, &corpus_twin, &out, &twin_out]);
    same_layers("rotate90 corpus", &out, &twin_out);
    assert_eq!(out.raster(0).dimensions(), (height, width));
    drop((out, twin_out, corpus, corpus_twin));

    let (doc, _) = offset_case();
    let twin = doc.twin();
    cold();
    let out = Doc::checked(unsafe { dr_doc_flip_horizontal(doc.0) });
    let twin_out = Doc::checked(unsafe { dr_doc_flip_horizontal(twin.0) });
    checks.no_view("flip_horizontal", &[&doc, &twin, &out, &twin_out]);
    same_layers("flip_horizontal", &out, &twin_out);
    checks.finish();
}

// ---------------------------------------------------------------- (e) --

#[test]
fn e_resize_leaves_no_view() {
    let _serial = serial();
    let mut checks = Checks::default();
    let (doc, _) = offset_case();
    let twin = doc.twin();
    for (what, filter) in [("lanczos3", LANCZOS3), ("nearest", NEAREST)] {
        cold();
        let out = Doc::checked(unsafe { dr_doc_resize(doc.0, 320, 240, filter) });
        let twin_out = Doc::checked(unsafe { dr_doc_resize(twin.0, 320, 240, filter) });
        checks.no_view(&format!("resize {what}"), &[&doc, &twin, &out, &twin_out]);
        same_layers(&format!("resize {what}"), &out, &twin_out);
        assert_eq!(out.raster(1).dimensions(), (150, 100));
    }
    checks.finish();
}

// ---------------------------------------------------------------- (f) --

#[test]
fn f_warp_edit_leaves_no_view() {
    let _serial = serial();
    let mut checks = Checks::default();
    let doc = Doc::owned(warp_fixture::byte_document());
    let twin = doc.twin();
    let request = warp_fixture::edit(warp_fixture::field(3, 3, "bicubic"));
    let warp = |d: &Doc| {
        Doc::owned(
            d.get()
                .warp_edit(&request, warp_fixture::LIMIT, 0, None)
                .unwrap()
                .unwrap(),
        )
    };
    cold();
    let out = warp(&doc);
    let twin_out = warp(&twin);
    checks.no_view("warp_edit", &[&doc, &twin, &out, &twin_out]);
    assert!(out.get().layers[0]
        .warp
        .as_ref()
        .is_some_and(|w| w.source_pixels.is_some()));
    same_layers("warp_edit", &out, &twin_out);
    checks.finish();
}

// ---------------------------------------------------------------- (g) --

fn expand(doc: &Doc, idx: usize) -> Doc {
    let mut error = ptr::null_mut();
    let out = unsafe { dr_doc_expand_retouch_destination(doc.0, idx, ptr::null(), &mut error) };
    assert!(error.is_null(), "{}", take_err_string(error));
    Doc::checked(out)
}

#[test]
fn g_expand_retouch_destination_copies_rows_and_leaves_no_view() {
    let _serial = serial();
    let mut checks = Checks::default();
    let (doc, source) = offset_case();
    let twin = doc.twin();
    cold();
    let out = expand(&doc, 1);
    let twin_out = expand(&twin, 1);
    checks.no_view("expand", &[&doc, &twin, &out, &twin_out]);
    same_layers("expand", &out, &twin_out);
    let layer = &out.get().layers[1];
    let (w, h) = layer.pixels.dimensions();
    assert_ne!((w, h), source.dimensions(), "the layer expanded");
    let (dx, dy) = (-30 - layer.offset.0, 40 - layer.offset.1);
    assert!(dx >= 0 && dy >= 0);
    let mut expected = RgbaImage::new(w, h);
    for (x, y, p) in source.enumerate_pixels() {
        expected.put_pixel(x + dx as u32, y + dy as u32, *p);
    }
    assert!(
        layer.pixels.equals_image(&expected),
        "expand: bytes == hand-built placement"
    );
    checks.finish();
}

// ---------------------------------------------------------------- (i) --

fn analysis(doc: &Doc, request: &Value) -> Value {
    let request = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_doc_scoped_analysis(
            doc.0,
            request.as_ptr(),
            ptr::null(),
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    unsafe { json_result(result, error) }
}

#[test]
fn i_scoped_layer_analysis_samples_in_place_and_leaves_no_view() {
    let _serial = serial();
    let mut checks = Checks::default();
    let (doc, _) = offset_case();
    let twin = doc.twin();
    let request = json!({"operation":"histogram","source":"layer","layer":1});
    cold();
    let result = analysis(&doc, &request);
    let expected = analysis(&twin, &request);
    checks.no_view("scoped analysis", &[&doc, &twin]);
    assert_eq!(result, expected, "histogram == contiguous twin");
    checks.finish();
}

// ---------------------------------------------------------------- (k) --

fn content_aware_new_layer(doc: &Doc, coverage: &[u8]) -> Doc {
    let request = CString::new(
        json!({"op":"content_aware_fill","layer":0,"x":200,"y":150,"width":64,"height":64,
            "ring":16,"seed":1,"output":"new_layer"})
        .to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_doc_content_aware_fill_edit(
            doc.0,
            request.as_ptr(),
            coverage.as_ptr(),
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", take_err_string(error));
    Doc::checked(out)
}

#[test]
fn k_content_aware_new_layer_leaves_no_view() {
    let _serial = serial();
    let mut checks = Checks::default();
    let (w, h) = (640, 480);
    let doc = Doc::from_image(&patterned(w, h));
    let twin = doc.twin();
    let coverage = boxed((w, h), (200, 150, 64, 64), &[255]);
    cold();
    let out = content_aware_new_layer(&doc, &coverage);
    let twin_out = content_aware_new_layer(&twin, &coverage);
    // The lift goes through the polled rect deposit, which writes the
    // new layer's tiles and seeds no canvas view.
    checks.no_view("content-aware new layer", &[&doc, &twin, &out, &twin_out]);
    assert_eq!(out.get().layers.len(), 2);
    same_layers("content-aware new layer", &out, &twin_out);
    checks.finish();
}

// ---------------------------------------------------------------- (l) --

/// The CurrentAndBelow plate of layer `idx`.
fn plate(doc: &Doc, idx: usize) -> Vec<u8> {
    let mut error = ptr::null_mut();
    let image = unsafe {
        dr_doc_retouch_source(
            doc.0,
            idx,
            CURRENT_AND_BELOW,
            false,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null() && !image.is_null(), "plate refused");
    let out = unsafe {
        let (w, h) = (dr_image_width(image), dr_image_height(image));
        std::slice::from_raw_parts(dr_image_pixels_rgba(image), w as usize * h as usize * 4)
            .to_vec()
    };
    unsafe { dr_image_free(image) };
    out
}

fn heal_to_new_layer(doc: &Doc, overlay: &[u8], plate: &[u8]) -> Doc {
    let d = doc.get();
    let name = CString::new("Retouch").unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_doc_heal_to_new_layer(
            doc.0,
            0,
            overlay.as_ptr(),
            d.width,
            d.height,
            1.0,
            plate.as_ptr(),
            name.as_ptr(),
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", take_err_string(error));
    Doc::checked(out)
}

#[test]
fn l_heal_to_new_layer_leaves_no_view() {
    let _serial = serial();
    let mut checks = Checks::default();
    let (w, h) = (640, 480);
    let doc = Doc::from_image(&patterned(w, h));
    let twin = doc.twin();
    let plate = plate(&doc, 0);
    let overlay = boxed((w, h), (100, 100, 60, 60), &DAB);
    cold();
    let out = heal_to_new_layer(&doc, &overlay, &plate);
    let twin_out = heal_to_new_layer(&twin, &overlay, &plate);
    checks.no_view("heal_to_new_layer", &[&doc, &twin, &out, &twin_out]);
    assert_eq!(out.get().layers.len(), 2);
    same_layers("heal_to_new_layer", &out, &twin_out);
    checks.finish();
}
