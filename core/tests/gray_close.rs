//! #1, the close of Grayscale mode: the tools no phase named, through the
//! FFI. Each lifted tool is the RGB kernel on R = G = B, so its oracle is the
//! RGB TWIN (`gray_ops::twin`) given the same input, its result taken through
//! Image ▸ Mode ▸ Grayscale where colour came in from outside (a placed
//! image, a Generative Layer's picture).
//!
//! * Place folds the rendered pixels and keeps no retained source: the layer
//!   equals the twin's place + rasterize converted to Gray, and its plan is
//!   the twin's plus the fold's image (8 bytes a pixel, 16 at 32 bits).
//! * RGB → Gray never refuses a Generative Layer: from the twin its
//!   variations stay live, from sRGB they are detached.
//! * Scalar morphology on a mask, a layer export with profile document, the
//!   32-bit tone map and Image Size's resolution equal the twin's or keep
//!   the mode.
//! * A request RGB rejects too gets RGB's own error; an RGB-only family and a
//!   scalar read of the Gray layer refuse for good by name.
//!
//! # Bounds, declared before the first run
//!
//! Every comparison is exact.

use std::ffi::{c_char, CStr, CString};
use std::io::Cursor;
use std::ptr;

use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_precision_doc_export_layer_encode,
};
use doctored_core::ffi_float::{dr_precision_doc_display_rgba8, dr_precision_doc_set_tone_map};
use doctored_core::ffi_gen_fill::dr_precision_doc_gen_fill_apply;
use doctored_core::ffi_gen_variations::dr_precision_doc_gen_fill_select_variation;
use doctored_core::ffi_gray::dr_precision_doc_convert_to_gray;
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_render, dr_precision_image_free,
};
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::ffi_precision_metadata::dr_precision_doc_set_resolution;
use doctored_core::ffi_retained_source::{
    dr_precision_doc_has_retained_source, dr_precision_doc_source_edit,
    dr_precision_doc_source_plan,
};
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_capability::require_gray;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_ops::{assert_neutral, edit, free, gray_doc, layer, result, twin, LIMIT};

const W: u32 = 9;
const H: u32 = 6;

/// A coloured image: R and G always differ, every seventh pixel at partial
/// alpha, so no pixel is neutral.
fn coloured(w: u32, h: u32) -> PrecisionImage {
    let pixels = (0..w * h)
        .map(|i| {
            let r = (i * 7919 + 211) % 60_000 + 100;
            let g = (r + 4000) % 60_000 + 100;
            let b = (i * 1299 + 501) % 61_000 + 300;
            let a = if i % 7 == 3 { 41_000 } else { 65_535 };
            [r as u16, g as u16, b as u16, a]
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap()
}

fn boxed(doc: PrecisionDocument) -> *mut PrecisionDocument {
    Box::into_raw(Box::new(doc))
}

/// A plain sRGB source document of the coloured image at `depth`.
fn source(w: u32, h: u32, depth: u32) -> *mut PrecisionDocument {
    let doc = boxed(
        PrecisionDocument::from_image(coloured(w, h), Metadata::default(), Resolution::default())
            .unwrap(),
    );
    if depth != 32 {
        return doc;
    }
    let mut err = ptr::null_mut();
    let float = unsafe { dr_precision_doc_promote_to_float(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!float.is_null(), "{}", take_err_string(err));
    free(doc);
    float
}

fn owned(text: *mut c_char) -> String {
    let out = unsafe { CStr::from_ptr(text) }.to_str().unwrap().to_owned();
    unsafe { dr_string_free(text) };
    out
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_descriptor(doc, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&owned(text)).unwrap()
}

/// `doc` through Image ▸ Mode ▸ Grayscale into Gray Gamma 2.2 at `depth`
/// (`gray_ops::gray_doc`'s profile).
fn to_gray(doc: *const PrecisionDocument, depth: u32) -> Result<*mut PrecisionDocument, String> {
    let options = if depth == 32 {
        r#"{"space":"gray_gamma_22"}"#.to_string()
    } else {
        format!(r#"{{"space":"gray_gamma_22","depth":{depth}}}"#)
    };
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_gray(
            doc,
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err).map(|d| d.expect("the mode changed"))
}

fn source_plan(
    doc: *const PrecisionDocument,
    input: *const PrecisionDocument,
    request: &Value,
) -> Value {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_source_plan(doc, input, text.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    serde_json::from_str(&owned(out)).unwrap()
}

fn source_edit(
    doc: *const PrecisionDocument,
    input: *const PrecisionDocument,
    request: &Value,
) -> Result<Option<*mut PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_source_edit(doc, input, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    result(out, err)
}

#[test]
fn place_folds_the_rendered_pixels_and_keeps_no_source() {
    let place = json!({"op":"place","source_id":"close-1","name":"Placed","x":1,"y":0});
    for depth in [8, 16, 32] {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        let input = source(5, 4, depth);
        let ours_plan = source_plan(gray, input, &place);
        let theirs_plan = source_plan(rgb, input, &place);
        let per_pixel = if depth == 32 { 16 } else { 8 };
        assert_eq!(
            ours_plan["scratch_bytes"].as_u64().unwrap(),
            theirs_plan["scratch_bytes"].as_u64().unwrap() + per_pixel * 5 * 4,
            "{depth}-bit plan"
        );
        let ours = source_edit(gray, input, &place).unwrap().unwrap();
        let index = unsafe { &*ours }.layer_count() - 1;
        assert!(
            !unsafe { dr_precision_doc_has_retained_source(ours, index) },
            "{depth}-bit: no retained source"
        );
        // The host's chained rasterize changes nothing.
        let rasterize = json!({"op":"rasterize","layer":index});
        assert!(source_edit(ours, ptr::null(), &rasterize)
            .unwrap()
            .is_none());
        let placed = source_edit(rgb, input, &place).unwrap().unwrap();
        let theirs = source_edit(placed, ptr::null(), &rasterize)
            .unwrap()
            .unwrap();
        let oracle = to_gray(theirs, depth).unwrap();
        let mine = layer(ours, index, depth);
        assert_neutral(&mine, &format!("{depth}-bit place"));
        assert_eq!(mine, layer(oracle, index, depth), "{depth}-bit place");
        for doc in [oracle, theirs, placed, ours, input, rgb, gray] {
            free(doc);
        }
    }
    assert!(require_gray("source_place", "document").is_ok());
}

type Picture = (Vec<u8>, (u32, u32));

fn solid(size: (u32, u32), rgb: [u8; 3]) -> Picture {
    let pixels = (0..size.0 * size.1)
        .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
        .collect();
    (pixels, size)
}

const NATIVE: (u32, u32) = (64, 64);

/// A Generative Fill layer over the left two thirds of the canvas from two
/// solid pictures (`gray_neural`'s); answers the document and the new
/// layer's index.
fn gen_fill(doc: *const PrecisionDocument) -> (*mut PrecisionDocument, usize) {
    let coverage: Vec<u16> = (0..W * H)
        .map(|i| if i % W < 2 * W / 3 { 65_535 } else { 0 })
        .collect();
    let pictures = [solid(NATIVE, [200, 30, 40]), solid(NATIVE, [30, 60, 220])];
    let mut bytes = Vec::new();
    let mut sizes = Vec::new();
    for (pixels, size) in &pictures {
        bytes.extend_from_slice(pixels);
        sizes.extend([size.0, size.1]);
    }
    let name = CString::new("a red boat").unwrap();
    let tag = CString::new(
        r#"{"type":"generated","version":1,"feature":"generative_fill","provider":"gemini","model":"gemini-nano-banana-2.1","prompt":"a red boat"}"#,
    )
    .unwrap();
    let mut index = usize::MAX;
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_gen_fill_apply(
            doc,
            0,
            coverage.as_ptr(),
            coverage.len(),
            NATIVE.0,
            NATIVE.1,
            bytes.as_ptr(),
            bytes.len(),
            sizes.as_ptr(),
            sizes.len(),
            name.as_ptr(),
            tag.as_ptr(),
            &mut index,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    (result(out, err).unwrap().unwrap(), index)
}

fn select_variation(
    doc: *const PrecisionDocument,
    layer: usize,
    k: u32,
) -> Result<*mut PrecisionDocument, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_gen_fill_select_variation(doc, layer, k, LIMIT, 0, ptr::null(), &mut err)
    };
    result(out, err).map(|d| d.expect("the variation changed"))
}

/// Layer `index` of `doc` alone as a one-layer document, its variations
/// detached.
fn layer_alone(doc: *const PrecisionDocument, index: usize) -> *mut PrecisionDocument {
    let image = unsafe { &*doc }.layer_source(index).unwrap().clone();
    boxed(PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap())
}

#[test]
fn a_generative_layer_from_the_twin_converts_live() {
    for depth in [8, 16] {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        let (theirs, index) = gen_fill(rgb);
        let ours = to_gray(theirs, depth).expect("no flatten needed");
        let what = format!("{depth}-bit twin");
        let alone = layer_alone(theirs, index);
        let oracle = to_gray(alone, depth).unwrap();
        assert_eq!(layer(ours, index, depth), layer(oracle, 0, depth), "{what}");
        let ours2 = select_variation(ours, index, 1).expect("the variations stay live");
        let theirs2 = select_variation(theirs, index, 1).unwrap();
        let oracle2 = to_gray(theirs2, depth).unwrap();
        let mine = layer(ours2, index, depth);
        assert_neutral(&mine, &what);
        assert_eq!(mine, layer(oracle2, index, depth), "{what}");
        for doc in [
            oracle2, theirs2, ours2, oracle, alone, ours, theirs, rgb, gray,
        ] {
            free(doc);
        }
    }
}

#[test]
fn a_generative_layer_from_srgb_converts_with_its_variations_detached() {
    for depth in [8, 16] {
        let rgb = boxed(
            PrecisionDocument::from_image(
                coloured(W, H),
                Metadata::default(),
                Resolution::default(),
            )
            .unwrap(),
        );
        let (theirs, index) = gen_fill(rgb);
        let ours = to_gray(theirs, depth).expect("no flatten needed");
        let alone = layer_alone(theirs, index);
        let oracle = to_gray(alone, depth).unwrap();
        let mine = layer(ours, index, depth);
        assert_neutral(&mine, &format!("{depth}-bit sRGB"));
        assert_eq!(mine, layer(oracle, 0, depth), "{depth}-bit sRGB");
        assert_eq!(
            select_variation(ours, index, 1).unwrap_err(),
            format!("generative layer {index} holds no variations")
        );
        for doc in [oracle, alone, ours, theirs, rgb] {
            free(doc);
        }
    }
}

#[test]
fn a_hue_layer_effect_blocks_without_yet() {
    let rgb = boxed(
        PrecisionDocument::from_image(coloured(W, H), Metadata::default(), Resolution::default())
            .unwrap(),
    );
    let styled = edit(
        rgb,
        &json!({"op":"layer_style","version":1,"layer":0,"style":{"effects":[{"type":"drop_shadow","blend":"hue","distance":3,"size":2}]}}),
        &[],
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        to_gray(styled, 16).unwrap_err(),
        "“Background” is a layer with a layer effect in the Hue blend mode, which a Grayscale document cannot hold; flatten the image to convert it"
    );
    free(styled);
    free(rgb);
}

#[test]
fn scalar_grow_of_a_gray_mask_equals_the_twin() {
    let mask = json!({"op":"set_mask","layer":0,"width":W,"height":H,"x":0,"y":0,
        "linked":true});
    let spot: Vec<u16> = (0..W * H)
        .map(|i| if i == W * 2 + 4 { 65_535 } else { 0 })
        .collect();
    let grow = json!({"op":"scalar_edit","target":"mask","index":0,"selection":false,
        "operation":{"kind":"morphology","operation":{"kind":"grow","radius":1}}});
    for depth in [8, 16] {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        let ours = edit(gray, &mask, &spot).unwrap().unwrap();
        let theirs = edit(rgb, &mask, &spot).unwrap().unwrap();
        let ours2 = edit(ours, &grow, &[]).unwrap().unwrap();
        let theirs2 = edit(theirs, &grow, &[]).unwrap().unwrap();
        let a = unsafe { &*ours2 }.layer(0).unwrap().mask().unwrap().plane();
        let b = unsafe { &*theirs2 }
            .layer(0)
            .unwrap()
            .mask()
            .unwrap()
            .plane();
        assert_eq!(a.samples(), b.samples(), "{depth}");
        assert_ne!(
            a.samples(),
            unsafe { &*ours }
                .layer(0)
                .unwrap()
                .mask()
                .unwrap()
                .plane()
                .samples(),
            "{depth}: the mask grew"
        );
        assert!(unsafe { &*ours2 }.is_gray());
        for doc in [theirs2, ours2, theirs, ours, rgb, gray] {
            free(doc);
        }
    }
}

/// A layer export's PNG: (colour type, bit depth, the decoded big-endian
/// u16 samples).
fn export_layer_png(doc: *const PrecisionDocument) -> (png::ColorType, png::BitDepth, Vec<u16>) {
    let request = CString::new(r#"{"format":"png","bit_depth":16,"profile":"document"}"#).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out: *mut ExportResult = unsafe {
        dr_precision_doc_export_layer_encode(
            doc,
            0,
            false,
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let mut length = 0usize;
    let pointer = unsafe { dr_export_result_bytes(out, &mut length) };
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec();
    unsafe { dr_export_result_free(out) };
    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    let mut buffer = vec![0u8; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buffer).unwrap();
    let samples = buffer[..info.buffer_size()]
        .chunks_exact(2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .collect();
    (info.color_type, info.bit_depth, samples)
}

#[test]
fn a_layer_export_keeps_one_channel_equal_to_the_twin_red() {
    let gray = gray_doc(W, H, 16);
    let rgb = twin(gray);
    let (ours_type, ours_depth, ours) = export_layer_png(gray);
    let (theirs_type, theirs_depth, theirs) = export_layer_png(rgb);
    assert_eq!(ours_type, png::ColorType::GrayscaleAlpha);
    assert_eq!(theirs_type, png::ColorType::Rgba);
    assert_eq!(
        (ours_depth, theirs_depth),
        (png::BitDepth::Sixteen, png::BitDepth::Sixteen)
    );
    let gray_alpha: Vec<[u16; 2]> = ours.chunks_exact(2).map(|p| [p[0], p[1]]).collect();
    let red_alpha: Vec<[u16; 2]> = theirs.chunks_exact(4).map(|p| [p[0], p[3]]).collect();
    assert_eq!(gray_alpha, red_alpha);
    free(rgb);
    free(gray);
}

/// The display RGBA8 of `doc`'s render through its own tone map.
fn display(doc: *const PrecisionDocument) -> Vec<u8> {
    let mut err = ptr::null_mut();
    let render = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!render.is_null(), "{}", take_err_string(err));
    let mut err = ptr::null_mut();
    let image =
        unsafe { dr_precision_doc_display_rgba8(doc, render, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "{}", take_err_string(err));
    let bytes = unsafe {
        let (w, h) = (dr_image_width(image), dr_image_height(image));
        std::slice::from_raw_parts(dr_image_pixels_rgba(image), w as usize * h as usize * 4)
            .to_vec()
    };
    unsafe {
        dr_image_free(image);
        dr_precision_image_free(render);
    }
    bytes
}

fn tone_mapped(doc: *const PrecisionDocument, json: &str) -> *mut PrecisionDocument {
    let request = CString::new(json).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_tone_map(doc, request.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{json}: {}", take_err_string(err));
    out
}

#[test]
fn a_32_bit_tone_map_renders_as_on_the_twin() {
    let gray = gray_doc(W, H, 32);
    let rgb = twin(gray);
    let map = r#"{"operator":"reinhard","white":4}"#;
    let ours = tone_mapped(gray, map);
    let theirs = tone_mapped(rgb, map);
    assert!(unsafe { &*ours }.is_gray());
    let shown = display(ours);
    assert_eq!(shown, display(theirs));
    assert_ne!(shown, display(gray), "the map changed the view");
    for doc in [theirs, ours, rgb, gray] {
        free(doc);
    }
}

#[test]
fn image_size_resolution_keeps_the_mode() {
    let gray = gray_doc(W, H, 16);
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_resolution(gray, 300.0, 300.0, &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let value = descriptor(out);
    assert_eq!(value["color_mode"], "gray");
    assert_eq!(value["resolution"]["x_ppi"], 300.0);
    free(out);
    free(gray);
}

fn selected_err(doc: *const PrecisionDocument, request: &Value) -> String {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err).unwrap_err()
}

#[test]
fn a_request_rgb_rejects_gets_the_rgb_error() {
    let gray = gray_doc(W, H, 16);
    let rgb = twin(gray);
    for request in [
        json!({"op":"bogus"}),
        json!({"op":"layer_style","version":1,"layer":0,"style":3}),
    ] {
        let theirs = edit(rgb, &request, &[]).unwrap_err();
        assert_eq!(edit(gray, &request, &[]).unwrap_err(), theirs, "{request}");
        assert!(!theirs.contains("Grayscale"), "{request}: {theirs}");
    }
    let adjustment = json!({"layer":0,"op":"adjustment","description":
        json!({"type":"adjust","evaluation":"precision_v1","op":"bogus","params":{}}).to_string()});
    let theirs = selected_err(rgb, &adjustment);
    assert_eq!(selected_err(gray, &adjustment), theirs);
    assert_eq!(theirs, "unsupported high-depth adjustment bogus");
    // An RGB-only family and a scalar read of the layer refuse for good.
    assert_eq!(
        require_gray("color_convert", "document").unwrap_err(),
        "color_convert is unavailable in a Grayscale document: Doctored offers it on RGB documents only."
    );
    assert_eq!(
        edit(gray, &json!({"op":"read_plane"}), &[]).unwrap_err(),
        "read_plane of the Gray layer is unavailable in a Grayscale document: its one colour channel, Gray, is the layer, so a scalar read takes target mask or channel."
    );
    free(rgb);
    free(gray);
}
