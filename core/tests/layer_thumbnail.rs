//! Layer-panel thumbnails fitted before any whole-layer copy:
//! `dr_doc_layer_thumbnail`'s raster arm and `dr_precision_image_thumbnail_rgba8`.
//!
//! Oracles are the paths these replace, run through the FFI: at 8 bits the
//! single Triangle pass over the layer (`dr_image_resize(layer,
//! FILTER_BILINEAR)`), at 16 bits the host's former pair
//! (`dr_precision_image_preview_rgba8` then `dr_image_resize`). A layer below
//! the box threshold (16× the thumbnail per axis) must equal them byte for
//! byte; a larger one must stay within 2 codes of them on alpha and on every
//! premultiplied channel (the bound `ffi_channel::THUMB_BOX_GUARD`
//! documents for the two-step plane thumbnail) — never widened.
//!
//! The same oracles and bound cover batch 80's additions: the plane
//! thumbnails' equal-area boxes (`dr_doc_composite_plane_image` against the
//! full plane through `dr_image_resize`), the float layer thumbnail
//! (`dr_precision_doc_display_thumbnail_rgba8` against
//! `dr_precision_doc_display_rgba8` + `dr_image_resize`) and the typed mask
//! thumbnail (`dr_precision_doc_layer_mask_thumbnail_rgba8` against the
//! per-sample `(v + 128) / 257` gray image + `dr_image_resize`).
//!
//! Batch 83 adds the 8-bit mask thumbnail (`dr_doc_layer_mask_thumbnail`
//! against `dr_doc_layer_mask_image` + `dr_image_resize`, raster masks and a
//! group's canvas view moved both ways), the 16-bit Channels rows
//! (`dr_precision_doc_channel_thumbnail_rgba8` /
//! `dr_precision_doc_layer_mask_canvas_thumbnail_rgba8` against
//! `dr_precision_doc_read_plane` with `canvas: true`, the per-sample gray
//! image and `dr_image_resize`, masks moved both ways), and SHA-256
//! pins of the existing box paths' bytes captured before
//! `doc_plane::box_reduced` became a wrapper over `box_reduced_rows`.
//!
//! Batch 87 pins the 8-bit mask and 16-bit Channels row bytes before the
//! reducers moved to `thumbnail_reduce`, and checks that a typed
//! GROUP's mask thumbnail (`dr_precision_doc_layer_mask_thumbnail_rgba8`) is
//! its canvas view: byte for byte `dr_doc_layer_mask_thumbnail` on the byte
//! group it was promoted from and
//! `dr_precision_doc_layer_mask_canvas_thumbnail_rgba8`.

use crate::common;

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;
use std::time::Instant;

use common::probe::{cancelled_parked_at, parked_line};
use common::take_err_string;
use doctored_core::doc::DrDocument;
use doctored_core::ffi::{
    dr_image_free, dr_image_from_rgba, dr_image_height, dr_image_pixels_rgba, dr_image_resize,
    dr_image_width,
};
use doctored_core::ffi_channel::{dr_doc_composite_plane_image, dr_image_plane_image};
use doctored_core::ffi_doc::{
    dr_doc_adding_layer_mask, dr_doc_free, dr_doc_layer_image, dr_doc_layer_mask_image,
    dr_doc_layer_mask_thumbnail, dr_doc_layer_thumbnail, dr_doc_with_layer_blend_mode,
};
use doctored_core::ffi_float::{
    dr_precision_doc_display_rgba8, dr_precision_doc_display_thumbnail_rgba8,
    dr_precision_doc_set_tone_map,
};
use doctored_core::ffi_group::{dr_doc_group_layers, dr_doc_layer_is_group};
use doctored_core::ffi_jobs::{dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_mask::{dr_doc_set_mask, dr_doc_transform_mask};
use doctored_core::ffi_precision::{
    dr_precision_doc_channel_thumbnail_rgba8, dr_precision_doc_free,
    dr_precision_doc_layer_mask_canvas_thumbnail_rgba8,
    dr_precision_doc_layer_mask_thumbnail_rgba8, dr_precision_image_preview_rgba8,
    dr_precision_image_thumbnail_rgba8,
};
use doctored_core::ffi_precision_conversion::dr_doc_promote_structure;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_read_plane};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::tiff_float::{self, FloatTiff};
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

const FILTER_BILINEAR: i32 = 1;
const SIDE: u32 = 34;

/// A deterministic hash in 0..=255, so the noise is fixture data, not a
/// random draw.
fn hash(x: u32, y: u32) -> u32 {
    let mut v = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77);
    v ^= v >> 15;
    v = v.wrapping_mul(0x2C1B_3C6D);
    v ^= v >> 12;
    v & 0xFF
}

/// Opaque: a diagonal gradient, ±24 of hashed noise and a hard edge (the
/// colour inverts at 37% of the width).
fn opaque(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        let base = |c: u32| {
            let g = (x * 255 / width + y * 255 / height + c * 60) % 256;
            let n = hash(x + c * 7919, y) as i32 / 5 - 25;
            (g as i32 + n).clamp(0, 255) as u8
        };
        let mut p = [base(0), base(1), base(2), 255];
        if x > width * 37 / 100 {
            for c in &mut p[..3] {
                *c = 255 - *c;
            }
        }
        Rgba(p)
    })
}

/// Partial alpha: the opaque picture under a vertical alpha ramp with hashed
/// noise, and a HIDDEN-COLOUR band (alpha 0, bright RGB) across 10% of the
/// height that must not tint its neighbours.
fn partial(width: u32, height: u32) -> RgbaImage {
    let picture = opaque(width, height);
    RgbaImage::from_fn(width, height, |x, y| {
        let mut p = picture.get_pixel(x, y).0;
        let band = (height * 45 / 100..height * 55 / 100).contains(&y);
        p[3] = if band {
            0
        } else {
            ((y * 255 / height) as i32 + hash(y, x) as i32 / 8 - 16).clamp(0, 255) as u8
        };
        if band {
            p[0] = 255;
            p[1] = 0;
            p[2] = 255;
        }
        Rgba(p)
    })
}

/// A u16 twin: each byte promoted by 257 plus hashed low bits, so the typed
/// path really carries more than 8 bits.
fn deep(image: &RgbaImage) -> PrecisionImage {
    let (width, height) = image.dimensions();
    let samples = image
        .enumerate_pixels()
        .map(|(x, y, p)| {
            let low = |c: u32| (hash(x ^ c, y + c) % 200) as u16;
            [
                (u16::from(p[0]) * 257).saturating_add(low(1)),
                (u16::from(p[1]) * 257).saturating_add(low(2)),
                (u16::from(p[2]) * 257).saturating_add(low(3)),
                u16::from(p[3]) * 257,
            ]
        })
        .collect();
    PrecisionImage::from_rgba16(width, height, samples, IccProfile::srgb(), None).unwrap()
}

/// An owned image answer (freed here) as `(w, h, bytes)`.
fn take(image: *mut DrImage) -> (u32, u32, Vec<u8>) {
    assert!(!image.is_null(), "an image");
    let out = unsafe {
        let (w, h) = (dr_image_width(image), dr_image_height(image));
        let raw =
            std::slice::from_raw_parts(dr_image_pixels_rgba(image), w as usize * h as usize * 4);
        (w, h, raw.to_vec())
    };
    unsafe { dr_image_free(image) };
    out
}

/// `dr_image_resize(image, w, h, FILTER_BILINEAR)`, consuming `image`.
fn resized(image: *mut DrImage, w: u32, h: u32) -> (u32, u32, Vec<u8>) {
    assert!(!image.is_null());
    let out = unsafe { dr_image_resize(image, w, h, FILTER_BILINEAR) };
    unsafe { dr_image_free(image) };
    take(out)
}

/// The largest difference on alpha and on each premultiplied channel
/// (`c·a/255`, rounded), in codes.
fn premultiplied_drift(a: &[u8], b: &[u8]) -> u32 {
    assert_eq!(a.len(), b.len());
    let pm = |p: &[u8], c: usize| (u32::from(p[c]) * u32::from(p[3]) + 127) / 255;
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .map(|(p, q)| {
            let alpha = p[3].abs_diff(q[3]) as u32;
            (0..3)
                .map(|c| pm(p, c).abs_diff(pm(q, c)))
                .fold(alpha, u32::max)
        })
        .max()
        .unwrap_or(0)
}

struct Doc(*mut DrDocument);
impl Drop for Doc {
    fn drop(&mut self) {
        unsafe { dr_doc_free(self.0) };
    }
}

fn byte_doc(image: &RgbaImage) -> Doc {
    Doc(Box::into_raw(Box::new(DrDocument::from_pixels(
        image.clone(),
    ))))
}

/// The old 8-bit thumbnail: the single Triangle pass over the whole layer.
fn byte_oracle(doc: &Doc, w: u32, h: u32) -> (u32, u32, Vec<u8>) {
    resized(unsafe { dr_doc_layer_image(doc.0, 0) }, w, h)
}

fn typed_thumbnail(
    image: &PrecisionImage,
    max_side: u32,
    limit: u64,
    control: *const DrJobControl,
) -> Result<(u32, u32, Vec<u8>), String> {
    let mut error: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_image_thumbnail_rgba8(image, max_side, limit, 0, control, &mut error)
    };
    if out.is_null() {
        return Err(take_err_string(error));
    }
    assert!(error.is_null());
    Ok(take(out))
}

/// The old 16-bit thumbnail: the whole image quantized, then (when it
/// shrinks) `dr_image_resize` to `w` x `h`.
fn typed_oracle(image: &PrecisionImage, w: u32, h: u32) -> (u32, u32, Vec<u8>) {
    let mut error: *mut c_char = ptr::null_mut();
    let preview = unsafe {
        dr_precision_image_preview_rgba8(image, REFERENCE_WORKING_BYTES, 0, ptr::null(), &mut error)
    };
    assert!(!preview.is_null(), "{}", take_err_string(error));
    if image.dimensions() == (w, h) {
        return take(preview);
    }
    resized(preview, w, h)
}

#[test]
fn a_large_byte_layer_is_box_reduced_within_two_codes_of_the_single_pass() {
    for (name, picture) in [
        ("opaque", opaque(2400, 1600)),
        ("partial alpha", partial(2400, 1600)),
    ] {
        let doc = byte_doc(&picture);
        let (w, h, thumb) = take(unsafe { dr_doc_layer_thumbnail(doc.0, 0, SIDE) });
        assert_eq!((w, h), (34, 23), "{name}: thumb_dims");
        let (_, _, oracle) = byte_oracle(&doc, w, h);
        let drift = premultiplied_drift(&thumb, &oracle);
        println!("8-bit {name} 2400x1600 -> {w}x{h}: max drift {drift}");
        assert!(drift <= 2, "{name}: drift {drift} > 2");
    }
}

#[test]
fn a_small_byte_layer_keeps_the_single_pass_byte_for_byte() {
    for (name, picture) in [
        ("opaque", opaque(200, 150)),
        ("partial alpha", partial(200, 150)),
        ("upscaled", opaque(9, 5)),
    ] {
        let doc = byte_doc(&picture);
        let (w, h, thumb) = take(unsafe { dr_doc_layer_thumbnail(doc.0, 0, SIDE) });
        let (_, _, oracle) = byte_oracle(&doc, w, h);
        assert_eq!(thumb, oracle, "{name}: identical bytes");
    }
}

#[test]
fn a_large_typed_layer_is_box_reduced_within_two_codes_of_the_former_pair() {
    for (name, picture) in [
        ("opaque", opaque(2400, 1600)),
        ("partial alpha", partial(2400, 1600)),
    ] {
        let image = deep(&picture);
        let (w, h, thumb) =
            typed_thumbnail(&image, SIDE, REFERENCE_WORKING_BYTES, ptr::null()).unwrap();
        assert_eq!((w, h), (34, 23), "{name}: the host's former fit");
        let (_, _, oracle) = typed_oracle(&image, w, h);
        let drift = premultiplied_drift(&thumb, &oracle);
        println!("16-bit {name} 2400x1600 -> {w}x{h}: max drift {drift}");
        assert!(drift <= 2, "{name}: drift {drift} > 2");
    }
}

#[test]
fn a_small_typed_layer_keeps_its_dimensions_and_bytes() {
    // (dimensions, max side, expected thumbnail size): never upscaled; the
    // former fit's tie rounding (56 x 42 at 34 is 34 x 25 where `thumb_dims`
    // says 34 x 26); an ordinary shrink below the box threshold.
    for (width, height, max_side, expected) in [
        (20, 10, SIDE, (20, 10)),
        (34, 12, SIDE, (34, 12)),
        (56, 42, SIDE, (34, 25)),
        (200, 150, SIDE, (34, 26)),
        (200, 80, 28, (28, 11)),
    ] {
        for (name, picture) in [
            ("opaque", opaque(width, height)),
            ("partial alpha", partial(width, height)),
        ] {
            let image = deep(&picture);
            let (w, h, thumb) =
                typed_thumbnail(&image, max_side, REFERENCE_WORKING_BYTES, ptr::null()).unwrap();
            assert_eq!((w, h), expected, "{name} {width}x{height} at {max_side}");
            let (_, _, oracle) = typed_oracle(&image, w, h);
            assert_eq!(thumb, oracle, "{name} {width}x{height}: identical bytes");
        }
    }
}

#[test]
fn the_typed_thumbnail_refuses_float_the_budget_and_a_cancelled_token() {
    let float = PrecisionImage::from_rgba32(
        4,
        4,
        vec![[0.25, 0.5, 0.75, 1.0]; 16],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    assert_eq!(
        typed_thumbnail(&float, SIDE, REFERENCE_WORKING_BYTES, ptr::null()).unwrap_err(),
        "a 32-bit float image is displayed through its document's tone map, not by direct RGBA8 conversion"
    );
    let image = deep(&opaque(400, 300));
    let refusal = typed_thumbnail(&image, SIDE, 1, ptr::null()).unwrap_err();
    assert!(
        refusal.contains("working limit is 1 bytes"),
        "limit 1: {refusal}"
    );
    let token = DrJobControl::default();
    token.cancel();
    let refusal = typed_thumbnail(&image, SIDE, REFERENCE_WORKING_BYTES, &token).unwrap_err();
    assert!(refusal.contains("cancelled"), "pre-cancelled: {refusal}");
    assert_eq!(token.scratch_bytes(), 0);
    let mut error: *mut c_char = ptr::null_mut();
    let null = unsafe {
        dr_precision_image_thumbnail_rgba8(
            ptr::null(),
            SIDE,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(null.is_null());
    assert_eq!(take_err_string(error), "required handle is NULL");
    assert!(unsafe {
        dr_precision_image_thumbnail_rgba8(
            ptr::null(),
            SIDE,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
}

#[test]
fn a_cancel_partway_through_the_box_pass_returns_and_holds_no_whole_layer_copy() {
    let (width, height) = (2400u32, 1600u32);
    let image = Arc::new(deep(&opaque(width, height)));
    // The box pass polls once per 4096 samples it visits; `begin` and the
    // reservation are the two checkpoints before it, and the resize and
    // quantize after it add more.
    let box_checkpoints = u64::from(width * height).div_ceil(4096);
    let control = unsafe { dr_job_control_create() };
    let full = typed_thumbnail(&image, SIDE, REFERENCE_WORKING_BYTES, control);
    let total = unsafe { &*control }.checkpoints();
    unsafe { dr_job_control_free(control) };
    assert!(full.is_ok());
    assert!(
        total > 2 + box_checkpoints,
        "the run polls through the box pass: {total} checkpoints"
    );
    let source = Arc::clone(&image);
    let parked = cancelled_parked_at(
        2 + box_checkpoints / 2,
        &|progress, _| progress == 0.0,
        move |token| match typed_thumbnail(
            &source,
            SIDE,
            REFERENCE_WORKING_BYTES,
            token as *const DrJobControl,
        ) {
            Ok(_) => (true, String::new()),
            Err(message) => (false, message),
        },
    );
    println!("box pass cancel: {}", parked_line(&parked));
    assert!(parked.scratch > 0, "the box arm reserved its scratch");
    assert!(
        parked.scratch < u64::from(width * height) * 4,
        "the box arm reserves less than a whole-layer RGBA8 copy: {}",
        parked.scratch
    );
}

// ------------------------------------------------- plane thumbnails --

const PLANE_RED: i32 = 0;

/// Opaque two-tone red: 30 left of 37% of the width, 220 right of it.
fn hard_edge(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, _| {
        let v = if x > width * 37 / 100 { 220 } else { 30 };
        Rgba([v, 90, 160, 255])
    })
}

/// The largest per-byte difference between two images, in codes.
fn byte_drift(a: &[u8], b: &[u8]) -> u32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b.iter())
        .map(|(p, q)| u32::from(p.abs_diff(*q)))
        .max()
        .unwrap_or(0)
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

#[test]
fn a_large_plane_thumbnail_is_box_reduced_within_two_codes_of_the_single_pass() {
    for (name, picture) in [
        ("opaque", opaque(2400, 1600)),
        ("hard edge", hard_edge(2400, 1600)),
    ] {
        let doc = byte_doc(&picture);
        let (w, h, thumb) = take(unsafe { dr_doc_composite_plane_image(doc.0, PLANE_RED, SIDE) });
        assert_eq!((w, h), (34, 23), "{name}: thumb_dims");
        let (_, _, oracle) = resized(
            unsafe { dr_doc_composite_plane_image(doc.0, PLANE_RED, 0) },
            w,
            h,
        );
        let drift = byte_drift(&thumb, &oracle);
        println!("plane {name} 2400x1600 -> {w}x{h}: max drift {drift}");
        assert!(drift <= 2, "{name}: drift {drift} > 2");
    }

    // The measured cost the plane reducer's comments cite: a 6000 x 4000
    // plane thumbnail at 34, median of three (printed, never asserted).
    let picture = opaque(6000, 4000);
    let doc = byte_doc(&picture);
    let image = unsafe { dr_doc_layer_image(doc.0, 0) };
    assert!(!image.is_null());
    let mut composite = Vec::new();
    let mut plane = Vec::new();
    for _ in 0..3 {
        let start = Instant::now();
        let out = unsafe { dr_doc_composite_plane_image(doc.0, PLANE_RED, SIDE) };
        composite.push(start.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(take(out).0, 34);
        let start = Instant::now();
        let out = unsafe { dr_image_plane_image(image, PLANE_RED, SIDE) };
        plane.push(start.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(take(out).0, 34);
    }
    unsafe { dr_image_free(image) };
    println!(
        "plane 6000x4000 -> 34: dr_doc_composite_plane_image {:.1} ms, dr_image_plane_image {:.1} ms (medians of 3)",
        median(composite),
        median(plane)
    );
}

// ------------------------------------------- float layer thumbnails --

struct Typed(*mut PrecisionDocument);
impl Drop for Typed {
    fn drop(&mut self) {
        unsafe { dr_precision_doc_free(self.0) };
    }
}
impl Typed {
    fn new(document: PrecisionDocument) -> Self {
        Typed(Box::into_raw(Box::new(document)))
    }
    fn source(&self) -> *const PrecisionImage {
        unsafe { &*self.0 }.layer_source(0).expect("layer 0") as *const PrecisionImage
    }
}

/// `image` as a float TIFF (RGB = byte/255·2, so above white; alpha =
/// byte/255, straight), opened as a float document.
fn float_doc(directory: &TempDir, name: &str, image: &RgbaImage) -> Typed {
    let samples = image
        .pixels()
        .flat_map(|p| {
            [
                f32::from(p[0]) / 255.0 * 2.0,
                f32::from(p[1]) / 255.0 * 2.0,
                f32::from(p[2]) / 255.0 * 2.0,
                f32::from(p[3]) / 255.0,
            ]
        })
        .collect();
    let bytes = tiff_float::encode_to_vec(
        &FloatTiff {
            width: image.width(),
            height: image.height(),
            channels: 4,
            alpha: Some(false),
            samples,
            icc: None,
            resolution: None,
            metadata: Metadata::default(),
            non_finite: 0,
        },
        false,
    )
    .unwrap();
    let path = directory.path().join(format!("{name}.tiff"));
    std::fs::write(&path, &bytes).unwrap();
    open_float(path.to_str().unwrap())
}

fn open_float(path: &str) -> Typed {
    let document =
        PrecisionDocument::open_preserving(path, REFERENCE_WORKING_BYTES, 0, None).unwrap();
    assert!(document.is_float(), "{path} opens as a float document");
    Typed::new(document)
}

/// The document under the tone map `json`.
fn tone_mapped(document: &Typed, json: &str) -> Typed {
    let request = CString::new(json).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_tone_map(document.0, request.as_ptr(), &mut error) };
    assert!(!out.is_null(), "{json}: {}", take_err_string(error));
    Typed(out)
}

fn float_thumbnail(
    document: &Typed,
    image: *const PrecisionImage,
    max_side: u32,
    limit: u64,
    control: *const DrJobControl,
) -> Result<(u32, u32, Vec<u8>), String> {
    let mut error: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_display_thumbnail_rgba8(
            document.0, image, max_side, limit, 0, control, &mut error,
        )
    };
    if out.is_null() {
        return Err(take_err_string(error));
    }
    assert!(error.is_null());
    Ok(take(out))
}

/// The host's former float thumbnail: the whole layer through the tone map,
/// then (when it shrinks) `dr_image_resize`.
fn float_oracle(document: &Typed, w: u32, h: u32) -> (u32, u32, Vec<u8>) {
    let mut error: *mut c_char = ptr::null_mut();
    let display = unsafe {
        dr_precision_doc_display_rgba8(
            document.0,
            document.source(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!display.is_null(), "{}", take_err_string(error));
    if unsafe { (dr_image_width(display), dr_image_height(display)) } == (w, h) {
        return take(display);
    }
    resized(display, w, h)
}

#[test]
fn a_large_float_layer_is_box_reduced_within_two_codes_of_the_former_pair() {
    let directory = TempDir::new().unwrap();
    for (name, picture) in [
        ("opaque", opaque(2400, 1600)),
        ("partial alpha", partial(2400, 1600)),
    ] {
        let clip = float_doc(&directory, name, &picture);
        let reinhard = tone_mapped(&clip, r#"{"operator":"reinhard","white":4}"#);
        let mut thumbs = Vec::new();
        for (map, document) in [("clip", &clip), ("reinhard", &reinhard)] {
            let (w, h, thumb) = float_thumbnail(
                document,
                document.source(),
                SIDE,
                REFERENCE_WORKING_BYTES,
                ptr::null(),
            )
            .unwrap();
            assert_eq!((w, h), (34, 23), "{name} {map}: the host's former fit");
            let (_, _, oracle) = float_oracle(document, w, h);
            let drift = premultiplied_drift(&thumb, &oracle);
            println!("float {name} {map} 2400x1600 -> {w}x{h}: max drift {drift}");
            assert!(drift <= 2, "{name} {map}: drift {drift} > 2");
            thumbs.push(thumb);
        }
        assert_ne!(
            thumbs[0], thumbs[1],
            "{name}: the tone map reaches the thumbnail"
        );
    }
}

#[test]
fn a_small_float_layer_keeps_the_former_pair_byte_for_byte() {
    let directory = TempDir::new().unwrap();
    let scene = open_float("tests/fixtures/float_11f/scene.tiff");
    let documents = [
        ("scene.tiff", scene),
        (
            "opaque 200x150",
            float_doc(&directory, "o", &opaque(200, 150)),
        ),
        (
            "partial 200x150",
            float_doc(&directory, "p", &partial(200, 150)),
        ),
        ("opaque 20x10", float_doc(&directory, "s", &opaque(20, 10))),
    ];
    for (name, clip) in &documents {
        let reinhard = tone_mapped(clip, r#"{"operator":"reinhard","white":4}"#);
        for (map, document) in [("clip", clip), ("reinhard", &reinhard)] {
            let (w, h, thumb) = float_thumbnail(
                document,
                document.source(),
                SIDE,
                REFERENCE_WORKING_BYTES,
                ptr::null(),
            )
            .unwrap();
            let (ow, oh, oracle) = float_oracle(document, w, h);
            assert_eq!((w, h), (ow, oh), "{name} {map}");
            assert_eq!(thumb, oracle, "{name} {map}: identical bytes");
        }
    }
}

#[test]
fn the_float_thumbnail_refuses_by_name() {
    let directory = TempDir::new().unwrap();
    let float = float_doc(&directory, "f", &opaque(400, 300));
    let deep_image = deep(&opaque(400, 300));
    let u16_document = Typed::new(
        PrecisionDocument::from_image(
            deep_image.clone(),
            Metadata::default(),
            Resolution::default(),
        )
        .unwrap(),
    );
    assert_eq!(
        float_thumbnail(
            &u16_document,
            u16_document.source(),
            SIDE,
            REFERENCE_WORKING_BYTES,
            ptr::null()
        )
        .unwrap_err(),
        "tone mapping requires a 32-bit float document"
    );
    assert_eq!(
        float_thumbnail(
            &float,
            &deep_image,
            SIDE,
            REFERENCE_WORKING_BYTES,
            ptr::null()
        )
        .unwrap_err(),
        "tone mapping requires rgba32_float storage"
    );
    let refusal = float_thumbnail(&float, float.source(), SIDE, 1, ptr::null()).unwrap_err();
    assert!(
        refusal.contains("working limit is 1 bytes"),
        "limit 1: {refusal}"
    );
    let token = DrJobControl::default();
    token.cancel();
    let refusal = float_thumbnail(
        &float,
        float.source(),
        SIDE,
        REFERENCE_WORKING_BYTES,
        &token,
    )
    .unwrap_err();
    assert!(refusal.contains("cancelled"), "pre-cancelled: {refusal}");
    assert_eq!(token.scratch_bytes(), 0);
    let null = Typed(ptr::null_mut());
    assert_eq!(
        float_thumbnail(
            &null,
            float.source(),
            SIDE,
            REFERENCE_WORKING_BYTES,
            ptr::null()
        )
        .unwrap_err(),
        "required handle is NULL"
    );
    assert_eq!(
        float_thumbnail(
            &float,
            ptr::null(),
            SIDE,
            REFERENCE_WORKING_BYTES,
            ptr::null()
        )
        .unwrap_err(),
        "required handle is NULL"
    );
    assert!(unsafe {
        dr_precision_doc_display_thumbnail_rgba8(
            ptr::null(),
            ptr::null(),
            SIDE,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
}

// --------------------------------------------- typed mask thumbnails --

/// A u16 document of `image` whose layer 0 carries `mask` (`w` x `h`),
/// set through `dr_precision_doc_edit`.
fn masked_doc(image: &PrecisionImage, mask: &[u16], w: u32, h: u32) -> Typed {
    let (iw, ih) = image.dimensions();
    let document = Typed::new(
        PrecisionDocument::from_parts(
            iw,
            ih,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![PrecisionDocumentLayer::raster("Base", image.clone(), (0, 0), 1.0, true).unwrap()],
        )
        .unwrap(),
    );
    let request = CString::new(format!(
        r#"{{"op":"set_mask","layer":0,"width":{w},"height":{h}}}"#
    ))
    .unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            document.0,
            request.as_ptr(),
            mask.as_ptr(),
            mask.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!out.is_null(), "set_mask: {}", take_err_string(error));
    Typed(out)
}

/// The red channel of `deep(picture)` as a mask plane.
fn red_mask(picture: &RgbaImage) -> Vec<u16> {
    deep(picture)
        .to_rgba16()
        .unwrap()
        .iter()
        .map(|p| p[0])
        .collect()
}

fn mask_thumbnail(
    document: &Typed,
    index: usize,
    max_side: u32,
    limit: u64,
    control: *const DrJobControl,
) -> Result<(u32, u32, Vec<u8>), String> {
    let mut error: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_layer_mask_thumbnail_rgba8(
            document.0, index, max_side, limit, 0, control, &mut error,
        )
    };
    if out.is_null() {
        return Err(take_err_string(error));
    }
    assert!(error.is_null());
    Ok(take(out))
}

/// The host's former mask thumbnail: every code as `(v + 128) / 257` gray,
/// then (when it shrinks) `dr_image_resize`.
fn mask_oracle(mask: &[u16], mw: u32, mh: u32, w: u32, h: u32) -> (u32, u32, Vec<u8>) {
    let bytes: Vec<u8> = mask
        .iter()
        .flat_map(|&v| {
            let g = ((u32::from(v) + 128) / 257) as u8;
            [g, g, g, 255]
        })
        .collect();
    let image = unsafe { dr_image_from_rgba(bytes.as_ptr(), mw, mh) };
    if (mw, mh) == (w, h) {
        return take(image);
    }
    resized(image, w, h)
}

#[test]
fn a_large_typed_mask_is_box_reduced_within_two_codes_of_the_former_pair() {
    let picture = opaque(2400, 1600);
    let mask = red_mask(&picture);
    let document = masked_doc(&deep(&picture), &mask, 2400, 1600);
    let (w, h, thumb) =
        mask_thumbnail(&document, 0, SIDE, REFERENCE_WORKING_BYTES, ptr::null()).unwrap();
    assert_eq!((w, h), (34, 23), "the former fit");
    let (_, _, oracle) = mask_oracle(&mask, 2400, 1600, w, h);
    let drift = byte_drift(&thumb, &oracle);
    println!("mask 2400x1600 -> {w}x{h}: max drift {drift}");
    assert!(drift <= 2, "drift {drift} > 2");
}

#[test]
fn a_small_typed_mask_keeps_the_former_pair_byte_for_byte() {
    for (width, height, expected) in [(200u32, 150u32, (34, 26)), (20, 10, (20, 10))] {
        let picture = opaque(width, height);
        let mask = red_mask(&picture);
        let document = masked_doc(&deep(&picture), &mask, width, height);
        let (w, h, thumb) =
            mask_thumbnail(&document, 0, SIDE, REFERENCE_WORKING_BYTES, ptr::null()).unwrap();
        assert_eq!((w, h), expected, "{width}x{height}");
        let (_, _, oracle) = mask_oracle(&mask, width, height, w, h);
        assert_eq!(thumb, oracle, "{width}x{height}: identical bytes");
    }
}

#[test]
fn the_mask_thumbnail_refuses_by_name() {
    let picture = opaque(400, 300);
    let image = deep(&picture);
    let plain = Typed::new(
        PrecisionDocument::from_image(image.clone(), Metadata::default(), Resolution::default())
            .unwrap(),
    );
    assert_eq!(
        mask_thumbnail(&plain, 0, SIDE, REFERENCE_WORKING_BYTES, ptr::null()).unwrap_err(),
        "high-depth target has no mask"
    );
    let masked = masked_doc(&image, &red_mask(&picture), 400, 300);
    assert_eq!(
        mask_thumbnail(&masked, 1, SIDE, REFERENCE_WORKING_BYTES, ptr::null()).unwrap_err(),
        "layer index is out of range"
    );
    let refusal = mask_thumbnail(&masked, 0, SIDE, 1, ptr::null()).unwrap_err();
    assert!(
        refusal.contains("working limit is 1 bytes"),
        "limit 1: {refusal}"
    );
    let token = DrJobControl::default();
    token.cancel();
    let refusal = mask_thumbnail(&masked, 0, SIDE, REFERENCE_WORKING_BYTES, &token).unwrap_err();
    assert!(refusal.contains("cancelled"), "pre-cancelled: {refusal}");
    assert_eq!(token.scratch_bytes(), 0);
    let null = Typed(ptr::null_mut());
    assert_eq!(
        mask_thumbnail(&null, 0, SIDE, REFERENCE_WORKING_BYTES, ptr::null()).unwrap_err(),
        "required handle is NULL"
    );
    assert!(unsafe {
        dr_precision_doc_layer_mask_thumbnail_rgba8(
            ptr::null(),
            0,
            SIDE,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
}

// ------------------------------------- box path byte pins (batch 83) --

/// SHA-256 of the box-path bytes captured at HEAD 8d7ed3d, BEFORE
/// `doc_plane::box_reduced`'s body moved into `box_reduced_rows`: the
/// one-run-per-row wrapper must keep every byte (same slice sums, same f64
/// order), which the ≤ 2 drift tests above cannot see.
#[test]
fn the_box_path_bytes_are_unchanged_by_the_row_run_refactor() {
    let pins = [
        (
            "plane opaque",
            "2603df3b049eaca2ae9e0c6f998077c0d0a3f0f12181ebd30a155dde35854e67",
        ),
        (
            "plane hard edge",
            "b5da2811096b8754921119ef8898c7937163928237504bacc5c0e42ae9e051d6",
        ),
        (
            "typed mask opaque",
            "0d24ed46d0f21e4bb3ff8032e3857c6bd4ca791bda5e7699cee04449746c7bc1",
        ),
        (
            "typed mask hard edge",
            "30232bea0f7dc8a6e904f769d4dec03933398a54a37eed0b9b4d78c64b42b804",
        ),
    ];
    let mut actual = Vec::new();
    for (name, picture) in [
        ("opaque", opaque(2400, 1600)),
        ("hard edge", hard_edge(2400, 1600)),
    ] {
        let doc = byte_doc(&picture);
        let image = unsafe { dr_doc_layer_image(doc.0, 0) };
        let (w, h, bytes) = take(unsafe { dr_image_plane_image(image, PLANE_RED, SIDE) });
        unsafe { dr_image_free(image) };
        assert_eq!((w, h), (34, 23), "plane {name}");
        actual.push(common::sha256::sha256_hex(&bytes));
    }
    for (name, picture) in [
        ("opaque", opaque(2400, 1600)),
        ("hard edge", hard_edge(2400, 1600)),
    ] {
        let mask = red_mask(&picture);
        let document = masked_doc(&deep(&picture), &mask, 2400, 1600);
        let (w, h, bytes) =
            mask_thumbnail(&document, 0, SIDE, REFERENCE_WORKING_BYTES, ptr::null()).unwrap();
        assert_eq!((w, h), (34, 23), "typed mask {name}");
        actual.push(common::sha256::sha256_hex(&bytes));
    }
    for ((name, _), hash) in pins.iter().zip(&actual) {
        println!("pin {name}: {hash}");
    }
    for ((name, pin), hash) in pins.iter().zip(&actual) {
        assert_eq!(hash, pin, "{name}: box path bytes changed");
    }
}

/// SHA-256 of the row-run box paths' bytes captured at HEAD cbeb0b7, BEFORE
/// the reducers moved from `doc_plane` to `thumbnail_reduce`: a pure
/// move keeps every byte of the 8-bit mask (raster and group canvas view)
/// and the 16-bit Channels rows.
#[test]
fn the_box_path_bytes_are_unchanged_by_the_reducer_move() {
    let pins = [
        (
            "8-bit mask opaque",
            "dd9b556f5f6287165640b98951f065fba7b7af3671ba1840affbe043f4c2e2e8",
        ),
        (
            "8-bit mask hard edge",
            "30232bea0f7dc8a6e904f769d4dec03933398a54a37eed0b9b4d78c64b42b804",
        ),
        (
            "8-bit group mask (-37, -23)",
            "143909737370a2851f113df674d4eea3a898b9f2dfb81dc6693ac815b8c10ea1",
        ),
        (
            "8-bit group mask (41, 19)",
            "3908a59636eb8cd9aa73c6f35be51b55c319c262d5c2a8ee6cf12426159f78a2",
        ),
        (
            "16-bit canvas mask",
            "5af7d89af6c9bf79eeed79945ef52fefe8c9626b2b6e71595be4f7330bc73bb6",
        ),
        (
            "16-bit channel",
            "ebd8426666d895ab741040851b5b6f19a0126bdcc9d5c77078a34e47caaf8ee8",
        ),
    ];
    let mut actual = Vec::new();
    for picture in [opaque(2400, 1600), hard_edge(2400, 1600)] {
        let doc = byte_masked_doc(&picture);
        actual.push(take(unsafe { dr_doc_layer_mask_thumbnail(doc.0, 0, SIDE) }));
    }
    for shift in [(-37, -23), (41, 19)] {
        let (doc, group) = byte_group_masked_doc(&opaque(2400, 1600), shift);
        actual.push(take(unsafe {
            dr_doc_layer_mask_thumbnail(doc.0, group, SIDE)
        }));
    }
    let picture = opaque(2400, 1600);
    let document = channels_doc(&picture, &red_mask(&picture), (2400, 1600), (-37, -23));
    for (_, export, _, index) in channels_rows() {
        let row = channels_thumbnail(
            export,
            &document,
            index,
            SIDE,
            REFERENCE_WORKING_BYTES,
            ptr::null(),
        );
        actual.push(row.unwrap());
    }
    let actual: Vec<String> = (pins.iter().zip(&actual))
        .map(|((name, _), (w, h, bytes))| {
            assert_eq!((*w, *h), (34, 23), "{name}");
            common::sha256::sha256_hex(bytes)
        })
        .collect();
    for ((name, _), hash) in pins.iter().zip(&actual) {
        println!("pin {name}: {hash}");
    }
    for ((name, pin), hash) in pins.iter().zip(&actual) {
        assert_eq!(hash, pin, "{name}: box path bytes changed");
    }
}

// ------------------------------------------ 8-bit mask thumbnails --

const MASK_FROM_SELECTION: i32 = 2;

/// The red plane of `picture`, one byte per pixel.
fn red_plane(picture: &RgbaImage) -> Vec<u8> {
    picture.pixels().map(|p| p[0]).collect()
}

/// A byte document of `picture` whose layer 0 carries the red plane as a
/// raster mask at its own size (`dr_doc_set_mask`).
fn byte_masked_doc(picture: &RgbaImage) -> Doc {
    let doc = byte_doc(picture);
    let plane = red_plane(picture);
    let (w, h) = picture.dimensions();
    let out = unsafe { dr_doc_set_mask(doc.0, 0, plane.as_ptr(), w, h, 0, 0, true, 0) };
    assert!(!out.is_null(), "set_mask");
    Doc(out)
}

/// A byte document of `picture` whose layer 0 sits in a GROUP carrying the
/// red plane as its canvas-sized mask, moved by the integer `shift`
/// (`dr_doc_transform_mask`: a lossless offset change) with outside
/// coverage 255. Answers the document and the group's index.
fn byte_group_masked_doc(picture: &RgbaImage, shift: (i32, i32)) -> (Doc, usize) {
    let doc = byte_doc(picture);
    let name = CString::new("Group").unwrap();
    let mut group = usize::MAX;
    let grouped = unsafe {
        dr_doc_group_layers(
            doc.0,
            [0usize].as_ptr(),
            1,
            name.as_ptr(),
            &mut group,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        )
    };
    assert!(!grouped.is_null(), "group_layers");
    let grouped = Doc(grouped);
    assert!(unsafe { dr_doc_layer_is_group(grouped.0, group) });
    let plane = red_plane(picture);
    let (w, h) = picture.dimensions();
    let masked = unsafe {
        dr_doc_adding_layer_mask(grouped.0, group, MASK_FROM_SELECTION, plane.as_ptr(), w, h)
    };
    assert!(!masked.is_null(), "group mask");
    let masked = Doc(masked);
    let matrix = [1.0, 0.0, 0.0, 1.0, f64::from(shift.0), f64::from(shift.1)];
    let moved = unsafe { dr_doc_transform_mask(masked.0, group, matrix.as_ptr(), FILTER_BILINEAR) };
    assert!(!moved.is_null(), "transform_mask {shift:?}");
    unsafe { (&mut *moved).layers[group].mask_properties.outside_coverage = 255 };
    (Doc(moved), group)
}

/// The former host path: `dr_doc_layer_mask_image`, then (when it shrinks)
/// `dr_image_resize` FILTER_BILINEAR.
fn byte_mask_oracle(doc: &Doc, idx: usize, w: u32, h: u32) -> (u32, u32, Vec<u8>) {
    let image = unsafe { dr_doc_layer_mask_image(doc.0, idx) };
    assert!(!image.is_null());
    if unsafe { (dr_image_width(image), dr_image_height(image)) } == (w, h) {
        return take(image);
    }
    resized(image, w, h)
}

#[test]
fn a_large_byte_mask_is_box_reduced_within_two_codes_of_the_former_pair() {
    for (name, picture) in [
        ("opaque", opaque(2400, 1600)),
        ("hard edge", hard_edge(2400, 1600)),
    ] {
        let doc = byte_masked_doc(&picture);
        let (w, h, thumb) = take(unsafe { dr_doc_layer_mask_thumbnail(doc.0, 0, SIDE) });
        assert_eq!((w, h), (34, 23), "{name}: the former fit");
        let (_, _, oracle) = byte_mask_oracle(&doc, 0, w, h);
        let drift = byte_drift(&thumb, &oracle);
        println!("8-bit mask {name} 2400x1600 -> {w}x{h}: max drift {drift}");
        assert!(drift <= 2, "{name}: drift {drift} > 2");
    }
    for shift in [(-37, -23), (41, 19)] {
        let (doc, group) = byte_group_masked_doc(&opaque(2400, 1600), shift);
        let (w, h, thumb) = take(unsafe { dr_doc_layer_mask_thumbnail(doc.0, group, SIDE) });
        assert_eq!((w, h), (34, 23), "group {shift:?}: the canvas view's fit");
        let (_, _, oracle) = byte_mask_oracle(&doc, group, w, h);
        let drift = byte_drift(&thumb, &oracle);
        println!("8-bit group mask {shift:?} 2400x1600 -> {w}x{h}: max drift {drift}");
        assert!(drift <= 2, "group {shift:?}: drift {drift} > 2");
    }
}

#[test]
fn a_small_byte_mask_keeps_the_former_pair_byte_for_byte() {
    for (width, height, expected, shifts) in [
        (200u32, 150u32, (34, 26), [(-13, -9), (11, 7)]),
        (20, 10, (20, 10), [(-3, -2), (4, 1)]),
    ] {
        for (name, picture) in [
            ("opaque", opaque(width, height)),
            ("hard edge", hard_edge(width, height)),
        ] {
            let doc = byte_masked_doc(&picture);
            let (w, h, thumb) = take(unsafe { dr_doc_layer_mask_thumbnail(doc.0, 0, SIDE) });
            assert_eq!((w, h), expected, "{name} {width}x{height}");
            let (_, _, oracle) = byte_mask_oracle(&doc, 0, w, h);
            assert_eq!(thumb, oracle, "{name} {width}x{height}: identical bytes");
        }
        for shift in shifts {
            let (doc, group) = byte_group_masked_doc(&opaque(width, height), shift);
            let (w, h, thumb) = take(unsafe { dr_doc_layer_mask_thumbnail(doc.0, group, SIDE) });
            assert_eq!((w, h), expected, "group {width}x{height} {shift:?}");
            let (_, _, oracle) = byte_mask_oracle(&doc, group, w, h);
            assert_eq!(
                thumb, oracle,
                "group {width}x{height} {shift:?}: identical bytes"
            );
        }
    }
}

#[test]
fn the_byte_mask_thumbnail_refuses_with_null() {
    let picture = opaque(40, 30);
    let plain = byte_doc(&picture);
    let masked = byte_masked_doc(&picture);
    unsafe {
        assert!(dr_doc_layer_mask_thumbnail(ptr::null(), 0, SIDE).is_null());
        assert!(dr_doc_layer_mask_thumbnail(plain.0, 0, SIDE).is_null());
        assert!(dr_doc_layer_mask_thumbnail(masked.0, 1, SIDE).is_null());
    }
    // `max_side` 0 is the full-size mask image itself.
    let (w, h, full) = take(unsafe { dr_doc_layer_mask_thumbnail(masked.0, 0, 0) });
    let (_, _, oracle) = byte_mask_oracle(&masked, 0, w, h);
    assert_eq!((w, h), (40, 30));
    assert_eq!(full, oracle);
}

// --------------------------------- 16-bit Channels row thumbnails --

/// A u16 document of `deep(picture)` whose layer 0 carries `mask` (`w` x
/// `h`) at canvas `origin` with outside coverage 255, and whose alpha
/// channel 0 is the green channel of `deep(picture)`.
fn channels_doc(picture: &RgbaImage, mask: &[u16], size: (u32, u32), origin: (i32, i32)) -> Typed {
    let image = deep(picture);
    let (iw, ih) = image.dimensions();
    let document = Typed::new(
        PrecisionDocument::from_parts(
            iw,
            ih,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![PrecisionDocumentLayer::raster("Base", image.clone(), (0, 0), 1.0, true).unwrap()],
        )
        .unwrap(),
    );
    let edit = |document: &Typed, json: String, samples: &[u16]| {
        let request = CString::new(json).unwrap();
        let mut error: *mut c_char = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_edit(
                document.0,
                request.as_ptr(),
                samples.as_ptr(),
                samples.len(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        };
        assert!(!out.is_null(), "edit: {}", take_err_string(error));
        Typed(out)
    };
    let (w, h) = size;
    let (x, y) = origin;
    let masked = edit(
        &document,
        format!(
            r#"{{"op":"set_mask","layer":0,"width":{w},"height":{h},"x":{x},"y":{y},"outside_coverage":255}}"#
        ),
        mask,
    );
    let green: Vec<u16> = image.to_rgba16().unwrap().iter().map(|p| p[1]).collect();
    edit(
        &masked,
        r#"{"op":"set_channel","name":"Alpha 1"}"#.to_string(),
        &green,
    )
}

/// `dr_precision_doc_read_plane` with `canvas: true`.
fn canvas_plane(document: &Typed, target: &str, index: usize) -> Vec<u16> {
    let (w, h) = unsafe { &*document.0 }.dimensions();
    let mut out = vec![0u16; w as usize * h as usize];
    let request = CString::new(format!(
        r#"{{"op":"read_plane","target":"{target}","index":{index},"canvas":true}}"#
    ))
    .unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_read_plane(
            document.0,
            request.as_ptr(),
            out.as_mut_ptr(),
            out.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(ok, "read_plane {target}: {}", take_err_string(error));
    out
}

type TypedExport = unsafe extern "C" fn(
    *const PrecisionDocument,
    usize,
    u32,
    u64,
    u64,
    *const DrJobControl,
    *mut *mut c_char,
) -> *mut DrImage;

fn channels_thumbnail(
    export: TypedExport,
    document: &Typed,
    index: usize,
    max_side: u32,
    limit: u64,
    control: *const DrJobControl,
) -> Result<(u32, u32, Vec<u8>), String> {
    let mut error: *mut c_char = ptr::null_mut();
    let out = unsafe { export(document.0, index, max_side, limit, 0, control, &mut error) };
    if out.is_null() {
        return Err(take_err_string(error));
    }
    assert!(error.is_null());
    Ok(take(out))
}

/// The two rows: (label, export, read_plane target, index).
fn channels_rows() -> [(&'static str, TypedExport, &'static str, usize); 2] {
    [
        (
            "mask",
            dr_precision_doc_layer_mask_canvas_thumbnail_rgba8,
            "mask",
            0,
        ),
        (
            "channel",
            dr_precision_doc_channel_thumbnail_rgba8,
            "channel",
            0,
        ),
    ]
}

#[test]
fn a_large_channels_row_is_box_reduced_within_two_codes_of_the_former_pair() {
    for (name, picture) in [
        ("opaque", opaque(2400, 1600)),
        ("hard edge", hard_edge(2400, 1600)),
    ] {
        let mask = red_mask(&picture);
        for origin in [(41, 19), (-37, -23)] {
            let document = channels_doc(&picture, &mask, (2400, 1600), origin);
            for (row, export, target, index) in channels_rows() {
                let (w, h, thumb) = channels_thumbnail(
                    export,
                    &document,
                    index,
                    SIDE,
                    REFERENCE_WORKING_BYTES,
                    ptr::null(),
                )
                .unwrap();
                assert_eq!((w, h), (34, 23), "{name} {row} {origin:?}: the former fit");
                let plane = canvas_plane(&document, target, index);
                let (_, _, oracle) = mask_oracle(&plane, 2400, 1600, w, h);
                let drift = byte_drift(&thumb, &oracle);
                println!("16-bit {row} {name} {origin:?} 2400x1600 -> {w}x{h}: max drift {drift}");
                assert!(drift <= 2, "{name} {row} {origin:?}: drift {drift} > 2");
            }
        }
    }
}

#[test]
fn a_small_channels_row_keeps_the_former_pair_byte_for_byte() {
    for (width, height, expected, origins) in [
        (200u32, 150u32, (34, 26), [(11, 7), (-13, -9)]),
        (20, 10, (20, 10), [(4, 1), (-3, -2)]),
    ] {
        let picture = opaque(width, height);
        let mask = red_mask(&picture);
        for origin in origins {
            let document = channels_doc(&picture, &mask, (width, height), origin);
            for (row, export, target, index) in channels_rows() {
                let (w, h, thumb) = channels_thumbnail(
                    export,
                    &document,
                    index,
                    SIDE,
                    REFERENCE_WORKING_BYTES,
                    ptr::null(),
                )
                .unwrap();
                assert_eq!((w, h), expected, "{row} {width}x{height} {origin:?}");
                let plane = canvas_plane(&document, target, index);
                let (_, _, oracle) = mask_oracle(&plane, width, height, w, h);
                assert_eq!(
                    thumb, oracle,
                    "{row} {width}x{height} {origin:?}: identical bytes"
                );
            }
        }
    }
}

#[test]
fn the_channels_row_thumbnails_refuse_by_name() {
    let picture = opaque(400, 300);
    let plain = Typed::new(
        PrecisionDocument::from_image(deep(&picture), Metadata::default(), Resolution::default())
            .unwrap(),
    );
    let mask = dr_precision_doc_layer_mask_canvas_thumbnail_rgba8 as TypedExport;
    let channel = dr_precision_doc_channel_thumbnail_rgba8 as TypedExport;
    let full = |export, document, index| {
        channels_thumbnail(
            export,
            document,
            index,
            SIDE,
            REFERENCE_WORKING_BYTES,
            ptr::null(),
        )
    };
    assert_eq!(
        full(mask, &plain, 0).unwrap_err(),
        "high-depth target has no mask"
    );
    assert_eq!(
        full(channel, &plain, 0).unwrap_err(),
        "high-depth channel index is out of range"
    );
    let document = channels_doc(&picture, &red_mask(&picture), (400, 300), (5, 5));
    assert_eq!(
        full(mask, &document, 1).unwrap_err(),
        "layer index is out of range"
    );
    assert_eq!(
        full(channel, &document, 1).unwrap_err(),
        "high-depth channel index is out of range"
    );
    for (row, export) in [("mask", mask), ("channel", channel)] {
        let refusal = channels_thumbnail(export, &document, 0, SIDE, 1, ptr::null()).unwrap_err();
        assert!(
            refusal.contains("working limit is 1 bytes"),
            "{row} limit 1: {refusal}"
        );
        let token = DrJobControl::default();
        token.cancel();
        let refusal =
            channels_thumbnail(export, &document, 0, SIDE, REFERENCE_WORKING_BYTES, &token)
                .unwrap_err();
        assert!(
            refusal.contains("cancelled"),
            "{row} pre-cancelled: {refusal}"
        );
        assert_eq!(token.scratch_bytes(), 0, "{row}");
        let null = Typed(ptr::null_mut());
        assert_eq!(
            channels_thumbnail(export, &null, 0, SIDE, REFERENCE_WORKING_BYTES, ptr::null())
                .unwrap_err(),
            "required handle is NULL",
            "{row}"
        );
        assert!(unsafe {
            export(
                ptr::null(),
                0,
                SIDE,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                ptr::null_mut(),
            )
        }
        .is_null());
    }
}

// ------------------------------------ typed group mask thumbnails --

const BLEND_NORMAL: i32 = 0;

/// `dr_doc_promote_structure` of `doc`: record `i` is byte layer `i`.
fn promoted(doc: &Doc) -> Typed {
    let mut error: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_promote_structure(doc.0, REFERENCE_WORKING_BYTES, 0, ptr::null(), &mut error)
    };
    assert!(!out.is_null(), "promote: {}", take_err_string(error));
    Typed(out)
}

#[test]
fn a_typed_group_mask_thumbnail_is_the_byte_canvas_view() {
    let canvas = dr_precision_doc_layer_mask_canvas_thumbnail_rgba8 as TypedExport;
    for (width, height, shifts) in [
        (2400u32, 1600u32, [(-37, -23), (41, 19)]),
        (200, 150, [(-13, -9), (11, 7)]),
        (20, 10, [(-3, -2), (4, 1)]),
    ] {
        for shift in shifts {
            // The byte group's default blend promotes to a pass-through
            // record; Normal to an isolated one.
            let (through, group) = byte_group_masked_doc(&opaque(width, height), shift);
            let isolated =
                Doc(unsafe { dr_doc_with_layer_blend_mode(through.0, group, BLEND_NORMAL) });
            assert!(!isolated.0.is_null(), "normal blend");
            for (blend, byte) in [("pass-through", &through), ("isolated", &isolated)] {
                let typed = promoted(byte);
                let sides: &[u32] = if width == 20 { &[SIDE, 0] } else { &[SIDE] };
                for &side in sides {
                    let label = format!("{blend} {width}x{height} {shift:?} side {side}");
                    let thumb =
                        mask_thumbnail(&typed, group, side, REFERENCE_WORKING_BYTES, ptr::null());
                    let oracle = take(unsafe { dr_doc_layer_mask_thumbnail(byte.0, group, side) });
                    let view = channels_thumbnail(
                        canvas,
                        &typed,
                        group,
                        side,
                        REFERENCE_WORKING_BYTES,
                        ptr::null(),
                    );
                    let thumb = thumb.unwrap();
                    assert_eq!(thumb, oracle, "{label}: the byte canvas view");
                    assert_eq!(thumb, view.unwrap(), "{label}: the canvas thumbnail");
                }
            }
        }
    }
    // The hand-off happens before the single-use control begins.
    let (byte, group) = byte_group_masked_doc(&opaque(200, 150), (11, 7));
    let typed = promoted(&byte);
    let token = DrJobControl::default();
    mask_thumbnail(&typed, group, SIDE, REFERENCE_WORKING_BYTES, &token).unwrap();
    assert_eq!(token.scratch_bytes(), 0);
}
