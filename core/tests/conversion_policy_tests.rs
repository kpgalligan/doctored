//! The Phase 11C conversion POLICY through the public C FFI: the options
//! object, the preflight, the per-kind adjustment rule, the two explicit
//! Assign workflows, and the claim the whole phase rests on — that the
//! DEFAULT conversion is byte-identical to the one this build already
//! shipped.
//!
//! That claim is measured rather than asserted: a document converted through
//! `dr_doc_convert_to_profile_with` at its defaults is compared pixel by
//! pixel against the SHIPPED `icc_transform::Transform` applied here, which
//! is an oracle outside the code under test — `dr_doc_convert_to_profile`
//! now delegates to the `_with` entry point, so comparing the two would only
//! measure that delegation (it is still checked, as exactly that).

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::*;
use doctored_core::ffi_color::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_output_color::*;
use doctored_core::icc::IccProfile;
use doctored_core::icc_transform::Transform;
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common;
use common::*;

const PROFILE_SRGB: std::ffi::c_int = 0;
const PROFILE_DISPLAY_P3: std::ffi::c_int = 1;

fn builtin_bytes(which: std::ffi::c_int) -> Vec<u8> {
    let len = dr_builtin_profile_len(which);
    let mut out = vec![0u8; len];
    assert!(unsafe { dr_builtin_profile(which, out.as_mut_ptr(), len) });
    out
}

fn json_of(raw: *mut c_char) -> serde_json::Value {
    assert!(!raw.is_null());
    let text = take_err_string(raw);
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON: {text} ({e})"))
}

/// An opaque gradient, so a preserve-appearance bake is legal.
fn opaque_gradient(w: u32, h: u32) -> RgbaImage {
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = ((x * 255) / w.max(1)) as u8;
            out.put_pixel(x, y, Rgba([v, 128, 255 - v, 255]));
        }
    }
    out
}

fn adjustment_meta(op: &str, params: &str) -> CString {
    CString::new(format!(
        "{{\"type\":\"adjust\",\"op\":\"{op}\",\"params\":{params}}}"
    ))
    .expect("static json")
}

/// A document with one opaque raster and `layers` adjustment layers above it.
fn document_with_adjustments(dir: &TempDir, metas: &[(&str, &str)]) -> *mut DrDocument {
    let mut doc = doc_from(dir, "base.png", &opaque_gradient(16, 12));
    for (i, (op, params)) in metas.iter().enumerate() {
        // `adding_image_layer` takes the index of the layer to insert ABOVE,
        // so the i-th adjustment goes above whatever is currently on top.
        doc = add_layer(
            dir,
            &format!("adj{i}.png"),
            doc,
            i,
            &opaque_gradient(16, 12),
            op,
        );
        let meta = adjustment_meta(op, params);
        doc = apply(doc, |d| unsafe {
            dr_doc_with_layer_meta(d, 1 + i, meta.as_ptr())
        });
    }
    doc
}

// ------------------------------------------------------------- byte identity --

#[test]
fn the_default_options_conversion_is_byte_identical_to_the_shipped_one() {
    let dir = TempDir::new().unwrap();
    let doc = document_with_adjustments(&dir, &[("bcs", "{\"brightness\":0.1}")]);
    let p3 = builtin_bytes(PROFILE_DISPLAY_P3);

    let shipped =
        unsafe { dr_doc_convert_to_profile(doc, p3.as_ptr(), p3.len(), std::ptr::null_mut()) };
    assert!(!shipped.is_null());

    // The independent answer: the shipped sRGB -> P3 `Transform`, run here
    // over each ORIGINAL layer's pixels. Nothing in `doc_color_policy` or
    // `icc_intent` takes part in producing it.
    let srgb = IccProfile::parse(&builtin_bytes(PROFILE_SRGB)).expect("sRGB parses");
    let destination = IccProfile::parse(&p3).expect("P3 parses");
    let transform = Transform::new(
        srgb.model().expect("sRGB is matrix/TRC"),
        destination.model().expect("P3 is matrix/TRC"),
    );
    let expected: Vec<Vec<u8>> = (0..unsafe { dr_doc_layer_count(doc) })
        .map(|i| {
            let (w, h) = layer_dims(doc, i);
            let mut buffer = RgbaImage::from_raw(w, h, layer_pixels(doc, i)).expect("layer pixels");
            transform.apply(&mut buffer);
            buffer.into_raw()
        })
        .collect();

    for options in [None, Some("{}"), Some("{\"intent\":\"relative\"}")] {
        let json = options.map(|o| CString::new(o).unwrap());
        let mut report: *mut c_char = ptr::null_mut();
        let mut err: *mut c_char = ptr::null_mut();
        let with = unsafe {
            dr_doc_convert_to_profile_with(
                doc,
                p3.as_ptr(),
                p3.len(),
                json.as_ref().map_or(ptr::null(), |j| j.as_ptr()),
                ptr::null(),
                &mut report,
                &mut err,
            )
        };
        assert!(!with.is_null(), "{:?}: {}", options, take_err_string(err));
        assert!(err.is_null());
        assert_eq!(unsafe { dr_doc_layer_count(with) }, unsafe {
            dr_doc_layer_count(shipped)
        });
        assert_eq!(expected.len(), unsafe { dr_doc_layer_count(shipped) });
        for (i, want) in expected.iter().enumerate() {
            // Against the independent transform FIRST: that is the property
            // the phase's byte-identity claim is about.
            assert_eq!(
                &layer_pixels(with, i),
                want,
                "layer {i} under {options:?} against the shipped Transform"
            );
            // And the old entry point still answers the same thing.
            assert_eq!(
                layer_pixels(with, i),
                layer_pixels(shipped, i),
                "layer {i} under {options:?}"
            );
        }
        assert_eq!(flat_pixels(with), flat_pixels(shipped));
        if !report.is_null() {
            let value = json_of(report);
            // The default policy reports every adjustment as KEPT.
            for row in value.as_array().expect("an array") {
                assert_eq!(row["action"], "kept");
            }
        }
        unsafe { dr_doc_free(with) };
    }
    unsafe { dr_doc_free(shipped) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn the_options_object_refuses_an_unknown_key_and_an_unknown_value() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "a.png", &opaque_gradient(8, 8));
    let p3 = builtin_bytes(PROFILE_DISPLAY_P3);
    for (json, expected) in [
        ("{\"intnet\":\"perceptual\"}", "intnet"),
        ("{\"intent\":\"Perceptual\"}", "Perceptual"),
        ("{\"adjustments\":\"bake\"}", "bake"),
        (
            "{\"black_point_compensation\":\"yes\"}",
            "black_point_compensation",
        ),
        ("not json", "JSON"),
    ] {
        let options = CString::new(json).unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe {
            dr_doc_conversion_preflight(
                doc,
                p3.as_ptr(),
                p3.len(),
                options.as_ptr(),
                std::ptr::null(),
                &mut err,
            )
        };
        assert!(out.is_null(), "{json} should refuse");
        let message = take_err_string(err);
        assert!(message.contains(expected), "{json}: {message}");
    }
    unsafe { dr_doc_free(doc) };
}

// --------------------------------------------------------- the per-kind rule --

#[test]
fn preserve_appearance_keeps_document_v2_exposure_and_bakes_the_rest() {
    let dir = TempDir::new().unwrap();
    let doc = document_with_adjustments(
        &dir,
        &[
            (
                "exposure",
                "{\"exposure\":0.5,\"evaluation\":\"document_v2\"}",
            ),
            ("bcs", "{\"brightness\":0.1}"),
            (
                "exposure",
                "{\"exposure\":0.25,\"evaluation\":\"legacy_srgb\"}",
            ),
        ],
    );
    let p3 = builtin_bytes(PROFILE_DISPLAY_P3);
    let options = CString::new("{\"adjustments\":\"preserve_appearance\"}").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let preflight = unsafe {
        dr_doc_conversion_preflight(
            doc,
            p3.as_ptr(),
            p3.len(),
            options.as_ptr(),
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(!preflight.is_null(), "{}", take_err_string(err));
    let value = json_of(preflight);
    let rows = value["adjustments"].as_array().expect("an array");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["op"], "exposure");
    assert_eq!(
        rows[0]["action"], "kept",
        "document_v2 exposure is light-linear"
    );
    assert!(rows[0]["reason"].as_str().unwrap().contains("document_v2"));
    assert_eq!(rows[1]["op"], "bcs");
    assert_eq!(rows[1]["action"], "baked");
    assert_eq!(rows[2]["op"], "exposure");
    assert_eq!(
        rows[2]["action"], "baked",
        "legacy_srgb evaluation is baked"
    );
    assert_eq!(value["options"]["adjustments"], "preserve_appearance");

    // And the conversion does exactly what the preflight said.
    let mut report: *mut c_char = ptr::null_mut();
    let mut err: *mut c_char = ptr::null_mut();
    let converted = unsafe {
        dr_doc_convert_to_profile_with(
            doc,
            p3.as_ptr(),
            p3.len(),
            options.as_ptr(),
            ptr::null(),
            &mut report,
            &mut err,
        )
    };
    assert!(!converted.is_null(), "{}", take_err_string(err));
    let landed = json_of(report);
    assert_eq!(landed.as_array().unwrap().len(), 3);
    // The STRUCTURE survives: four entries in, four entries out, nothing
    // flattened.
    assert_eq!(unsafe { dr_doc_layer_count(converted) }, 4);
    // The kept entry is still an adjustment layer; the baked ones are not.
    assert!(unsafe { dr_doc_layer_is_adjustment(converted, 1) });
    assert!(!unsafe { dr_doc_layer_is_adjustment(converted, 2) });
    assert!(!unsafe { dr_doc_layer_is_adjustment(converted, 3) });
    assert_eq!(layer_name(converted, 2), "bcs (baked)");
    assert_eq!(layer_name(converted, 3), "exposure (baked)");
    unsafe { dr_doc_free(converted) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn preserve_appearance_keeps_the_appearance_it_promises() {
    // The whole point: the composite before the conversion and the composite
    // after it, compared IN A COMMON SPACE. The baked layer holds the
    // pre-conversion appearance and is then converted like every other
    // pixel, so converting back must land within the two round trips'
    // quantization.
    let dir = TempDir::new().unwrap();
    let doc = document_with_adjustments(&dir, &[("bcs", "{\"brightness\":0.2,\"contrast\":0.3}")]);
    let before = flat_pixels(doc);
    let p3 = builtin_bytes(PROFILE_DISPLAY_P3);
    let srgb = builtin_bytes(PROFILE_SRGB);
    let options = CString::new("{\"adjustments\":\"preserve_appearance\"}").unwrap();
    let mut report: *mut c_char = ptr::null_mut();
    let mut err: *mut c_char = ptr::null_mut();
    let converted = unsafe {
        dr_doc_convert_to_profile_with(
            doc,
            p3.as_ptr(),
            p3.len(),
            options.as_ptr(),
            ptr::null(),
            &mut report,
            &mut err,
        )
    };
    assert!(!converted.is_null(), "{}", take_err_string(err));
    if !report.is_null() {
        unsafe { dr_string_free(report) };
    }
    // Back to sRGB through the shipped path, so the two composites are
    // comparable byte for byte.
    let back = unsafe {
        dr_doc_convert_to_profile(converted, srgb.as_ptr(), srgb.len(), std::ptr::null_mut())
    };
    assert!(!back.is_null());
    let after = flat_pixels(back);
    assert_eq!(before.len(), after.len());
    let mut worst = 0i32;
    let mut total = 0i64;
    for (a, b) in before.iter().zip(after.iter()) {
        let delta = (i32::from(*a) - i32::from(*b)).abs();
        worst = worst.max(delta);
        total += i64::from(delta);
    }
    let mean = total as f64 / before.len() as f64;
    // Two sRGB <-> P3 round trips at 8 bits; the shipped transform's own
    // requantization is the whole budget.
    assert!(
        worst <= 3,
        "preserve-appearance drifted {worst} codes (mean {mean:.3})"
    );
    assert!(mean <= 0.6, "mean drift {mean:.3}");
    unsafe { dr_doc_free(back) };
    unsafe { dr_doc_free(converted) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn keep_parameters_is_the_default_and_moves_the_composite() {
    // The honest counterpart: with parameters kept, a stack whose result is
    // computed FROM the numbers genuinely moves. Recorded, not gated.
    let dir = TempDir::new().unwrap();
    let doc = document_with_adjustments(&dir, &[("bcs", "{\"brightness\":0.2,\"contrast\":0.3}")]);
    let before = flat_pixels(doc);
    let p3 = builtin_bytes(PROFILE_DISPLAY_P3);
    let srgb = builtin_bytes(PROFILE_SRGB);
    let converted =
        unsafe { dr_doc_convert_to_profile(doc, p3.as_ptr(), p3.len(), std::ptr::null_mut()) };
    assert!(!converted.is_null());
    let back = unsafe {
        dr_doc_convert_to_profile(converted, srgb.as_ptr(), srgb.len(), std::ptr::null_mut())
    };
    assert!(!back.is_null());
    let after = flat_pixels(back);
    let worst = before
        .iter()
        .zip(after.iter())
        .map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs())
        .max()
        .unwrap_or(0);
    // The number itself is not a gate — it is the measurement that makes
    // "preserve appearance" worth offering.
    println!("keep-parameters round-trip drift: {worst} codes");
    unsafe { dr_doc_free(back) };
    unsafe { dr_doc_free(converted) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_bake_over_transparency_is_refused_by_name() {
    let dir = TempDir::new().unwrap();
    // A base with a transparent corner makes the substitution non-identical,
    // which the rule refuses rather than quietly changing the picture.
    let mut base = opaque_gradient(8, 8);
    base.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
    let mut doc = doc_from(&dir, "base.png", &base);
    doc = add_layer(&dir, "adj.png", doc, 0, &opaque_gradient(8, 8), "Curves");
    let meta = adjustment_meta("bcs", "{\"brightness\":0.1}");
    doc = apply(doc, |d| unsafe {
        dr_doc_with_layer_meta(d, 1, meta.as_ptr())
    });
    let p3 = builtin_bytes(PROFILE_DISPLAY_P3);
    let options = CString::new("{\"adjustments\":\"preserve_appearance\"}").unwrap();

    // The PREFLIGHT refuses, which is what the host's Apply button and the
    // MCP tool ask: a refusal discovered only inside the bake would have
    // reported this layer as "baked" first.
    let mut err: *mut c_char = ptr::null_mut();
    let preflight = unsafe {
        dr_doc_conversion_preflight(
            doc,
            p3.as_ptr(),
            p3.len(),
            options.as_ptr(),
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(preflight.is_null());
    let early = take_err_string(err);
    assert!(
        early.contains("Curves") && early.contains("transparent"),
        "{early}"
    );

    let mut report: *mut c_char = ptr::null_mut();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_convert_to_profile_with(
            doc,
            p3.as_ptr(),
            p3.len(),
            options.as_ptr(),
            ptr::null(),
            &mut report,
            &mut err,
        )
    };
    assert!(out.is_null());
    let message = take_err_string(err);
    assert!(
        message.contains("Curves"),
        "the refusal names the layer: {message}"
    );
    assert!(message.contains("transparent"), "{message}");
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_hidden_adjustment_is_left_alone() {
    let dir = TempDir::new().unwrap();
    let doc = document_with_adjustments(&dir, &[("bcs", "{\"brightness\":0.1}")]);
    let doc = apply(doc, |d| unsafe { dr_doc_with_layer_visible(d, 1, false) });
    let p3 = builtin_bytes(PROFILE_DISPLAY_P3);
    let options = CString::new("{\"adjustments\":\"preserve_appearance\"}").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let preflight = unsafe {
        dr_doc_conversion_preflight(
            doc,
            p3.as_ptr(),
            p3.len(),
            options.as_ptr(),
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(!preflight.is_null(), "{}", take_err_string(err));
    let value = json_of(preflight);
    let rows = value["adjustments"].as_array().expect("an array");
    assert_eq!(rows[0]["action"], "kept");
    assert!(rows[0]["reason"].as_str().unwrap().contains("hidden"));
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------------------------ assign --

#[test]
fn a_document_with_no_retained_source_has_no_assign_refusal() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "a.png", &opaque_gradient(8, 8));
    let value = json_of(unsafe { dr_doc_assign_refusal(doc) });
    assert!(value["refusal"].is_null());
    assert_eq!(value["retained_sources"], 0);
    assert!(value.get("live_filters").is_none(), "{value}");
    // And `assign_profile_rasterizing` still works: with nothing bound it is
    // the plain assign.
    let p3 = builtin_bytes(PROFILE_DISPLAY_P3);
    let before = layer_pixels(doc, 0);
    let out = unsafe {
        dr_doc_assign_profile_rasterizing(doc, p3.as_ptr(), p3.len(), std::ptr::null_mut())
    };
    assert!(!out.is_null());
    assert_eq!(layer_pixels(out, 0), before, "assign touches no pixel");
    // Assigning the profile it already carries changes nothing, and says
    // nothing: a no-change edit is NULL with no message.
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe { dr_doc_assign_profile_rasterizing(out, p3.as_ptr(), p3.len(), &mut err) }
            .is_null()
    );
    assert!(
        err.is_null(),
        "the same profile bytes are no change, not a refusal"
    );
    unsafe { dr_doc_free(out) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_retained_source_refuses_assign_and_the_disclosed_path_rasterizes_it() {
    // The 8A rule, end to end: a document holding a placed source refuses,
    // names the count, and rasterizes-then-assigns as ONE result whose pixels
    // are exactly the ones that were there.
    let base = DrDocument::from_pixels(opaque_gradient(16, 12));
    let source = DrDocument::from_pixels(opaque_gradient(8, 8));
    let mut err: *mut c_char = ptr::null_mut();
    let placed = {
        let request = CString::new(
            "{\"op\":\"place\",\"source_id\":\"fixture-a\",\"name\":\"Placed\",\"x\":0,\"y\":0}",
        )
        .unwrap();
        let result = unsafe {
            doctored_core::ffi_retained_source::dr_doc_mixed_source_edit(
                &base,
                &source,
                ptr::null(),
                request.as_ptr(),
                doctored_core::precision_image::REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!result.is_null(), "{}", take_err_string(err));
        result
    };
    let value = json_of(unsafe { dr_doc_assign_refusal(placed) });
    assert_eq!(value["retained_sources"], 1);
    let reason = value["refusal"].as_str().expect("a refusal");
    assert!(reason.contains("1 image source"), "{reason}");

    // The plain assign refuses outright, with that sentence.
    let p3 = builtin_bytes(PROFILE_DISPLAY_P3);
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_doc_assign_profile(placed, p3.as_ptr(), p3.len(), &mut err) }.is_null());
    assert_eq!(take_err_string(err), reason);

    // A bound layer whose pixels are locked cannot be rasterized, and the
    // rasterizing assign says so.
    let locked =
        unsafe { doctored_core::ffi_group::dr_doc_with_layer_locks(placed, 1, LOCK_PIXELS) };
    assert!(!locked.is_null());
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe { dr_doc_assign_profile_rasterizing(locked, p3.as_ptr(), p3.len(), &mut err) }
            .is_null()
    );
    assert_eq!(take_err_string(err), "source layer pixels are locked");
    unsafe { dr_doc_free(locked) };

    // The disclosed one succeeds, leaves the pixels alone and drops the
    // binding — which is what "rasterize" means.
    let before = flat_pixels(placed);
    let out = unsafe {
        dr_doc_assign_profile_rasterizing(placed, p3.as_ptr(), p3.len(), std::ptr::null_mut())
    };
    assert!(
        !out.is_null(),
        "the disclosed rasterizing assign must succeed"
    );
    assert_eq!(flat_pixels(out), before, "assign touches no pixel");
    let after = json_of(unsafe { dr_doc_assign_refusal(out) });
    assert_eq!(after["retained_sources"], 0);
    assert!(after["refusal"].is_null());
    unsafe { dr_doc_free(out) };
    unsafe { dr_doc_free(placed) };
}

// ----------------------------------------------------------------- refusals --

#[test]
fn a_lut_source_can_be_converted_from_while_the_matrix_gate_stays_false() {
    let Ok(bytes) =
        std::fs::read("/Library/Application Support/Adobe/Color/Profiles/VideoHD16-235.icc")
    else {
        eprintln!("skipping: VideoHD16-235.icc is not installed");
        return;
    };
    assert!(IccProfile::parse(&bytes).is_some());
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "a.png", &opaque_gradient(8, 8));
    let tagged =
        unsafe { dr_doc_assign_profile(doc, bytes.as_ptr(), bytes.len(), std::ptr::null_mut()) };
    assert!(!tagged.is_null(), "an RGB LUT profile IS stored");
    // The two gates, side by side — this is the objection the phase
    // answered, as a test.
    assert!(!unsafe { dr_doc_profile_is_convertible(tagged) });
    assert!(unsafe { dr_doc_profile_can_decode(tagged) });
    // And a conversion OUT of it now works.
    let srgb = builtin_bytes(PROFILE_SRGB);
    let mut err: *mut c_char = ptr::null_mut();
    let converted = unsafe {
        dr_doc_convert_to_profile_with(
            tagged,
            srgb.as_ptr(),
            srgb.len(),
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            &mut err,
        )
    };
    assert!(!converted.is_null(), "{}", take_err_string(err));
    unsafe { dr_doc_free(converted) };
    unsafe { dr_doc_free(tagged) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn every_policy_export_tolerates_null() {
    let mut err: *mut c_char = ptr::null_mut();
    let mut report: *mut c_char = ptr::null_mut();
    unsafe {
        assert!(dr_doc_conversion_preflight(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            std::ptr::null(),
            &mut err
        )
        .is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_doc_conversion_preflight(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            std::ptr::null(),
            ptr::null_mut()
        )
        .is_null());

        assert!(dr_doc_convert_to_profile_with(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            ptr::null(),
            &mut report,
            &mut err
        )
        .is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_doc_convert_to_profile_with(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut()
        )
        .is_null());

        assert!(dr_doc_assign_refusal(ptr::null()).is_null());
        assert!(dr_doc_assign_profile_refusal(ptr::null(), ptr::null(), 0).is_null());
        assert!(dr_doc_assign_profile_rasterizing(
            ptr::null(),
            ptr::null(),
            0,
            std::ptr::null_mut()
        )
        .is_null());
        assert!(!dr_doc_profile_can_decode(ptr::null()));

        assert!(dr_precision_doc_conversion_preflight(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            std::ptr::null(),
            &mut err
        )
        .is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_precision_doc_convert_to_profile_with(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            ptr::null(),
            &mut report,
            &mut err
        )
        .is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(
            doctored_core::ffi_precision_color::dr_precision_doc_conversion_plan(
                ptr::null(),
                ptr::null(),
                0,
                ptr::null(),
                0,
                0,
                &mut err
            )
            .is_null()
        );
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(
            doctored_core::ffi_precision_color::dr_precision_doc_conversion_plan(
                ptr::null(),
                ptr::null(),
                0,
                ptr::null(),
                0,
                0,
                ptr::null_mut()
            )
            .is_null()
        );
        assert!(dr_precision_doc_assign_refusal(ptr::null()).is_null());
        assert!(dr_precision_doc_assign_profile_rasterizing(
            ptr::null(),
            ptr::null(),
            0,
            0,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        dr_string_free(err);
    }
    if !report.is_null() {
        unsafe { dr_string_free(report) };
    }
}
