//! #547, the close of Lab Color mode: the tools and requests no phase names,
//! through the FFI. Each oracle is an RGB document given the same input, its
//! result taken through Image ▸ Mode ▸ Lab Color
//! (`dr_precision_doc_convert_to_lab`) where colour comes in.
//!
//! * (a) Import Channel on a Lab document writes the samples given, as on an
//!   sRGB document.
//! * (b) a full-coverage legacy `rasterize` on a 16-bit Lab document equals
//!   the same op on an sRGB document converted to Lab.
//! * (c) Place on a Lab document equals Place on a ProPhoto RGB document
//!   converted to Lab, keeps no retained source (a following `rasterize`
//!   changes nothing), and its plan is ProPhoto's plus the converted image (8
//!   bytes a pixel) and the ProPhoto → Lab pipeline's scratch.
//! * (d) an sRGB document with a Generative Fill layer converts to Lab with
//!   no flatten; the layer equals the conversion of its raster alone, and its
//!   variations are detached.
//! * (e) the placed and warped blockers name no phase and say no "yet".
//! * (f) an unknown op, a malformed style, an unknown selection kind and an
//!   unknown adjustment get the sRGB document's own error; a scalar read of
//!   the layer names the targets a Lab document takes.
//! * (g) Image Size's resolution keeps the mode.
//! * (h) a Lab layer's export equals the export of a Lab document holding
//!   that layer alone.
//!
//! # Bounds, declared before the first run
//!
//! Every comparison is exact.

use std::ffi::{c_char, CStr, CString};
use std::io::Cursor;
use std::ptr;

use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_precision_doc_export_encode,
    dr_precision_doc_export_layer_encode,
};
use doctored_core::ffi_gen_fill::dr_precision_doc_gen_fill_apply;
use doctored_core::ffi_gen_variations::dr_precision_doc_gen_fill_select_variation;
use doctored_core::ffi_precision::dr_precision_doc_descriptor;
use doctored_core::ffi_precision_metadata::dr_precision_doc_set_resolution;
use doctored_core::ffi_retained_source::{
    dr_precision_doc_has_retained_source, dr_precision_doc_source_edit,
    dr_precision_doc_source_plan,
};
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::ffi_warp::dr_precision_doc_warp_edit;
use doctored_core::icc::IccProfile;
use doctored_core::icc_intent::{ColorPipeline, RenderingIntent};
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_ops::{edit, free, result, LIMIT};
use crate::lab_targets::{codes, pattern, srgb_doc, to_lab, H, W};

fn boxed(doc: PrecisionDocument) -> *mut PrecisionDocument {
    Box::into_raw(Box::new(doc))
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

/// A coloured `profile` image: the channels all differ, every seventh pixel
/// at partial alpha.
fn coloured(w: u32, h: u32, profile: std::sync::Arc<IccProfile>) -> PrecisionImage {
    let pixels = (0..w * h)
        .map(|i| {
            let r = (i * 7919 + 211) % 60_000 + 100;
            let g = (r + 4000) % 60_000 + 100;
            let b = (i * 1299 + 501) % 61_000 + 300;
            let a = if i % 7 == 3 { 41_000 } else { 65_535 };
            [r as u16, g as u16, b as u16, a]
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, profile, None).unwrap()
}

/// A W × H document in `profile` of the given canvas-sized layers.
fn doc_of(
    profile: std::sync::Arc<IccProfile>,
    images: Vec<PrecisionImage>,
) -> *mut PrecisionDocument {
    let layers = images
        .into_iter()
        .enumerate()
        .map(|(i, image)| {
            PrecisionDocumentLayer::raster(&format!("Layer {i}"), image, (0, 0), 1.0, true).unwrap()
        })
        .collect();
    boxed(
        PrecisionDocument::from_parts(
            W,
            H,
            profile,
            Metadata::default(),
            Resolution::default(),
            layers,
        )
        .unwrap(),
    )
}

/// The pattern as a Lab document at `depth`.
fn lab_doc(depth: u32) -> *mut PrecisionDocument {
    let rgb = srgb_doc(pattern());
    let lab = to_lab(rgb, &format!(r#"{{"depth":{depth}}}"#)).unwrap();
    free(rgb);
    lab
}

fn depth(depth: u32) -> String {
    format!(r#"{{"depth":{depth}}}"#)
}

#[test]
fn import_channel_writes_its_samples_as_on_rgb() {
    let lab = lab_doc(16);
    let rgb = srgb_doc(pattern());
    let samples: Vec<u16> = (0..W * H).map(|i| ((i * 2741) % 65_536) as u16).collect();
    let request = json!({"op":"import_channel","name":"Imported","width":W,"height":H});
    let ours = edit(lab, &request, &samples).unwrap().unwrap();
    let theirs = edit(rgb, &request, &samples).unwrap().unwrap();
    let read = |doc: *const PrecisionDocument| {
        let channels = unsafe { &*doc }.channels();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name(), "Imported");
        channels[0].plane().samples().to_vec()
    };
    assert_eq!(read(ours), samples);
    assert_eq!(read(ours), read(theirs));
    assert_eq!(descriptor(ours)["color_mode"], "lab");
    for doc in [theirs, ours, rgb, lab] {
        free(doc);
    }
}

#[test]
fn a_full_coverage_rasterize_equals_srgb_converted_to_lab() {
    let lab = lab_doc(16);
    let rgb = srgb_doc(pattern());
    let coverage = vec![65_535u16; (W * H) as usize];
    for color in [
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0, 1.0],
    ] {
        let request = json!({"op":"rasterize","name":"Raster","width":W,"height":H,"x":0,"y":0,
            "colors":[color],"color_space":"srgb"});
        let ours = edit(lab, &request, &coverage).unwrap().unwrap();
        let theirs = edit(rgb, &request, &coverage).unwrap().unwrap();
        let oracle = to_lab(theirs, &depth(16)).unwrap();
        assert_eq!(codes(ours, 1), codes(oracle, 1), "{color:?}");
        assert_eq!(descriptor(ours)["color_mode"], "lab");
        for doc in [oracle, theirs, ours] {
            free(doc);
        }
    }
    free(rgb);
    free(lab);
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

/// A plain sRGB 5 × 4 picture to place.
fn picture() -> *mut PrecisionDocument {
    boxed(
        PrecisionDocument::from_image(
            coloured(5, 4, IccProfile::srgb()),
            Metadata::default(),
            Resolution::default(),
        )
        .unwrap(),
    )
}

#[test]
fn place_renders_through_prophoto_and_keeps_no_source() {
    let place = json!({"op":"place","source_id":"lab-close-1","name":"Placed","x":3,"y":2});
    let prophoto = IccProfile::prophoto_rgb();
    for bits in [8, 16] {
        let lab = lab_doc(bits);
        let pro = doc_of(prophoto.clone(), vec![coloured(W, H, prophoto.clone())]);
        let input = picture();
        let ours_plan = source_plan(lab, input, &place);
        let theirs_plan = source_plan(pro, input, &place);
        let pipeline = ColorPipeline::new(
            &prophoto,
            unsafe { &*lab }.profile(),
            RenderingIntent::RelativeColorimetric,
            false,
        )
        .unwrap()
        .scratch_bytes();
        assert_eq!(
            ours_plan["scratch_bytes"].as_u64().unwrap(),
            theirs_plan["scratch_bytes"].as_u64().unwrap() + 8 * 5 * 4 + pipeline,
            "{bits}-bit plan"
        );
        let ours = source_edit(lab, input, &place).unwrap().unwrap();
        let index = unsafe { &*ours }.layer_count() - 1;
        assert!(
            !unsafe { dr_precision_doc_has_retained_source(ours, index) },
            "{bits}-bit: no retained source"
        );
        let rasterize = json!({"op":"rasterize","layer":index});
        assert!(source_edit(ours, ptr::null(), &rasterize)
            .unwrap()
            .is_none());
        let placed = source_edit(pro, input, &place).unwrap().unwrap();
        let theirs = source_edit(placed, ptr::null(), &rasterize)
            .unwrap()
            .unwrap();
        let oracle = to_lab(theirs, &depth(bits)).unwrap();
        assert_eq!(codes(ours, index), codes(oracle, index), "{bits}-bit place");
        assert_eq!(
            unsafe { &*ours }.layer(index).unwrap().offset(),
            (3, 2),
            "{bits}-bit offset"
        );
        assert_eq!(descriptor(ours)["color_mode"], "lab");
        for doc in [oracle, theirs, placed, ours, input, pro, lab] {
            free(doc);
        }
    }
    let lab = lab_doc(16);
    let families = descriptor(lab)["supported_operations"].clone();
    assert!(families
        .as_array()
        .unwrap()
        .contains(&"source_place".into()));
    free(lab);
}

/// An asset already in the twin (Load Files into Stack's Lab clip) holds Lab
/// codes: it places unchanged, as before the close.
#[test]
fn a_twin_asset_places_its_codes_unchanged() {
    let lab = lab_doc(16);
    let asset = lab_doc(16);
    let place = json!({"op":"place","source_id":"lab-close-3","name":"Twin","x":0,"y":0});
    let ours = source_edit(lab, asset, &place).unwrap().unwrap();
    let index = unsafe { &*ours }.layer_count() - 1;
    assert_eq!(codes(ours, index), codes(asset, 0));
    for doc in [ours, asset, lab] {
        free(doc);
    }
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
/// solid pictures (`gray_close`'s); answers the document and the layer.
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

#[test]
fn a_generative_layer_converts_to_lab_with_its_variations_detached() {
    for bits in [8, 16] {
        let rgb = srgb_doc(pattern());
        let (theirs, index) = gen_fill(rgb);
        let ours = to_lab(theirs, &depth(bits)).expect("no flatten needed");
        let image = unsafe { &*theirs }.layer_source(index).unwrap().clone();
        let alone = boxed(
            PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
                .unwrap(),
        );
        let oracle = to_lab(alone, &depth(bits)).unwrap();
        assert_eq!(codes(ours, index), codes(oracle, 0), "{bits}-bit layer");
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_gen_fill_select_variation(
                ours,
                index,
                1,
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert_eq!(
            result(out, err).unwrap_err(),
            format!("generative layer {index} holds no variations"),
            "{bits}-bit"
        );
        for doc in [oracle, alone, ours, theirs, rgb] {
            free(doc);
        }
    }
}

#[test]
fn the_placed_and_warped_blockers_name_no_phase() {
    let rgb = srgb_doc(pattern());
    let input = picture();
    let place = json!({"op":"place","source_id":"lab-close-2","name":"Placed","x":0,"y":0});
    let placed = source_edit(rgb, input, &place).unwrap().unwrap();
    assert_eq!(
        to_lab(placed, &depth(16)).unwrap_err(),
        "“Placed” is a placed image, which a Lab Color document cannot hold; flatten the image to convert it"
    );
    let mut displacements = vec![[0.0, 0.0]; 9];
    displacements[4] = [2.0, 1.5];
    let warp = CString::new(
        json!({"op":"set","layer":0,"target":"pixels","columns":3,"rows":3,
            "displacements":displacements,"filter":"bicubic"})
        .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let warped =
        unsafe { dr_precision_doc_warp_edit(rgb, warp.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    let warped = result(warped, err).unwrap().unwrap();
    assert_eq!(
        to_lab(warped, &depth(16)).unwrap_err(),
        "“Layer” is a warped layer, which a Lab Color document cannot hold; flatten the image to convert it"
    );
    for doc in [warped, placed, input, rgb] {
        free(doc);
    }
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
    let lab = lab_doc(16);
    let rgb = srgb_doc(pattern());
    for request in [
        json!({"op":"bogus"}),
        json!({"op":"layer_style","version":1,"layer":0,"style":3}),
        json!({"op":"selection","operation":{"kind":"bogus"}}),
    ] {
        let theirs = edit(rgb, &request, &[]).unwrap_err();
        assert_eq!(edit(lab, &request, &[]).unwrap_err(), theirs, "{request}");
        assert!(!theirs.contains("Lab Color"), "{request}: {theirs}");
    }
    let adjustment = json!({"layer":0,"op":"adjustment","description":
        json!({"type":"adjust","evaluation":"precision_v1","op":"bogus","params":{}}).to_string()});
    let theirs = selected_err(rgb, &adjustment);
    assert_eq!(selected_err(lab, &adjustment), theirs);
    assert_eq!(theirs, "unsupported high-depth adjustment bogus");
    assert_eq!(
        edit(lab, &json!({"op":"read_plane","target":"layer","index":0}), &[]).unwrap_err(),
        "read_plane on a Lab Color document takes target mask, channel, red (Lightness), green (a), blue (b) or alpha."
    );
    free(rgb);
    free(lab);
}

#[test]
fn image_size_resolution_keeps_the_mode() {
    let lab = lab_doc(16);
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_resolution(lab, 300.0, 300.0, &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let value = descriptor(out);
    assert_eq!(value["color_mode"], "lab");
    assert_eq!(value["lab"]["depth"], 16);
    assert_eq!(value["resolution"]["x_ppi"], 300.0);
    free(out);
    free(lab);
}

/// An export's PNG samples (big-endian u16 pairs decoded), with its type.
fn png_samples(out: *mut ExportResult, err: *mut c_char) -> (png::ColorType, Vec<u16>) {
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
    (info.color_type, samples)
}

#[test]
fn a_layer_export_equals_the_layer_alone_exported() {
    let srgb = IccProfile::srgb();
    let rgb = doc_of(
        srgb.clone(),
        vec![
            PrecisionImage::from_rgba16(W, H, pattern(), srgb.clone(), None).unwrap(),
            coloured(W, H, srgb),
        ],
    );
    let lab = to_lab(rgb, &depth(16)).unwrap();
    let request = CString::new(r#"{"format":"png","bit_depth":16}"#).unwrap();
    let mut err = ptr::null_mut();
    let layer = unsafe {
        dr_precision_doc_export_layer_encode(
            lab,
            1,
            false,
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let ours = png_samples(layer, err);
    let image = unsafe { &*lab }.layer_source(1).unwrap().clone();
    let alone = boxed(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
            .unwrap()
            .with_lab_depth(16)
            .unwrap(),
    );
    let mut err = ptr::null_mut();
    let whole = unsafe {
        dr_precision_doc_export_encode(
            alone,
            ptr::null(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let theirs = png_samples(whole, err);
    assert_eq!(ours.0, png::ColorType::Rgba);
    assert_eq!(ours, theirs);
    for doc in [alone, lab, rgb] {
        free(doc);
    }
}
