//! A pattern tile's NATIVE samples (`designs/pattern-tiles.md`), through the
//! FFI: Define Pattern's samples (`dr_clipboard_payload_pattern_native`) of
//! a CMYK, 16-bit RGB, 16-bit Gray and 32-bit float composite, filled back
//! (Edit ▸ Fill…'s `paint_blend`) and drawn as a Pattern Overlay in the same
//! kind of document.
//!
//! The oracle is the source document's own numbers: a K-only black stays
//! K-only, a 16-bit code off the byte grid and a float value above white
//! come back as they were. Each case also fills the same tile WITHOUT its
//! `native` key and asserts what the RGBA8 raster alone gives, so the red
//! state is part of the test. A bevel texture's height reads the raster
//! whatever samples its tile holds. Then the JSON's refusals, `.rz` word 115
//! both ways, the request and style-body caps a 512² `rgb32` tile meets, Convert
//! to Profile's ink map (checked against the document's own conversion of
//! the same ink), and a linked tile's turn.

use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_string_free};
use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_free, dr_clipboard_payload_image, dr_clipboard_payload_rgba8,
    dr_precision_doc_clipboard_copy, DrClipboardPayload,
};
use doctored_core::ffi_cmyk_colour::{
    dr_precision_doc_cmyk_profile_edit, dr_precision_doc_new_cmyk,
};
use doctored_core::ffi_gray::dr_precision_doc_open_gray;
use doctored_core::ffi_precision::dr_precision_image_free;
use doctored_core::ffi_style::{dr_clipboard_payload_pattern_native, dr_pattern_tile_from_linear};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::style::{Effect, LayerStyle};
use doctored_core::{samples_from_linear, TileSamples};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::{press_profile, press_profile_b};
use crate::common::{cpath, img_pixels, solid, take_err_string};
use crate::float_fixture::float_doc;
use crate::gray_fixture::{png, srgb_chunk};
use crate::indexed_fixture::{free, layer16, paint, LIMIT};

// ------------------------------------------------------------- helpers --

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                T[((n >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

fn unbase64(text: &str) -> Vec<u8> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let digits: Vec<u32> = text
        .bytes()
        .filter(|&c| c != b'=')
        .map(|c| T.iter().position(|&t| t == c).expect("a base64 digit") as u32)
        .collect();
    let mut out = Vec::new();
    for chunk in digits.chunks(4) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, d)| n | (d << (18 - 6 * i)));
        for i in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    out
}

fn u16s(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect()
}

fn f32s(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

fn le16(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn le32(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// `doc`'s whole canvas as a Copy Merged payload: Define Pattern's read.
fn copy_merged(doc: *const PrecisionDocument) -> *mut DrClipboardPayload {
    let (w, h) = unsafe { &*doc }.dimensions();
    let request = CString::new(
        json!({"op": "copy_merged", "x": 0, "y": 0, "width": w, "height": h}).to_string(),
    )
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
    assert!(!out.is_null(), "copy merged: {}", take_err_string(err));
    out
}

/// `dr_clipboard_payload_pattern_native` of `payload`, parsed.
fn native_of(payload: *const DrClipboardPayload) -> Value {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_clipboard_payload_pattern_native(payload, &mut err) };
    assert!(!out.is_null(), "native: {}", take_err_string(err));
    let text = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
    unsafe { dr_string_free(out) };
    serde_json::from_str(&text).unwrap()
}

/// The RGBA8 raster Define Pattern writes for `payload` (the host's
/// `DefinePattern.tile`): a float composite through
/// `dr_pattern_tile_from_linear`, a CMYK one converted into sRGB, any other
/// its own 8-bit numbers.
fn raster_of(payload: *const DrClipboardPayload, srgb: bool, float: bool) -> Vec<u8> {
    let mut err = ptr::null_mut();
    let image = if float {
        let linear = unsafe { dr_clipboard_payload_image(payload) };
        assert!(!linear.is_null());
        let out = unsafe { dr_pattern_tile_from_linear(linear, &mut err) };
        unsafe { dr_precision_image_free(linear) };
        out
    } else {
        let profile = IccProfile::srgb();
        let bytes = profile.bytes();
        let (p, n) = if srgb {
            (bytes.as_ptr(), bytes.len())
        } else {
            (ptr::null(), 0)
        };
        unsafe { dr_clipboard_payload_rgba8(payload, p, n, false, LIMIT, 0, ptr::null(), &mut err) }
    };
    assert!(!image.is_null(), "raster: {}", take_err_string(err));
    let pixels = img_pixels(image);
    unsafe { dr_image_free(image) };
    pixels
}

/// Define Pattern of `doc`'s whole canvas: the pattern object with its
/// `native` key, and the same object without it (what the RGBA8 raster alone
/// carries).
fn define(doc: *const PrecisionDocument, srgb: bool, float: bool) -> (Value, Value) {
    let (w, h) = unsafe { &*doc }.dimensions();
    let payload = copy_merged(doc);
    let native = native_of(payload);
    let raster = raster_of(payload, srgb, float);
    unsafe { dr_clipboard_payload_free(payload) };
    let without = json!({"name": "Defined", "width": w, "height": h, "rgba": base64(&raster)});
    assert!(native.is_object(), "the composite keeps samples: {native}");
    let mut with = without.clone();
    with["native"] = native;
    (with, without)
}

/// The typed `paint_blend` of `pattern` over the whole canvas (`ink` for a
/// CMYK document, in place of the colour).
fn fill(doc: *const PrecisionDocument, pattern: &Value, cmyk: bool) -> *mut PrecisionDocument {
    let mut request = json!({"op": "paint_blend", "layer": 0, "opacity": 1.0,
        "blend_mode": 0, "pattern": pattern});
    if cmyk {
        request["ink"] = json!([0, 0, 0, 0, 1]);
    } else {
        request["color"] = json!([0, 0, 0, 1]);
        request["color_space"] = json!("document");
    }
    let (w, h) = unsafe { &*doc }.dimensions();
    paint(doc, &request, &vec![65535; (w * h) as usize])
        .unwrap_or_else(|e| panic!("fill: {e}"))
        .expect("the fill changed something")
}

/// A one-layer document: `image` as an opaque raster at (0, 0).
fn typed(image: PrecisionImage) -> PrecisionDocument {
    let (w, h) = image.dimensions();
    let layer = PrecisionDocumentLayer::raster("Layer", image, (0, 0), 1.0, true).unwrap();
    PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap()
}

fn boxed(doc: PrecisionDocument) -> *mut PrecisionDocument {
    Box::into_raw(Box::new(doc))
}

fn rgb16_doc(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    boxed(typed(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
    ))
}

fn float_rgb_doc(w: u32, h: u32, pixels: Vec<[f32; 4]>) -> *mut PrecisionDocument {
    let profile = IccProfile::srgb();
    boxed(float_doc(w, h, &profile, &[pixels]))
}

fn float_samples(doc: *const PrecisionDocument) -> Vec<[f32; 4]> {
    unsafe { &*doc }
        .layer_source(0)
        .unwrap()
        .to_rgba32()
        .unwrap()
}

/// A 16-bit Grayscale document of `codes` (an opaque 16-bit gray PNG).
fn gray16_doc(w: u32, h: u32, codes: &[u16]) -> *mut PrecisionDocument {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("g16.png");
    std::fs::write(&path, png(w, h, 16, 0, 1, codes, &[srgb_chunk()])).unwrap();
    let working = CString::new("gray_gamma_22").unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_gray(
            cpath(&path).as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

/// File ▸ New in CMYK: `w × h` of one `fill` ink in `profile` at `depth`.
fn cmyk_doc(w: u32, h: u32, profile: &[u8], depth: u32, fill: [f64; 5]) -> *mut PrecisionDocument {
    let request = CString::new(json!({"depth": depth, "fill": fill}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_new_cmyk(
            w,
            h,
            profile.as_ptr(),
            profile.len(),
            request.as_ptr(),
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(!doc.is_null(), "new CMYK: {}", take_err_string(err));
    doc
}

fn cmyk_samples(doc: *const PrecisionDocument) -> Vec<[u16; 5]> {
    unsafe { &*doc }.cmyk_layer_samples(0).unwrap()
}

/// The typed `layer_style` edit of layer `layer`.
fn set_style(
    doc: *const PrecisionDocument,
    layer: usize,
    style: &Value,
) -> Result<*mut PrecisionDocument, String> {
    let request = json!({"op": "layer_style", "version": 1, "layer": layer, "style": style});
    paint(doc, &request, &[]).map(|d| d.expect("the style changed something"))
}

fn overlay(pattern: &Value) -> Value {
    json!({"effects": [{"type": "pattern_overlay", "pattern": pattern}]})
}

/// Texel (x mod w, y mod h) of a `w`-wide tile: the 100 % read.
fn texel_index(w: u32, h: u32, x: u32, y: u32) -> usize {
    ((y % h) * w + (x % w)) as usize
}

/// The `native` object a stored style's first pattern overlay holds.
fn stored_native(style: &LayerStyle) -> Value {
    let json: Value = serde_json::from_str(&style.to_json()).unwrap();
    json["effects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "pattern_overlay")
        .map(|e| e["pattern"]["native"].clone())
        .unwrap_or(Value::Null)
}

// --------------------------------------------------- define then fill --

#[test]
fn a_k_only_black_survives_define_then_fill_at_both_depths() {
    let press = press_profile("Press Test");
    for depth in [8, 16] {
        let black = cmyk_doc(4, 3, &press, depth, [0.0, 0.0, 0.0, 1.0, 1.0]);
        // Its left half painted with an ink off the 8-bit grid, which a CMYK
        // document's paint stores as given at both depths.
        let request = json!({"op": "paint_blend", "layer": 0, "opacity": 1.0,
            "blend_mode": 0, "ink": [0.6992447, 0.0, 0.8996, 0.0, 1.0]});
        let half: Vec<u16> = (0..12).map(|i| if i % 4 < 2 { 65535 } else { 0 }).collect();
        let source = paint(black, &request, &half).unwrap().unwrap();
        let own = cmyk_samples(source);
        assert!(
            own.contains(&[0, 0, 0, 65535, 65535]),
            "{depth} bits: K-only"
        );
        assert!(
            own.iter().any(|s| !s[0].is_multiple_of(257)),
            "{depth} bits: an ink off the 8-bit grid: {own:?}"
        );
        let (with, without) = define(source, true, false);
        assert_eq!(with["native"]["kind"], "cmyk16", "{depth} bits");
        let paper = cmyk_doc(6, 5, &press, depth, [0.0, 0.0, 0.0, 0.0, 1.0]);
        let kept = fill(paper, &with, true);
        for (i, s) in cmyk_samples(kept).iter().enumerate() {
            let want = own[texel_index(4, 3, i as u32 % 6, i as u32 / 6)];
            assert_eq!(*s, want, "{depth} bits, pixel {i}");
        }
        // The RGBA8 raster alone: sRGB back to ink through the profile, not
        // the K-only separation.
        let lost = fill(paper, &without, true);
        let k_only =
            |i: usize| own[texel_index(4, 3, i as u32 % 6, i as u32 / 6)][..4] == [0, 0, 0, 65535];
        assert!(
            cmyk_samples(lost)
                .iter()
                .enumerate()
                .all(|(i, s)| !k_only(i) || s[..4] != [0, 0, 0, 65535]),
            "{depth} bits: the raster alone comes back K-only"
        );
        for doc in [black, source, paper, kept, lost] {
            free(doc);
        }
    }
}

const OFF_GRID: [[u16; 4]; 4] = [
    [12345, 40000, 777, 65535],
    [1, 2, 3, 65535],
    [65534, 32768, 257, 65535],
    [100, 200, 300, 65535],
];

#[test]
fn a_16_bit_code_off_the_byte_grid_survives_define_then_fill() {
    let source = rgb16_doc(2, 2, OFF_GRID.to_vec());
    let (with, without) = define(source, false, false);
    assert_eq!(with["native"]["kind"], "rgb16");
    assert_eq!(
        u16s(&unbase64(with["native"]["data"].as_str().unwrap())),
        OFF_GRID.iter().flatten().copied().collect::<Vec<_>>()
    );
    let target = rgb16_doc(5, 3, vec![[65535; 4]; 15]);
    let kept = fill(target, &with, false);
    let lost = fill(target, &without, false);
    for (i, (k, l)) in layer16(kept).iter().zip(layer16(lost)).enumerate() {
        let (x, y) = (i as u32 % 5, i as u32 / 5);
        let want = OFF_GRID[texel_index(2, 2, x, y)];
        assert_eq!(*k, want, "({x}, {y})");
        // The raster's 8-bit code × 257: 12345 comes back 12336.
        let rounded = want.map(|c| (u32::from(c) + 128) / 257 * 257);
        assert_eq!(l.map(u32::from), rounded, "({x}, {y}) without samples");
    }
    for doc in [source, target, kept, lost] {
        free(doc);
    }
}

#[test]
fn a_composite_the_raster_holds_keeps_no_samples() {
    let on_grid: Vec<[u16; 4]> = OFF_GRID.iter().map(|p| p.map(|c| c / 257 * 257)).collect();
    let rgb = rgb16_doc(2, 2, on_grid);
    let float = float_rgb_doc(
        2,
        1,
        vec![[0.2, 1.0, 0.0, 1.0], [10.0 / 255.0, 0.0, 1.0, 1.0]],
    );
    for doc in [rgb, float] {
        let payload = copy_merged(doc);
        assert_eq!(native_of(payload), Value::Null);
        unsafe { dr_clipboard_payload_free(payload) };
        free(doc);
    }
}

const LINEAR: [[f32; 4]; 4] = [
    [2.5, 0.25, 0.123, 1.0],
    [0.0, 1.0, 0.5, 1.0],
    [3.75, 3.75, 3.75, 1.0],
    [0.01, 64.0, 1.5, 1.0],
];

#[test]
fn a_float_sample_above_white_survives_define_then_fill() {
    let source = float_rgb_doc(2, 2, LINEAR.to_vec());
    let (with, without) = define(source, false, true);
    assert_eq!(with["native"]["kind"], "rgb32");
    assert_eq!(
        f32s(&unbase64(with["native"]["data"].as_str().unwrap())),
        LINEAR.concat()
    );
    let target = float_rgb_doc(5, 3, vec![[0.0, 0.0, 0.0, 1.0]; 15]);
    let kept = fill(target, &with, false);
    let lost = fill(target, &without, false);
    for (i, (k, l)) in float_samples(kept)
        .iter()
        .zip(float_samples(lost))
        .enumerate()
    {
        let (x, y) = (i as u32 % 5, i as u32 / 5);
        let want = LINEAR[texel_index(2, 2, x, y)];
        assert_eq!(*k, want, "({x}, {y})");
        // The raster: round(255 · clamp(v, 0, 1)) / 255.
        let clipped = want.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() / 255.0);
        for c in 0..4 {
            assert!(
                (l[c] - clipped[c]).abs() <= 1e-6,
                "({x}, {y}) {c} without samples"
            );
        }
    }
    for doc in [source, target, kept, lost] {
        free(doc);
    }
}

#[test]
fn a_gray_16_neutral_code_off_the_byte_grid_survives_define_then_fill() {
    let source = gray16_doc(2, 2, &[12345, 40000, 777, 3]);
    let codes = layer16(source);
    assert!(
        codes.iter().any(|p| !p[0].is_multiple_of(257)),
        "the source is off the byte grid"
    );
    let (with, without) = define(source, false, false);
    assert_eq!(with["native"]["kind"], "rgb16");
    let target = gray16_doc(5, 3, &[0; 15]);
    let kept = fill(target, &with, false);
    let lost = fill(target, &without, false);
    for (i, (k, l)) in layer16(kept).iter().zip(layer16(lost)).enumerate() {
        let (x, y) = (i as u32 % 5, i as u32 / 5);
        let want = codes[texel_index(2, 2, x, y)];
        assert_eq!(*k, want, "({x}, {y})");
        if !want[0].is_multiple_of(257) {
            assert_ne!(l, want, "({x}, {y}) without samples");
        }
    }
    for doc in [source, target, kept, lost] {
        free(doc);
    }
}

#[test]
fn a_pattern_overlay_reads_the_samples_at_16_and_32_bits() {
    let rgb16 = json!({"name": "R", "width": 2, "height": 2, "rgba": base64(&[0; 16]),
        "native": {"kind": "rgb16", "data": base64(&le16(&OFF_GRID.concat()))}});
    let doc = rgb16_doc(5, 3, vec![[65535; 4]; 15]);
    let styled = set_style(doc, 0, &overlay(&rgb16)).unwrap();
    let rendered = unsafe { &*styled }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap();
    for (i, p) in rendered.iter().enumerate() {
        let (x, y) = (i as u32 % 5, i as u32 / 5);
        assert_eq!(*p, OFF_GRID[texel_index(2, 2, x, y)], "16 bits ({x}, {y})");
    }
    free(styled);

    let rgb32 = json!({"name": "F", "width": 2, "height": 2, "rgba": base64(&[0; 16]),
        "native": {"kind": "rgb32", "data": base64(&le32(&LINEAR.concat()))}});
    // A 16-bit reader takes an rgb32 value clamped to 0…1, as it reads a code.
    let styled = set_style(doc, 0, &overlay(&rgb32)).unwrap();
    let rendered = unsafe { &*styled }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap();
    for (i, p) in rendered.iter().enumerate() {
        let (x, y) = (i as u32 % 5, i as u32 / 5);
        let want = LINEAR[texel_index(2, 2, x, y)]
            .map(|v| (f64::from(v).clamp(0.0, 1.0) * 65535.0).round() as u16);
        for c in 0..3 {
            assert!(
                p[c].abs_diff(want[c]) <= 1,
                "16 bits ({x}, {y}) {c}: {p:?} vs {want:?}"
            );
        }
    }
    free(styled);
    free(doc);

    let float = float_rgb_doc(5, 3, vec![[0.0, 0.0, 0.0, 1.0]; 15]);
    let styled = set_style(float, 0, &overlay(&rgb32)).unwrap();
    let rendered = unsafe { &*styled }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba32()
        .unwrap();
    for (i, p) in rendered.iter().enumerate() {
        let (x, y) = (i as u32 % 5, i as u32 / 5);
        let want = LINEAR[texel_index(2, 2, x, y)];
        for c in 0..3 {
            assert!(
                (p[c] - want[c]).abs() <= 1e-5 * want[c].max(1.0),
                "32 bits ({x}, {y}) {c}: {} vs {}",
                p[c],
                want[c]
            );
        }
    }
    free(styled);
    free(float);
}

/// A 16-bit 12 × 12 layer, an opaque 8 × 8 square at (2, 2), under a Bevel &
/// Emboss whose texture is `pattern` at 100 %: the rendered composite.
fn textured_render(pattern: &Value) -> Vec<[u16; 4]> {
    let mut pixels = vec![[0u16; 4]; 144];
    for y in 2..10 {
        for x in 2..10 {
            pixels[y * 12 + x] = [30000, 30000, 30000, 65535];
        }
    }
    let doc = rgb16_doc(12, 12, pixels);
    let style = json!({"effects": [{"type": "bevel_emboss", "size": 3, "texture_enabled": true,
        "texture": {"pattern": pattern, "depth": 1.0}}]});
    let styled = set_style(doc, 0, &style).unwrap();
    let rendered = unsafe { &*styled }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap();
    free(styled);
    free(doc);
    rendered
}

#[test]
fn a_bevel_textures_height_reads_the_raster_whatever_samples_the_tile_holds() {
    // The raster is flat mid-grey, a texture of no relief; the samples vary
    // (OFF_GRID), so a height read from them would add relief. The ADR's
    // rule (`designs/pattern-tiles.md`, decision 2): a texture's height reads
    // the raster in every document, so the samples change nothing.
    let flat = json!({"name": "T", "width": 2, "height": 2,
        "rgba": base64(&[128u8, 128, 128, 255].repeat(4))});
    let mut with = flat.clone();
    with["native"] = json!({"kind": "rgb16", "data": base64(&le16(&OFF_GRID.concat()))});
    let reference = textured_render(&flat);
    assert!(
        textured_render(&with) == reference,
        "the tile's samples changed the bevel texture's height"
    );
    // The comparison is live: the samples' own image as the RASTER does
    // give the texture relief.
    let varied: Vec<u8> = OFF_GRID
        .iter()
        .flat_map(|p| p.map(|c| ((u32::from(c) + 128) / 257) as u8))
        .collect();
    let relief = json!({"name": "T", "width": 2, "height": 2, "rgba": base64(&varied)});
    assert!(
        textured_render(&relief) != reference,
        "a varied raster gives the texture no relief"
    );
}

#[test]
fn define_holds_a_float_sample_to_the_stored_range() {
    let held =
        samples_from_linear(&[[f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.5]]).expect("samples");
    assert_eq!(held, TileSamples::Rgb32(vec![0.0, 65504.0, -65504.0, 1.0]));
    let TileSamples::Rgb32(values) = &held else {
        unreachable!()
    };
    // What it holds, the parser takes.
    let pattern = json!({"name": "H", "width": 1, "height": 1, "rgba": base64(&[0, 255, 0, 255]),
        "native": {"kind": "rgb32", "data": base64(&le32(values))}});
    let doc = float_rgb_doc(2, 1, vec![[0.0, 0.0, 0.0, 1.0]; 2]);
    let filled = fill(doc, &pattern, false);
    assert_eq!(float_samples(filled)[0], [0.0, 65504.0, -65504.0, 1.0]);
    free(filled);
    free(doc);
}

// ------------------------------------------------------------- the JSON --

#[test]
fn a_malformed_native_object_refuses_by_name() {
    let doc = rgb16_doc(2, 2, vec![[65535; 4]; 4]);
    let tile = |native: Value| json!({"name": "M", "width": 2, "height": 1, "rgba": base64(&[0; 8]), "native": native});
    let nan = le32(&[f32::NAN, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    let alpha = le32(&[0.5, 0.0, 0.0, 1.5, 0.0, 0.0, 0.0, 1.0]);
    for (native, want) in [
        (
            json!({"kind": "rgb8", "data": ""}),
            "pattern.native.kind must be one of rgb16, rgb32, cmyk16",
        ),
        (
            json!({"kind": "rgb16", "data": base64(&[0; 10])}),
            "pattern.native.data must hold width x height x 4 2-byte samples",
        ),
        (
            json!({"kind": "cmyk16", "data": base64(&[0; 16])}),
            "pattern.native.data must hold width x height x 5 2-byte samples",
        ),
        (
            json!({"kind": "rgb32", "data": base64(&nan)}),
            "pattern.native.data must hold finite rgb32 colour within ±65504 and alpha 0…1",
        ),
        (
            json!({"kind": "rgb32", "data": base64(&alpha)}),
            "pattern.native.data must hold finite rgb32 colour within ±65504 and alpha 0…1",
        ),
        (
            json!({"kind": "rgb16", "data": base64(&[0; 16]), "extra": 1}),
            "pattern.native has no key extra",
        ),
        (
            json!("rgb16"),
            "pattern.native must be an object of kind and data",
        ),
    ] {
        let request = json!({"op": "paint_blend", "layer": 0, "color": [0, 0, 0, 1],
            "color_space": "document", "opacity": 1.0, "blend_mode": 0, "pattern": tile(native.clone())});
        let refusal = paint(doc, &request, &[65535; 4]).unwrap_err();
        assert!(refusal.contains(want), "{native}: {refusal}");
    }
    free(doc);
}

#[test]
fn a_style_holding_samples_echoes_them_and_a_tile_without_them_writes_no_key() {
    let native = json!({"kind": "rgb16", "data": base64(&le16(&OFF_GRID.concat()))});
    let tile = json!({"name": "E", "width": 2, "height": 2, "rgba": base64(&[9; 16])});
    let mut with = tile.clone();
    with["native"] = native.clone();
    let doc = rgb16_doc(2, 2, vec![[65535; 4]; 4]);
    let styled = set_style(doc, 0, &overlay(&with)).unwrap();
    let stored = unsafe { &*styled }
        .layer(0)
        .unwrap()
        .style()
        .unwrap()
        .clone();
    assert_eq!(stored_native(&stored), native);
    let plain = set_style(doc, 0, &overlay(&tile)).unwrap();
    let stored = unsafe { &*plain }
        .layer(0)
        .unwrap()
        .style()
        .unwrap()
        .clone();
    assert!(!stored.to_json().contains("native"), "{}", stored.to_json());
    for d in [doc, styled, plain] {
        free(d);
    }
}

// ------------------------------------------------------------ word 115 --

const WORD: [u8; 4] = 115u32.to_le_bytes();
const WORD_REFUSAL: &str = "native pattern samples require DRDC 115";
const INVALID_WORD: &str = "invalid native pattern samples word";

fn native_overlay() -> Value {
    overlay(
        &json!({"name": "W", "width": 2, "height": 2, "rgba": base64(&[7; 16]),
        "native": {"kind": "rgb16", "data": base64(&le16(&OFF_GRID.concat()))}}),
    )
}

/// `bytes` with the four at 8.. removed (the first conditional word).
fn stripped(bytes: &[u8]) -> Vec<u8> {
    [&bytes[..8], &bytes[12..]].concat()
}

/// `bytes` with word 115 put at 8.
fn claimed(bytes: &[u8]) -> Vec<u8> {
    [&bytes[..8], &WORD[..], &bytes[8..]].concat()
}

#[test]
fn the_typed_word_115_is_written_read_and_checked_both_ways() {
    let dir = TempDir::new().unwrap();
    let doc = rgb16_doc(2, 2, vec![[65535; 4]; 4]);
    let styled = set_style(doc, 0, &native_overlay()).unwrap();
    let path = dir.path().join("samples.rz");
    let p = path.to_str().unwrap();
    unsafe { &*styled }.save_native(p, LIMIT, 0, None).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes[8..12], WORD, "115 right after the identity wrapper");
    let back = PrecisionDocument::open_native(p, LIMIT, 0, None).unwrap();
    assert_eq!(
        back.layer(0).unwrap().style().unwrap().to_json(),
        unsafe { &*styled }
            .layer(0)
            .unwrap()
            .style()
            .unwrap()
            .to_json()
    );
    std::fs::write(&path, stripped(&bytes)).unwrap();
    let refusal = PrecisionDocument::open_native(p, LIMIT, 0, None)
        .err()
        .expect("a refusal");
    assert!(refusal.contains(WORD_REFUSAL), "{refusal}");

    unsafe { &*doc }.save_native(p, LIMIT, 0, None).unwrap();
    let plain = std::fs::read(&path).unwrap();
    assert_ne!(plain[8..12], WORD, "no samples, no word");
    std::fs::write(&path, claimed(&plain)).unwrap();
    let refusal = PrecisionDocument::open_native(p, LIMIT, 0, None)
        .err()
        .expect("a refusal");
    assert!(refusal.contains(INVALID_WORD), "{refusal}");
    free(doc);
    free(styled);
}

#[test]
fn the_byte_word_115_is_written_read_and_checked_both_ways() {
    let dir = TempDir::new().unwrap();
    let doc = DrDocument::from_pixels(solid(4, 3, [255, 255, 255, 255]));
    let style = LayerStyle::from_json(&native_overlay().to_string()).unwrap();
    let styled = doc.set_layer_style(0, Some(Arc::new(style))).unwrap();
    let path = dir.path().join("samples.rz");
    let p = path.to_str().unwrap();
    styled.save_native(p).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes[8..12], WORD, "115 first inside 45");
    let back = DrDocument::open(p).unwrap();
    assert_eq!(
        back.layers[0].style.as_ref().unwrap().to_json(),
        styled.layers[0].style.as_ref().unwrap().to_json()
    );
    std::fs::write(&path, stripped(&bytes)).unwrap();
    let refusal = DrDocument::open(p).err().expect("a refusal");
    assert!(refusal.contains(WORD_REFUSAL), "{refusal}");

    doc.save_native(p).unwrap();
    let plain = std::fs::read(&path).unwrap();
    std::fs::write(&path, claimed(&plain)).unwrap();
    let refusal = DrDocument::open(p).err().expect("a refusal");
    assert!(refusal.contains(INVALID_WORD), "{refusal}");
}

// --------------------------------------------- a 512² rgb32 tile's caps --

#[test]
fn a_512_square_rgb32_tile_is_set_filled_saved_and_reopened() {
    let side = 512u32;
    let texels = (side * side) as usize;
    let values: Vec<f32> = (0..texels)
        .flat_map(|i| [1.0 + (i % 997) as f32 / 64.0, 0.5, (i % 13) as f32, 1.0])
        .collect();
    let tile = json!({"name": "Big", "width": side, "height": side,
        "rgba": base64(&vec![128; texels * 4]),
        "native": {"kind": "rgb32", "data": base64(&le32(&values))}});
    let request_bytes = overlay(&tile).to_string().len();
    assert!(
        request_bytes > 2 * 1024 * 1024 + 64 * 1024,
        "{request_bytes}"
    );
    let doc = float_rgb_doc(8, 8, vec![[0.0, 0.0, 0.0, 1.0]; 64]);
    let styled = set_style(doc, 0, &overlay(&tile)).unwrap();
    let rendered = unsafe { &*styled }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba32()
        .unwrap();
    for (x, y) in [(0u32, 0u32), (7, 3), (5, 7)] {
        let at = (y * side + x) as usize * 4;
        let got = rendered[(y * 8 + x) as usize];
        assert!(
            (got[0] - values[at]).abs() <= 1e-5 * values[at],
            "({x}, {y})"
        );
    }
    let filled = fill(doc, &tile, false);
    assert_eq!(float_samples(filled)[9][0], values[(side as usize + 1) * 4]);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("big.rz");
    let p = path.to_str().unwrap();
    unsafe { &*styled }.save_native(p, LIMIT, 0, None).unwrap();
    let back = PrecisionDocument::open_native(p, LIMIT, 0, None).unwrap();
    assert_eq!(
        back.layer(0).unwrap().style().unwrap().to_json(),
        unsafe { &*styled }
            .layer(0)
            .unwrap()
            .style()
            .unwrap()
            .to_json()
    );
    for d in [doc, styled, filled] {
        free(d);
    }
}

#[test]
fn a_request_past_2_mib_without_samples_still_refuses() {
    let side = 512u32;
    let rgba = base64(&vec![77; (side * side * 4) as usize]);
    let tile = json!({"name": "A", "width": side, "height": side, "rgba": rgba});
    let style = json!({"effects": [
        {"type": "pattern_overlay", "pattern": tile},
        {"type": "stroke", "fill_type": "pattern", "pattern_fill": {"pattern": tile}},
    ]});
    let doc = rgb16_doc(4, 4, vec![[65535; 4]; 16]);
    let refusal = set_style(doc, 0, &style).unwrap_err();
    assert!(
        refusal.contains("request exceeds the supported byte limit"),
        "{refusal}"
    );
    free(doc);
}

// ------------------------------------------------------ Convert to Profile --

#[test]
fn convert_to_profile_maps_a_cmyk16_tile_through_the_documents_own_leg() {
    let (a, b) = (press_profile("Press A"), press_profile_b("Press B"));
    let inks: [[u16; 4]; 4] = [
        [0, 0, 0, 65535],
        [40000, 10000, 0, 2000],
        [0, 52000, 30000, 0],
        [12000, 12000, 12000, 30000],
    ];
    let samples: Vec<u16> = inks
        .iter()
        .flat_map(|i| [i[0], i[1], i[2], i[3], 65535])
        .collect();
    let tile = json!({"name": "Ink", "width": 2, "height": 2, "rgba": base64(&[0; 16]),
        "native": {"kind": "cmyk16", "data": base64(&le16(&samples))}});
    let doc = cmyk_doc(2, 2, &a, 16, [0.0, 0.0, 0.0, 0.0, 1.0]);
    let styled = set_style(doc, 0, &overlay(&tile)).unwrap();
    let converted = profile_edit(styled, &b);
    let style = unsafe { &*converted }
        .layer(0)
        .unwrap()
        .style()
        .unwrap()
        .clone();
    let native = stored_native(&style);
    assert_eq!(native["kind"], "cmyk16");
    let mapped = u16s(&unbase64(native["data"].as_str().unwrap()));
    let mut moved = false;
    for (i, ink) in inks.iter().enumerate() {
        // The oracle: a pixel of this ink in a document of profile A, through
        // the same Convert to Profile.
        let unit = ink.map(|c| f64::from(c) / 65535.0);
        let one = cmyk_doc(1, 1, &a, 16, [unit[0], unit[1], unit[2], unit[3], 1.0]);
        let one_b = profile_edit(one, &b);
        let want = cmyk_samples(one_b)[0];
        for c in 0..4 {
            assert!(
                mapped[i * 5 + c].abs_diff(want[c]) <= 1,
                "texel {i} plate {c}: {} vs {}",
                mapped[i * 5 + c],
                want[c]
            );
        }
        assert_eq!(mapped[i * 5 + 4], 65535, "texel {i}: alpha kept");
        moved |= mapped[i * 5..i * 5 + 4] != ink[..];
        free(one);
        free(one_b);
    }
    assert!(moved, "the two profiles differ, so some ink moves");
    for d in [doc, styled, converted] {
        free(d);
    }
}

fn profile_edit(doc: *const PrecisionDocument, icc: &[u8]) -> *mut PrecisionDocument {
    let request = CString::new(r#"{"op":"convert"}"#).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_profile_edit(
            doc,
            icc.as_ptr(),
            icc.len(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "convert: {}", take_err_string(err));
    out
}

// ---------------------------------------------------------------- turns --

#[test]
fn a_linked_tiles_samples_take_its_turn() {
    // Six distinct texels; each sample is its raster code × 256 + 7, so after
    // the turn every sample must still be that function of the texel the
    // turned raster holds at the same index.
    let rgba: Vec<u8> = (0..6u8)
        .flat_map(|i| [10 + 30 * i, 200 - 20 * i, 5 * i, 255 - i])
        .collect();
    let samples: Vec<u16> = rgba.iter().map(|&c| u16::from(c) * 256 + 7).collect();
    let style = overlay(
        &json!({"name": "T", "width": 3, "height": 2, "rgba": base64(&rgba),
        "native": {"kind": "rgb16", "data": base64(&le16(&samples))}}),
    );
    let doc = DrDocument::from_pixels(solid(5, 4, [255, 255, 255, 255]));
    let styled = doc
        .set_layer_style(
            0,
            Some(Arc::new(LayerStyle::from_json(&style.to_string()).unwrap())),
        )
        .unwrap();
    for (what, turned) in [
        ("rotate90", styled.rotate90()),
        ("rotate180", styled.rotate180()),
        ("flip_horizontal", styled.flip_horizontal()),
        ("flip_vertical", styled.flip_vertical()),
    ] {
        let turned = turned.expect(what);
        let style = turned.layers[0].style.as_ref().unwrap();
        let tile = style
            .effects
            .iter()
            .find_map(|e| match e {
                Effect::PatternOverlay(p) => Some(Arc::clone(&p.fill.pattern.tile)),
                _ => None,
            })
            .unwrap();
        assert_ne!(*tile.rgba, rgba[..], "{what}: the raster turned");
        let Some(TileSamples::Rgb16(got)) = tile.samples.as_deref() else {
            panic!("{what}: the samples were dropped");
        };
        let want: Vec<u16> = tile.rgba.iter().map(|&c| u16::from(c) * 256 + 7).collect();
        assert_eq!(*got, want, "{what}");
    }
}
