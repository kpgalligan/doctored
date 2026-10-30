//! How a packet is FRAMED in the file it travels in: an 8BIM run too large
//! for one JPEG segment is split across consecutive APP13s on export, and a
//! compressed PNG XMP `iTXt` (flag 1, method 0) is inflated on open, bounded
//! by the blob cap and at most once per walk. Exercised through the public C
//! FFI; the oracles are this crate's test-side walkers in
//! `common::metadata_fixtures`, never the core's. No golden images.

use std::ffi::{c_char, c_int, CStr};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_metadata_report::dr_path_metadata_import_report;
use doctored_core::ffi_preservation::dr_doc_flat_save_losses;
use tempfile::TempDir;

use crate::common;
use common::metadata_fixtures::{
    bim, jpeg_segments, open_file, packet, png_chunk_bytes, png_chunks, png_with, save, set_packet,
};
use common::*;

/// Mirrored from the header, like the constants in `metadata_tests.rs`.
const FORMAT_PNG: c_int = 0;
const FORMAT_JPEG: c_int = 1;
const METADATA_XMP: c_int = 1;
const METADATA_IPTC: c_int = 2;
const CARRIES_XMP: u32 = 4;
const CARRIES_IPTC: u32 = 8;

const IPTC_ID: &[u8] = b"Photoshop 3.0\0";
const PNG_XMP_KEYWORD: &[u8] = b"XML:com.adobe.xmp";

/// The run bytes one APP13 carries: 65533 (the largest JPEG segment
/// payload) minus the 14-byte `Photoshop 3.0\0` identifier.
const IPTC_SEGMENT_DATA: usize = 65519;

/// `MAX_DRDC_BLOB_LEN` (16 MiB), which is `pub(crate)` in the core.
const BLOB_CAP: usize = 16 * 1024 * 1024;

const SAMPLE_XMP: &[u8] =
    b"<?xpacket begin=''?><x:xmpmeta xmlns:x='adobe:ns:meta/'/><?xpacket end='w'?>";

// ------------------------------------------------------------- helpers --

/// A document with `run` set as its 8BIM packet.
fn doc_with_iptc(dir: &TempDir, run: &[u8]) -> *mut doctored_core::doc::DrDocument {
    let doc = doc_from(dir, "base.png", &opaque_pattern(6, 4));
    set_packet(doc, METADATA_IPTC, run)
}

/// Every `Photoshop 3.0\0` APP13 of a JPEG as (segment index, payload after
/// the identifier), in file order.
fn iptc_segments(bytes: &[u8]) -> Vec<(usize, Vec<u8>)> {
    jpeg_segments(bytes)
        .into_iter()
        .enumerate()
        .filter(|(_, (m, p))| *m == 0xED && p.starts_with(IPTC_ID))
        .map(|(i, (_, p))| (i, p[IPTC_ID.len()..].to_vec()))
        .collect()
}

/// Asserts the APP13s are adjacent, each within one segment, every one but
/// the last full, and that they join back into `run`.
fn assert_split(bytes: &[u8], run: &[u8], expected_segments: usize) {
    let segments = iptc_segments(bytes);
    assert_eq!(segments.len(), expected_segments, "APP13 count");
    for pair in segments.windows(2) {
        assert_eq!(pair[1].0, pair[0].0 + 1, "no other marker between APP13s");
    }
    for (n, (_, data)) in segments.iter().enumerate() {
        // The length field counts itself: 2 + identifier + data <= 65535.
        assert!(2 + IPTC_ID.len() + data.len() <= 65535, "segment {n} fits");
        if n + 1 < segments.len() {
            assert_eq!(data.len(), IPTC_SEGMENT_DATA, "segment {n} is full");
        }
    }
    let joined: Vec<u8> = segments.into_iter().flat_map(|(_, d)| d).collect();
    assert_eq!(joined, run, "the APP13 payloads concatenate to the run");
}

fn flat_save_losses(doc: *const doctored_core::doc::DrDocument, format: c_int) -> Vec<String> {
    unsafe {
        let mut err: *mut c_char = ptr::null_mut();
        let json = dr_doc_flat_save_losses(doc, format, true, false, &mut err);
        assert!(!json.is_null(), "{}", take_err_string(err));
        let result = serde_json::from_str(CStr::from_ptr(json).to_str().unwrap()).unwrap();
        dr_string_free(json);
        result
    }
}

/// The data of an XMP `iTXt`: keyword, NUL, flag, method, empty language
/// tag and translated keyword, then `text`.
fn xmp_itxt(flag: u8, method: u8, text: &[u8]) -> Vec<u8> {
    let mut data = PNG_XMP_KEYWORD.to_vec();
    data.extend_from_slice(&[0, flag, method, 0, 0]);
    data.extend_from_slice(text);
    data
}

fn zlib(bytes: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(bytes, 6)
}

/// A 4x4 PNG with the given chunks after `IHDR`.
fn png_file(chunks: &[Vec<u8>]) -> Vec<u8> {
    png_with(&opaque_pattern(4, 4), chunks)
}

/// The XMP a PNG opens with, freeing the document.
fn opened_xmp(dir: &TempDir, name: &str, file: &[u8]) -> Option<Vec<u8>> {
    let doc = open_file(dir, name, file);
    let xmp = packet(doc, METADATA_XMP);
    unsafe { dr_doc_free(doc) };
    xmp
}

// ------------------------------------------------------ the 8BIM split --

#[test]
fn an_oversize_8bim_run_is_split_across_consecutive_app13_segments() {
    let dir = TempDir::new().unwrap();
    let caption: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
    let slices: Vec<u8> = (0..100_001u32).map(|i| (i % 241) as u8).collect();
    let run = [bim(0x0404, b"", &caption), bim(0x040C, b"thumb", &slices)].concat();
    assert!(
        run.len() > 3 * IPTC_SEGMENT_DATA,
        "about 200 KB: four segments"
    );
    let doc = doc_with_iptc(&dir, &run);

    let path = dir.path().join("split.jpg");
    let (bytes, carried) = save(doc, &path, FORMAT_JPEG, true, false);
    assert!(
        carried & CARRIES_IPTC != 0,
        "the run is written, not dropped"
    );
    assert_split(&bytes, &run, run.len().div_ceil(IPTC_SEGMENT_DATA));

    let reopened = open_file(&dir, "split-reopen.jpg", &bytes);
    assert_eq!(packet(reopened, METADATA_IPTC), Some(run.clone()));
    unsafe { dr_doc_free(reopened) };

    let losses = flat_save_losses(doc, FORMAT_JPEG);
    assert!(
        !losses.iter().any(|l| l == "IPTC metadata"),
        "the Save preflight no longer reports an oversize IPTC loss: {losses:?}"
    );
    unsafe { dr_doc_free(doc) };
}

#[test]
fn the_app13_split_boundary_is_one_full_segment() {
    let dir = TempDir::new().unwrap();
    // An empty-name 8BIM block is 12 bytes of header before its data.
    for (total, segments) in [(IPTC_SEGMENT_DATA - 1, 1), (IPTC_SEGMENT_DATA + 1, 2)] {
        let run = bim(0x0404, b"", &vec![0x5A; total - 12]);
        assert_eq!(run.len(), total);
        let doc = doc_with_iptc(&dir, &run);
        let (bytes, carried) = save(
            doc,
            &dir.path().join(format!("edge-{total}.jpg")),
            FORMAT_JPEG,
            true,
            false,
        );
        assert!(carried & CARRIES_IPTC != 0);
        assert_split(&bytes, &run, segments);
        let reopened = open_file(&dir, &format!("edge-{total}-reopen.jpg"), &bytes);
        assert_eq!(packet(reopened, METADATA_IPTC), Some(run));
        unsafe {
            dr_doc_free(reopened);
            dr_doc_free(doc);
        }
    }
}

// ------------------------------------------------- the compressed iTXt --

#[test]
fn a_compressed_png_xmp_itxt_is_inflated_on_open() {
    let dir = TempDir::new().unwrap();
    let file = png_file(&[png_chunk_bytes(b"iTXt", &xmp_itxt(1, 0, &zlib(SAMPLE_XMP)))]);
    let doc = open_file(&dir, "compressed.png", &file);
    assert_eq!(packet(doc, METADATA_XMP).as_deref(), Some(SAMPLE_XMP));

    let (bytes, carried) = save(
        doc,
        &dir.path().join("resaved.png"),
        FORMAT_PNG,
        true,
        false,
    );
    assert!(carried & CARRIES_XMP != 0);
    let itxt = png_chunks(&bytes)
        .into_iter()
        .find(|(k, d)| k == b"iTXt" && d.starts_with(PNG_XMP_KEYWORD))
        .map(|(_, d)| d)
        .expect("an XMP iTXt");
    let key_end = PNG_XMP_KEYWORD.len();
    assert_eq!(itxt[key_end], 0, "keyword terminator");
    assert_eq!(
        itxt[key_end + 1],
        0,
        "the writer writes an uncompressed iTXt"
    );
    assert_eq!(&itxt[key_end + 5..], SAMPLE_XMP);
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_compressed_xmp_that_cannot_be_trusted_reads_as_absent() {
    let dir = TempDir::new().unwrap();
    let stream = zlib(SAMPLE_XMP);
    let itxt = |text: &[u8]| png_chunk_bytes(b"iTXt", &xmp_itxt(1, 0, text));

    let mut flipped_adler = stream.clone();
    *flipped_adler.last_mut().unwrap() ^= 0x01;
    let mut bad_crc = itxt(&stream);
    *bad_crc.last_mut().unwrap() ^= 0x01;

    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "method1",
            png_chunk_bytes(b"iTXt", &xmp_itxt(1, 1, &stream)),
        ),
        ("truncated", itxt(&stream[..stream.len() - 5])),
        ("adler", itxt(&flipped_adler)),
        ("trailing", itxt(&[stream.clone(), vec![0]].concat())),
        ("crc", bad_crc),
        ("overcap", itxt(&zlib(&vec![0u8; BLOB_CAP + 1]))),
    ];
    for (name, chunk) in cases {
        let file = png_file(&[chunk]);
        assert_eq!(
            opened_xmp(&dir, &format!("{name}.png"), &file),
            None,
            "{name}: an untrustworthy compressed XMP reads as absent"
        );
    }

    let at_cap = vec![0u8; BLOB_CAP];
    let file = png_file(&[itxt(&zlib(&at_cap))]);
    let xmp = opened_xmp(&dir, "atcap.png", &file).expect("exactly the cap is captured");
    assert_eq!(xmp.len(), BLOB_CAP);
    assert!(xmp == at_cap);
}

#[test]
fn the_first_xmp_itxt_wins_compressed_or_not() {
    let dir = TempDir::new().unwrap();
    let a: &[u8] = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'>A</x:xmpmeta>";
    let b: &[u8] = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'>B</x:xmpmeta>";
    let plain = |t: &[u8]| png_chunk_bytes(b"iTXt", &xmp_itxt(0, 0, t));
    let deflated = |t: &[u8]| png_chunk_bytes(b"iTXt", &xmp_itxt(1, 0, &zlib(t)));
    let failed = {
        let mut s = zlib(a);
        *s.last_mut().unwrap() ^= 0x01;
        png_chunk_bytes(b"iTXt", &xmp_itxt(1, 0, &s))
    };

    let cases = [
        ("plain-then-deflated", vec![plain(a), deflated(b)], Some(a)),
        ("deflated-then-plain", vec![deflated(a), plain(b)], Some(a)),
        (
            "failed-then-deflated",
            vec![failed.clone(), deflated(b)],
            None,
        ),
        ("failed-then-plain", vec![failed, plain(b)], Some(b)),
    ];
    for (name, chunks, expected) in cases {
        let file = png_file(&chunks);
        assert_eq!(
            opened_xmp(&dir, &format!("{name}.png"), &file).as_deref(),
            expected,
            "{name}"
        );
    }
}

/// A walk inflates at most ONE compressed XMP, whatever its outcome — the
/// style of `metadata_tests::an_oversize_chunk_is_skipped_before_its_crc_is_computed`.
/// Each chunk inflates to one byte past the blob cap, so each attempt costs
/// a full 16 MiB of output before it fails; without the one-attempt rule
/// the 64-chunk file costs about 64 times the 1-chunk one. The default test
/// checks every open; its `mod bench` twin (`make cancel-bench`) times them.
#[test]
fn only_one_compressed_xmp_is_inflated_per_open() {
    let dir = TempDir::new().unwrap();
    let chunk = png_chunk_bytes(b"iTXt", &xmp_itxt(1, 0, &zlib(&vec![0u8; BLOB_CAP + 1])));
    let open = |count: usize, name: &str| {
        let file = png_file(&vec![chunk.clone(); count]);
        std::fs::write(dir.path().join(name), &file).unwrap();
        let c = cpath(&dir.path().join(name));
        let mut err: *mut c_char = ptr::null_mut();
        let doc = unsafe { dr_doc_open(c.as_ptr(), &mut err) };
        assert!(!doc.is_null(), "{}", take_err_string(err));
        assert_eq!(packet(doc, METADATA_XMP), None, "past the blob cap");
        unsafe { dr_doc_free(doc) };
    };
    open(1, "one.png");
    open(64, "many.png");
}

#[test]
fn the_png_import_report_lists_xmp_without_a_compression_caveat() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("report.png");
    std::fs::write(&path, png_file(&[])).unwrap();
    let c = cpath(&path);
    let report: serde_json::Value = unsafe {
        let mut err: *mut c_char = ptr::null_mut();
        let json = dr_path_metadata_import_report(c.as_ptr(), &mut err);
        assert!(!json.is_null(), "{}", take_err_string(err));
        let value = serde_json::from_str(CStr::from_ptr(json).to_str().unwrap()).unwrap();
        dr_string_free(json);
        value
    };
    assert_eq!(report["container"], "png");
    assert_eq!(
        report["supported_packets"],
        serde_json::json!(["EXIF", "XMP"])
    );
    assert_eq!(
        report["limitations"],
        serde_json::json!(["IPTC is not read"])
    );
}

mod bench {
    use super::*;

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn only_one_compressed_xmp_is_inflated_per_open() {
        let dir = TempDir::new().unwrap();
        let chunk = png_chunk_bytes(b"iTXt", &xmp_itxt(1, 0, &zlib(&vec![0u8; BLOB_CAP + 1])));
        let elapsed = |count: usize, name: &str| {
            let file = png_file(&vec![chunk.clone(); count]);
            std::fs::write(dir.path().join(name), &file).unwrap();
            let c = cpath(&dir.path().join(name));
            let mut best = std::time::Duration::MAX;
            for _ in 0..3 {
                let start = std::time::Instant::now();
                let mut err: *mut c_char = ptr::null_mut();
                let doc = unsafe { dr_doc_open(c.as_ptr(), &mut err) };
                best = best.min(start.elapsed());
                assert!(!doc.is_null(), "{}", take_err_string(err));
                assert_eq!(packet(doc, METADATA_XMP), None, "past the blob cap");
                unsafe { dr_doc_free(doc) };
            }
            best
        };
        let one = elapsed(1, "one.png");
        let many = elapsed(64, "many.png");
        println!("one {one:?} many {many:?}");
        assert!(
            many < one * 4,
            "later compressed XMP chunks must not be inflated (one {one:?}, many {many:?})"
        );
    }
}
