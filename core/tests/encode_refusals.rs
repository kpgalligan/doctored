//! The core-owned refusals of an encode INTO a matrix/TRC profile whose tone
//! curves fail the precision check, through the FFI:
//!
//! - export: every byte and typed export entry point refuses an
//!   encode into such a profile with exactly `dr_icc_describe`'s
//!   `export_refusal`, before any work — except the document's own bytes
//!   under a non-absolute intent with BPC off; `"profile":"document"` never
//!   converts, so it never refuses;
//! - convert: both conversion preflights and the byte Convert refuse
//!   with exactly `convert_refusal`, after the own-bytes rule;
//! - own profile: an edit that encodes colour into a document's own
//!   failing profile answers one capitalised sentence naming it. Its oracle
//!   is written out literally here, not read back from the core;
//! - assign: assigning such a profile is admitted (rasterizing or
//!   not), except to a linear-light document or one holding text/shape
//!   artwork, which refuse in Assign words at both depths (literal oracles);
//!   - a document holding a calibrated (`document_v2`) Exposure/White
//!     Balance layer refuses too, naming the layer, at both depths, hidden
//!     and grouped layers included; a `legacy_srgb` one is admitted;
//! - calibrated: a NEW or EDITED `document_v2` Exposure/White Balance
//!   in such a document is refused at both depths with the own-profile
//!   sentence (`dr_doc_calibrated_refusal` / its typed twin, for either op,
//!   and the destructive `dr_doc_adjust_image`), while an EXISTING 8-bit
//!   layer keeps rendering through flatten, reopen and merge down (literal
//!   oracles);
//! - foreground: the profile-linear estimate refuses with the
//!   own-profile sentence.
//!
//! The failing profile is a gamma-0 `curv` matrix/TRC profile named
//! "Synthetic Bad Gamma", built byte by byte (`common/icc_lut_builders`). The
//! falling-boundary split (#2) is `encode_refusals_falling.rs`, which reuses
//! the `pub(crate)` documents and outcome helpers here.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use crate::live_filter_support;

use live_filter_support::Doc;

use doctored_core::doc::DrDocument;
use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::{
    dr_image_free, dr_image_from_rgba, dr_image_height, dr_image_pixels_rgba, dr_image_width,
    dr_string_free,
};
use doctored_core::ffi_adjust::dr_lut_parse_profile;
use doctored_core::ffi_artwork::{dr_artwork_render, dr_precision_doc_artwork_metadata};
use doctored_core::ffi_calibrated::{
    dr_doc_adjust_image, dr_doc_calibrated_refusal, dr_precision_doc_calibrated_refusal,
};
use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_free, dr_clipboard_payload_from_rgba8, dr_precision_doc_clipboard_paste,
};
use doctored_core::ffi_color::{dr_doc_assign_profile, dr_doc_convert_to_profile};
use doctored_core::ffi_doc::{
    dr_doc_duplicating_layer, dr_doc_flattened, dr_doc_free, dr_doc_from_image, dr_doc_layer_count,
    dr_doc_layer_image, dr_doc_layer_meta, dr_doc_merging_down, dr_doc_open, dr_doc_save_native,
    dr_doc_with_layer_meta, dr_doc_with_layer_name, dr_doc_with_layer_visible,
};
use doctored_core::ffi_evaluation::{dr_doc_set_evaluation, dr_precision_doc_set_evaluation};
use doctored_core::ffi_export::{
    dr_doc_export_encode, dr_doc_export_layer_encode, dr_export_result_free,
    dr_precision_doc_export_encode, dr_precision_doc_export_layer_plan,
    dr_precision_doc_export_plan,
};
use doctored_core::ffi_foreground::dr_image_estimate_foreground;
use doctored_core::ffi_insert::dr_doc_adding_layer_at;
use doctored_core::ffi_layer_clipboard::{dr_doc_paste_layers, dr_precision_doc_paste_layers};
use doctored_core::ffi_output_color::{
    dr_doc_assign_profile_rasterizing, dr_doc_assign_profile_refusal, dr_doc_conversion_preflight,
    dr_icc_describe, dr_precision_doc_assign_profile_rasterizing,
    dr_precision_doc_conversion_preflight, dr_proof_render,
};
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_image_free};
use doctored_core::ffi_precision_color::{
    dr_precision_doc_add_adjustment, dr_precision_doc_assign_profile, dr_precision_doc_color_plan,
};
use doctored_core::ffi_precision_conversion::{
    dr_doc_promote_flattened, dr_doc_promote_structure_plan,
};
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_retained_source::{
    dr_doc_has_retained_source, dr_precision_doc_has_retained_source, dr_precision_doc_source_edit,
};
use doctored_core::icc::IccProfile;
use doctored_core::icc_lut::D50;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::DrImage;
use serde_json::Value;

use crate::icc_lut_builders as builders;
use builders::{build, tag_desc, tag_mft2, tag_wtpt};

/// The own-profile sentence, written independently of the core.
const OWN_PROFILE_SENTENCE: &str = "Doctored cannot encode colour into \u{201c}Synthetic Bad \
     Gamma\u{201d} \u{2014} its matrix/TRC tone curves fail the precision check (they must be \
     finite and monotone). Convert the document to another profile first (Image \u{25b8} Mode \
     \u{25b8} Convert to Profile\u{2026}).";

/// The linear-light Assign sentence, written independently of the
/// core.
const LINEAR_LIGHT_ASSIGN_SENTENCE: &str = "Doctored cannot assign \u{201c}Synthetic Bad \
     Gamma\u{201d} to a document that uses linear-light evaluation \u{2014} its matrix/TRC tone \
     curves fail the precision check (they must be finite and monotone). Switch Image \u{25b8} \
     Evaluation back to Encoded first, or choose another profile.";

/// The artwork Assign sentence, written independently of the core.
const ARTWORK_ASSIGN_SENTENCE: &str = "Doctored cannot assign \u{201c}Synthetic Bad \
     Gamma\u{201d} to a document that holds text or shape layers \u{2014} its matrix/TRC tone \
     curves fail the precision check (they must be finite and monotone), and those layers would \
     have to render into it. Rasterize or delete them first (Layer \u{25b8} Rasterize Artwork), \
     or choose another profile.";

/// The calibrated-layer Assign sentence for the layer "Exposure",
/// written independently of the core.
const CALIBRATED_ASSIGN_SENTENCE: &str = "Doctored cannot assign \u{201c}Synthetic Bad \
     Gamma\u{201d} to a document whose layer \u{201c}Exposure\u{201d} uses calibrated \
     Exposure/White Balance (document_v2) \u{2014} its matrix/TRC tone curves fail the precision \
     check (they must be finite and monotone), and that layer would have to evaluate through \
     them. Merge it down or delete it first, or choose another profile.";

/// An RGB matrix/TRC profile whose three TRCs are a count-1 `curv` of
/// gamma 0, the precision-incompatible curve (`icc_describe_fields`'s
/// `matrix_profile(0)`).
pub(crate) fn bad_profile() -> Vec<u8> {
    let mut curve = b"curv\0\0\0\0".to_vec();
    curve.extend_from_slice(&1u32.to_be_bytes());
    curve.extend_from_slice(&0u16.to_be_bytes());
    let xyz = |v: [f64; 3]| tag_wtpt(v);
    build(
        [0x02, 0x10, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Synthetic Bad Gamma")),
            (b"rXYZ", xyz([0.4361, 0.2225, 0.0139])),
            (b"gXYZ", xyz([0.3851, 0.7169, 0.0971])),
            (b"bXYZ", xyz([0.1431, 0.0606, 0.7141])),
            (b"rTRC", curve.clone()),
            (b"gTRC", curve.clone()),
            (b"bTRC", curve),
        ],
    )
}

fn identity3() -> Vec<u8> {
    tag_mft2(3, 3, 2, &|c| c.to_vec())
}

/// An RGB LUT profile with A2B0 and B2A0, which a proof setup accepts.
pub(crate) fn rgb_lut_proof() -> Vec<u8> {
    build(
        [0x02, 0x20, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Synthetic LUT RGB")),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", identity3()),
            (b"B2A0", identity3()),
        ],
    )
}

/// An identity Lab→Lab abstract profile, a look Color Lookup loads.
fn identity_abstract() -> Vec<u8> {
    build(
        [0x02, 0x20, 0, 0],
        b"abst",
        b"Lab ",
        b"Lab ",
        &[
            (b"desc", tag_desc("Synthetic Abstract")),
            (b"A2B0", identity3()),
        ],
    )
}

pub(crate) unsafe fn text(pointer: *mut c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let string = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    Some(string)
}

fn describe(bytes: &[u8]) -> Value {
    let mut err = ptr::null_mut();
    let raw = unsafe { dr_icc_describe(bytes.as_ptr(), bytes.len(), &mut err) };
    let json = unsafe { text(raw) }.expect("describes");
    assert_eq!(unsafe { text(err) }, None);
    serde_json::from_str(&json).expect("describe JSON")
}

/// A two-pixel byte document, assigned `profile` when one is given.
pub(crate) fn byte_document(profile: Option<&[u8]>) -> *mut DrDocument {
    let pixels = [200u8, 40, 90, 255, 10, 220, 130, 255];
    let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), 2, 1) };
    assert!(!image.is_null());
    let plain = unsafe { dr_doc_from_image(image) };
    assert!(!plain.is_null());
    unsafe { dr_image_free(image) };
    let Some(profile) = profile else {
        return plain;
    };
    let mut err = ptr::null_mut();
    let assigned =
        unsafe { dr_doc_assign_profile(plain, profile.as_ptr(), profile.len(), &mut err) };
    assert!(!assigned.is_null(), "{:?}", unsafe { text(err) });
    unsafe { dr_doc_free(plain) };
    assigned
}

/// A two-pixel sRGB u16 document, assigned `profile` when one is given.
pub(crate) fn typed_document(profile: Option<&[u8]>) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(
        2,
        1,
        vec![[32123, 12345, 56789, 65535], [1000, 60000, 30000, 65535]],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let doc =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    let plain = Box::into_raw(Box::new(doc));
    let Some(profile) = profile else {
        return plain;
    };
    let mut err = ptr::null_mut();
    let assigned = unsafe {
        dr_precision_doc_assign_profile(
            plain,
            profile.as_ptr(),
            profile.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!assigned.is_null(), "{:?}", unsafe { text(err) });
    unsafe { dr_precision_doc_free(plain) };
    assigned
}

/// `Ok(())` for a non-NULL export result (freed here), else the error.
fn export_outcome(
    encode: impl Fn(*const c_char, *mut *mut c_char) -> *mut ExportResult,
    request: &str,
) -> Result<(), String> {
    let json = CString::new(request).expect("request");
    let mut err = ptr::null_mut();
    let result = encode(json.as_ptr(), &mut err);
    if result.is_null() {
        return Err(unsafe { text(err) }.unwrap_or_default());
    }
    assert_eq!(unsafe { text(err) }, None);
    unsafe { dr_export_result_free(result) };
    Ok(())
}

/// `Ok(())` for a non-NULL owned string (freed here), else the error.
fn string_outcome(
    call: impl Fn(*const c_char, *mut *mut c_char) -> *mut c_char,
    request: &str,
) -> Result<(), String> {
    let json = CString::new(request).expect("request");
    let mut err = ptr::null_mut();
    let result = call(json.as_ptr(), &mut err);
    match unsafe { text(result) } {
        Some(_) => {
            assert_eq!(unsafe { text(err) }, None);
            Ok(())
        }
        None => Err(unsafe { text(err) }.unwrap_or_default()),
    }
}

fn byte_export(doc: *const DrDocument, request: &str) -> Result<(), String> {
    export_outcome(
        |json, err| unsafe { dr_doc_export_encode(doc, ptr::null(), json, ptr::null(), err) },
        request,
    )
}

fn byte_layer_export(doc: *const DrDocument, request: &str) -> Result<(), String> {
    export_outcome(
        |json, err| unsafe { dr_doc_export_layer_encode(doc, 0, false, json, ptr::null(), err) },
        request,
    )
}

fn typed_export(doc: *const PrecisionDocument, request: &str) -> Result<(), String> {
    export_outcome(
        |json, err| unsafe {
            dr_precision_doc_export_encode(
                doc,
                ptr::null(),
                json,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                err,
            )
        },
        request,
    )
}

fn typed_export_plan(doc: *const PrecisionDocument, request: &str) -> Result<(), String> {
    string_outcome(
        |json, err| unsafe { dr_precision_doc_export_plan(doc, ptr::null(), json, err) },
        request,
    )
}

fn typed_layer_plan(doc: *const PrecisionDocument, request: &str) -> Result<(), String> {
    string_outcome(
        |json, err| unsafe { dr_precision_doc_export_layer_plan(doc, 0, false, json, err) },
        request,
    )
}

/// The failing profile written to a temporary file, and a request builder
/// naming it as the export destination.
struct BadFile {
    _dir: tempfile::TempDir,
    path: Value,
}

impl BadFile {
    fn new(bytes: &[u8]) -> Self {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("bad.icc");
        std::fs::write(&path, bytes).expect("write profile");
        let path = Value::from(path.to_string_lossy().as_ref());
        BadFile { _dir: dir, path }
    }

    fn request(&self, extra: &str) -> String {
        format!(
            r#"{{"format":"png","profile":"file","profile_path":{}{extra}}}"#,
            self.path
        )
    }
}

// --------------------------------------------------------------- export --

#[test]
fn every_export_entry_point_refuses_an_encode_into_a_failing_profile_with_one_sentence() {
    let bad = bad_profile();
    let sentence = describe(&bad)["export_refusal"]
        .as_str()
        .expect("export_refusal")
        .to_string();
    let file = BadFile::new(&bad);

    let byte = byte_document(None);
    let typed = typed_document(None);
    let at16 = file.request(r#","bit_depth":16"#);
    let at8 = file.request(r#","bit_depth":8"#);
    for (arm, outcome) in [
        ("byte", byte_export(byte, &file.request(""))),
        ("byte layer", byte_layer_export(byte, &file.request(""))),
        ("typed 16", typed_export(typed, &at16)),
        ("typed 8", typed_export(typed, &at8)),
        ("typed 16 plan", typed_export_plan(typed, &at16)),
        ("typed 8 plan", typed_export_plan(typed, &at8)),
        ("typed layer plan", typed_layer_plan(typed, &at16)),
    ] {
        assert_eq!(outcome, Err(sentence.clone()), "{arm}");
    }
    unsafe { dr_doc_free(byte) };
    unsafe { dr_precision_doc_free(typed) };
}

#[test]
fn a_document_in_the_failing_profile_exports_its_own_bytes_only_without_a_conversion() {
    let bad = bad_profile();
    let sentence = describe(&bad)["export_refusal"]
        .as_str()
        .expect("export_refusal")
        .to_string();
    let file = BadFile::new(&bad);
    let document = |extra: &str| format!(r#"{{"format":"png","profile":"document"{extra}}}"#);

    let byte = byte_document(Some(&bad));
    let typed = typed_document(Some(&bad));
    for (arm, export) in [
        (
            "byte",
            &(|request: &str| byte_export(byte, request)) as &dyn Fn(&str) -> Result<(), String>,
        ),
        ("byte layer", &|request: &str| {
            byte_layer_export(byte, request)
        }),
        ("typed 16", &|request: &str| {
            typed_export(typed, &request.replacen('{', r#"{"bit_depth":16,"#, 1))
        }),
        ("typed 8", &|request: &str| typed_export(typed, request)),
        ("typed 16 plan", &|request: &str| {
            typed_export_plan(typed, &request.replacen('{', r#"{"bit_depth":16,"#, 1))
        }),
        ("typed layer plan", &|request: &str| {
            typed_layer_plan(typed, request)
        }),
    ] {
        assert_eq!(
            export(&file.request("")),
            Ok(()),
            "{arm}: own bytes, relative"
        );
        for extra in [
            r#","black_point_compensation":true"#,
            r#","intent":"absolute""#,
        ] {
            assert_eq!(
                export(&file.request(extra)),
                Err(sentence.clone()),
                "{arm}: own bytes {extra}"
            );
            assert_eq!(export(&document(extra)), Ok(()), "{arm}: document {extra}");
        }
    }
    unsafe { dr_doc_free(byte) };
    unsafe { dr_precision_doc_free(typed) };
}

// -------------------------------------------------------------- convert --

#[test]
fn every_convert_entry_point_refuses_a_failing_destination_after_the_own_bytes_rule() {
    let bad = bad_profile();
    let sentence = describe(&bad)["convert_refusal"]
        .as_str()
        .expect("convert_refusal")
        .to_string();

    let byte_preflight = |doc: *const DrDocument| {
        let mut err = ptr::null_mut();
        let json = unsafe {
            dr_doc_conversion_preflight(
                doc,
                bad.as_ptr(),
                bad.len(),
                ptr::null(),
                std::ptr::null(),
                &mut err,
            )
        };
        match unsafe { text(json) } {
            Some(_) => Ok(()),
            None => Err(unsafe { text(err) }.unwrap_or_default()),
        }
    };
    let typed_preflight = |doc: *const PrecisionDocument| {
        let mut err = ptr::null_mut();
        let json = unsafe {
            dr_precision_doc_conversion_preflight(
                doc,
                bad.as_ptr(),
                bad.len(),
                ptr::null(),
                REFERENCE_WORKING_BYTES,
                0,
                std::ptr::null(),
                &mut err,
            )
        };
        match unsafe { text(json) } {
            Some(_) => Ok(()),
            None => Err(unsafe { text(err) }.unwrap_or_default()),
        }
    };

    let byte = byte_document(None);
    let typed = typed_document(None);
    assert_eq!(
        byte_preflight(byte),
        Err(sentence.clone()),
        "byte preflight"
    );
    assert_eq!(
        typed_preflight(typed),
        Err(sentence.clone()),
        "typed preflight"
    );
    let mut err = ptr::null_mut();
    let converted = unsafe { dr_doc_convert_to_profile(byte, bad.as_ptr(), bad.len(), &mut err) };
    assert!(converted.is_null());
    assert_eq!(unsafe { text(err) }, Some(sentence), "byte convert");
    unsafe { dr_doc_free(byte) };
    unsafe { dr_precision_doc_free(typed) };

    let byte = byte_document(Some(&bad));
    let typed = typed_document(Some(&bad));
    assert_eq!(byte_preflight(byte), Ok(()), "byte own bytes");
    assert_eq!(typed_preflight(typed), Ok(()), "typed own bytes");
    unsafe { dr_doc_free(byte) };
    unsafe { dr_precision_doc_free(typed) };
}

// ---------------------------------------------------------- own profile --

#[test]
fn edits_encoding_into_a_documents_own_failing_profile_name_it() {
    let bad = bad_profile();

    // Byte: linear-light evaluation.
    let byte = byte_document(Some(&bad));
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_set_evaluation(byte, 1, &mut err) };
    assert!(out.is_null());
    assert_eq!(
        unsafe { text(err) }.as_deref(),
        Some(OWN_PROFILE_SENTENCE),
        "byte linear light"
    );
    unsafe { dr_doc_free(byte) };

    // Byte: a soft proof, whose output space is the document's own profile.
    let pixels = [200u8, 40, 90, 255, 10, 220, 130, 255];
    let flat = unsafe { dr_image_from_rgba(pixels.as_ptr(), 2, 1) };
    assert!(!flat.is_null());
    let proof = rgb_lut_proof();
    let setup = CString::new("{}").expect("setup");
    let mut info = ptr::null_mut();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_proof_render(
            flat,
            bad.as_ptr(),
            bad.len(),
            proof.as_ptr(),
            proof.len(),
            setup.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut info,
            &mut err,
        )
    };
    assert!(out.is_null());
    unsafe { text(info) };
    assert_eq!(
        unsafe { text(err) }.as_deref(),
        Some(OWN_PROFILE_SENTENCE),
        "byte proof"
    );
    unsafe { dr_image_free(flat) };

    // Typed: linear-light evaluation and a calibrated Exposure.
    let typed = typed_document(Some(&bad));
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_evaluation(typed, 1, &mut err) };
    assert!(out.is_null());
    assert_eq!(
        unsafe { text(err) }.as_deref(),
        Some(OWN_PROFILE_SENTENCE),
        "typed linear light"
    );
    let name = CString::new("Exposure").expect("name");
    let exposure = CString::new(
        serde_json::json!({
            "type": "adjust",
            "evaluation": "precision_v1",
            "op": "exposure",
            "params": {"exposure": 0.7, "evaluation": "document_v2"},
        })
        .to_string(),
    )
    .expect("description");
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            typed,
            name.as_ptr(),
            exposure.as_ptr(),
            1.0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(
        unsafe { text(err) }.as_deref(),
        Some(OWN_PROFILE_SENTENCE),
        "typed calibrated exposure"
    );
    unsafe { dr_precision_doc_free(typed) };

    // Color Lookup: an abstract look is baked through the document's own
    // profile, and the look file is not at fault, so no path prefix.
    let dir = tempfile::TempDir::new().expect("tempdir");
    let look = dir.path().join("look.icc");
    std::fs::write(&look, identity_abstract()).expect("write look");
    let look = CString::new(look.to_string_lossy().as_ref()).expect("path");
    let mut err = ptr::null_mut();
    let out = unsafe { dr_lut_parse_profile(look.as_ptr(), bad.as_ptr(), bad.len(), &mut err) };
    assert_eq!(unsafe { text(out) }, None);
    assert_eq!(
        unsafe { text(err) }.as_deref(),
        Some(OWN_PROFILE_SENTENCE),
        "abstract look"
    );
}

// --------------------------------------------------------------- assign --

/// `Ok(())` for a non-NULL typed document (freed here), else the error.
pub(crate) fn typed_outcome(out: *mut PrecisionDocument, err: *mut c_char) -> Result<(), String> {
    if out.is_null() {
        return Err(unsafe { text(err) }.unwrap_or_default());
    }
    assert_eq!(unsafe { text(err) }, None);
    unsafe { dr_precision_doc_free(out) };
    Ok(())
}

/// `Ok(())` for a non-NULL byte document (freed here), else the error.
fn byte_outcome(out: *mut DrDocument, err: *mut c_char) -> Result<(), String> {
    if out.is_null() {
        return Err(unsafe { text(err) }.unwrap_or_default());
    }
    assert_eq!(unsafe { text(err) }, None);
    unsafe { dr_doc_free(out) };
    Ok(())
}

/// The byte Assign refusal, plain Assign and rasterizing Assign of `profile`.
pub(crate) fn byte_assigns(doc: *const DrDocument, profile: &[u8]) -> [Result<(), String>; 3] {
    let refusal = unsafe { dr_doc_assign_profile_refusal(doc, profile.as_ptr(), profile.len()) };
    let refusal = unsafe { text(refusal) }.map_or(Ok(()), Err);
    let mut err = ptr::null_mut();
    let plain = unsafe { dr_doc_assign_profile(doc, profile.as_ptr(), profile.len(), &mut err) };
    let plain = byte_outcome(plain, err);
    let mut err = ptr::null_mut();
    let rasterizing = unsafe {
        dr_doc_assign_profile_rasterizing(doc, profile.as_ptr(), profile.len(), &mut err)
    };
    [refusal, plain, byte_outcome(rasterizing, err)]
}

/// The typed Assign plan, plain Assign and rasterizing Assign of `profile`.
pub(crate) fn typed_assigns(
    doc: *const PrecisionDocument,
    profile: &[u8],
) -> [Result<(), String>; 3] {
    let mut err = ptr::null_mut();
    let plan = unsafe {
        dr_precision_doc_color_plan(
            doc,
            profile.as_ptr(),
            profile.len(),
            false,
            REFERENCE_WORKING_BYTES,
            0,
            &mut err,
        )
    };
    let plan = match unsafe { text(plan) } {
        Some(_) => Ok(()),
        None => Err(unsafe { text(err) }.unwrap_or_default()),
    };
    let mut err = ptr::null_mut();
    let plain = unsafe {
        dr_precision_doc_assign_profile(
            doc,
            profile.as_ptr(),
            profile.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let plain = typed_outcome(plain, err);
    let mut err = ptr::null_mut();
    let rasterizing = unsafe {
        dr_precision_doc_assign_profile_rasterizing(
            doc,
            profile.as_ptr(),
            profile.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    [plan, plain, typed_outcome(rasterizing, err)]
}

#[test]
fn assigning_a_failing_profile_is_admitted_with_or_without_rasterizing_sources() {
    let bad = bad_profile();
    let pixels: Vec<[u16; 4]> = (0..48u16)
        .map(|i| [i * 1300, 40000, 65535 - i * 1300, 65535])
        .collect();
    for typed in [false, true] {
        let doc = Doc::retained(
            typed,
            true,
            8,
            6,
            &pixels,
            IccProfile::srgb(),
            IccProfile::srgb(),
        );
        match &doc {
            Doc::Byte(d) => {
                let mut err = ptr::null_mut();
                let out = unsafe {
                    dr_doc_assign_profile_rasterizing(d, bad.as_ptr(), bad.len(), &mut err)
                };
                assert!(!out.is_null(), "byte rasterizing: {:?}", unsafe {
                    text(err)
                });
                assert!(
                    !unsafe { dr_doc_has_retained_source(out, 0) },
                    "byte source"
                );
                assert_eq!(
                    &unsafe { &*out }.profile.bytes()[..],
                    &bad[..],
                    "byte profile"
                );
                unsafe { dr_doc_free(out) };
            }
            Doc::Typed(d) => {
                let [plan, plain, rasterizing] = typed_assigns(d, &bad);
                assert_eq!(plan, Ok(()), "typed plan");
                let plain = plain.expect_err("typed plain assign refuses the source");
                assert!(
                    plain.starts_with("assigning a profile requires rasterizing"),
                    "{plain}"
                );
                assert_eq!(rasterizing, Ok(()), "typed rasterizing");
                let mut err = ptr::null_mut();
                let out = unsafe {
                    dr_precision_doc_assign_profile_rasterizing(
                        d,
                        bad.as_ptr(),
                        bad.len(),
                        REFERENCE_WORKING_BYTES,
                        0,
                        ptr::null(),
                        &mut err,
                    )
                };
                assert!(!out.is_null(), "typed rasterizing: {:?}", unsafe {
                    text(err)
                });
                assert!(
                    !unsafe { dr_precision_doc_has_retained_source(out, 0) },
                    "typed source"
                );
                assert_eq!(
                    &unsafe { &*out }.profile().bytes()[..],
                    &bad[..],
                    "typed profile"
                );
                unsafe { dr_precision_doc_free(out) };
            }
        }
    }
}

#[test]
fn assigning_a_failing_profile_to_a_linear_light_document_refuses_in_assign_words() {
    let bad = bad_profile();
    let refused = || Err(LINEAR_LIGHT_ASSIGN_SENTENCE.to_string());

    let plain = byte_document(None);
    let mut err = ptr::null_mut();
    let byte = unsafe { dr_doc_set_evaluation(plain, 1, &mut err) };
    assert!(!byte.is_null(), "{:?}", unsafe { text(err) });
    unsafe { dr_doc_free(plain) };
    assert_eq!(byte_assigns(byte, &bad), [refused(), refused(), refused()]);
    unsafe { dr_doc_free(byte) };

    let plain = typed_document(None);
    let mut err = ptr::null_mut();
    let typed = unsafe { dr_precision_doc_set_evaluation(plain, 1, &mut err) };
    assert!(!typed.is_null(), "{:?}", unsafe { text(err) });
    unsafe { dr_precision_doc_free(plain) };
    assert_eq!(
        typed_assigns(typed, &bad),
        [refused(), refused(), refused()]
    );
    unsafe { dr_precision_doc_free(typed) };
}

#[test]
fn assigning_a_failing_profile_to_a_document_with_artwork_refuses_in_assign_words() {
    let bad = bad_profile();
    let refused = || Err(ARTWORK_ASSIGN_SENTENCE.to_string());
    let artwork = CString::new(r#"{"type":"text","version":1}"#).expect("description");

    let plain = byte_document(None);
    let byte = unsafe { dr_doc_with_layer_meta(plain, 0, artwork.as_ptr()) };
    assert!(!byte.is_null());
    unsafe { dr_doc_free(plain) };
    assert_eq!(byte_assigns(byte, &bad), [refused(), refused(), refused()]);
    unsafe { dr_doc_free(byte) };

    let plain = typed_document(None);
    let mut err = ptr::null_mut();
    let typed = unsafe {
        dr_precision_doc_artwork_metadata(
            plain,
            0,
            artwork.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            &mut err,
        )
    };
    assert!(!typed.is_null(), "{:?}", unsafe { text(err) });
    unsafe { dr_precision_doc_free(plain) };
    assert_eq!(
        typed_assigns(typed, &bad),
        [refused(), refused(), refused()]
    );

    // The typed check reaches artwork nested inside a group.
    let group = CString::new(r#"{"op":"group_layers","layers":[0],"name":"G"}"#).expect("group");
    let mut err = ptr::null_mut();
    let grouped = unsafe {
        dr_precision_doc_edit(
            typed,
            group.as_ptr(),
            ptr::null(),
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!grouped.is_null(), "{:?}", unsafe { text(err) });
    unsafe { dr_precision_doc_free(typed) };
    assert_eq!(
        typed_assigns(grouped, &bad),
        [refused(), refused(), refused()]
    );
    unsafe { dr_precision_doc_free(grouped) };
}

#[test]
fn assigning_a_failing_profile_to_a_document_with_a_calibrated_layer_refuses_in_assign_words() {
    let bad = bad_profile();
    let refused = || Err(CALIBRATED_ASSIGN_SENTENCE.to_string());
    let admitted = || [Ok(()), Ok(()), Ok(())];
    let name = CString::new("Exposure").expect("name");

    // Byte: the evaluation decides, not the op, and a hidden layer counts.
    for evaluation in ["document_v2", "legacy_srgb"] {
        let meta = CString::new(
            serde_json::json!({
                "type": "adjust",
                "op": "exposure",
                "params": {"exposure": 0.7, "evaluation": evaluation},
            })
            .to_string(),
        )
        .expect("meta");
        let plain = byte_document(None);
        let adjusted = unsafe { dr_doc_with_layer_meta(plain, 0, meta.as_ptr()) };
        assert!(!adjusted.is_null());
        unsafe { dr_doc_free(plain) };
        let byte = unsafe { dr_doc_with_layer_name(adjusted, 0, name.as_ptr()) };
        assert!(!byte.is_null());
        unsafe { dr_doc_free(adjusted) };
        if evaluation == "legacy_srgb" {
            assert_eq!(byte_assigns(byte, &bad), admitted(), "byte legacy");
            unsafe { dr_doc_free(byte) };
            continue;
        }
        assert_eq!(
            byte_assigns(byte, &bad),
            [refused(), refused(), refused()],
            "byte"
        );
        // Linear-light is asked first at this depth too.
        let mut err = ptr::null_mut();
        let linear = unsafe { dr_doc_set_evaluation(byte, 1, &mut err) };
        assert!(!linear.is_null(), "{:?}", unsafe { text(err) });
        let linear_refused = || Err(LINEAR_LIGHT_ASSIGN_SENTENCE.to_string());
        assert_eq!(
            byte_assigns(linear, &bad),
            [linear_refused(), linear_refused(), linear_refused()],
            "byte linear light wins"
        );
        unsafe { dr_doc_free(linear) };
        let hidden = unsafe { dr_doc_with_layer_visible(byte, 0, false) };
        assert!(!hidden.is_null());
        unsafe { dr_doc_free(byte) };
        assert_eq!(
            byte_assigns(hidden, &bad),
            [refused(), refused(), refused()],
            "byte hidden"
        );
        unsafe { dr_doc_free(hidden) };
    }

    // Typed: plain, inside an isolated group and inside a pass-through group.
    for evaluation in ["document_v2", "legacy_srgb"] {
        let description = CString::new(
            serde_json::json!({
                "type": "adjust",
                "evaluation": "precision_v1",
                "op": "exposure",
                "params": {"exposure": 0.7, "evaluation": evaluation},
            })
            .to_string(),
        )
        .expect("description");
        let plain = typed_document(None);
        let mut err = ptr::null_mut();
        let typed = unsafe {
            dr_precision_doc_add_adjustment(
                plain,
                name.as_ptr(),
                description.as_ptr(),
                1.0,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!typed.is_null(), "{:?}", unsafe { text(err) });
        unsafe { dr_precision_doc_free(plain) };
        if evaluation == "legacy_srgb" {
            assert_eq!(typed_assigns(typed, &bad), admitted(), "typed legacy");
            unsafe { dr_precision_doc_free(typed) };
            continue;
        }
        assert_eq!(
            typed_assigns(typed, &bad),
            [refused(), refused(), refused()],
            "typed"
        );
        let hide =
            CString::new(r#"{"op":"layer_visible","layer":1,"visible":false}"#).expect("hide");
        let mut err = ptr::null_mut();
        let hidden = unsafe {
            dr_precision_doc_edit(
                typed,
                hide.as_ptr(),
                ptr::null(),
                0,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!hidden.is_null(), "{:?}", unsafe { text(err) });
        assert_eq!(
            typed_assigns(hidden, &bad),
            [refused(), refused(), refused()],
            "typed hidden"
        );
        unsafe { dr_precision_doc_free(hidden) };
        // Linear-light is asked first: a calibrated layer under linear-light
        // evaluation answers the linear-light sentence.
        let mut err = ptr::null_mut();
        let linear = unsafe { dr_precision_doc_set_evaluation(typed, 1, &mut err) };
        assert!(!linear.is_null(), "{:?}", unsafe { text(err) });
        let linear_refused = || Err(LINEAR_LIGHT_ASSIGN_SENTENCE.to_string());
        assert_eq!(
            typed_assigns(linear, &bad),
            [linear_refused(), linear_refused(), linear_refused()],
            "typed linear light wins"
        );
        unsafe { dr_precision_doc_free(linear) };
        for pass_through in [false, true] {
            let group = CString::new(
                serde_json::json!({
                    "op": "group_layers",
                    "layers": [1],
                    "name": "G",
                    "pass_through": pass_through,
                })
                .to_string(),
            )
            .expect("group");
            let mut err = ptr::null_mut();
            let grouped = unsafe {
                dr_precision_doc_edit(
                    typed,
                    group.as_ptr(),
                    ptr::null(),
                    0,
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    &mut err,
                )
            };
            assert!(!grouped.is_null(), "{:?}", unsafe { text(err) });
            assert_eq!(
                typed_assigns(grouped, &bad),
                [refused(), refused(), refused()],
                "typed grouped, pass_through {pass_through}"
            );
            unsafe { dr_precision_doc_free(grouped) };
        }
        unsafe { dr_precision_doc_free(typed) };
    }
}

#[test]
fn the_paint_artwork_color16_and_converted_paste_sites_name_the_own_profile() {
    let bad = bad_profile();
    let own = || Err(OWN_PROFILE_SENTENCE.to_string());
    let typed = typed_document(Some(&bad));

    // Paint: sRGB ink encoded into the document's profile.
    let paint = CString::new(
        r#"{"op":"paint","layer":0,"color":[1,0,0,1],"color_space":"srgb","opacity":1.0}"#,
    )
    .expect("paint");
    let mut err = ptr::null_mut();
    let plan = unsafe {
        dr_precision_doc_edit_plan(typed, paint.as_ptr(), REFERENCE_WORKING_BYTES, 0, &mut err)
    };
    let plan: Value = serde_json::from_str(
        &unsafe { text(plan) }.unwrap_or_else(|| panic!("{:?}", unsafe { text(err) })),
    )
    .expect("plan JSON");
    assert_eq!(plan["input_samples"], 2);
    let coverage = [65535u16; 2];
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            typed,
            paint.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(typed_outcome(out, err), own(), "paint");

    // Artwork: coverage ink rendered into the document's profile.
    let components =
        CString::new(r#"[{"kind":"coverage","color":[1,0,0,1]}]"#).expect("components");
    let samples = [65535u16];
    let mut err = ptr::null_mut();
    let image = unsafe {
        dr_artwork_render(
            1,
            1,
            components.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            ptr::null(),
            0,
            bad.as_ptr(),
            bad.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let artwork = if image.is_null() {
        Err(unsafe { text(err) }.unwrap_or_default())
    } else {
        unsafe { dr_precision_image_free(image) };
        Ok(())
    };
    assert_eq!(artwork, own(), "artwork");

    // Color16: an sRGB 16-bit source placed into the document's profile.
    let source = typed_document(None);
    let place =
        CString::new(r#"{"op":"place","source_id":"placed-1","name":"Placed","x":0,"y":0}"#)
            .expect("place");
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_source_edit(
            typed,
            source,
            place.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(typed_outcome(out, err), own(), "color16 place");
    unsafe { dr_precision_doc_free(source) };

    // Converted paste: an sRGB byte payload converted into the profile.
    let srgb = IccProfile::srgb();
    let pixels = [200u8, 40, 90, 255, 10, 220, 130, 255];
    let mut err = ptr::null_mut();
    let payload = unsafe {
        dr_clipboard_payload_from_rgba8(
            pixels.as_ptr(),
            pixels.len(),
            2,
            1,
            srgb.bytes().as_ptr(),
            srgb.bytes().len(),
            0,
            0,
            72.0,
            72.0,
            REFERENCE_WORKING_BYTES,
            0,
            &mut err,
        )
    };
    assert!(!payload.is_null(), "{:?}", unsafe { text(err) });
    let paste = CString::new(
        r#"{"name":"Pasted","anchor":0,"position":"above","placement":"in_place","profile":"convert"}"#,
    )
    .expect("paste");
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_clipboard_paste(
            typed,
            payload,
            paste.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(typed_outcome(out, err), own(), "converted paste");
    unsafe { dr_clipboard_payload_free(payload) };
    unsafe { dr_precision_doc_free(typed) };
}

/// The no-model calibrated sentence, written independently of the core.
const PROFILE_SENTENCE: &str = "Calibrated Exposure and White Balance require an RGB matrix/TRC \
     document profile. Convert or assign a supported profile first.";

/// `dr_doc_adjust_image` on a fresh two-pixel image with the document's
/// pixels: `Ok(())` for a non-NULL image (freed here), else the error.
pub(crate) fn byte_adjust(doc: *const DrDocument, op: &str, params: &str) -> Result<(), String> {
    let pixels = [200u8, 40, 90, 255, 10, 220, 130, 255];
    let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), 2, 1) };
    assert!(!image.is_null());
    let op = CString::new(op).expect("op");
    let params = CString::new(params).expect("params");
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_adjust_image(doc, image, op.as_ptr(), params.as_ptr(), &mut err) };
    unsafe { dr_image_free(image) };
    if out.is_null() {
        return Err(unsafe { text(err) }.unwrap_or_default());
    }
    assert_eq!(unsafe { text(err) }, None);
    unsafe { dr_image_free(out) };
    Ok(())
}

/// The RGBA8 bytes of `image`, which is freed here.
fn image_bytes(image: *mut DrImage) -> Vec<u8> {
    assert!(!image.is_null());
    let length = unsafe { dr_image_width(image) * dr_image_height(image) } as usize * 4;
    let bytes = unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(image), length) }.to_vec();
    unsafe { dr_image_free(image) };
    bytes
}

fn flattened(doc: *const DrDocument) -> Vec<u8> {
    let mut err = ptr::null_mut();
    let image = unsafe { dr_doc_flattened(doc, 0, std::ptr::null(), &mut err) };
    assert!(!image.is_null(), "{:?}", unsafe { text(err) });
    image_bytes(image)
}

#[test]
fn a_new_or_edited_calibrated_adjustment_refuses_in_a_failing_own_profile_at_both_depths() {
    let bad = bad_profile();
    let byte = byte_document(Some(&bad));
    let typed = typed_document(Some(&bad));
    // Gamma 0 is a repaired curve: both ops refuse, and NULL answers as
    // White Balance.
    let ops = [Some("exposure"), Some("white_balance"), None];
    for op in ops {
        let name = op.map(|o| CString::new(o).expect("op"));
        let op = name.as_ref().map_or(ptr::null(), |o| o.as_ptr());
        assert_eq!(
            unsafe { text(dr_doc_calibrated_refusal(byte, op)) }.as_deref(),
            Some(OWN_PROFILE_SENTENCE)
        );
        assert_eq!(
            unsafe { text(dr_precision_doc_calibrated_refusal(typed, op)) }.as_deref(),
            Some(OWN_PROFILE_SENTENCE)
        );
    }

    // sRGB documents are admitted at both depths; a LUT-only profile has no
    // matrix/TRC model at all, for either op.
    let srgb_byte = byte_document(None);
    let srgb_typed = typed_document(None);
    let lut = byte_document(Some(&rgb_lut_proof()));
    for op in ops {
        let name = op.map(|o| CString::new(o).expect("op"));
        let op = name.as_ref().map_or(ptr::null(), |o| o.as_ptr());
        assert_eq!(
            unsafe { text(dr_doc_calibrated_refusal(srgb_byte, op)) },
            None
        );
        assert_eq!(
            unsafe { text(dr_precision_doc_calibrated_refusal(srgb_typed, op)) },
            None
        );
        assert_eq!(
            unsafe { text(dr_doc_calibrated_refusal(lut, op)) }.as_deref(),
            Some(PROFILE_SENTENCE)
        );
    }
    unsafe { dr_doc_free(srgb_byte) };
    unsafe { dr_precision_doc_free(srgb_typed) };
    unsafe { dr_doc_free(lut) };

    // The destructive apply is the core backstop: document_v2 refuses with
    // the same sentence, legacy_srgb still runs its numeric kernel.
    let own = || Err(OWN_PROFILE_SENTENCE.to_string());
    assert_eq!(
        byte_adjust(
            byte,
            "exposure",
            r#"{"exposure":0.7,"evaluation":"document_v2"}"#
        ),
        own(),
        "exposure"
    );
    assert_eq!(
        byte_adjust(
            byte,
            "white_balance",
            r#"{"temperature":5200,"evaluation":"document_v2"}"#
        ),
        own(),
        "white balance"
    );
    assert_eq!(
        byte_adjust(
            byte,
            "exposure",
            r#"{"exposure":0.7,"evaluation":"legacy_srgb"}"#
        ),
        Ok(()),
        "legacy"
    );
    unsafe { dr_doc_free(byte) };
    unsafe { dr_precision_doc_free(typed) };
}

#[test]
fn an_existing_calibrated_layer_in_a_failing_own_profile_keeps_rendering_at_8_bits() {
    let bad = bad_profile();
    let plain = byte_document(Some(&bad));
    let name = CString::new("Exposure").expect("name");
    let added = unsafe { dr_doc_adding_layer_at(plain, 0, 0, name.as_ptr()) };
    assert!(!added.is_null());
    unsafe { dr_doc_free(plain) };
    let meta = serde_json::json!({
        "type": "adjust",
        "op": "exposure",
        "params": {"exposure": 0.7, "evaluation": "document_v2"},
    })
    .to_string();
    let meta_c = CString::new(meta.clone()).expect("meta");
    let doc = unsafe { dr_doc_with_layer_meta(added, 1, meta_c.as_ptr()) };
    assert!(!doc.is_null());
    unsafe { dr_doc_free(added) };

    // Oracle, derived by hand: the gamma-0 TRC reads as linear (identity
    // decode); the gain is 2^0.7 ≈ 1.6245; the profile→reference and
    // reference→profile matrices are inverses, so a uniform gain passes
    // straight through; the forward-threshold encode of a linear curve
    // rounds. 200·1.6245 = 324.9 → 255, 40 → 64.98 → 65, 90 → 146.2 → 146,
    // 10 → 16.2 → 16, 220 → 357.4 → 255, 130 → 211.2 → 211; alpha untouched.
    let oracle = vec![255u8, 65, 146, 255, 16, 255, 211, 255];
    assert_eq!(flattened(doc), oracle, "render");

    // Save and reopen: the description is kept and renders the same.
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = CString::new(dir.path().join("kept.rz").to_str().expect("utf8")).expect("path");
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(doc, path.as_ptr(), &mut err) },
        "{:?}",
        unsafe { text(err) }
    );
    let mut err = ptr::null_mut();
    let reopened = unsafe { dr_doc_open(path.as_ptr(), &mut err) };
    assert!(!reopened.is_null(), "{:?}", unsafe { text(err) });
    assert_eq!(
        unsafe { text(dr_doc_layer_meta(reopened, 1)) }.as_deref(),
        Some(meta.as_str())
    );
    assert_eq!(flattened(reopened), oracle, "reopened render");
    unsafe { dr_doc_free(reopened) };

    // Merge down is the exit the calibrated-layer Assign sentence names.
    let merged = unsafe { dr_doc_merging_down(doc, 1) };
    assert!(!merged.is_null());
    assert_eq!(unsafe { dr_doc_layer_count(merged) }, 1);
    assert_eq!(
        image_bytes(unsafe { dr_doc_layer_image(merged, 0) }),
        oracle,
        "merged"
    );
    unsafe { dr_doc_free(merged) };

    let duplicated = unsafe { dr_doc_duplicating_layer(doc, 1) };
    assert!(!duplicated.is_null());
    unsafe { dr_doc_free(duplicated) };

    let mut err = ptr::null_mut();
    let promoted =
        unsafe { dr_doc_promote_flattened(doc, REFERENCE_WORKING_BYTES, 0, ptr::null(), &mut err) };
    assert!(!promoted.is_null(), "{:?}", unsafe { text(err) });
    unsafe { dr_precision_doc_free(promoted) };

    // The structure promotion already blocks the layer by name.
    let mut err = ptr::null_mut();
    let plan = unsafe { text(dr_doc_promote_structure_plan(doc, &mut err)) }
        .unwrap_or_else(|| panic!("{:?}", unsafe { text(err) }));
    let plan: Value = serde_json::from_str(&plan).expect("plan JSON");
    assert_eq!(plan["blocked"][0]["index"], 1, "{plan}");
    assert!(
        plan["blocked"][0]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("Synthetic Bad Gamma")),
        "{plan}"
    );

    // The document still refuses a NEW or EDITED calibrated description.
    let exposure = CString::new("exposure").expect("op");
    assert_eq!(
        unsafe { text(dr_doc_calibrated_refusal(doc, exposure.as_ptr())) }.as_deref(),
        Some(OWN_PROFILE_SENTENCE)
    );
    unsafe { dr_doc_free(doc) };
}

/// The foreground estimate's outcome on `doc` for `params`: `Ok(true)` for a
/// result (freed here), `Ok(false)` for NULL without an error, else the error.
fn foreground(doc: *const DrDocument, params: &str) -> Result<bool, String> {
    let pixels = [200u8, 40, 90, 255, 10, 220, 130, 255];
    let source = unsafe { dr_image_from_rgba(pixels.as_ptr(), 2, 1) };
    assert!(!source.is_null());
    let alpha = [64u8, 191];
    let json = CString::new(params).expect("params");
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_image_estimate_foreground(
            doc,
            source,
            alpha.as_ptr(),
            json.as_ptr(),
            ptr::null(),
            &mut err,
        )
    };
    unsafe { dr_image_free(source) };
    let error = unsafe { text(err) };
    let produced = !out.is_null();
    if produced {
        unsafe { dr_image_free(out) };
    }
    match error {
        Some(error) => {
            assert!(!produced, "an error with a result");
            Err(error)
        }
        None => Ok(produced),
    }
}

#[test]
fn foreground_estimation_in_linear_names_a_failing_own_profile() {
    let bad = byte_document(Some(&bad_profile()));
    let linear = r#"{"color_space":"linear","strength":1}"#;
    assert_eq!(
        foreground(bad, linear),
        Err(OWN_PROFILE_SENTENCE.to_string()),
        "linear encodes into the own profile"
    );
    assert_eq!(
        foreground(bad, r#"{"color_space":"linear","strength":0}"#),
        Ok(false),
        "strength 0 is a no-op, not a refusal"
    );
    assert!(
        foreground(bad, r#"{"color_space":"encoded","strength":1}"#).is_ok(),
        "the encoded domain has no encoder"
    );
    unsafe { dr_doc_free(bad) };
    let plain = byte_document(None);
    assert!(foreground(plain, linear).is_ok(), "a passing profile");
    unsafe { dr_doc_free(plain) };
}

/// Don't Convert of a layers paste RELABELS the clip, so the
/// Assign check of the copied layers against the destination's profile runs
/// first and refuses in its own words, at 8 and 16 bits.
#[test]
fn pasting_layers_unconverted_into_a_failing_profile_refuses_in_assign_words() {
    let bad = bad_profile();
    let artwork = CString::new(r#"{"type":"text","version":1}"#).expect("description");

    let plain = byte_document(None);
    let clip = unsafe { dr_doc_with_layer_meta(plain, 0, artwork.as_ptr()) };
    assert!(!clip.is_null());
    unsafe { dr_doc_free(plain) };
    let dest = byte_document(Some(&bad));
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_paste_layers(dest, clip, 0, 0, 0, 0, true, &mut err) };
    assert_eq!(
        byte_outcome(out, err),
        Err(ARTWORK_ASSIGN_SENTENCE.to_string())
    );
    unsafe { dr_doc_free(clip) };
    unsafe { dr_doc_free(dest) };

    let plain = typed_document(None);
    let mut err = ptr::null_mut();
    let clip = unsafe {
        dr_precision_doc_artwork_metadata(
            plain,
            0,
            artwork.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            &mut err,
        )
    };
    assert!(!clip.is_null(), "{:?}", unsafe { text(err) });
    unsafe { dr_precision_doc_free(plain) };
    let dest = typed_document(Some(&bad));
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_paste_layers(
            dest,
            clip,
            0,
            0,
            0,
            0,
            true,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null_mut(),
            &mut err,
        )
    };
    assert_eq!(
        typed_outcome(out, err),
        Err(ARTWORK_ASSIGN_SENTENCE.to_string())
    );
    unsafe { dr_precision_doc_free(clip) };
    unsafe { dr_precision_doc_free(dest) };
}
