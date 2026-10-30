//! The agent's pixel-art surface through the FFI: `dr_doc_put_pixels`'
//! replace semantics against an independent byte oracle (alpha 0 and 128
//! written verbatim, masked samples untouched), the layer-offset mapping and
//! the samples it ignores, every refusal (rect off the canvas, zero size,
//! NULL inputs, a group or out-of-range index, a pixel lock, a no-change
//! write), the transparency lock's alpha restore and input purity — plus the
//! contract `render`'s integer `scale` relies on: `dr_image_resize` with
//! DR_FILTER_NEAREST at an integer factor is exact block replication, latent
//! RGB under alpha 0 included.

use std::ffi::CString;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::*;
use doctored_core::ffi_pixels::dr_doc_put_pixels;
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common;
use common::*;

/// The op through its FFI entry point: `Some(new doc)` or `None` on NULL.
fn put(
    doc: *const DrDocument,
    idx: usize,
    rect: (u32, u32, u32, u32),
    rgba: &[u8],
    mask: Option<&[u8]>,
) -> Option<*mut DrDocument> {
    let (x, y, w, h) = rect;
    let mask = mask.map_or(ptr::null(), |m| m.as_ptr());
    let out = unsafe { dr_doc_put_pixels(doc, idx, x, y, w, h, rgba.as_ptr(), mask) };
    (!out.is_null()).then_some(out)
}

/// A `w × h` buffer whose every sample is distinct, with alpha cycling
/// through 255, 128 and 0 (the RGB under alpha 0 is nonzero on purpose).
fn source(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..w * h {
        let alpha = [255u8, 128, 0][(i % 3) as usize];
        out.extend_from_slice(&[(i * 37 % 251) as u8, (i * 11 % 241) as u8, 200, alpha]);
    }
    out
}

fn with_locks(doc: *mut DrDocument, idx: usize, locks: u32) -> *mut DrDocument {
    apply(doc, |d| unsafe { dr_doc_with_layer_locks(d, idx, locks) })
}

#[test]
fn written_samples_replace_all_four_bytes_and_masked_ones_keep_theirs() {
    let dir = TempDir::new().unwrap();
    let (cw, ch) = (8u32, 6u32);
    let doc = doc_from(&dir, "bg.png", &opaque_pattern(cw, ch));
    let before = layer_pixels(doc, 0);
    let rect = (1u32, 2u32, 5u32, 3u32);
    let rgba = source(rect.2, rect.3);
    // Every fourth sample unmasked.
    let mask: Vec<u8> = (0..rect.2 * rect.3)
        .map(|i| if i % 4 == 3 { 0 } else { 9 })
        .collect();

    // Independent oracle: the original bytes, with each masked-in sample of
    // the rect replaced verbatim.
    let mut expected = before.clone();
    for sy in 0..rect.3 {
        for sx in 0..rect.2 {
            let i = (sy * rect.2 + sx) as usize;
            if mask[i] == 0 {
                continue;
            }
            let at = (((rect.1 + sy) * cw + rect.0 + sx) * 4) as usize;
            expected[at..at + 4].copy_from_slice(&rgba[i * 4..i * 4 + 4]);
        }
    }
    let out = put(doc, 0, rect, &rgba, Some(&mask)).expect("write");
    let after = layer_pixels(out, 0);
    assert_eq!(after, expected);
    // The alpha 128 and alpha 0 samples landed exactly, latent RGB included.
    assert_eq!(pixel(&after, cw, 2, 2), [37, 11, 200, 128]);
    assert_eq!(pixel(&after, cw, 3, 2), [74, 22, 200, 0]);
    assert_eq!(
        layer_pixels(doc, 0),
        before,
        "the input document is untouched"
    );

    // With no mask every sample of the rect is written.
    let whole = put(doc, 0, rect, &rgba, None).expect("unmasked write");
    let after = layer_pixels(whole, 0);
    for sy in 0..rect.3 {
        for sx in 0..rect.2 {
            let i = (sy * rect.2 + sx) as usize;
            let got = pixel(&after, cw, rect.0 + sx, rect.1 + sy);
            assert_eq!(got.as_slice(), &rgba[i * 4..i * 4 + 4]);
        }
    }
    unsafe {
        dr_doc_free(whole);
        dr_doc_free(out);
        dr_doc_free(doc);
    }
}

#[test]
fn the_rect_maps_through_the_layer_offset_and_samples_off_the_layer_are_ignored() {
    let dir = TempDir::new().unwrap();
    let (cw, ch) = (8u32, 6u32);
    let doc = doc_from(&dir, "bg.png", &solid(cw, ch, [1, 2, 3, 255]));
    let doc = add_layer(
        &dir,
        "top.png",
        doc,
        0,
        &solid(cw, ch, [9, 9, 9, 255]),
        "Top",
    );
    // A 4 × 4 layer at (5, 3): only its top-left 3 × 3 lies on the canvas.
    let content = [50u8; 4 * 4 * 4];
    let doc = apply(doc, |d| unsafe {
        dr_doc_set_layer_content(d, 1, content.as_ptr(), 4, 4, 5, 3, ptr::null())
    });
    let rgba = source(cw, ch);
    let out = put(doc, 1, (0, 0, cw, ch), &rgba, None).expect("write");
    assert_eq!(layer_dims(out, 1), (4, 4), "the layer never grows");
    assert_eq!(layer_offset(out, 1), (5, 3));
    let after = layer_pixels(out, 1);
    for ly in 0..4u32 {
        for lx in 0..4u32 {
            let got = pixel(&after, 4, lx, ly);
            let (cx, cy) = (lx + 5, ly + 3);
            if cx < cw && cy < ch {
                let i = ((cy * cw + cx) * 4) as usize;
                assert_eq!(got.as_slice(), &rgba[i..i + 4], "({lx}, {ly})");
            } else {
                assert_eq!(got, [50, 50, 50, 50], "off-canvas ({lx}, {ly}) untouched");
            }
        }
    }
    assert_eq!(
        layer_pixels(out, 0),
        layer_pixels(doc, 0),
        "other layers untouched"
    );
    // A rect that misses the layer entirely refuses.
    assert!(put(doc, 1, (0, 0, 5, 3), &source(5, 3), None).is_none());
    unsafe {
        dr_doc_free(out);
        dr_doc_free(doc);
    }
}

#[test]
fn malformed_requests_refuse() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "bg.png", &opaque_pattern(8, 6));
    let one = [1u8, 2, 3, 4];
    unsafe {
        assert!(put(doc, 0, (8, 0, 1, 1), &one, None).is_none(), "x off");
        assert!(put(doc, 0, (0, 6, 1, 1), &one, None).is_none(), "y off");
        assert!(
            put(doc, 0, (7, 0, 2, 1), &source(2, 1), None).is_none(),
            "straddles"
        );
        assert!(put(doc, 0, (u32::MAX, 0, 2, 1), &source(2, 1), None).is_none());
        assert!(
            put(doc, 0, (0, 0, 0, 1), &one, None).is_none(),
            "zero width"
        );
        assert!(
            put(doc, 0, (0, 0, 1, 0), &one, None).is_none(),
            "zero height"
        );
        assert!(dr_doc_put_pixels(doc, 0, 0, 0, 1, 1, ptr::null(), ptr::null()).is_null());
        assert!(dr_doc_put_pixels(ptr::null(), 0, 0, 0, 1, 1, one.as_ptr(), ptr::null()).is_null());
        assert!(
            put(doc, 1, (0, 0, 1, 1), &one, None).is_none(),
            "out of range"
        );
        dr_doc_free(doc);
    }
}

#[test]
fn a_write_that_changes_no_byte_refuses() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "bg.png", &opaque_pattern(8, 6));
    let current = layer_pixels(doc, 0);
    assert!(put(doc, 0, (0, 0, 8, 6), &current, None).is_none());
    // A mask that writes nothing changes nothing either.
    let zero = vec![0u8; 8 * 6];
    assert!(put(doc, 0, (0, 0, 8, 6), &source(8, 6), Some(&zero)).is_none());
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_pixel_lock_refuses_and_a_transparency_lock_keeps_alpha() {
    let dir = TempDir::new().unwrap();
    for lock in [LOCK_PIXELS, LOCK_ALL] {
        let doc = doc_from(&dir, "bg.png", &opaque_pattern(4, 4));
        let doc = with_locks(doc, 0, lock);
        assert!(
            put(doc, 0, (0, 0, 4, 4), &source(4, 4), None).is_none(),
            "{lock}"
        );
        unsafe { dr_doc_free(doc) };
    }

    let doc = doc_from(&dir, "bg.png", &solid(4, 4, [10, 20, 30, 255]));
    let doc = with_locks(doc, 0, LOCK_TRANSPARENCY);
    let out = put(doc, 0, (1, 1, 1, 1), &[200, 100, 50, 128], None).expect("RGB write");
    assert_eq!(
        pixel(&layer_pixels(out, 0), 4, 1, 1),
        [200, 100, 50, 255],
        "the transparency lock restores alpha and keeps the new RGB"
    );
    // An alpha-only write is nothing once alpha is restored.
    assert!(put(doc, 0, (1, 1, 1, 1), &[10, 20, 30, 0], None).is_none());
    unsafe {
        dr_doc_free(out);
        dr_doc_free(doc);
    }
}

#[test]
fn a_group_index_refuses() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "bg.png", &solid(4, 4, [10, 20, 30, 255]));
    let doc = add_layer(
        &dir,
        "top.png",
        doc,
        0,
        &solid(4, 4, [0, 0, 255, 255]),
        "Top",
    );
    let cname = CString::new("G").unwrap();
    let set = [1usize];
    let mut g = usize::MAX;
    let grouped = unsafe {
        dr_doc_group_layers(
            doc,
            set.as_ptr(),
            1,
            cname.as_ptr(),
            &mut g,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        )
    };
    assert!(!grouped.is_null());
    assert!(put(grouped, g, (0, 0, 1, 1), &[1, 2, 3, 4], None).is_none());
    unsafe {
        dr_doc_free(grouped);
        dr_doc_free(doc);
    }
}

/// `render`'s integer `scale` is `dr_image_resize` with DR_FILTER_NEAREST;
/// at an integer factor that must be exact block replication of every byte,
/// the RGB under alpha 0 included.
#[test]
fn nearest_resize_at_an_integer_factor_is_block_replication() {
    let (w, h) = (5u32, 3u32);
    let src = source(w, h);
    let img = unsafe { dr_image_from_rgba(src.as_ptr(), w, h) };
    assert!(!img.is_null());
    for k in [2u32, 3, 8] {
        let out = unsafe { dr_image_resize(img, w * k, h * k, FILTER_NEAREST) };
        assert!(!out.is_null(), "×{k}");
        assert_eq!(img_dims(out), (w * k, h * k));
        let got = img_pixels(out);
        let expected = RgbaImage::from_fn(w * k, h * k, |x, y| {
            let i = (((y / k) * w + x / k) * 4) as usize;
            Rgba([src[i], src[i + 1], src[i + 2], src[i + 3]])
        });
        assert_eq!(got, expected.into_raw(), "×{k}");
        unsafe { dr_image_free(out) };
    }
    unsafe { dr_image_free(img) };
}
