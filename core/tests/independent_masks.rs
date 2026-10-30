//! Independent mask storage, nondestructive coverage and v10 migration.
use std::ffi::CString;
use std::ptr;
use std::sync::Arc;

use crate::common;
use crate::serial;
use common::*;
use doctored_core::doc::{DrDocument, LayerRaster, MaskKind};
use doctored_core::doc_plane::Plane;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_mask::*;
use doctored_core::ffi_scope::*;
use image::RgbaImage;
use serde_json::Value;
use tempfile::TempDir;

use crate::resource_report;

fn owned(doc: *mut DrDocument) -> Box<DrDocument> {
    assert!(!doc.is_null(), "document operation refused");
    unsafe { Box::from_raw(doc) }
}
fn properties(doc: &DrDocument, idx: usize) -> Value {
    serde_json::from_str(&take_err_string(unsafe {
        dr_doc_layer_mask_properties(doc, idx)
    }))
    .unwrap()
}
fn effective(doc: &DrDocument, idx: usize) -> Vec<u8> {
    doc.layer_effective_mask_image(idx)
        .expect("an effective mask")
        .pixels()
        .map(|p| p[0])
        .collect()
}
fn translate(doc: &DrDocument, idx: usize, x: f64, y: f64) -> Box<DrDocument> {
    owned(unsafe {
        dr_doc_transform_mask(
            doc,
            idx,
            [1.0, 0.0, 0.0, 1.0, x, y].as_ptr(),
            FILTER_NEAREST,
        )
    })
}
fn configured(
    doc: &DrDocument,
    idx: usize,
    linked: bool,
    density: f32,
    feather: f32,
) -> Box<DrDocument> {
    owned(unsafe { dr_doc_set_mask_properties(doc, idx, linked, density, feather) })
}

#[test]
fn properties_preserve_raw_and_pixels_with_independent_gaussian_oracle() {
    let _serial = serial();
    let doc = mask_fixture((9, 7), (9, 7), (0, 0));
    let selection = selection(9, 7, |x, _| if x == 4 { 255 } else { 0 });
    let doc = doc
        .add_mask(1, MaskKind::FromSelection(&selection))
        .unwrap();
    let adjusted = configured(&doc, 1, false, 0.5, 1.0);
    assert!(LayerRaster::ptr_eq(
        &doc.layers[1].pixels,
        &adjusted.layers[1].pixels
    ));
    assert!(Arc::ptr_eq(
        doc.layers[1].mask.as_ref().unwrap(),
        adjusted.layers[1].mask.as_ref().unwrap()
    ));
    assert_eq!(adjusted.layer_plane(1, Plane::Mask).unwrap(), selection);
    let values = effective(&adjusted, 1);
    // Radius1 => sigma0.8 and a normalized three-tap Gaussian. Row3 is
    // farther than the support from top/bottom, making the 2D oracle 1D.
    let side = (-0.5_f64 / (0.8 * 0.8)).exp();
    let sum = 1.0 + 2.0 * side;
    for x in 0..9 {
        let blurred = match x {
            3 | 5 => 255.0 * side / sum,
            4 => 255.0 / sum,
            _ => 0.0,
        };
        let want = (255.0 - (255.0 - blurred.round()) * 0.5).round() as u8;
        assert_eq!(values[3 * 9 + x], want, "column{x}");
    }
    let reset = configured(&adjusted, 1, true, 1.0, 0.0);
    assert_eq!(effective(&reset, 1), selection);
    assert_eq!(reset.flattened().unwrap(), doc.flattened().unwrap());
    let reveal = configured(&adjusted, 1, false, 0.0, 1.0);
    assert!(effective(&reveal, 1).iter().all(|v| *v == 255));
    assert!(unsafe { dr_doc_set_mask_properties(&*reset, 1, true, 1.0, 0.0) }.is_null());
    for (density, feather) in [
        (f32::NAN, 0.0),
        (-0.1, 0.0),
        (1.1, 0.0),
        (1.0, -1.0),
        (1.0, 1001.0),
        (1.0, f32::INFINITY),
    ] {
        assert!(unsafe { dr_doc_set_mask_properties(&doc, 1, true, density, feather) }.is_null());
    }
}

#[test]
fn mask_equality_compares_raw_placement_properties_and_enabled_without_pixels() {
    let _serial = serial();
    let doc = mask_fixture((6, 4), (6, 4), (0, 0));
    let equal = |a: &DrDocument, b: &DrDocument| unsafe { dr_doc_layer_masks_equal(a, b, 1) };
    assert!(equal(&doc, &doc));
    let mask = doc.add_mask(1, MaskKind::RevealAll).unwrap();
    let repeated = mask.add_mask(1, MaskKind::RevealAll).unwrap();
    assert!(
        equal(&mask, &repeated),
        "equal separately allocated coverage is a no-op"
    );
    assert!(!equal(&doc, &mask));
    assert!(!equal(&mask, &mask.add_mask(1, MaskKind::HideAll).unwrap()));
    assert!(!equal(&mask, &configured(&mask, 1, false, 1.0, 0.0)));
    assert!(!equal(&mask, &configured(&mask, 1, true, 0.5, 0.0)));
    assert!(!equal(&mask, &configured(&mask, 1, true, 1.0, 1.0)));
    assert!(!equal(&mask, &mask.set_mask_enabled(1, false).unwrap()));
    assert!(!equal(&mask, &translate(&mask, 1, 1.0, 0.0)));
    let changed_pixels = mask.with_layer_pixels(1, solid(3, 2, GREEN)).unwrap();
    assert!(
        equal(&mask, &changed_pixels),
        "pixel changes do not change the mask"
    );
    let larger = owned(unsafe {
        dr_doc_transform_mask(
            &mask,
            1,
            [2.0, 0.0, 0.0, 1.0, 0.0, 0.0].as_ptr(),
            FILTER_NEAREST,
        )
    });
    assert!(!equal(&mask, &larger));
    assert!(!unsafe { dr_doc_layer_masks_equal(ptr::null(), &mask, 1) });
    assert!(!unsafe { dr_doc_layer_masks_equal(&mask, ptr::null(), 1) });
    assert!(!unsafe { dr_doc_layer_masks_equal(&mask, &mask, 99) });
}

#[test]
fn mask_affine_keeps_pixels_and_raw_extent_out_and_back() {
    let _serial = serial();
    let doc = mask_fixture((8, 6), (3, 2), (2, 1));
    let doc = doc
        .add_mask(
            1,
            MaskKind::FromSelection(&selection(8, 6, |x, y| (x * 17 + y * 31) as u8)),
        )
        .unwrap();
    let moved = translate(&doc, 1, 100.0, -80.0);
    assert!(LayerRaster::ptr_eq(
        &doc.layers[1].pixels,
        &moved.layers[1].pixels
    ));
    assert_eq!(doc.layers[1].offset, moved.layers[1].offset);
    assert!(Arc::ptr_eq(
        doc.layers[1].mask.as_ref().unwrap(),
        moved.layers[1].mask.as_ref().unwrap()
    ));
    assert_eq!(properties(&moved, 1)["offset_x"], 102);
    assert!(effective(&moved, 1).iter().all(|v| *v == 0));
    let back = translate(&moved, 1, -100.0, 80.0);
    assert_eq!(
        effective(&back, 1),
        doc.layer_plane(1, Plane::Mask).unwrap()
    );
    let rotation = [0.0, 1.0, -1.0, 0.0, 7.0, 0.0];
    let rotated =
        owned(unsafe { dr_doc_transform_mask(&doc, 1, rotation.as_ptr(), FILTER_NEAREST) });
    assert_eq!(properties(&rotated, 1)["width"], 2);
    assert_eq!(properties(&rotated, 1)["height"], 3);
    assert_eq!(properties(&rotated, 1)["offset_x"], 4);
    assert_eq!(properties(&rotated, 1)["offset_y"], 2);
    let raw = doc.layers[1].mask.as_ref().unwrap();
    let expected = vec![
        raw[(0, 1)][0],
        raw[(0, 0)][0],
        raw[(1, 1)][0],
        raw[(1, 0)][0],
        raw[(2, 1)][0],
        raw[(2, 0)][0],
    ];
    assert_eq!(mask_bytes(&rotated, 1), expected);
    assert!(LayerRaster::ptr_eq(
        &doc.layers[1].pixels,
        &rotated.layers[1].pixels
    ));
    for matrix in [
        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        [0.0; 6],
        [f64::NAN; 6],
        [1e9, 0.0, 0.0, 1e9, 0.0, 0.0],
    ] {
        assert!(
            unsafe { dr_doc_transform_mask(&doc, 1, matrix.as_ptr(), FILTER_NEAREST) }.is_null()
        );
    }
}

#[test]
fn link_controls_content_geometry_and_canvas_geometry_moves_both() {
    let _serial = serial();
    let doc = mask_fixture((12, 10), (4, 3), (2, 1))
        .add_mask(1, MaskKind::RevealAll)
        .unwrap();
    let displaced = translate(&doc, 1, 1.0, 2.0);
    let unlinked = configured(&displaced, 1, false, 1.0, 0.0);
    for linked in [true, false] {
        let source = if linked { &*displaced } else { &*unlinked };
        let moved = owned(unsafe { dr_doc_with_layer_offset(source, 1, 6, 5) });
        assert_eq!(
            properties(&moved, 1)["offset_x"],
            if linked { 7 } else { 3 }
        );
        assert_eq!(
            properties(&moved, 1)["offset_y"],
            if linked { 7 } else { 3 }
        );
        let transformed = owned(unsafe {
            dr_doc_transform_layer(
                source,
                1,
                [2.0, 0.0, 0.0, 2.0, 0.0, 0.0].as_ptr(),
                FILTER_NEAREST,
            )
        });
        assert_eq!(
            properties(&transformed, 1)["width"],
            if linked { 8 } else { 4 }
        );
        assert_eq!(
            properties(&transformed, 1)["offset_x"],
            if linked { 6 } else { 3 }
        );
    }
    let cropped = owned(unsafe { dr_doc_crop(&*unlinked, 2, 1, 8, 7) });
    assert_eq!(properties(&cropped, 1)["offset_x"], 1);
    assert_eq!(properties(&cropped, 1)["offset_y"], 2);
    assert!(!properties(&cropped, 1)["linked"].as_bool().unwrap());
    let rotated = owned(unsafe { dr_doc_rotate90(&*unlinked) });
    assert_eq!(properties(&rotated, 1)["offset_x"], 4);
    assert_eq!(properties(&rotated, 1)["offset_y"], 3);
    let resized = owned(unsafe { dr_doc_resize(&*unlinked, 24, 20, FILTER_NEAREST) });
    assert_eq!(properties(&resized, 1)["offset_x"], 6);
    assert_eq!(properties(&resized, 1)["width"], 8);
}

#[test]
fn raw_paint_and_scoped_filter_follow_mask_placement_and_apply_bakes_effective() {
    let _serial = serial();
    let doc = mask_fixture((8, 4), (3, 2), (1, 1))
        .add_mask(1, MaskKind::HideAll)
        .unwrap();
    let moved = translate(&doc, 1, 2.0, 0.0);
    let moved = configured(&moved, 1, false, 0.5, 0.0);
    let mut x = 0;
    let mut y = 0;
    let storage = unsafe { dr_doc_layer_mask_storage_image(&*moved, 1, &mut x, &mut y) };
    assert_eq!((x, y), (3, 1));
    assert_eq!(img_dims(storage), (3, 2));
    unsafe { dr_image_free(storage) };
    let mut overlay = [0u8; 8 * 4 * 4];
    overlay[(8 + 3) * 4..(8 + 3) * 4 + 4].copy_from_slice(&[255; 4]);
    let painted = owned(unsafe { dr_doc_painting_layer_mask(&*moved, 1, overlay.as_ptr(), 8, 4) });
    assert_eq!(mask_bytes(&painted, 1), [255, 0, 0, 0, 0, 0]);
    assert_eq!(properties(&painted, 1), properties(&moved, 1));
    let result = RgbaImage::from_pixel(3, 2, image::Rgba([255; 4]));
    let result = unsafe { dr_image_from_rgba(result.as_ptr(), 3, 2) };
    let mut selection = [0; 32];
    selection[2 * 8 + 5] = 255;
    let filtered =
        owned(unsafe { dr_doc_apply_layer_mask(&*painted, 1, result, selection.as_ptr(), 8, 4) });
    unsafe { dr_image_free(result) };
    assert_eq!(mask_bytes(&filtered, 1), [255, 0, 0, 0, 0, 255]);
    let disabled = filtered.set_mask_enabled(1, false).unwrap();
    let applied = owned(unsafe { dr_doc_removing_layer_mask(&disabled, 1, true) });
    let coverage = effective(&filtered, 1);
    let applied_image = applied.layers[1].pixels.contiguous_image();
    for y in 0..2 {
        for x in 0..3 {
            assert_eq!(
                applied_image[(x, y)][3],
                coverage[(y as usize + 1) * 8 + x as usize + 1]
            );
        }
    }
    assert!(applied.layers[1].mask.is_none());
    assert_eq!(
        *filtered.layers[1].pixels.contiguous_image(),
        *doc.layers[1].pixels.contiguous_image()
    );
}

#[test]
fn native_v10_roundtrip_preserves_independent_storage_and_rejects_bad_properties() {
    let _serial = serial();
    let dir = TempDir::new().unwrap();
    let doc = mask_fixture((10, 8), (3, 2), (2, 1))
        .add_mask(1, MaskKind::RevealAll)
        .unwrap();
    let moved = translate(&doc, 1, -4.0, 3.0);
    let adjusted = configured(&moved, 1, false, 0.375, 2.5);
    let path = cpath(&dir.path().join("independent.rz"));
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(&*adjusted, path.as_ptr(), &mut err) },
        "{}",
        take_err_string(err)
    );
    let back = owned(unsafe { dr_doc_open(path.as_ptr(), &mut err) });
    assert_eq!(properties(&back, 1), properties(&adjusted, 1));
    assert_eq!(mask_bytes(&back, 1), mask_bytes(&adjusted, 1));
    assert_eq!(effective(&back, 1), effective(&adjusted, 1));
    let bytes = std::fs::read(dir.path().join("independent.rz")).unwrap();
    assert_eq!(drdc_word(&bytes, 0), IDENTITY_BYTE_VERSION);
    assert_eq!(drdc_word(&bytes, 1), 10);
    // Mutating the exact serialized density is independent of parser offsets.
    let density = 0.375_f32.to_le_bytes();
    let slot = bytes.windows(4).position(|v| v == density).unwrap();
    let mut corrupt = bytes.clone();
    corrupt[slot..slot + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    std::fs::write(dir.path().join("bad.rz"), corrupt).unwrap();
    assert!(
        DrDocument::open(dir.path().join("bad.rz").to_str().unwrap())
            .unwrap_err_message()
            .contains("mask properties")
    );
}

trait ErrorMessage {
    fn unwrap_err_message(self) -> String;
}
impl ErrorMessage for Result<DrDocument, String> {
    fn unwrap_err_message(self) -> String {
        match self {
            Err(e) => e,
            Ok(_) => panic!("bad native accepted"),
        }
    }
}

#[test]
fn feather_cache_is_counted_evictable_and_safe_for_concurrent_reads() {
    let _serial = serial();
    let doc = mask_fixture((31, 23), (31, 23), (0, 0))
        .add_mask(
            1,
            MaskKind::FromSelection(&selection(31, 23, |x, _| if x > 14 { 255 } else { 0 })),
        )
        .unwrap();
    let doc = Arc::new(*configured(&doc, 1, true, 0.8, 4.0));
    let expected = effective(&doc, 1);
    let report = || {
        let ptrs = [Arc::as_ptr(&doc)];
        resource_report::report(&ptrs, &[], &[], &[], 0, 0).expect("a resource report")
    };
    assert!(
        report()["categories"]["mask_cache_planes"]["unique_bytes"]
            .as_u64()
            .unwrap()
            > 31 * 23
    );
    assert!(
        report()["projection_scratch_estimate_bytes"]
            .as_u64()
            .unwrap()
            > 31 * 23 * 20
    );
    let mut workers = Vec::new();
    for _ in 0..3 {
        let doc = Arc::clone(&doc);
        let expected = expected.clone();
        workers.push(std::thread::spawn(move || {
            for _ in 0..8 {
                assert_eq!(effective(&doc, 1), expected);
            }
        }));
    }
    for _ in 0..8 {
        let ptrs = [Arc::as_ptr(&doc)];
        resource_report::evict(&ptrs).expect("an eviction report");
    }
    for worker in workers {
        worker.join().unwrap();
    }
}

#[test]
fn every_legacy_version_migrates_default_properties_and_mask_location() {
    let _serial = serial();
    let dir = TempDir::new().unwrap();
    let mut png = Vec::new();
    solid(2, 1, BLUE)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    for version in 1u32..=9 {
        let mut bytes = b"RZDC".to_vec();
        for v in [version, 6, 4, 1] {
            bytes.extend(v.to_le_bytes());
        }
        if version >= 4 {
            bytes.extend(120f32.to_le_bytes());
            bytes.extend(30f32.to_le_bytes());
        }
        bytes.extend(3u32.to_le_bytes());
        bytes.extend(b"Old");
        bytes.extend(2i32.to_le_bytes());
        bytes.extend(1i32.to_le_bytes());
        bytes.extend(1f32.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.push(1);
        bytes.extend((png.len() as u32).to_le_bytes());
        bytes.extend(&png);
        if version >= 2 {
            bytes.extend([1, 1]);
            bytes.extend(2u32.to_le_bytes());
            bytes.extend([64, 255]);
            bytes.push(0);
        }
        if version >= 3 {
            bytes.push(0);
        }
        if version >= 4 {
            bytes.push(0);
        }
        if version >= 7 {
            bytes.extend([0u8; 11]);
            bytes.push(1);
        }
        if version >= 5 {
            bytes.extend(0u32.to_le_bytes());
        }
        if version >= 6 {
            bytes.extend(72f32.to_le_bytes());
            bytes.extend(72f32.to_le_bytes());
            bytes.extend([0u8; 4]);
        }
        if version >= 8 {
            bytes.extend([0u8; 20]);
        }
        let path = dir.path().join(format!("v{version}.rz"));
        std::fs::write(&path, bytes).unwrap();
        let path = cpath(&path);
        let mut err = ptr::null_mut();
        let raw = unsafe { dr_doc_open(path.as_ptr(), &mut err) };
        assert!(!raw.is_null(), "v{version}: {}", take_err_string(err));
        let doc = owned(raw);
        if version == 1 {
            assert!(doc.layers[0].mask.is_none());
            continue;
        }
        assert_eq!(
            properties(&doc, 0),
            serde_json::json!({"linked":true,"density":1.0,"feather":0.0,"outside_coverage":0,"offset_x":2,"offset_y":1,"width":2,"height":1})
        );
        let values = effective(&doc, 0);
        assert_eq!(&values[8..10], &[64, 255]);
        assert_eq!(values.iter().filter(|v| **v != 0).count(), 2);
        let new_path = cpath(&dir.path().join(format!("v{version}-saved.rz")));
        assert!(unsafe { dr_doc_save_native(&*doc, new_path.as_ptr(), &mut err) });
        let back = owned(unsafe { dr_doc_open(new_path.as_ptr(), &mut err) });
        assert_eq!(effective(&back, 0), values);
    }
}

#[test]
fn fractional_mask_affine_is_bilinear_and_rerasterizing_content_preserves_mask() {
    let _serial = serial();
    let doc = mask_fixture((6, 6), (1, 1), (2, 2))
        .add_mask(1, MaskKind::RevealAll)
        .unwrap();
    let transformed = owned(unsafe {
        dr_doc_transform_mask(
            &doc,
            1,
            [1.0, 0.0, 0.0, 1.0, 0.5, 0.5].as_ptr(),
            FILTER_BILINEAR,
        )
    });
    assert_eq!(mask_bytes(&transformed, 1), [64, 64, 64, 64]);
    assert_eq!(properties(&transformed, 1)["width"], 2);
    assert!(LayerRaster::ptr_eq(
        &transformed.layers[1].pixels,
        &doc.layers[1].pixels
    ));
    let configured = configured(&transformed, 1, true, 0.75, 1.0);
    let content = solid(4, 3, GREEN);
    let rendered = owned(unsafe {
        dr_doc_set_layer_content_preserving_mask(&*configured, 1, content.as_ptr(), 4, 3, -1, 0)
    });
    assert_eq!(properties(&rendered, 1), properties(&configured, 1));
    assert_eq!(mask_bytes(&rendered, 1), mask_bytes(&configured, 1));
    assert!(Arc::ptr_eq(
        rendered.layers[1].mask.as_ref().unwrap(),
        configured.layers[1].mask.as_ref().unwrap()
    ));
    assert_eq!(*rendered.layers[1].pixels.contiguous_image(), content);
    assert_eq!(rendered.layers[1].offset, (-1, 0));
    assert!(unsafe {
        dr_doc_set_layer_content_preserving_mask(&*rendered, 1, content.as_ptr(), 4, 3, -1, 0)
    }
    .is_null());
}

#[test]
fn independent_mask_capacity_is_checked_before_large_decode_and_duplicate() {
    let _serial = serial();
    let dir = TempDir::new().unwrap();
    let doc = DrDocument::from_pixels(solid(1, 1, BLUE))
        .add_mask(0, MaskKind::RevealAll)
        .unwrap();
    let path = dir.path().join("small.rz");
    doc.save_native(path.to_str().unwrap()).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    // PNG IEND is followed by mask present/enabled, length, then v10 width.
    let png_end = bytes.windows(4).position(|w| w == b"IEND").unwrap() + 8;
    let mask_width = png_end + 2 + 4;
    bytes[mask_width..mask_width + 4].copy_from_slice(&100_000_001u32.to_le_bytes());
    std::fs::write(&path, bytes).unwrap();
    let err = DrDocument::open(path.to_str().unwrap()).unwrap_err_message();
    assert!(err.contains("mask dimensions"), "{err}");
    //50MB actual raw bytes, referenced136times:6.8B serialized mask pixels,
    // the prior active-group6.4B + raster400M budgets. No gigabyte allocation
    // or encoding is needed to test framing independently of live heap usage.
    let raw = Arc::new(image::GrayImage::new(10000, 5000));
    let mut large = DrDocument::from_pixels(solid(1, 1, BLUE));
    large.layers[0].mask = Some(raw);
    large.layers = vec![large.layers[0].clone(); 136];
    assert!(unsafe { dr_doc_duplicating_layer(&large, 0) }.is_null());
    assert!(unsafe {
        dr_doc_transform_mask(
            &large,
            0,
            [2.0, 0.0, 0.0, 1.0, 0.0, 0.0].as_ptr(),
            FILTER_NEAREST,
        )
    }
    .is_null());
    assert!(unsafe {
        dr_doc_transform_mask(
            &large,
            0,
            [10000.0, 0.0, 0.0, 10000.0, 0.0, 0.0].as_ptr(),
            FILTER_NEAREST,
        )
    }
    .is_null());
    let reduced = owned(unsafe {
        dr_doc_transform_mask(
            &large,
            0,
            [0.5, 0.0, 0.0, 0.5, 0.0, 0.0].as_ptr(),
            FILTER_NEAREST,
        )
    });
    assert_eq!(properties(&reduced, 0)["width"], 5000);
}

#[test]
fn group_gating_and_styled_shapes_use_effective_mask_and_invalidate_memos() {
    let _serial = serial();
    let doc = mask_fixture((8, 4), (8, 4), (0, 0));
    let (doc, group, _, _) = doc.group_layers(&[1], "Group").unwrap();
    let doc = doc.add_mask(group, MaskKind::HideAll).unwrap();
    let half = configured(&doc, group, false, 0.5, 0.0);
    let image = half.flattened().unwrap();
    // Blue over red at coverage128, independent of group implementation.
    for pixel in image.pixels() {
        assert_eq!(pixel.0, [127, 0, 128, 255]);
    }
    let moved = half.move_layers(&[group], 2, 0).unwrap();
    assert_eq!(
        properties(&moved, group)["offset_x"],
        0,
        "unlinked group mask stays fixed"
    );
    let raster = mask_fixture((8, 4), (8, 4), (0, 0))
        .add_mask(1, MaskKind::HideAll)
        .unwrap();
    assert!(raster.layer_bounds(1).is_none());
    let reveal = configured(&raster, 1, true, 0.0, 0.0);
    assert!(
        reveal.layer_bounds(1).is_some(),
        "density changes invalidate content-bound memo"
    );
    let style =
        CString::new(r##"{"effects":[{"type":"color_overlay","color":"#00ff00","opacity":1}]}"##)
            .unwrap();
    let mut err = ptr::null_mut();
    let styled = unsafe {
        doctored_core::ffi_style::dr_doc_set_layer_style(&raster, 1, style.as_ptr(), &mut err)
    };
    assert!(!styled.is_null(), "{}", take_err_string(err));
    let styled = owned(styled);
    let hidden = styled.flattened().unwrap();
    let shown = configured(&styled, 1, true, 0.0, 0.0);
    assert_ne!(
        shown.flattened().unwrap(),
        hidden,
        "style shape cache observes density"
    );
    let again = configured(&shown, 1, true, 1.0, 0.0);
    assert_eq!(again.flattened().unwrap(), hidden);
}
