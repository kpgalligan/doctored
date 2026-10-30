//! Host-encoded export formats (Phase 11D): HEIC.
//!
//! HEIC has no encoder in this crate, so what the core owes is exactly three
//! things and this file checks all three:
//!
//! * The bytes it hands the host are the `"tiff"` request's bytes at the same
//!   settings — an EQUALITY, so the host never transcodes something the TIFF
//!   path did not already write (including the capture packets, on a document
//!   carrying EXIF, XMP and IPTC).
//! * A pending result cannot reach disk or a preview: `write` and `preview`
//!   both refuse with their named sentences.
//! * Adoption replaces the bytes with the host's, reports the depth and the
//!   packets the host's file was MEASURED to carry, clears the intermediate,
//!   and only then does `write` succeed.
//!
//! Plus the flat-save refusal (`dr_doc_save_image` never writes HEIC) and the
//! NULL sweep for the new export.
use crate::common;
use common::{cpath, take_err_string};

use doctored_core::doc::DrDocument;
use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_string_free};
use doctored_core::ffi_color::{dr_doc_save_image, dr_doc_set_metadata};
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_from_image};
use doctored_core::ffi_export::{
    dr_doc_export_encode, dr_export_result_adopt_host_bytes, dr_export_result_bytes,
    dr_export_result_free, dr_export_result_info, dr_export_result_preview, dr_export_result_write,
};
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_render, dr_precision_image_free,
};
use doctored_core::ffi_precision_input::dr_precision_doc_from_rgba16;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use tempfile::TempDir;

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const FORMAT_HEIC: i32 = 6;
const W: u32 = 20;
const H: u32 = 14;

// The three inherited packets a TIFF can carry, so the equality test is over
// a document with something to lose. Each is the smallest thing the shared
// writer accepts in that slot.
const EXIF: &[u8] = b"II\x2a\x00\x08\x00\x00\x00\x00\x00\x00\x00\x00\x00";
const XMP: &[u8] = b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description/></rdf:RDF></x:xmpmeta>";
const IPTC: &[u8] = b"8BIM\x04\x04\x00\x00\x00\x00\x00\x08\x1c\x02\x78\x00\x03abc";

fn plate() -> Vec<u8> {
    (0..W * H)
        .flat_map(|i| {
            [
                ((i * 17 + 5) % 256) as u8,
                ((i * 53 + 9) % 256) as u8,
                ((i * 7 + 200) % 256) as u8,
                255,
            ]
        })
        .collect()
}

fn samples16() -> Vec<u16> {
    plate().iter().map(|v| u16::from(*v) * 257).collect()
}

fn byte_doc(packets: bool) -> *mut DrDocument {
    let pixels = plate();
    let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), W, H) };
    assert!(!image.is_null());
    let mut doc = unsafe { dr_doc_from_image(image) };
    unsafe { dr_image_free(image) };
    assert!(!doc.is_null());
    if packets {
        for (kind, bytes) in [(0, EXIF), (1, XMP), (2, IPTC)] {
            let next = unsafe {
                dr_doc_set_metadata(doc, kind, bytes.as_ptr(), bytes.len(), ptr::null_mut())
            };
            assert!(!next.is_null(), "packet {kind} should attach");
            unsafe { dr_doc_free(doc) };
            doc = next;
        }
    }
    doc
}

fn typed_doc() -> *mut PrecisionDocument {
    let samples = samples16();
    let profile = {
        let len = doctored_core::ffi_color::dr_builtin_profile_len(0);
        let mut bytes = vec![0u8; len];
        assert!(unsafe {
            doctored_core::ffi_color::dr_builtin_profile(0, bytes.as_mut_ptr(), len)
        });
        bytes
    };
    let mut err: *mut c_char = ptr::null_mut();
    let document = unsafe {
        dr_precision_doc_from_rgba16(
            samples.as_ptr(),
            samples.len(),
            W,
            H,
            profile.as_ptr(),
            profile.len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!document.is_null(), "{}", take_err_string(err));
    document
}

fn encode_byte(doc: *const DrDocument, request: &str) -> Result<*mut ExportResult, String> {
    let json = CString::new(request).expect("request");
    let mut err: *mut c_char = ptr::null_mut();
    let result =
        unsafe { dr_doc_export_encode(doc, ptr::null(), json.as_ptr(), ptr::null(), &mut err) };
    if result.is_null() {
        return Err(take_err_string(err));
    }
    Ok(result)
}

fn encode_typed(
    document: *const PrecisionDocument,
    projection: *const PrecisionImage,
    request: &str,
) -> Result<*mut ExportResult, String> {
    let json = CString::new(request).expect("request");
    let mut err: *mut c_char = ptr::null_mut();
    let result = unsafe {
        doctored_core::ffi_export::dr_precision_doc_export_encode(
            document,
            projection,
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
    let text = unsafe { CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(json) };
    serde_json::from_str(&text).expect("info json")
}

fn write_result(result: *const ExportResult, path: &std::path::Path) -> Result<(), String> {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    if unsafe { dr_export_result_write(result, c.as_ptr(), false, &mut err) } {
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

fn adopt(
    result: *const ExportResult,
    bytes: &[u8],
    bit_depth: u32,
    carried: u32,
) -> Result<*mut ExportResult, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let next = unsafe {
        dr_export_result_adopt_host_bytes(
            result,
            bytes.as_ptr(),
            bytes.len(),
            bit_depth,
            carried,
            &mut err,
        )
    };
    if next.is_null() {
        return Err(take_err_string(err));
    }
    Ok(next)
}

// --------------------------------------------------- the intermediate --

#[test]
fn heic_hands_the_host_exactly_the_tiff_requests_bytes() {
    for packets in [false, true] {
        let doc = byte_doc(packets);
        for settings in [
            r#""size":{"mode":"long_edge","value":12}"#,
            r#""profile":"srgb","intent":"perceptual""#,
            r#""sharpen":{"radius":1.0,"amount":0.8,"threshold":0.02}"#,
            r#""strip_metadata":true"#,
        ] {
            let heic = encode_byte(doc, &format!(r#"{{"format":"heic",{settings}}}"#))
                .expect("heic request");
            let tiff = encode_byte(doc, &format!(r#"{{"format":"tiff",{settings}}}"#))
                .expect("tiff request");
            assert_eq!(
                result_bytes(heic),
                result_bytes(tiff),
                "the HEIC intermediate is the TIFF request's bytes ({settings}, packets {packets})"
            );
            let info = result_info(heic);
            assert_eq!(info["format"], "heic");
            assert_eq!(info["host_encoded"], true);
            assert_eq!(info["intermediate"], "tiff");
            assert_eq!(result_info(tiff)["host_encoded"], false);
            assert!(result_info(tiff)["intermediate"].is_null());
            unsafe { dr_export_result_free(heic) };
            unsafe { dr_export_result_free(tiff) };
        }
        unsafe { dr_doc_free(doc) };
    }
}

#[test]
fn a_typed_heic_request_is_admitted_at_both_depths_and_matches_tiff() {
    let document = typed_doc();
    let mut err: *mut c_char = ptr::null_mut();
    let projection = unsafe { dr_precision_doc_render(document, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!projection.is_null(), "{}", take_err_string(err));
    for depth in [8, 16] {
        let heic = encode_typed(
            document,
            projection,
            &format!(r#"{{"format":"heic","bit_depth":{depth}}}"#),
        )
        .expect("typed heic");
        let tiff = encode_typed(
            document,
            projection,
            &format!(r#"{{"format":"tiff","bit_depth":{depth}}}"#),
        )
        .expect("typed tiff");
        assert_eq!(result_bytes(heic), result_bytes(tiff), "at {depth} bits");
        let info = result_info(heic);
        assert_eq!(info["bit_depth"], depth);
        assert_eq!(info["intermediate"], "tiff");
        unsafe { dr_export_result_free(heic) };
        unsafe { dr_export_result_free(tiff) };
    }
    unsafe { dr_precision_image_free(projection) };
    unsafe { dr_precision_doc_free(document) };
}

// ------------------------------------------------- pending refusals --

#[test]
fn a_pending_host_result_refuses_to_be_written_or_previewed() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("pending.heic");
    let doc = byte_doc(false);
    let pending = encode_byte(doc, r#"{"format":"heic"}"#).expect("heic");
    assert_eq!(
        write_result(pending, &path).unwrap_err(),
        "a host-encoded export must adopt the host encoder's bytes before it is written"
    );
    assert!(!path.exists(), "nothing may reach disk while pending");

    let mut err: *mut c_char = ptr::null_mut();
    let preview = unsafe { dr_export_result_preview(pending, &mut err) };
    assert!(preview.is_null());
    assert_eq!(
        take_err_string(err),
        "a host-encoded export is previewed by the host"
    );
    unsafe { dr_export_result_free(pending) };
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------------- adoption --

#[test]
fn adoption_reports_what_the_host_wrote_and_then_writes() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("adopted.heic");
    let doc = byte_doc(true);
    let pending = encode_byte(doc, r#"{"format":"heic"}"#).expect("heic");
    // A stand-in for the host's bytes: this crate has no HEIC encoder, so
    // what is under test is the bookkeeping, not the container.
    let host_bytes: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
    // profile | exif | xmp | resolution — what a HEIC carries; IPTC is what
    // the host drops, and the report must say so.
    let carried = 1 | 2 | 4 | 16;
    let adopted = adopt(pending, &host_bytes, 10, carried).expect("adopt");

    let info = result_info(adopted);
    assert_eq!(info["format"], "heic");
    assert_eq!(info["host_encoded"], true);
    assert!(info["intermediate"].is_null());
    assert_eq!(info["bit_depth"], 10);
    assert_eq!(info["carried"], carried);
    assert_eq!(info["bytes"], host_bytes.len());
    assert_eq!(info["metadata"]["iptc"], false, "the host dropped IPTC");
    assert_eq!(info["metadata"]["exif"], true);
    assert_eq!(info["metadata"]["xmp"], true);
    assert_eq!(info["metadata"]["profile"], true);
    assert_eq!(info["metadata"]["density"], true);
    assert_eq!(result_bytes(adopted), host_bytes);

    write_result(adopted, &path).expect("adopted results write");
    assert_eq!(std::fs::read(&path).expect("written"), host_bytes);

    // The pending result is untouched: adoption is pure.
    assert_eq!(result_info(pending)["intermediate"], "tiff");

    // Adopting twice is refused.
    assert_eq!(
        adopt(adopted, &host_bytes, 8, 1).unwrap_err(),
        "this export is not a host-encoded export awaiting the host's bytes"
    );
    unsafe { dr_export_result_free(adopted) };
    unsafe { dr_export_result_free(pending) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn adoption_is_refused_on_an_ordinary_export() {
    let doc = byte_doc(false);
    let png = encode_byte(doc, r#"{"format":"png"}"#).expect("png");
    assert_eq!(
        adopt(png, &[1, 2, 3], 8, 1).unwrap_err(),
        "this export is not a host-encoded export awaiting the host's bytes"
    );
    unsafe { dr_export_result_free(png) };
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------------ flat saves --

#[test]
fn a_flat_save_never_writes_heic() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("flat.heic");
    let doc = byte_doc(false);
    let c = cpath(&path);
    let mut carried: u32 = 0;
    let mut err: *mut c_char = ptr::null_mut();
    let saved = unsafe {
        dr_doc_save_image(
            doc,
            ptr::null(),
            c.as_ptr(),
            FORMAT_HEIC,
            90,
            false,
            true,
            &mut carried,
            &mut err,
        )
    };
    assert!(!saved);
    assert_eq!(
        take_err_string(err),
        "HEIC is encoded by the host through the export workspace (format \"heic\")"
    );
    assert!(!path.exists());
    unsafe { dr_doc_free(doc) };
}

// ----------------------------------------------------- the NULL sweep --

#[test]
fn adopt_host_bytes_tolerates_null() {
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe {
        dr_export_result_adopt_host_bytes(ptr::null(), ptr::null(), 0, 8, 0, &mut err).is_null()
    });
    assert!(!err.is_null());
    unsafe { dr_string_free(err) };

    let doc = byte_doc(false);
    let pending = encode_byte(doc, r#"{"format":"heic"}"#).expect("heic");
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe {
        dr_export_result_adopt_host_bytes(pending, ptr::null(), 16, 8, 0, &mut err).is_null()
    });
    assert_eq!(
        take_err_string(err),
        "host-encoded export bytes are required"
    );
    unsafe { dr_export_result_free(pending) };
    unsafe { dr_doc_free(doc) };
}
