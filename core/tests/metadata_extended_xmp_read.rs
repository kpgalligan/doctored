//! Reading JPEG ExtendedXMP: the
//! `http://ns.adobe.com/xmp/extension/` chunks a JPEG carries when its XMP
//! outgrows one segment are merged into the document's one logical XMP
//! packet on open, and every way that can fail keeps the standard packet
//! and says why in the import report. Also the RFC 1321 MD5 the GUID is,
//! through its `#[doc(hidden)]` seam.
//!
//! Every JPEG here is assembled by `common::extended_xmp_fixture` from bytes
//! this crate's tests write; no real Pixel or Adobe file is available, so
//! all fixtures are synthetic. Exercised through the public C FFI
//! (`dr_doc_open`, `dr_doc_metadata`, `dr_path_metadata_import_report`).

use std::ffi::{c_char, c_int, CStr};
use std::path::Path;
use std::process::Command;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_metadata_report::dr_path_metadata_import_report;
use doctored_core::md5::md5;
use tempfile::TempDir;

use crate::common;
use common::extended_xmp_fixture::*;
use common::metadata_fixtures::{open_file, packet};
use common::*;

const METADATA_XMP: c_int = 1;
/// `MAX_DRDC_BLOB_LEN` (16 MiB), which is `pub(crate)` in the core.
const BLOB_CAP: usize = 16 * 1024 * 1024;

const W_INCOMPLETE: &str =
    "Extended XMP is incomplete or inconsistent; only the standard XMP packet was read";
const W_TOO_LARGE: &str = "Extended XMP exceeds 16 MiB; only the standard XMP packet was read";
const W_ORPHAN: &str = "Extended XMP segments did not match the XMP packet and were not read";

/// Chunk size the fixtures split at: smaller than the writer's, so a
/// ~120 KB extension makes three chunks.
const FIXTURE_CHUNK: usize = 50_000;

// -------------------------------------------------------------- md5 --

fn hex(digest: [u8; 16]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn md5_matches_the_rfc_1321_test_suite() {
    // RFC 1321, appendix A.5.
    for (input, expected) in [
        ("", "d41d8cd98f00b204e9800998ecf8427e"),
        ("a", "0cc175b9c0f1b6a831c399e269772661"),
        ("abc", "900150983cd24fb0d6963f7d28e17f72"),
        ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
        (
            "abcdefghijklmnopqrstuvwxyz",
            "c3fcd3d76192e4007dfb496cca67e13b",
        ),
        (
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
            "d174ab98d277d9f5a5611c2c9f419d9f",
        ),
        (
            "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
            "57edf4a22be3c955ac49da2e2107b67a",
        ),
    ] {
        assert_eq!(hex(md5(input.as_bytes())), expected, "md5({input:?})");
    }
}

/// The system's own MD5 (`/sbin/md5 -q` on macOS, `md5sum` elsewhere) over
/// 64 consecutive lengths — every residue of the padding modulo 64,
/// including the lengths that need a whole extra block. Skipped when
/// neither tool exists.
#[test]
fn md5_agrees_with_the_system_md5_at_every_padding_residue() {
    let tool: Option<(&str, &[&str])> = if Path::new("/sbin/md5").exists() {
        Some(("/sbin/md5", &["-q"]))
    } else if Command::new("md5sum").arg("--version").output().is_ok() {
        Some(("md5sum", &[]))
    } else {
        None
    };
    let Some((program, args)) = tool else {
        println!("skipping: no system md5 tool");
        return;
    };
    let dir = TempDir::new().unwrap();
    let mut residues = std::collections::BTreeSet::new();
    for len in 1000..1064usize {
        let bytes: Vec<u8> = (0..len).map(|i| (i * 31 + len) as u8).collect();
        let path = dir.path().join(format!("m{len}"));
        std::fs::write(&path, &bytes).unwrap();
        let out = Command::new(program)
            .args(args)
            .arg(&path)
            .output()
            .unwrap();
        assert!(out.status.success(), "{program} ran");
        let text = String::from_utf8(out.stdout).unwrap();
        let system = text.split_whitespace().next().unwrap().to_string();
        assert_eq!(hex(md5(&bytes)), system, "length {len}");
        residues.insert(len % 64);
    }
    assert_eq!(residues.len(), 64);
}

// ---------------------------------------------------------- fixtures --

/// A standard packet whose note names `guid`, an extension holding a large
/// `photoshop:History` and a large `crs:` attribute, and the GUID.
struct Case {
    standard: Vec<u8>,
    extension: Vec<u8>,
    guid: Vec<u8>,
    history: String,
    attribute: String,
}

fn standard_with_note(note: &str) -> Vec<u8> {
    xmp_packet(
        concat!(
            " xmlns:dc=\"http://purl.org/dc/elements/1.1/\"",
            " xmlns:xmpNote=\"http://ns.adobe.com/xmp/note/\""
        ),
        &format!(" xmpNote:HasExtendedXMP=\"{note}\""),
        "   <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">Standard title</rdf:li></rdf:Alt></dc:title>",
    )
}

fn case() -> Case {
    let history = filler(20, 90_000);
    let attribute = filler(21, 30_000);
    let extension = extension_packet(
        "",
        &format!(
            "<rdf:Description rdf:about=\"\" \
             xmlns:photoshop=\"http://ns.adobe.com/photoshop/1.0/\" \
             xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
             crs:Big=\"{attribute}\"><photoshop:History>{history}</photoshop:History>\
             </rdf:Description>"
        ),
    );
    let guid = guid_of(&extension);
    Case {
        standard: standard_with_note(std::str::from_utf8(&guid).unwrap()),
        extension,
        guid,
        history,
        attribute,
    }
}

/// What opening a JPEG of `segments` gives: the XMP packet and the import
/// report's warnings.
fn opened(dir: &TempDir, name: &str, segments: &[Vec<u8>]) -> (Option<Vec<u8>>, Vec<String>) {
    let bytes = jpeg_with(segments);
    let doc = open_file(dir, name, &bytes);
    let xmp = packet(doc, METADATA_XMP);
    unsafe { dr_doc_free(doc) };
    let report = report(&dir.path().join(name));
    let warnings = report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap().to_string())
        .collect();
    (xmp, warnings)
}

fn report(path: &Path) -> serde_json::Value {
    let c = cpath(path);
    unsafe {
        let mut err: *mut c_char = ptr::null_mut();
        let json = dr_path_metadata_import_report(c.as_ptr(), &mut err);
        assert!(!json.is_null(), "{}", take_err_string(err));
        let value = serde_json::from_str(CStr::from_ptr(json).to_str().unwrap()).unwrap();
        dr_string_free(json);
        value
    }
}

/// Asserts `xmp` is the merge of `c`: every extension property present,
/// the standard's own kept, the note gone, one `rdf:RDF`, and no start tag
/// repeating an attribute.
fn assert_merged(xmp: &[u8], c: &Case) {
    assert!(
        contains(xmp, c.history.as_bytes()),
        "photoshop:History merged"
    );
    assert!(contains(xmp, c.attribute.as_bytes()), "crs:Big merged");
    assert!(contains(xmp, b"Standard title"), "standard property kept");
    assert!(!contains(xmp, b"HasExtendedXMP"), "the note is gone");
    assert_eq!(count(xmp, b"<rdf:RDF"), 1);
    assert_eq!(count(xmp, b"</rdf:RDF>"), 1);
    assert_eq!(repeated_attribute(xmp), None);
}

// ------------------------------------------------------------ merging --

#[test]
fn chunks_in_order_merge_into_one_logical_packet() {
    let dir = TempDir::new().unwrap();
    let c = case();
    let mut segments = vec![standard_segment(&c.standard)];
    segments.extend(extension_segments(&c.extension, &c.guid, FIXTURE_CHUNK));
    assert_eq!(segments.len(), 4, "three chunks");
    let (xmp, warnings) = opened(&dir, "in-order.jpg", &segments);
    let xmp = xmp.expect("an XMP packet");
    assert_merged(&xmp, &c);
    assert!(
        xmp.len() > STANDARD_LIMIT,
        "the logical packet is the large one"
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn chunks_out_of_order_and_before_the_standard_packet_merge_the_same() {
    let dir = TempDir::new().unwrap();
    let c = case();
    let chunks = extension_segments(&c.extension, &c.guid, FIXTURE_CHUNK);
    let in_order = [vec![standard_segment(&c.standard)], chunks.clone()].concat();
    let shuffled = vec![
        chunks[2].clone(),
        chunks[0].clone(),
        standard_segment(&c.standard),
        chunks[1].clone(),
    ];
    let (a, wa) = opened(&dir, "a.jpg", &in_order);
    let (b, wb) = opened(&dir, "b.jpg", &shuffled);
    assert_eq!(a, b, "the merge does not depend on chunk order");
    assert_merged(&b.unwrap(), &c);
    assert!(wa.is_empty() && wb.is_empty());
}

#[test]
fn a_lowercase_note_guid_still_names_its_chunks() {
    let dir = TempDir::new().unwrap();
    let mut c = case();
    let lower = String::from_utf8(c.guid.clone()).unwrap().to_lowercase();
    c.standard = standard_with_note(&format!("  {lower} "));
    let segments = [
        vec![standard_segment(&c.standard)],
        extension_segments(&c.extension, &c.guid, FIXTURE_CHUNK),
    ]
    .concat();
    let (xmp, warnings) = opened(&dir, "lower.jpg", &segments);
    assert_merged(&xmp.unwrap(), &c);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn a_stale_second_extension_merges_ours_and_reports_the_orphans() {
    let dir = TempDir::new().unwrap();
    let c = case();
    let stale = extension_packet("", "<rdf:Description rdf:about=\"\"/>");
    let stale_guid = guid_of(&stale);
    let segments = [
        vec![standard_segment(&c.standard)],
        extension_segments(&c.extension, &c.guid, FIXTURE_CHUNK),
        extension_segments(&stale, &stale_guid, FIXTURE_CHUNK),
    ]
    .concat();
    let (xmp, warnings) = opened(&dir, "stale.jpg", &segments);
    assert_merged(&xmp.unwrap(), &c);
    assert_eq!(warnings, vec![W_ORPHAN.to_string()]);
}

#[test]
fn a_non_hex_guid_keeps_the_standard_packet() {
    let dir = TempDir::new().unwrap();
    let c = case();
    for (name, note) in [
        ("short", "ABCDEF"),
        ("nonhex", "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"),
        ("long", "0123456789ABCDEF0123456789ABCDEF0"),
    ] {
        let standard = standard_with_note(note);
        let segments = [
            vec![standard_segment(&standard)],
            extension_segments(&c.extension, &c.guid, FIXTURE_CHUNK),
        ]
        .concat();
        let (xmp, warnings) = opened(&dir, &format!("{name}.jpg"), &segments);
        assert_eq!(xmp.as_deref(), Some(standard.as_slice()), "{name}");
        assert_eq!(warnings, vec![W_INCOMPLETE.to_string()], "{name}");
    }
}

#[test]
fn missing_overlapping_and_mismatched_chunks_keep_the_standard_packet() {
    let dir = TempDir::new().unwrap();
    let c = case();
    let full = c.extension.len() as u32;
    let piece = |from: usize, to: usize| &c.extension[from..to];
    let n = c.extension.len();
    let at = |offset: usize, to: usize| {
        extension_segment(&c.guid, full, offset as u32, piece(offset, to))
    };
    assert!(n > 100_000 && n < 150_000, "three fixture chunks");
    let cases: Vec<(&str, Vec<Vec<u8>>)> = vec![
        ("missing", vec![at(0, 50_000), at(100_000, n)]),
        (
            "overlapping",
            vec![at(0, 50_000), at(49_990, 100_000), at(100_000, n)],
        ),
        (
            "duplicate",
            vec![
                at(0, 50_000),
                at(0, 50_000),
                at(50_000, 100_000),
                at(100_000, n),
            ],
        ),
        (
            "mismatched-length",
            vec![
                at(0, 50_000),
                extension_segment(&c.guid, full + 1, 50_000, piece(50_000, 100_000)),
                at(100_000, n),
            ],
        ),
        ("short-of-full", vec![at(0, 50_000), at(50_000, 100_000)]),
        (
            "past-full",
            vec![
                extension_segment(&c.guid, 60_000, 0, piece(0, 50_000)),
                extension_segment(&c.guid, 60_000, 50_000, piece(50_000, 61_000)),
            ],
        ),
        (
            "empty-chunk",
            vec![
                at(0, 50_000),
                at(50_000, 50_000),
                at(50_000, 100_000),
                at(100_000, n),
            ],
        ),
        ("note-without-chunks", vec![]),
    ];
    for (name, chunks) in cases {
        let segments = [vec![standard_segment(&c.standard)], chunks].concat();
        let (xmp, warnings) = opened(&dir, &format!("{name}.jpg"), &segments);
        assert_eq!(xmp.as_deref(), Some(c.standard.as_slice()), "{name}");
        assert_eq!(warnings, vec![W_INCOMPLETE.to_string()], "{name}");
    }
}

#[test]
fn chunks_nothing_points_at_are_orphans() {
    let dir = TempDir::new().unwrap();
    let c = case();
    let plain = xmp_packet(
        " xmlns:dc=\"http://purl.org/dc/elements/1.1/\"",
        " dc:format=\"image/jpeg\"",
        "",
    );
    let chunks = extension_segments(&c.extension, &c.guid, FIXTURE_CHUNK);
    let (xmp, warnings) = opened(
        &dir,
        "orphan.jpg",
        &[vec![standard_segment(&plain)], chunks.clone()].concat(),
    );
    assert_eq!(xmp.as_deref(), Some(plain.as_slice()));
    assert_eq!(warnings, vec![W_ORPHAN.to_string()]);

    let (xmp, warnings) = opened(&dir, "orphan-alone.jpg", &chunks);
    assert_eq!(xmp, None, "no standard packet, nothing to merge into");
    assert_eq!(warnings, vec![W_ORPHAN.to_string()]);
}

#[test]
fn a_declared_length_past_the_blob_cap_is_refused_before_allocating() {
    let dir = TempDir::new().unwrap();
    let c = case();
    let over = (BLOB_CAP - c.standard.len() + 1) as u32;
    let segments = vec![
        standard_segment(&c.standard),
        extension_segment(&c.guid, over, 0, &c.extension[..1000]),
    ];
    let (xmp, warnings) = opened(&dir, "too-large.jpg", &segments);
    assert_eq!(xmp.as_deref(), Some(c.standard.as_slice()));
    assert_eq!(warnings, vec![W_TOO_LARGE.to_string()]);

    // One byte less is within the cap; the same lone chunk then simply does
    // not tile its declared length.
    let at_cap = (BLOB_CAP - c.standard.len()) as u32;
    let segments = vec![
        standard_segment(&c.standard),
        extension_segment(&c.guid, at_cap, 0, &c.extension[..1000]),
    ];
    let (xmp, warnings) = opened(&dir, "at-cap.jpg", &segments);
    assert_eq!(xmp.as_deref(), Some(c.standard.as_slice()));
    assert_eq!(warnings, vec![W_INCOMPLETE.to_string()]);
}

#[test]
fn declarations_inherited_from_the_extensions_rdf_travel_with_its_descriptions() {
    let dir = TempDir::new().unwrap();
    let extension = extension_packet(
        " xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" xml:lang=\"en\"",
        &format!(
            "<rdf:Description rdf:about=\"\" crs:Exposure=\"+1.25\" crs:Notes=\"{}\"/>\
             <rdf:Description rdf:about=\"\" \
             xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" crs:Shadows=\"12\"/>",
            filler(30, 70_000)
        ),
    );
    let guid = guid_of(&extension);
    let standard = standard_with_note(std::str::from_utf8(&guid).unwrap());
    let segments = [
        vec![standard_segment(&standard)],
        extension_segments(&extension, &guid, FIXTURE_CHUNK),
    ]
    .concat();
    let (xmp, warnings) = opened(&dir, "inherited.jpg", &segments);
    let xmp = xmp.unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    // The start tag holding crs:Exposure now declares crs and xml:lang.
    let at = xmp
        .windows(b"crs:Exposure".len())
        .position(|w| w == b"crs:Exposure")
        .unwrap();
    let open = xmp[..at]
        .windows(b"<rdf:Description".len())
        .rposition(|w| w == b"<rdf:Description")
        .unwrap();
    let close = at + xmp[at..].iter().position(|&b| b == b'>').unwrap();
    let tag = &xmp[open..close];
    assert!(contains(
        tag,
        b"xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\""
    ));
    assert!(contains(tag, b"xml:lang=\"en\""));
    // The second Description declared crs itself and keeps one declaration.
    assert_eq!(repeated_attribute(&xmp), None);
    assert!(contains(&xmp, b"crs:Shadows=\"12\""));
}

#[test]
fn the_jpeg_import_report_lists_extended_xmp_as_read() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("plain.jpg");
    std::fs::write(&path, jpeg_with(&[])).unwrap();
    let report = report(&path);
    assert_eq!(report["container"], "jpeg");
    assert_eq!(
        report["supported_packets"],
        serde_json::json!(["EXIF", "XMP (including Extended XMP)", "IPTC"])
    );
    assert_eq!(report["limitations"], serde_json::json!([]));
    assert!(!report.to_string().contains("Extended XMP is not read"));
    assert_eq!(report["warnings"], serde_json::json!([]));
}

// --------------------------------------------------- adversarial input --

#[test]
fn an_extension_the_scanner_cannot_describe_keeps_the_standard_packet() {
    let dir = TempDir::new().unwrap();
    let deep = format!(
        "<rdf:Description rdf:about=\"\"><a:x xmlns:a=\"urn:a\">{}{}</a:x></rdf:Description>",
        "<a:y>".repeat(200),
        "</a:y>".repeat(200)
    );
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("deep", extension_packet("", &deep)),
        (
            "comment",
            extension_packet("", "<!-- never closed <rdf:Description rdf:about=\"\"/>"),
        ),
        (
            "cdata",
            extension_packet(
                "",
                "<rdf:Description rdf:about=\"\"><a:x xmlns:a=\"urn:a\"><![CDATA[ open",
            ),
        ),
        (
            "mismatched",
            extension_packet(
                "",
                "<rdf:Description rdf:about=\"\"><a:x xmlns:a=\"urn:a\"></a:y></rdf:Description>",
            ),
        ),
        ("not-xml", vec![0xFF; 3000]),
    ];
    for (name, extension) in cases {
        let guid = guid_of(&extension);
        let standard = standard_with_note(std::str::from_utf8(&guid).unwrap());
        let segments = [
            vec![standard_segment(&standard)],
            extension_segments(&extension, &guid, FIXTURE_CHUNK),
        ]
        .concat();
        let (xmp, warnings) = opened(&dir, &format!("adv-{name}.jpg"), &segments);
        assert_eq!(xmp.as_deref(), Some(standard.as_slice()), "{name}");
        assert_eq!(warnings, vec![W_INCOMPLETE.to_string()], "{name}");
    }
}

#[test]
fn a_quoted_angle_bracket_and_many_attributes_still_merge() {
    let dir = TempDir::new().unwrap();
    let attrs: String = (0..100_000).map(|i| format!(" a:p{i}=\"{i}\"")).collect();
    let extension = extension_packet(
        " xmlns:a=\"urn:a\"",
        &format!(
            "<rdf:Description rdf:about=\"\" a:quoted=\"x > y\"{attrs}>\
             <a:t>1 > 0</a:t></rdf:Description>"
        ),
    );
    let guid = guid_of(&extension);
    let standard = standard_with_note(std::str::from_utf8(&guid).unwrap());
    let segments = [
        vec![standard_segment(&standard)],
        extension_segments(&extension, &guid, CHUNK_DATA),
    ]
    .concat();
    let (xmp, warnings) = opened(&dir, "many.jpg", &segments);
    let xmp = xmp.unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(contains(&xmp, b"a:quoted=\"x > y\""));
    assert!(contains(&xmp, b" a:p99999=\"99999\""));
    assert!(contains(&xmp, b"<a:t>1 > 0</a:t>"));
}
