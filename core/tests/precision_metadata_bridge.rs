//! The typed host-decoder metadata bridge (Phase 11D): capture packets and
//! the original PPI reconstructed INTO a typed (16-bit) document.
//!
//! The claim under test is an equality with the byte path, not a new rule:
//! the same carrier handed to `dr_doc_import_metadata` and to
//! `dr_precision_doc_import_metadata` leaves the same three packets and the
//! same warnings. Then the two things that make the bridge worth having:
//! a typed export WRITES those packets, and a native save/reopen keeps them.
//!
//! The carrier is a TIFF this test builds with the crate's own document
//! writer from packets assembled by `common::metadata_fixtures`, so nothing
//! is read from the repository.
use crate::common;
use common::{cpath, metadata_fixtures, take_err_string};

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_string_free};
use doctored_core::ffi_color::{
    dr_doc_metadata, dr_doc_metadata_len, dr_doc_save_image, dr_doc_set_metadata,
    dr_doc_set_resolution,
};
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_from_image};
use doctored_core::ffi_metadata_import::dr_doc_import_metadata;
use doctored_core::ffi_precision::{
    dr_precision_doc_export16, dr_precision_doc_free, dr_precision_doc_info, dr_precision_doc_open,
    dr_precision_doc_save_native,
};
use doctored_core::ffi_precision_input::dr_precision_doc_from_rgba16;
use doctored_core::ffi_precision_inspect::dr_precision_doc_metadata;
use doctored_core::ffi_precision_metadata::{
    dr_precision_doc_import_metadata, dr_precision_doc_set_metadata,
    dr_precision_doc_set_resolution,
};
use doctored_core::precision_document::PrecisionDocument;

use std::ffi::{c_char, CStr};
use std::ptr;
use tempfile::TempDir;

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const FORMAT_TIFF: i32 = 2;
const W: u32 = 8;
const H: u32 = 6;

const XMP: &[u8] = b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description tiff:XResolution=\"300/1\"/></rdf:RDF></x:xmpmeta>";

fn plate() -> Vec<u8> {
    (0..W * H)
        .flat_map(|i| [(i * 3) as u8, (i * 5) as u8, (i * 7) as u8, 255])
        .collect()
}

fn byte_doc() -> *mut DrDocument {
    let pixels = plate();
    let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), W, H) };
    assert!(!image.is_null());
    let doc = unsafe { dr_doc_from_image(image) };
    unsafe { dr_image_free(image) };
    assert!(!doc.is_null());
    doc
}

fn typed_doc() -> *mut PrecisionDocument {
    let samples: Vec<u16> = plate().iter().map(|v| u16::from(*v) * 257).collect();
    let len = doctored_core::ffi_color::dr_builtin_profile_len(0);
    let mut profile = vec![0u8; len];
    assert!(unsafe { doctored_core::ffi_color::dr_builtin_profile(0, profile.as_mut_ptr(), len) });
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

/// A TIFF carrying EXIF, XMP and IPTC — written by the crate's own document
/// saver so the carrier is a real file rather than a hand-cut buffer.
fn carrier(dir: &TempDir) -> Vec<u8> {
    let exif = metadata_fixtures::build_exif(false, 1, (300, 300), Some((W, H)), false);
    let iptc = metadata_fixtures::iptc_run();
    let mut doc = byte_doc();
    for (kind, bytes) in [(0, exif.bytes.as_slice()), (1, XMP), (2, iptc.as_slice())] {
        let next =
            unsafe { dr_doc_set_metadata(doc, kind, bytes.as_ptr(), bytes.len(), ptr::null_mut()) };
        assert!(!next.is_null(), "packet {kind}");
        unsafe { dr_doc_free(doc) };
        doc = next;
    }
    let path = dir.path().join("carrier.tiff");
    let c = cpath(&path);
    let mut carried: u32 = 0;
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe {
            dr_doc_save_image(
                doc,
                ptr::null(),
                c.as_ptr(),
                FORMAT_TIFF,
                90,
                true,
                false,
                &mut carried,
                &mut err,
            )
        },
        "{}",
        take_err_string(err)
    );
    unsafe { dr_doc_free(doc) };
    std::fs::read(&path).expect("carrier")
}

fn byte_packets(doc: *const DrDocument) -> [Option<Vec<u8>>; 3] {
    std::array::from_fn(|kind| {
        let len = unsafe { dr_doc_metadata_len(doc, kind as i32) };
        if len == 0 {
            return None;
        }
        let mut bytes = vec![0u8; len];
        assert!(unsafe { dr_doc_metadata(doc, kind as i32, bytes.as_mut_ptr(), len) });
        Some(bytes)
    })
}

fn typed_packets(document: *const PrecisionDocument) -> [Option<Vec<u8>>; 3] {
    std::array::from_fn(|kind| {
        let mut len = 0usize;
        let pointer = unsafe { dr_precision_doc_metadata(document, kind as u32, &mut len) };
        if pointer.is_null() {
            return None;
        }
        Some(unsafe { std::slice::from_raw_parts(pointer, len) }.to_vec())
    })
}

fn typed_info(document: *const PrecisionDocument) -> serde_json::Value {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_precision_doc_info(document, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let text = unsafe { CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(json) };
    serde_json::from_str(&text).expect("info json")
}

fn import_typed(
    document: *const PrecisionDocument,
    bytes: &[u8],
) -> (Option<*mut PrecisionDocument>, String, Option<String>) {
    let mut warnings: *mut c_char = ptr::null_mut();
    let mut err: *mut c_char = ptr::null_mut();
    let next = unsafe {
        dr_precision_doc_import_metadata(
            document,
            bytes.as_ptr(),
            bytes.len(),
            &mut warnings,
            &mut err,
        )
    };
    let warning_text = if warnings.is_null() {
        String::new()
    } else {
        let text = unsafe { CStr::from_ptr(warnings) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(warnings) };
        text
    };
    if next.is_null() {
        let error = if err.is_null() {
            None
        } else {
            Some(take_err_string(err))
        };
        return (None, warning_text, error);
    }
    (Some(next), warning_text, None)
}

// ------------------------------------------------------------------ tests --

#[test]
fn the_typed_import_lands_the_byte_imports_packets_and_warnings() {
    let dir = TempDir::new().expect("tmp");
    let bytes = carrier(&dir);

    // The byte twin.
    let plain = byte_doc();
    let mut byte_warnings: *mut c_char = ptr::null_mut();
    let mut err: *mut c_char = ptr::null_mut();
    let imported = unsafe {
        dr_doc_import_metadata(
            plain,
            bytes.as_ptr(),
            bytes.len(),
            &mut byte_warnings,
            &mut err,
        )
    };
    assert!(!imported.is_null(), "{}", take_err_string(err));
    let byte_warning_text = unsafe { CStr::from_ptr(byte_warnings) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(byte_warnings) };
    let expected = byte_packets(imported);
    assert!(expected[0].is_some(), "the carrier carries EXIF");
    assert!(expected[1].is_some(), "the carrier carries XMP");
    assert!(expected[2].is_some(), "the carrier carries IPTC");

    // The typed one.
    let document = typed_doc();
    let (next, warnings, error) = import_typed(document, &bytes);
    assert!(error.is_none(), "{error:?}");
    let next = next.expect("the typed import produces a document");
    assert_eq!(typed_packets(next), expected, "the same three packets");
    assert_eq!(warnings, byte_warning_text, "the same omission report");

    // Purity: the original typed document is untouched.
    assert_eq!(typed_packets(document), [None, None, None]);

    // And the same carrier a second time is a no-op.
    let (again, warnings_again, error) = import_typed(next, &bytes);
    assert!(error.is_none(), "{error:?}");
    assert!(again.is_none(), "an unchanged import returns NULL");
    assert_eq!(warnings_again, byte_warning_text);

    unsafe { dr_precision_doc_free(next) };
    unsafe { dr_precision_doc_free(document) };
    unsafe { dr_doc_free(imported) };
    unsafe { dr_doc_free(plain) };
}

#[test]
fn the_typed_import_refuses_what_the_byte_one_refuses() {
    let document = typed_doc();
    let (next, _, error) = import_typed(document, b"not a container at all");
    assert!(next.is_none());
    assert_eq!(error.unwrap(), "metadata carrier must be JPEG, PNG or TIFF");

    // A carrier past the bounded import limit, refused before it is scanned.
    let huge = vec![0u8; 50 * 1024 * 1024];
    let (next, _, error) = import_typed(document, &huge);
    assert!(next.is_none());
    assert_eq!(
        error.unwrap(),
        "metadata carrier exceeds the bounded packet import limit"
    );
    unsafe { dr_precision_doc_free(document) };
}

#[test]
fn set_metadata_stores_clears_and_refuses_the_same_things_as_the_byte_twin() {
    let document = typed_doc();
    let with_xmp = unsafe {
        dr_precision_doc_set_metadata(document, 1, XMP.as_ptr(), XMP.len(), ptr::null_mut())
    };
    assert!(!with_xmp.is_null());
    assert_eq!(typed_packets(with_xmp)[1].as_deref(), Some(XMP));

    // The same value again is no change, and no change is not an error.
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe {
        dr_precision_doc_set_metadata(with_xmp, 1, XMP.as_ptr(), XMP.len(), &mut err)
    }
    .is_null());
    assert!(err.is_null(), "a no-op sets no error");
    // An empty packet is distinct from an absent one.
    let empty =
        unsafe { dr_precision_doc_set_metadata(with_xmp, 1, [].as_ptr(), 0, ptr::null_mut()) };
    assert!(!empty.is_null());
    assert_eq!(typed_packets(empty)[1].as_deref(), Some(&[][..]));
    // Clearing.
    let cleared =
        unsafe { dr_precision_doc_set_metadata(with_xmp, 1, ptr::null(), 0, ptr::null_mut()) };
    assert!(!cleared.is_null());
    assert_eq!(typed_packets(cleared)[1], None);
    // An unknown kind, and a payload past the 16 MiB blob cap, are named
    // refusals.
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe {
        dr_precision_doc_set_metadata(document, 9, XMP.as_ptr(), XMP.len(), &mut err)
    }
    .is_null());
    assert_eq!(take_err_string(err), "unknown metadata kind 9");
    let huge = vec![0u8; 16 * 1024 * 1024 + 1];
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe {
        dr_precision_doc_set_metadata(document, 0, huge.as_ptr(), huge.len(), &mut err)
    }
    .is_null());
    assert_eq!(
        take_err_string(err),
        "metadata packet exceeds the 16 MiB limit"
    );

    // The byte twin answers the identical sentences, and its no-op sets no
    // error either.
    let byte = byte_doc();
    let byte_xmp =
        unsafe { dr_doc_set_metadata(byte, 1, XMP.as_ptr(), XMP.len(), ptr::null_mut()) };
    assert!(!byte_xmp.is_null());
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe { dr_doc_set_metadata(byte_xmp, 1, XMP.as_ptr(), XMP.len(), &mut err) }.is_null()
    );
    assert!(err.is_null(), "the byte no-op sets no error");
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_doc_set_metadata(byte, 9, XMP.as_ptr(), XMP.len(), &mut err) }.is_null());
    assert_eq!(take_err_string(err), "unknown metadata kind 9");
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_doc_set_metadata(byte, 0, huge.as_ptr(), huge.len(), &mut err) }.is_null());
    assert_eq!(
        take_err_string(err),
        "metadata packet exceeds the 16 MiB limit"
    );
    unsafe { dr_doc_free(byte_xmp) };
    unsafe { dr_doc_free(byte) };

    unsafe { dr_precision_doc_free(cleared) };
    unsafe { dr_precision_doc_free(empty) };
    unsafe { dr_precision_doc_free(with_xmp) };
    unsafe { dr_precision_doc_free(document) };
}

#[test]
fn set_resolution_sanitizes_exactly_as_the_byte_twin_does() {
    let document = typed_doc();
    let byte = byte_doc();
    for (x, y) in [(300.0f32, 300.0), (72.5, 96.25), (0.5, 40_000.0)] {
        let typed_next =
            unsafe { dr_precision_doc_set_resolution(document, x, y, ptr::null_mut()) };
        let byte_next = unsafe { dr_doc_set_resolution(byte, x, y, ptr::null_mut()) };
        assert_eq!(
            typed_next.is_null(),
            byte_next.is_null(),
            "the two paths agree about {x} x {y}"
        );
        if typed_next.is_null() {
            continue;
        }
        let info = typed_info(typed_next);
        let bx = unsafe { doctored_core::ffi_color::dr_doc_resolution_x(byte_next) };
        let by = unsafe { doctored_core::ffi_color::dr_doc_resolution_y(byte_next) };
        assert_eq!(info["resolution"]["x_ppi"].as_f64().unwrap() as f32, bx);
        assert_eq!(info["resolution"]["y_ppi"].as_f64().unwrap() as f32, by);
        unsafe { dr_precision_doc_free(typed_next) };
        unsafe { dr_doc_free(byte_next) };
    }
    // Non-finite and non-positive are named refusals.
    for (x, y) in [(f32::NAN, 300.0), (0.0, 300.0), (-1.0, 300.0)] {
        let mut err: *mut c_char = ptr::null_mut();
        assert!(
            unsafe { dr_precision_doc_set_resolution(document, x, y, &mut err) }.is_null(),
            "{x} x {y} is refused"
        );
        assert_eq!(
            take_err_string(err),
            "print resolution must be finite and positive"
        );
        // The byte twin answers the identical sentence.
        let mut err: *mut c_char = ptr::null_mut();
        assert!(
            unsafe { dr_doc_set_resolution(byte, x, y, &mut err) }.is_null(),
            "{x} x {y} is refused by the byte twin"
        );
        assert_eq!(
            take_err_string(err),
            "print resolution must be finite and positive"
        );
    }
    // The no-op answers NULL without an error, at both depths.
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_precision_doc_set_resolution(document, 72.0, 72.0, &mut err) }.is_null());
    assert!(err.is_null(), "a no-op sets no error");
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_doc_set_resolution(byte, 72.0, 72.0, &mut err) }.is_null());
    assert!(err.is_null(), "the byte no-op sets no error");
    unsafe { dr_doc_free(byte) };
    unsafe { dr_precision_doc_free(document) };
}

#[test]
fn reconstructed_packets_survive_a_native_round_trip_and_reach_an_export() {
    let dir = TempDir::new().expect("tmp");
    let bytes = carrier(&dir);
    let document = typed_doc();
    let (imported, _, error) = import_typed(document, &bytes);
    assert!(error.is_none(), "{error:?}");
    let imported = imported.expect("import");
    let with_ppi =
        unsafe { dr_precision_doc_set_resolution(imported, 300.0, 300.0, ptr::null_mut()) };
    assert!(!with_ppi.is_null());
    let expected = typed_packets(with_ppi);

    // Native save and reopen.
    let native = dir.path().join("bridge.rz");
    let c = cpath(&native);
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(with_ppi, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        take_err_string(err)
    );
    let mut err: *mut c_char = ptr::null_mut();
    let reopened = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!reopened.is_null(), "{}", take_err_string(err));
    assert_eq!(
        typed_packets(reopened),
        expected,
        "packets survive .doctored"
    );
    let info = typed_info(reopened);
    assert_eq!(info["resolution"]["x_ppi"], 300.0);

    // A typed export writes them.
    let exported = dir.path().join("bridge.tiff");
    let c = cpath(&exported);
    let mut report: u32 = 0;
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_export16(
                reopened,
                c.as_ptr(),
                1,
                true,
                false,
                LIMIT,
                0,
                &mut report,
                &mut err,
            )
        },
        "{}",
        take_err_string(err)
    );
    // profile | exif | xmp | iptc | resolution.
    assert_eq!(report, 1 | 2 | 4 | 8 | 16, "the export wrote every packet");

    // And the written file's packets read back as the document's.
    let written = std::fs::read(&exported).expect("exported");
    let plain = byte_doc();
    let mut warnings: *mut c_char = ptr::null_mut();
    let mut err: *mut c_char = ptr::null_mut();
    let round = unsafe {
        dr_doc_import_metadata(
            plain,
            written.as_ptr(),
            written.len(),
            &mut warnings,
            &mut err,
        )
    };
    assert!(!round.is_null(), "{}", take_err_string(err));
    unsafe { dr_string_free(warnings) };
    let back = byte_packets(round);
    assert!(back[0].is_some() && back[1].is_some() && back[2].is_some());

    unsafe { dr_doc_free(round) };
    unsafe { dr_doc_free(plain) };
    unsafe { dr_precision_doc_free(reopened) };
    unsafe { dr_precision_doc_free(with_ppi) };
    unsafe { dr_precision_doc_free(imported) };
    unsafe { dr_precision_doc_free(document) };
}

#[test]
fn the_three_typed_exports_tolerate_null() {
    let mut warnings: *mut c_char = ptr::null_mut();
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe {
        dr_precision_doc_import_metadata(ptr::null(), ptr::null(), 0, &mut warnings, &mut err)
            .is_null()
    });
    assert!(err.is_null(), "a NULL input is a refusal, not an error");
    assert!(warnings.is_null());
    assert!(unsafe {
        dr_precision_doc_set_metadata(ptr::null(), 0, ptr::null(), 0, &mut err).is_null()
    });
    assert!(err.is_null(), "a NULL document is a refusal, not an error");
    assert!(unsafe {
        dr_precision_doc_set_resolution(ptr::null(), 300.0, 300.0, &mut err).is_null()
    });
    assert!(err.is_null(), "a NULL document is a refusal, not an error");

    let document = typed_doc();
    assert!(unsafe {
        dr_precision_doc_import_metadata(document, ptr::null(), 0, ptr::null_mut(), ptr::null_mut())
            .is_null()
    });
    unsafe { dr_precision_doc_free(document) };
}
