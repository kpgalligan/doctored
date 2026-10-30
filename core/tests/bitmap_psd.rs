//! #1 Bitmap phase 1c's colour-mode-0 PSD and PSB files through the FFI
//! (Adobe's Photoshop File Format Specification: depth 1, mode 0, an empty
//! Color Mode Data section, rows of `ceil(w/8)` bytes, 1 BLACK). A file the
//! test writes byte by byte, Raw and RLE, 13 px wide, gives the import
//! report and opens with the plane inverted from the file; the PSD and PSB
//! exports are parsed here (header 1 channel, depth 1, mode 0) and their rows
//! decoded by this file's own PackBits, each row the plane's inverse, and
//! they reopen equal. The layered TIFF, the side limit (30001 × 1), a depth
//! other than 1 and a ZIP composite refuse by sentence, and (phase 2a-i) a
//! short raw composite and a bad RLE row refuse in the report by the open's
//! own sentence; ImageMagick, when
//! installed, reads the PSD (SKIP otherwise).

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_bitmap::dr_precision_doc_open_bitmap_psd;
use doctored_core::ffi_psd::{
    dr_precision_doc_psd_export_report, dr_precision_doc_save_psd, dr_psd_import_report,
};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::bitmap_files::{magick, magick_gray, oracle, oracle_doc, pack};
use crate::bitmap_mode::{bits, gray_doc, to_bitmap};
use crate::common::{cpath, take_err_string};
use crate::indexed_fixture::{descriptor, free, LIMIT};

const W: u32 = 13;
const H: u32 = 3;

fn json_of(text: *mut c_char, err: *mut c_char) -> Result<Value, String> {
    if text.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    Ok(value)
}

/// One PackBits row, every run a literal of at most 128 bytes.
pub(crate) fn packbits(row: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in row.chunks(128) {
        out.push((chunk.len() - 1) as u8);
        out.extend_from_slice(chunk);
    }
    out
}

/// This file's own PackBits decoder (Apple's TN1023): a control byte n of 0
/// to 127 copies the next n + 1 bytes, −1 to −127 repeats the next byte
/// 1 − n times, −128 is a no-op.
pub(crate) fn unpackbits(mut bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some((&control, rest)) = bytes.split_first() {
        let n = control as i8;
        if n >= 0 {
            let count = n as usize + 1;
            out.extend_from_slice(&rest[..count]);
            bytes = &rest[count..];
        } else if n != -128 {
            out.extend(std::iter::repeat_n(rest[0], (1 - i32::from(n)) as usize));
            bytes = &rest[1..];
        } else {
            bytes = rest;
        }
    }
    out
}

/// A PSD (`large` false) or PSB of colour mode 0 at `depth`: one channel,
/// an empty Color Mode Data section, resource 1005 at `ppi` when given, an
/// empty layer section, then compression `word` and `body`.
fn psd(
    w: u32,
    h: u32,
    depth: u16,
    ppi: Option<u32>,
    large: bool,
    word: u16,
    body: &[u8],
) -> Vec<u8> {
    let mut out = b"8BPS".to_vec();
    out.extend_from_slice(&(if large { 2u16 } else { 1 }).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&h.to_be_bytes());
    out.extend_from_slice(&w.to_be_bytes());
    out.extend_from_slice(&depth.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    let mut resources = Vec::new();
    if let Some(ppi) = ppi {
        resources.extend_from_slice(b"8BIM");
        resources.extend_from_slice(&1005u16.to_be_bytes());
        resources.extend_from_slice(&[0, 0]);
        resources.extend_from_slice(&16u32.to_be_bytes());
        for _ in 0..2 {
            resources.extend_from_slice(&(ppi << 16).to_be_bytes());
            resources.extend_from_slice(&1u16.to_be_bytes());
            resources.extend_from_slice(&1u16.to_be_bytes());
        }
    }
    out.extend_from_slice(&(resources.len() as u32).to_be_bytes());
    out.extend_from_slice(&resources);
    if large {
        out.extend_from_slice(&0u64.to_be_bytes());
    } else {
        out.extend_from_slice(&0u32.to_be_bytes());
    }
    out.extend_from_slice(&word.to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// An RLE body: the row table (u16 counts) then each row's PackBits.
fn rle_body(rows: &[u8], line: usize) -> Vec<u8> {
    let encoded: Vec<Vec<u8>> = rows.chunks_exact(line).map(packbits).collect();
    let mut out = Vec::new();
    for row in &encoded {
        out.extend_from_slice(&(row.len() as u16).to_be_bytes());
    }
    for row in encoded {
        out.extend_from_slice(&row);
    }
    out
}

fn report(path: &Path) -> Result<Value, String> {
    let mut err = ptr::null_mut();
    json_of(
        unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) },
        err,
    )
}

/// The stored composite of the Bitmap PSD at `path`, after its report.
fn open_psd(path: &Path) -> Result<*mut PrecisionDocument, String> {
    let fingerprint = report(path)?["source_fingerprint"]
        .as_str()
        .unwrap()
        .to_string();
    let fingerprint = CString::new(fingerprint).unwrap();
    let working = CString::new("gray_gamma_22").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_open_bitmap_psd(
            cpath(path).as_ptr(),
            fingerprint.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn export_report(doc: *const PrecisionDocument, container: i32, mode: i32) -> Value {
    let mut err = ptr::null_mut();
    json_of(
        unsafe { dr_precision_doc_psd_export_report(doc, container, mode, &mut err) },
        err,
    )
    .unwrap()
}

fn save(
    doc: *const PrecisionDocument,
    path: &Path,
    container: i32,
    mode: i32,
) -> Result<(), String> {
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            container,
            mode,
            LIMIT,
            0,
            &mut err,
        )
    };
    if ok {
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

#[test]
fn a_crafted_mode_zero_psd_reports_and_opens_inverted_from_the_file() {
    let dir = TempDir::new().unwrap();
    let line = (W as usize).div_ceil(8);
    // The file's 1 is black: the oracle's white bits are its 0 bits, and
    // the RLE file sets every pad bit.
    let raw_rows = pack(&oracle(), W, false, false);
    let rle_rows = pack(&oracle(), W, false, true);
    for (name, word, body) in [
        ("raw.psd", 0u16, raw_rows.clone()),
        ("rle.psd", 1, rle_body(&rle_rows, line)),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, psd(W, H, 1, Some(300), false, word, &body)).unwrap();
        let r = report(&path).unwrap();
        for (key, value) in [
            ("format", json!("PSD")),
            ("depth", json!(1)),
            ("color_mode", json!("Bitmap")),
            ("import_precision", json!("bitmap")),
            ("layer_count", json!(0)),
            ("group_count", json!(0)),
            ("layered_available", json!(false)),
            (
                "layered_refusal",
                json!("A Bitmap file holds no layers; open its stored composite."),
            ),
            ("stored_composite_available", json!(true)),
            ("stored_composite_refusal", json!(null)),
            ("profile", json!(null)),
            ("resolution", json!({"x": 300.0, "y": 300.0})),
            ("metadata_captured", json!(false)),
            ("requires_explicit_choice", json!(true)),
        ] {
            assert_eq!(r[key], value, "{name}: {key}");
        }
        for key in [
            "path",
            "source_fingerprint",
            "width",
            "height",
            "layered_note",
            "stored_composite_note",
            "verified_losses",
            "observations",
            "general_limitations",
        ] {
            assert!(r.get(key).is_some(), "{name}: {key}");
        }
        let doc = open_psd(&path).unwrap();
        let d = descriptor(doc);
        assert_eq!(d["color_mode"], json!("bitmap"), "{name}");
        assert_eq!(
            d["resolution"],
            json!({"x_ppi":300.0,"y_ppi":300.0}),
            "{name}"
        );
        assert_eq!(bits(doc), oracle(), "{name}");
        free(doc);
    }
}

#[test]
fn psd_and_psb_exports_write_the_inverted_plane_and_reopen_equal() {
    let dir = TempDir::new().unwrap();
    let doc = oracle_doc();
    let line = (W as usize).div_ceil(8);
    let inverse = pack(&oracle(), W, false, false);
    for (container, large, name) in [(0, false, "out.psd"), (1, true, "out.psb")] {
        let flattened = export_report(doc, container, 1);
        let layers = export_report(doc, container, 0);
        for r in [&flattened, &layers] {
            assert_eq!(
                (
                    r["available"].clone(),
                    r["depth"].clone(),
                    r["color_mode"].clone(),
                    r["refusal"].clone()
                ),
                (json!(true), json!(1), json!("bitmap"), json!(null)),
                "{name}"
            );
            let plan = r["working_memory_plan_bytes"].as_u64().unwrap();
            let worst = r["worst_case_bytes"].as_u64().unwrap();
            assert_eq!(
                plan,
                worst + (line * H as usize) as u64 + 1024 * 1024,
                "{name}"
            );
            assert!(
                r["losses"][0].as_str().unwrap().contains("Gray profile"),
                "{r}"
            );
        }
        assert!(layers["observations"][0]
            .as_str()
            .unwrap()
            .contains("one layer is its composite"));
        let path = dir.path().join(name);
        save(doc, &path, container, 1).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        // Both modes write the same file.
        let layered = dir.path().join(format!("layers-{name}"));
        save(doc, &layered, container, 0).unwrap();
        assert_eq!(std::fs::read(&layered).unwrap(), bytes, "{name}");
        assert!(bytes.len() as u64 <= flattened["worst_case_bytes"].as_u64().unwrap());
        let be16 = |at: usize| u16::from_be_bytes([bytes[at], bytes[at + 1]]);
        let be32 = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        assert_eq!(&bytes[..4], b"8BPS");
        assert_eq!(
            (
                be16(4),
                be16(12),
                be32(14),
                be32(18),
                be16(22),
                be16(24),
                be32(26)
            ),
            (if large { 2 } else { 1 }, 1, H, W, 1, 0, 0),
            "{name}: version, 1 channel, height, width, depth 1, mode 0, no colour data"
        );
        let resources = be32(30) as usize;
        let mut at = 34 + resources;
        let layer_section = if large {
            let v = u64::from_be_bytes(bytes[at..at + 8].try_into().unwrap());
            at += 8;
            v
        } else {
            let v = u64::from(be32(at));
            at += 4;
            v
        };
        assert_eq!(layer_section, 0, "{name}: no layer records");
        assert_eq!(be16(at), 1, "{name}: RLE");
        at += 2;
        let width = if large { 4 } else { 2 };
        let counts: Vec<usize> = (0..H as usize)
            .map(|y| {
                let p = at + y * width;
                if large {
                    be32(p) as usize
                } else {
                    usize::from(be16(p))
                }
            })
            .collect();
        at += width * H as usize;
        for (y, count) in counts.into_iter().enumerate() {
            let row = unpackbits(&bytes[at..at + count]);
            assert_eq!(
                row,
                &inverse[y * line..(y + 1) * line],
                "{name} row {y}: 1 is black"
            );
            at += count;
        }
        assert_eq!(at, bytes.len(), "{name}: nothing after the plane");
        let r = report(&path).unwrap();
        assert_eq!(
            (r["format"].clone(), r["import_precision"].clone()),
            (json!(if large { "PSB" } else { "PSD" }), json!("bitmap"))
        );
        let reopened = open_psd(&path).unwrap();
        assert_eq!(bits(reopened), oracle(), "{name} reopens equal");
        free(reopened);
    }
    free(doc);
}

#[test]
fn the_tiff_container_the_side_limit_a_depth_and_zip_refuse() {
    let dir = TempDir::new().unwrap();
    let doc = oracle_doc();
    let tiff = "A Bitmap document writes a 1-bit PSD or PSB; a TIFF with layers cannot hold it. \
                Save a Copy writes its 1-bit TIFF.";
    for mode in [0, 1] {
        let r = export_report(doc, 2, mode);
        assert_eq!(
            (r["available"].clone(), r["refusal"].clone()),
            (json!(false), json!(tiff))
        );
    }
    let path = dir.path().join("t.tif");
    assert_eq!(save(doc, &path, 2, 1).unwrap_err(), tiff);
    assert!(!path.exists());
    free(doc);

    // 30001 × 1: past a classic PSD's side, inside a PSB's.
    let codes: Vec<u8> = (0..30001u32)
        .map(|x| if x % 3 == 0 { 255 } else { 0 })
        .collect();
    let gray = gray_doc(30001, 1, &codes);
    let wide = to_bitmap(gray, &json!({"method":"threshold"})).unwrap();
    free(gray);
    let side = "This canvas is 30001 x 1 pixels; the classic PSD format stores at most 30000 \
                pixels per side. Export it as a Large Document (PSB), which stores up to 300000.";
    assert_eq!(export_report(wide, 0, 1)["refusal"], json!(side));
    let refused = dir.path().join("wide.psd");
    assert_eq!(save(wide, &refused, 0, 1).unwrap_err(), side);
    assert!(!refused.exists());
    assert_eq!(export_report(wide, 1, 1)["refusal"], json!(null));
    let psb = dir.path().join("wide.psb");
    save(wide, &psb, 1, 1).unwrap();
    let reopened = open_psd(&psb).unwrap();
    assert_eq!(unsafe { &*reopened }.dimensions(), (30001, 1));
    assert_eq!(bits(reopened), bits(wide));
    free(reopened);
    free(wide);

    // A depth other than 1, and a ZIP composite.
    let rows = pack(&oracle(), W, false, false);
    for (name, depth, word, sentence) in [
        (
            "d8.psd",
            8u16,
            0u16,
            "a Bitmap PSD stores 1 bit a pixel; this file declares 8",
        ),
        (
            "zip.psd",
            1,
            2,
            "a Bitmap PSD's composite is Raw or RLE; this file's uses ZIP",
        ),
    ] {
        let path = dir.path().join(name);
        let body = if depth == 1 {
            rows.clone()
        } else {
            vec![0; W as usize * H as usize]
        };
        std::fs::write(&path, psd(W, H, depth, None, false, word, &body)).unwrap();
        let r = report(&path).unwrap();
        assert_eq!(
            (
                r["stored_composite_available"].clone(),
                r["stored_composite_refusal"].clone()
            ),
            (json!(false), json!(sentence)),
            "{name}"
        );
        let err = open_psd(&path).unwrap_err();
        assert!(err.contains(sentence), "{name}: {err}");
    }
}

/// The report frames the composite without decoding it and refuses what the
/// open refuses, by the open's own sentence: a raw composite one byte short,
/// and an RLE file whose middle row decodes one byte short.
#[test]
fn the_report_refuses_a_bad_composite_by_the_opens_sentence() {
    let dir = TempDir::new().unwrap();
    let line = (W as usize).div_ceil(8);
    let rows = pack(&oracle(), W, false, false);
    let short = rows[..rows.len() - 1].to_vec();
    let mut encoded: Vec<Vec<u8>> = rows.chunks_exact(line).map(packbits).collect();
    encoded[1] = packbits(&rows[line..2 * line - 1]);
    let mut bad_row = Vec::new();
    for row in &encoded {
        bad_row.extend_from_slice(&(row.len() as u16).to_be_bytes());
    }
    for row in &encoded {
        bad_row.extend_from_slice(row);
    }
    for (name, word, body, sentence) in [
        (
            "short.psd",
            0u16,
            short,
            "PSD raw composite length mismatch",
        ),
        (
            "row.psd",
            1,
            bad_row,
            "PSD RLE row has incorrect decoded width",
        ),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, psd(W, H, 1, None, false, word, &body)).unwrap();
        let r = report(&path).unwrap();
        assert_eq!(
            (
                r["stored_composite_available"].clone(),
                r["stored_composite_refusal"].clone()
            ),
            (json!(false), json!(sentence)),
            "{name}"
        );
        let err = open_psd(&path).unwrap_err();
        assert!(err.contains(sentence), "{name}: {err}");
    }
}

#[test]
fn imagemagick_reads_the_bitmap_psd() {
    if magick().is_none() {
        eprintln!("SKIP: ImageMagick (magick) is not installed");
        return;
    }
    let dir = TempDir::new().unwrap();
    let doc = oracle_doc();
    let path = dir.path().join("im.psd");
    save(doc, &path, 0, 1).unwrap();
    let expected: Vec<u8> = oracle().iter().map(|&b| b * 255).collect();
    assert_eq!(magick_gray(&path), expected);
    free(doc);
}
