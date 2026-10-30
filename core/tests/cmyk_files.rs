//! CMYK files as CMYK documents (#1 phase 1): the TIFF, JPEG and PSD
//! opens keep the file's own ink and embedded profile; the TIFF, PSD and
//! `.rz` saves are bit-exact at 8 and 16 bits, each re-read by an
//! independent reader — the `tiff` crate for TIFF, a PSD parser written here
//! from Adobe's file-format specification, the typed native reader for `.rz`
//! — and the CMYK PSD round trip goes through the same report and the opens
//! the host uses (since #1 phase 3 Import Layers too). ImageMagick-authored
//! fixtures skip when it is absent. No test here sets the working CMYK
//! profile.

use std::ffi::CString;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::*;
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_open, dr_precision_doc_save_native,
};
use doctored_core::ffi_psd::{dr_precision_doc_open_psd, dr_psd_import_report};
use doctored_core::precision_document::PrecisionDocument;
use tempfile::TempDir;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;
use tiff::ColorType;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_jpeg_builder::{cmyk_jpeg, CmykJpeg};
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{cpath, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

fn open(path: &Path) -> *mut PrecisionDocument {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "open {path:?}: {}", take_err_string(err));
    doc
}

fn samples(doc: *const PrecisionDocument) -> Vec<[u16; 5]> {
    unsafe { &*doc }
        .cmyk_layer_samples(0)
        .expect("a CMYK record")
}

fn profile_of(doc: *const PrecisionDocument) -> Vec<u8> {
    let mut length = 0usize;
    let bytes = unsafe { dr_precision_doc_cmyk_profile(doc, &mut length) };
    assert!(!bytes.is_null());
    unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec()
}

fn info(doc: *const PrecisionDocument) -> serde_json::Value {
    let text = unsafe { dr_precision_doc_cmyk_info(doc) };
    assert!(!text.is_null());
    let value =
        serde_json::from_str(unsafe { std::ffi::CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn save(doc: *const PrecisionDocument, path: &Path, request: &str) {
    let c = cpath(path);
    let request = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_cmyk(
                doc,
                c.as_ptr(),
                request.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        },
        "save {path:?}: {}",
        take_err_string(err)
    );
}

/// A deterministic CMYK pattern at `bits`: every ink distinct per pixel.
fn pattern(w: u32, h: u32, bits: u16) -> Vec<[u16; 4]> {
    let max = if bits == 8 { 255u32 } else { 65535 };
    (0..w * h)
        .map(|i| std::array::from_fn(|c| ((i * 7919 + c as u32 * 104729) % (max + 1)) as u16))
        .collect()
}

fn tiff_bytes(pattern: &[[u16; 4]], w: u32, h: u32, bits: u16, icc: Option<&[u8]>) -> Vec<u8> {
    let data: Vec<u8> = if bits == 8 {
        pattern.iter().flatten().map(|v| *v as u8).collect()
    } else {
        pattern
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect()
    };
    cmyk_tiff(w, h, bits, 4, 0, &data, icc)
}

fn stored(pattern: &[[u16; 4]], bits: u16) -> Vec<[u16; 5]> {
    let scale = if bits == 8 { 257 } else { 1 };
    pattern
        .iter()
        .map(|p| {
            [
                p[0] * scale,
                p[1] * scale,
                p[2] * scale,
                p[3] * scale,
                65535,
            ]
        })
        .collect()
}

#[test]
fn cmyk_tiff_opens_as_cmyk_and_saves_bit_exact_at_both_depths() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Test");
    for bits in [8u16, 16] {
        let (w, h) = (37u32, 11u32);
        let pattern = pattern(w, h, bits);
        let source = dir.path().join(format!("in{bits}.tif"));
        std::fs::write(&source, tiff_bytes(&pattern, w, h, bits, Some(&profile))).unwrap();
        assert_eq!(unsafe { dr_path_cmyk_source(cpath(&source).as_ptr()) }, 1);
        let doc = open(&source);
        assert_eq!(samples(doc), stored(&pattern, bits));
        assert_eq!(profile_of(doc), profile);
        assert_eq!(info(doc)["depth"], u32::from(bits));
        assert_eq!(info(doc)["profile"], "Press Test");
        let out = dir.path().join(format!("out{bits}.tif"));
        save(
            doc,
            &out,
            r#"{"format":"tiff","embed_profile":true,"strip_metadata":false}"#,
        );
        // The `tiff` crate re-reads it: CMYK at the declared depth, the same
        // ink, the profile embedded.
        let bytes = std::fs::read(&out).unwrap();
        let mut decoder = Decoder::new(Cursor::new(&bytes)).unwrap();
        assert_eq!(decoder.colortype().unwrap(), ColorType::CMYK(bits as u8));
        let icc = decoder.get_tag_u8_vec(Tag::Unknown(34675)).unwrap();
        assert_eq!(icc, profile);
        let read: Vec<u16> = match decoder.read_image().unwrap() {
            DecodingResult::U8(v) => v.into_iter().map(u16::from).collect(),
            DecodingResult::U16(v) => v,
            _ => panic!("unexpected sample type"),
        };
        assert_eq!(read, pattern.iter().flatten().copied().collect::<Vec<_>>());
        // And opens again as the same document.
        let again = open(&out);
        assert_eq!(samples(again), samples(doc));
        assert_eq!(profile_of(again), profile);
        unsafe {
            dr_precision_doc_free(again);
            dr_precision_doc_free(doc);
        }
    }
}

#[test]
fn the_native_save_reopens_bit_exact_with_its_depth_and_profile() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Test");
    for bits in [8u16, 16] {
        let pattern = pattern(19, 7, bits);
        let source = dir.path().join(format!("n{bits}.tif"));
        std::fs::write(&source, tiff_bytes(&pattern, 19, 7, bits, Some(&profile))).unwrap();
        let doc = open(&source);
        let rz = dir.path().join(format!("n{bits}.rz"));
        let c = cpath(&rz);
        let mut err = ptr::null_mut();
        assert!(
            unsafe {
                dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
            },
            "{}",
            take_err_string(err)
        );
        let bytes = std::fs::read(&rz).unwrap();
        assert_eq!(&bytes[..4], b"DRDC");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 46);
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 82);
        let again = open(&rz);
        assert!(unsafe { dr_precision_doc_is_cmyk(again) });
        assert_eq!(samples(again), samples(doc));
        assert_eq!(profile_of(again), profile);
        assert_eq!(info(again)["depth"], u32::from(bits));
        assert_eq!(
            unsafe { &*again }.layer_id(0),
            unsafe { &*doc }.layer_id(0),
            "the record keeps its id"
        );
        unsafe {
            dr_precision_doc_free(again);
            dr_precision_doc_free(doc);
        }
    }
}

/// The PackBits rows of a PSD composite, decoded from Adobe's specification.
fn unpack(mut row: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some((&n, rest)) = row.split_first() {
        let n = n as i8;
        if n >= 0 {
            let len = n as usize + 1;
            out.extend_from_slice(&rest[..len]);
            row = &rest[len..];
        } else if n != -128 {
            out.extend(std::iter::repeat_n(rest[0], (1 - n as isize) as usize));
            row = &rest[1..];
        } else {
            row = rest;
        }
    }
    out
}

/// A classic PSD's header fields, its resource 1039 and its composite planes
/// (as stored), read by hand.
fn parse_psd(bytes: &[u8]) -> (u16, u16, u16, Option<Vec<u8>>, Vec<Vec<u8>>) {
    let be16 = |at: usize| u16::from_be_bytes([bytes[at], bytes[at + 1]]);
    let be32 = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    assert_eq!(&bytes[..4], b"8BPS");
    assert_eq!(be16(4), 1);
    let channels = be16(12);
    let (h, w) = (be32(14), be32(18));
    let depth = be16(22);
    let mode = be16(24);
    let mut at = 26;
    at += 4 + be32(at); // colour mode data
    let resources_len = be32(at);
    let mut r = at + 4;
    let end = r + resources_len;
    let mut icc = None;
    while r < end {
        assert_eq!(&bytes[r..r + 4], b"8BIM");
        let id = be16(r + 4);
        let name = bytes[r + 6] as usize;
        r += 6 + ((name + 2) & !1);
        let len = be32(r);
        if id == 1039 {
            icc = Some(bytes[r + 4..r + 4 + len].to_vec());
        }
        r += 4 + len + (len & 1);
    }
    at = end;
    at += 4 + be32(at); // layer and mask information
    assert_eq!(be16(at), 1, "RLE composite");
    at += 2;
    let rows = h * channels as usize;
    let counts: Vec<usize> = (0..rows).map(|i| be16(at + 2 * i) as usize).collect();
    at += 2 * rows;
    let row_bytes = w * depth as usize / 8;
    let mut planes = vec![Vec::new(); channels as usize];
    for (i, count) in counts.iter().enumerate() {
        let row = unpack(&bytes[at..at + count]);
        assert_eq!(row.len(), row_bytes);
        planes[i / h].extend_from_slice(&row);
        at += count;
    }
    (channels, depth, mode, icc, planes)
}

fn report(path: &Path) -> serde_json::Value {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let text = unsafe { dr_psd_import_report(c.as_ptr(), &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value =
        serde_json::from_str(unsafe { std::ffi::CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn open_psd(path: &Path, mode: i32) -> Result<*mut PrecisionDocument, String> {
    let fingerprint = report(path)["source_fingerprint"]
        .as_str()
        .unwrap()
        .to_string();
    let c = cpath(path);
    let fingerprint = CString::new(fingerprint).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_psd(
            c.as_ptr(),
            mode,
            fingerprint.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if doc.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(doc)
    }
}

#[test]
fn cmyk_psd_saves_flattened_and_reopens_through_the_hosts_route() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Test");
    for bits in [8u16, 16] {
        let (w, h) = (23u32, 9u32);
        let pattern = pattern(w, h, bits);
        let source = dir.path().join(format!("p{bits}.tif"));
        std::fs::write(&source, tiff_bytes(&pattern, w, h, bits, Some(&profile))).unwrap();
        let doc = open(&source);
        let out = dir.path().join(format!("p{bits}.psd"));
        save(doc, &out, r#"{"format":"psd","embed_profile":true}"#);
        let (channels, depth, mode, icc, planes) = parse_psd(&std::fs::read(&out).unwrap());
        assert_eq!((channels, depth, mode), (4, bits, 4));
        assert_eq!(icc.as_deref(), Some(&profile[..]));
        // Stored inverted, as Photoshop stores CMYK.
        let max = if bits == 8 { 255u16 } else { 65535 };
        for (c, plane) in planes.iter().enumerate() {
            let values: Vec<u16> = if bits == 8 {
                plane.iter().map(|v| u16::from(*v)).collect()
            } else {
                plane
                    .chunks(2)
                    .map(|v| u16::from_be_bytes([v[0], v[1]]))
                    .collect()
            };
            let expected: Vec<u16> = pattern.iter().map(|p| max - p[c]).collect();
            assert_eq!(values, expected, "plane {c}");
        }
        // The host's route: the report, then mode 1.
        // A PSD is the report's question, never the path query's.
        assert_eq!(unsafe { dr_path_cmyk_source(cpath(&out).as_ptr()) }, 0);
        let r = report(&out);
        assert_eq!(r["import_precision"], "cmyk");
        assert_eq!(r["color_mode"], "CMYK");
        assert_eq!(r["stored_composite_available"], true);
        // #1 phase 3: Import Layers is available; a flattened file holds
        // no record, so it opens the saved composite.
        assert_eq!(r["layered_available"], true);
        assert_eq!(r["profile"], "Press Test");
        let again = open_psd(&out, 1).unwrap();
        assert_eq!(samples(again), stored(&pattern, bits));
        assert_eq!(profile_of(again), profile);
        assert_eq!(info(again)["depth"], u32::from(bits));
        let layered = open_psd(&out, 0).unwrap();
        assert_eq!(samples(layered), stored(&pattern, bits));
        assert_eq!(profile_of(layered), profile);
        unsafe {
            dr_precision_doc_free(layered);
            dr_precision_doc_free(again);
            dr_precision_doc_free(doc);
        }
    }
}

#[test]
fn a_cmyk_jpeg_opens_with_its_own_ink_and_profile() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Test");
    // An Adobe CMYK JPEG stores inverted samples (`cmyk_jpeg`).
    let stored_samples = [200u8, 150, 100, 20];
    let bytes = cmyk_jpeg(
        16,
        8,
        stored_samples,
        &CmykJpeg {
            adobe_transform: Some(0),
            icc: Some(&profile),
            exif: None,
        },
    );
    let path = dir.path().join("ink.jpg");
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(unsafe { dr_path_cmyk_source(cpath(&path).as_ptr()) }, 2);
    // Header work only: the file cut right after its SOS header answers alike.
    let segment = |at: usize| 2 + usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
    let mut at = 2;
    while bytes[at + 1] != 0xDA {
        at += segment(at);
    }
    let cut = dir.path().join("header.jpg");
    std::fs::write(&cut, &bytes[..at + segment(at)]).unwrap();
    assert_eq!(unsafe { dr_path_cmyk_source(cpath(&cut).as_ptr()) }, 2);
    let doc = open(&path);
    let ink = stored_samples.map(|s| u16::from(255 - s) * 257);
    assert!(samples(doc)
        .iter()
        .all(|s| s == &[ink[0], ink[1], ink[2], ink[3], 65535]));
    assert_eq!(profile_of(doc), profile);
    assert_eq!(info(doc)["depth"], 8);
    // The JPEG intermediate is an 8-bit CMYK TIFF of the same ink.
    let intermediate = dir.path().join("intermediate.tif");
    save(
        doc,
        &intermediate,
        r#"{"format":"jpeg_intermediate","embed_profile":true}"#,
    );
    let mut decoder = Decoder::new(Cursor::new(std::fs::read(&intermediate).unwrap())).unwrap();
    assert_eq!(decoder.colortype().unwrap(), ColorType::CMYK(8));
    let DecodingResult::U8(read) = decoder.read_image().unwrap() else {
        panic!("8-bit samples");
    };
    assert!(read.chunks(4).all(|p| p == ink.map(|v| (v / 257) as u8)));
    // ImageMagick transcodes it to a CMYK JPEG, which reopens within 6/255
    // on the flat patch (a lossy codec's bound, declared before measuring).
    let jpeg = dir.path().join("reencoded.jpg");
    if Command::new("magick")
        .arg(&intermediate)
        .arg("-quality")
        .arg("95")
        .arg(&jpeg)
        .status()
        .is_ok_and(|s| s.success())
    {
        let again = open(&jpeg);
        for s in samples(again) {
            for c in 0..4 {
                let got = (s[c] / 257) as i32;
                let want = (ink[c] / 257) as i32;
                assert!((got - want).abs() <= 6, "ink {c}: {got} vs {want}");
            }
        }
        unsafe { dr_precision_doc_free(again) };
    }
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn an_imagemagick_cmyk_psd_opens_with_the_tiffs_ink() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Test");
    let pattern = pattern(13, 5, 8);
    let tiff = dir.path().join("im.tif");
    std::fs::write(&tiff, tiff_bytes(&pattern, 13, 5, 8, Some(&profile))).unwrap();
    let psd = dir.path().join("im.psd");
    let made = Command::new("magick")
        .arg(&tiff)
        .arg(&psd)
        .status()
        .is_ok_and(|s| s.success());
    if !made {
        eprintln!("ImageMagick is absent; skipping the ImageMagick CMYK PSD fixture");
        return;
    }
    let r = report(&psd);
    assert_eq!(r["import_precision"], "cmyk");
    let doc = open_psd(&psd, 1).unwrap();
    assert_eq!(samples(doc), stored(&pattern, 8));
    unsafe { dr_precision_doc_free(doc) };
}
