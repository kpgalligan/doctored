//! Assign Profile and Convert to Profile in a 32-bit float document,
//! through the typed colour FFI (`dr_precision_doc_color_plan`,
//! `_assign_profile`, `_convert_to_profile`, `_convert_to_profile_with`,
//! `_conversion_preflight`, `_conversion_plan`).
//!
//! Oracles, none read off the core: an Assign keeps every sample bit for bit
//! on the same storage; a Convert is the f64 matrix
//! `inv(destination.to_pcs) · source.to_pcs` over the two profiles' own
//! colorant tags (`common::icc_chain`; the samples are linear, so no curve),
//! never clamped, within 2e-6 · max(1, |want|), alpha bit for bit. Every
//! refusal sentence is written out here.
//!
//! The admission record (`admission_measure`, ignored;
//! `plans_cover_the_measured_peaks`, default) is the module doc of
//! `float_color.rs`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc_guide::GuideOrientation;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::{
    dr_precision_doc_artwork_description, dr_precision_doc_artwork_metadata,
};
use doctored_core::ffi_output_color::{
    dr_precision_doc_assign_profile_rasterizing, dr_precision_doc_conversion_preflight,
    dr_precision_doc_convert_to_profile_with,
};
use doctored_core::ffi_precision::{
    dr_precision_doc_duplicate_layer, dr_precision_image_storage_identity,
};
use doctored_core::ffi_precision_color::{
    dr_precision_doc_assign_profile, dr_precision_doc_color_plan, dr_precision_doc_conversion_plan,
    dr_precision_doc_convert_to_profile,
};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::float_document::{float_operators_json, ToneMap};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::{json, Value};

use crate::common;
use crate::encode_refusals::{bad_profile, rgb_lut_proof};
use crate::float_fixture::float_doc;
use crate::float_layer_edits::{
    footprint_reset, measure_fixture, rusage, INTERVAL_MAX_PHYS_FOOTPRINT, MLIMIT,
};
use common::icc_chain::{apply, inverse, Chain};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const TOLERANCE: f64 = 2e-6;
const ROUND_TRIP: f64 = 1e-5;

// ------------------------------------------------------------ sentences --

const LUT_ASSIGN: &str = "Doctored cannot assign \u{201c}Synthetic LUT RGB\u{201d} to a 32-bit \
     float document \u{2014} it has no matrix/TRC model, and a float document's linear values are \
     defined by its profile's primaries. Choose a matrix/TRC RGB profile.";
const LUT_CONVERT: &str = "Doctored cannot convert a 32-bit float document into \
     \u{201c}Synthetic LUT RGB\u{201d} \u{2014} it has no matrix/TRC model, and a float \
     document's linear values are defined by its profile's primaries. Choose a matrix/TRC RGB \
     profile.";
const CURVE_ASSIGN: &str = "Doctored cannot assign \u{201c}Synthetic Bad Gamma\u{201d} to a \
     32-bit float document \u{2014} its matrix/TRC tone curves fail the precision check (they \
     must be finite and monotone), and a float document's display and 8/16-bit export encode \
     through them. Choose another profile.";
const CURVE_CONVERT: &str = "Doctored cannot convert into \u{201c}Synthetic Bad Gamma\u{201d} \
     \u{2014} its matrix/TRC tone curves fail the precision check every conversion into it runs \
     (they must be finite and monotone). Choose another profile.";

fn options_sentence(what: &str) -> String {
    format!(
        "Convert to Profile in a 32-bit float document converts in linear light with no clamp \
         (relative colorimetric, no black-point compensation): {what} is unavailable there."
    )
}

fn range_sentence(name: &str) -> String {
    format!(
        "the document's colours convert to a value outside the 32-bit float range (±65504) in \
         \u{201c}{name}\u{201d}; the conversion is refused rather than clipped"
    )
}

// ------------------------------------------------------------- plumbing --

fn owned(pointer: *mut c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    Some(text)
}

type Edited = Result<Option<PrecisionDocument>, String>;

fn outcome(out: *mut PrecisionDocument, err: *mut c_char) -> Edited {
    if let Some(message) = owned(err) {
        assert!(out.is_null(), "a refusal leaves no document");
        return Err(message);
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

fn assign_at(doc: &PrecisionDocument, profile: &[u8], limit: u64) -> Edited {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_assign_profile(
            doc,
            profile.as_ptr(),
            profile.len(),
            limit,
            0,
            ptr::null(),
            &mut err,
        )
    };
    outcome(out, err)
}

fn assign(doc: &PrecisionDocument, profile: &[u8]) -> Edited {
    assign_at(doc, profile, LIMIT)
}

fn assign_rasterizing(doc: &PrecisionDocument, profile: &[u8]) -> Edited {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_assign_profile_rasterizing(
            doc,
            profile.as_ptr(),
            profile.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    outcome(out, err)
}

fn convert_at(doc: &PrecisionDocument, profile: &[u8], limit: u64) -> Edited {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_profile(
            doc,
            profile.as_ptr(),
            profile.len(),
            limit,
            0,
            ptr::null(),
            &mut err,
        )
    };
    outcome(out, err)
}

fn convert(doc: &PrecisionDocument, profile: &[u8]) -> Edited {
    convert_at(doc, profile, LIMIT)
}

fn options_text(options: &Value) -> Option<CString> {
    (!options.is_null()).then(|| CString::new(options.to_string()).unwrap())
}

fn convert_with(doc: &PrecisionDocument, profile: &[u8], options: &Value) -> Edited {
    let text = options_text(options);
    let mut err = ptr::null_mut();
    let mut report = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_profile_with(
            doc,
            profile.as_ptr(),
            profile.len(),
            text.as_ref().map_or(ptr::null(), |t| t.as_ptr()),
            LIMIT,
            0,
            ptr::null(),
            &mut report,
            &mut err,
        )
    };
    owned(report);
    outcome(out, err)
}

fn json_outcome(out: *mut c_char, err: *mut c_char) -> Result<Value, String> {
    match owned(out) {
        Some(text) => {
            assert_eq!(owned(err), None);
            Ok(serde_json::from_str(&text).expect("JSON"))
        }
        None => Err(owned(err).unwrap_or_default()),
    }
}

fn color_plan_at(
    doc: &PrecisionDocument,
    profile: &[u8],
    convert: bool,
    limit: u64,
) -> Result<Value, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_color_plan(
            doc,
            profile.as_ptr(),
            profile.len(),
            convert,
            limit,
            0,
            &mut err,
        )
    };
    json_outcome(out, err)
}

fn color_plan(doc: &PrecisionDocument, profile: &[u8], convert: bool) -> Result<Value, String> {
    color_plan_at(doc, profile, convert, LIMIT)
}

fn conversion_plan(
    doc: &PrecisionDocument,
    profile: &[u8],
    options: &Value,
) -> Result<Value, String> {
    let text = options_text(options);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_conversion_plan(
            doc,
            profile.as_ptr(),
            profile.len(),
            text.as_ref().map_or(ptr::null(), |t| t.as_ptr()),
            LIMIT,
            0,
            &mut err,
        )
    };
    json_outcome(out, err)
}

fn preflight(doc: &PrecisionDocument, profile: &[u8], options: &Value) -> Result<Value, String> {
    let text = options_text(options);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_conversion_preflight(
            doc,
            profile.as_ptr(),
            profile.len(),
            text.as_ref().map_or(ptr::null(), |t| t.as_ptr()),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    json_outcome(out, err)
}

fn edit(doc: &PrecisionDocument, request: Value, samples: &[u16]) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if samples.is_empty() {
                ptr::null()
            } else {
                samples.as_ptr()
            },
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    outcome(out, err)
        .unwrap_or_else(|message| panic!("{request}: {message}"))
        .unwrap_or_else(|| panic!("{request} reported no change"))
}

fn floats(doc: &PrecisionDocument, index: usize) -> Vec<[f32; 4]> {
    doc.layer_source(index)
        .expect("a raster record")
        .to_rgba32()
        .expect("float samples")
}

fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

fn identity(doc: &PrecisionDocument, index: usize) -> u64 {
    unsafe { dr_precision_image_storage_identity(doc.layer_source(index).expect("a raster")) }
}

fn description(doc: &PrecisionDocument, index: usize) -> Option<String> {
    owned(unsafe { dr_precision_doc_artwork_description(doc, index) })
}

/// `inv(dst.to_pcs) · src.to_pcs`, from the two profiles' colorant tags.
fn combined(src: &Arc<IccProfile>, dst: &Arc<IccProfile>) -> [[f64; 3]; 3] {
    let (src, dst) = (Chain::of(src), Chain::of(dst));
    let inv = inverse(dst.to_pcs);
    std::array::from_fn(|r| {
        std::array::from_fn(|c| (0..3).map(|k| inv[r][k] * src.to_pcs[k][c]).sum())
    })
}

fn assert_converted(got: &[[f32; 4]], source: &[[f32; 4]], matrix: [[f64; 3]; 3], what: &str) {
    assert_eq!(got.len(), source.len(), "{what}");
    for (i, (g, s)) in got.iter().zip(source).enumerate() {
        let want = apply(matrix, [s[0], s[1], s[2]].map(f64::from));
        for c in 0..3 {
            let bound = TOLERANCE * want[c].abs().max(1.0);
            assert!(
                (f64::from(g[c]) - want[c]).abs() <= bound,
                "{what} pixel {i} channel {c}: {} against {}",
                g[c],
                want[c]
            );
        }
        assert_eq!(g[3].to_bits(), s[3].to_bits(), "{what} pixel {i}: alpha");
    }
}

// ------------------------------------------------------------ the fixture --

const W: u32 = 4;
const H: u32 = 3;

/// Above white, below zero and partly covered, among ordinary values.
fn extended() -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| match i % 4 {
            0 => [8.0, 8.0, 8.0, 1.0],
            1 => [0.125, 3.5, -0.0625, 0.5],
            2 => [-0.5, 0.75, 1.25, 0.25],
            _ => [i as f32 / 16.0, 0.375, 0.875, 1.0],
        })
        .collect()
}

fn fixture(profile: &Arc<IccProfile>) -> PrecisionDocument {
    float_doc(W, H, profile, &[extended()])
}

// ----------------------------------------------------------------- assign --

#[test]
fn assign_relabels_and_keeps_every_sample_on_the_same_storage() {
    let srgb = IccProfile::srgb();
    let p3 = IccProfile::display_p3();
    let base = fixture(&srgb)
        .with_tone_map(ToneMap::Reinhard { white: 6.0 })
        .unwrap()
        .unwrap();
    let out = assign(&base, p3.bytes()).unwrap().expect("another profile");
    assert!(out.is_float());
    assert_eq!(&out.profile().bytes()[..], &p3.bytes()[..]);
    assert_eq!(bits(&floats(&out, 0)), bits(&floats(&base, 0)));
    assert_eq!(identity(&out, 0), identity(&base, 0), "the plane is shared");
    assert_eq!(out.tone_map(), Some(ToneMap::Reinhard { white: 6.0 }));
    // The plan converts nothing.
    let plan = color_plan(&base, p3.bytes(), false).unwrap();
    assert_eq!(plan["converted_pixel_bytes"], json!(0));
    // The document's own bytes are no change, and no error.
    assert!(assign(&base, srgb.bytes()).unwrap().is_none());
    assert!(assign_rasterizing(&base, srgb.bytes()).unwrap().is_none());
    assert!(assign_rasterizing(&base, p3.bytes()).unwrap().is_some());
}

#[test]
fn assign_clears_a_description_as_the_16_bit_assign_does() {
    let base = fixture(&IccProfile::srgb());
    let text =
        CString::new(r#"{"type":"shape","version":4,"artwork":{"ink":"linear_srgb"}}"#).unwrap();
    let mut err = ptr::null_mut();
    let described =
        unsafe { dr_precision_doc_artwork_metadata(&base, 0, text.as_ptr(), LIMIT, 0, &mut err) };
    let described = outcome(described, err).unwrap().expect("described");
    assert!(description(&described, 0).is_some());
    let out = assign(&described, IccProfile::display_p3().bytes())
        .unwrap()
        .unwrap();
    assert_eq!(description(&out, 0), None);
    assert_eq!(bits(&floats(&out, 0)), bits(&floats(&described, 0)));
}

// ---------------------------------------------------------------- convert --

#[test]
fn convert_applies_the_matrix_between_the_primaries_with_no_clamp() {
    let srgb = IccProfile::srgb();
    let p3 = IccProfile::display_p3();
    let base = fixture(&srgb)
        .with_tone_map(ToneMap::Reinhard { white: 6.0 })
        .unwrap()
        .unwrap();
    let source = floats(&base, 0);
    let out = convert(&base, p3.bytes()).unwrap().expect("converted");
    assert!(out.is_float());
    assert_eq!(&out.profile().bytes()[..], &p3.bytes()[..]);
    assert_eq!(out.tone_map(), Some(ToneMap::Reinhard { white: 6.0 }));
    let got = floats(&out, 0);
    assert_converted(&got, &source, combined(&srgb, &p3), "sRGB to P3");
    // 8.0 grey stays far above white: nothing is clamped to 1.
    assert!(got[0][..3].iter().all(|v| *v > 7.9), "{:?}", got[0]);
    assert_ne!(identity(&out, 0), identity(&base, 0), "a new plane");

    // The plan charges the one plane, and the document's own bytes are no
    // change with no error.
    let plan = color_plan(&base, p3.bytes(), true).unwrap();
    assert_eq!(plan["converted_pixel_bytes"], json!(u64::from(W * H) * 16));
    assert!(convert(&base, srgb.bytes()).unwrap().is_none());

    // Back again lands within the round-trip bound.
    let back = convert(&out, srgb.bytes())
        .unwrap()
        .expect("converted back");
    for (i, (b, s)) in floats(&back, 0).iter().zip(&source).enumerate() {
        for c in 0..3 {
            let bound = ROUND_TRIP * f64::from(s[c]).abs().max(1.0);
            assert!(
                (f64::from(b[c]) - f64::from(s[c])).abs() <= bound,
                "round trip pixel {i} channel {c}: {} against {}",
                b[c],
                s[c]
            );
        }
        assert_eq!(b[3].to_bits(), s[3].to_bits());
    }
}

#[test]
fn a_colour_outside_the_destination_gamut_keeps_a_negative_component() {
    let srgb = IccProfile::srgb();
    let p3 = IccProfile::display_p3();
    let primaries = vec![
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 0.5],
        [1.0, 1.0, 1.0, 1.0],
    ];
    let base = float_doc(2, 2, &p3, std::slice::from_ref(&primaries));
    let out = convert(&base, srgb.bytes()).unwrap().expect("converted");
    let got = floats(&out, 0);
    assert_converted(&got, &primaries, combined(&p3, &srgb), "P3 to sRGB");
    // P3 red is outside sRGB: more than 1 red, negative green and blue.
    assert!(got[0][0] > 1.2, "{:?}", got[0]);
    assert!(got[0][1] < -0.03 && got[0][2] < -0.01, "{:?}", got[0]);
    // P3 green: negative red.
    assert!(got[1][0] < -0.2, "{:?}", got[1]);
}

#[test]
fn convert_keeps_structure_masks_channels_guides_and_shared_planes() {
    let srgb = IccProfile::srgb();
    let p3 = IccProfile::display_p3();
    let base = fixture(&srgb);
    let mut err = ptr::null_mut();
    let duplicated = unsafe { dr_precision_doc_duplicate_layer(&base, 0, &mut err) };
    let doc = outcome(duplicated, err).unwrap().expect("duplicated");
    assert_eq!(doc.layer_count(), 2);
    assert_eq!(identity(&doc, 0), identity(&doc, 1), "one shared plane");
    let levels = json!({"type":"adjust","evaluation":"precision_v1","op":"levels",
        "params":{"black":0.1,"white":0.9,"gamma":1.2}})
    .to_string();
    let doc = doc
        .add_adjustment("Adjust", &levels, 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let plane: Vec<u16> = (0..W * H).map(|i| (i * 5000) as u16).collect();
    let doc = edit(
        &doc,
        json!({"op": "set_mask", "layer": 0, "width": W, "height": H}),
        &plane,
    );
    let channel: Vec<u16> = (0..W * H).map(|i| 65535 - (i * 3000) as u16).collect();
    let doc = edit(
        &doc,
        json!({"op": "set_channel", "index": null, "name": "Alpha 1"}),
        &channel,
    );
    let doc = doc.add_guide(GuideOrientation::Vertical, 1.5).unwrap();
    let doc = doc.set_ruler_origin(1.0, 2.0).unwrap();
    let names = |d: &PrecisionDocument| -> Vec<String> {
        (0..d.layer_count())
            .map(|i| d.layer(i).unwrap().name().to_string())
            .collect()
    };

    // The plan counts the shared plane once.
    let plan = color_plan(&doc, p3.bytes(), true).unwrap();
    assert_eq!(plan["converted_pixel_bytes"], json!(u64::from(W * H) * 16));

    let out = convert(&doc, p3.bytes()).unwrap().expect("converted");
    assert_eq!(names(&out), names(&doc));
    let matrix = combined(&srgb, &p3);
    let source = floats(&doc, 0);
    assert_converted(&floats(&out, 0), &source, matrix, "record 0");
    assert_converted(&floats(&out, 1), &source, matrix, "record 1");
    assert_eq!(
        identity(&out, 0),
        identity(&out, 1),
        "converted once, shared"
    );
    for i in 0..doc.layer_count() {
        assert_eq!(out.layer_id(i), doc.layer_id(i), "record {i} keeps its id");
    }
    let adjustment = (0..out.layer_count())
        .find_map(|i| out.adjustment_description(i))
        .expect("the adjustment is kept");
    assert_eq!(adjustment, levels);
    let mask = out.layer(0).unwrap().mask().expect("the mask is kept");
    assert_eq!(mask.plane().samples(), &plane[..]);
    assert_eq!(out.channels().len(), 1);
    assert_eq!(out.channels()[0].name(), "Alpha 1");
    assert_eq!(out.channels()[0].plane().samples(), &channel[..]);
    assert_eq!(out.guides(), doc.guides());
    assert_eq!(out.guides().len(), 1);
    assert_eq!(out.ruler_origin(), (1.0, 2.0));
}

#[test]
fn convert_keeps_a_description_byte_for_byte_while_its_samples_convert() {
    let srgb = IccProfile::srgb();
    let p3 = IccProfile::display_p3();
    let base = fixture(&srgb);
    let written = r#"{"type":"shape","version":4,"artwork":{"ink":"linear_srgb"},"note":"é ✓"}"#;
    let text = CString::new(written).unwrap();
    let mut err = ptr::null_mut();
    let described =
        unsafe { dr_precision_doc_artwork_metadata(&base, 0, text.as_ptr(), LIMIT, 0, &mut err) };
    let described = outcome(described, err).unwrap().expect("described");
    let before = description(&described, 0).expect("a description");
    let out = convert(&described, p3.bytes()).unwrap().expect("converted");
    assert_eq!(description(&out, 0).as_deref(), Some(before.as_str()));
    assert_converted(
        &floats(&out, 0),
        &floats(&described, 0),
        combined(&srgb, &p3),
        "described record",
    );
}

#[test]
fn perceptual_and_saturation_give_the_default_samples() {
    let srgb = IccProfile::srgb();
    let p3 = IccProfile::display_p3();
    let base = fixture(&srgb);
    let default = floats(&convert(&base, p3.bytes()).unwrap().unwrap(), 0);
    for options in [
        Value::Null,
        json!({}),
        json!({"intent": "relative"}),
        json!({"intent": "perceptual"}),
        json!({"intent": "saturation"}),
        json!({"adjustments": "keep", "black_point_compensation": false}),
    ] {
        let out = convert_with(&base, p3.bytes(), &options)
            .unwrap()
            .unwrap_or_else(|| panic!("{options} converts"));
        assert_eq!(bits(&floats(&out, 0)), bits(&default), "{options}");
        assert!(preflight(&base, p3.bytes(), &options).is_ok(), "{options}");
        assert!(
            conversion_plan(&base, p3.bytes(), &options).is_ok(),
            "{options}"
        );
    }
}

#[test]
fn convert_reports_progress_after_each_converted_plane() {
    use doctored_core::job_control::DrJobControl;
    let srgb = IccProfile::srgb();
    let p3 = IccProfile::display_p3();
    // Two distinct planes of equal size: half the converted bytes each.
    let second: Vec<[f32; 4]> = extended().into_iter().rev().collect();
    let base = float_doc(W, H, &srgb, &[extended(), second]);
    let control = DrJobControl::default();
    control.record_reports();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_profile(
            &base,
            p3.bytes().as_ptr(),
            p3.bytes().len(),
            LIMIT,
            0,
            &control,
            &mut err,
        )
    };
    assert!(outcome(out, err).unwrap().is_some());
    // Stage 6 is the conversion: 0.05 + 0.9 · converted bytes / all of them,
    // stored in millionths.
    let converting: Vec<f64> = control
        .reports()
        .iter()
        .filter(|report| report.stage == 6)
        .map(|report| report.progress)
        .collect();
    assert_eq!(converting.len(), 2, "{converting:?}");
    for (got, want) in converting.iter().zip([0.5, 0.95]) {
        assert!((got - want).abs() <= 2e-6, "{got} against {want}");
    }
    assert_eq!(control.progress(), 1.0);
}

#[test]
fn convert_re_renders_a_bound_record_from_its_master_and_keeps_the_binding() {
    use crate::float_placed_image::{
        assert_rendered, info, mixed, oracle, p3_codes, place, typed_master,
    };
    let srgb = IccProfile::srgb();
    let p3 = IccProfile::display_p3();
    let dest = fixture(&srgb);
    // A 16-bit Display P3 master bound to a record of the sRGB float document.
    let codes = p3_codes(3, 2);
    let master = typed_master(3, 2, codes.clone(), p3.clone());
    let placed = mixed(&dest, None, Some(&master), place("bound", 0, 0))
        .unwrap()
        .expect("placed");
    assert_eq!(info(&placed, 1)["valid"], true);

    // The plan counts the raster plane and the bound record's render.
    let plan = color_plan(&placed, p3.bytes(), true).unwrap();
    assert_eq!(
        plan["converted_pixel_bytes"],
        json!(u64::from(W * H + 3 * 2) * 16)
    );
    let out = convert(&placed, p3.bytes()).unwrap().expect("converted");
    assert_eq!(info(&out, 1)["valid"], true, "the binding is kept");
    // Master and document are both Display P3 now: the render is the master's
    // own decode with no matrix.
    let chain = Chain::of(&p3);
    assert_rendered(&floats(&out, 1), &codes, |code| oracle(&chain, None, code));
    assert_converted(
        &floats(&out, 0),
        &floats(&placed, 0),
        combined(&srgb, &p3),
        "the ordinary record",
    );

    // Assign reinterprets a master's render, so it refuses until the caller
    // rasterizes; the rasterizing Assign keeps the rendered samples.
    let refusal = assign(&placed, p3.bytes())
        .err()
        .expect("a bound record refuses");
    assert!(
        refusal.starts_with("assigning a profile requires rasterizing"),
        "{refusal}"
    );
    let assigned = assign_rasterizing(&placed, p3.bytes())
        .unwrap()
        .expect("assigned");
    assert_eq!(bits(&floats(&assigned, 1)), bits(&floats(&placed, 1)));
}

// --------------------------------------------------------------- refusals --

#[test]
fn the_options_a_float_convert_cannot_follow_refuse_by_name() {
    let srgb = IccProfile::srgb();
    let p3 = IccProfile::display_p3();
    let base = fixture(&srgb);
    for (options, what) in [
        (
            json!({"intent": "absolute"}),
            "absolute colorimetric intent",
        ),
        (
            json!({"black_point_compensation": true}),
            "black-point compensation",
        ),
        (
            json!({"adjustments": "preserve_appearance"}),
            "preserve-appearance adjustments",
        ),
        (
            json!({"intent": "absolute", "black_point_compensation": true,
                   "adjustments": "preserve_appearance"}),
            "absolute colorimetric intent",
        ),
        (
            json!({"black_point_compensation": true, "adjustments": "preserve_appearance"}),
            "black-point compensation",
        ),
    ] {
        let sentence = options_sentence(what);
        assert_eq!(
            convert_with(&base, p3.bytes(), &options).err(),
            Some(sentence.clone()),
            "{options}: convert"
        );
        assert_eq!(
            preflight(&base, p3.bytes(), &options).err(),
            Some(sentence.clone()),
            "{options}: preflight"
        );
        assert_eq!(
            conversion_plan(&base, p3.bytes(), &options).err(),
            Some(sentence),
            "{options}: plan"
        );
        // The document's own bytes are no change at any options.
        assert!(
            convert_with(&base, srgb.bytes(), &options)
                .unwrap()
                .is_none(),
            "{options}: own bytes"
        );
        assert!(
            preflight(&base, srgb.bytes(), &options).is_ok(),
            "{options}"
        );
    }
}

#[test]
fn a_profile_with_no_model_or_failing_curves_refuses_by_name() {
    let base = fixture(&IccProfile::srgb());
    let lut = rgb_lut_proof();
    let bad = bad_profile();

    // No matrix/TRC model: Assign and Convert, plan and edit.
    assert_eq!(
        color_plan(&base, &lut, false).err().as_deref(),
        Some(LUT_ASSIGN)
    );
    assert_eq!(assign(&base, &lut).err().as_deref(), Some(LUT_ASSIGN));
    assert_eq!(
        assign_rasterizing(&base, &lut).err().as_deref(),
        Some(LUT_ASSIGN)
    );
    assert_eq!(
        color_plan(&base, &lut, true).err().as_deref(),
        Some(LUT_CONVERT)
    );
    assert_eq!(convert(&base, &lut).err().as_deref(), Some(LUT_CONVERT));
    assert_eq!(
        convert_with(&base, &lut, &Value::Null).err().as_deref(),
        Some(LUT_CONVERT)
    );
    assert_eq!(
        preflight(&base, &lut, &Value::Null).err().as_deref(),
        Some(LUT_CONVERT)
    );
    assert_eq!(
        conversion_plan(&base, &lut, &Value::Null).err().as_deref(),
        Some(LUT_CONVERT)
    );

    // Failing tone curves: Assign in its own words, Convert in the
    // destination's.
    assert_eq!(
        color_plan(&base, &bad, false).err().as_deref(),
        Some(CURVE_ASSIGN)
    );
    assert_eq!(assign(&base, &bad).err().as_deref(), Some(CURVE_ASSIGN));
    assert_eq!(
        assign_rasterizing(&base, &bad).err().as_deref(),
        Some(CURVE_ASSIGN)
    );
    assert_eq!(
        color_plan(&base, &bad, true).err().as_deref(),
        Some(CURVE_CONVERT)
    );
    assert_eq!(convert(&base, &bad).err().as_deref(), Some(CURVE_CONVERT));
    assert_eq!(
        convert_with(&base, &bad, &Value::Null).err().as_deref(),
        Some(CURVE_CONVERT)
    );
    assert_eq!(
        preflight(&base, &bad, &Value::Null).err().as_deref(),
        Some(CURVE_CONVERT)
    );
}

#[test]
fn a_converted_value_outside_the_float_range_refuses_rather_than_clipping() {
    let srgb = IccProfile::srgb();
    let p3 = IccProfile::display_p3();
    let sentence = range_sentence(srgb.name());
    for edge in [65504.0f32, -65504.0] {
        // The most saturated P3 red at the range's edge is past it in sRGB.
        let pixels = vec![[0.5, 0.25, 0.125, 1.0], [edge, 0.0, 0.0, 1.0]];
        let base = float_doc(2, 1, &p3, std::slice::from_ref(&pixels));
        assert_eq!(
            convert(&base, srgb.bytes()).err(),
            Some(sentence.clone()),
            "{edge}"
        );
        assert_eq!(
            convert_with(&base, srgb.bytes(), &Value::Null).err(),
            Some(sentence.clone()),
            "{edge}"
        );
        // Assign converts nothing, so it has nothing to refuse.
        assert!(assign(&base, srgb.bytes()).unwrap().is_some());
    }
    // A grey at the edge stays inside it (white maps to white).
    let grey = vec![[60000.0, 60000.0, 60000.0, 1.0]];
    let base = float_doc(1, 1, &p3, std::slice::from_ref(&grey));
    assert!(convert(&base, srgb.bytes()).unwrap().is_some());
}

#[test]
fn a_bound_float_master_past_the_range_refuses_in_convert_words() {
    use crate::float_placed_image::{info, mixed, place};
    let srgb = IccProfile::srgb();
    let p3 = IccProfile::display_p3();
    // A Display P3 f32 master in a Display P3 float document: the place runs
    // no matrix, so the saturated red at 60000 lands inside the range.
    let master = float_doc(2, 2, &p3, &[vec![[60000.0, 0.0, 0.0, 1.0]; 4]]);
    let placed = mixed(&fixture(&p3), None, Some(&master), place("hot", 0, 0))
        .unwrap()
        .expect("placed");
    assert_eq!(info(&placed, 1)["valid"], true);
    // Re-rendered into sRGB the red is past ±65504: the Convert's own
    // sentence, naming the destination, not the place's.
    assert_eq!(
        convert(&placed, srgb.bytes()).err(),
        Some(range_sentence(srgb.name()))
    );
    assert_eq!(
        convert_with(&placed, srgb.bytes(), &Value::Null).err(),
        Some(range_sentence(srgb.name()))
    );
}

#[test]
fn the_plan_is_the_threshold() {
    let p3 = IccProfile::display_p3();
    let base = fixture(&IccProfile::srgb());
    for convert_edit in [false, true] {
        let plan = color_plan(&base, p3.bytes(), convert_edit).unwrap();
        let total =
            plan["retained_bytes"].as_u64().unwrap() + plan["scratch_bytes"].as_u64().unwrap();
        assert!(color_plan_at(&base, p3.bytes(), convert_edit, total).is_ok());
        let run = |limit: u64| {
            if convert_edit {
                convert_at(&base, p3.bytes(), limit)
            } else {
                assign_at(&base, p3.bytes(), limit)
            }
        };
        assert!(run(total).unwrap().is_some(), "convert {convert_edit}");
        // Far under the plan refuses before any pixel is converted.
        let floor = plan["retained_bytes"].as_u64().unwrap();
        assert!(run(floor).is_err(), "convert {convert_edit}");
    }
}

#[test]
fn the_operator_table_names_what_runs_and_what_stays_refused() {
    let table: Value = serde_json::from_str(&float_operators_json()).unwrap();
    let admitted: Vec<&str> = table["admitted"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for row in ["color_assign", "color_convert"] {
        assert!(admitted.contains(&row), "{row} is admitted");
    }
    let refused: Vec<&str> = table["refused"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["operator"].as_str())
        .collect();
    for row in [
        "Assign Profile or Convert to Profile with a profile that has no matrix/TRC model, and Assign Profile with one whose tone curves fail the precision check",
        "Convert to Profile with absolute colorimetric intent, black-point compensation or preserve-appearance adjustments",
        "Apply Image",
    ] {
        assert!(refused.contains(&row), "{row}");
    }
    assert!(!refused.contains(&"Assign Profile and Convert to Profile"));
    for row in table["refused"].as_array().unwrap() {
        assert!(
            row["reason"]
                .as_str()
                .is_some_and(|reason| !reason.is_empty()),
            "{row}"
        );
    }
}

// ---------------------------------------------------- measured admission --

/// Max E of the measured Convert, bytes, from `admission_measure` (the module
/// doc of `float_color.rs` holds the record).
const MEASURED_MAX_E: u64 = 385_007_904;

fn measured_plan(doc: &PrecisionDocument) -> u64 {
    color_plan_at(doc, IccProfile::display_p3().bytes(), true, MLIMIT).unwrap()["scratch_bytes"]
        .as_u64()
        .unwrap()
}

#[test]
fn plans_cover_the_measured_peaks() {
    let scratch = measured_plan(&measure_fixture());
    let floor = (MEASURED_MAX_E * 5).div_ceil(4);
    assert!(
        scratch >= floor,
        "convert: plan {scratch} under ⌈1.25 × {MEASURED_MAX_E}⌉ = {floor}"
    );
}

/// One case per process, selected by `DR_FLOAT_CASE` (`baseline`,
/// `convert`): build `float_layer_edits`' 4000×3000 sRGB fixture, read the
/// footprint, reset the interval peak, convert to Display P3 through the FFI
/// at the 8 GiB limit, and print E = interval peak − pre-edit footprint
/// beside the plan's scratch, with the result held until E is read.
/// `baseline` builds the same fixture and runs nothing.
#[test]
#[ignore = "measurement: DR_FLOAT_CASE=baseline|convert, Release, MallocLargeCache=0"]
fn admission_measure() {
    let case = std::env::var("DR_FLOAT_CASE").expect("DR_FLOAT_CASE");
    assert!(
        case == "baseline" || case == "convert",
        "unknown DR_FLOAT_CASE {case}"
    );
    let doc = measure_fixture();
    let p3 = IccProfile::display_p3();
    let scratch = measured_plan(&doc);
    let pre = footprint_reset();
    let out = if case == "baseline" {
        None
    } else {
        Some(
            convert_at(&doc, p3.bytes(), MLIMIT)
                .expect("the conversion is admitted")
                .expect("the conversion changes the document"),
        )
    };
    let peak = rusage().fields[INTERVAL_MAX_PHYS_FOOTPRINT];
    assert_eq!(out.is_none(), case == "baseline");
    drop(out);
    println!(
        "FLOAT_MEASURE case={case} scratch={scratch} pre={pre} peak={peak} E={}",
        peak.saturating_sub(pre)
    );
}
