//! `dr_precision_doc_layer_canvas_image`, one typed record's raw
//! pixels on a transparent canvas in the document's render storage — the
//! source of MCP `render layer:` and the plane renders on a 16-bit or
//! 32-bit float document.
//!
//! Oracles are independent of the implementation: the expected canvas is
//! built HERE from the row-major samples a fixture was authored with,
//! placed at the layer's offset with zeros outside (`placed`), compared bit
//! for bit (`to_bits` at float). The u16 display oracle is the nearest
//! rounding `(v + 128) / 257`, written out per component. Float documents
//! are authored as float TIFFs and opened with `open_preserving`, so a raw
//! leaf holds exactly the samples the oracle reads.

use std::ffi::{c_char, CStr};
use std::ptr;

use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_float::dr_precision_doc_display_rgba8;
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_layer_canvas_image, dr_precision_doc_render,
    dr_precision_doc_set_layer_opacity, dr_precision_image_free, dr_precision_image_preview_rgba8,
};
use doctored_core::metadata::Metadata;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::tiff_float::FloatTiff;
use doctored_core::DrImage;
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use serde_json::json;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 19;
const H: u32 = 13;

// ------------------------------------------------------------- fixtures --

/// Extended-range straight samples: exact zeros, negatives, above-white RGB,
/// alpha 0 (hidden RGB) and alpha 1.
fn extended(width: u32, height: u32, seed: u32) -> Vec<[f32; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let v = ((x * 37 + y * 101 + seed * 13) % 1000) as f32 / 1000.0;
            let r = if (x + y) % 9 == 0 { 0.0 } else { v * 6.0 - 1.0 };
            let g = if (x * y) % 7 == 3 { -0.25 } else { 1.0 - v };
            let b = ((x ^ y) % 97) as f32 / 24.0;
            let a = match (x + 2 * y) % 11 {
                0 => 0.0,
                1 | 2 => 1.0,
                k => k as f32 / 11.0,
            };
            [r, g, b, a]
        })
        .collect()
}

/// The same range with alpha 1 everywhere, so Normal over transparent is
/// exact and a group or composite render reproduces its child's samples.
fn opaque(width: u32, height: u32, seed: u32) -> Vec<[f32; 4]> {
    extended(width, height, seed)
        .into_iter()
        .map(|[r, g, b, _]| [r, g, b, 1.0])
        .collect()
}

/// u16 samples covering 0, 65535 and the rounding boundaries of `/257`.
fn codes(width: u32, height: u32) -> Vec<[u16; 4]> {
    (0..width * height)
        .map(|i| {
            let k = i * 7919;
            [
                (k % 65536) as u16,
                ((k * 3 + 128) % 65536) as u16,
                if i % 5 == 0 {
                    65535
                } else {
                    (i * 257 + 128) as u16
                },
                match i % 4 {
                    0 => 0,
                    1 => 65535,
                    _ => (k % 65536) as u16,
                },
            ]
        })
        .collect()
}

fn float_document(width: u32, height: u32, samples: &[[f32; 4]]) -> PrecisionDocument {
    let tiff = FloatTiff {
        width,
        height,
        channels: 4,
        alpha: Some(false),
        samples: samples.iter().flatten().copied().collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("layer.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

fn u16_document(width: u32, height: u32, samples: &[[u16; 4]]) -> PrecisionDocument {
    let bytes: Vec<u8> = samples
        .iter()
        .flatten()
        .flat_map(|v| v.to_ne_bytes())
        .collect();
    let mut encoded = Vec::new();
    PngEncoder::new(&mut encoded)
        .write_image(&bytes, width, height, ExtendedColorType::Rgba16)
        .unwrap();
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("layer.png");
    std::fs::write(&path, encoded).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the 16-bit document opens");
    assert!(!doc.is_float());
    doc
}

/// The document with its canvas grown to (W+4, H+3), the layer at (2, 1).
fn resized(doc: &PrecisionDocument) -> PrecisionDocument {
    let edit = GeometryEdit::parse(
        &json!({"op":"canvas_size","width":W + 4,"height":H + 3,"x":2,"y":1}).to_string(),
    )
    .unwrap();
    doc.geometry(&edit, LIMIT, 0, None)
        .unwrap()
        .expect("resized")
}

/// `samples` (W×H at `offset`) placed on a `canvas`-sized transparent plane.
fn placed<T: Copy + Default>(
    samples: &[[T; 4]],
    size: (u32, u32),
    canvas: (u32, u32),
    offset: (i32, i32),
) -> Vec<[T; 4]> {
    (0..canvas.0 * canvas.1)
        .map(|i| {
            let x = i64::from(i % canvas.0) - i64::from(offset.0);
            let y = i64::from(i / canvas.0) - i64::from(offset.1);
            if x < 0 || y < 0 || x >= i64::from(size.0) || y >= i64::from(size.1) {
                [T::default(); 4]
            } else {
                samples[y as usize * size.0 as usize + x as usize]
            }
        })
        .collect()
}

fn bits(samples: &[[f32; 4]]) -> Vec<[u32; 4]> {
    samples.iter().map(|p| p.map(f32::to_bits)).collect()
}

// ------------------------------------------------------------------ FFI --

struct Image(*mut PrecisionImage);
impl Drop for Image {
    fn drop(&mut self) {
        unsafe { dr_precision_image_free(self.0) }
    }
}
impl Image {
    fn get(&self) -> &PrecisionImage {
        unsafe { &*self.0 }
    }
}

unsafe fn take_error(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    text
}

fn layer_canvas(doc: *const PrecisionDocument, index: usize, limit: u64) -> Result<Image, String> {
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_layer_canvas_image(doc, index, limit, 0, ptr::null(), &mut error)
    };
    if out.is_null() {
        Err(unsafe { take_error(error) })
    } else {
        assert!(error.is_null());
        Ok(Image(out))
    }
}

fn rgba8(image: *mut DrImage) -> (u32, u32, Vec<u8>) {
    assert!(!image.is_null());
    unsafe {
        let (w, h) = (dr_image_width(image), dr_image_height(image));
        let bytes =
            std::slice::from_raw_parts(dr_image_pixels_rgba(image), (w * h * 4) as usize).to_vec();
        dr_image_free(image);
        (w, h, bytes)
    }
}

fn float_display(doc: &PrecisionDocument, image: &Image) -> (u32, u32, Vec<u8>) {
    let mut error = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_display_rgba8(doc, image.0, LIMIT, 0, ptr::null(), &mut error) };
    assert!(!out.is_null(), "{}", unsafe { take_error(error) });
    rgba8(out)
}

// ---------------------------------------------------------------- tests --

#[test]
fn a_float_raster_lands_bit_for_bit_at_its_offset_with_zeros_outside() {
    let samples = extended(W, H, 4);
    let doc = resized(&float_document(W, H, &samples));
    let canvas = (W + 4, H + 3);
    assert_eq!(doc.dimensions(), canvas);
    let expected = placed(&samples, (W, H), canvas, (2, 1));
    assert!(expected.iter().any(|p| p[3] == 0.0));
    assert!(expected.iter().any(|p| p[..3].iter().any(|v| *v < 0.0)));
    assert!(expected.iter().any(|p| p[..3].iter().any(|v| *v > 1.0)));

    let image = layer_canvas(&doc, 0, LIMIT).unwrap();
    assert_eq!(image.get().storage(), PrecisionStorage::Rgba32Float);
    assert_eq!(image.get().dimensions(), canvas);
    assert_eq!(bits(&image.get().to_rgba32().unwrap()), bits(&expected));

    // Opacity is the record's, not its pixels': 0.5 reads the same samples.
    let mut error = ptr::null_mut();
    let half = unsafe { dr_precision_doc_set_layer_opacity(&doc, 0, 0.5, &mut error) };
    assert!(!half.is_null(), "{}", unsafe { take_error(error) });
    let at_half = layer_canvas(half, 0, LIMIT).unwrap();
    assert_eq!(bits(&at_half.get().to_rgba32().unwrap()), bits(&expected));
    unsafe { dr_precision_doc_free(half) };
}

#[test]
fn a_float_group_renders_alone_and_equals_its_opaque_child() {
    let solid = opaque(W, H, 5);
    let doc = float_document(W, H, &solid);
    let doubled = doc.duplicate_layer(0).unwrap().expect("duplicated");
    let (grouped, _) = doubled
        .group_layers(&[1], "Group", false)
        .unwrap()
        .expect("grouped");
    assert!(grouped.layer(2).and_then(|l| l.children()).is_some());
    let image = layer_canvas(&grouped, 2, LIMIT).unwrap();
    assert_eq!(image.get().storage(), PrecisionStorage::Rgba32Float);
    assert_eq!(bits(&image.get().to_rgba32().unwrap()), bits(&solid));
}

#[test]
fn a_u16_layer_keeps_its_codes_and_previews_by_nearest_rounding() {
    let samples = codes(W, H);
    let doc = resized(&u16_document(W, H, &samples));
    let canvas = (W + 4, H + 3);
    let expected = placed(&samples, (W, H), canvas, (2, 1));
    let image = layer_canvas(&doc, 0, LIMIT).unwrap();
    assert_eq!(image.get().storage(), PrecisionStorage::Rgba16Unorm);
    assert_eq!(image.get().dimensions(), canvas);
    assert_eq!(image.get().to_rgba16().unwrap(), expected);

    let mut error = ptr::null_mut();
    let preview =
        unsafe { dr_precision_image_preview_rgba8(image.0, LIMIT, 0, ptr::null(), &mut error) };
    assert!(!preview.is_null(), "{}", unsafe { take_error(error) });
    let (w, h, bytes) = rgba8(preview);
    assert_eq!((w, h), canvas);
    let oracle: Vec<u8> = expected
        .iter()
        .flatten()
        .map(|v| ((u32::from(*v) + 128) / 257) as u8)
        .collect();
    assert_eq!(bytes, oracle);
}

#[test]
fn a_float_layer_displays_through_the_tone_map_like_the_composite() {
    let samples = extended(W, H, 6);
    let doc = resized(&float_document(W, H, &samples));
    let image = layer_canvas(&doc, 0, LIMIT).unwrap();
    let (w, h, _) = float_display(&doc, &image);
    assert_eq!((w, h), (W + 4, H + 3));

    // One full-canvas opaque raster: the layer IS the composite.
    let solid = opaque(W, H, 7);
    let doc = float_document(W, H, &solid);
    let layer = layer_canvas(&doc, 0, LIMIT).unwrap();
    let mut error = ptr::null_mut();
    let composite = unsafe { dr_precision_doc_render(&doc, LIMIT, 0, ptr::null(), &mut error) };
    assert!(!composite.is_null(), "{}", unsafe { take_error(error) });
    let composite = Image(composite);
    assert_eq!(float_display(&doc, &layer), float_display(&doc, &composite));
}

#[test]
fn refusals_are_named() {
    let doc = float_document(W, H, &extended(W, H, 8));
    assert!(layer_canvas(&doc, 7, LIMIT)
        .err()
        .unwrap()
        .contains("analysis layer is out of range"));
    let exposure = doc
        .add_adjustment(
            "Exposure",
            "{\"type\":\"adjust\",\"evaluation\":\"precision_v1\",\"op\":\"exposure\",\"params\":{\"exposure\":1.0}}",
            1.0,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert!(layer_canvas(&exposure, 1, LIMIT)
        .err()
        .unwrap()
        .contains("an adjustment has no raw pixels; inspect its backdrop"));
    assert!(layer_canvas(&doc, 0, 1)
        .err()
        .unwrap()
        .contains("working limit"));
    let message = layer_canvas(ptr::null(), 0, LIMIT).err().unwrap();
    assert_eq!(message, "required handle is NULL");
}
