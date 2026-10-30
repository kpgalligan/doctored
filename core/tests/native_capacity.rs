//! Native writer/reader capacity boundaries, early refusal and retained work.
//! Large logical documents share tiny allocations; crafted PNG headers prove
//! preflight ordering without inflating hundreds of millions of pixels.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{DrDocument, LayerRaster};
use doctored_core::ffi_doc::*;
use image::{GrayImage, Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common;
use common::*;

fn save_error(doc: &DrDocument, path: &std::path::Path) -> String {
    let path = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    assert!(!unsafe { dr_doc_save_native(doc, path.as_ptr(), &mut err) });
    take_err_string(err)
}

fn open_error(bytes: &[u8], dir: &TempDir) -> String {
    let path = dir.path().join("crafted.rz");
    std::fs::write(&path, bytes).unwrap();
    let path = cpath(&path);
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_doc_open(path.as_ptr(), &mut err) }.is_null());
    take_err_string(err)
}

fn shared_layers(side: u32, count: usize) -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(side, side, Rgba(RED)));
    doc.layers = vec![doc.layers[0].clone(); count];
    // ONE pixel allocation, but DISTINCT identities. Ids are persisted since
    // 12E-1 and a repeated id is refused at BOTH ends (`identity_native`);
    // no real op can produce one, because every duplicate mints. Cloning the
    // struct is this fixture's way of sharing the pixels, so it has to mint
    // as well.
    for (index, layer) in doc.layers.iter_mut().enumerate() {
        layer.id = 1 + index as u64;
    }
    doc
}

#[test]
fn writer_refuses_aggregate_before_encoding_and_preserves_previous_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("work.rz");
    let good = DrDocument::from_pixels(solid(1, 1, BLUE));
    good.save_native(path.to_str().unwrap()).unwrap();
    let previous = std::fs::read(&path).unwrap();
    // 626² × 1024 = 401,281,024 pixels. Actual retained pixels = 1.5 MiB.
    let oversized = shared_layers(626, 1024);
    let err = save_error(&oversized, &path);
    assert!(err.contains("total layer pixels exceed 400000000"), "{err}");
    assert!(
        err.contains("reduce image size") && err.contains("export"),
        "{err}"
    );
    assert!(err.contains(path.to_str().unwrap()), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), previous);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    assert_eq!(
        DrDocument::open(path.to_str().unwrap()).unwrap().layers[0]
            .pixels
            .contiguous_image()
            .get_pixel(0, 0)
            .0,
        BLUE
    );
    assert!(LayerRaster::ptr_eq(
        &oversized.layers[0].pixels,
        &oversized.layers[1023].pixels
    ));
}

#[test]
fn ordinary_duplicate_add_resize_and_transform_refuse_unsupported_growth() {
    let doc = shared_layers(625, 1023);
    // Exact boundary: 625² × 1024 = 400,000,000 pixels. Both the native
    // pixel limit and the layer-count limit are inclusive.
    let boundary = unsafe { dr_doc_duplicating_layer(&doc, 0) };
    assert!(!boundary.is_null());
    assert_eq!(unsafe { dr_doc_layer_count(boundary) }, 1024);
    assert!(unsafe { dr_doc_duplicating_layer(boundary, 0) }.is_null());
    assert!(unsafe { dr_doc_resize(boundary, 626, 626, FILTER_NEAREST) }.is_null());
    let one_more_pixel = RgbaImage::new(626, 625);
    assert!(unsafe { &*boundary }
        .with_layer_pixels(0, one_more_pixel)
        .is_none());
    assert!(unsafe { &*boundary }
        .transform_layer(
            0,
            doctored_core::doc_transform::Affine::from_array([1.01, 0.0, 0.0, 1.0, 0.0, 0.0]),
            image::imageops::FilterType::Nearest
        )
        .is_none());
    unsafe { dr_doc_free(boundary) };
    let pixels_limit = shared_layers(626, 1020);
    assert!(unsafe { dr_doc_duplicating_layer(&pixels_limit, 0) }.is_null());
    assert!(pixels_limit.adding_layer(0, "New layer").is_none());
}

#[test]
fn oversized_memory_state_remains_editable_reducible_and_exportable() {
    let dir = TempDir::new().unwrap();
    let mut oversized = shared_layers(626, 1024);
    // Off-canvas retained pixels count for native capacity, while the visible
    // canvas and a flattened recovery copy can still be tiny.
    oversized.width = 1;
    oversized.height = 1;
    let export_path = dir.path().join("recovered.png");
    let export_cpath = cpath(&export_path);
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe {
            doctored_core::ffi_color::dr_doc_save_image(
                &oversized,
                ptr::null(),
                export_cpath.as_ptr(),
                0,
                90,
                true,
                false,
                ptr::null_mut(),
                &mut err,
            )
        },
        "{}",
        take_err_string(err)
    );
    assert_eq!(
        image::open(&export_path)
            .unwrap()
            .to_rgba8()
            .get_pixel(0, 0)
            .0,
        RED
    );
    assert_eq!(
        oversized.layers.len(),
        1024,
        "export leaves the editable source intact"
    );
    let reduced = unsafe { dr_doc_removing_layer(&oversized, 0) };
    assert!(
        !reduced.is_null(),
        "partial reductions must remain possible above the cap"
    );
    assert_eq!(unsafe { dr_doc_layer_count(reduced) }, 1023);
    let named = unsafe { dr_doc_with_layer_name(reduced, 0, c"Recovered layer".as_ptr()) };
    assert!(!named.is_null(), "same-size edits remain possible");
    let flattened = unsafe { dr_doc_flattening(named) };
    assert!(!flattened.is_null(), "flattened recovery remains available");
    let path = dir.path().join("recovered.rz");
    unsafe { &*flattened }
        .save_native(path.to_str().unwrap())
        .unwrap();
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    assert_eq!(reopened.layers.len(), 1);
    assert_eq!(
        reopened.layers[0]
            .pixels
            .contiguous_image()
            .get_pixel(0, 0)
            .0,
        RED
    );
    unsafe {
        dr_doc_free(reduced);
        dr_doc_free(named);
        dr_doc_free(flattened);
    }
}

#[test]
fn writer_refuses_invalid_mask_instead_of_silently_discarding_it() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("mask.rz");
    let mut doc = DrDocument::from_pixels(solid(2, 2, RED));
    doc.save_native(path.to_str().unwrap()).unwrap();
    let previous = std::fs::read(&path).unwrap();
    doc.layers[0].mask = Some(Arc::new(GrayImage::new(0, 1)));
    let err = save_error(&doc, &path);
    assert!(err.contains("mask dimensions"), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), previous);
}

#[test]
fn writer_refuses_canvas_and_count_states_the_reader_cannot_open() {
    let dir = TempDir::new().unwrap();
    let mut doc = DrDocument::from_pixels(solid(1, 1, RED));
    doc.width = 0;
    assert!(save_error(&doc, &dir.path().join("zero.rz")).contains("invalid canvas size"));
    doc.width = 1;
    doc.layers.clear();
    assert!(save_error(&doc, &dir.path().join("empty.rz")).contains("invalid layer count"));
    let doc = shared_layers(1, 1025);
    assert!(save_error(&doc, &dir.path().join("many.rz")).contains("invalid layer count"));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

/// Independently authored version-8 records. Their PNG headers are valid but
/// their deflate data still holds one pixel, so decoding any large layer would
/// fail. A bad guide at the end lets the exact capacity boundary finish all
/// preflight checks without starting such a decode.
fn craft(dims: &[(u32, u32)], bad_guide: bool) -> Vec<u8> {
    let mut out = b"RZDC".to_vec();
    for value in [8u32, 1, 1, dims.len() as u32] {
        out.extend(value.to_le_bytes());
    }
    out.extend(120.0f32.to_le_bytes());
    out.extend(30.0f32.to_le_bytes());
    for &(w, h) in dims {
        out.extend(0u32.to_le_bytes()); // name
        out.extend([0u8; 8]); // offsets
        out.extend(1.0f32.to_le_bytes());
        out.extend(0u32.to_le_bytes()); // normal
        out.push(1); // visible
        let png = png_header(w, h);
        out.extend((png.len() as u32).to_le_bytes());
        out.extend(png);
        out.extend([0, 1, 0, 0, 0]); // mask absent/enabled, meta/clipped/style absent
        out.extend([0u8; 11]); // locks/link/depth/kind
        out.push(1); // group-open
    }
    out.extend(0u32.to_le_bytes()); // channels
    out.extend(72.0f32.to_le_bytes());
    out.extend(72.0f32.to_le_bytes());
    out.extend([0u8; 4]); // ICC, EXIF, XMP, IPTC absent
    out.extend([0u8; 16]); // ruler origin
    out.extend(u32::from(bad_guide).to_le_bytes());
    if bad_guide {
        out.push(2);
        out.extend([0u8; 8]);
    }
    out
}

fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    solid(1, 1, RED)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes[16..20].copy_from_slice(&width.to_be_bytes());
    bytes[20..24].copy_from_slice(&height.to_be_bytes());
    // PNG's standard reflected IEEE CRC-32, independently computed over the
    // IHDR type and data. No image pixels are created for the declared size.
    let mut crc = !0u32;
    for &byte in &bytes[12..29] {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    bytes[29..33].copy_from_slice(&(!crc).to_be_bytes());
    bytes
}

#[test]
fn reader_preflights_aggregate_before_decoding_even_the_first_png() {
    let dir = TempDir::new().unwrap();
    let mut dims = vec![(10_000, 10_000); 4];
    dims.push((1, 1));
    let err = open_error(&craft(&dims, false), &dir);
    assert!(err.contains("total layer pixels exceed 400000000"), "{err}");
    assert!(!err.contains("decode layer"), "{err}");
}

#[test]
fn reader_accepts_exact_aggregate_boundary_before_validating_the_tail() {
    let dir = TempDir::new().unwrap();
    let err = open_error(&craft(&[(10_000, 10_000); 4], true), &dir);
    assert!(err.contains("unsupported guide orientation 2"), "{err}");
}

#[test]
fn maximum_layer_count_round_trips() {
    let dir = TempDir::new().unwrap();
    let doc = shared_layers(1, 1024);
    let path = dir.path().join("max.rz");
    doc.save_native(path.to_str().unwrap()).unwrap();
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    assert_eq!(reopened.layers.len(), 1024);
    assert!(reopened
        .layers
        .iter()
        .all(|l| l.pixels.contiguous_image().get_pixel(0, 0).0 == RED));
}
