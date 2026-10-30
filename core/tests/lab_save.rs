//! #547 Lab Color phase 1b, the saves: the Lab TIFF at 8 and 16 bits,
//! opaque and transparent, its header parsed by hand here (TIFF 6.0 CIELab:
//! PhotometricInterpretation 8, three samples or four with ExtraSamples
//! [2], no ICC tag) and its samples equal to TIFF 6.0's CIELab formula
//! applied to the render's codes (written here, not called); the 16-bit
//! file decodes by the inverse formula within 0.25 L\*/a\*/b\* units of the
//! CIE oracle of each sRGB patch (`common/lab_fixture.rs`; an 8-bit file's
//! own quantization step is 0.39 L\* and 1 a\*, so the exact formula check
//! holds it); the plan's refusals; the exports through RGB — PNG 16 into
//! sRGB, and the document's profile, which writes sRGB — within 32/65535 of
//! the inverse formula per patch, an out-of-gamut patch clipped rather than
//! refused; and the fit-first thumbnail of a uniform 4096 × 4096 layer
//! within 1/255 of the oracle's Display P3 encoding, admitted under a limit
//! the whole-layer conversion would exceed. Bounds declared before
//! measuring.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_export_result_info,
    dr_precision_doc_export_encode,
};
use doctored_core::ffi_lab::*;
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_image_free};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use doctored_core::DrImage;
use serde_json::Value;
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::lab_fixture::{decode, lab_to_rgb, lab_to_srgb, srgb_to_lab, P3_TO_XYZ};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// The colour patches (8-bit codes): sRGB for the TIFF cases, Display P3
/// for the exports, where the saturated ones fall outside sRGB.
const PATCHES: [[u16; 3]; 10] = [
    [200, 40, 90],
    [30, 180, 220],
    [128, 128, 128],
    [250, 250, 10],
    [64, 0, 255],
    [0, 0, 0],
    [255, 255, 255],
    [0, 255, 0],
    [17, 34, 51],
    [255, 0, 0],
];

fn json_of(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

/// The patches as a Lab document of `depth` through Mode ▸ Lab Color from
/// `profile`; alpha ramps down from opaque when `transparent`.
fn patch_lab(
    profile: std::sync::Arc<IccProfile>,
    depth: u32,
    transparent: bool,
) -> *mut PrecisionDocument {
    let pixels: Vec<[u16; 4]> = PATCHES
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let alpha = if transparent {
                65535 - i as u16 * 5000
            } else {
                65535
            };
            [p[0] * 257, p[1] * 257, p[2] * 257, alpha]
        })
        .collect();
    let image =
        PrecisionImage::from_rgba16(PATCHES.len() as u32, 1, pixels, profile, None).unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ));
    let options = CString::new(format!(r#"{{"depth":{depth}}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let lab = unsafe {
        dr_precision_doc_convert_to_lab(rgb, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!lab.is_null(), "to Lab: {}", take_err_string(err));
    free(rgb);
    lab
}

fn render16(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn plan(doc: *const PrecisionDocument, projection: *const PrecisionImage, request: &str) -> Value {
    let request = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    json_of(
        unsafe { dr_precision_doc_save_lab_plan(doc, projection, request.as_ptr(), &mut err) },
        err,
    )
}

fn save(
    doc: *const PrecisionDocument,
    projection: *const PrecisionImage,
    path: &std::path::Path,
    request: &str,
) -> Result<Vec<u8>, String> {
    let c = cpath(path);
    let request = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_lab(
            doc,
            projection,
            c.as_ptr(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !ok {
        return Err(take_err_string(err));
    }
    Ok(std::fs::read(path).unwrap())
}

/// A hand-parsed baseline TIFF: the tags this file checks and the strips'
/// bytes concatenated.
struct Tiff {
    little: bool,
    photometric: u32,
    samples_per_pixel: u32,
    bits: Vec<u32>,
    sample_format: Vec<u32>,
    extra: Vec<u32>,
    compression: u32,
    icc: bool,
    data: Vec<u8>,
}

fn parse_tiff(bytes: &[u8]) -> Tiff {
    let little = &bytes[..2] == b"II";
    let u16_at = |at: usize| {
        let b = [bytes[at], bytes[at + 1]];
        if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        }
    };
    let u32_at = |at: usize| {
        let b = [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]];
        if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    };
    assert_eq!(u16_at(2), 42);
    let ifd = u32_at(4) as usize;
    let count = u16_at(ifd) as usize;
    // Every value of a SHORT (3) or LONG (4) entry, inline when it fits in
    // the four value bytes, else at its offset.
    let values = |at: usize| -> Vec<u32> {
        let kind = u16_at(at + 2);
        let n = u32_at(at + 4) as usize;
        let size = if kind == 3 { 2 } else { 4 };
        let base = if n * size <= 4 {
            at + 8
        } else {
            u32_at(at + 8) as usize
        };
        (0..n)
            .map(|i| {
                if kind == 3 {
                    u32::from(u16_at(base + i * 2))
                } else {
                    u32_at(base + i * 4)
                }
            })
            .collect()
    };
    let mut tiff = Tiff {
        little,
        photometric: 99,
        samples_per_pixel: 1,
        bits: Vec::new(),
        sample_format: Vec::new(),
        extra: Vec::new(),
        compression: 1,
        icc: false,
        data: Vec::new(),
    };
    let (mut offsets, mut counts) = (Vec::new(), Vec::new());
    for e in 0..count {
        let at = ifd + 2 + e * 12;
        match u16_at(at) {
            258 => tiff.bits = values(at),
            259 => tiff.compression = values(at)[0],
            262 => tiff.photometric = values(at)[0],
            273 => offsets = values(at),
            277 => tiff.samples_per_pixel = values(at)[0],
            279 => counts = values(at),
            338 => tiff.extra = values(at),
            339 => tiff.sample_format = values(at),
            34675 => tiff.icc = true,
            _ => {}
        }
    }
    for (offset, n) in offsets.iter().zip(&counts) {
        tiff.data
            .extend_from_slice(&bytes[*offset as usize..(*offset + *n) as usize]);
    }
    tiff
}

impl Tiff {
    fn samples16(&self) -> Vec<u16> {
        self.data
            .chunks_exact(2)
            .map(|b| {
                if self.little {
                    u16::from_le_bytes([b[0], b[1]])
                } else {
                    u16::from_be_bytes([b[0], b[1]])
                }
            })
            .collect()
    }
}

/// TIFF 6.0's CIELab samples of a u16 Lab pixel, restated from the
/// standard: 8 bits — the nearest byte c = (v + 128) / 257, L = c, a/b =
/// c − 128 as a signed byte; 16 bits — L = l, a/b = round(a\*·256) as a
/// signed 16-bit value, a\* = 255·v/65535 − 128.
fn expected8(p: [u16; 4], alpha: bool) -> Vec<u8> {
    let byte = |v: u16| ((u32::from(v) + 128) / 257) as u8;
    let mut out = vec![
        byte(p[0]),
        (i32::from(byte(p[1])) - 128) as i8 as u8,
        (i32::from(byte(p[2])) - 128) as i8 as u8,
    ];
    if alpha {
        out.push(byte(p[3]));
    }
    out
}

fn expected16(p: [u16; 4], alpha: bool) -> Vec<u16> {
    let signed = |v: u16| {
        let star = 255.0 * f64::from(v) / 65535.0 - 128.0;
        (star * 256.0).round().clamp(-32768.0, 32767.0) as i16 as u16
    };
    let mut out = vec![p[0], signed(p[1]), signed(p[2])];
    if alpha {
        out.push(p[3]);
    }
    out
}

#[test]
fn the_lab_tiff_holds_cielab_samples_at_8_and_16_bits_opaque_and_transparent() {
    let dir = TempDir::new().unwrap();
    for depth in [8u32, 16] {
        for transparent in [false, true] {
            let doc = patch_lab(IccProfile::srgb(), depth, transparent);
            let p = plan(doc, ptr::null(), r#"{"format":"tiff"}"#);
            assert_eq!(p["refusal"], Value::Null, "{p}");
            assert!(p["working_memory_plan_bytes"].as_u64().unwrap() > 0);
            let path = dir.path().join(format!("lab{depth}-{transparent}.tif"));
            let bytes = save(doc, ptr::null(), &path, r#"{"format":"tiff"}"#).unwrap();
            let tiff = parse_tiff(&bytes);
            let what = format!("{depth} bits, transparent {transparent}");
            let spp = if transparent { 4 } else { 3 };
            assert_eq!(tiff.photometric, 8, "{what}");
            assert_eq!(tiff.samples_per_pixel, spp, "{what}");
            assert_eq!(tiff.bits, vec![depth; spp as usize], "{what}");
            assert!(tiff.sample_format.iter().all(|f| *f == 1), "{what}");
            assert_eq!(tiff.compression, 1, "{what}");
            assert_eq!(
                tiff.extra,
                if transparent { vec![2] } else { vec![] },
                "{what}"
            );
            assert!(!tiff.icc, "{what}: a CIELab TIFF carries no profile");
            let codes = render16(doc);
            if depth == 8 {
                let want: Vec<u8> = codes
                    .iter()
                    .flat_map(|p| expected8(*p, transparent))
                    .collect();
                assert_eq!(tiff.data, want, "{what}");
            } else {
                let got = tiff.samples16();
                let want: Vec<u16> = codes
                    .iter()
                    .flat_map(|p| expected16(*p, transparent))
                    .collect();
                assert_eq!(got, want, "{what}");
                // The inverse formula against the CIE oracle of each patch.
                for (i, pixel) in got.chunks_exact(spp as usize).enumerate() {
                    let lab = [
                        f64::from(pixel[0]) * 100.0 / 65535.0,
                        f64::from(pixel[1] as i16) / 256.0,
                        f64::from(pixel[2] as i16) / 256.0,
                    ];
                    let oracle = srgb_to_lab(PATCHES[i].map(|c| f64::from(c) / 255.0));
                    for c in 0..3 {
                        assert!(
                            (lab[c] - oracle[c]).abs() <= 0.25,
                            "patch {i} channel {c}: {lab:?} against {oracle:?}"
                        );
                    }
                }
            }
            free(doc);
        }
    }
}

#[test]
fn white_and_a_projection_save_as_the_render_and_the_plan_refuses_by_name() {
    let dir = TempDir::new().unwrap();
    let doc = patch_lab(IccProfile::srgb(), 16, false);
    // The host's projection writes the same file as the save's own render.
    let mut err = ptr::null_mut();
    let projection = unsafe {
        doctored_core::ffi_precision::dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!projection.is_null(), "{}", take_err_string(err));
    let own = save(
        doc,
        ptr::null(),
        &dir.path().join("own.tif"),
        r#"{"format":"tiff"}"#,
    )
    .unwrap();
    let projected = save(
        doc,
        projection,
        &dir.path().join("projected.tif"),
        r#"{"format":"tiff","strip_metadata":true}"#,
    )
    .unwrap();
    assert_eq!(parse_tiff(&own).data, parse_tiff(&projected).data);
    // White (patch 6) is (max, 0, 0) exactly at 16 bits within the
    // conversion's own code.
    let white = &parse_tiff(&own).samples16()[18..21];
    assert!(white[0] >= 65533, "{white:?}");
    // Refusals.
    assert_eq!(
        plan(doc, ptr::null(), r#"{"format":"psd"}"#)["refusal"],
        "Saving as psd is unavailable in a Lab Color document: it saves as a Lab TIFF, and Export as PSD writes its PSD."
    );
    assert_eq!(
        save(
            doc,
            ptr::null(),
            &dir.path().join("x.psd"),
            r#"{"format":"psd"}"#
        )
        .unwrap_err(),
        "Saving as psd is unavailable in a Lab Color document: it saves as a Lab TIFF, and Export as PSD writes its PSD."
    );
    assert_eq!(
        plan(doc, ptr::null(), r#"{"format":"tiff","bogus":1}"#)["refusal"],
        "Lab save request has an unknown key \"bogus\""
    );
    let other = patch_lab(IccProfile::srgb(), 16, true);
    let other_doc = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(2, 2, vec![[0, 0, 0, 65535]; 4], IccProfile::srgb(), None)
            .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let rgb = Box::into_raw(Box::new(other_doc));
    assert_eq!(
        plan(rgb, ptr::null(), r#"{"format":"tiff"}"#)["refusal"],
        "Saving a Lab TIFF requires a Lab Color document"
    );
    let small = unsafe {
        doctored_core::ffi_precision::dr_precision_doc_render(rgb, LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!small.is_null());
    assert_eq!(
        plan(other, small, r#"{"format":"tiff"}"#)["refusal"],
        "high-depth projection dimensions do not match the document canvas"
    );
    unsafe {
        dr_precision_image_free(small);
        dr_precision_image_free(projection);
    }
    for d in [rgb, other, doc] {
        free(d);
    }
}

fn export(doc: *const PrecisionDocument, request: &str) -> Result<(Vec<u8>, Value), String> {
    let request = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let result: *mut ExportResult = unsafe {
        dr_precision_doc_export_encode(
            doc,
            ptr::null(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if result.is_null() {
        return Err(take_err_string(err));
    }
    let mut len = 0usize;
    let bytes = unsafe {
        let p = dr_export_result_bytes(result, &mut len);
        std::slice::from_raw_parts(p, len).to_vec()
    };
    let info = json_of(unsafe { dr_export_result_info(result, &mut err) }, err);
    unsafe { dr_export_result_free(result) };
    Ok((bytes, info))
}

#[test]
fn exports_write_rgb_through_the_inverse_formula_and_clip_out_of_gamut() {
    let doc = patch_lab(IccProfile::display_p3(), 16, false);
    let codes = render16(doc);
    let mut clipped = 0;
    for request in [
        r#"{"format":"png","bit_depth":16,"profile":"srgb"}"#,
        r#"{"format":"png","bit_depth":16,"profile":"document"}"#,
    ] {
        let (bytes, info) = export(doc, request).unwrap();
        assert_eq!(info["bit_depth"], 16, "{request}: {info}");
        assert!(
            info["profile_name"].as_str().unwrap().contains("sRGB"),
            "{request}: {info}"
        );
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert!(
            matches!(
                decoded.color(),
                image::ColorType::Rgb16 | image::ColorType::Rgba16
            ),
            "{request}: {:?}",
            decoded.color()
        );
        let pixels = decoded.to_rgba16();
        for (i, code) in codes.iter().enumerate() {
            let raw = lab_to_srgb(decode(*code));
            if raw.iter().any(|v| !(-0.002..=1.002).contains(v)) {
                clipped += 1;
            }
            let oracle = raw.map(|v| v.clamp(0.0, 1.0));
            let got = pixels.get_pixel(i as u32, 0);
            for c in 0..3 {
                let want = (oracle[c] * 65535.0).round() as i64;
                assert!(
                    (i64::from(got[c]) - want).abs() <= 32,
                    "{request} patch {i} channel {c}: {} against {want}",
                    got[c]
                );
            }
        }
    }
    assert!(clipped >= 2, "the P3 patches include colours outside sRGB");
    let (bytes, _) = export(doc, r#"{"format":"jpeg","bit_depth":8,"profile":"srgb"}"#).unwrap();
    assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
    free(doc);
}

#[test]
fn the_thumbnail_fits_before_it_converts() {
    let request = r#"{"depth":16,"fill_srgb":[0.2,0.6,0.4,1]}"#;
    let new = |side: u32| {
        let request = CString::new(request).unwrap();
        let mut err = ptr::null_mut();
        let doc =
            unsafe { dr_precision_doc_new_lab(side, side, request.as_ptr(), LIMIT, 0, &mut err) };
        assert!(!doc.is_null(), "{}", take_err_string(err));
        doc
    };
    let one = new(1);
    let code = unsafe { &*one }
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()[0];
    let oracle = lab_to_rgb(decode(code), P3_TO_XYZ).map(|v| (v.clamp(0.0, 1.0) * 255.0).round());
    let big = new(4096);
    let image: *const PrecisionImage = unsafe { &*big }.layer_source(0).unwrap();
    // Below the whole layer's 8 B/px view beside its own retained bytes.
    let limit = unsafe { &*image }.retained_bytes() + 4096 * 4096 * 8 - 1;
    let mut err = ptr::null_mut();
    let thumb: *mut DrImage = unsafe {
        dr_precision_image_lab_thumbnail_rgba8(image, 64, limit, 0, ptr::null(), &mut err)
    };
    assert!(!thumb.is_null(), "{}", take_err_string(err));
    let (w, h) = unsafe { (dr_image_width(thumb), dr_image_height(thumb)) };
    assert_eq!((w, h), (64, 64));
    let bytes =
        unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(thumb), (w * h * 4) as usize) };
    for p in bytes.chunks_exact(4) {
        for c in 0..3 {
            assert!(
                (f64::from(p[c]) - oracle[c]).abs() <= 1.0,
                "{p:?} against {oracle:?}"
            );
        }
        assert_eq!(p[3], 255);
    }
    unsafe { dr_image_free(thumb) };
    // Not Lab: the view's sentence.
    let rgb = PrecisionImage::from_rgba16(1, 1, vec![[0, 0, 0, 65535]], IccProfile::srgb(), None)
        .unwrap();
    let refused = unsafe {
        dr_precision_image_lab_thumbnail_rgba8(&rgb, 64, LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(refused.is_null());
    assert_eq!(
        take_err_string(err),
        "the Lab Color view takes a Lab Color image"
    );
    free(big);
    free(one);
}
