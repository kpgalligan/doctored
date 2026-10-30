//! An 8- or 16-bit document converted into a 32-bit float document
//! (`precision_conversion_float`), through `dr_precision_doc_promote_to_float*`
//! and `dr_doc_promote_to_float*`.
//!
//! Oracles, none read off the implementation: the published transfer
//! functions written out here — the sRGB EOTF (sRGB and Display P3), within
//! 1e-5 absolute (the built-in TRC is a 1024-entry 16-bit `curv`, so its
//! quantization is at most 0.5/65535 plus interpolation); Adobe RGB's γ =
//! 563/256 and ProPhoto's γ = 461/256, within 1e-6 relative and exactly 0 at
//! code 0 — and the source codes themselves for the round trip back, which
//! must match EXACTLY. Carriage compares each field of the source with the
//! result; refusals compare the op's error with the plan's first row.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument, Layer, LayerKind};
use doctored_core::doc_comps::CompRequest;
use doctored_core::doc_guide::GuideOrientation;
use doctored_core::doc_lock::{LOCK_PIXELS, LOCK_POSITION};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_float::dr_precision_doc_float_to_u16;
use doctored_core::ffi_path::{dr_precision_doc_paths_json, dr_precision_doc_with_paths_json};
use doctored_core::ffi_precision_conversion::*;
use doctored_core::ffi_raw_source::{
    dr_precision_doc_attach_raw_source, dr_precision_doc_raw_source_length,
};
use doctored_core::ffi_vector_mask::{
    dr_precision_doc_set_vector_mask, dr_precision_doc_vector_mask_json,
};
use doctored_core::float_document::ToneMap;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_paint::PaintEdit;
use doctored_core::retained_source::SourceEdit;
use doctored_core::style::LayerStyle;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::float_placed_image::{mixed, place, typed_master};
use crate::warp_fixture;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 40;
const H: u32 = 30;
/// The sRGB EOTF's tolerance (module doc).
const SRGB_TOLERANCE: f64 = 1e-5;
/// The pure-gamma curves' relative tolerance (module doc).
const GAMMA_TOLERANCE: f64 = 1e-6;
const STALE: &str = "stale binding not carried; a save drops it too";
const WARP_REFUSAL: &str = "a 32-bit float document holds no warp binding";

// ------------------------------------------------------------- plumbing --

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

fn json_of(out: *mut c_char, err: *mut c_char) -> Result<Value, String> {
    match owned(err) {
        Some(message) => Err(message),
        None => Ok(serde_json::from_str(&owned(out).expect("a plan")).unwrap()),
    }
}

fn typed_of(out: *mut PrecisionDocument, err: *mut c_char) -> Result<PrecisionDocument, String> {
    match owned(err) {
        Some(message) => {
            assert!(out.is_null());
            Err(message)
        }
        None => {
            assert!(!out.is_null());
            Ok(*unsafe { Box::from_raw(out) })
        }
    }
}

fn plan16(doc: &PrecisionDocument) -> Result<Value, String> {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_promote_to_float_plan(doc, &mut err) };
    json_of(out, err)
}

fn plan8(doc: &DrDocument) -> Result<Value, String> {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_promote_to_float_plan(doc, &mut err) };
    json_of(out, err)
}

fn float16(
    doc: &PrecisionDocument,
    control: Option<&DrJobControl>,
) -> Result<PrecisionDocument, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_promote_to_float(
            doc,
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut err,
        )
    };
    typed_of(out, err)
}

fn float8(doc: &DrDocument, control: Option<&DrJobControl>) -> Result<PrecisionDocument, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_promote_to_float(doc, LIMIT, 0, control.map_or(ptr::null(), |c| c), &mut err)
    };
    typed_of(out, err)
}

fn floats(doc: &PrecisionDocument, index: usize) -> Vec<[f32; 4]> {
    doc.layer_source(index)
        .expect("a raster record")
        .to_rgba32()
        .expect("float samples")
}

fn codes(doc: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    doc.layer_source(index)
        .expect("a raster record")
        .to_rgba16()
        .expect("16-bit samples")
}

fn notes(plan: &Value, key: &str, field: &str) -> Vec<(u64, String)> {
    plan[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["index"].as_u64().unwrap(),
                row[field].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// The plan's refusal sentence, written out here from its first row
/// (`StructureConversionPlan::refusal`'s two shapes).
fn first_refusal(plan: &Value) -> String {
    let row = &plan["blocked"][0];
    if row["id"] == 0 {
        format!(
            "Preserve Layers cannot convert this document: {}",
            row["reason"].as_str().unwrap()
        )
    } else {
        format!(
            "Preserve Layers cannot convert this document: layer {} \"{}\": {}",
            row["index"],
            row["name"].as_str().unwrap(),
            row["reason"].as_str().unwrap()
        )
    }
}

// --------------------------------------------------------------- oracles --

fn srgb_eotf(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The four built-ins with their published decode and tolerance kind.
fn builtins() -> Vec<(&'static str, Arc<IccProfile>, f64)> {
    vec![
        ("sRGB", IccProfile::srgb(), 0.0),
        ("Display P3", IccProfile::display_p3(), 0.0),
        ("Adobe RGB", IccProfile::adobe_rgb(), 563.0 / 256.0),
        ("ProPhoto", IccProfile::prophoto_rgb(), 461.0 / 256.0),
    ]
}

fn assert_decoded(name: &str, gamma: f64, code: u16, got: f32) {
    let c = f64::from(code) / 65535.0;
    let got = f64::from(got);
    if gamma == 0.0 {
        let want = srgb_eotf(c);
        assert!(
            (got - want).abs() <= SRGB_TOLERANCE,
            "{name} code {code}: {got} vs {want}"
        );
    } else if code == 0 {
        assert_eq!(got, 0.0, "{name} code 0");
    } else {
        let want = c.powf(gamma);
        assert!(
            (got - want).abs() <= GAMMA_TOLERANCE * want,
            "{name} code {code}: {got} vs {want}"
        );
    }
}

/// A 256×256 16-bit document of one layer holding every code in R, G, B
/// and A (G and B permuted, so no pixel is grey); `opaque` pins alpha.
fn every_code(profile: Arc<IccProfile>, opaque: bool) -> (PrecisionDocument, Vec<[u16; 4]>) {
    let samples: Vec<[u16; 4]> = (0..65536_u32)
        .map(|i| {
            [
                i as u16,
                (65535 - i) as u16,
                (i.wrapping_mul(7) % 65536) as u16,
                if opaque {
                    65535
                } else {
                    (i.wrapping_mul(3) % 65536) as u16
                },
            ]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(256, 256, samples.clone(), profile, None).unwrap();
    let doc =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    (doc, samples)
}

/// A 16×16 byte document holding every byte code in R, G, B and A.
fn every_byte(profile: Arc<IccProfile>) -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_fn(16, 16, |x, y| {
        let i = y * 16 + x;
        Rgba([
            i as u8,
            255 - i as u8,
            (i * 7 % 256) as u8,
            (i * 3 % 256) as u8,
        ])
    }));
    doc.profile = profile;
    doc
}

// ----------------------------------------------------------- (a) oracle --

#[test]
fn every_16_bit_code_decodes_through_the_profiles_published_curve() {
    for (name, profile, gamma) in builtins() {
        let (doc, samples) = every_code(profile, false);
        let float = float16(&doc, None).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(float.is_float());
        assert_eq!(float.tone_map(), Some(ToneMap::Clip));
        assert_eq!(float.profile().bytes(), doc.profile().bytes());
        for (code, got) in samples.iter().zip(floats(&float, 0)) {
            for c in 0..3 {
                assert_decoded(name, gamma, code[c], got[c]);
            }
            // Alpha is coverage: code / 65535, never decoded.
            assert_eq!(got[3], f32::from(code[3]) / 65535.0, "{name} alpha");
        }
    }
    // An 8-bit 128 in sRGB is 128 · 257 decoded.
    let mut byte = DrDocument::from_pixels(RgbaImage::from_pixel(2, 2, Rgba([128, 128, 128, 255])));
    byte.profile = IccProfile::srgb();
    let float = float8(&byte, None).unwrap();
    for pixel in floats(&float, 0) {
        assert!(
            (f64::from(pixel[0]) - 0.21586).abs() <= SRGB_TOLERANCE,
            "{pixel:?}"
        );
        assert_eq!(pixel[3], 1.0);
    }
}

// ------------------------------------------------------ (b) round trip --

#[test]
fn the_round_trip_back_returns_every_code_exactly() {
    for (name, profile, _) in builtins() {
        // Per record (the structure narrowing), every code in R, G, B and A.
        let (doc, samples) = every_code(profile.clone(), false);
        let float = float16(&doc, None).unwrap();
        let mut err = ptr::null_mut();
        let back = typed_of(
            unsafe {
                dr_precision_doc_narrow_structure_to_u16(&float, LIMIT, 0, ptr::null(), &mut err)
            },
            err,
        )
        .unwrap();
        assert_eq!(codes(&back, 0), samples, "{name}: structure narrowing");
        // Flattened (`float_to_u16`, through the render), opaque.
        let (opaque, samples) = every_code(profile.clone(), true);
        let float = float16(&opaque, None).unwrap();
        let mut err = ptr::null_mut();
        let back = typed_of(
            unsafe { dr_precision_doc_float_to_u16(&float, LIMIT, 0, ptr::null(), &mut err) },
            err,
        )
        .unwrap();
        assert_eq!(codes(&back, 0), samples, "{name}: float_to_u16");
        // Every byte code through 8 → float → 8 (dither none).
        let byte = every_byte(profile);
        let float = float8(&byte, None).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_narrow_structure_to_u8(&float, 0, LIMIT, 0, ptr::null(), &mut err)
        };
        assert!(owned(err).is_none(), "{name}");
        assert!(!out.is_null(), "{name}");
        let back = *unsafe { Box::from_raw(out) };
        assert_eq!(
            back.layers[0].pixels.contiguous_image().as_raw(),
            byte.layers[0].pixels.contiguous_image().as_raw(),
            "{name}: 8 → float → 8"
        );
    }
}

// ------------------------------------------------------- (c) carriage --

fn pixels(w: u32, h: u32, seed: u32, alpha: bool) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let a = if alpha {
            ((x * 23 + y * 41 + seed * 7) % 256) as u8
        } else {
            255
        };
        Rgba([
            ((x * 13 + seed * 29) % 256) as u8,
            ((y * 17 + seed * 11) % 256) as u8,
            ((x + y + seed * 5) * 9 % 256) as u8,
            a,
        ])
    })
}

fn fresh(image: RgbaImage, name: &str) -> Layer {
    let mut layer = DrDocument::from_pixels(image).layers.remove(0);
    layer.name = name.into();
    layer
}

fn group(name: &str, blend: BlendMode, open: bool) -> Layer {
    let mut layer = fresh(RgbaImage::new(1, 1), name);
    layer.kind = LayerKind::Group;
    layer.blend = blend;
    layer.open = open;
    layer
}

fn nested(mut layer: Layer, depth: u16) -> Layer {
    layer.depth = depth;
    layer
}

const STYLE: &str = r#"{"fill_opacity":0.8,"knockout":"shallow","blend_if":{"channel":"gray","this_layer":[0,0,200,255],"underlying":[0,30,255,255]},"effects":[{"type":"drop_shadow","blend":"normal","use_global_light":false,"distance":3,"size":2}]}"#;
const LEVELS: &str = r#"{"type":"adjust","op":"levels","params":{"gamma":0.8}}"#;

/// Every record kind the float mode holds, at mixed levels, in Display P3.
fn carriage_bytes() -> DrDocument {
    let mut doc = DrDocument::from_pixels(pixels(W, H, 1, false));
    doc.layers[0].name = "Background".into();
    let mut masked = fresh(pixels(17, 11, 2, true), "Masked");
    masked.offset = (5, 4);
    masked.opacity = 0.8;
    masked.blend = BlendMode::Multiply;
    masked.mask = Some(Arc::new(GrayImage::from_fn(17, 11, |x, y| {
        Luma([((x * 15 + y * 7) % 256) as u8])
    })));
    masked.mask_properties = MaskProperties::at((5, 4));
    masked.mask_properties.feather = 2.0;
    masked.label = 3;
    masked.locks = LOCK_POSITION;
    masked.link = 1;
    let mut clipped = fresh(pixels(12, 9, 3, true), "Clipped");
    clipped.offset = (8, 6);
    clipped.clipped = true;
    clipped.link = 1;
    let child_a = nested(fresh(pixels(W, H, 4, true), "Child A"), 1);
    let mut child_b = nested(fresh(pixels(20, 12, 5, true), "Child B"), 1);
    child_b.offset = (-3, 10);
    let isolated = group("Isolated", BlendMode::Normal, false);
    let pass_child = nested(fresh(pixels(25, 20, 6, true), "Pass Child"), 1);
    let mut levels = nested(fresh(RgbaImage::new(1, 1), "Levels"), 1);
    levels.meta = Some(LEVELS.into());
    levels.opacity = 0.6;
    let mut pass = group("Pass", BlendMode::PassThrough, true);
    pass.label = 5;
    let mut title = fresh(pixels(14, 6, 7, true), "Title");
    title.offset = (2, 20);
    title.meta = Some(r#"{"type":"text","version":1,"string":"Hi"}"#.into());
    let mut styled = fresh(pixels(16, 12, 8, true), "Styled");
    styled.offset = (18, 9);
    styled.style = Some(Arc::new(LayerStyle::from_json(STYLE).unwrap()));
    styled.locks = LOCK_PIXELS;
    doc.layers.extend([
        masked, clipped, child_a, child_b, isolated, pass_child, levels, pass, title, styled,
    ]);
    doc = doc
        .add_channel(
            "Saved",
            &(0..W * H).map(|i| (i % 256) as u8).collect::<Vec<_>>(),
            W,
            H,
            [0, 128, 255],
            0.25,
        )
        .unwrap();
    doc = doc
        .edit_layer_comps(
            &CompRequest::parse(r#"{"op":"layer_comp","action":"save","name":"Comp A"}"#).unwrap(),
        )
        .unwrap()
        .unwrap();
    doc = doc.add_guide(GuideOrientation::Vertical, 12.5).unwrap();
    doc = doc.add_guide(GuideOrientation::Horizontal, 7.0).unwrap();
    doc = doc.set_ruler_origin(3.0, 4.0).unwrap();
    doc.profile = IccProfile::display_p3();
    doc.resolution = Resolution { x: 240.0, y: 180.0 };
    doc.metadata.xmp = Some(Arc::from(b"captured XMP".as_slice()));
    doc
}

const ORIGINAL: &[u8] = b"II*\0camera original for the float promote tests\0\xff\x89\x11";

fn raw_descriptor() -> Value {
    json!({"schema_version":1,"original_filename":"camera.dng",
        "content_sha256":"b".repeat(64),
        "decoder":{"identifier":"test.decoder","version":"1.0","implementation":"test-platform"},
        "recipe":{"schema_version":1,"exposure":0.0,"white_balance":[5500.0,0.0],
            "output_space":"display_p3"}})
}

fn raw_length(doc: &PrecisionDocument, layer: usize) -> usize {
    let mut err = ptr::null_mut();
    let length = unsafe { dr_precision_doc_raw_source_length(doc, layer, &mut err) };
    assert!(owned(err).is_none());
    length
}

/// The carriage fixture at 16 bits, with a vector mask on the Background, a
/// saved path and a RAW-bound record on top (`float_raw_source.rs`'s shape).
fn carriage16() -> PrecisionDocument {
    let sixteen = carriage_bytes().promote_structure(LIMIT, 0, None).unwrap();
    let shape = json!({"fill_rule":"nonzero","subpaths":[{"closed":true,
        "anchors":[[4.0,3.0],[30.0,5.0],[26.0,24.0],[6.0,20.0]]}]});
    let text = CString::new(shape.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let masked = typed_of(
        unsafe {
            dr_precision_doc_set_vector_mask(
                &sixteen,
                0,
                text.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        },
        err,
    )
    .unwrap();
    let paths = json!({"paths":[{"id":0,"name":"Outline","kind":"saved","fill_rule":"nonzero",
        "subpaths":[{"closed":true,"anchors":[[2.0,2.0],[20.0,2.0],[20.0,14.0]]}]}]});
    let text = CString::new(paths.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let pathed = typed_of(
        unsafe { dr_precision_doc_with_paths_json(&masked, text.as_ptr(), &mut err) },
        err,
    )
    .unwrap();
    let master = typed_master(
        5,
        4,
        (0..20_u32)
            .map(|i| {
                [
                    (i * 3001) as u16,
                    (i * 7919) as u16,
                    65000 - i as u16 * 127,
                    65535,
                ]
            })
            .collect(),
        IccProfile::display_p3(),
    );
    let placed = mixed(&pathed, None, Some(&master), place("camera", 2, 3))
        .unwrap()
        .unwrap();
    let top = placed.layer_count() - 1;
    let text = CString::new(raw_descriptor().to_string()).unwrap();
    let mut err = ptr::null_mut();
    let attached = typed_of(
        unsafe {
            dr_precision_doc_attach_raw_source(
                &placed,
                top,
                ORIGINAL.as_ptr(),
                ORIGINAL.len(),
                text.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        },
        err,
    )
    .unwrap();
    assert!(raw_length(&attached, top) > 0);
    attached
}

fn vector_mask(doc: &PrecisionDocument, layer: usize) -> Option<String> {
    owned(unsafe { dr_precision_doc_vector_mask_json(doc, layer) })
}

fn assert_carried(source: &PrecisionDocument, float: &PrecisionDocument) {
    assert_eq!(float.layer_count(), source.layer_count());
    for i in 0..source.layer_count() {
        let (a, b) = (source.layer(i).unwrap(), float.layer(i).unwrap());
        assert_eq!(float.layer_id(i), source.layer_id(i), "id {i}");
        assert_eq!(b.name(), a.name(), "name {i}");
        assert_eq!(b.opacity(), a.opacity(), "opacity {i}");
        assert_eq!(b.visible(), a.visible(), "visible {i}");
        assert_eq!(b.clipped(), a.clipped(), "clipped {i}");
        assert_eq!(b.label(), a.label(), "label {i}");
        assert_eq!(b.locks(), a.locks(), "locks {i}");
        assert_eq!(b.link(), a.link(), "link {i}");
        assert_eq!(b.open(), a.open(), "open {i}");
        assert_eq!(b.blend_mode(), a.blend_mode(), "blend {i}");
        assert_eq!(b.offset(), a.offset(), "offset {i}");
        assert_eq!(b.is_pass_through(), a.is_pass_through(), "pass through {i}");
        assert_eq!(
            b.children().map(<[_]>::len),
            a.children().map(<[_]>::len),
            "children {i}"
        );
        assert_eq!(
            b.mask().map(|m| m.plane().samples().to_vec()),
            a.mask().map(|m| m.plane().samples().to_vec()),
            "mask samples {i}"
        );
        assert!(
            b.mask().map(|m| m.properties()) == a.mask().map(|m| m.properties()),
            "mask properties {i}"
        );
        assert_eq!(
            b.style().map(|s| s.to_json()),
            a.style().map(|s| s.to_json()),
            "style {i}"
        );
        assert_eq!(b.blend_if(), a.blend_if(), "blend if {i}");
        assert_eq!(
            float.artwork_description(i),
            source.artwork_description(i),
            "description {i}"
        );
        assert_eq!(
            float.adjustment_description(i),
            source.adjustment_description(i),
            "adjustment {i}"
        );
        assert_eq!(
            vector_mask(float, i),
            vector_mask(source, i),
            "vector mask {i}"
        );
        // Every raster keeps its extent.
        assert_eq!(
            b.source().map(PrecisionImage::dimensions),
            a.source().map(PrecisionImage::dimensions),
            "extent {i}"
        );
    }
    assert_eq!(float.channels().len(), source.channels().len());
    for (a, b) in source.channels().iter().zip(float.channels()) {
        assert_eq!(b.name(), a.name());
        assert_eq!(b.plane().samples(), a.plane().samples());
    }
    assert_eq!(float.layer_comps(), source.layer_comps());
    assert_eq!(float.guides(), source.guides());
    assert_eq!(float.ruler_origin(), source.ruler_origin());
    assert_eq!(float.global_light(), source.global_light());
    assert_eq!(float.resolution().x, source.resolution().x);
    assert_eq!(float.metadata(), source.metadata());
    assert_eq!(float.profile().bytes(), source.profile().bytes());
    assert_eq!(
        owned(unsafe { dr_precision_doc_paths_json(float) }),
        owned(unsafe { dr_precision_doc_paths_json(source) })
    );
}

#[test]
fn every_record_and_the_chrome_are_carried_into_float() {
    let sixteen = carriage16();
    let plan = plan16(&sixteen).unwrap();
    assert_eq!(plan["direction"], "rgba16_to_rgba32_float");
    assert_eq!(plan["flattened"], false);
    assert_eq!(plan["structure"], "preserve");
    assert_eq!(
        plan["preserved"][0],
        "raster_pixels_decoded_to_linear_light"
    );
    assert!(!plan["preserved"]
        .as_array()
        .unwrap()
        .contains(&json!("evaluation_policy")));
    assert_eq!(plan["decoding"], "profile_trc_to_linear_light");
    assert_eq!(plan["tone_map"], json!({"operator":"clip"}));
    assert!(plan.get("rgb_quantization").is_none());
    assert!(plan.get("widening").is_none());
    assert_eq!(plan["blocked"], json!([]), "{plan}");
    let rows = notes(&plan, "rendering_differences", "note");
    let count = sixteen.layer_count() as u64;
    let has = |index: u64, note: &str| rows.iter().any(|(i, n)| *i == index && n == note);
    assert!(has(count, "composites in linear light; this document composited encoded values, so blended or partly opaque layers render differently"));
    assert!(has(7, "applies its 32-bit float form in linear light"));
    assert!(has(10, "style and Blend If composite in linear light"));
    assert!(has(
        9,
        "text or shape keeps its description; it re-renders at 32 bits on the next edit"
    ));
    assert!(has(
        count - 1,
        "source re-renders into linear light on the next edit"
    ));
    let float = float16(&sixteen, None).unwrap();
    assert!(float.is_float());
    assert_carried(&sixteen, &float);
    let top = sixteen.layer_count() - 1;
    assert!(
        raw_length(&float, top) > 0,
        "the RAW record keeps its original"
    );
    // The 8-bit fixture reaches the same document's records.
    let bytes = carriage_bytes();
    let plan = plan8(&bytes).unwrap();
    assert_eq!(plan["direction"], "rgba8_to_rgba32_float");
    assert_eq!(plan["blocked"], json!([]), "{plan}");
    let float = float8(&bytes, None).unwrap();
    assert_carried(&bytes.promote_structure(LIMIT, 0, None).unwrap(), &float);
    // The output: 16 B/px of every converting raster.
    let pixels: u64 = (0..sixteen.layer_count())
        .filter_map(|i| sixteen.layer_source(i))
        .map(|s| u64::from(s.dimensions().0) * u64::from(s.dimensions().1))
        .sum();
    assert_eq!(
        plan16(&sixteen).unwrap()["output_bytes"].as_u64().unwrap(),
        16 * pixels
    );
}

// --------------------------------------------------- (d) refusal by name --

fn warp_field(layer: usize) -> Value {
    let mut field = warp_fixture::field(3, 3, "bicubic");
    field["layer"] = json!(layer);
    field
}

/// A warped Background, a Screen layer, an Outer Glow in Screen and an
/// Invert layer, at 8 bits.
fn refused_bytes() -> DrDocument {
    let mut doc = DrDocument::from_pixels(pixels(W, H, 21, true));
    doc.layers[0].name = "Warped".into();
    let mut screen = fresh(pixels(10, 8, 22, true), "Screen");
    screen.blend = BlendMode::Screen;
    let mut glow = fresh(pixels(10, 8, 23, true), "Glow");
    glow.style = Some(Arc::new(
        LayerStyle::from_json(r#"{"effects":[{"type":"outer_glow","blend":"screen"}]}"#).unwrap(),
    ));
    let mut invert = fresh(RgbaImage::new(1, 1), "Invert");
    invert.meta = Some(r#"{"type":"adjust","op":"invert","params":{}}"#.into());
    doc.layers.extend([screen, glow, invert]);
    doc.warp_edit(&warp_fixture::edit(warp_field(0)), LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

#[test]
fn what_float_cannot_hold_blocks_by_name_with_the_input_untouched() {
    let bytes = refused_bytes();
    let sixteen = bytes.promote_structure(LIMIT, 0, None).unwrap();
    assert!(sixteen.warp_info(0).unwrap()["field"].is_object());
    let before = sixteen.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    for (label, plan, failure) in [
        (
            "16",
            plan16(&sixteen).unwrap(),
            float16(&sixteen, None).err(),
        ),
        ("8", plan8(&bytes).unwrap(), float8(&bytes, None).err()),
    ] {
        let blocked = notes(&plan, "blocked", "reason");
        assert_eq!(blocked.len(), 4, "{label}: {plan}");
        assert_eq!(blocked[0], (0, WARP_REFUSAL.to_string()), "{label}");
        assert_eq!(blocked[1].0, 1);
        assert!(
            blocked[1].1.to_lowercase().contains("screen"),
            "{label}: {}",
            blocked[1].1
        );
        assert_eq!(blocked[2].0, 2);
        assert!(
            blocked[2].1.to_lowercase().contains("screen"),
            "{label}: {}",
            blocked[2].1
        );
        assert_eq!(blocked[3].0, 3);
        assert!(
            blocked[3].1.to_lowercase().contains("invert"),
            "{label}: {}",
            blocked[3].1
        );
        assert_eq!(plan["blocked"][1]["name"], "Screen");
        assert_eq!(failure, Some(first_refusal(&plan)), "{label}");
        // The capacity row never runs once something blocked.
        assert!(!blocked.iter().any(|(_, n)| n.contains("save and reopen")));
    }
    assert_eq!(
        sixteen.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap(),
        before
    );
    assert!(sixteen.warp_info(0).unwrap()["field"].is_object());

    // A profile with no matrix/TRC model: one document row, id 0.
    use crate::icc_lut_builders::{build, tag_desc, tag_mft2, tag_wtpt};
    let lut = Arc::new(
        IccProfile::parse(&build(
            [0x02, 0x20, 0, 0],
            b"mntr",
            b"RGB ",
            b"XYZ ",
            &[
                (b"desc", tag_desc("Synthetic LUT RGB")),
                (b"wtpt", tag_wtpt([0.9642, 1.0, 0.8249])),
                (b"A2B0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
                (b"B2A0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
            ],
        ))
        .unwrap(),
    );
    assert!(lut.model().is_none());
    let doc = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(4, 4, vec![[9000, 30000, 50000, 65535]; 16], lut, None)
            .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let plan = plan16(&doc).unwrap();
    assert_eq!(plan["blocked"].as_array().unwrap().len(), 1, "{plan}");
    assert_eq!(plan["blocked"][0]["id"], 0);
    assert_eq!(plan["blocked"][0]["index"], 1);
    assert_eq!(float16(&doc, None).err(), Some(first_refusal(&plan)));
    // A float source is an error, not a row.
    let float = float16(&every_code(IccProfile::srgb(), true).0, None).unwrap();
    assert_eq!(
        plan16(&float).err().as_deref(),
        Some("the document already stores 32-bit float pixels")
    );
    assert_eq!(
        float16(&float, None).err().as_deref(),
        Some("the document already stores 32-bit float pixels")
    );
}

// ------------------------------------------------- (e) stale bindings --

#[test]
fn a_stale_warp_converts_and_is_dropped() {
    let warped = warp_fixture::typed_document()
        .warp_edit(&warp_fixture::edit(warp_field(0)), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert!(warped.warp_info(0).unwrap()["field"].is_object());
    let erase = PaintEdit::parse(r#"{"op":"erase","layer":0,"opacity":1.0}"#).unwrap();
    let stale = warped
        .paint(
            &erase,
            &vec![30000; (warp_fixture::W * warp_fixture::H) as usize],
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    // A typed rebuild drops a binding its guard no longer accepts
    // (`retained_source::clean_typed_layers`), so the painted record holds
    // none: it converts with no blocked row.
    assert!(stale.warp_info(0).unwrap()["field"].is_null());
    let plan = plan16(&stale).unwrap();
    assert_eq!(plan["blocked"], json!([]), "{plan}");
    // The effective warp, by contrast, blocks.
    assert_eq!(
        notes(&plan16(&warped).unwrap(), "blocked", "reason"),
        vec![(0, WARP_REFUSAL.to_string())]
    );
    let float = float16(&stale, None).unwrap();
    // The result holds no warp: narrowing it back would block one by name.
    let mut err = ptr::null_mut();
    let back = json_of(
        unsafe { dr_precision_doc_narrow_structure_plan(&float, 16, &mut err) },
        err,
    )
    .unwrap();
    assert_eq!(back["blocked"], json!([]), "{back}");

    // A byte document keeps a stale binding until its next mutation: the
    // warped pixels replaced after the warp. The 8→float plan discloses it
    // once, blocks nothing, and the result holds no warp.
    let mut stale8 = warp_fixture::byte_document()
        .warp_edit(&warp_fixture::edit(warp_field(0)), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert!(stale8.warp_info(0).unwrap()["field"].is_object());
    stale8.layers[0].pixels =
        doctored_core::doc::LayerRaster::new(pixels(warp_fixture::W, warp_fixture::H, 40, true));
    assert!(stale8.warp_info(0).unwrap()["field"].is_null());
    let plan = plan8(&stale8).unwrap();
    assert_eq!(plan["blocked"], json!([]), "{plan}");
    let rows = notes(&plan, "rendering_differences", "note");
    assert_eq!(
        rows.iter().filter(|(i, n)| *i == 0 && n == STALE).count(),
        1,
        "{plan}"
    );
    let float = float8(&stale8, None).unwrap();
    let mut err = ptr::null_mut();
    let back = json_of(
        unsafe { dr_precision_doc_narrow_structure_plan(&float, 16, &mut err) },
        err,
    )
    .unwrap();
    assert_eq!(back["blocked"], json!([]), "{back}");

    // 8 bits: a Blend If layer and a placed layer take the float rows, not
    // the 16-bit ones.
    let mut doc = DrDocument::from_pixels(pixels(W, H, 31, false));
    let mut blended = fresh(pixels(12, 9, 32, true), "Blended");
    blended.style = Some(Arc::new(
        LayerStyle::from_json(
            r#"{"blend_if":{"channel":"gray","this_layer":[0,0,200,255],"underlying":[0,30,255,255]}}"#,
        )
        .unwrap(),
    ));
    doc.layers.push(blended);
    let master = DrDocument::from_pixels(pixels(6, 4, 33, false));
    let place = SourceEdit::parse(
        &json!({"op":"place","source_id":"487-master","revision_id":"v1",
                "name":"Placed","x":2,"y":1})
        .to_string(),
    )
    .unwrap();
    let doc = doc
        .source_edit(Some(&master), &place, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let plan = plan8(&doc).unwrap();
    assert_eq!(plan["blocked"], json!([]), "{plan}");
    let rows = notes(&plan, "rendering_differences", "note");
    assert!(!rows
        .iter()
        .any(|(_, n)| n == "Blend If evaluates on the 16-bit ramp (10D)"
            || n.starts_with("source re-renders at 16 bits")
            || n.starts_with("warp re-edits at 16 bits")));
    assert!(rows
        .iter()
        .any(|(i, n)| *i == 1 && n == "style and Blend If composite in linear light"));
    assert!(rows
        .iter()
        .any(|(i, n)| *i == 2 && n == "source re-renders into linear light on the next edit"));
    let float = float8(&doc, None).unwrap();
    assert!(float.layer(2).unwrap().effective_source().is_some());
}

// ---------------------------------------------- (f) new-document fills --

#[test]
fn a_blank_promotes_to_exact_white_black_and_the_decoded_colour() {
    let colour = [0x33_u16 * 257, 0x66 * 257, 0x99 * 257, 65535];
    for (fill, want) in [
        ([65535_u16; 4], [1.0_f64; 4]),
        ([0, 0, 0, 65535], [0.0, 0.0, 0.0, 1.0]),
        ([0; 4], [0.0; 4]),
        (
            colour,
            [
                srgb_eotf(f64::from(colour[0]) / 65535.0),
                srgb_eotf(f64::from(colour[1]) / 65535.0),
                srgb_eotf(f64::from(colour[2]) / 65535.0),
                1.0,
            ],
        ),
    ] {
        let image =
            PrecisionImage::from_rgba16(8, 6, vec![fill; 48], IccProfile::srgb(), None).unwrap();
        let blank =
            PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
                .unwrap();
        let float = float16(&blank, None).unwrap();
        for pixel in floats(&float, 0) {
            for c in 0..4 {
                if fill == colour && c < 3 {
                    assert!(
                        (f64::from(pixel[c]) - want[c]).abs() <= SRGB_TOLERANCE,
                        "{fill:?}: {pixel:?}"
                    );
                } else {
                    assert_eq!(f64::from(pixel[c]), want[c], "{fill:?}: {pixel:?}");
                }
            }
        }
    }
}

// ------------------------------------- (g) cancellation and progress --

#[test]
fn a_cancelled_control_refuses_and_a_run_reports_to_completion() {
    let (sixteen, _) = every_code(IccProfile::srgb(), true);
    let bytes = carriage_bytes();
    let token = DrJobControl::default();
    token.cancel();
    assert_eq!(
        float16(&sixteen, Some(&token)).err().as_deref(),
        Some("operation cancelled")
    );
    assert_eq!(token.scratch_bytes(), 0);
    let token = DrJobControl::default();
    token.cancel();
    assert_eq!(
        float8(&bytes, Some(&token)).err().as_deref(),
        Some("operation cancelled")
    );
    for run in 0..2 {
        let token = DrJobControl::default();
        if run == 0 {
            float16(&sixteen, Some(&token)).unwrap();
        } else {
            float8(&bytes, Some(&token)).unwrap();
        }
        assert_eq!(token.progress(), 1.0, "run {run}");
        assert_eq!(token.scratch_bytes(), 0);
    }
}

// ----------------------------------------------------- (h) NULL safety --

#[test]
fn the_four_exports_are_null_safe() {
    unsafe {
        assert!(dr_precision_doc_promote_to_float_plan(ptr::null(), ptr::null_mut()).is_null());
        assert!(dr_doc_promote_to_float_plan(ptr::null(), ptr::null_mut()).is_null());
        assert!(dr_precision_doc_promote_to_float(
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(
            dr_doc_promote_to_float(ptr::null(), LIMIT, 0, ptr::null(), ptr::null_mut()).is_null()
        );
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_promote_to_float_plan(ptr::null(), &mut err).is_null());
        assert!(owned(err).is_some());
        let mut err = ptr::null_mut();
        assert!(dr_doc_promote_to_float(ptr::null(), LIMIT, 0, ptr::null(), &mut err).is_null());
        assert!(owned(err).is_some());
    }
}
