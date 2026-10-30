//! Black-box suite for 13B-2b: byte layer pixels held as 256×256 tiles, the
//! writers that share unchanged tiles, and the bounded contiguous-view cache.
//! Oracles are independent of the implementation: expected bytes are the
//! source image patched by hand, expected sharing is the tile count derived
//! from rect arithmetic, and expected report figures are dimension products.
//!
//! The view cache is process-wide, so every test holds `serial()`: a count
//! of `contiguous_views` must not see another test's rasters.

use crate::phase4_corpus;
use crate::serial;

use std::ffi::CString;
use std::ptr;
use std::sync::{Arc, Barrier};

use doctored_core::doc::{DrDocument, LayerRaster};
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::dr_doc_with_layer_locks;
use doctored_core::ffi_retained_source::dr_doc_mixed_source_edit;
use doctored_core::layer_raster::{contiguous_view_bytes, CONTIGUOUS_CACHE_BYTES};
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::resource_report;
use crate::rgba_layer;

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

    fn from_image(image: &RgbaImage) -> Self {
        let handle =
            unsafe { dr_image_from_rgba(image.as_raw().as_ptr(), image.width(), image.height()) };
        assert!(!handle.is_null());
        let document = Self::checked(unsafe { dr_doc_from_image(handle) });
        unsafe { dr_image_free(handle) };
        document
    }

    fn get(&self) -> &DrDocument {
        unsafe { &*self.0 }
    }

    fn raster(&self, idx: usize) -> &LayerRaster {
        &self.get().layers[idx].pixels
    }

    fn paint(&self, idx: usize, overlay: &[u8]) -> *mut DrDocument {
        let d = self.get();
        unsafe { dr_doc_painting_layer(self.0, idx, overlay.as_ptr(), d.width, d.height, 0, 1.0) }
    }
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

/// A canvas-frame premultiplied overlay holding `value` over the canvas part
/// of `(x, y, w, h)`; answers the clipped canvas rect.
fn overlay(
    canvas: (u32, u32),
    (x, y, w, h): (i64, i64, i64, i64),
    value: [u8; 4],
) -> (Vec<u8>, (i64, i64, i64, i64)) {
    let mut out = vec![0_u8; canvas.0 as usize * canvas.1 as usize * 4];
    let (x0, y0) = (x.max(0), y.max(0));
    let x1 = (x + w).min(i64::from(canvas.0));
    let y1 = (y + h).min(i64::from(canvas.1));
    for cy in y0..y1 {
        for cx in x0..x1 {
            let at = (cy as usize * canvas.0 as usize + cx as usize) * 4;
            out[at..at + 4].copy_from_slice(&value);
        }
    }
    (out, (x0, y0, x1 - x0, y1 - y0))
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

#[test]
fn new_round_trips_exactly_at_every_edge_size() {
    let _serial = serial();
    let (cw, ch) = phase4_corpus::DIMENSIONS[0];
    for (width, height) in [(1, 1), (255, 255), (256, 256), (257, 257), (cw, ch)] {
        let image = if width == cw {
            phase4_corpus::image(width, height)
        } else {
            patterned(width, height)
        };
        let raster = LayerRaster::new(image.clone());
        assert_eq!(raster.dimensions(), (width, height));
        assert_eq!(raster.byte_len(), u64::from(width) * u64::from(height) * 4);
        assert_eq!(raster.shared_tile_count(&raster), tiles(width, height));
        assert_eq!(raster.to_contiguous().unwrap(), image, "{width}×{height}");
        assert_eq!(*raster.contiguous_image(), image, "{width}×{height}");
        evict();
        assert_eq!(*raster.contiguous_image(), image, "re-materialized");
    }
}

#[test]
fn a_shared_raster_aliases_until_its_first_write_tiles_it() {
    let _serial = serial();
    let image = patterned(600, 300);
    let adopted = Arc::new(image.clone());
    let shared = LayerRaster::shared(Arc::clone(&adopted));
    assert_eq!(shared.shared_tile_count(&shared), 0, "contiguous");
    assert!(Arc::ptr_eq(&shared.contiguous_image(), &adopted));
    let all = tiles(600, 300);

    let rebuilt = shared.rebuilt(image.clone());
    assert_eq!(rebuilt.shared_tile_count(&rebuilt), all, "tiled");
    assert!(!LayerRaster::ptr_eq(&rebuilt, &shared));
    assert_eq!(*rebuilt.contiguous_image(), image);

    let rect = (250, 10, 20, 5);
    let patch = vec![[1, 2, 3, 4]; 100];
    let mut expected = image.clone();
    for y in 10..15 {
        for x in 250..270 {
            expected.put_pixel(x, y, Rgba([1, 2, 3, 4]));
        }
    }
    let written = shared
        .with_rect(rect.0, rect.1, rect.2, rect.3, &patch)
        .unwrap()
        .expect("a changed raster");
    assert_eq!(written.shared_tile_count(&written), all, "tiled");
    assert_eq!(*written.contiguous_image(), expected);
    assert!(
        Arc::ptr_eq(&shared.contiguous_image(), &adopted),
        "input untouched"
    );
    // The same write over the tiled raster shares all but the changed tiles.
    let over_tiles = rebuilt
        .with_rect(rect.0, rect.1, rect.2, rect.3, &patch)
        .unwrap()
        .expect("a changed raster");
    let changed = touched((600, 300), rect).len();
    assert_eq!(changed, 2);
    assert_eq!(rebuilt.shared_tile_count(&over_tiles), all - changed);
    assert_eq!(*over_tiles.contiguous_image(), expected);
    // Writing the bytes already there changes nothing on either arm.
    let same = image.get_pixel(0, 0).0;
    assert!(shared.with_rect(0, 0, 1, 1, &[same]).unwrap().is_none());
    assert!(rebuilt.with_rect(0, 0, 1, 1, &[same]).unwrap().is_none());
    assert!(shared.with_rect(599, 0, 2, 1, &[same, same]).is_err());
}

#[test]
fn zero_area_rasters_stay_contiguous_and_tile_on_rebuild() {
    let _serial = serial();
    for raster in [
        LayerRaster::new(RgbaImage::new(0, 0)),
        LayerRaster::shared(Arc::new(RgbaImage::new(0, 0))),
    ] {
        assert_eq!(raster.dimensions(), (0, 0));
        assert_eq!(raster.read_rect(0, 0, 0, 0), Some(vec![]));
        assert_eq!(raster.shared_tile_count(&raster), 0);
        let image = patterned(2, 2);
        let rebuilt = raster.rebuilt(image.clone());
        assert_eq!(rebuilt.shared_tile_count(&rebuilt), 1, "tiled");
        assert_eq!(*rebuilt.contiguous_image(), image);
    }
}

#[test]
fn a_placed_byte_source_counts_its_asset_and_layer_once() {
    let _serial = serial();
    let base = patterned(640, 480);
    let source = patterned(300, 200);
    let host = Doc::from_image(&base);
    let input = Doc::from_image(&source);
    let request = CString::new(
        json!({"op":"place","source_id":"asset","name":"Placed","x":0,"y":0}).to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let placed = Doc::checked(unsafe {
        dr_doc_mixed_source_edit(
            host.0,
            input.0,
            ptr::null(),
            request.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    });
    assert!(error.is_null());
    drop(input);
    assert_eq!(*placed.raster(1).contiguous_image(), source);
    let measured = report(&[&placed]);
    // The base layer's tiles; the placed layer IS its asset's tile plane
    // under a fresh identity (13B-2c-2c), so its tiles count once, under
    // `embedded_source_pixels` (the source walk runs first).
    assert_eq!(
        category(&measured, "layer_pixels", "unique_bytes"),
        640 * 480 * 4
    );
    assert_eq!(
        category(&measured, "embedded_source_pixels", "unique_bytes"),
        300 * 200 * 4
    );
    let asset = placed.get().layers[1]
        .retained_source
        .as_ref()
        .unwrap()
        .asset
        .rgba8()
        .unwrap();
    assert_eq!(placed.raster(1).shared_tile_count(asset), tiles(300, 200));
    assert!(!LayerRaster::ptr_eq(placed.raster(1), asset));
}

/// Paints one opaque dab at canvas `rect` onto a corpus document whose layer
/// sits at `offset` and checks bytes and sharing against the hand oracle.
fn dab_case(offset: (i32, i32), rect: (i64, i64, i64, i64)) {
    let (width, height) = phase4_corpus::DIMENSIONS[0];
    let image = phase4_corpus::image(width, height);
    let mut base = Doc::from_image(&image);
    if offset != (0, 0) {
        base = Doc::checked(unsafe { dr_doc_with_layer_offset(base.0, 0, offset.0, offset.1) });
    }
    let (paint, (cx, cy, cw, ch)) = overlay((width, height), rect, DAB);
    let dabbed = Doc::checked(base.paint(0, &paint));
    drop(paint);
    let mut expected = image;
    let (ox, oy) = (i64::from(offset.0), i64::from(offset.1));
    let (lx0, ly0) = ((cx - ox).max(0), (cy - oy).max(0));
    let lx1 = (cx + cw - ox).min(i64::from(width));
    let ly1 = (cy + ch - oy).min(i64::from(height));
    for ly in ly0..ly1 {
        for lx in lx0..lx1 {
            expected.put_pixel(lx as u32, ly as u32, Rgba(DAB));
        }
    }
    let local = (
        lx0 as u32,
        ly0 as u32,
        (lx1 - lx0) as u32,
        (ly1 - ly0) as u32,
    );
    assert!(*dabbed.raster(0).contiguous_image() == expected, "{rect:?}");
    let changed = touched((width, height), local).len();
    assert_eq!(
        base.raster(0).shared_tile_count(dabbed.raster(0)),
        tiles(width, height) - changed,
        "{rect:?}"
    );
}

#[test]
fn dabs_touch_only_the_tiles_they_intersect() {
    let _serial = serial();
    dab_case((0, 0), (250, 200, 300, 300)); // across the 256/512 seams
    dab_case((0, 0), (-100, -100, 300, 300)); // off the top-left corner
    dab_case((0, 0), (5900, 3900, 300, 300)); // off the bottom-right corner
    dab_case((-50, 30), (250, 200, 300, 300)); // an offset layer
}

#[test]
fn an_all_zero_overlay_is_a_fresh_identity_sharing_every_tile() {
    let _serial = serial();
    let image = patterned(700, 500);
    let base = Doc::from_image(&image);
    let zero = vec![0_u8; 700 * 500 * 4];
    let painted = Doc::checked(base.paint(0, &zero));
    assert!(!LayerRaster::ptr_eq(base.raster(0), painted.raster(0)));
    assert_eq!(*painted.raster(0).contiguous_image(), image);
    assert_eq!(
        base.raster(0).shared_tile_count(painted.raster(0)),
        tiles(700, 500)
    );
}

#[test]
fn multiply_by_white_changes_nothing_and_refuses() {
    let _serial = serial();
    // Multiply by white is the identity only over an opaque backdrop.
    let mut opaque = patterned(700, 500);
    opaque.pixels_mut().for_each(|p| p[3] = 255);
    let base = Doc::from_image(&opaque);
    let (white, _) = overlay((700, 500), (100, 100, 300, 300), [255; 4]);
    let result =
        unsafe { dr_doc_painting_layer_blend(base.0, 0, white.as_ptr(), 700, 500, 1, 1.0) };
    assert!(result.is_null());
    let (dab, _) = overlay((700, 500), (100, 100, 300, 300), [40, 40, 40, 255]);
    let multiplied = Doc::checked(unsafe {
        dr_doc_painting_layer_blend(base.0, 0, dab.as_ptr(), 700, 500, 1, 1.0)
    });
    assert_eq!(
        base.raster(0).shared_tile_count(multiplied.raster(0)),
        tiles(700, 500) - touched((700, 500), (100, 100, 300, 300)).len()
    );
}

#[test]
fn with_layer_pixels_shares_every_unchanged_tile() {
    let _serial = serial();
    let (width, height) = (1000, 700);
    let image = patterned(width, height);
    let base = Doc::from_image(&image);
    let mut edited = image.clone();
    edited.put_pixel(600, 300, Rgba([9, 9, 9, 9]));
    let one = Doc::checked(rgba_layer::with_layer_rgba(
        base.0,
        0,
        edited.as_raw(),
        width,
        height,
    ));
    assert_eq!(
        base.raster(0).shared_tile_count(one.raster(0)),
        tiles(width, height) - 1
    );
    assert_eq!(*one.raster(0).contiguous_image(), edited);

    evict();
    let _view = base.raster(0).contiguous_image();
    let equal = Doc::checked(rgba_layer::with_layer_rgba(
        base.0,
        0,
        image.as_raw(),
        width,
        height,
    ));
    assert!(!LayerRaster::ptr_eq(base.raster(0), equal.raster(0)));
    assert_eq!(
        base.raster(0).shared_tile_count(equal.raster(0)),
        tiles(width, height)
    );
    let measured = report(&[&base, &equal]);
    assert_eq!(
        category(&measured, "contiguous_views", "allocation_count"),
        1,
        "one view per plane"
    );
    assert_eq!(*equal.raster(0).contiguous_image(), image);
}

#[test]
fn a_transparency_locked_paint_shares_the_untouched_tiles() {
    let _serial = serial();
    let (width, height) = (1200, 900);
    let image = patterned(width, height);
    let base = Doc::from_image(&image);
    let locked = Doc::checked(unsafe { dr_doc_with_layer_locks(base.0, 0, 1) });
    let (paint, _) = overlay((width, height), (100, 100, 300, 300), DAB);
    let painted = Doc::checked(locked.paint(0, &paint));
    assert!(locked.raster(0).shared_tile_count(painted.raster(0)) >= tiles(width, height) - 4);
    let after = painted.raster(0).contiguous_image();
    for (x, y, pixel) in image.enumerate_pixels() {
        assert_eq!(
            after.get_pixel(x, y)[3],
            pixel[3],
            "alpha frozen at ({x}, {y})"
        );
    }
}

#[test]
fn the_report_counts_shared_tiles_once_and_views_as_cache() {
    let _serial = serial();
    let (width, height) = phase4_corpus::DIMENSIONS[0];
    let image = phase4_corpus::image(width, height);
    let base = Doc::from_image(&image);
    let rect = (250, 200, 300, 300);
    let (paint, _) = overlay((width, height), (250, 200, 300, 300), DAB);
    let dabbed = Doc::checked(base.paint(0, &paint));
    drop(paint);
    let intersected: u64 = touched((width, height), rect).iter().sum();
    let measured = report(&[&base, &dabbed]);
    assert_eq!(
        category(&measured, "layer_pixels", "unique_bytes"),
        u64::from(width) * u64::from(height) * 4 + intersected
    );
    assert!(category(&measured, "tile_structure", "allocation_count") > 0);
    let views = measured["contiguous_view_bytes"].as_u64().unwrap();
    assert!(views <= CONTIGUOUS_CACHE_BYTES);
    assert!(views <= measured["cache_bytes"].as_u64().unwrap());

    let before = dabbed.raster(0).contiguous_image();
    evict();
    let evicted = report(&[&base, &dabbed]);
    assert_eq!(evicted["contiguous_view_bytes"].as_u64(), Some(0));
    assert_eq!(contiguous_view_bytes(), 0);
    let again = dabbed.raster(0).contiguous_image();
    assert!(!Arc::ptr_eq(&before, &again), "re-materialized");
    assert!(*before == *again);
}

#[test]
fn a_projection_leaves_no_view_behind() {
    let _serial = serial();
    let document = DrDocument::from_pixels(patterned(1024, 1024))
        .adding_image_layer(0, patterned(1024, 1024), "Second")
        .unwrap()
        .adding_image_layer(1, patterned(1024, 1024), "Third")
        .unwrap();
    let doc = Doc(Box::into_raw(Box::new(document)));
    evict();
    let first = doc.get().flattened().unwrap();
    let second = doc.get().flattened().unwrap();
    assert_eq!(first, second);
    let measured = report(&[&doc]);
    assert_eq!(
        category(&measured, "contiguous_views", "allocation_count"),
        0
    );
}

#[test]
fn concurrent_readers_share_one_materialized_view() {
    let _serial = serial();
    let image = patterned(2048, 2048);
    let doc = Doc::from_image(&image);
    evict();
    let raster = doc.raster(0);
    let barrier = Barrier::new(8);
    let views: Vec<Arc<RgbaImage>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    raster.contiguous_image()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert!(views.iter().all(|v| Arc::ptr_eq(v, &views[0])));
    assert!(*views[0] == image);
    let measured = report(&[&doc]);
    assert_eq!(
        category(&measured, "contiguous_views", "allocation_count"),
        1
    );
}

#[test]
fn the_view_cache_holds_at_most_its_entry_bound() {
    let _serial = serial();
    let mut document = DrDocument::from_pixels(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255])));
    for i in 1..300usize {
        let pixel = RgbaImage::from_pixel(1, 1, Rgba([i as u8, (i >> 8) as u8, 0, 255]));
        document = document.adding_image_layer(i - 1, pixel, "L").unwrap();
    }
    let doc = Doc(Box::into_raw(Box::new(document)));
    assert_eq!(doc.get().layers.len(), 300);
    evict();
    for layer in &doc.get().layers {
        let _ = layer.pixels.contiguous_image();
    }
    let measured = report(&[&doc]);
    let count = category(&measured, "contiguous_views", "allocation_count");
    assert!(count > 0 && count <= 256, "{count}");
}

#[test]
#[ignore = "allocates ~2.2 GB: seventeen 64 MiB views plus their tiles"]
fn view_bound_evicts_least_recently_used() {
    let _serial = serial();
    evict();
    let side = 4096;
    // Each view is taken right after its raster, so raster 0 is the least
    // recently used when the seventeenth (1088 MiB > the bound) arrives.
    let mut rasters = Vec::new();
    let mut held: Vec<Arc<RgbaImage>> = Vec::new();
    for i in 0..17u8 {
        let raster = LayerRaster::new(RgbaImage::from_pixel(side, side, Rgba([i, 1, 2, 255])));
        held.push(raster.contiguous_image());
        rasters.push(raster);
    }
    assert!(contiguous_view_bytes() <= CONTIGUOUS_CACHE_BYTES);
    let last = rasters[16].contiguous_image();
    assert!(Arc::ptr_eq(&last, &held[16]), "the most recent view hits");
    let first = rasters[0].contiguous_image();
    assert!(
        !Arc::ptr_eq(&first, &held[0]),
        "the oldest view was evicted"
    );
    assert!(*first == *held[0]);
    println!(
        "VIEW_BOUND views={} bytes={} bound={}",
        held.len(),
        contiguous_view_bytes(),
        CONTIGUOUS_CACHE_BYTES
    );
}
