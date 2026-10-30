//! Convert to Profile's `rasterize_sources` option, through the public
//! FFI at both depths.
//!
//! A retained source re-renders relative colorimetric with black point
//! compensation off, so a conversion at any other setting refuses while a
//! layer is bound. With `"rasterize_sources": true` those settings rasterize
//! every bound layer first and convert the result, as ONE result. The oracle
//! is the two steps done by hand: `rasterize` of the layer, then the same
//! conversion without the flag, compared sample for sample. Settings the
//! source can follow keep it bound whatever the flag says.
use std::{ffi::CString, ptr};

use doctored_core::ffi_output_color::*;
use doctored_core::icc::IccProfile;
use serde_json::{json, Value};

use crate::live_filter_support::{json_result, text, Doc, LIMIT};

const REFUSAL: &str = "converting with these settings cannot keep 1 image source bound; convert at relative colorimetric with black point compensation off";

fn ramp(w: u32, h: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let v = (i * 65535 / (w * h)) as u16;
            [
                v,
                40000 - v / 4,
                65535 - v,
                if i % 5 == 0 { 30000 } else { 65535 },
            ]
        })
        .collect()
}

/// One retained layer (a Display P3 master) in an sRGB document.
fn bound(typed: bool) -> Doc {
    Doc::retained(
        typed,
        true,
        8,
        6,
        &ramp(8, 6),
        IccProfile::display_p3(),
        IccProfile::srgb(),
    )
}

fn is_bound(doc: &Doc) -> bool {
    match doc {
        Doc::Byte(d) => d.has_retained_source(0),
        Doc::Typed(d) => d.has_retained_source(0),
    }
}

fn profile_bytes(doc: &Doc) -> Vec<u8> {
    match doc {
        Doc::Byte(d) => d.profile.bytes().to_vec(),
        Doc::Typed(d) => d.profile().bytes().to_vec(),
    }
}

fn preflight(doc: &Doc, profile: &[u8], options: &Value) -> Result<Value, String> {
    let options = CString::new(options.to_string()).unwrap();
    let mut e = ptr::null_mut();
    let p = unsafe {
        match doc {
            Doc::Byte(d) => dr_doc_conversion_preflight(
                d,
                profile.as_ptr(),
                profile.len(),
                options.as_ptr(),
                ptr::null(),
                &mut e,
            ),
            Doc::Typed(d) => dr_precision_doc_conversion_preflight(
                d,
                profile.as_ptr(),
                profile.len(),
                options.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut e,
            ),
        }
    };
    unsafe { json_result(p, e) }
}

/// The converted document, `Ok(None)` for the no-op answer.
fn convert(doc: &Doc, profile: &[u8], options: &Value) -> Result<Option<Doc>, String> {
    let options = CString::new(options.to_string()).unwrap();
    let mut report = ptr::null_mut();
    let mut e = ptr::null_mut();
    let result = unsafe {
        match doc {
            Doc::Byte(d) => {
                let p = dr_doc_convert_to_profile_with(
                    d,
                    profile.as_ptr(),
                    profile.len(),
                    options.as_ptr(),
                    ptr::null(),
                    &mut report,
                    &mut e,
                );
                (!p.is_null()).then(|| Doc::Byte(*Box::from_raw(p)))
            }
            Doc::Typed(d) => {
                let p = dr_precision_doc_convert_to_profile_with(
                    d,
                    profile.as_ptr(),
                    profile.len(),
                    options.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut report,
                    &mut e,
                );
                (!p.is_null()).then(|| Doc::Typed(*Box::from_raw(p)))
            }
        }
    };
    let error = unsafe { text(e) };
    unsafe { text(report) };
    if !error.is_empty() {
        assert!(result.is_none());
        return Err(error);
    }
    Ok(result)
}

/// The settings a source render cannot follow.
fn refusing_settings() -> [Value; 2] {
    [
        json!({"black_point_compensation": true}),
        json!({"intent": "absolute"}),
    ]
}

fn with_flag(settings: &Value) -> Value {
    let mut options = settings.clone();
    options["rasterize_sources"] = json!(true);
    options
}

#[test]
fn settings_a_source_cannot_follow_refuse_without_the_flag() {
    let p3 = IccProfile::display_p3().bytes().to_vec();
    for typed in [false, true] {
        let doc = bound(typed);
        assert!(is_bound(&doc), "typed={typed}: the fixture is bound");
        for settings in refusing_settings() {
            let what = format!("{settings} typed={typed}");
            assert_eq!(
                preflight(&doc, &p3, &settings).unwrap_err(),
                REFUSAL,
                "{what}"
            );
            let refused = match convert(&doc, &p3, &settings) {
                Err(message) => message,
                Ok(_) => panic!("{what}: converted"),
            };
            assert_eq!(refused, REFUSAL, "{what}");
            // An explicit false is the default.
            let mut off = settings.clone();
            off["rasterize_sources"] = json!(false);
            assert_eq!(preflight(&doc, &p3, &off).unwrap_err(), REFUSAL, "{what}");
        }
    }
}

#[test]
fn the_flag_rasterizes_then_converts_as_one_result() {
    let p3 = IccProfile::display_p3().bytes().to_vec();
    for typed in [false, true] {
        let doc = bound(typed);
        for settings in refusing_settings() {
            let what = format!("{settings} typed={typed}");
            let options = with_flag(&settings);
            preflight(&doc, &p3, &options)
                .unwrap_or_else(|message| panic!("{what}: the preflight refuses: {message}"));
            let converted = convert(&doc, &p3, &options)
                .unwrap_or_else(|message| panic!("{what}: {message}"))
                .expect("the conversion changes the document");
            assert!(!is_bound(&converted), "{what}: no layer stays bound");
            assert_eq!(profile_bytes(&converted), p3, "{what}");

            // The oracle: rasterize by hand, then the same conversion.
            let rasterized = doc
                .source_edit(json!({"op":"rasterize","layer":0}), None)
                .unwrap()
                .expect("the rasterization changes the document");
            assert!(!is_bound(&rasterized));
            assert_eq!(
                rasterized.pixels(),
                doc.pixels(),
                "{what}: rasterize keeps pixels"
            );
            let expected = convert(&rasterized, &p3, &settings)
                .unwrap_or_else(|message| panic!("{what}: the oracle refuses: {message}"))
                .expect("the oracle conversion changes the document");
            assert_eq!(converted.dimensions(), expected.dimensions(), "{what}");
            assert!(
                converted.pixels() == expected.pixels(),
                "{what}: differs from rasterize-then-convert"
            );
            assert!(
                converted.pixels() != doc.pixels(),
                "{what}: the conversion moved the samples"
            );
            // The input is untouched.
            assert!(is_bound(&doc), "{what}");
        }
    }
}

#[test]
fn settings_a_source_can_follow_keep_it_bound_whatever_the_flag_says() {
    let p3 = IccProfile::display_p3().bytes().to_vec();
    for typed in [false, true] {
        let doc = bound(typed);
        let flagged = convert(&doc, &p3, &json!({"rasterize_sources": true}))
            .unwrap()
            .expect("the conversion changes the document");
        assert!(is_bound(&flagged), "typed={typed}: the source stays bound");
        let plain = convert(&doc, &p3, &json!({}))
            .unwrap()
            .expect("the conversion changes the document");
        assert!(
            flagged.pixels() == plain.pixels(),
            "typed={typed}: the flag changes nothing here"
        );
    }
}

#[test]
fn the_flag_changes_nothing_without_a_source_and_must_be_a_boolean() {
    let p3 = IccProfile::display_p3().bytes().to_vec();
    for typed in [false, true] {
        let plain = bound(typed)
            .source_edit(json!({"op":"rasterize","layer":0}), None)
            .unwrap()
            .unwrap();
        let settings = json!({"black_point_compensation": true});
        let without = convert(&plain, &p3, &settings).unwrap().unwrap();
        let with = convert(&plain, &p3, &with_flag(&settings))
            .unwrap()
            .unwrap();
        assert!(with.pixels() == without.pixels(), "typed={typed}");
        let refused = preflight(&plain, &p3, &json!({"rasterize_sources": "yes"})).unwrap_err();
        assert_eq!(
            refused, "conversion option rasterize_sources must be a boolean",
            "typed={typed}"
        );
    }
}
