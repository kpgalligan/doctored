//! Selection writeback tested through the FFI against independent numeric
//! references. Fixtures distinguish storage/canvas coordinates, hidden RGB,
//! scalar targets, partial coverage, neighborhood reads and no-op refusals.

use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{DrDocument, LayerKind, LayerRaster};
use doctored_core::ffi::*;
use doctored_core::ffi_channel::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::*;
use doctored_core::ffi_scope::*;
use doctored_core::DrImage;
use image::{GrayImage, Rgba, RgbaImage};

use crate::common;
use common::*;

fn image(src: &RgbaImage) -> *mut DrImage {
    let out = unsafe { dr_image_from_rgba(src.as_ptr(), src.width(), src.height()) };
    assert!(!out.is_null());
    out
}

fn gray(values: &[u8], w: u32, h: u32) -> *mut DrImage {
    image(&RgbaImage::from_fn(w, h, |x, y| {
        let value = values[(y * w + x) as usize];
        Rgba([value, value, value, 255])
    }))
}

fn fixture(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            ((x * 61 + y * 43 + 17) % 256) as u8,
            ((x * 37 + y * 79 + 9) % 256) as u8,
            ((x * 89 + y * 19 + 41) % 256) as u8,
            [0, 37, 111, 255][((x + y) % 4) as usize],
        ])
    })
}

/// Canvas smaller than the layer, retaining a real off-canvas ring.
fn offset_doc(src: &RgbaImage, w: u32, h: u32, offset: (i32, i32)) -> DrDocument {
    let mut doc = DrDocument::from_pixels(src.clone());
    doc.width = w;
    doc.height = h;
    doc.layers[0].offset = offset;
    doc
}

/// Independent f64 coverage interpolation: premultiplied colors are weighted
/// by both selection coverage and each input's alpha, then normalized once.
fn reference(before: [u8; 4], result: [u8; 4], q: u8) -> [u8; 4] {
    if q == 0 || before == result {
        return before;
    }
    if q == 255 {
        return result;
    }
    let wb = f64::from(255 - q) * f64::from(before[3]);
    let wr = f64::from(q) * f64::from(result[3]);
    let alpha = ((wb + wr) / 255.0).round() as u8;
    if alpha == 0 {
        return [0; 4];
    }
    let mut out = [0, 0, 0, alpha];
    for c in 0..3 {
        out[c] =
            ((wb * f64::from(before[c]) + wr * f64::from(result[c])) / (wb + wr)).round() as u8;
    }
    out
}

fn scalar(before: u8, result: u8, q: u8) -> u8 {
    ((f64::from(before) * f64::from(255 - q) + f64::from(result) * f64::from(q)) / 255.0).round()
        as u8
}

fn q_at(selection: &[u8], canvas: (u32, u32), x: u32, y: u32, offset: (i32, i32)) -> u8 {
    let cx = i64::from(x) + i64::from(offset.0);
    let cy = i64::from(y) + i64::from(offset.1);
    if !(0..i64::from(canvas.0)).contains(&cx) || !(0..i64::from(canvas.1)).contains(&cy) {
        return 0;
    }
    selection[(cy * i64::from(canvas.0) + cx) as usize]
}

#[test]
fn rgba_partial_coverage_maps_both_offsets_and_preserves_unselected_hidden_rgb() {
    let src = fixture(7, 5);
    let result = RgbaImage::from_fn(7, 5, |x, y| {
        let p = src.get_pixel(x, y);
        Rgba([
            255 - p[0],
            255 - p[1],
            255 - p[2],
            ((x * 31 + y * 17 + 73) % 256) as u8,
        ])
    });
    let result_handle = image(&result);
    let selection = [0, 1, 63, 128, 201, 255, 0, 255, 42, 157, 0, 255];
    for offset in [(-2, -1), (1, 1), (i32::MIN, i32::MAX)] {
        let doc = offset_doc(&src, 4, 3, offset);
        let out =
            unsafe { dr_doc_apply_layer_pixels(&doc, 0, result_handle, selection.as_ptr(), 4, 3) };
        if offset.0 == i32::MIN {
            assert!(
                out.is_null(),
                "entirely off-canvas selected edit must be a no-op"
            );
            continue;
        }
        assert!(!out.is_null());
        let got = layer_pixels(out, 0);
        for (x, y, before) in src.enumerate_pixels() {
            let q = q_at(&selection, (4, 3), x, y, offset);
            let want = reference(before.0, result.get_pixel(x, y).0, q);
            let at = ((y * src.width() + x) * 4) as usize;
            assert_eq!(
                &got[at..at + 4],
                &want,
                "offset {offset:?}, pixel {x},{y}, q {q}"
            );
        }
        assert_eq!(&*doc.layers[0].pixels.contiguous_image(), &src);
        assert_eq!(layer_offset(out, 0), offset);
        unsafe { dr_doc_free(out) };
    }
    unsafe { dr_image_free(result_handle) };
}

#[test]
fn no_selection_writes_entire_storage_and_identity_or_empty_coverage_refuses() {
    let src = fixture(7, 5);
    let doc = offset_doc(&src, 4, 3, (-2, -1));
    let original = image(&src);
    let inverted = unsafe { dr_image_invert(original) };
    let empty = [0u8; 12];
    unsafe {
        assert!(dr_doc_apply_layer_pixels(&doc, 0, original, ptr::null(), 4, 3).is_null());
        assert!(dr_doc_apply_layer_pixels(&doc, 0, inverted, empty.as_ptr(), 4, 3).is_null());
        let out = dr_doc_apply_layer_pixels(&doc, 0, inverted, ptr::null(), 4, 3);
        assert!(!out.is_null());
        assert_eq!(layer_pixels(out, 0), img_pixels(inverted));
        dr_doc_free(out);
        dr_image_free(inverted);
        dr_image_free(original);
    }
}

#[test]
fn blur_samples_outside_selection_but_only_writes_feathered_coverage() {
    // A white sample outside the selection must influence the selected black
    // center. Blurring a pre-masked source would produce black throughout.
    let src = RgbaImage::from_fn(7, 3, |x, _| {
        let value = if x == 4 { 255 } else { 0 };
        Rgba([value, value, value, 255])
    });
    let doc = DrDocument::from_pixels(src.clone());
    let original = image(&src);
    // Gaussian Blur is `dr_image_filter_op` `blur`.
    let op = std::ffi::CString::new("blur").unwrap();
    let params = std::ffi::CString::new(r#"{"sigma":0.8}"#).unwrap();
    let blurred = unsafe {
        doctored_core::ffi_filters::dr_image_filter_op(
            original,
            op.as_ptr(),
            params.as_ptr(),
            ptr::null(),
            ptr::null_mut(),
        )
    };
    let mut selection = [0u8; 21];
    selection[10] = 128;
    let out = unsafe { dr_doc_apply_layer_pixels(&doc, 0, blurred, selection.as_ptr(), 7, 3) };
    assert!(!out.is_null());
    let got = layer_pixels(out, 0);
    // sigma .8 has a 3-tap Gaussian neighborhood. Rows are constant, so the
    // independent direct 1-D convolution suffices for this fixture.
    let side = (-1.0f64 / (2.0 * 0.8 * 0.8)).exp();
    let filtered = (255.0 * side / (1.0 + 2.0 * side)).round() as u8;
    let expected = scalar(0, filtered, 128);
    assert!(expected > 0);
    for (i, pixel) in got.chunks_exact(4).enumerate() {
        let want = if i == 10 {
            [expected, expected, expected, 255]
        } else {
            src.get_pixel(i as u32 % 7, i as u32 / 7).0
        };
        assert_eq!(pixel, want);
    }
    unsafe {
        dr_doc_free(out);
        dr_image_free(blurred);
        dr_image_free(original);
    }
}

#[test]
fn layer_mask_scoping_preserves_pixels_properties_and_disabled_state() {
    let src = fixture(6, 4);
    let mut doc = offset_doc(&src, 4, 3, (-1, -1));
    let mask = GrayImage::from_fn(6, 4, |x, y| image::Luma([((x * 43 + y * 17) % 256) as u8]));
    doc.layers[0].mask_properties.offset = doc.layers[0].offset;
    doc.layers[0].mask = Some(Arc::new(mask.clone()));
    doc.layers[0].mask_enabled = false;
    doc.layers[0].meta = Some("unchanged source description".to_string());
    let values: Vec<u8> = mask.as_raw().iter().map(|v| 255 - v).collect();
    let result = gray(&values, 6, 4);
    let selection = [0, 64, 255, 128, 0, 255, 31, 200, 0, 255, 128, 1];
    let out = unsafe { dr_doc_apply_layer_mask(&doc, 0, result, selection.as_ptr(), 4, 3) };
    assert!(!out.is_null());
    let after = unsafe { &*out };
    assert_eq!(
        after.layers[0].pixels.contiguous_image().as_raw(),
        src.as_raw()
    );
    assert!(!after.layers[0].mask_enabled);
    assert_eq!(after.layers[0].meta, doc.layers[0].meta);
    for (x, y, before) in mask.enumerate_pixels() {
        let q = q_at(&selection, (4, 3), x, y, (-1, -1));
        assert_eq!(
            after.layers[0].mask.as_ref().unwrap().get_pixel(x, y)[0],
            scalar(before[0], 255 - before[0], q)
        );
    }
    assert_eq!(doc.layers[0].mask.as_ref().unwrap().as_ref(), &mask);
    unsafe {
        dr_doc_free(out);
        dr_image_free(result);
    }
}

#[test]
fn translated_group_mask_source_and_result_keep_raw_offcanvas_storage() {
    let mut doc = DrDocument::from_pixels(fixture(4, 3));
    // A group dummy plus child, as native group ops store them. Its offset
    // places some raw mask samples outside the canvas; those must survive.
    let child = doc.layers[0].clone();
    doc.layers[0].kind = LayerKind::Group;
    doc.layers[0].pixels = LayerRaster::new(RgbaImage::new(1, 1));
    doc.layers[0].offset = (2, -1);
    doc.layers[0].mask_properties.offset = (2, -1);
    doc.layers[0].mask = Some(Arc::new(
        GrayImage::from_raw(4, 3, (0..12).map(|v| v * 19).collect()).unwrap(),
    ));
    doc.layers.push(child);
    doc.layers[1].depth = 1;
    let mut x = 99;
    let mut y = 99;
    let source = unsafe { dr_doc_layer_mask_storage_image(&doc, 0, &mut x, &mut y) };
    assert!(!source.is_null());
    assert_eq!((x, y), (2, -1));
    let source_values: Vec<u8> = img_pixels(source).chunks_exact(4).map(|p| p[0]).collect();
    assert_eq!(
        &source_values,
        doc.layers[0].mask.as_ref().unwrap().as_raw()
    );
    let result = unsafe { dr_image_invert(source) };
    let selected = [255u8; 12];
    let out = unsafe { dr_doc_apply_layer_mask(&doc, 0, result, selected.as_ptr(), 4, 3) };
    assert!(!out.is_null());
    let after = unsafe { &*out };
    assert_eq!(after.layers[0].offset, (2, -1));
    for (i, &before) in source_values.iter().enumerate() {
        let q = q_at(&selected, (4, 3), i as u32 % 4, i as u32 / 4, (2, -1));
        assert_eq!(
            after.layers[0].mask.as_ref().unwrap().as_raw()[i],
            scalar(before, 255 - before, q)
        );
    }
    unsafe {
        dr_doc_free(out);
        dr_image_free(result);
        dr_image_free(source);
    }
}

#[test]
fn color_plane_writes_one_byte_and_alpha_zero_preserves_unselected_hidden_rgb() {
    let src = fixture(6, 4);
    let doc = offset_doc(&src, 4, 3, (-1, 1));
    let values: Vec<u8> = (0..24).map(|i| (i * 11) as u8).collect();
    let result = gray(&values, 6, 4);
    let selection = [0, 255, 64, 128, 1, 200, 0, 255, 0, 255, 117, 63];
    for plane in [0, 1, 2, 3] {
        let out =
            unsafe { dr_doc_apply_layer_plane(&doc, 0, plane, result, selection.as_ptr(), 4, 3) };
        assert!(!out.is_null());
        for (i, actual) in layer_pixels(out, 0).chunks_exact(4).enumerate() {
            let x = i as u32 % 6;
            let y = i as u32 / 6;
            let mut want = src.get_pixel(x, y).0;
            let old = want[plane as usize];
            want[plane as usize] = scalar(old, values[i], q_at(&selection, (4, 3), x, y, (-1, 1)));
            if plane == 3 && want[3] != old && want[3] == 0 {
                want = [0; 4];
            }
            assert_eq!(actual, want, "plane {plane}, pixel {i}");
        }
        unsafe { dr_doc_free(out) };
    }
    for plane in [-1, 4, 5, 999] {
        assert!(
            unsafe { dr_doc_apply_layer_plane(&doc, 0, plane, result, ptr::null(), 4, 3) }
                .is_null()
        );
    }
    // Explicitly drive selected visible alpha to zero beside unselected
    // latent color. The latter must remain bit-for-bit unchanged.
    let alpha_src = RgbaImage::from_raw(2, 1, vec![91, 13, 207, 0, 39, 71, 211, 255]).unwrap();
    let alpha_doc = DrDocument::from_pixels(alpha_src.clone());
    let zero = gray(&[0, 0], 2, 1);
    let out = unsafe { dr_doc_apply_layer_plane(&alpha_doc, 0, 3, zero, [0, 255].as_ptr(), 2, 1) };
    assert_eq!(layer_pixels(out, 0), [91, 13, 207, 0, 0, 0, 0, 0]);
    // Without a selection, only actual alpha changes clear RGB. Existing
    // hidden color whose alpha was already zero remains an exact identity.
    let whole = unsafe { dr_doc_apply_layer_plane(&alpha_doc, 0, 3, zero, ptr::null(), 2, 1) };
    assert_eq!(layer_pixels(whole, 0), [91, 13, 207, 0, 0, 0, 0, 0]);
    unsafe {
        dr_doc_free(whole);
        dr_doc_free(out);
        dr_image_free(zero);
        dr_image_free(result);
    }
}

#[test]
fn saved_channel_invert_uses_feather_coverage_and_preserves_identity() {
    let pixels = image(&fixture(4, 3));
    let base = unsafe { dr_doc_from_image(pixels) };
    let before: Vec<u8> = (0..12).map(|v| v * 19).collect();
    let doc = apply(base, |d| unsafe {
        dr_doc_add_channel(
            d,
            c"Saved selection".as_ptr(),
            before.as_ptr(),
            4,
            3,
            20,
            40,
            60,
            0.7,
        )
    });
    let identity = unsafe { dr_doc_channel_id(doc, 0) };
    let source = gray(&before, 4, 3);
    let result = unsafe { dr_image_invert(source) };
    let selection = [0, 1, 64, 128, 200, 255, 17, 63, 201, 255, 0, 255];
    let out = unsafe { dr_doc_apply_channel(doc, 0, result, selection.as_ptr(), 4, 3) };
    assert!(!out.is_null());
    let mut actual = [0; 12];
    assert!(unsafe { dr_doc_channel_plane(out, 0, actual.as_mut_ptr(), 4, 3) });
    let want: Vec<u8> = before
        .iter()
        .zip(selection)
        .map(|(&v, q)| scalar(v, 255 - v, q))
        .collect();
    assert_eq!(actual.as_slice(), want);
    assert_eq!(unsafe { dr_doc_channel_id(out, 0) }, identity);
    assert_eq!(layer_pixels(out, 0), layer_pixels(doc, 0));
    unsafe {
        assert!(dr_doc_apply_channel(doc, 0, source, ptr::null(), 4, 3).is_null());
        assert!(dr_doc_apply_channel(doc, 0, result, [0; 12].as_ptr(), 4, 3).is_null());
        dr_doc_free(out);
        dr_doc_free(doc);
        dr_image_free(source);
        dr_image_free(result);
        dr_image_free(pixels);
    }
}

#[test]
fn scoped_writers_enforce_locks_and_do_not_create_phantom_alpha_edits() {
    let mut doc = DrDocument::from_pixels(fixture(4, 3));
    doc.layers[0].mask = Some(Arc::new(GrayImage::from_pixel(4, 3, image::Luma([100]))));
    let result = gray(&[0; 12], 4, 3);
    let locked = unsafe { dr_doc_with_layer_locks(&doc, 0, LOCK_PIXELS) };
    unsafe {
        assert!(dr_doc_apply_layer_pixels(locked, 0, result, ptr::null(), 4, 3).is_null());
        assert!(dr_doc_apply_layer_plane(locked, 0, 0, result, ptr::null(), 4, 3).is_null());
        let mask_edit = dr_doc_apply_layer_mask(locked, 0, result, ptr::null(), 4, 3);
        assert!(!mask_edit.is_null(), "pixel lock permits mask editing");
        dr_doc_free(mask_edit);
        dr_doc_free(locked);
        let all = dr_doc_with_layer_locks(&doc, 0, LOCK_ALL);
        assert!(dr_doc_apply_layer_mask(all, 0, result, ptr::null(), 4, 3).is_null());
        dr_doc_free(all);
        let alpha = dr_doc_with_layer_locks(&doc, 0, LOCK_TRANSPARENCY);
        assert!(
            dr_doc_apply_layer_plane(alpha, 0, 3, result, ptr::null(), 4, 3).is_null(),
            "alpha-only removal is unchanged after lock restoration"
        );
        let recolored = dr_doc_apply_layer_pixels(alpha, 0, result, ptr::null(), 4, 3);
        assert!(!recolored.is_null());
        for (before, after) in doc.layers[0]
            .pixels
            .contiguous_image()
            .pixels()
            .zip(layer_pixels(recolored, 0).chunks_exact(4))
        {
            assert_eq!(before[3], after[3]);
            if before[3] == 0 {
                assert_eq!(before.0, after);
            } else {
                assert_eq!(&after[..3], [0, 0, 0]);
            }
        }
        dr_doc_free(recolored);
        dr_doc_free(alpha);
        dr_image_free(result);
    }
}

#[test]
fn null_invalid_dimensions_and_missing_targets_refuse_without_reading_selection() {
    let doc = DrDocument::from_pixels(fixture(4, 3));
    let result = image(&fixture(4, 3));
    let wrong = image(&fixture(3, 4));
    type Writer = unsafe extern "C" fn(
        *const DrDocument,
        usize,
        *const DrImage,
        *const u8,
        u32,
        u32,
    ) -> *mut DrDocument;
    let writers: [Writer; 3] = [
        dr_doc_apply_layer_pixels,
        dr_doc_apply_layer_mask,
        dr_doc_apply_channel,
    ];
    unsafe {
        for writer in writers {
            assert!(writer(ptr::null(), 0, result, ptr::null(), 4, 3).is_null());
            assert!(writer(&doc, 0, ptr::null(), ptr::null(), 4, 3).is_null());
            assert!(writer(&doc, usize::MAX, result, ptr::null(), 4, 3).is_null());
            assert!(writer(&doc, 0, wrong, ptr::null(), 4, 3).is_null());
            // Short storage is valid for declared 1x1, but dimensions refuse
            // before any canvas-sized read can overrun that allocation.
            assert!(writer(&doc, 0, result, [255].as_ptr(), 1, 1).is_null());
        }
        assert!(dr_doc_apply_layer_mask(&doc, 0, result, ptr::null(), 4, 3).is_null());
        assert!(dr_doc_apply_channel(&doc, 0, result, ptr::null(), 4, 3).is_null());
        assert!(dr_doc_apply_layer_plane(ptr::null(), 0, 0, result, ptr::null(), 4, 3).is_null());
        assert!(dr_doc_apply_layer_plane(&doc, 0, 0, ptr::null(), ptr::null(), 4, 3).is_null());
        assert!(dr_doc_apply_layer_plane(&doc, usize::MAX, 0, result, ptr::null(), 4, 3).is_null());
        assert!(dr_doc_apply_layer_plane(&doc, 0, 0, wrong, ptr::null(), 4, 3).is_null());
        assert!(dr_doc_apply_layer_plane(&doc, 0, 0, result, [255].as_ptr(), 1, 1).is_null());
        let mut offset = 57;
        assert!(
            dr_doc_layer_mask_storage_image(ptr::null(), 0, &mut offset, ptr::null_mut()).is_null()
        );
        assert!(dr_doc_layer_mask_storage_image(&doc, 0, &mut offset, ptr::null_mut()).is_null());
        assert!(dr_doc_layer_mask_storage_image(
            &doc,
            usize::MAX,
            ptr::null_mut(),
            ptr::null_mut()
        )
        .is_null());
        assert_eq!(offset, 57, "failed getter wrote outputs");
        dr_image_free(result);
        dr_image_free(wrong);
    }
}
