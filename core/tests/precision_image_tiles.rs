//! Black-box suite for 13B-3b-1: typed `PrecisionImage` samples held as
//! 256×256 tiles (the typed twin of `layer_raster_tiles.rs`), the view cache
//! it shares with byte rasters, the handle-pinned FFI lend and (13B-3e-4a,
//! `the_storage_identity_names_the_shared_plane`) the storage identity the
//! FFI exports in the lend pointer's place. Oracles are
//! independent of the implementation: expected samples are a patterned `Vec`
//! built from a closure before the constructor adopts it, expected report
//! figures are dimension products, and identity is a second construction of
//! the same samples.
//!
//! The view cache is process-wide, so every test holds `serial()`: a count
//! of `contiguous_views` must not see another test's images.

use std::ptr;
use std::sync::Arc;

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::ffi_precision::{
    dr_precision_image_pixels_rgba16, dr_precision_image_storage_identity,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use serde_json::Value;

use crate::resource_report;
use crate::serial;

/// The sizes around one tile edge (256) plus a multi-tile plane.
const SIZES: [(u32, u32); 5] = [(1, 1), (255, 257), (256, 256), (257, 255), (1000, 700)];

fn pattern16(width: u32, height: u32) -> Vec<[u16; 4]> {
    (0..width * height)
        .map(|i| {
            [
                (i.wrapping_mul(977) & 0xffff) as u16,
                (65535 - (i.wrapping_mul(131) & 0xffff)) as u16,
                ((i ^ 0x5a5a) & 0xffff) as u16,
                if i.is_multiple_of(3) {
                    0
                } else {
                    (30000 + i % 30000) as u16
                },
            ]
        })
        .collect()
}

fn pattern32(width: u32, height: u32) -> Vec<[f32; 4]> {
    let n = (width * height) as f32;
    (0..width * height)
        .map(|i| {
            let v = i as f32 / n;
            [
                v,
                1.0 - v,
                v * 0.5,
                if i.is_multiple_of(2) { 1.0 } else { v },
            ]
        })
        .collect()
}

fn image16(width: u32, height: u32, pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(width, height, pixels, IccProfile::srgb(), None).unwrap()
}

fn image32(width: u32, height: u32, pixels: Vec<[f32; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba32(width, height, pixels, IccProfile::srgb(), None).unwrap()
}

fn bits(samples: &[[f32; 4]]) -> Vec<[u32; 4]> {
    samples.iter().map(|p| p.map(f32::to_bits)).collect()
}

fn report(images: &[&PrecisionImage]) -> Value {
    let handles: Vec<*const PrecisionImage> =
        images.iter().map(|i| *i as *const PrecisionImage).collect();
    resource_report::report(&[], &[], &[], &handles, 0, 0).expect("a resource report")
}

fn evict() {
    resource_report::evict(&[]).expect("an eviction report");
}

fn category(report: &Value, category: &str, field: &str) -> u64 {
    report["categories"][category][field].as_u64().unwrap_or(0)
}

fn has_category(report: &Value, category: &str) -> bool {
    report["categories"].get(category).is_some()
}

#[test]
fn round_trips_are_exact_on_both_storages_around_the_tile_edge() {
    let _serial = serial();
    for (w, h) in SIZES {
        let expected = pattern16(w, h);
        let image = image16(w, h, expected.clone());
        assert_eq!(image.to_rgba16().unwrap(), expected, "{w}×{h} u16 copy");
        assert_eq!(
            image
                .runs_rgba16()
                .unwrap()
                .flatten()
                .copied()
                .collect::<Vec<_>>(),
            expected,
            "{w}×{h} u16 runs"
        );
        let twin = image16(w, h, expected.clone());
        assert!(image.same_samples(&twin), "{w}×{h} u16 same_samples");
        assert!(!PrecisionImage::ptr_eq(&image, &twin));

        let expected = pattern32(w, h);
        let image = image32(w, h, expected.clone());
        assert_eq!(
            bits(&image.to_rgba32().unwrap()),
            bits(&expected),
            "{w}×{h} f32 copy"
        );
        assert_eq!(
            bits(
                &image
                    .runs_rgba32()
                    .unwrap()
                    .flatten()
                    .copied()
                    .collect::<Vec<_>>()
            ),
            bits(&expected),
            "{w}×{h} f32 runs"
        );
        let twin = image32(w, h, expected.clone());
        assert!(image.same_samples(&twin), "{w}×{h} f32 same_samples");
        assert!(!PrecisionImage::ptr_eq(&image, &twin));
    }
}

#[test]
fn same_samples_sees_one_changed_sample_in_a_tiled_plane() {
    let _serial = serial();
    let (w, h) = (600, 300);
    let expected = pattern16(w, h);
    let mut changed = expected.clone();
    changed[(299 * w + 599) as usize][2] ^= 1;
    assert!(!image16(w, h, expected.clone()).same_samples(&image16(w, h, changed)));
    let mut changed = pattern32(w, h);
    changed[(257 * w + 300) as usize][0] = -0.0;
    let float = image32(w, h, pattern32(w, h));
    let other = image32(w, h, changed);
    assert!(!float.same_samples(&other), "-0.0 differs from 0.0 bitwise");
    assert!(
        !float.same_samples(&image16(w, h, expected)),
        "storage differs"
    );
}

/// `pixel(index)` is the per-sample read a cross-storage `convert` walks for
/// every index, so the converted plane is the oracle for each sample.
#[test]
fn every_pixel_of_a_multi_tile_plane_reads_its_own_sample() {
    let _serial = serial();
    let (w, h) = (300, 300);
    let expected = pattern16(w, h);
    let image = image16(w, h, expected.clone());
    let float = image
        .convert(
            PrecisionStorage::Rgba32Float,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap();
    let converted = float.to_rgba32().unwrap();
    assert_eq!(converted.len(), expected.len());
    for (i, (actual, source)) in converted.iter().zip(&expected).enumerate() {
        assert_eq!(
            actual.map(f32::to_bits),
            source.map(|v| (f32::from(v) / 65535.0).to_bits()),
            "pixel {i}"
        );
    }

    let expected = pattern32(w, h);
    let image = image32(w, h, expected.clone());
    let back = image
        .convert(
            PrecisionStorage::Rgba16Unorm,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .to_rgba16()
        .unwrap();
    for (i, (actual, source)) in back.iter().zip(&expected).enumerate() {
        assert_eq!(
            *actual,
            source.map(|v| (v * 65535.0).round() as u16),
            "pixel {i}"
        );
    }
}

#[test]
fn identity_is_shared_by_clone_profile_rebind_and_same_storage_convert() {
    let _serial = serial();
    let (w, h) = (300, 280);
    let image = image16(w, h, pattern16(w, h));
    assert!(PrecisionImage::ptr_eq(&image, &image.clone()));
    let same = image
        .convert(
            PrecisionStorage::Rgba16Unorm,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap();
    assert!(PrecisionImage::ptr_eq(&image, &same));
    // Assign Profile reinterprets the samples through `with_profile`.
    let document =
        PrecisionDocument::from_image(image.clone(), Metadata::default(), Resolution::default())
            .unwrap();
    let assigned = document
        .assign_profile(&IccProfile::display_p3(), REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .expect("a different profile is an edit");
    let rebound = assigned.layer_source(0).unwrap();
    assert!(!Arc::ptr_eq(rebound.profile(), image.profile()));
    assert!(PrecisionImage::ptr_eq(rebound, &image));

    assert!(!PrecisionImage::ptr_eq(
        &image,
        &image16(w, h, pattern16(w, h))
    ));
    let float = image
        .convert(
            PrecisionStorage::Rgba32Float,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap();
    assert!(!PrecisionImage::ptr_eq(&image, &float));
}

#[test]
fn a_construction_seeds_the_cache_with_the_adopted_buffer() {
    let _serial = serial();
    evict();
    let (w, h) = (700, 300);
    let pixels = pattern16(w, h);
    assert_eq!(pixels.capacity(), pixels.len());
    let before = contiguous_view_bytes();
    let image = image16(w, h, pixels);
    assert_eq!(
        contiguous_view_bytes() - before,
        u64::from(w) * u64::from(h) * 8,
        "the seed is the adopted buffer"
    );
    drop(image);

    let pixels = pattern32(w, h);
    let before = contiguous_view_bytes();
    let float = image32(w, h, pixels);
    assert_eq!(
        contiguous_view_bytes() - before,
        0,
        "no f32 constructor seeds since 13B-3e-4c-2a"
    );
    drop(float);
}

#[test]
fn slack_capacity_moves_from_storage_to_the_seeded_view() {
    let _serial = serial();
    evict();
    let mut pixels = Vec::with_capacity(1024);
    pixels.push([12345u16, 30001, 40001, 65535]);
    let before = contiguous_view_bytes();
    let image = image16(1, 1, pixels);
    assert_eq!(image.storage_bytes(), 8, "a tiled plane is w·h·bpp");
    assert_eq!(
        contiguous_view_bytes() - before,
        8192,
        "the view keeps capacity"
    );
}

#[test]
fn the_report_counts_tiles_the_index_and_a_u16_seed_once() {
    let _serial = serial();
    evict();
    let (w, h) = (600, 520);
    for storage in [PrecisionStorage::Rgba16Unorm, PrecisionStorage::Rgba32Float] {
        let (image, bpp) = match storage {
            PrecisionStorage::Rgba16Unorm => (image16(w, h, pattern16(w, h)), 8),
            PrecisionStorage::Rgba32Float => (image32(w, h, pattern32(w, h)), 16),
        };
        let bytes = u64::from(w) * u64::from(h) * bpp;
        // Only the u16 constructor seeds; no f32 one does since 13B-3e-4c-2a.
        let (seeds, seed_bytes) = match storage {
            PrecisionStorage::Rgba16Unorm => (1, bytes),
            PrecisionStorage::Rgba32Float => (0, 0),
        };
        assert_eq!(image.storage_bytes(), bytes);
        let measured = report(&[&image]);
        assert_eq!(
            category(&measured, "precision_pixels", "unique_bytes"),
            bytes
        );
        assert_eq!(
            category(&measured, "precision_pixels", "allocation_count"),
            9,
            "3×3 tiles"
        );
        assert!(has_category(&measured, "tile_structure"));
        assert!(!has_category(&measured, "precision_buffer"));
        assert!(!has_category(&measured, "precision_views"));
        assert_eq!(
            category(&measured, "contiguous_views", "allocation_count"),
            seeds,
            "the seed counts once"
        );
        assert_eq!(measured["contiguous_view_bytes"].as_u64(), Some(seed_bytes));
        let twice = report(&[&image, &image.clone()]);
        assert_eq!(
            category(&twice, "contiguous_views", "allocation_count"),
            seeds,
            "a clone shares the plane and its one view"
        );

        let retained = image.retained_bytes();
        match storage {
            PrecisionStorage::Rgba16Unorm => drop(image.to_rgba16().ok()),
            PrecisionStorage::Rgba32Float => drop(image.to_rgba32().ok()),
        }
        assert_eq!(image.retained_bytes(), retained, "a copy retains nothing");

        evict();
        let measured = report(&[&image]);
        assert_eq!(
            category(&measured, "contiguous_views", "allocation_count"),
            0
        );
        assert_eq!(measured["contiguous_view_bytes"].as_u64(), Some(0));
        assert_eq!(
            category(&measured, "precision_pixels", "unique_bytes"),
            bytes
        );
    }
}

#[test]
fn the_ffi_lend_is_pinned_by_the_handle() {
    let _serial = serial();
    evict();
    let (w, h) = (520, 300);
    let expected = pattern16(w, h);
    let image = image16(w, h, expected.clone());
    let bytes = u64::from(w) * u64::from(h) * 8;
    // The constructor seeded the cache; the lend neither reads nor writes it.
    let views = contiguous_view_bytes();
    assert_eq!(views, bytes, "the seed");
    let first = unsafe { dr_precision_image_pixels_rgba16(&image) };
    assert!(!first.is_null());
    assert_eq!(unsafe { dr_precision_image_pixels_rgba16(&image) }, first);
    assert_eq!(contiguous_view_bytes(), views, "a lend writes no view");
    // Since 13B-3e-4a the pin is a PRIVATE copy, retained as
    // `precision_views`; the seed is a separate cache allocation.
    let measured = report(&[&image]);
    assert_eq!(
        category(&measured, "precision_views", "unique_bytes"),
        bytes
    );
    assert_eq!(measured["contiguous_view_bytes"].as_u64(), Some(bytes));
    assert_eq!(
        measured["cache_bytes"].as_u64(),
        Some(bytes),
        "the seed is cache, the pin is retained"
    );
    evict();
    let measured = report(&[&image]);
    assert_eq!(
        category(&measured, "precision_views", "unique_bytes"),
        bytes
    );
    assert_eq!(measured["contiguous_view_bytes"].as_u64(), Some(0));
    assert_eq!(measured["cache_bytes"].as_u64(), Some(0));
    assert_eq!(
        unsafe { dr_precision_image_pixels_rgba16(&image) },
        first,
        "an eviction cannot move the lend"
    );
    let lent = unsafe { std::slice::from_raw_parts(first, (w * h * 4) as usize) };
    assert_eq!(lent, expected.as_flattened());

    let clone = image.clone();
    let cloned = unsafe { dr_precision_image_pixels_rgba16(&clone) };
    assert_ne!(cloned, first, "a clone pins its own private copy");
    let lent = unsafe { std::slice::from_raw_parts(cloned, (w * h * 4) as usize) };
    assert_eq!(lent, expected.as_flattened());

    // Each handle retains its own pin; neither is a cached view.
    let measured = report(&[&image, &clone]);
    assert_eq!(
        category(&measured, "precision_views", "unique_bytes"),
        2 * bytes
    );
    assert_eq!(measured["contiguous_view_bytes"].as_u64(), Some(0));
    assert_eq!(measured["cache_bytes"].as_u64(), Some(0));
    assert_eq!(contiguous_view_bytes(), 0);

    let float = image32(w, h, pattern32(w, h));
    assert!(unsafe { dr_precision_image_pixels_rgba16(&float) }.is_null());
    assert_eq!(
        float
            .to_rgba32()
            .unwrap()
            .as_flattened()
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        pattern32(w, h)
            .as_flattened()
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>()
    );
    assert!(image.to_rgba32().is_err());
    assert!(
        !has_category(&report(&[&float]), "precision_views"),
        "an f32 image pins nothing"
    );
}

#[test]
fn the_storage_identity_names_the_shared_plane() {
    let _serial = serial();
    evict();
    let identity = |image: &PrecisionImage| unsafe { dr_precision_image_storage_identity(image) };
    assert_eq!(
        unsafe { dr_precision_image_storage_identity(ptr::null()) },
        0,
        "NULL"
    );
    let (w, h) = (520, 300);
    let expected = pattern16(w, h);
    let image = image16(w, h, expected.clone());
    let key = identity(&image);
    assert_ne!(key, 0);
    let clone = image.clone();
    let views = contiguous_view_bytes();
    assert_eq!(identity(&clone), key, "a clone shares the storage");
    assert_eq!(contiguous_view_bytes(), views, "the identity reads no view");
    assert!(
        !has_category(&report(&[&clone]), "precision_views"),
        "the identity pins nothing"
    );
    assert!(!unsafe { dr_precision_image_pixels_rgba16(&image) }.is_null());
    evict();
    assert_eq!(identity(&image), key, "a lend and an eviction move nothing");

    let twin = image16(w, h, expected.clone());
    assert_ne!(identity(&twin), key, "a second construction");
    let mut changed = expected[0];
    changed[0] ^= 1;
    let edited = image
        .with_rect_rgba16(0, 0, 1, 1, &[changed])
        .unwrap()
        .unwrap();
    assert_ne!(identity(&edited), key, "a changed sample");
    let tiles = (w.div_ceil(256) * h.div_ceil(256)) as usize;
    assert_eq!(edited.shared_tile_count(&image), tiles - 1);

    let document =
        PrecisionDocument::from_image(image.clone(), Metadata::default(), Resolution::default())
            .unwrap();
    assert_eq!(
        identity(document.layer_source(0).unwrap()),
        key,
        "layer_source"
    );
    let assigned = document
        .assign_profile(&IccProfile::display_p3(), REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        identity(assigned.layer_source(0).unwrap()),
        key,
        "a profile assignment"
    );
    let moved = document
        .geometry(
            &GeometryEdit::parse(
                r#"{"op":"affine_set","layers":[0],"matrix":[1,0,0,1,11,5],"filter":"nearest"}"#,
            )
            .unwrap(),
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        identity(moved.layer_source(0).unwrap()),
        key,
        "an integer move shares the storage"
    );
    let resampled = document
        .geometry(
            &GeometryEdit::parse(
                r#"{"op":"affine_set","layers":[0],"matrix":[1,0,0,1,0.5,0],"filter":"bilinear"}"#,
            )
            .unwrap(),
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_ne!(
        identity(resampled.layer_source(0).unwrap()),
        key,
        "a fractional move resamples"
    );

    let float = image32(w, h, pattern32(w, h));
    let float_key = identity(&float);
    assert_ne!(float_key, 0);
    assert_eq!(identity(&float.clone()), float_key, "an f32 clone");
    assert_ne!(
        identity(&image32(w, h, pattern32(w, h))),
        float_key,
        "a second f32 construction"
    );
}

#[test]
fn a_refused_buffer_seeds_nothing() {
    let _serial = serial();
    let (w, h) = (300, 300);
    let mut pixels = pattern32(w, h);
    pixels[w as usize * 200 + 100][1] = f32::NAN;
    let before = contiguous_view_bytes();
    assert!(PrecisionImage::from_rgba32(w, h, pixels, IccProfile::srgb(), None).is_err());
    assert_eq!(contiguous_view_bytes(), before);
}

/// A plane the tile store refuses stays contiguous; the test seam
/// `contiguous_rgba32_extended` leaves one so (no other plane is). Its
/// storage identity is the adopted buffer and the report names it.
#[test]
fn the_contiguous_fallback_reports_its_buffer() {
    let _serial = serial();
    evict();
    let edge = 300u32;
    let n = edge as usize * edge as usize;
    let pixels = vec![[0.25f32, 0.5, 0.75, 1.0]; n];
    let pointer = pixels.as_ptr();
    let capacity = pixels.capacity() as u64;
    let before = contiguous_view_bytes();
    let image =
        PrecisionImage::contiguous_rgba32_extended(edge, edge, pixels, IccProfile::srgb()).unwrap();
    assert_eq!(
        unsafe { dr_precision_image_storage_identity(&image) },
        pointer as u64
    );
    assert_eq!(contiguous_view_bytes(), before, "no seed, no view");
    let measured = report(&[&image]);
    assert!(has_category(&measured, "precision_buffer"));
    assert!(!has_category(&measured, "tile_structure"));
    assert_eq!(image.storage_bytes(), capacity * 16);
    let tiny = image32(1, 1, vec![[0.25, 0.5, 0.75, 1.0]]);
    assert!(!image.same_samples(&tiny));
    assert!(!tiny.same_samples(&image));
}

/// 8193² f32 = 1,074,003,984 bytes: above `CONTIGUOUS_CACHE_BYTES` (a view
/// the cache would refuse), admitted under `REFERENCE_WORKING_BYTES`. Such
/// an adopted plane is tiled (33 × 33 tiles), not kept as
/// the adopted buffer, and nothing is seeded.
#[test]
#[ignore = "allocates ~2.2 GB; run once per float-plane tiling change"]
fn an_adopted_plane_above_the_cache_bound_is_tiled() {
    let _serial = serial();
    evict();
    let edge = 8193u32;
    let n = edge as usize * edge as usize;
    let pixels = vec![[0.25f32, 0.5, 0.75, 1.0]; n];
    let pointer = pixels.as_ptr();
    let before = contiguous_view_bytes();
    let image = image32(edge, edge, pixels);
    assert_ne!(
        unsafe { dr_precision_image_storage_identity(&image) },
        pointer as u64
    );
    assert_eq!(contiguous_view_bytes(), before, "no seed, no view");
    let measured = report(&[&image]);
    assert!(has_category(&measured, "tile_structure"));
    assert!(!has_category(&measured, "precision_buffer"));
    assert_eq!(image.shared_tile_count(&image), 1089);
}
