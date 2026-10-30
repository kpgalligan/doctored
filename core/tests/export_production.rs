//! Phase 11B — production outputs: per-item layer/group exports, contact
//! sheets and the metadata policy, all at the C boundary.
//!
//! Every oracle is written HERE. The isolated render is compared against
//! `common::ref_composite` over a transparent backdrop; the trim rectangle,
//! the contact-sheet geometry and the `{index}` padding are compared against
//! the formulas restated in this file; and every written packet is found by
//! this file's own JPEG segment and PNG chunk walkers, never by asking the
//! core what it thinks it wrote. No golden images.

use crate::common;
use common::metadata_fixtures::{build_exif, iptc_run};
use common::{ref_composite, solid, take_err_string, to_unit, BLEND_NORMAL};

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::doc_color::MetadataKind;
use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::{dr_image_free, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_export::*;
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};

use std::ffi::{c_char, CStr, CString};
use std::ptr;

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

// --------------------------------------------------------------- plumbing --

fn owned_string(text: *mut c_char) -> String {
    assert!(!text.is_null());
    let owned = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    owned
}

fn json_of(text: *mut c_char) -> serde_json::Value {
    serde_json::from_str(&owned_string(text)).expect("json")
}

fn result_bytes(result: *const ExportResult) -> Vec<u8> {
    let mut length = 0usize;
    let pointer = unsafe { dr_export_result_bytes(result, &mut length) };
    assert!(!pointer.is_null());
    unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec()
}

fn result_info(result: *const ExportResult) -> serde_json::Value {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_export_result_info(result, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    json_of(json)
}

fn export_layer(
    doc: &DrDocument,
    idx: usize,
    trimmed: bool,
    request: &str,
) -> Result<*mut ExportResult, String> {
    let json = CString::new(request).expect("request");
    let mut err: *mut c_char = ptr::null_mut();
    let result = unsafe {
        dr_doc_export_layer_encode(doc, idx, trimmed, json.as_ptr(), ptr::null(), &mut err)
    };
    if result.is_null() {
        return Err(take_err_string(err));
    }
    Ok(result)
}

fn export_layer16(
    document: &PrecisionDocument,
    idx: usize,
    trimmed: bool,
    request: &str,
) -> Result<*mut ExportResult, String> {
    let json = CString::new(request).expect("request");
    let mut err: *mut c_char = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_export_layer_encode(
            document,
            idx,
            trimmed,
            json.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if result.is_null() {
        return Err(take_err_string(err));
    }
    Ok(result)
}

fn sheet_size(request: &str, count: usize) -> Result<(u32, u32, u32), String> {
    let json = CString::new(request).expect("request");
    let mut err: *mut c_char = ptr::null_mut();
    let text = unsafe { dr_contact_sheet_size(json.as_ptr(), count, &mut err) };
    if text.is_null() {
        return Err(take_err_string(err));
    }
    let value = json_of(text);
    Ok((
        value["width"].as_u64().unwrap() as u32,
        value["height"].as_u64().unwrap() as u32,
        value["rows"].as_u64().unwrap() as u32,
    ))
}

fn compose_sheet(
    request: &str,
    sources: &[&DrDocument],
    captions: Option<&[*const DrImage]>,
) -> Result<*mut DrImage, String> {
    let json = CString::new(request).expect("request");
    let pointers: Vec<*const DrDocument> =
        sources.iter().map(|d| *d as *const DrDocument).collect();
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe {
        dr_contact_sheet_compose(
            json.as_ptr(),
            pointers.as_ptr(),
            captions.map_or(ptr::null(), |c| c.as_ptr()),
            pointers.len(),
            ptr::null(),
            &mut err,
        )
    };
    if image.is_null() {
        return Err(take_err_string(err));
    }
    Ok(image)
}

fn sheet_pixels(image: *const DrImage, w: u32, h: u32) -> Vec<u8> {
    let pointer = unsafe { dr_image_pixels_rgba(image) };
    assert!(!pointer.is_null());
    unsafe { std::slice::from_raw_parts(pointer, (w * h * 4) as usize) }.to_vec()
}

fn at(pixels: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

fn expand(pattern: &str, tokens: &str) -> Result<String, String> {
    let pattern = CString::new(pattern).expect("pattern");
    let tokens = CString::new(tokens).expect("tokens");
    let mut err: *mut c_char = ptr::null_mut();
    let text = unsafe { dr_export_expand_name(pattern.as_ptr(), tokens.as_ptr(), &mut err) };
    if text.is_null() {
        return Err(take_err_string(err));
    }
    Ok(owned_string(text))
}

fn validate_policy(policy: &str) -> Result<serde_json::Value, String> {
    let json = CString::new(policy).expect("policy");
    let mut err: *mut c_char = ptr::null_mut();
    let text = unsafe { dr_metadata_policy_validate(json.as_ptr(), &mut err) };
    if text.is_null() {
        return Err(take_err_string(err));
    }
    Ok(json_of(text))
}

// ------------------------------------------- independent container walkers --

/// Every JPEG marker segment as `(marker, payload)`, stopping at the scan.
fn jpeg_segments(bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
    assert_eq!(&bytes[..2], b"\xff\xd8");
    let mut out = Vec::new();
    let mut i = 2usize;
    while i + 4 <= bytes.len() {
        assert_eq!(bytes[i], 0xff, "a segment must start with 0xff");
        let marker = bytes[i + 1];
        if marker == 0xda {
            break;
        }
        let length = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        out.push((marker, bytes[i + 4..i + 2 + length].to_vec()));
        i += 2 + length;
    }
    out
}

/// The payload of the first APP segment whose identifier is `prefix`.
fn app_payload(bytes: &[u8], marker: u8, prefix: &[u8]) -> Option<Vec<u8>> {
    jpeg_segments(bytes)
        .into_iter()
        .find(|(m, payload)| *m == marker && payload.starts_with(prefix))
        .map(|(_, payload)| payload[prefix.len()..].to_vec())
}

/// The JFIF APP0's `(units, Xdensity, Ydensity)`. Units 0 means "no units",
/// which is the JPEG encoder's way of stating no resolution at all.
fn jfif_density(bytes: &[u8]) -> Option<(u8, u16, u16)> {
    let payload = app_payload(bytes, 0xe0, b"JFIF\0")?;
    Some((
        payload[2],
        u16::from_be_bytes([payload[3], payload[4]]),
        u16::from_be_bytes([payload[5], payload[6]]),
    ))
}

fn png_chunks(bytes: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let mut out = Vec::new();
    let mut i = 8usize;
    while i + 12 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = bytes[i + 4..i + 8].try_into().unwrap();
        out.push((kind, bytes[i + 8..i + 8 + length].to_vec()));
        i += 12 + length;
    }
    out
}

fn png_chunk(bytes: &[u8], kind: &[u8; 4]) -> Option<Vec<u8>> {
    png_chunks(bytes)
        .into_iter()
        .find(|(k, _)| k == kind)
        .map(|(_, d)| d)
}

/// The PNG `iTXt` payload whose keyword is `XML:com.adobe.xmp`, if any.
fn png_xmp(bytes: &[u8]) -> Option<Vec<u8>> {
    for (kind, data) in png_chunks(bytes) {
        if &kind != b"iTXt" {
            continue;
        }
        let nul = data.iter().position(|&b| b == 0)?;
        if &data[..nul] != b"XML:com.adobe.xmp" {
            continue;
        }
        // keyword\0 compression-flag compression-method language\0 translated\0
        let mut i = nul + 3;
        i += data[i..].iter().position(|&b| b == 0)? + 1;
        i += data[i..].iter().position(|&b| b == 0)? + 1;
        return Some(data[i..].to_vec());
    }
    None
}

/// IFD0's inline Orientation SHORT in a little-endian EXIF TIFF stream.
fn exif_orientation(blob: &[u8]) -> Option<u16> {
    assert_eq!(&blob[..2], b"II", "this file only assembles little-endian");
    let u16_at = |a: usize| u16::from_le_bytes([blob[a], blob[a + 1]]);
    let ifd = u32::from_le_bytes(blob[4..8].try_into().unwrap()) as usize;
    (0..usize::from(u16_at(ifd)))
        .map(|i| ifd + 2 + 12 * i)
        .find(|&e| u16_at(e) == 0x0112)
        .map(|e| u16_at(e + 8))
}

/// Every `tiff:Orientation` value an XMP packet states, in both spellings.
fn xmp_orientations(blob: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(blob).into_owned();
    let mut out = Vec::new();
    for (pattern, terminator) in [
        ("tiff:Orientation=\"", '"'),
        ("tiff:Orientation='", '\''),
        ("<tiff:Orientation>", '<'),
    ] {
        let mut rest = text.as_str();
        while let Some(at) = rest.find(pattern) {
            let tail = &rest[at + pattern.len()..];
            let end = tail.find(terminator).unwrap_or(tail.len());
            out.push(tail[..end].to_string());
            rest = &tail[end..];
        }
    }
    out
}

// --------------------------------------------------------------- fixtures --

const W: u32 = 16;
const H: u32 = 12;

/// A canvas-sized transparent plate with one solid rectangle in it, so the
/// alpha bounding box is a rectangle this file states rather than one the
/// core discovers.
fn patch(rect: (u32, u32, u32, u32), colour: [u8; 4]) -> RgbaImage {
    let mut image = RgbaImage::from_pixel(W, H, Rgba([0, 0, 0, 0]));
    for y in rect.1..rect.1 + rect.3 {
        for x in rect.0..rect.0 + rect.2 {
            image.put_pixel(x, y, Rgba(colour));
        }
    }
    image
}

/// Three top-level items over an opaque background:
///   0 background (opaque grey)
///   1 "Sky / Water" — a blue rectangle at (2,3) 5x4, opacity 0.5, masked
///   2 "Empty"       — wholly transparent
fn byte_fixture() -> DrDocument {
    let doc = DrDocument::from_pixels(solid(W, H, [40, 40, 40, 255]));
    let doc = doc
        .adding_image_layer(0, patch((2, 3, 5, 4), [10, 80, 200, 255]), "Sky / Water")
        .expect("layer 1");
    let doc = doc.with_layer_opacity(1, 0.5).expect("opacity");
    // Coverage 128 over the left half of the rectangle, 255 over the right.
    let selection: Vec<u8> = (0..W * H)
        .map(|i| if (i % W) < 4 { 128 } else { 255 })
        .collect();
    let doc = doc
        .add_mask(1, MaskKind::FromSelection(&selection))
        .expect("mask");
    doc.adding_image_layer(1, RgbaImage::from_pixel(W, H, Rgba([0, 0, 0, 0])), "Empty")
        .expect("layer 2")
}

fn typed_fixture() -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let plate = |f: &dyn Fn(u32, u32) -> [u16; 4]| {
        let samples: Vec<[u16; 4]> = (0..W * H).map(|i| f(i % W, i / W)).collect();
        PrecisionImage::from_rgba16(W, H, samples, profile.clone(), None).expect("plate")
    };
    let background = plate(&|_, _| [10_000, 10_000, 10_000, 65535]);
    let subject = plate(&|x, y| {
        if (2..7).contains(&x) && (3..7).contains(&y) {
            [1_000, 20_000, 50_000, 65535]
        } else {
            [0, 0, 0, 0]
        }
    });
    let empty = plate(&|_, _| [0, 0, 0, 0]);
    PrecisionDocument::from_parts(
        W,
        H,
        profile,
        Default::default(),
        Default::default(),
        vec![
            PrecisionDocumentLayer::raster("Background", background, (0, 0), 1.0, true)
                .expect("background"),
            PrecisionDocumentLayer::raster("Subject", subject, (0, 0), 1.0, true).expect("subject"),
            PrecisionDocumentLayer::raster("Empty", empty, (0, 0), 1.0, true).expect("empty"),
        ],
    )
    .expect("typed document")
}

/// A document carrying one EXIF block (Orientation 6, 300 ppi), one XMP
/// packet with an `rdf:Description` to stamp, and one 8BIM IPTC run — the
/// three packets every policy rule is checked against.
fn packet_doc(xmp: &[u8]) -> DrDocument {
    let exif = build_exif(false, 6, (300, 300), Some((W, H)), false);
    let doc = DrDocument::from_pixels(solid(W, H, [200, 120, 60, 255]));
    let doc = doc
        .set_metadata(MetadataKind::Exif, Some(&exif.bytes))
        .expect("exif admitted")
        .expect("exif");
    let doc = doc
        .set_metadata(MetadataKind::Xmp, Some(xmp))
        .expect("xmp admitted")
        .expect("xmp");
    let doc = doc
        .set_metadata(MetadataKind::Iptc, Some(&iptc_run()))
        .expect("iptc admitted")
        .expect("iptc");
    doc.set_resolution(150.0, 150.0)
        .expect("resolution admitted")
        .expect("resolution")
}

const XMP_WITH_DESCRIPTION: &[u8] = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description rdf:about='' xmlns:tiff='http://ns.adobe.com/tiff/1.0/' tiff:Orientation='6' tiff:XResolution='300/1'></rdf:Description></rdf:RDF></x:xmpmeta>";

/// The same packet with NOTHING to stamp: a self-closing description, which
/// has no content model to insert a property into.
const XMP_UNSTAMPABLE: &[u8] = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description rdf:about='' xmlns:tiff='http://ns.adobe.com/tiff/1.0/' tiff:Orientation='6'/></rdf:RDF></x:xmpmeta>";

// ------------------------------------------------ (a) the isolated render --

#[test]
fn an_isolated_item_composites_against_transparency() {
    let doc = byte_fixture();
    // Panel order: top first — "Empty" (index 2), "Sky / Water" (1),
    // "Background" (0).
    assert_eq!(doc.top_level_items(), vec![2, 1, 0]);

    let result = export_layer(&doc, 1, false, r#"{"format":"png"}"#).expect("layer export");
    let info = result_info(result);
    assert_eq!(info["layer"]["index"], 1);
    assert_eq!(info["layer"]["name"], "Sky / Water");
    assert_eq!(info["layer"]["origin"], serde_json::json!([0, 0]));
    assert_eq!(info["layer"]["size"], serde_json::json!([W, H]));

    let decoded = image::load_from_memory(&result_bytes(result))
        .expect("decode")
        .to_rgba8();
    unsafe { dr_export_result_free(result) };
    assert_eq!(decoded.dimensions(), (W, H));

    // The oracle: the layer's own pixel over a FULLY TRANSPARENT backdrop,
    // at `opacity * mask/255`. The background layer must contribute nothing.
    let source = patch((2, 3, 5, 4), [10, 80, 200, 255]);
    for y in 0..H {
        for x in 0..W {
            let coverage = if x < 4 { 128.0 / 255.0 } else { 1.0 };
            let expected = ref_composite(
                [0.0, 0.0, 0.0, 0.0],
                to_unit(source.get_pixel(x, y).0),
                0.5 * coverage,
                BLEND_NORMAL,
            );
            let actual = to_unit(decoded.get_pixel(x, y).0);
            for c in 0..4 {
                assert!(
                    (actual[c] - expected[c]).abs() <= 2.0 / 255.0,
                    "({x},{y}) channel {c}: {actual:?} vs {expected:?}"
                );
            }
        }
    }
}

#[test]
fn a_trim_delivers_the_alpha_bounding_box_and_reports_it() {
    let doc = byte_fixture();
    let result = export_layer(&doc, 1, true, r#"{"format":"png"}"#).expect("trimmed export");
    let info = result_info(result);
    assert_eq!(info["layer"]["origin"], serde_json::json!([2, 3]));
    assert_eq!(info["layer"]["size"], serde_json::json!([5, 4]));
    assert_eq!(info["width"], 5);
    assert_eq!(info["height"], 4);
    let decoded = image::load_from_memory(&result_bytes(result))
        .expect("decode")
        .to_rgba8();
    unsafe { dr_export_result_free(result) };
    assert_eq!(decoded.dimensions(), (5, 4));
    assert!(decoded.pixels().all(|p| p.0[3] > 0));
}

#[test]
fn a_transparent_item_is_refused_only_when_trimmed() {
    let doc = byte_fixture();
    let refusal = export_layer(&doc, 2, true, r#"{"format":"png"}"#).unwrap_err();
    assert_eq!(
        refusal,
        "layer \"Empty\" renders no visible pixels; nothing to trim"
    );

    let result = export_layer(&doc, 2, false, r#"{"format":"png"}"#).expect("canvas frame");
    let info = result_info(result);
    assert_eq!(info["layer"]["origin"], serde_json::json!([0, 0]));
    assert_eq!(info["layer"]["size"], serde_json::json!([W, H]));
    let decoded = image::load_from_memory(&result_bytes(result))
        .expect("decode")
        .to_rgba8();
    unsafe { dr_export_result_free(result) };
    assert_eq!(decoded.dimensions(), (W, H));
    assert!(decoded.pixels().all(|p| p.0[3] == 0), "every alpha is 0");
}

#[test]
fn the_typed_layer_export_trims_and_refuses_the_same_way() {
    let document = typed_fixture();
    assert_eq!(document.top_level_items(), vec![2, 1, 0]);

    let mut err: *mut c_char = ptr::null_mut();
    let request = CString::new(r#"{"format":"png","bit_depth":16}"#).unwrap();
    let plan = unsafe {
        dr_precision_doc_export_layer_plan(&document, 1, true, request.as_ptr(), &mut err)
    };
    assert!(!plan.is_null(), "{}", take_err_string(err));
    let plan = json_of(plan);
    assert!(plan["scratch_bytes"].as_u64().unwrap() > 0);

    let result =
        export_layer16(&document, 1, true, r#"{"format":"png","bit_depth":16}"#).expect("typed");
    let info = result_info(result);
    assert_eq!(info["layer"]["origin"], serde_json::json!([2, 3]));
    assert_eq!(info["layer"]["size"], serde_json::json!([5, 4]));
    assert_eq!(info["bit_depth"], 16);
    let bytes = result_bytes(result);
    unsafe { dr_export_result_free(result) };

    let decoded =
        doctored_core::precision_codec::decode(&bytes, "the trimmed export", LIMIT, 0, None)
            .expect("decode16");
    assert_eq!(decoded.image.dimensions(), (5, 4));
    let samples = decoded.image.to_rgba16().expect("rgba16");
    assert!(samples.iter().all(|s| *s == [1_000, 20_000, 50_000, 65535]));

    let refusal = export_layer16(&document, 2, true, r#"{"format":"png","bit_depth":16}"#)
        .expect_err("transparent");
    assert_eq!(
        refusal,
        "layer \"Empty\" renders no visible pixels; nothing to trim"
    );

    let result = export_layer16(&document, 2, false, r#"{"format":"png","bit_depth":16}"#)
        .expect("canvas frame");
    let bytes = result_bytes(result);
    unsafe { dr_export_result_free(result) };
    let decoded =
        doctored_core::precision_codec::decode(&bytes, "the empty export", LIMIT, 0, None)
            .expect("decode16");
    assert_eq!(decoded.image.dimensions(), (W, H));
    assert!(decoded
        .image
        .to_rgba16()
        .expect("rgba16")
        .iter()
        .all(|s| s[3] == 0));
}

/// A 1000×4 u16 canvas whose one item is an opaque 4×4 patch: trimmed, a
/// width of 1000 makes the 4×4 cut a 1000×1000 deliverable, far past the
/// canvas-based plan. The export re-admits with the cut's own plan.
#[test]
fn a_trimmed_typed_resample_re_admits_with_the_cut_plan() {
    use doctored_core::doc_export::ExportRequest;
    let (w, h) = (1000u32, 4u32);
    let profile = IccProfile::srgb();
    let samples: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            if (100..104).contains(&(i % w)) {
                [1_000, 20_000, 50_000, 65535]
            } else {
                [0, 0, 0, 0]
            }
        })
        .collect();
    let doc = PrecisionDocument::from_parts(
        w,
        h,
        profile.clone(),
        Default::default(),
        Default::default(),
        vec![PrecisionDocumentLayer::raster(
            "Patch",
            PrecisionImage::from_rgba16(w, h, samples, profile, None).expect("plate"),
            (0, 0),
            1.0,
            true,
        )
        .expect("patch")],
    )
    .expect("typed document");
    let wide = ExportRequest::parse(
        r#"{"format":"png","bit_depth":16,"size":{"mode":"width","value":1000}}"#,
    )
    .unwrap();
    let exact = ExportRequest::parse(r#"{"format":"png","bit_depth":16}"#).unwrap();
    let tight = |trimmed: bool, request: &ExportRequest| {
        let plan = doc.export_layer_plan(0, trimmed, request).unwrap();
        plan["retained_bytes"].as_u64().unwrap()
            + plan["scratch_bytes"].as_u64().unwrap()
            + (1 << 20)
    };
    let error = doc
        .export_layer_encoded(0, true, &wide, tight(true, &wide), 0, None)
        .err()
        .expect("the cut's 1000×1000 output does not fit the canvas plan");
    assert!(error.contains("working limit"), "{error}");
    assert!(error.contains("trimmed to 4 × 4 and resized to"), "{error}");
    let result = doc
        .export_layer_encoded(0, true, &wide, LIMIT, 0, None)
        .expect("the full limit admits it");
    assert_eq!(
        (
            result.info_json()["width"].as_u64(),
            result.info_json()["height"].as_u64()
        ),
        (Some(1000), Some(1000))
    );
    doc.export_layer_encoded(0, false, &wide, tight(false, &wide), 0, None)
        .expect("untrimmed, the canvas plan is the plan");
    doc.export_layer_encoded(0, true, &exact, tight(true, &exact), 0, None)
        .expect("trimmed with size none, the cut is smaller than the canvas");
}

// --------------------------------------------------------- (b) the tokens --

const BASE: &str =
    r#""name":"Beach","width":800,"height":600,"format":"jpeg","date":"2026-09-17","preset":"Web""#;

#[test]
fn the_layer_and_index_tokens_sanitize_pad_and_refuse() {
    let tokens = format!(r#"{{{BASE},"layer":"Sky / Water","index":3,"count":12}}"#);
    assert_eq!(
        expand("{name}-{layer}-{index}", &tokens).unwrap(),
        "Beach-Sky - Water-03"
    );

    // The three refused characters and a control character become "-", and
    // the surrounding whitespace is trimmed off afterwards.
    let punctuation = format!(r#"{{{BASE},"layer":"  :  ","index":1,"count":1}}"#);
    assert_eq!(expand("{name}-{layer}", &punctuation).unwrap(), "Beach--");
    let control = format!("{{{BASE},\"layer\":\"a\\u0007b\",\"index\":1,\"count\":1}}");
    assert_eq!(expand("{name}-{layer}", &control).unwrap(), "Beach-a-b");
    // A name that sanitizes to nothing becomes the literal "layer".
    let blank = format!(r#"{{{BASE},"layer":"   ","index":1,"count":1}}"#);
    assert_eq!(expand("{name}-{layer}", &blank).unwrap(), "Beach-layer");

    // The digit width follows `count`, not the index.
    for (index, count, expected) in [(3u32, 12u32, "03"), (3, 9, "3"), (7, 1000, "0007")] {
        let tokens = format!(r#"{{{BASE},"layer":"L","index":{index},"count":{count}}}"#);
        assert_eq!(
            expand("{name}{index}", &tokens).unwrap(),
            format!("Beach{expected}")
        );
    }

    // Absent tokens refuse rather than expanding to nothing.
    let plain = format!("{{{BASE}}}");
    assert!(expand("{name}-{layer}", &plain)
        .unwrap_err()
        .contains("this export has no layer"));
    assert!(expand("{name}-{index}", &plain)
        .unwrap_err()
        .contains("this export has no item index"));
    assert!(expand("{name}-{bogus}", &plain)
        .unwrap_err()
        .contains("unknown token {bogus}"));
}

#[test]
fn a_layer_export_pattern_needs_layer_or_index_not_name() {
    let tokens =
        |flag: &str| format!(r#"{{{BASE},"layer":"Sky / Water","index":3,"count":12{flag}}}"#);
    let layer = tokens(r#","layer_export":true"#);
    assert_eq!(expand("{index}", &layer).unwrap(), "03");
    assert_eq!(expand("{layer}", &layer).unwrap(), "Sky - Water");
    assert_eq!(
        expand("{name}-{layer}", &layer).unwrap(),
        "Beach-Sky - Water"
    );
    for pattern in ["{name}", "{name}-{width}"] {
        assert_eq!(
            expand(pattern, &layer).unwrap_err(),
            "a layer export naming pattern must contain {layer} or {index}"
        );
    }

    // Every other export keeps {name} mandatory, flagged false or unflagged.
    for flag in [r#","layer_export":false"#, ""] {
        assert_eq!(
            expand("{index}", &tokens(flag)).unwrap_err(),
            "a naming pattern must contain {name}"
        );
    }
    assert_eq!(
        expand("{index}", &tokens(r#","layer_export":"yes""#)).unwrap_err(),
        "export name tokens' layer_export must be true or false"
    );

    // The absent-token rule still applies to a layer export.
    let bare = format!(r#"{{{BASE},"layer_export":true}}"#);
    assert!(expand("{index}", &bare)
        .unwrap_err()
        .contains("this export has no item index"));
}

// -------------------------------------------------- (c) contact sheets --

/// The formulas, restated here rather than imported.
fn sheet_formula(
    columns: u32,
    cell: (u32, u32),
    gutter: u32,
    caption: u32,
    count: u32,
) -> (u32, u32, u32) {
    let rows = count.div_ceil(columns);
    (
        gutter + columns * (cell.0 + gutter),
        gutter + rows * (cell.1 + caption + gutter),
        rows,
    )
}

#[test]
fn contact_sheet_geometry_matches_the_formula() {
    for (columns, cw, ch, gutter, caption, count) in [
        (4u32, 256u32, 256u32, 16u32, 0u32, 7u32),
        (3, 100, 80, 0, 24, 3),
        (1, 64, 64, 8, 16, 5),
        (5, 32, 48, 4, 12, 13),
    ] {
        let request = format!(
            r#"{{"columns":{columns},"cell_width":{cw},"cell_height":{ch},"gutter":{gutter},"caption_height":{caption}}}"#
        );
        assert_eq!(
            sheet_size(&request, count as usize).unwrap(),
            sheet_formula(columns, (cw, ch), gutter, caption, count)
        );
    }

    // Defaults.
    assert_eq!(
        sheet_size("{}", 5).unwrap(),
        sheet_formula(4, (256, 256), 16, 0, 5)
    );

    // Caps.
    assert!(sheet_size("{}", 0)
        .unwrap_err()
        .contains("at least one cell"));
    assert!(sheet_size("{}", 257)
        .unwrap_err()
        .contains("at most 256 cells"));
    assert!(
        sheet_size(r#"{"columns":32,"cell_width":2048,"gutter":256}"#, 256)
            .unwrap_err()
            .contains("30000 pixels on a side")
    );
    assert!(sheet_size(r#"{"columns":0}"#, 1)
        .unwrap_err()
        .contains("columns must be an integer in [1,32]"));
    assert!(sheet_size(r#"{"bogus":1}"#, 1)
        .unwrap_err()
        .contains("unknown key \"bogus\""));
    assert!(sheet_size(r#"{"background":"blue"}"#, 1)
        .unwrap_err()
        .contains("rrggbb"));
}

#[test]
fn a_solid_cell_lands_exactly_where_the_geometry_says() {
    // Cells 40x30, gutter 6, two columns, one caption strip of 10.
    const REQUEST: &str = r##"{"columns":2,"cell_width":40,"cell_height":30,"gutter":6,"caption_height":10,"background":"#204080","filter":"nearest"}"##;
    let solid_doc = DrDocument::from_pixels(solid(40, 30, [255, 0, 0, 255]));
    let small = DrDocument::from_pixels(solid(20, 10, [0, 255, 0, 255]));
    let empty = DrDocument::from_pixels(RgbaImage::from_pixel(40, 30, Rgba([0, 0, 0, 0])));

    // A red caption bitmap 40x10, so its placement is checkable too.
    let caption_pixels: Vec<u8> = (0..40 * 10).flat_map(|_| [255, 255, 0, 255]).collect();
    let caption =
        unsafe { doctored_core::ffi::dr_image_from_rgba(caption_pixels.as_ptr(), 40, 10) };
    assert!(!caption.is_null());
    let captions: Vec<*const DrImage> = vec![caption, ptr::null(), ptr::null()];

    let (w, h, rows) = sheet_size(REQUEST, 3).unwrap();
    assert_eq!((w, h, rows), sheet_formula(2, (40, 30), 6, 10, 3));

    let sheet =
        compose_sheet(REQUEST, &[&solid_doc, &small, &empty], Some(&captions)).expect("compose");
    let pixels = sheet_pixels(sheet, w, h);
    unsafe { dr_image_free(sheet) };
    unsafe { dr_image_free(caption) };

    const BACKGROUND: [u8; 4] = [0x20, 0x40, 0x80, 255];
    // The gutter is background.
    assert_eq!(at(&pixels, w, 0, 0), BACKGROUND);
    assert_eq!(at(&pixels, w, 3, 3), BACKGROUND);

    // Cell 0 fills its 40x30 area exactly, and one pixel outside it does not.
    for (x, y) in [(6, 6), (45, 35), (6, 35), (45, 6)] {
        assert_eq!(
            at(&pixels, w, x, y),
            [255, 0, 0, 255],
            "cell 0 at ({x},{y})"
        );
    }
    assert_eq!(at(&pixels, w, 5, 6), BACKGROUND);
    assert_eq!(at(&pixels, w, 46, 6), BACKGROUND);

    // Cell 0's caption sits in the strip below it.
    assert_eq!(at(&pixels, w, 6, 36), [255, 255, 0, 255]);
    assert_eq!(at(&pixels, w, 45, 45), [255, 255, 0, 255]);

    // Cell 1 is a 20x10 source, never upscaled and centred: x from
    // 6 + (40+6) + (40-20)/2, y from 6 + (30-10)/2.
    let (cx, cy) = (6 + 46 + 10, 6 + 10);
    assert_eq!(at(&pixels, w, cx, cy), [0, 255, 0, 255]);
    assert_eq!(at(&pixels, w, cx - 1, cy), BACKGROUND);
    assert_eq!(at(&pixels, w, cx + 20, cy), BACKGROUND);

    // Cell 2 is wholly transparent, so it is background everywhere.
    let (ex, ey) = (6, 6 + 30 + 10 + 6);
    assert_eq!(at(&pixels, w, ex, ey), BACKGROUND);
    assert_eq!(at(&pixels, w, ex + 20, ey + 15), BACKGROUND);

    // Zero cells refuse before anything is composed.
    assert!(compose_sheet(REQUEST, &[], None)
        .unwrap_err()
        .contains("at least one cell"));
}

// ------------------------------------------------ (d) the metadata policy --

fn encode_with(doc: &DrDocument, request: &str) -> (Vec<u8>, serde_json::Value) {
    let json = CString::new(request).expect("request");
    let mut err: *mut c_char = ptr::null_mut();
    let result =
        unsafe { dr_doc_export_encode(doc, ptr::null(), json.as_ptr(), ptr::null(), &mut err) };
    assert!(!result.is_null(), "{}", take_err_string(err));
    let bytes = result_bytes(result);
    let info = result_info(result);
    unsafe { dr_export_result_free(result) };
    (bytes, info)
}

fn refuse(doc: &DrDocument, request: &str) -> String {
    let json = CString::new(request).expect("request");
    let mut err: *mut c_char = ptr::null_mut();
    let result =
        unsafe { dr_doc_export_encode(doc, ptr::null(), json.as_ptr(), ptr::null(), &mut err) };
    assert!(result.is_null(), "expected a refusal");
    take_err_string(err)
}

#[test]
fn the_default_policy_writes_exactly_the_bytes_no_policy_writes() {
    let doc = packet_doc(XMP_WITH_DESCRIPTION);
    for format in ["jpeg", "png"] {
        let (plain, plain_info) = encode_with(&doc, &format!(r#"{{"format":"{format}"}}"#));
        let (explicit, explicit_info) = encode_with(
            &doc,
            &format!(
                r#"{{"format":"{format}","metadata":{{"exif":"default","xmp":"default","iptc":"default","density":"default","stamp":null}}}}"#
            ),
        );
        assert_eq!(plain, explicit, "{format}: default policy changes bytes");
        assert_eq!(plain_info["metadata"], explicit_info["metadata"]);
    }
}

#[test]
fn each_packet_rule_decides_what_a_jpeg_carries() {
    let doc = packet_doc(XMP_WITH_DESCRIPTION);
    let cases = [
        (r#""exif":"drop""#, (false, true, true)),
        (r#""xmp":"drop""#, (true, false, true)),
        (r#""iptc":"drop""#, (true, true, false)),
        (
            r#""exif":"drop","xmp":"drop","iptc":"drop""#,
            (false, false, false),
        ),
        (
            r#""exif":"keep","xmp":"keep","iptc":"keep""#,
            (true, true, true),
        ),
    ];
    for (rules, (exif, xmp, iptc)) in cases {
        // `keep` must win over strip_metadata; `drop` must win over its
        // absence, so both switches are exercised on the same case.
        let strip = rules.contains("keep");
        let (bytes, info) = encode_with(
            &doc,
            &format!(r#"{{"format":"jpeg","strip_metadata":{strip},"metadata":{{{rules}}}}}"#),
        );
        let written_exif = app_payload(&bytes, 0xe1, b"Exif\0\0");
        let written_xmp = app_payload(&bytes, 0xe1, b"http://ns.adobe.com/xap/1.0/\0");
        let written_iptc = app_payload(&bytes, 0xed, b"Photoshop 3.0\0");
        assert_eq!(written_exif.is_some(), exif, "{rules}: exif");
        assert_eq!(written_xmp.is_some(), xmp, "{rules}: xmp");
        assert_eq!(written_iptc.is_some(), iptc, "{rules}: iptc");
        // The REPORT must equal what this file just parsed out of the bytes.
        assert_eq!(info["metadata"]["exif"], exif, "{rules}: report exif");
        assert_eq!(info["metadata"]["xmp"], xmp, "{rules}: report xmp");
        assert_eq!(info["metadata"]["iptc"], iptc, "{rules}: report iptc");
        assert_eq!(info["metadata"]["stamped"], false);
        // Orientation is never a policy choice.
        if let Some(blob) = written_exif {
            assert_eq!(
                exif_orientation(&blob),
                Some(1),
                "{rules}: exif orientation"
            );
        }
        if let Some(blob) = written_xmp {
            assert!(
                xmp_orientations(&blob).iter().all(|v| v == "1"),
                "{rules}: xmp orientation"
            );
        }
    }
}

#[test]
fn each_packet_rule_decides_what_a_png_carries() {
    let doc = packet_doc(XMP_WITH_DESCRIPTION);
    for (rules, (exif, xmp)) in [
        (r#""exif":"drop""#, (false, true)),
        (r#""xmp":"drop""#, (true, false)),
        (r#""exif":"keep","xmp":"keep""#, (true, true)),
    ] {
        let strip = rules.contains("keep");
        let (bytes, info) = encode_with(
            &doc,
            &format!(r#"{{"format":"png","strip_metadata":{strip},"metadata":{{{rules}}}}}"#),
        );
        assert_eq!(png_chunk(&bytes, b"eXIf").is_some(), exif, "{rules}: eXIf");
        assert_eq!(png_xmp(&bytes).is_some(), xmp, "{rules}: iTXt xmp");
        assert_eq!(info["metadata"]["exif"], exif);
        assert_eq!(info["metadata"]["xmp"], xmp);
        // PNG cannot carry IPTC at all, whatever the rule says.
        assert_eq!(info["metadata"]["iptc"], false);
        if let Some(blob) = png_chunk(&bytes, b"eXIf") {
            assert_eq!(exif_orientation(&blob), Some(1));
        }
        if let Some(blob) = png_xmp(&bytes) {
            assert!(xmp_orientations(&blob).iter().all(|v| v == "1"));
        }
    }
}

#[test]
fn dropping_the_density_needs_the_packets_dropped_too() {
    let doc = packet_doc(XMP_WITH_DESCRIPTION);

    // The valid drop: no packet restates the ppi, so nothing contradicts it.
    let (kept, _) = encode_with(&doc, r#"{"format":"jpeg"}"#);
    assert_eq!(
        jfif_density(&kept),
        Some((1, 150, 150)),
        "the default policy states the document ppi in the APP0"
    );
    let (jpeg, info) = encode_with(
        &doc,
        r#"{"format":"jpeg","metadata":{"exif":"drop","xmp":"drop","density":"drop"}}"#,
    );
    // The encoder always emits a JFIF APP0; a dropped density is its
    // "no units" form, which states an aspect ratio and no resolution.
    assert_eq!(jfif_density(&jpeg).map(|d| d.0), Some(0), "no APP0 density");
    assert_eq!(info["metadata"]["density"], false);

    let (png, info) = encode_with(
        &doc,
        r#"{"format":"png","metadata":{"exif":"drop","xmp":"drop","density":"drop"}}"#,
    );
    assert!(png_chunk(&png, b"pHYs").is_none(), "no pHYs");
    assert_eq!(info["metadata"]["density"], false);

    // The refusal, at validate time.
    const REASON: &str = "density: drop requires the exif and xmp packets to be dropped too; each carries its own copy of the ppi";
    assert_eq!(
        refuse(
            &doc,
            r#"{"format":"jpeg","metadata":{"exif":"keep","xmp":"drop","density":"drop"}}"#
        ),
        REASON
    );
    assert_eq!(
        refuse(
            &doc,
            r#"{"format":"png","metadata":{"exif":"drop","xmp":"keep","density":"drop"}}"#
        ),
        REASON
    );
    // `strip_metadata` alone is enough to make the drop legal.
    let (_, info) = encode_with(
        &doc,
        r#"{"format":"jpeg","strip_metadata":true,"metadata":{"density":"drop"}}"#,
    );
    assert_eq!(info["metadata"]["density"], false);
}

#[test]
fn the_stamp_lands_in_the_xmp_packet_or_reports_that_it_did_not() {
    const STAMP: &str = r#""stamp":{"author":"Ada <L>","copyright":"(c) 2026"}"#;

    // A packet with an `rdf:Description` takes the properties.
    let doc = packet_doc(XMP_WITH_DESCRIPTION);
    let (bytes, info) = encode_with(
        &doc,
        &format!(r#"{{"format":"png","metadata":{{{STAMP}}}}}"#),
    );
    assert_eq!(info["metadata"]["stamped"], true);
    let packet = png_xmp(&bytes).expect("xmp");
    let text = String::from_utf8_lossy(&packet).into_owned();
    assert!(text.contains("<dc:creator><rdf:Seq><rdf:li>Ada &lt;L&gt;</rdf:li>"));
    assert!(text.contains("<dc:rights><rdf:Alt><rdf:li xml:lang=\"x-default\">(c) 2026"));
    assert!(xmp_orientations(&packet).iter().all(|v| v == "1"));

    // A packet with nothing to stamp is written UNCHANGED (normalized) and
    // reports `stamped: false`.
    let plain = packet_doc(XMP_UNSTAMPABLE);
    let (unstamped, info) = encode_with(&plain, r#"{"format":"png"}"#);
    let (attempted, stamped_info) = encode_with(
        &plain,
        &format!(r#"{{"format":"png","metadata":{{{STAMP}}}}}"#),
    );
    assert_eq!(info["metadata"]["stamped"], false);
    assert_eq!(stamped_info["metadata"]["stamped"], false);
    assert_eq!(
        png_xmp(&unstamped).expect("xmp"),
        png_xmp(&attempted).expect("xmp"),
        "an unstampable packet is byte-equal to the normalized one"
    );

    // A document with NO XMP gets a minimal packet holding only the credit.
    let bare = DrDocument::from_pixels(solid(W, H, [10, 10, 10, 255]));
    let (bytes, info) = encode_with(
        &bare,
        &format!(r#"{{"format":"png","metadata":{{{STAMP}}}}}"#),
    );
    assert_eq!(info["metadata"]["stamped"], true);
    assert_eq!(info["metadata"]["xmp"], true);
    let packet = png_xmp(&bytes).expect("minimal xmp");
    let text = String::from_utf8_lossy(&packet).into_owned();
    assert!(text.starts_with("<?xpacket begin="));
    assert!(text.contains("x:xmpmeta"));
    assert!(text.contains("http://purl.org/dc/elements/1.1/"));
    assert!(text.contains("Ada &lt;L&gt;"));
    assert!(text.ends_with("<?xpacket end=\"w\"?>"));
    assert!(xmp_orientations(&packet).is_empty());

    // A stamp with nowhere to go refuses at validate time.
    let refusal = refuse(
        &doc,
        &format!(r#"{{"format":"png","metadata":{{"xmp":"drop",{STAMP}}}}}"#),
    );
    assert_eq!(
        refusal,
        "a metadata stamp is written into the xmp packet, which this export does not write"
    );
    // ... including a format that cannot carry XMP at all.
    assert!(refuse(
        &doc,
        &format!(r#"{{"format":"bmp","metadata":{{{STAMP}}}}}"#)
    )
    .contains("does not write"));
}

#[test]
fn the_policy_validator_echoes_and_refuses() {
    let echo = validate_policy(r#"{"exif":"drop","stamp":{"author":"Ada"}}"#).unwrap();
    assert_eq!(
        echo,
        serde_json::json!({
            "exif": "drop",
            "xmp": "default",
            "iptc": "default",
            "density": "default",
            "stamp": {"author": "Ada", "copyright": serde_json::Value::Null},
        })
    );
    assert_eq!(
        validate_policy("{}").unwrap(),
        serde_json::json!({
            "exif": "default", "xmp": "default", "iptc": "default",
            "density": "default", "stamp": serde_json::Value::Null,
        })
    );
    assert!(validate_policy(r#"{"exif":"maybe"}"#)
        .unwrap_err()
        .contains("not one of default, keep or drop"));
    assert!(validate_policy(r#"{"bogus":"keep"}"#)
        .unwrap_err()
        .contains("unknown key \"bogus\""));
    assert!(validate_policy(r#"{"stamp":{}}"#)
        .unwrap_err()
        .contains("requires an author or a copyright"));
    assert!(validate_policy(r#"{"stamp":{"author":"a\u0007b"}}"#)
        .unwrap_err()
        .contains("control characters"));
    let long = "x".repeat(513);
    assert!(
        validate_policy(&format!(r#"{{"stamp":{{"author":"{long}"}}}}"#))
            .unwrap_err()
            .contains("at most 512 characters")
    );
    assert!(validate_policy("not json")
        .unwrap_err()
        .contains("not valid JSON"));
}

// ---------------------------------------------------------- (e) the rest --

#[test]
fn a_pre_cancelled_control_refuses_every_new_encoder() {
    let doc = byte_fixture();
    let document = typed_fixture();

    let control = unsafe { dr_job_control_create() };
    assert!(!control.is_null());
    unsafe { dr_job_control_cancel(control) };
    let request = CString::new(r#"{"format":"png"}"#).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result =
        unsafe { dr_doc_export_layer_encode(&doc, 1, false, request.as_ptr(), control, &mut err) };
    assert!(result.is_null());
    assert!(take_err_string(err).contains("cancel"));
    unsafe { dr_job_control_free(control) };

    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let request = CString::new(r#"{"format":"png","bit_depth":16}"#).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_export_layer_encode(
            &document,
            1,
            false,
            request.as_ptr(),
            LIMIT,
            0,
            control,
            &mut err,
        )
    };
    assert!(result.is_null());
    assert!(take_err_string(err).contains("cancel"));
    unsafe { dr_job_control_free(control) };

    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let request = CString::new("{}").unwrap();
    let pointers: Vec<*const DrDocument> = vec![&doc];
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe {
        dr_contact_sheet_compose(
            request.as_ptr(),
            pointers.as_ptr(),
            ptr::null(),
            1,
            control,
            &mut err,
        )
    };
    assert!(image.is_null());
    assert!(take_err_string(err).contains("cancel"));
    unsafe { dr_job_control_free(control) };
}

#[test]
fn every_production_export_tolerates_null() {
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe {
        dr_doc_export_layer_encode(ptr::null(), 0, false, ptr::null(), ptr::null(), &mut err)
    }
    .is_null());
    assert!(!take_err_string(err).is_empty());

    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe {
        dr_precision_doc_export_layer_plan(ptr::null(), 0, false, ptr::null(), &mut err)
    }
    .is_null());
    assert!(!take_err_string(err).is_empty());

    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe {
        dr_precision_doc_export_layer_encode(
            ptr::null(),
            0,
            false,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    }
    .is_null());
    assert!(!take_err_string(err).is_empty());

    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_contact_sheet_size(ptr::null(), 1, &mut err) }.is_null());
    assert!(!take_err_string(err).is_empty());

    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe {
        dr_contact_sheet_compose(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            &mut err,
        )
    }
    .is_null());
    assert!(!take_err_string(err).is_empty());

    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_metadata_policy_validate(ptr::null(), &mut err) }.is_null());
    assert!(!take_err_string(err).is_empty());

    // Every one of them tolerates a NULL err_out too.
    assert!(unsafe {
        dr_doc_export_layer_encode(
            ptr::null(),
            0,
            false,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe { dr_metadata_policy_validate(ptr::null(), ptr::null_mut()) }.is_null());
}

#[test]
fn a_nested_or_missing_index_is_refused() {
    let doc = byte_fixture();
    assert!(export_layer(&doc, 99, false, r#"{"format":"png"}"#)
        .unwrap_err()
        .contains("out of range"));
    assert!(
        export_layer(&doc, 1, false, r#"{"format":"png","bit_depth":16}"#)
            .unwrap_err()
            .contains("8 bits")
    );
    let document = typed_fixture();
    assert!(
        export_layer16(&document, 99, false, r#"{"format":"png","bit_depth":16}"#)
            .unwrap_err()
            .contains("top-level layer or group")
    );
}
