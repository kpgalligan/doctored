//! Black-box suite for 13B-2c-2c: byte source assets held as tiles. A placed
//! or re-rendered byte source's layer raster IS its asset's tile plane under a
//! fresh identity, so the asset counts once — before and after the layer's
//! first edit — and a converted plane is tiled only where it is stored, never
//! seeded. Oracles are independent of the implementation: expected bytes are
//! the source image (patched by hand, or converted per pixel through the
//! public `icc_transform::Transform`), expected sharing is the tile count
//! derived from rect arithmetic, and expected report figures are dimension
//! products.
//!
//! The view cache is process-wide, so every test holds `serial()` and evicts
//! before the operation it measures.

use crate::phase4_corpus;
use crate::serial;

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::{DrDocument, LayerRaster};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_retained_source::{
    dr_doc_mixed_source_edit, dr_precision_doc_mixed_source_edit,
};
use doctored_core::icc::IccProfile;
use doctored_core::icc_transform::Transform;
use doctored_core::layer_raster::contiguous_view_bytes;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::resource_report;

const EDGE: u32 = 256;
const DAB: [u8; 4] = [200, 40, 90, 255];

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
        assert!(error.is_null(), "{}", unsafe { text(error) });
        Self::checked(out)
    }

    fn place(&self, input: &Doc) -> Self {
        self.source_edit(
            Some(input),
            json!({"op":"place","source_id":"asset","name":"Placed","x":0,"y":0}),
        )
    }
}

unsafe fn text(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let message = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    message
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

fn tiles(width: u32, height: u32) -> usize {
    (width.div_ceil(EDGE) * height.div_ceil(EDGE)) as usize
}

/// The tiles a layer-local rect touches, with each clipped tile's bytes.
fn touched(size: (u32, u32), (x, y, w, h): (u32, u32, u32, u32)) -> Vec<u64> {
    let mut out = Vec::new();
    for ty in y / EDGE..=(y + h - 1) / EDGE {
        for tx in x / EDGE..=(x + w - 1) / EDGE {
            let tw = (size.0 - tx * EDGE).min(EDGE);
            let th = (size.1 - ty * EDGE).min(EDGE);
            out.push(u64::from(tw) * u64::from(th) * 4);
        }
    }
    out
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

/// (a): a 300×200 source placed at (0, 0) over a 640×480 host.
fn placed_case() -> (Doc, RgbaImage) {
    let host = Doc::from_image(&patterned(640, 480));
    let source = patterned(300, 200);
    let input = Doc::from_image(&source);
    evict();
    let placed = host.place(&input);
    (placed, source)
}

#[test]
fn a_placed_source_is_its_asset_plane_under_a_fresh_identity() {
    let _serial = serial();
    let (placed, source) = placed_case();
    assert!(placed.raster(1).equals_image(&source));
    assert_eq!(placed.asset(1).shared_tile_count(placed.raster(1)), 2);
    assert_eq!(tiles(300, 200), 2);
    assert!(!LayerRaster::ptr_eq(placed.asset(1), placed.raster(1)));
    let measured = report(&[&placed]);
    assert_eq!(
        category(&measured, "layer_pixels", "unique_bytes"),
        640 * 480 * 4
    );
    assert_eq!(
        category(&measured, "embedded_source_pixels", "unique_bytes"),
        300 * 200 * 4
    );
    assert_eq!(
        category(&measured, "embedded_source_pixels", "allocation_count"),
        2
    );
    assert_eq!(
        contiguous_view_bytes(),
        0,
        "a matching place leaves no view"
    );
}

#[test]
fn a_dabbed_placed_corpus_counts_its_asset_once() {
    let _serial = serial();
    let (width, height) = phase4_corpus::DIMENSIONS[0];
    let corpus = phase4_corpus::image(width, height);
    let host = Doc::from_image(&corpus);
    let input = Doc::from_image(&corpus);
    evict();
    let placed = host.place(&input);
    drop(input);
    let rect = (100, 100, 300, 300);
    let dabbed = placed.paint(1, &overlay((width, height), rect, DAB));
    let mut expected = corpus;
    for y in 100..400 {
        for x in 100..400 {
            expected.put_pixel(x, y, Rgba(DAB));
        }
    }
    assert!(dabbed.raster(1).equals_image(&expected));
    let plane = u64::from(width) * u64::from(height) * 4;
    assert_eq!(plane, 96_000_000);
    let intersected: u64 = touched((width, height), rect).iter().sum();
    let measured = report(&[&host, &placed, &dabbed]);
    assert_eq!(
        category(&measured, "layer_pixels", "unique_bytes"),
        plane + intersected
    );
    assert_eq!(
        category(&measured, "embedded_source_pixels", "unique_bytes"),
        plane
    );
    assert_eq!(tiles(width, height), 384);
    assert_eq!(
        placed.asset(1).shared_tile_count(dabbed.raster(1)),
        384 - touched((width, height), rect).len()
    );
    assert_eq!(touched((width, height), rect).len(), 4);
    assert_eq!(contiguous_view_bytes(), 0);
}

#[test]
fn a_placed_asset_adopts_the_input_raster() {
    let _serial = serial();
    let host = Doc::from_image(&patterned(640, 480));
    let source = patterned(300, 200);
    let input = Doc::from_image(&source);
    let placed = host.place(&input);
    assert!(LayerRaster::ptr_eq(placed.asset(1), input.raster(0)));
    drop(input);
    assert!(placed.asset(1).equals_image(&source));
}

#[test]
fn a_converting_place_tiles_the_converted_plane_without_a_seed() {
    let _serial = serial();
    let host = Doc::from_image(&patterned(640, 480));
    let source = patterned(257, 257);
    let mut p3 = DrDocument::from_pixels(source.clone());
    p3.profile = IccProfile::display_p3();
    let input = Doc::owned(p3);
    evict();
    let placed = host.place(&input);
    // `color8`'s per-pixel statement, alpha kept: the public `apply` runs the
    // same `Transform::color` over every pixel.
    let mut expected = source.clone();
    Transform::new(
        IccProfile::display_p3().model().unwrap(),
        IccProfile::srgb().model().unwrap(),
    )
    .apply(&mut expected);
    assert_ne!(expected, source, "the conversion changes the bytes");
    assert!(placed.raster(1).equals_image(&expected));
    assert!(!placed.raster(1).same_bytes(placed.asset(1)));
    assert_eq!(placed.asset(1).shared_tile_count(placed.raster(1)), 0);
    let measured = report(&[&placed]);
    assert_eq!(
        category(&measured, "embedded_source_pixels", "unique_bytes"),
        257 * 257 * 4
    );
    assert_eq!(
        category(&measured, "layer_pixels", "unique_bytes"),
        640 * 480 * 4 + 257 * 257 * 4
    );
    assert_eq!(contiguous_view_bytes(), 0, "a converting place is unseeded");
}

#[test]
fn a_resampled_render_leaves_no_view_and_restores_its_tiles() {
    let _serial = serial();
    let (placed, source) = placed_case();
    let scale =
        |f: f64| json!({"op":"transform","layer":1,"matrix":[f,0,0,f,0,0],"filter":"nearest"});
    evict();
    let smaller = placed.source_edit(None, scale(0.5));
    assert_eq!(placed.asset(1).shared_tile_count(smaller.raster(1)), 0);
    assert_eq!(
        contiguous_view_bytes(),
        0,
        "the Sample arm reads a transient copy of the asset (13B-2c-3b)"
    );
    let restored = smaller.source_edit(None, scale(2.0));
    assert!(restored.raster(1).equals_image(&source));
    assert_eq!(
        placed.asset(1).shared_tile_count(restored.raster(1)),
        tiles(300, 200)
    );
    let measured = report(&[&placed, &smaller, &restored]);
    assert_eq!(
        category(&measured, "embedded_source_pixels", "unique_bytes"),
        300 * 200 * 4
    );
}

#[test]
fn a_byte_source_placed_into_sixteen_bits_promotes_by_257() {
    let _serial = serial();
    let source = patterned(257, 257);
    let input = Doc::from_image(&source);
    let destination = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(257, 257, vec![[0; 4]; 257 * 257], IccProfile::srgb(), None)
            .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let request = CString::new(
        json!({"op":"place","source_id":"family-a","revision_id":"version-a","name":"Placed","x":0,"y":0,"link":Value::Null,"depth_conversion":"convert_fallback"})
            .to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_mixed_source_edit(
            &destination,
            input.0,
            ptr::null(),
            request.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    assert!(!out.is_null());
    let placed = unsafe { &*out };
    let samples = placed.layer_source(1).unwrap().to_rgba16().ok().unwrap();
    for (i, pixel) in source.pixels().enumerate() {
        assert_eq!(samples[i], pixel.0.map(|v| u16::from(v) * 257), "{i}");
    }
    unsafe { dr_precision_doc_free(out) };
}

#[test]
fn a_placed_source_round_trips_through_rz_tiled() {
    let _serial = serial();
    let (placed, source) = placed_case();
    let dir = tempfile::TempDir::new().unwrap();
    let save = |doc: &Doc, name: &str| -> Vec<u8> {
        let path = dir.path().join(name);
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut error = ptr::null_mut();
        let ok = unsafe { dr_doc_save_native(doc.0, c_path.as_ptr(), &mut error) };
        assert!(ok, "{}", unsafe { text(error) });
        std::fs::read(&path).unwrap()
    };
    let first = save(&placed, "first.rz");
    let c_path = CString::new(dir.path().join("first.rz").to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let reopened = Doc::checked(unsafe { dr_doc_open(c_path.as_ptr(), &mut error) });
    assert!(error.is_null(), "{}", unsafe { text(error) });
    let second = save(&reopened, "second.rz");
    assert!(first == second, "reopen → resave is byte-identical");
    let asset = reopened.asset(1);
    assert!(asset.equals_image(&source));
    assert_eq!(asset.shared_tile_count(asset), tiles(300, 200), "tiled");
    let measured = report(&[&reopened]);
    assert_eq!(
        category(&measured, "embedded_source_pixels", "unique_bytes"),
        300 * 200 * 4
    );
}
