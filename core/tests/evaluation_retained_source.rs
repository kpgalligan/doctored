//! Linear-light evaluation admits a retained source, at 8
//! and 16 bits.
//!
//! Oracles, none read off the implementation: the composite is the f64
//! W3C Normal step in linear light over the analytic sRGB curve written here
//! (11E's gates: 1 code at byte depth, 24 of 65535 typed); the re-render
//! domain is the per-channel weight of a two-colour edge, which is one number
//! across R, G and B only in the domain the resample ran in — the STORED
//! encoding, the domain raster Free Transform and warp use under linear light
//! — and the result must equal the encoded twin's bit for bit.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_evaluation::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_quantize::PrecisionDither;
use doctored_core::retained_source::{SourceEdit, SourceInput};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const LINEAR: i32 = 1;
const TYPED_GATE: i64 = 24;
const BACKDROP: [u8; 3] = [200, 40, 90];
const SOURCE: [u8; 3] = [20, 180, 240];
const LEFT: [u8; 3] = [16, 200, 100];
const RIGHT: [u8; 3] = [240, 40, 180];

// ------------------------------------------------------------- the oracle --

fn dec(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

fn enc(linear: f64) -> f64 {
    let linear = linear.clamp(0.0, 1.0);
    if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// Normal at `alpha` over an opaque backdrop, in linear light, encoded.
fn over(source: f64, backdrop: f64, alpha: f64) -> f64 {
    enc(alpha * dec(source) + (1.0 - alpha) * dec(backdrop))
}

// ------------------------------------------------------------- plumbing --

fn take_err(err: *mut c_char) -> Option<String> {
    if err.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    Some(text)
}

fn parsed(value: Value) -> SourceEdit {
    SourceEdit::parse(&value.to_string()).unwrap()
}

fn place() -> SourceEdit {
    parsed(
        json!({"op":"place","source_id":"master","revision_id":"v1","name":"Placed","x":2,"y":0}),
    )
}

fn transform() -> SourceEdit {
    parsed(json!({"op":"transform","layer":1,"matrix":[3,0,0,1,1,1],"filter":"bilinear"}))
}

fn byte_doc(w: u32, h: u32, rgb: [u8; 3]) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_pixel(
        w,
        h,
        Rgba([rgb[0], rgb[1], rgb[2], 255]),
    ))
}

fn byte_edge() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(2, 1, |x, _| {
        let c = if x == 0 { LEFT } else { RIGHT };
        Rgba([c[0], c[1], c[2], 255])
    }))
}

fn wide(rgb: [u8; 3]) -> [u16; 4] {
    [
        u16::from(rgb[0]) * 257,
        u16::from(rgb[1]) * 257,
        u16::from(rgb[2]) * 257,
        65535,
    ]
}

fn typed_doc(w: u32, h: u32, pixel: impl Fn(u32) -> [u16; 4]) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            w,
            h,
            (0..w * h).map(pixel).collect(),
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

fn typed_edge() -> PrecisionDocument {
    typed_doc(2, 1, |i| wide(if i == 0 { LEFT } else { RIGHT }))
}

fn byte_switch(doc: &DrDocument, policy: i32) -> Result<DrDocument, String> {
    let handle = Box::into_raw(Box::new(doc.clone()));
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_set_evaluation(handle, policy, &mut err) };
    unsafe { dr_doc_free(handle) };
    match take_err(err) {
        Some(message) => Err(message),
        None => {
            assert!(!out.is_null());
            Ok(*unsafe { Box::from_raw(out) })
        }
    }
}

fn typed_switch(doc: &PrecisionDocument, policy: i32) -> Result<PrecisionDocument, String> {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_evaluation(doc, policy, &mut err) };
    match take_err(err) {
        Some(message) => Err(message),
        None => {
            assert!(!out.is_null(), "the policy changes");
            Ok(*unsafe { Box::from_raw(out) })
        }
    }
}

fn byte_edit(doc: &DrDocument, input: Option<&DrDocument>, edit: &SourceEdit) -> DrDocument {
    doc.source_edit(input, edit, LIMIT, 0, None)
        .unwrap()
        .expect("the edit changes the document")
}

fn typed_edit(
    doc: &PrecisionDocument,
    input: Option<&PrecisionDocument>,
    edit: &SourceEdit,
) -> PrecisionDocument {
    doc.source_edit_input(input.map(SourceInput::Typed), edit, LIMIT, 0, None)
        .unwrap()
        .expect("the edit changes the document")
}

fn byte_pixels(doc: &DrDocument, index: usize) -> Vec<[u8; 4]> {
    doc.layers[index]
        .pixels
        .contiguous_image()
        .pixels()
        .map(|p| p.0)
        .collect()
}

fn typed_pixels(doc: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    doc.layer_source(index).unwrap().to_rgba16().unwrap()
}

/// The byte placed fixture: an 8×4 backdrop, a 4×4 master at x 2, opacity 0.5.
fn byte_placed() -> DrDocument {
    let master = byte_doc(4, 4, SOURCE);
    let placed = byte_edit(&byte_doc(8, 4, BACKDROP), Some(&master), &place());
    placed
        .with_layer_opacity(1, 0.5)
        .expect("the opacity changes")
}

fn typed_placed() -> PrecisionDocument {
    let master = typed_doc(4, 4, |_| wide(SOURCE));
    let placed = typed_edit(
        &typed_doc(8, 4, |_| wide(BACKDROP)),
        Some(&master),
        &place(),
    );
    placed
        .set_layer_opacity(1, 0.5)
        .unwrap()
        .expect("the opacity changes")
}

// ------------------------------------------------------------- the tests --

#[test]
fn a_byte_placed_image_switches_to_linear_light_and_composites_there() {
    let doc = byte_placed();
    let linear = byte_switch(&doc, LINEAR).expect("a placed image is admitted");
    assert!(linear.linear_light_refusal().is_none());
    let flat = linear.flattened().unwrap();
    for (x, y, pixel) in flat.enumerate_pixels() {
        // The layer's 50 % at its byte, 128 / 255 (#2).
        let alpha = if (2..6).contains(&x) {
            crate::common::byte_unit(0.5)
        } else {
            0.0
        };
        for c in 0..3 {
            let want = (255.0
                * over(
                    f64::from(SOURCE[c]) / 255.0,
                    f64::from(BACKDROP[c]) / 255.0,
                    alpha,
                ))
            .round();
            let got = f64::from(pixel.0[c]);
            assert!(
                (got - want).abs() <= 1.0,
                "({x}, {y}) channel {c}: {got} vs {want}"
            );
        }
    }
    let raster = byte_edit(&doc, None, &parsed(json!({"op":"rasterize","layer":1})));
    let raster = byte_switch(&raster, LINEAR).unwrap();
    assert_eq!(
        flat,
        raster.flattened().unwrap(),
        "the binding changes nothing"
    );
}

#[test]
fn a_typed_placed_image_switches_to_linear_light_and_composites_there() {
    let doc = typed_placed();
    let linear = typed_switch(&doc, LINEAR).expect("a placed image is admitted");
    assert!(linear.linear_light_refusal().is_none());
    let samples = linear.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    for (i, pixel) in samples.iter().enumerate() {
        let x = i as u32 % 8;
        // The layer's 50 % at its byte, 128 / 255 (#2).
        let alpha = if (2..6).contains(&x) {
            crate::common::byte_unit(0.5)
        } else {
            0.0
        };
        for (c, got) in pixel.iter().take(3).enumerate() {
            let s = f64::from(wide(SOURCE)[c]) / 65535.0;
            let b = f64::from(wide(BACKDROP)[c]) / 65535.0;
            let want = (65535.0 * over(s, b, alpha)).round() as i64;
            let got = i64::from(*got);
            assert!(
                (got - want).abs() <= TYPED_GATE,
                "pixel {i} channel {c}: {got} vs {want}"
            );
        }
    }
    let raster = typed_edit(&doc, None, &parsed(json!({"op":"rasterize","layer":1})));
    let raster = typed_switch(&raster, LINEAR).unwrap();
    assert_eq!(
        samples,
        raster.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap(),
        "the binding changes nothing"
    );
}

/// `(encoded weights, linear weights)` of `pixel` between the edge's colours.
fn weights(pixel: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let (a, b) = (
        LEFT.map(|v| f64::from(v) / 255.0),
        RIGHT.map(|v| f64::from(v) / 255.0),
    );
    (
        std::array::from_fn(|c| (pixel[c] - a[c]) / (b[c] - a[c])),
        std::array::from_fn(|c| (dec(pixel[c]) - dec(a[c])) / (dec(b[c]) - dec(a[c]))),
    )
}

fn spread(w: [f64; 3]) -> f64 {
    w.iter().cloned().fold(f64::MIN, f64::max) - w.iter().cloned().fold(f64::MAX, f64::min)
}

/// The mixed samples' (encoded spread, linear spread) along a row.
fn mixed(row: &[[f64; 3]]) -> Vec<(f64, f64)> {
    row.iter()
        .filter_map(|p| {
            let (e, l) = weights(*p);
            let mean = (e[0] + e[1] + e[2]) / 3.0;
            (0.05..0.95).contains(&mean).then(|| (spread(e), spread(l)))
        })
        .collect()
}

#[test]
fn a_re_render_resamples_in_the_stored_encoding_at_both_depths() {
    let base = byte_doc(12, 4, BACKDROP);
    let encoded = byte_edit(&base, Some(&byte_edge()), &place());
    let linear = byte_edit(
        &byte_switch(&base, LINEAR).unwrap(),
        Some(&byte_edge()),
        &place(),
    );
    let (encoded, linear) = (
        byte_edit(&encoded, None, &transform()),
        byte_edit(&linear, None, &transform()),
    );
    assert!(linear.linear_light_refusal().is_none());
    assert_eq!(byte_pixels(&linear, 1), byte_pixels(&encoded, 1));
    let row: Vec<[f64; 3]> = byte_pixels(&linear, 1)
        .iter()
        .map(|p| std::array::from_fn(|c| f64::from(p[c]) / 255.0))
        .collect();
    let samples = mixed(&row);
    assert!(samples.len() >= 2, "{samples:?}");
    assert!(samples.iter().all(|(e, _)| *e <= 0.01), "{samples:?}");
    assert!(samples.iter().any(|(_, l)| *l > 0.05), "{samples:?}");

    let base = typed_doc(12, 4, |_| wide(BACKDROP));
    let encoded = typed_edit(&base, Some(&typed_edge()), &place());
    let linear = typed_edit(
        &typed_switch(&base, LINEAR).unwrap(),
        Some(&typed_edge()),
        &place(),
    );
    let (encoded, linear) = (
        typed_edit(&encoded, None, &transform()),
        typed_edit(&linear, None, &transform()),
    );
    assert!(linear.linear_light_refusal().is_none());
    assert_eq!(typed_pixels(&linear, 1), typed_pixels(&encoded, 1));
    let row: Vec<[f64; 3]> = typed_pixels(&linear, 1)
        .iter()
        .map(|p| std::array::from_fn(|c| f64::from(p[c]) / 65535.0))
        .collect();
    let samples = mixed(&row);
    assert!(samples.len() >= 2, "{samples:?}");
    assert!(samples.iter().all(|(e, _)| *e <= 1e-3), "{samples:?}");
    assert!(samples.iter().any(|(_, l)| *l > 0.05), "{samples:?}");
}

fn byte_save(doc: &DrDocument, path: &std::path::Path) -> Vec<u8> {
    let handle = Box::into_raw(Box::new(doc.clone()));
    let c = CString::new(path.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let ok = unsafe { dr_doc_save_native(handle, c.as_ptr(), &mut err) };
    unsafe { dr_doc_free(handle) };
    assert!(ok, "{:?}", take_err(err));
    std::fs::read(path).unwrap()
}

fn byte_open(path: &std::path::Path) -> DrDocument {
    let c = CString::new(path.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_open(c.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{:?}", take_err(err));
    *unsafe { Box::from_raw(out) }
}

#[test]
fn a_linear_light_placed_document_round_trips_at_both_depths() {
    let dir = TempDir::new().unwrap();
    let doc = byte_switch(&byte_placed(), LINEAR).unwrap();
    let bytes = byte_save(&doc, &dir.path().join("a.rz"));
    let reopened = byte_open(&dir.path().join("a.rz"));
    assert_eq!(unsafe { dr_doc_evaluation(&reopened) }, LINEAR);
    assert_eq!(reopened.source_info(1).unwrap()["valid"], true);
    assert_eq!(
        reopened.source_info(1).unwrap(),
        doc.source_info(1).unwrap()
    );
    assert_eq!(byte_pixels(&reopened, 1), byte_pixels(&doc, 1));
    assert_eq!(byte_save(&reopened, &dir.path().join("b.rz")), bytes);
    let before = byte_edit(&doc, None, &transform());
    let after = byte_edit(&reopened, None, &transform());
    assert_eq!(byte_pixels(&after, 1), byte_pixels(&before, 1));
    assert_eq!(after.layers[1].offset, before.layers[1].offset);

    let doc = typed_switch(&typed_placed(), LINEAR).unwrap();
    let bytes = doc.encode_native(LIMIT, 0, None).unwrap();
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert_eq!(unsafe { dr_precision_doc_evaluation(&reopened) }, LINEAR);
    assert_eq!(reopened.source_info(1).unwrap()["valid"], true);
    assert_eq!(
        reopened.source_info(1).unwrap(),
        doc.source_info(1).unwrap()
    );
    assert_eq!(typed_pixels(&reopened, 1), typed_pixels(&doc, 1));
    assert_eq!(reopened.encode_native(LIMIT, 0, None).unwrap(), bytes);
    let before = typed_edit(&doc, None, &transform());
    let after = typed_edit(&reopened, None, &transform());
    assert_eq!(typed_pixels(&after, 1), typed_pixels(&before, 1));
    assert_eq!(
        after.layer(1).unwrap().offset(),
        before.layer(1).unwrap().offset()
    );
}

#[test]
fn depth_conversions_carry_the_binding_under_linear_light() {
    let doc = byte_switch(&byte_placed(), LINEAR).unwrap();
    let sixteen = doc.promote_structure(LIMIT, 0, None).unwrap();
    assert_eq!(unsafe { dr_precision_doc_evaluation(&sixteen) }, LINEAR);
    assert!(sixteen.layer(1).unwrap().effective_source().is_some());
    let eight = sixteen
        .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
        .unwrap();
    assert_eq!(unsafe { dr_doc_evaluation(&eight) }, LINEAR);
    assert!(eight.layers[1].effective_source(&eight.profile).is_some());
}

#[test]
fn the_operator_table_migrates_retained_sources() {
    let raw = unsafe { dr_evaluation_operators() };
    assert!(!raw.is_null());
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(raw) };
    let table: Value = serde_json::from_str(&text).unwrap();
    assert!(table["migrated"]
        .as_array()
        .unwrap()
        .contains(&json!("retained_source")));
    let refused = table["refused"].as_array().unwrap();
    assert!(!refused.contains(&json!({"operator":"a live filter","phase":2})));

    assert!(!refused.iter().any(|row| row["operator"]
        .as_str()
        .is_some_and(|s| s.contains("retained source"))));
}
