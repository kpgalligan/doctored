//! Convert to Profile on a document holding a retained source, through the
//! public FFI at both depths.
//!
//! The rule under test: relative, perceptual and saturation between two
//! matrix/TRC profiles convert with every binding kept, and their pixels are
//! the DEFAULT conversion's (the shipped pipeline ignores a non-absolute
//! intent); BPC, absolute, and perceptual or saturation with a LUT side
//! refuse with the sentence naming the setting that keeps the sources bound
//! (relative into a LUT converts at 8 bits: `source_conversion_lut.rs`);
//! preserve appearance bakes beside a retained source.
use crate::live_filter_support;
use doctored_core::ffi_doc::dr_doc_with_layer_meta;
use doctored_core::ffi_output_color::*;
use doctored_core::icc::IccProfile;
use image::RgbaImage;
use live_filter_support::*;
use serde_json::Value;
use std::{ffi::CString, ptr};

const LUT_PROFILE: &str = "/Library/Application Support/Adobe/Color/Profiles/VideoHD16-235.icc";
const SETTINGS: &str = "converting with these settings cannot keep";
fn opaque(w: u32, h: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let v = (i * 65535 / (w * h)) as u16;
            [v, 40000, 65535 - v, 65535]
        })
        .collect()
}

fn source_doc(typed: bool) -> Doc {
    Doc::retained(
        typed,
        true,
        8,
        6,
        &opaque(8, 6),
        IccProfile::srgb(),
        IccProfile::srgb(),
    )
}

fn p3() -> Vec<u8> {
    IccProfile::display_p3().bytes().to_vec()
}

/// The preflight's adjustment list, or its refusal sentence.
fn preflight(doc: &Doc, profile: &[u8], options: &str) -> Result<Value, String> {
    let options = CString::new(options).unwrap();
    let mut e = ptr::null_mut();
    let p = unsafe {
        match doc {
            Doc::Byte(d) => dr_doc_conversion_preflight(
                d,
                profile.as_ptr(),
                profile.len(),
                options.as_ptr(),
                std::ptr::null(),
                &mut e,
            ),
            Doc::Typed(d) => dr_precision_doc_conversion_preflight(
                d,
                profile.as_ptr(),
                profile.len(),
                options.as_ptr(),
                LIMIT,
                0,
                std::ptr::null(),
                &mut e,
            ),
        }
    };
    unsafe { json_result(p, e) }.map(|v| v["adjustments"].clone())
}

/// The converted document and its report, `Ok(None)` for the no-op answer.
fn convert(doc: &Doc, profile: &[u8], options: &str) -> Result<Option<(Doc, Value)>, String> {
    let options = CString::new(options).unwrap();
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
    let report = unsafe { text(report) };
    if !error.is_empty() {
        assert!(result.is_none());
        return Err(error);
    }
    Ok(result.map(|d| (d, serde_json::from_str(&report).unwrap())))
}

/// A conversion's refusal sentence; panics when it converted.
fn refusal(result: Result<Option<(Doc, Value)>, String>) -> String {
    match result {
        Err(error) => error,
        Ok(converted) => panic!("expected a refusal, got changed={}", converted.is_some()),
    }
}

/// Whether layer 0 still holds its retained source.
fn binding(doc: &Doc) -> bool {
    match doc {
        Doc::Byte(d) => d.layers[0].effective_source(&d.profile).is_some(),
        Doc::Typed(d) => d.layer(0).and_then(|l| l.effective_source()).is_some(),
    }
}

fn profile_bytes(doc: &Doc) -> Vec<u8> {
    match doc {
        Doc::Byte(d) => d.profile.bytes().to_vec(),
        Doc::Typed(d) => d.profile().bytes().to_vec(),
    }
}

#[test]
fn matrix_equivalent_intents_keep_bindings_and_default_bytes() {
    let p3 = p3();
    for doc in [source_doc(false), source_doc(true)] {
        let (default, _) = convert(&doc, &p3, "{}").unwrap().unwrap();
        for intent in ["perceptual", "saturation"] {
            let options = format!("{{\"intent\":\"{intent}\"}}");
            assert!(
                preflight(&doc, &p3, &options).is_ok(),
                "{intent} typed={}",
                doc.typed()
            );
            let (converted, _) = convert(&doc, &p3, &options).unwrap().unwrap();
            assert!(binding(&converted));
            assert_eq!(profile_bytes(&converted), p3);
            assert_eq!(converted.dimensions(), default.dimensions());
            assert!(
                converted.pixels() == default.pixels(),
                "{intent} typed={} differs from the default conversion",
                doc.typed()
            );
        }
    }
}

#[test]
fn bpc_absolute_and_lut_still_refuse_on_source_documents() {
    let p3 = p3();
    let lut = std::fs::read(LUT_PROFILE).ok();
    if lut.is_none() {
        eprintln!("skipping the LUT leg: VideoHD16-235.icc is not installed");
    }
    for typed in [false, true] {
        let doc = source_doc(typed);
        for options in [
            "{\"black_point_compensation\":true}",
            "{\"intent\":\"absolute\"}",
        ] {
            let refused = preflight(&doc, &p3, options).unwrap_err();
            assert!(refused.contains(SETTINGS), "{refused}");
            let refused = refusal(convert(&doc, &p3, options));
            assert!(refused.contains(SETTINGS), "{refused}");
        }
        if let Some(lut) = &lut {
            let perceptual = "{\"intent\":\"perceptual\"}";
            let refused = preflight(&doc, lut, perceptual).unwrap_err();
            assert!(refused.contains(SETTINGS), "{refused}");
            let refused = refusal(convert(&doc, lut, perceptual));
            assert!(refused.contains(SETTINGS), "{refused}");
        }
        let bpc = "{\"black_point_compensation\":true}";
        assert_eq!(
            preflight(&source_doc(typed), &p3, bpc).unwrap_err(),
            "converting with these settings cannot keep 1 image source bound; \
             convert at relative colorimetric with black point compensation off"
        );
    }
}

fn with_invert(doc: Doc) -> Doc {
    match doc {
        Doc::Byte(d) => {
            let (w, h) = (d.width, d.height);
            let added = d
                .adding_image_layer(0, RgbaImage::new(w, h), "Invert")
                .unwrap();
            let meta = CString::new(r#"{"type":"adjust","op":"invert","params":{}}"#).unwrap();
            let p = unsafe { dr_doc_with_layer_meta(&added, 1, meta.as_ptr()) };
            assert!(!p.is_null());
            Doc::Byte(unsafe { *Box::from_raw(p) })
        }
        Doc::Typed(d) => Doc::Typed(
            d.add_adjustment(
                "Invert",
                r#"{"type":"adjust","evaluation":"precision_v1","op":"invert","params":{}}"#,
                1.0,
                LIMIT,
                0,
                None,
            )
            .unwrap()
            .unwrap(),
        ),
    }
}

#[test]
fn preserve_appearance_bakes_with_retained_sources() {
    let p3 = p3();
    let preserve = "{\"adjustments\":\"preserve_appearance\"}";
    for typed in [false, true] {
        let doc = with_invert(source_doc(typed));
        let report = preflight(&doc, &p3, preserve).unwrap();
        assert_eq!(report.as_array().unwrap().len(), 1, "{report}");
        assert_eq!(report[0]["action"], "baked", "{report}");
        let (converted, landed) = convert(&doc, &p3, preserve).unwrap().unwrap();
        assert_eq!(landed[0]["action"], "baked", "{landed}");
        assert!(binding(&converted));
        assert_eq!(profile_bytes(&converted), p3);
    }
}

#[test]
fn byte_and_typed_preflights_agree() {
    let p3 = p3();
    let (byte, typed) = (source_doc(false), source_doc(true));
    for options in [
        "{}",
        "{\"intent\":\"perceptual\"}",
        "{\"intent\":\"saturation\"}",
        "{\"black_point_compensation\":true}",
        "{\"intent\":\"absolute\"}",
        "{\"adjustments\":\"preserve_appearance\"}",
    ] {
        assert_eq!(
            preflight(&byte, &p3, options),
            preflight(&typed, &p3, options),
            "{options}"
        );
    }
}

/// A Convert into the document's own profile bytes is no change at
/// every option — before the source rule's refusals (BPC, absolute) and
/// before the absolute/BPC pipeline's phantom round trip — at both depths.
#[test]
fn the_documents_own_profile_bytes_are_no_change_at_every_option() {
    let srgb = IccProfile::srgb().bytes().to_vec();
    for typed in [false, true] {
        let doc = source_doc(typed);
        assert_eq!(profile_bytes(&doc), srgb);
        for options in [
            "{}",
            "{\"black_point_compensation\":true}",
            "{\"intent\":\"absolute\"}",
            "{\"adjustments\":\"preserve_appearance\"}",
        ] {
            assert_eq!(
                preflight(&doc, &srgb, options),
                Ok(Value::Array(vec![])),
                "{options} typed={typed}"
            );
            assert!(
                matches!(convert(&doc, &srgb, options), Ok(None)),
                "{options} typed={typed}: a same-bytes convert is no change"
            );
        }
    }
}
