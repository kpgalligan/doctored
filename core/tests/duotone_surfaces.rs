//! #1 Duotone (`duotone_document`, `duotone_convert`, `gray_profile`'s
//! Duotone arm, the clipboard's Duotone payload, through the C API): Assign
//! and Convert to Profile and 8 → 16 refused with 16 → 8 keeping the inks,
//! the render's one table and its second stage admitted beside the source,
//! the thumbnail through the inks and New from Clipboard of a Duotone copy.
//!
//! Oracles, each independent of the code under test: the plain Gray source
//! for the profile refusals; the view of each layer's own pixels
//! (`dr_precision_doc_duotone_view`) for the render and the thumbnail; the
//! source Grayscale document for the clipboard. Tolerances, fixed before
//! measuring: the thumbnail one 8-bit code a channel, everything else exact.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width};
use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_bytes, dr_clipboard_payload_decode, dr_clipboard_payload_free,
    dr_precision_doc_clipboard_copy, dr_precision_doc_from_clipboard, DrClipboardPayload,
};
use doctored_core::ffi_duotone::*;
use doctored_core::ffi_gray::dr_precision_doc_convert_to_gray;
use doctored_core::ffi_output_color::dr_precision_doc_convert_to_profile_with;
use doctored_core::ffi_precision::dr_precision_image_free;
use doctored_core::ffi_precision_inspect::dr_precision_doc_profile;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::DrImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::duotone_document::{
    descriptor, duotone, free, gray_doc, ink, options, profile_edit, LIMIT,
};

const CODES16: [u16; 6] = [0, 9000, 21000, 33000, 50000, 65535];
const CODES8: [u16; 6] = [0, 30, 90, 128, 200, 255];
const DEPTH32_REFUSAL: &str =
    "32 Bits/Channel is unavailable in a Duotone document: the 32-bit modes are RGB and Grayscale.";

fn inks2() -> Value {
    json!({"inks": [ink(0), ink(1)]})
}

/// A profile edit that must change the document, its plan admitting it.
fn edit(doc: *const PrecisionDocument, request: &Value) -> *mut PrecisionDocument {
    let (plan, out) = profile_edit(doc, request);
    assert_eq!(plan["refusal"], Value::Null, "{request}");
    assert_eq!(plan["changes"], true, "{request}");
    out.unwrap().expect("the edit changes the document")
}

fn boxed(doc: PrecisionDocument) -> *mut PrecisionDocument {
    Box::into_raw(Box::new(doc))
}

/// A `w × h` neutral sRGB raster whose gray code is `code(x, y)`, opaque but
/// for every seventh pixel.
fn neutral(
    name: &str,
    (w, h): (u32, u32),
    offset: (i32, i32),
    code: &dyn Fn(u32, u32) -> u16,
) -> PrecisionDocumentLayer {
    let pixels: Vec<[u16; 4]> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| {
            let v = code(x, y);
            let a = if (x + y * w) % 7 == 3 { 30000 } else { 65535 };
            [v, v, v, a]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocumentLayer::raster(name, image, offset, 1.0, true).unwrap()
}

/// `layers` on a `w × h` sRGB canvas through Mode ▸ Grayscale into 16-bit
/// sGray.
fn gray_of(w: u32, h: u32, layers: Vec<PrecisionDocumentLayer>) -> *mut PrecisionDocument {
    let rgb = boxed(
        PrecisionDocument::from_parts(
            w,
            h,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            layers,
        )
        .unwrap(),
    );
    let request = CString::new(r#"{"space":"sgray","depth":16}"#).unwrap();
    let mut err = ptr::null_mut();
    let gray = unsafe {
        dr_precision_doc_convert_to_gray(
            rgb,
            ptr::null(),
            0,
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!gray.is_null(), "{}", take_err_string(err));
    free(rgb);
    gray
}

/// A three-layer 16-bit sGray document of `w × h`.
fn three_layers(w: u32, h: u32) -> *mut PrecisionDocument {
    gray_of(
        w,
        h,
        vec![
            neutral("Base", (w, h), (0, 0), &|x, y| {
                ((x * 977 + y * 4099) % 65536) as u16
            }),
            neutral("Middle", (w / 2, h / 2), (1, 1), &|x, y| {
                ((x * 15013 + y * 311 + 7000) % 65536) as u16
            }),
            neutral("Top", (w / 3, h), (w as i32 / 2, 0), &|x, y| {
                ((x * 211 + y * 23011 + 40000) % 65536) as u16
            }),
        ],
    )
}

fn result_of(
    out: *mut PrecisionDocument,
    err: *mut c_char,
) -> Result<*mut PrecisionDocument, String> {
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

/// The view of `image` (a source of `doc`) as u16 codes.
fn view_of(doc: *const PrecisionDocument, image: &PrecisionImage) -> Vec<[u16; 4]> {
    let mut err = ptr::null_mut();
    let view =
        unsafe { dr_precision_doc_duotone_view(doc, image, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!view.is_null(), "{}", take_err_string(err));
    let pixels = unsafe { &*view }.to_rgba16().unwrap();
    unsafe { dr_precision_image_free(view) };
    pixels
}

fn layer16(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(index)
        .expect("a raster")
        .to_rgba16()
        .unwrap()
}

fn profile(doc: *const PrecisionDocument) -> Vec<u8> {
    let mut length = 0;
    let bytes = unsafe { dr_precision_doc_profile(doc, &mut length) };
    unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec()
}

fn image_bytes(image: *mut DrImage) -> ((u32, u32), Vec<u8>) {
    let (w, h) = unsafe { (dr_image_width(image), dr_image_height(image)) };
    let bytes = unsafe {
        std::slice::from_raw_parts(dr_image_pixels_rgba(image), w as usize * h as usize * 4)
    }
    .to_vec();
    unsafe { dr_image_free(image) };
    ((w, h), bytes)
}

// ---- Assign and Convert to Profile ----------------------------------------------

#[test]
fn a_duotone_document_refuses_assign_convert_and_more_bits_and_keeps_inks_to_8() {
    for (codes, depth) in [(CODES16, 16u8), (CODES8, 8u8)] {
        let gray = gray_doc(&codes, depth, None);
        let doc = duotone(gray, &inks2());
        let mut refused = vec![
            (
                json!({"op": "assign", "space": "gray_gamma_22"}),
                "Assign Profile is not available in Duotone.",
            ),
            (
                json!({"op": "convert", "space": "gray_gamma_22"}),
                "Convert to Profile is not available in Duotone.",
            ),
            (json!({"op": "depth", "depth": 32}), DEPTH32_REFUSAL),
        ];
        if depth == 8 {
            refused.push((
                json!({"op": "depth", "depth": 16}),
                "16 Bits/Channel is not available in Duotone.",
            ));
        }
        for (request, sentence) in refused {
            let (plan, out) = profile_edit(doc, &request);
            assert_eq!(plan["refusal"], sentence, "{depth} bits: {request}");
            assert_eq!(out, Err(sentence.to_string()), "{depth} bits: {request}");
            // The same request on the plain Gray source still runs.
            let (plan, _) = profile_edit(gray, &request);
            if request["depth"] != 32 {
                assert_eq!(plan["refusal"], Value::Null, "Gray: {request}");
            }
        }
        if depth == 16 {
            // 16 → 8 keeps the inks.
            let eight = edit(doc, &json!({"op": "depth", "depth": 8}));
            assert_eq!(options(eight), options(doc));
            assert_eq!(descriptor(eight)["gray"]["depth"], 8);
            free(eight);
        }
        free(doc);
        free(gray);
    }
}

// ---- the render ------------------------------------------------------------------

fn render_at(doc: *const PrecisionDocument, limit: u64) -> Result<*mut PrecisionDocument, String> {
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_duotone_render(doc, false, limit, 0, ptr::null(), &mut err) };
    result_of(out, err)
}

fn to_rgb_at(doc: *const PrecisionDocument, limit: u64) -> Result<*mut PrecisionDocument, String> {
    let srgb = IccProfile::srgb();
    let bytes = srgb.bytes();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_duotone_to_rgb(
            doc,
            bytes.as_ptr(),
            bytes.len(),
            ptr::null(),
            limit,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result_of(out, err)
}

/// Whether Convert to Profile (sRGB, default options, other 0) of `doc`
/// succeeds at `limit`: a document, or NULL with no error (no change).
fn converts_at(doc: *const PrecisionDocument, limit: u64) -> bool {
    let srgb = IccProfile::srgb();
    let bytes = srgb.bytes();
    let mut report = ptr::null_mut();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_profile_with(
            doc,
            bytes.as_ptr(),
            bytes.len(),
            ptr::null(),
            limit,
            0,
            ptr::null(),
            &mut report,
            &mut err,
        )
    };
    if !report.is_null() {
        unsafe { doctored_core::ffi::dr_string_free(report) };
    }
    if !out.is_null() {
        free(out);
        return true;
    }
    if err.is_null() {
        return true;
    }
    let reason = take_err_string(err);
    assert!(reason.contains("working limit is"), "{reason}");
    false
}

#[test]
fn the_render_maps_every_leaf_through_the_view_of_its_own_pixels() {
    let gray = three_layers(12, 9);
    let doc = duotone(gray, &inks2());
    let out = render_at(doc, LIMIT).unwrap();
    let source = unsafe { &*doc };
    assert_eq!(source.layer_count(), 3);
    assert_eq!(unsafe { &*out }.layer_count(), 3);
    for index in 0..source.layer_count() {
        let image = source.layer_source(index).unwrap();
        assert_eq!(layer16(out, index), view_of(doc, image), "leaf {index}");
    }
    free(out);
    free(doc);
    free(gray);
}

#[test]
fn duotone_to_rgb_admits_its_conversion_beside_the_source() {
    // A gamma 2.2 Gray document, so the second stage is a real conversion
    // out of its twin into sRGB.
    let gray = three_layers(96, 64);
    let gamma = edit(gray, &json!({"op": "assign", "space": "gray_gamma_22"}));
    let doc = duotone(gamma, &inks2());
    let s = unsafe { &*doc }.retained_bytes();
    let rendered = render_at(doc, LIMIT).unwrap();
    // C: the least limit at which the second stage alone succeeds.
    let (mut low, mut high) = (1u64, LIMIT);
    assert!(converts_at(rendered, high));
    assert!(!converts_at(rendered, low));
    while high - low > 1 {
        let mid = low + (high - low) / 2;
        if converts_at(rendered, mid) {
            high = mid;
        } else {
            low = mid;
        }
    }
    let c = high;
    let l = c + s - 1;
    let admitted = render_at(doc, l).expect("the render fits at L");
    free(admitted);
    let refusal = to_rgb_at(doc, l).unwrap_err();
    assert!(
        refusal.contains("working limit is"),
        "the second stage is admitted beside the source: {refusal}"
    );
    let converted = to_rgb_at(doc, c + s).expect("C + S admits both stages");
    free(converted);
    free(rendered);
    free(doc);
    free(gamma);
    free(gray);
}

// ---- the thumbnail ---------------------------------------------------------------

fn thumbnail(
    doc: *const PrecisionDocument,
    image: *const PrecisionImage,
    max_side: u32,
) -> Result<((u32, u32), Vec<u8>), String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_duotone_thumbnail_rgba8(
            doc,
            image,
            max_side,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    Ok(image_bytes(out))
}

fn narrow(v: u16) -> i32 {
    (f64::from(v) / 257.0).round() as i32
}

fn assert_byte_near(actual: &[u8], expected: [u16; 4], what: &str) {
    for c in 0..4 {
        let d = (i32::from(actual[c]) - narrow(expected[c])).abs();
        assert!(d <= 1, "{what}: {actual:?} against {expected:?}");
    }
}

#[test]
fn a_thumbnail_that_needs_no_fit_is_the_view_narrowed() {
    let gray = gray_doc(&CODES16, 16, Some(&[65535, 0, 30000, 65535, 12000, 65535]));
    let doc = duotone(gray, &inks2());
    let image = unsafe { &*doc }.render(LIMIT, 0, None).unwrap();
    let view = view_of(doc, &image);
    let ((w, h), bytes) = thumbnail(doc, &image, 64).unwrap();
    assert_eq!((w, h), image.dimensions(), "never upscaled");
    for (i, (pixel, expected)) in bytes.chunks_exact(4).zip(view).enumerate() {
        assert_byte_near(pixel, expected, &format!("pixel {i}"));
    }
    free(doc);
    free(gray);
}

#[test]
fn a_fitted_thumbnail_of_flat_blocks_is_each_blocks_view() {
    // An 8 × 8 grid of flat 64 × 64 blocks fitted to 32: each block is 4 × 4
    // thumbnail pixels, and the inner 2 × 2 of each sees only its block.
    const SIDE: u32 = 512;
    const BLOCK: u32 = 64;
    let code = |x: u32, y: u32| ((x / BLOCK * 8 + y / BLOCK) * 1031 % 65536) as u16;
    let image = PrecisionImage::from_rgba16(
        SIDE,
        SIDE,
        (0..SIDE * SIDE)
            .map(|i| {
                let v = code(i % SIDE, i / SIDE);
                [v, v, v, 65535]
            })
            .collect(),
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let layer = PrecisionDocumentLayer::raster("Blocks", image, (0, 0), 1.0, true).unwrap();
    let gray = gray_of(SIDE, SIDE, vec![layer]);
    let doc = duotone(gray, &inks2());
    let composite = unsafe { &*doc }.render(LIMIT, 0, None).unwrap();
    let view = view_of(doc, &composite);
    let ((w, h), bytes) = thumbnail(doc, &composite, 32).unwrap();
    assert_eq!((w, h), (32, 32));
    let per = SIDE / w;
    for ty in 0..h {
        for tx in 0..w {
            let inner = |t: u32| matches!(t % (BLOCK / per), 1 | 2);
            if !(inner(tx) && inner(ty)) {
                continue;
            }
            let (sx, sy) = (tx * per, ty * per);
            let expected = view[(sy * SIDE + sx) as usize];
            let at = ((ty * w + tx) * 4) as usize;
            assert_byte_near(&bytes[at..at + 4], expected, &format!("({tx}, {ty})"));
        }
    }
    free(doc);
    free(gray);
}

#[test]
fn the_thumbnail_refuses_a_document_that_is_not_duotone_and_tolerates_null() {
    let gray = gray_doc(&CODES16, 16, None);
    let image = unsafe { &*gray }.render(LIMIT, 0, None).unwrap();
    assert_eq!(
        thumbnail(gray, &image, 64).unwrap_err(),
        "the Duotone view takes a Duotone document"
    );
    let doc = duotone(gray, &inks2());
    // The working limit, beside the image's own bytes.
    let mut err = ptr::null_mut();
    let refused = unsafe {
        dr_precision_doc_duotone_thumbnail_rgba8(
            doc,
            &image,
            64,
            image.retained_bytes() + 1,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(refused.is_null());
    assert!(take_err_string(err).contains("working limit is"));
    assert!(thumbnail(doc, ptr::null(), 64).is_err());
    assert!(thumbnail(ptr::null(), &image, 64).is_err());
    assert!(thumbnail(ptr::null(), ptr::null(), 64).is_err());
    let none = unsafe {
        dr_precision_doc_duotone_thumbnail_rgba8(
            ptr::null(),
            ptr::null(),
            64,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    };
    assert!(none.is_null());
    free(doc);
    free(gray);
}

// ---- New from Clipboard ------------------------------------------------------------

fn copy_layer(doc: *const PrecisionDocument) -> *mut DrClipboardPayload {
    let (w, h) = unsafe { &*doc }.dimensions();
    let request = CString::new(format!(
        r#"{{"op":"copy","layer":0,"x":0,"y":0,"width":{w},"height":{h}}}"#
    ))
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_clipboard_copy(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    out
}

fn from_clipboard(payload: *const DrClipboardPayload) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_from_clipboard(payload, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    out
}

#[test]
fn new_from_clipboard_of_a_duotone_copy_is_a_grayscale_document() {
    for (codes, depth) in [(CODES16, 16u8), (CODES8, 8u8)] {
        let gray = gray_doc(&codes, depth, None);
        let doc = duotone(gray, &inks2());
        let payload = copy_layer(doc);
        let mut length = 0usize;
        let pointer = unsafe { dr_clipboard_payload_bytes(payload, &mut length) };
        let bytes = unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec();
        let mut err = ptr::null_mut();
        let decoded =
            unsafe { dr_clipboard_payload_decode(bytes.as_ptr(), bytes.len(), LIMIT, 0, &mut err) };
        assert!(!decoded.is_null(), "{}", take_err_string(err));
        for (what, handle) in [
            ("the handle", payload),
            ("after encode and decode", decoded),
        ] {
            let made = from_clipboard(handle);
            let shown = descriptor(made);
            let what = format!("{what} at {depth} bits");
            assert_eq!(shown["color_mode"], "gray", "{what}: {shown}");
            assert!(shown.get("duotone").is_none(), "{what}: {shown}");
            assert_eq!(options(made), None, "{what}");
            assert_eq!(shown["gray"], descriptor(gray)["gray"], "{what}");
            assert_eq!(profile(made), profile(gray), "{what}: the twin");
            assert_eq!(layer16(made, 0), layer16(gray, 0), "{what}: the samples");
            free(made);
        }
        unsafe { dr_clipboard_payload_free(decoded) };
        unsafe { dr_clipboard_payload_free(payload) };
        free(doc);
        free(gray);
    }
}
