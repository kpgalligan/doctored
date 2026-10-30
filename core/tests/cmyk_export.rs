//! The export workspace on a CMYK document (#547 phase 5 and round 2),
//! through the FFI: a converting export against the test's own
//! `ink_to_srgb` of the composite on paper (`common/cmyk_fixture.rs`, the
//! synthetic press's tables and the built-in sRGB's own tags); a default
//! kept-CMYK TIFF against `dr_precision_doc_save_cmyk`'s own bytes and a
//! kept-CMYK JPEG against its intermediate, pending the host; the kept
//! branch's stages against independent oracles written here (the
//! nearest-neighbour index map and a linear ramp's exact value for the
//! resample, an f64 Unsharp Mask on `L = 1 − ink` for the sharpen, `round(v /
//! 257)` and ×257 for the depth), the packets read back with the `tiff`
//! crate under a metadata policy, the plan's terms, and both refusals of the
//! kept branch by their sentences.

use std::ffi::{CStr, CString};
use std::io::Cursor;
use std::ptr;

use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::dr_precision_doc_save_cmyk;
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_export_result_info, dr_export_result_write,
    dr_precision_doc_export_encode, dr_precision_doc_export_plan,
};
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_open};
use doctored_core::ffi_precision_metadata::dr_precision_doc_set_metadata;
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::Value;
use tempfile::TempDir;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;
use tiff::ColorType;

use crate::cmyk_fixture::{ink_to_srgb, press_profile};
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::metadata_fixtures::{build_exif, iptc_run};
use crate::common::{cpath, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

fn cmyk_doc(dir: &TempDir, w: u32, h: u32, samples: &[[u16; 4]]) -> *mut PrecisionDocument {
    let data: Vec<u8> = samples
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let path = dir.path().join("export.tif");
    std::fs::write(
        &path,
        cmyk_tiff(w, h, 16, 4, 0, &data, Some(&press_profile("Press A"))),
    )
    .unwrap();
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

fn ink_grid() -> Vec<[u16; 4]> {
    let level = |i: u16| i * 21845;
    let mut out = Vec::new();
    for c in 0..4u16 {
        for m in 0..4u16 {
            for y in 0..4u16 {
                for k in 0..4u16 {
                    out.push([level(c), level(m), level(y), level(k)]);
                }
            }
        }
    }
    out
}

fn export(doc: *const PrecisionDocument, request: &str) -> Result<*mut ExportResult, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_export_encode(
            doc,
            ptr::null(),
            text.as_ptr(),
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

fn bytes(result: *const ExportResult) -> Vec<u8> {
    let mut length = 0usize;
    let pointer = unsafe { dr_export_result_bytes(result, &mut length) };
    unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec()
}

fn info(result: *const ExportResult) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_export_result_info(result, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

#[test]
fn a_converting_export_is_the_composite_on_paper_in_srgb() {
    let dir = TempDir::new().unwrap();
    let grid = ink_grid();
    let doc = cmyk_doc(&dir, 16, 16, &grid);
    for profile in ["srgb", "document"] {
        for bpc in [false, true] {
            let request = format!(
                r#"{{"format":"png","profile":"{profile}","black_point_compensation":{bpc}}}"#
            );
            let result = export(doc, &request).unwrap();
            let report = info(result);
            assert_eq!(report["profile"], profile);
            assert_eq!(report["bit_depth"], 8);
            assert!(report.get("cmyk").is_none());
            let decoded = image::load_from_memory(&bytes(result)).unwrap().to_rgba8();
            assert_eq!(decoded.dimensions(), (16, 16));
            for (ink, got) in grid.iter().zip(decoded.pixels()) {
                let expected = ink_to_srgb(ink.map(|v| f64::from(v) / 65535.0), bpc)
                    .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
                for c in 0..3 {
                    assert!(
                        got[c].abs_diff(expected[c]) <= 1,
                        "{request} {ink:?}: channel {c} {} vs {}",
                        got[c],
                        expected[c]
                    );
                }
                assert_eq!(got[3], 255, "flattened on paper");
            }
            unsafe { dr_export_result_free(result) };
        }
    }
    // The plan covers the conversion beside the stages.
    let mut err = ptr::null_mut();
    let text = unsafe {
        dr_precision_doc_export_plan(doc, ptr::null(), cr#"{"format":"png"}"#.as_ptr(), &mut err)
    };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let plan: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    assert!(plan["scratch_bytes"].as_u64().unwrap() > 256 * 12);
    assert_eq!(plan["width"], 16);
    unsafe { dr_precision_doc_free(doc) };
}

fn save(doc: *const PrecisionDocument, path: &std::path::Path, request: &str) -> Vec<u8> {
    let c = cpath(path);
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_cmyk(
                doc,
                c.as_ptr(),
                text.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        },
        "{}",
        take_err_string(err)
    );
    std::fs::read(path).unwrap()
}

#[test]
fn a_kept_cmyk_export_is_the_cmyk_saves_bytes() {
    let dir = TempDir::new().unwrap();
    let doc = cmyk_doc(&dir, 16, 16, &ink_grid());
    let tiff = export(doc, r#"{"format":"tiff","bit_depth":16}"#).unwrap();
    assert_eq!(
        bytes(tiff),
        save(doc, &dir.path().join("saved.tif"), r#"{"format":"tiff"}"#)
    );
    let report = info(tiff);
    assert_eq!(report["cmyk"], true);
    assert_eq!(report["bit_depth"], 16);
    assert_eq!(report["profile"], "document");
    assert_eq!(report["profile_name"], "Press A");
    assert_eq!(report["metadata"]["profile"], true);
    // Without the profile, the save without it.
    let bare = export(
        doc,
        r#"{"format":"tiff","bit_depth":16,"embed_profile":false}"#,
    )
    .unwrap();
    assert_eq!(
        bytes(bare),
        save(
            doc,
            &dir.path().join("bare.tif"),
            r#"{"format":"tiff","embed_profile":false}"#
        )
    );
    assert_eq!(info(bare)["metadata"]["profile"], false);
    // A JPEG is the CMYK intermediate, pending the host's transcode.
    let jpeg = export(doc, r#"{"format":"jpeg"}"#).unwrap();
    assert_eq!(
        bytes(jpeg),
        save(
            doc,
            &dir.path().join("intermediate.tif"),
            r#"{"format":"jpeg_intermediate"}"#
        )
    );
    let report = info(jpeg);
    assert_eq!(report["cmyk"], true);
    assert_eq!(report["intermediate"], "tiff");
    assert_eq!(report["format"], "jpeg");
    let target = cpath(&dir.path().join("never.jpg"));
    let mut err = ptr::null_mut();
    assert!(!unsafe { dr_export_result_write(jpeg, target.as_ptr(), false, &mut err) });
    assert_eq!(
        take_err_string(err),
        "a host-encoded export must adopt the host encoder's bytes before it is written"
    );
    for result in [tiff, bare, jpeg] {
        unsafe { dr_export_result_free(result) };
    }
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn both_kept_cmyk_refusals_name_themselves() {
    let dir = TempDir::new().unwrap();
    let doc = cmyk_doc(&dir, 4, 4, &[[100, 200, 300, 400]; 16]);
    assert_eq!(
        export(doc, r#"{"format":"tiff","bit_depth":32}"#).unwrap_err(),
        "a CMYK TIFF is written at 8 or 16 bits; set bit_depth to 8 or 16"
    );
    assert_eq!(
        export(doc, r#"{"format":"tiff","bit_depth":8,"dither":"ordered"}"#).unwrap_err(),
        "a CMYK TIFF that keeps CMYK rounds 16 bits to 8 without dithering; set dither to none"
    );
    // A kept JPEG ignores dither as round 1 does: the same bytes as none.
    let plain = export(doc, r#"{"format":"jpeg"}"#).unwrap();
    let dithered = export(doc, r#"{"format":"jpeg","dither":"ordered"}"#).unwrap();
    assert_eq!(bytes(plain), bytes(dithered));
    // So does an 8-bit TIFF of an 8-bit document.
    let eight = cmyk_doc_bits(&dir, "eight.tif", 4, 4, 8, &[[10, 20, 30, 40]; 16]);
    let none = export(eight, r#"{"format":"tiff","bit_depth":8}"#).unwrap();
    let ordered = export(
        eight,
        r#"{"format":"tiff","bit_depth":8,"dither":"ordered"}"#,
    )
    .unwrap();
    assert_eq!(bytes(none), bytes(ordered));
    // The same request to sRGB converts, and resizes.
    let converted = export(
        doc,
        r#"{"format":"jpeg","profile":"srgb","size":{"mode":"width","value":2}}"#,
    )
    .unwrap();
    assert_eq!(info(converted)["width"], 2);
    for result in [plain, dithered, none, ordered, converted] {
        unsafe { dr_export_result_free(result) };
    }
    unsafe { dr_precision_doc_free(eight) };
    unsafe { dr_precision_doc_free(doc) };
}

// ---------------------------------------------------------- round 2 helpers --

/// A one-layer CMYK document from a `bits`-deep CMYK TIFF of `samples` (at
/// that depth) in the synthetic press.
fn cmyk_doc_bits(
    dir: &TempDir,
    name: &str,
    w: u32,
    h: u32,
    bits: u16,
    samples: &[[u16; 4]],
) -> *mut PrecisionDocument {
    let data: Vec<u8> = samples
        .iter()
        .flatten()
        .flat_map(|v| {
            if bits == 8 {
                vec![*v as u8]
            } else {
                v.to_le_bytes().to_vec()
            }
        })
        .collect();
    let path = dir.path().join(name);
    std::fs::write(
        &path,
        cmyk_tiff(w, h, bits, 4, 0, &data, Some(&press_profile("Press A"))),
    )
    .unwrap();
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

/// Pseudo-random 16-bit inks (a multiplicative hash, nothing smooth).
fn scattered(w: u32, h: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            std::array::from_fn(|c| {
                (i.wrapping_mul(2_654_435_761)
                    .wrapping_add(c as u32 * 40_503)
                    .rotate_left(7 + c as u32)
                    % 65536) as u16
            })
        })
        .collect()
}

/// A kept-CMYK TIFF read back with the `tiff` crate: its dimensions, its
/// bits, its interleaved CMYK samples and its embedded profile.
struct Read {
    dimensions: (u32, u32),
    bits: u8,
    samples: Vec<u16>,
    icc: Option<Vec<u8>>,
}

fn read_tiff(bytes: &[u8]) -> Read {
    let mut decoder = Decoder::new(Cursor::new(bytes)).unwrap();
    let bits = match decoder.colortype().unwrap() {
        ColorType::CMYK(bits) => bits,
        other => panic!("not CMYK: {other:?}"),
    };
    let dimensions = decoder.dimensions().unwrap();
    let icc = decoder.get_tag_u8_vec(Tag::Unknown(34675)).ok();
    let samples = match decoder.read_image().unwrap() {
        DecodingResult::U8(v) => v.into_iter().map(u16::from).collect(),
        DecodingResult::U16(v) => v,
        _ => panic!("unexpected sample type"),
    };
    Read {
        dimensions,
        bits,
        samples,
        icc,
    }
}

fn plan(doc: *const PrecisionDocument, request: &str) -> Value {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_export_plan(doc, ptr::null(), text.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    value
}

/// The kept TIFF `request` writes, read back, with the result's report.
fn kept(doc: *const PrecisionDocument, request: &str) -> (Read, Value, Vec<u8>) {
    let result = export(doc, request).unwrap_or_else(|e| panic!("{request}: {e}"));
    let encoded = bytes(result);
    let report = info(result);
    unsafe { dr_export_result_free(result) };
    (read_tiff(&encoded), report, encoded)
}

// ---------------------------------------------------------------- resize --

#[test]
fn a_kept_resize_is_the_cmyk_resampler_on_ink() {
    let dir = TempDir::new().unwrap();
    // Nearest: 32 × 24 → 16 × 12 is the exact pixel-centre index map.
    let ink = scattered(32, 24);
    let doc = cmyk_doc(&dir, 32, 24, &ink);
    let request =
        r#"{"format":"tiff","bit_depth":16,"filter":"nearest","size":{"mode":"width","value":16}}"#;
    let (read, report, _) = kept(doc, request);
    assert_eq!(read.dimensions, (16, 12));
    assert_eq!(
        (report["width"].clone(), report["height"].clone()),
        (16.into(), 12.into())
    );
    let planned = plan(doc, request);
    assert_eq!(
        (planned["width"].clone(), planned["height"].clone()),
        (16.into(), 12.into())
    );
    for y in 0..12u32 {
        for x in 0..16u32 {
            let (sx, sy) = ((2 * x + 1) * 32 / 32, (2 * y + 1) * 24 / 24);
            let at = ((y * 16 + x) * 4) as usize;
            assert_eq!(
                &read.samples[at..at + 4],
                &ink[(sy * 32 + sx) as usize][..],
                "({x}, {y})"
            );
        }
    }
    unsafe { dr_precision_doc_free(doc) };

    // Every kernel reproduces a per-plate linear ramp at the output centre's
    // source position, away from the edges. The slopes stay at most 60 codes
    // a pixel: Lanczos3 is not exactly linear-precise (its normalized taps
    // keep a small first moment at a fractional phase), so a steep ramp
    // would read that kernel property, not the resample, against ±2 codes.
    let (w, h) = (64u32, 48u32);
    let ramp = |x: f64, y: f64| -> [f64; 4] {
        [
            2000.0 + 60.0 * x,
            3000.0 + 50.0 * y,
            1000.0 + 40.0 * x + 30.0 * y,
            60000.0 - 50.0 * x,
        ]
    };
    let ink: Vec<[u16; 4]> = (0..w * h)
        .map(|i| ramp(f64::from(i % w), f64::from(i / w)).map(|v| v as u16))
        .collect();
    let doc = cmyk_doc(&dir, w, h, &ink);
    const MARGIN: u32 = 6;
    for filter in ["bilinear", "catmull-rom", "lanczos3"] {
        for (size, out) in [
            (r#"{"mode":"width","value":32}"#, (32u32, 24u32)),
            (r#"{"mode":"scale","value":1.5}"#, (96, 72)),
        ] {
            let request =
                format!(r#"{{"format":"tiff","bit_depth":16,"filter":"{filter}","size":{size}}}"#);
            let (read, report, _) = kept(doc, &request);
            assert_eq!(read.dimensions, out, "{request}");
            assert_eq!(report["width"], out.0);
            assert_eq!(report["height"], out.1);
            let planned = plan(doc, &request);
            assert_eq!(planned["width"], out.0);
            assert_eq!(planned["height"], out.1);
            for y in MARGIN..out.1 - MARGIN {
                for x in MARGIN..out.0 - MARGIN {
                    let sx = (f64::from(x) + 0.5) * f64::from(w) / f64::from(out.0) - 0.5;
                    let sy = (f64::from(y) + 0.5) * f64::from(h) / f64::from(out.1) - 0.5;
                    let expected = ramp(sx, sy);
                    let at = ((y * out.0 + x) * 4) as usize;
                    for (c, want) in expected.iter().enumerate() {
                        let got = f64::from(read.samples[at + c]);
                        assert!(
                            (got - want.round()).abs() <= 2.0,
                            "{request} ({x}, {y}) plate {c}: {got} vs {want}"
                        );
                    }
                }
            }
        }
    }
    unsafe { dr_precision_doc_free(doc) };
}

// --------------------------------------------------------------- sharpen --

/// The f64 Unsharp Mask oracle on one row of `L = 1 − ink` (constant down
/// each column): `L + amount·(L − G_σ*L)`, a normalized Gaussian of σ =
/// `radius` truncated at 4σ, the row's ends held, clamped to [0, 1].
fn unsharp_row(ink: &[u16], radius: f64, amount: f64) -> Vec<u16> {
    let l: Vec<f64> = ink.iter().map(|v| 1.0 - f64::from(*v) / 65535.0).collect();
    let reach = (4.0 * radius).ceil() as i64;
    let weights: Vec<f64> = (-reach..=reach)
        .map(|k| (-((k * k) as f64) / (2.0 * radius * radius)).exp())
        .collect();
    let total: f64 = weights.iter().sum();
    let n = l.len() as i64;
    (0..n)
        .map(|x| {
            let blurred: f64 = (-reach..=reach)
                .zip(&weights)
                .map(|(k, w)| w * l[(x + k).clamp(0, n - 1) as usize])
                .sum::<f64>()
                / total;
            let i = x as usize;
            let sharpened = (l[i] + amount * (l[i] - blurred)).clamp(0.0, 1.0);
            ((1.0 - sharpened) * 65535.0).round() as u16
        })
        .collect()
}

#[test]
fn a_kept_sharpen_is_unsharp_mask_on_l_per_plate() {
    let dir = TempDir::new().unwrap();
    // A C/K step edge down the middle; M and Y flat.
    let (w, h) = (64u32, 16u32);
    let column = |x: u32| -> [u16; 4] {
        if x < w / 2 {
            [13107, 6553, 3276, 39321]
        } else {
            [45875, 6553, 3276, 6553]
        }
    };
    let ink: Vec<[u16; 4]> = (0..w * h).map(|i| column(i % w)).collect();
    let doc = cmyk_doc(&dir, w, h, &ink);
    let (radius, amount) = (1.5, 1.0);
    let (read, _, _) = kept(
        doc,
        r#"{"format":"tiff","bit_depth":16,"sharpen":{"radius":1.5,"amount":1.0,"threshold":0.0}}"#,
    );
    let plates: Vec<Vec<u16>> = (0..4)
        .map(|c| {
            unsharp_row(
                &(0..w).map(|x| column(x)[c]).collect::<Vec<_>>(),
                radius,
                amount,
            )
        })
        .collect();
    for y in 0..h {
        for x in 0..w {
            for (c, plate) in plates.iter().enumerate() {
                let got = read.samples[((y * w + x) * 4) as usize + c];
                assert!(
                    got.abs_diff(plate[x as usize]) <= 257,
                    "({x}, {y}) plate {c}: {got} vs {}",
                    plate[x as usize]
                );
            }
        }
    }
    let edge = ((h / 2 * w + w / 2) * 4) as usize;
    assert_ne!(
        read.samples[edge],
        column(w / 2)[0],
        "the edge column is sharpened"
    );
    unsafe { dr_precision_doc_free(doc) };

    // A uniform field comes out unchanged.
    let flat = cmyk_doc_bits(
        &dir,
        "flat.tif",
        32,
        16,
        16,
        &[[10000, 20000, 30000, 40000]; 512],
    );
    let (plain, _, _) = kept(flat, r#"{"format":"tiff","bit_depth":16}"#);
    let (sharpened, _, _) = kept(
        flat,
        r#"{"format":"tiff","bit_depth":16,"sharpen":{"radius":2.0,"amount":1.0,"threshold":0.0}}"#,
    );
    assert_eq!(sharpened.samples, plain.samples);
    unsafe { dr_precision_doc_free(flat) };
}

#[test]
fn a_kept_smart_sharpen_is_the_cmyk_filter_then_exported() {
    let dir = TempDir::new().unwrap();
    let doc = cmyk_doc(&dir, 24, 16, &scattered(24, 16));
    let (sharpened, _, _) = kept(
        doc,
        r#"{"format":"tiff","bit_depth":16,"sharpen":{"method":"smart_sharpen"}}"#,
    );
    let request = CString::new(r#"{"layer":0,"op":"smart_sharpen","params":{}}"#).unwrap();
    let mut err = ptr::null_mut();
    let filtered = unsafe {
        dr_precision_doc_selected_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!filtered.is_null(), "{}", take_err_string(err));
    let (then_exported, _, _) = kept(filtered, r#"{"format":"tiff","bit_depth":16}"#);
    assert_eq!(sharpened.samples, then_exported.samples);
    unsafe { dr_precision_doc_free(filtered) };
    unsafe { dr_precision_doc_free(doc) };
}

// ----------------------------------------------------------------- depth --

#[test]
fn a_kept_tiff_changes_depth_on_the_saves_grid() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press A");
    let doc = cmyk_doc(&dir, 16, 12, &scattered(16, 12));
    let (sixteen, report16, _) = kept(doc, r#"{"format":"tiff","bit_depth":16}"#);
    let (eight, report8, _) = kept(doc, r#"{"format":"tiff","bit_depth":8}"#);
    assert_eq!((sixteen.bits, eight.bits), (16, 8));
    assert_eq!(
        (report16["bit_depth"].clone(), report8["bit_depth"].clone()),
        (16.into(), 8.into())
    );
    for read in [&sixteen, &eight] {
        assert_eq!(
            read.icc.as_deref(),
            Some(&profile[..]),
            "carries the profile"
        );
    }
    let rounded: Vec<u16> = sixteen
        .samples
        .iter()
        .map(|v| (f64::from(*v) / 257.0).round() as u16)
        .collect();
    assert_eq!(eight.samples, rounded);
    unsafe { dr_precision_doc_free(doc) };

    // An 8-bit document's 16-bit TIFF is its 8-bit samples ×257.
    let small: Vec<[u16; 4]> = scattered(16, 12)
        .into_iter()
        .map(|p| p.map(|v| v % 256))
        .collect();
    let doc = cmyk_doc_bits(&dir, "eight.tif", 16, 12, 8, &small);
    let (eight, _, _) = kept(doc, r#"{"format":"tiff","bit_depth":8}"#);
    let (sixteen, report, _) = kept(doc, r#"{"format":"tiff","bit_depth":16}"#);
    assert_eq!((eight.bits, sixteen.bits), (8, 16));
    assert_eq!(report["bit_depth"], 16);
    assert_eq!(
        sixteen.samples,
        eight.samples.iter().map(|v| v * 257).collect::<Vec<_>>()
    );
    assert_eq!(sixteen.icc.as_deref(), Some(&profile[..]));
    unsafe { dr_precision_doc_free(doc) };
}

// -------------------------------------------------------------- metadata --

/// An XMP packet with a description the stamp can land in.
const XMP: &[u8] = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description rdf:about='' xmlns:dc='http://purl.org/dc/elements/1.1/'></rdf:Description></rdf:RDF></x:xmpmeta>";

fn with_packet(doc: *mut PrecisionDocument, kind: i32, packet: &[u8]) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_set_metadata(doc, kind, packet.as_ptr(), packet.len(), &mut err)
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    unsafe { dr_precision_doc_free(doc) };
    out
}

/// What a CMYK TIFF's tags carry: ExifIFD, the XMP text, the IPTC resource.
fn packets(bytes: &[u8]) -> (bool, Option<String>, bool, bool) {
    let mut decoder = Decoder::new(Cursor::new(bytes)).unwrap();
    let exif = decoder.find_tag(Tag::Unknown(34665)).unwrap().is_some();
    let xmp = decoder
        .get_tag_u8_vec(Tag::Unknown(700))
        .ok()
        .map(|v| String::from_utf8_lossy(&v).into_owned());
    let iptc = decoder.find_tag(Tag::Unknown(34377)).unwrap().is_some();
    let icc = decoder.find_tag(Tag::Unknown(34675)).unwrap().is_some();
    (exif, xmp, iptc, icc)
}

#[test]
fn a_kept_export_follows_the_metadata_policy() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (16u32, 12u32);
    let doc = cmyk_doc(&dir, w, h, &scattered(w, h));
    let exif = build_exif(false, 1, (72, 72), Some((w, h)), false).bytes;
    let doc = with_packet(doc, 0, &exif);
    let doc = with_packet(doc, 1, XMP);
    let doc = with_packet(doc, 2, &iptc_run());
    // The fixture carries all three: the default policy writes them.
    let (_, report, encoded) = kept(doc, r#"{"format":"tiff","bit_depth":16}"#);
    assert!(packets(&encoded).0, "the default keeps ExifIFD");
    assert!(packets(&encoded).1.is_some() && packets(&encoded).2);
    assert_eq!(report["metadata"]["exif"], true);
    const POLICY: &str = r#""metadata":{"exif":"drop","xmp":"keep","iptc":"keep","stamp":{"copyright":"(c) 2026 Press"}}"#;
    for (format, embed) in [("tiff", true), ("tiff", false), ("jpeg", true)] {
        let request = format!(
            r#"{{"format":"{format}","bit_depth":{},"embed_profile":{embed},{POLICY}}}"#,
            if format == "tiff" { 16 } else { 8 }
        );
        let result = export(doc, &request).unwrap_or_else(|e| panic!("{request}: {e}"));
        let encoded = bytes(result);
        let report = info(result);
        unsafe { dr_export_result_free(result) };
        let (exif, xmp, iptc, icc) = packets(&encoded);
        assert!(!exif, "{request}: no ExifIFD");
        let xmp = xmp.unwrap_or_else(|| panic!("{request}: no XMP"));
        assert!(
            xmp.contains("dc:rights") && xmp.contains("(c) 2026 Press"),
            "{xmp}"
        );
        assert!(iptc, "{request}: IPTC");
        assert_eq!(icc, embed, "{request}: ICC follows embed_profile");
        let outcome = &report["metadata"];
        assert_eq!(outcome["exif"], exif, "{request}");
        assert_eq!(outcome["xmp"], true, "{request}");
        assert_eq!(outcome["iptc"], iptc, "{request}");
        assert_eq!(outcome["profile"], icc, "{request}");
        assert_eq!(outcome["stamped"], true, "{request}");
        if format == "jpeg" {
            assert_eq!(report["intermediate"], "tiff");
        }
    }
    unsafe { dr_precision_doc_free(doc) };
}

// ------------------------------------------------------------------ plan --

#[test]
fn the_kept_plan_charges_its_stages() {
    let dir = TempDir::new().unwrap();
    // Large enough that the per-pixel terms, not the fixed ones (the
    // Gaussian's own plan, bookkeeping), order the three plans.
    let doc = cmyk_doc(&dir, 1024, 768, &scattered(1024, 768));
    let scratch = |request: &str| plan(doc, request)["scratch_bytes"].as_u64().unwrap();
    let unstaged = scratch(r#"{"format":"tiff","bit_depth":16}"#);
    let scaled = scratch(r#"{"format":"tiff","bit_depth":16,"size":{"mode":"scale","value":1.5}}"#);
    let sharpened = scratch(
        r#"{"format":"tiff","bit_depth":16,"sharpen":{"radius":1.5,"amount":1.0,"threshold":0.0}}"#,
    );
    assert!(scaled >= unstaged, "{scaled} vs {unstaged}");
    assert!(sharpened >= unstaged, "{sharpened} vs {unstaged}");
    assert!(scaled > sharpened, "{scaled} vs {sharpened}");
    let scaled_plan = plan(
        doc,
        r#"{"format":"tiff","bit_depth":16,"size":{"mode":"scale","value":1.5}}"#,
    );
    assert_eq!(
        (scaled_plan["width"].clone(), scaled_plan["height"].clone()),
        (1536.into(), 1152.into())
    );
    let down = plan(
        doc,
        r#"{"format":"jpeg","size":{"mode":"width","value":512}}"#,
    );
    assert_eq!(
        (down["width"].clone(), down["height"].clone()),
        (512.into(), 384.into())
    );
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn the_staged_plan_is_its_largest_stage_plus_the_result() {
    // The staged plan restated from its terms (cmyk_export_ink's module
    // doc): ink is 10 B/px, the samples 8 B/px, the encode 24 B/px, the
    // result 16 B/px, an adoption copy 64 B per 256-pixel tile. The stack
    // term (stack + one row + bookkeeping) is read off the unstaged plan,
    // which is the save's (24 B/px + that term) plus 16 B/px.
    let dir = TempDir::new().unwrap();
    let (w, h) = (1024u64, 768u64);
    let doc = cmyk_doc(&dir, w as u32, h as u32, &scattered(w as u32, h as u32));
    let scratch = |request: &str| plan(doc, request)["scratch_bytes"].as_u64().unwrap();
    let c = w * h;
    let stack = scratch(r#"{"format":"tiff","bit_depth":16}"#) - c * 40;
    let staged = |ow: u64, oh: u64| {
        let o = ow * oh;
        let tiles = ow.div_ceil(256) * oh.div_ceil(256);
        let on_paper = stack + c * 10;
        // Nearest has no separable temporary, so the adoption copy is the
        // larger of the two.
        let resize = c * 10 + c * 10 + o * 10 + (o * 10 + tiles * 64);
        let samples = o * 10 + o * 8;
        let encode = o * 8 + o * 24;
        on_paper.max(resize).max(samples).max(encode) + o * 16
    };
    let down = scratch(
        r#"{"format":"tiff","bit_depth":16,"filter":"nearest","size":{"mode":"width","value":512}}"#,
    );
    assert_eq!(down, staged(512, 384));
    let up = scratch(
        r#"{"format":"tiff","bit_depth":16,"filter":"nearest","size":{"mode":"scale","value":1.5}}"#,
    );
    assert_eq!(up, staged(1536, 1152));
    // The sum of the stages (the old plan) is strictly larger than the max.
    let sum_down = stack + c * 10 + (c * 10 + 512 * 384 * 20 + 4 * 64) + 512 * 384 * (24 + 16);
    assert!(down < sum_down, "{down} vs {sum_down}");
    unsafe { dr_precision_doc_free(doc) };
}
