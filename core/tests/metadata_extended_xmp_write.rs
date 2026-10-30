//! Writing JPEG ExtendedXMP: an XMP packet too large for one
//! APP1 is split on JPEG export into a standard packet of at most 65 504
//! bytes, carrying `xmpNote:HasExtendedXMP`, plus ExtendedXMP chunks — where
//! it used to be dropped — and every flat export leaves out Google's
//! pixel-bound `GImage:`/`GDepth:` properties while the `.rz` keeps them.
//!
//! Exercised through the public C FFI (`dr_doc_save_image`,
//! `dr_doc_export_encode`, `dr_precision_doc_export_encode`,
//! `dr_doc_flat_save_losses`, `dr_doc_save_native`). Every exported file is
//! walked with the test-side walker in `common::extended_xmp_fixture`, never
//! the core's; the GUID oracle is the MD5 seam that
//! `metadata_extended_xmp_read.rs` checks against RFC 1321 and the system's
//! `md5`. All fixtures are synthetic.

use std::ffi::{c_char, c_int, CStr, CString};
use std::path::Path;
use std::process::Command;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_color::{dr_builtin_profile, dr_builtin_profile_len};
use doctored_core::ffi_doc::*;
use doctored_core::ffi_export::*;
use doctored_core::ffi_metadata_report::dr_path_metadata_import_report;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_input::dr_precision_doc_from_rgba16;
use doctored_core::ffi_precision_metadata::dr_precision_doc_set_metadata;
use doctored_core::ffi_preservation::dr_doc_flat_save_losses;
use tempfile::TempDir;

use crate::common;
use common::extended_xmp_fixture::*;
use common::metadata_fixtures::{open_file, packet, save, set_packet};
use common::*;

const FORMAT_PNG: c_int = 0;
const FORMAT_JPEG: c_int = 1;
const FORMAT_TIFF: c_int = 2;
const METADATA_XMP: c_int = 1;
const CARRIES_XMP: u32 = 4;
const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

// ------------------------------------------------------------- helpers --

fn doc_with_xmp(dir: &TempDir, xmp: &[u8]) -> *mut DrDocument {
    let doc = doc_from(dir, "base.png", &opaque_pattern(6, 4));
    set_packet(doc, METADATA_XMP, xmp)
}

fn reopened_xmp(dir: &TempDir, name: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    let doc = open_file(dir, name, bytes);
    let xmp = packet(doc, METADATA_XMP);
    unsafe { dr_doc_free(doc) };
    xmp
}

fn import_warnings(path: &Path) -> Vec<String> {
    let c = cpath(path);
    unsafe {
        let mut err: *mut c_char = ptr::null_mut();
        let json = dr_path_metadata_import_report(c.as_ptr(), &mut err);
        assert!(!json.is_null(), "{}", take_err_string(err));
        let value: serde_json::Value =
            serde_json::from_str(CStr::from_ptr(json).to_str().unwrap()).unwrap();
        dr_string_free(json);
        value["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w.as_str().unwrap().to_string())
            .collect()
    }
}

fn flat_save_losses(doc: *const DrDocument, format: c_int) -> Vec<String> {
    unsafe {
        let mut err: *mut c_char = ptr::null_mut();
        let json = dr_doc_flat_save_losses(doc, format, true, false, &mut err);
        assert!(!json.is_null(), "{}", take_err_string(err));
        let result = serde_json::from_str(CStr::from_ptr(json).to_str().unwrap()).unwrap();
        dr_string_free(json);
        result
    }
}

fn result_bytes(result: *mut ExportResult) -> Vec<u8> {
    let mut length = 0usize;
    let pointer = unsafe { dr_export_result_bytes(result, &mut length) };
    assert!(!pointer.is_null());
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec();
    unsafe { dr_export_result_free(result) };
    bytes
}

fn export_byte(doc: *const DrDocument, request: &str) -> Vec<u8> {
    let json = CString::new(request).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result =
        unsafe { dr_doc_export_encode(doc, ptr::null(), json.as_ptr(), ptr::null(), &mut err) };
    assert!(!result.is_null(), "{}", take_err_string(err));
    result_bytes(result)
}

/// The note's value in a standard packet.
fn note_value(standard: &[u8]) -> Vec<u8> {
    let key = b"HasExtendedXMP=\"";
    let at = standard
        .windows(key.len())
        .position(|w| w == key)
        .expect("a HasExtendedXMP attribute")
        + key.len();
    standard[at..at + 32].to_vec()
}

/// Asserts `jpeg`'s XMP is a valid split — one standard APP1 within the
/// limit, first, then consistent chunks by ascending offset, the note naming
/// the GUID that is the MD5 of the reassembled extension, and no start tag
/// repeating an attribute — and returns (standard, extension).
fn assert_split(jpeg: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let found = xmp_in(jpeg);
    let standard = found.standard().to_vec();
    assert!(standard.len() <= STANDARD_LIMIT, "{}", standard.len());
    assert!(!found.chunks.is_empty(), "the packet was split");
    for pair in found.indices.windows(2) {
        assert_eq!(pair[1], pair[0] + 1, "the XMP segments are consecutive");
    }
    let standard_index = found.indices[0];
    assert_eq!(
        jpeg_segment_kind(jpeg, standard_index),
        "standard",
        "the standard packet comes first"
    );
    for pair in found.chunks.windows(2) {
        assert!(
            pair[0].offset < pair[1].offset,
            "chunks by ascending offset"
        );
    }
    let extension = reassembled(&found.chunks);
    let guid = guid_of(&extension);
    assert_eq!(found.chunks[0].guid, guid, "GUID = MD5 of the extension");
    assert_eq!(count(&standard, b"HasExtendedXMP"), 1, "exactly one note");
    assert_eq!(note_value(&standard), guid);
    assert_eq!(repeated_attribute(&standard), None, "standard");
    assert_eq!(repeated_attribute(&extension), None, "extension");
    (standard, extension)
}

fn jpeg_segment_kind(jpeg: &[u8], index: usize) -> &'static str {
    let segments = common::metadata_fixtures::jpeg_segments(jpeg);
    let payload = &segments[index].1;
    if payload.starts_with(XMP_ID) {
        "standard"
    } else if payload.starts_with(EXT_ID) {
        "extension"
    } else {
        "other"
    }
}

fn typed_doc() -> *mut doctored_core::precision_document::PrecisionDocument {
    let len = dr_builtin_profile_len(0);
    let mut profile = vec![0u8; len];
    assert!(unsafe { dr_builtin_profile(0, profile.as_mut_ptr(), len) });
    let samples: Vec<u16> = (0..6 * 4 * 4).map(|i| (i * 2311 % 65536) as u16).collect();
    let mut err: *mut c_char = ptr::null_mut();
    let document = unsafe {
        dr_precision_doc_from_rgba16(
            samples.as_ptr(),
            samples.len(),
            6,
            4,
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

// --------------------------------------------------------------- split --

#[test]
fn a_200_kb_packet_is_split_and_reopens_whole() {
    let dir = TempDir::new().unwrap();
    let r = realistic(80_000, 60_000, 40_000);
    assert!(r.bytes.len() > 180_000);
    let doc = doc_with_xmp(&dir, &r.bytes);

    let losses = flat_save_losses(doc, FORMAT_JPEG);
    assert!(
        !losses.iter().any(|l| l == "XMP metadata"),
        "the preflight no longer reports the XMP lost: {losses:?}"
    );

    let path = dir.path().join("split.jpg");
    let (jpeg, carried) = save(doc, &path, FORMAT_JPEG, true, false);
    assert!(carried & CARRIES_XMP != 0, "the XMP is carried");
    let (standard, extension) = assert_split(&jpeg);
    // The small properties stay where a plain reader looks; the large ones
    // moved, photoshop:History among them.
    for kept in ["image/jpeg", "xmp:Rating=\"4\"", "Small title"] {
        assert!(
            contains(&standard, kept.as_bytes()),
            "{kept} stays standard"
        );
    }
    assert!(
        !contains(&standard, r.markers[3].as_bytes()),
        "History moved"
    );
    assert!(contains(&extension, b"<photoshop:History>"));
    assert_eq!(count(&standard, b"xmlns:xmpNote="), 1, "declared once");

    let reopened = reopened_xmp(&dir, "split-reopen.jpg", &jpeg).unwrap();
    for marker in &r.markers {
        assert!(
            contains(&reopened, marker.as_bytes()),
            "{marker:.40} survives"
        );
    }
    assert!(!contains(&reopened, b"HasExtendedXMP"));
    assert!(import_warnings(&dir.path().join("split-reopen.jpg")).is_empty());

    // exiftool, when installed, reads the extension as Adobe's tools do.
    if Command::new("exiftool").arg("-ver").output().is_ok() {
        let out = Command::new("exiftool")
            .args(["-b", "-XMP-photoshop:History"])
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), r.markers[3]);
    } else {
        println!("skipping the exiftool cross-check: exiftool is not installed");
    }
    unsafe { dr_doc_free(doc) };
}

#[test]
fn sixty_four_exports_cover_every_extension_length_residue() {
    let dir = TempDir::new().unwrap();
    let mut residues = std::collections::BTreeSet::new();
    for k in 0..64 {
        let r = realistic(80_000 + k, 20_000, 1_000);
        let doc = doc_with_xmp(&dir, &r.bytes);
        let (jpeg, carried) = save(
            doc,
            &dir.path().join(format!("r{k}.jpg")),
            FORMAT_JPEG,
            true,
            false,
        );
        assert!(carried & CARRIES_XMP != 0);
        let (_, extension) = assert_split(&jpeg);
        residues.insert(extension.len() % 64);
        unsafe { dr_doc_free(doc) };
    }
    assert_eq!(
        residues.len(),
        64,
        "every MD5 padding residue was exercised"
    );
}

#[test]
fn a_stale_note_and_its_declaration_are_written_once() {
    let dir = TempDir::new().unwrap();
    let body = format!(
        "   <photoshop:History>{}</photoshop:History>",
        filler(40, 90_000)
    );
    let stale = xmp_packet(
        &format!("{NAMESPACES} xmlns:xmpNote=\"{NOTE_NS}\""),
        " xmpNote:HasExtendedXMP=\"00000000000000000000000000000000\"",
        &body,
    );
    let doc = doc_with_xmp(&dir, &stale);
    let (jpeg, _) = save(doc, &dir.path().join("stale.jpg"), FORMAT_JPEG, true, false);
    let (standard, _) = assert_split(&jpeg);
    assert_eq!(count(&standard, b"xmlns:xmpNote="), 1);
    assert!(!contains(&standard, b"00000000000000000000000000000000"));
    unsafe { dr_doc_free(doc) };

    // A prefix already bound to the note namespace is reused.
    let other = xmp_packet(&format!("{NAMESPACES} xmlns:n=\"{NOTE_NS}\""), "", &body);
    let doc = doc_with_xmp(&dir, &other);
    let (jpeg, _) = save(
        doc,
        &dir.path().join("prefix.jpg"),
        FORMAT_JPEG,
        true,
        false,
    );
    let (standard, _) = assert_split(&jpeg);
    assert!(contains(&standard, b" n:HasExtendedXMP=\""));
    assert!(!contains(&standard, b"xmlns:xmpNote"));
    unsafe { dr_doc_free(doc) };

    // A Description binding xmpNote to something else cannot take the
    // note: the split refuses and the XMP is dropped, as before.
    let rebound = xmp_packet(
        &format!("{NAMESPACES} xmlns:xmpNote=\"urn:not-the-note\""),
        "",
        &body,
    );
    let doc = doc_with_xmp(&dir, &rebound);
    let (jpeg, carried) = save(
        doc,
        &dir.path().join("rebound.jpg"),
        FORMAT_JPEG,
        true,
        false,
    );
    assert_eq!(carried & CARRIES_XMP, 0);
    assert!(xmp_in(&jpeg).standards.is_empty());
    unsafe { dr_doc_free(doc) };

    // The same when the other binding is inherited from `rdf:RDF`.
    let inherited = String::from_utf8(xmp_packet(NAMESPACES, "", &body))
        .unwrap()
        .replacen(
            "<rdf:RDF ",
            "<rdf:RDF xmlns:xmpNote=\"urn:not-the-note\" ",
            1,
        )
        .into_bytes();
    let doc = doc_with_xmp(&dir, &inherited);
    let (jpeg, carried) = save(
        doc,
        &dir.path().join("inherited.jpg"),
        FORMAT_JPEG,
        true,
        false,
    );
    assert_eq!(carried & CARRIES_XMP, 0);
    assert!(xmp_in(&jpeg).standards.is_empty());
    unsafe { dr_doc_free(doc) };
}

#[test]
fn padding_alone_over_the_limit_is_trimmed_into_one_segment() {
    let dir = TempDir::new().unwrap();
    let small = xmp_packet(NAMESPACES, " dc:format=\"image/jpeg\"", "");
    let trailer = b"<?xpacket end=\"w\"?>";
    let at = small.len() - trailer.len();
    let padded = [&small[..at], "\n".repeat(70_000).as_bytes(), &small[at..]].concat();
    assert!(padded.len() > STANDARD_LIMIT);
    let doc = doc_with_xmp(&dir, &padded);
    let (jpeg, carried) = save(doc, &dir.path().join("pad.jpg"), FORMAT_JPEG, true, false);
    assert!(carried & CARRIES_XMP != 0);
    let found = xmp_in(&jpeg);
    assert!(found.chunks.is_empty(), "no ExtendedXMP");
    assert!(!contains(found.standard(), b"HasExtendedXMP"));
    assert!(found.standard().ends_with(trailer));
    assert!(found.standard().len() < 1000);
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_stamp_lands_in_the_standard_packet() {
    let dir = TempDir::new().unwrap();
    let r = realistic(80_000, 60_000, 40_000);
    let doc = doc_with_xmp(&dir, &r.bytes);
    let jpeg = export_byte(
        doc,
        r#"{"format":"jpeg","metadata":{"stamp":{"author":"Ada Lovelace"}}}"#,
    );
    let (standard, _) = assert_split(&jpeg);
    assert!(contains(
        &standard,
        b"<dc:creator><rdf:Seq><rdf:li>Ada Lovelace"
    ));
    unsafe { dr_doc_free(doc) };
}

#[test]
fn both_export_encoders_split_an_oversize_packet() {
    let dir = TempDir::new().unwrap();
    let r = realistic(80_000, 60_000, 40_000);
    let doc = doc_with_xmp(&dir, &r.bytes);
    let jpeg = export_byte(doc, r#"{"format":"jpeg"}"#);
    assert_split(&jpeg);
    unsafe { dr_doc_free(doc) };

    let typed = typed_doc();
    let mut err: *mut c_char = ptr::null_mut();
    let with_xmp = unsafe {
        dr_precision_doc_set_metadata(
            typed,
            METADATA_XMP,
            r.bytes.as_ptr(),
            r.bytes.len(),
            &mut err,
        )
    };
    assert!(!with_xmp.is_null(), "{}", take_err_string(err));
    let json = CString::new(r#"{"format":"jpeg","bit_depth":8}"#).unwrap();
    let result = unsafe {
        dr_precision_doc_export_encode(
            with_xmp,
            ptr::null(),
            json.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!result.is_null(), "{}", take_err_string(err));
    let jpeg = result_bytes(result);
    let (_, extension) = assert_split(&jpeg);
    assert!(contains(&extension, r.markers[3].as_bytes()));
    unsafe {
        dr_precision_doc_free(with_xmp);
        dr_precision_doc_free(typed);
    }
}

// ---------------------------------------------------- open, then export --

/// A JPEG whose ~150 KB of XMP arrives as a standard packet and three
/// extension chunks, the extension's `rdf:RDF` declaring namespaces its
/// Descriptions rely on and one Description re-declaring one of them.
fn merged_source() -> (Vec<u8>, Vec<String>) {
    let big = filler(50, 70_000);
    let history = filler(51, 60_000);
    let extension = extension_packet(
        concat!(
            " xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"",
            " xmlns:photoshop=\"http://ns.adobe.com/photoshop/1.0/\""
        ),
        &format!(
            "<rdf:Description rdf:about=\"\" crs:Big=\"{big}\" tiff:Orientation=\"6\" \
             xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\"/>\
             <rdf:Description rdf:about=\"\" \
             xmlns:photoshop=\"http://ns.adobe.com/photoshop/1.0/\">\
             <photoshop:History>{history}</photoshop:History></rdf:Description>"
        ),
    );
    let guid = guid_of(&extension);
    let standard = xmp_packet(
        concat!(
            " xmlns:dc=\"http://purl.org/dc/elements/1.1/\"",
            " xmlns:xmpNote=\"http://ns.adobe.com/xmp/note/\""
        ),
        &format!(
            " dc:format=\"image/jpeg\" xmpNote:HasExtendedXMP=\"{}\"",
            String::from_utf8(guid.clone()).unwrap()
        ),
        "",
    );
    let segments = [
        vec![standard_segment(&standard)],
        extension_segments(&extension, &guid, 50_000),
    ]
    .concat();
    assert_eq!(segments.len(), 4);
    (jpeg_with(&segments), vec![big, history])
}

#[test]
fn a_merged_jpeg_reexports_without_repeating_an_attribute() {
    let dir = TempDir::new().unwrap();
    let (source, markers) = merged_source();
    let doc = open_file(&dir, "merged.jpg", &source);
    let logical = packet(doc, METADATA_XMP).unwrap();
    assert!(logical.len() > 130_000, "the open merged the extension");
    assert!(import_warnings(&dir.path().join("merged.jpg")).is_empty());

    let (jpeg, carried) = save(doc, &dir.path().join("again.jpg"), FORMAT_JPEG, true, false);
    assert!(carried & CARRIES_XMP != 0);
    assert_split(&jpeg);
    let reopened = reopened_xmp(&dir, "again-reopen.jpg", &jpeg).unwrap();
    for marker in &markers {
        assert!(contains(&reopened, marker.as_bytes()));
    }
    assert_eq!(repeated_attribute(&reopened), None);
    unsafe { dr_doc_free(doc) };
}

#[test]
fn export_reopen_export_writes_the_same_xmp() {
    let dir = TempDir::new().unwrap();
    for (name, doc) in [
        (
            "realistic",
            doc_with_xmp(&dir, &realistic(80_000, 60_000, 40_000).bytes),
        ),
        ("merged", open_file(&dir, "merged.jpg", &merged_source().0)),
    ] {
        let (first, _) = save(
            doc,
            &dir.path().join(format!("{name}-1.jpg")),
            FORMAT_JPEG,
            true,
            false,
        );
        let again = open_file(&dir, &format!("{name}-1-open.jpg"), &first);
        let (second, _) = save(
            again,
            &dir.path().join(format!("{name}-2.jpg")),
            FORMAT_JPEG,
            true,
            false,
        );
        let (a, b) = (xmp_in(&first), xmp_in(&second));
        assert_eq!(a.standard(), b.standard(), "{name}: standard packet stable");
        assert_eq!(
            reassembled(&a.chunks),
            reassembled(&b.chunks),
            "{name}: extension stable"
        );
        unsafe {
            dr_doc_free(again);
            dr_doc_free(doc)
        };
    }
}

#[test]
fn an_orientation_from_the_extension_is_reset_on_export() {
    let dir = TempDir::new().unwrap();
    let (source, _) = merged_source();
    let doc = open_file(&dir, "rotated.jpg", &source);
    assert!(contains(
        &packet(doc, METADATA_XMP).unwrap(),
        b"tiff:Orientation=\"6\""
    ));
    for (format, name) in [(FORMAT_JPEG, "o.jpg"), (FORMAT_PNG, "o.png")] {
        let (bytes, _) = save(doc, &dir.path().join(name), format, true, false);
        let xmp = reopened_xmp(&dir, &format!("re-{name}"), &bytes).unwrap();
        assert!(contains(&xmp, b"tiff:Orientation=\"1\""), "{name}");
        assert!(!contains(&xmp, b"tiff:Orientation=\"6\""), "{name}");
    }
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_failed_merge_exports_no_note_and_reopens_clean() {
    let dir = TempDir::new().unwrap();
    let (source, _) = merged_source();
    // Keep the standard packet and only the FIRST chunk.
    let found = xmp_in(&source);
    let truncated = jpeg_with(&[
        standard_segment(found.standard()),
        extension_segment(
            &found.chunks[0].guid,
            found.chunks[0].full_len,
            found.chunks[0].offset,
            &found.chunks[0].data,
        ),
    ]);
    let doc = open_file(&dir, "truncated.jpg", &truncated);
    assert_eq!(
        import_warnings(&dir.path().join("truncated.jpg")),
        vec!["Extended XMP is incomplete or inconsistent; only the standard XMP packet was read"]
    );
    assert!(contains(
        &packet(doc, METADATA_XMP).unwrap(),
        b"HasExtendedXMP"
    ));

    let (jpeg, carried) = save(doc, &dir.path().join("clean.jpg"), FORMAT_JPEG, true, false);
    assert!(carried & CARRIES_XMP != 0);
    let written = xmp_in(&jpeg);
    assert!(written.chunks.is_empty());
    assert!(
        !contains(written.standard(), b"HasExtendedXMP"),
        "no stale note"
    );
    let path = dir.path().join("clean-reopen.jpg");
    std::fs::write(&path, &jpeg).unwrap();
    assert!(
        import_warnings(&path).is_empty(),
        "the export reopens clean"
    );
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------------- GImage / GDepth --

#[test]
fn google_pixel_bound_properties_leave_flat_exports_but_stay_in_the_rz() {
    let dir = TempDir::new().unwrap();
    let image = filler(60, 150_000);
    let depth = filler(61, 40_000);
    let pixel = xmp_packet(
        concat!(
            " xmlns:dc=\"http://purl.org/dc/elements/1.1/\"",
            " xmlns:GImage=\"http://ns.google.com/photos/1.0/image/\"",
            " xmlns:GDepth=\"http://ns.google.com/photos/1.0/depthmap/\"",
            " xmlns:GCamera=\"http://ns.google.com/photos/1.0/camera/\""
        ),
        &format!(
            " GCamera:MicroVideo=\"1\" GImage:Mime=\"image/jpeg\" GImage:Data=\"{image}\" \
             GDepth:Format=\"RangeInverse\""
        ),
        &format!(
            "   <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">Pixel</rdf:li></rdf:Alt></dc:title>\n   \
             <GDepth:Data>{depth}</GDepth:Data>"
        ),
    );
    let doc = doc_with_xmp(&dir, &pixel);
    for (format, name) in [
        (FORMAT_JPEG, "p.jpg"),
        (FORMAT_PNG, "p.png"),
        (FORMAT_TIFF, "p.tif"),
    ] {
        let (bytes, carried) = save(doc, &dir.path().join(name), format, true, false);
        assert!(carried & CARRIES_XMP != 0, "{name}");
        if format == FORMAT_JPEG {
            assert!(xmp_in(&bytes).chunks.is_empty(), "fits once they are gone");
        }
        let xmp = reopened_xmp(&dir, &format!("re-{name}"), &bytes).unwrap();
        for gone in ["GImage:Mime", "GImage:Data", "GDepth:Format", "GDepth:Data"] {
            assert!(!contains(&xmp, gone.as_bytes()), "{name}: {gone}");
        }
        assert!(!contains(&xmp, image.as_bytes()) && !contains(&xmp, depth.as_bytes()));
        assert!(
            contains(&xmp, b"GCamera:MicroVideo=\"1\""),
            "{name}: other XMP kept"
        );
        assert!(contains(&xmp, b"Pixel"), "{name}: dc:title kept");
    }

    let rz = dir.path().join("p.rz");
    let c = cpath(&rz);
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(doc, c.as_ptr(), &mut err) },
        "{}",
        take_err_string(err)
    );
    let reopened = unsafe { dr_doc_open(c.as_ptr(), &mut err) };
    assert!(!reopened.is_null(), "{}", take_err_string(err));
    assert_eq!(
        packet(reopened, METADATA_XMP),
        Some(pixel),
        "the .doctored keeps them"
    );
    unsafe {
        dr_doc_free(reopened);
        dr_doc_free(doc);
    }
}

// --------------------------------------------------- adversarial input --

#[test]
fn packets_the_scanner_cannot_describe_are_dropped_without_panicking() {
    let dir = TempDir::new().unwrap();
    let pad = format!("<a:pad>{}</a:pad>", filler(70, 70_000));
    let cases = [
        (
            "deep",
            format!(
                "{pad}<a:x>{}{}</a:x>",
                "<a:y>".repeat(200),
                "</a:y>".repeat(200)
            ),
        ),
        ("comment", format!("{pad}<!-- never closed")),
        ("cdata", format!("{pad}<a:x><![CDATA[ never closed</a:x>")),
        ("mismatched", format!("{pad}<a:x></a:y>")),
    ];
    for (name, body) in cases {
        let bad = xmp_packet(" xmlns:a=\"urn:a\"", "", &body);
        let doc = doc_with_xmp(&dir, &bad);
        let (jpeg, carried) = save(
            doc,
            &dir.path().join(format!("{name}.jpg")),
            FORMAT_JPEG,
            true,
            false,
        );
        assert_eq!(carried & CARRIES_XMP, 0, "{name}");
        assert!(xmp_in(&jpeg).standards.is_empty(), "{name}");
        unsafe { dr_doc_free(doc) };
    }
}

#[test]
fn quoted_angle_brackets_and_a_hundred_thousand_attributes_still_split() {
    let dir = TempDir::new().unwrap();
    let attrs: String = (0..100_000).map(|i| format!(" a:p{i}=\"{i}\"")).collect();
    let xmp = xmp_packet(
        " xmlns:a=\"urn:a\"",
        &format!(" a:quoted=\"x > y\"{attrs}"),
        "   <a:t>1 > 0</a:t>",
    );
    let doc = doc_with_xmp(&dir, &xmp);
    let (jpeg, carried) = save(doc, &dir.path().join("many.jpg"), FORMAT_JPEG, true, false);
    assert!(carried & CARRIES_XMP != 0);
    assert_split(&jpeg);
    let reopened = reopened_xmp(&dir, "many-reopen.jpg", &jpeg).unwrap();
    assert!(contains(&reopened, b"a:quoted=\"x > y\""));
    assert!(contains(&reopened, b" a:p99999=\"99999\""));
    assert!(contains(&reopened, b"<a:t>1 > 0</a:t>"));
    unsafe { dr_doc_free(doc) };
}
