//! #1 Lab phase 3a, flat Lab TIFFs as Lab Color documents through
//! `dr_path_lab_source` and `dr_precision_doc_open_lab`, the files written
//! here (`lab_psd_fixture::write_lab_tiff`) and decoded by the independent
//! oracle (`lab_psd_fixture`'s `cielab_units`, a thin wrapper over
//! `lab_blend_oracle::cielab_unit`, and `icclab_units`):
//!
//! * CIELab (Photometric 8) at 8 and 16 bits, `II` and `MM`, SampleFormat
//!   absent or [1, 2, 2], and ICCLab (Photometric 9, TIFF Technote 4's
//!   published encoding, declared, not measured) at 8 and 16 bits open with
//!   every code the oracle's: exact at 8 bits, within one code at 16 (the
//!   oracle rounds in f64);
//! * an unassociated alpha (ExtraSamples 2) is kept (8 bits ×257);
//! * an associated alpha (ExtraSamples 1) is divided out of the colour in
//!   its offset form (L as stored; a\*/b\* signed + the half code for
//!   CIELab, as stored for ICCLab) at the file's depth: the codes are the
//!   oracle's of the colour divided back out in f64, and a pixel at alpha 0
//!   reads code 0 on every channel. Photoshop 26.10.0's own 16-bit layered
//!   Lab TIFFs (`tiff_little_endian_modes`, both byte orders) open flat with
//!   those codes, and multiplied back by alpha they are the stored samples
//!   within 2 of 65535 (`lab_psd_photoshop.rs`'s `associated` rule);
//! * `dr_path_lab_source` answers NONE for a planar file, an RGB TIFF, a
//!   fourth sample declared ExtraSamples 0 and an absent file, and the open
//!   refuses them;
//! * our own Lab TIFF save reopens with codes equal at 8 bits, and at 16
//!   bits L\* and alpha equal, a\*/b\* within one code (the CIELab 16-bit
//!   write/read pair returns code + 1 when code mod 257 = 128);
//! * the resolution survives the save and the open;
//! * the open refuses at a working limit below what it must hold: below
//!   the file read's charge, and (a PackBits 6000 × 4000 canvas) above it
//!   but below the open plan, which then refuses;
//! * a PhotometricInterpretation stored as a LONG is not claimed.

use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_lab::{
    dr_path_lab_source, dr_precision_doc_convert_to_lab, dr_precision_doc_open_lab,
    dr_precision_doc_save_lab,
};
use doctored_core::ffi_precision::{dr_precision_doc_descriptor, dr_precision_doc_free};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::Value;
use tempfile::TempDir;

use crate::common::psd_photoshop::fixture;
use crate::common::{cpath, take_err_string};
use crate::lab_psd_fixture::{
    cielab_sample, cielab_units, code_of, icclab_sample, icclab_units, read_lab_tiff,
    write_lab_tiff, TiffShape,
};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 7;
const H: u32 = 5;

fn units(i: u32, bits: u16) -> [f64; 3] {
    let frac = if bits == 16 { 0.41 } else { 0.0 };
    [
        f64::from(3 + (i * 29) % 95) + frac,
        f64::from((i * 47) % 250) - 125.0 + frac,
        f64::from((i * 61 + 13) % 250) - 124.0 - frac,
    ]
}

fn source(path: &Path) -> i32 {
    unsafe { dr_path_lab_source(cpath(path).as_ptr()) }
}

fn open(path: &Path, limit: u64) -> Result<*mut PrecisionDocument, String> {
    let mut err = ptr::null_mut();
    let doc =
        unsafe { dr_precision_doc_open_lab(cpath(path).as_ptr(), limit, 0, ptr::null(), &mut err) };
    if doc.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(doc)
    }
}

fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

fn depth(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_descriptor(doc, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value["lab"]["depth"].clone()
}

fn render(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

/// A flat Lab TIFF of the `units` pattern (and an alpha ramp when `extra`)
/// and the oracle's codes of its samples.
fn lab_file(shape: &TiffShape) -> (Vec<u8>, Vec<[u16; 4]>) {
    let bits = shape.bits;
    let top = if bits == 8 { 255u32 } else { 65535 };
    let mut samples = Vec::new();
    let mut want = Vec::new();
    for i in 0..W * H {
        let u = units(i, bits);
        let mut pixel = [0u16; 4];
        for c in 0..3 {
            let (raw, unit) = if shape.photometric == 8 {
                let raw = cielab_sample(bits, c, u[c]);
                (raw, cielab_units(bits, c, raw))
            } else {
                let raw = icclab_sample(bits, c, u[c]);
                (raw, icclab_units(bits, c, raw))
            };
            samples.push(raw);
            pixel[c] = code_of(c, unit);
        }
        pixel[3] = 65535;
        if shape.extra.is_some() {
            let a = ((i * 37) % 230 + 20) * top / 255;
            samples.push(a as u16);
            pixel[3] = if bits == 8 { a as u16 * 257 } else { a as u16 };
        }
        want.push(pixel);
    }
    (write_lab_tiff(W, H, &samples, shape), want)
}

#[test]
fn cielab_and_icclab_tiffs_open_with_the_oracles_codes() {
    let dir = TempDir::new().unwrap();
    // (photometric, bits, big-endian, SampleFormat, ExtraSamples)
    type Case = (u16, u16, bool, Option<Vec<u16>>, Option<u16>);
    let cases: Vec<Case> = vec![
        (8, 8, false, None, None),
        (8, 8, true, Some(vec![1, 2, 2]), None),
        (8, 16, false, Some(vec![1, 2, 2]), None),
        (8, 16, true, None, None),
        (8, 16, false, None, Some(2)),
        (8, 8, true, Some(vec![1, 2, 2, 1]), Some(2)),
        (9, 8, false, None, None),
        (9, 8, true, Some(vec![1, 1, 1]), None),
        (9, 16, true, None, None),
        (9, 16, false, None, Some(2)),
    ];
    for (n, (photometric, bits, big, sample_format, extra)) in cases.into_iter().enumerate() {
        let shape = TiffShape {
            photometric,
            bits,
            big,
            extra,
            sample_format,
            planar: false,
        };
        let (bytes, want) = lab_file(&shape);
        let path = dir.path().join(format!("case{n}.tif"));
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(source(&path), 1, "case {n}: DR_LAB_SOURCE_TIFF");
        let doc = open(&path, LIMIT).unwrap_or_else(|e| panic!("case {n}: {e}"));
        assert!(unsafe { &*doc }.is_lab(), "case {n}");
        assert_eq!(depth(doc), bits, "case {n}");
        assert_eq!(unsafe { &*doc }.dimensions(), (W, H));
        let got = render(doc);
        let bound = if bits == 8 { 0 } else { 1 };
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            for c in 0..4 {
                assert!(
                    g[c].abs_diff(w[c]) <= bound,
                    "case {n} (photometric {photometric}, {bits} bits) pixel {i} channel {c}: {} against {}",
                    g[c],
                    w[c]
                );
            }
        }
        free(doc);
    }
}

#[test]
fn other_tiffs_answer_none_and_keep_their_own_open() {
    let dir = TempDir::new().unwrap();
    let base = TiffShape {
        photometric: 8,
        bits: 8,
        big: false,
        extra: None,
        sample_format: None,
        planar: false,
    };
    let shapes = [
        (
            // ExtraSamples 0: a fourth sample that is no transparency.
            "unspecified extra sample",
            TiffShape {
                extra: Some(0),
                ..base_clone(&base)
            },
        ),
        (
            "planar",
            TiffShape {
                planar: true,
                ..base_clone(&base)
            },
        ),
        (
            "rgb",
            TiffShape {
                photometric: 2,
                ..base_clone(&base)
            },
        ),
    ];
    for (name, shape) in shapes {
        let (bytes, _) = lab_file(&shape);
        let path = dir.path().join(format!("{name}.tif"));
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(source(&path), 0, "{name}: DR_LAB_SOURCE_NONE");
        let error = open(&path, LIMIT).unwrap_err();
        assert!(error.contains("not a Lab Color TIFF"), "{name}: {error}");
    }
    // A PhotometricInterpretation stored as a LONG is not claimed: the
    // decoder's patch is a SHORT.
    let (mut bytes, _) = lab_file(&base);
    let directory = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let count = usize::from(u16::from_le_bytes([bytes[directory], bytes[directory + 1]]));
    let entry = (0..count)
        .map(|i| directory + 2 + i * 12)
        .find(|&at| u16::from_le_bytes([bytes[at], bytes[at + 1]]) == 262)
        .unwrap();
    bytes[entry + 2..entry + 4].copy_from_slice(&4u16.to_le_bytes());
    bytes[entry + 8..entry + 12].copy_from_slice(&8u32.to_le_bytes());
    let path = dir.path().join("long-photometric.tif");
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(source(&path), 0, "a LONG Photometric: DR_LAB_SOURCE_NONE");
    let error = open(&path, LIMIT).unwrap_err();
    assert!(error.contains("not a Lab Color TIFF"), "{error}");
    let absent = dir.path().join("absent.tif");
    assert_eq!(source(&absent), 0, "an absent file");
    assert!(open(&absent, LIMIT).is_err());
}

/// The oracle's codes of one associated pixel: each colour sample in its
/// offset form (`flip` is the half code XORed into a CIELab a\*/b\*, 0 for L
/// and for ICCLab) divided by `alpha` in f64, rounded and capped at `top`, 0
/// at alpha 0, then back to the file's encoding and decoded by the oracle.
fn straight_codes(photometric: u16, bits: u16, stored: [u16; 3], alpha: u16) -> [u16; 4] {
    let top = if bits == 8 { 255.0 } else { 65535.0 };
    let half: u16 = 1 << (bits - 1);
    let mut out = [0u16; 4];
    for c in 0..3 {
        let flip = if photometric == 8 && c > 0 { half } else { 0 };
        let straight = if alpha == 0 {
            0
        } else {
            (f64::from(stored[c] ^ flip) * top / f64::from(alpha))
                .round()
                .min(top) as u16
        };
        let raw = straight ^ flip;
        let unit = if photometric == 8 {
            cielab_units(bits, c, raw)
        } else {
            icclab_units(bits, c, raw)
        };
        out[c] = code_of(c, unit);
    }
    out[3] = if bits == 8 { alpha * 257 } else { alpha };
    out
}

/// A flat Lab TIFF whose alpha is associated (ExtraSamples 1): the `units`
/// pattern's samples in their offset form multiplied by an alpha ramp that
/// holds 0 and full, and [`straight_codes`] of what was stored.
fn associated_file(shape: &TiffShape) -> (Vec<u8>, Vec<[u16; 4]>) {
    let bits = shape.bits;
    let top = if bits == 8 { 255.0 } else { 65535.0 };
    let half: u16 = 1 << (bits - 1);
    let mut samples = Vec::new();
    let mut want = Vec::new();
    for i in 0..W * H {
        let u = units(i, bits);
        let alpha = match i % 5 {
            0 => 0,
            1 => top as u16,
            _ => (f64::from((i * 37) % 230 + 20) * top / 255.0).round() as u16,
        };
        let mut stored = [0u16; 3];
        for c in 0..3 {
            let raw = if shape.photometric == 8 {
                cielab_sample(bits, c, u[c])
            } else {
                icclab_sample(bits, c, u[c])
            };
            let flip = if shape.photometric == 8 && c > 0 {
                half
            } else {
                0
            };
            let premultiplied = (f64::from(raw ^ flip) * f64::from(alpha) / top).round() as u16;
            stored[c] = premultiplied ^ flip;
            samples.push(stored[c]);
        }
        samples.push(alpha);
        want.push(straight_codes(shape.photometric, bits, stored, alpha));
    }
    (write_lab_tiff(W, H, &samples, shape), want)
}

#[test]
fn an_associated_alpha_is_divided_out_in_its_offset_form() {
    let dir = TempDir::new().unwrap();
    // (photometric, bits, big-endian, SampleFormat)
    let cases: [(u16, u16, bool, Option<Vec<u16>>); 5] = [
        (8, 8, false, None),
        (8, 8, true, Some(vec![1, 2, 2, 1])),
        (8, 16, true, None),
        (9, 8, false, None),
        (9, 16, false, None),
    ];
    for (n, (photometric, bits, big, sample_format)) in cases.into_iter().enumerate() {
        let shape = TiffShape {
            photometric,
            bits,
            big,
            extra: Some(1),
            sample_format,
            planar: false,
        };
        let (bytes, want) = associated_file(&shape);
        let path = dir.path().join(format!("associated{n}.tif"));
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(source(&path), 1, "case {n}: DR_LAB_SOURCE_TIFF");
        let doc = open(&path, LIMIT).unwrap_or_else(|e| panic!("case {n}: {e}"));
        assert_eq!(depth(doc), bits, "case {n}");
        let got = render(doc);
        let bound = if bits == 8 { 0 } else { 1 };
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            if w[3] == 0 {
                assert_eq!(*g, [0; 4], "case {n} pixel {i}: alpha 0 has no colour");
            }
            for c in 0..4 {
                assert!(
                    g[c].abs_diff(w[c]) <= bound,
                    "case {n} (photometric {photometric}, {bits} bits) pixel {i} channel {c}: {} against {}",
                    g[c],
                    w[c]
                );
            }
        }
        free(doc);
    }
}

#[test]
fn photoshops_associated_lab_tiffs_open_flat_in_either_byte_order() {
    for scene in ["s1", "s2"] {
        for compression in ["rle", "zip"] {
            let mut opened = Vec::new();
            for order in ["ibm", "mac"] {
                let name = format!("{scene}_lab_16_{compression}_{order}.tif");
                let path = fixture("tiff_little_endian_modes", &name);
                let tiff = read_lab_tiff(&std::fs::read(&path).unwrap());
                assert_eq!(
                    (tiff.photometric, tiff.bits, tiff.extra_samples.as_slice()),
                    (8, 16, &[1][..]),
                    "{name}: a 16-bit CIELab image with an associated alpha"
                );
                assert_eq!(source(&path), 1, "{name}: DR_LAB_SOURCE_TIFF");
                let doc = open(&path, LIMIT).unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(depth(doc), 16, "{name}");
                assert_eq!(unsafe { &*doc }.dimensions(), (tiff.width, tiff.height));
                let got = render(doc);
                free(doc);
                let partial = tiff.planes[3]
                    .iter()
                    .filter(|&&a| a != 0 && a != 65535)
                    .count();
                assert!(partial > 0, "{name}: no partial alpha to divide out");
                for (i, g) in got.iter().enumerate() {
                    let stored = [0, 1, 2].map(|c| tiff.planes[c][i]);
                    let alpha = tiff.planes[3][i];
                    let want = straight_codes(8, 16, stored, alpha);
                    for c in 0..4 {
                        assert!(
                            g[c].abs_diff(want[c]) <= 1,
                            "{name} pixel {i} channel {c}: {} against the oracle's {}",
                            g[c],
                            want[c]
                        );
                    }
                    // The `associated` rule (`lab_psd_photoshop.rs`): ours on
                    // the file's scale (a*/b* code · 256/257 is the offset
                    // sample) times alpha is Photoshop's stored offset sample.
                    for c in 0..3 {
                        let scale = if c == 0 { 1.0 } else { 256.0 / 257.0 };
                        let mine = (f64::from(g[c]) * scale * f64::from(alpha) / 65535.0).round();
                        let theirs = if c == 0 {
                            stored[c]
                        } else {
                            stored[c] ^ 0x8000
                        };
                        assert!(
                            (mine - f64::from(theirs)).abs() <= 2.0,
                            "{name} pixel {i} channel {c}: ours premultiplied {mine}, stored {theirs}"
                        );
                    }
                }
                opened.push(got);
            }
            assert!(
                opened[0] == opened[1],
                "{scene} {compression}: the IBM file and its Macintosh twin differ"
            );
        }
    }
}

/// A PackBits CIELab TIFF of `width` × `height` 8-bit zero samples, `II`:
/// a large canvas in a small file.
fn packbits_lab_tiff(width: u32, height: u32) -> Vec<u8> {
    let mut row = Vec::new();
    let mut left = width as usize * 3;
    while left > 0 {
        let run = left.min(128);
        row.push((257 - run) as u8);
        row.push(0);
        left -= run;
    }
    let mut out = b"II*\0".to_vec();
    out.extend([0; 4]);
    let strip_at = out.len() as u32;
    for _ in 0..height {
        out.extend(&row);
    }
    let strip_len = out.len() as u32 - strip_at;
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let bits_at = out.len() as u32;
    for _ in 0..3 {
        out.extend(8u16.to_le_bytes());
    }
    let directory = out.len() as u32;
    let short = |v: u16| [v.to_le_bytes()[0], v.to_le_bytes()[1], 0, 0];
    let entries: [(u16, u16, u32, [u8; 4]); 10] = [
        (256, 4, 1, width.to_le_bytes()),
        (257, 4, 1, height.to_le_bytes()),
        (258, 3, 3, bits_at.to_le_bytes()),
        (259, 3, 1, short(32773)),
        (262, 3, 1, short(8)),
        (273, 4, 1, strip_at.to_le_bytes()),
        (277, 3, 1, short(3)),
        (278, 4, 1, height.to_le_bytes()),
        (279, 4, 1, strip_len.to_le_bytes()),
        (284, 3, 1, short(1)),
    ];
    out.extend((entries.len() as u16).to_le_bytes());
    for (tag, kind, count, value) in entries {
        out.extend(tag.to_le_bytes());
        out.extend(kind.to_le_bytes());
        out.extend(count.to_le_bytes());
        out.extend(value);
    }
    out.extend([0; 4]);
    out[4..8].copy_from_slice(&directory.to_le_bytes());
    out
}

#[test]
fn the_open_plan_refuses_a_large_canvas_in_a_small_file() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (6000u32, 4000u32);
    let path = dir.path().join("large.tif");
    let bytes = packbits_lab_tiff(w, h);
    let len = bytes.len() as u64;
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(source(&path), 1);
    // Above the file read's own charge (the file plus the codec's fixed
    // 256 MiB decode reservation), below what the decode must hold: the
    // u16 codes and the u16 RGBA plane alone are 14 B a pixel.
    let limit = 300 * 1024 * 1024;
    let held = u64::from(w) * u64::from(h) * (3 * 2 + 8);
    assert!(len + 256 * 1024 * 1024 < limit && held > limit);
    let error = open(&path, limit).unwrap_err();
    let needs: u64 = error
        .strip_prefix("precision operation needs ")
        .and_then(|rest| rest.split(' ').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("not the open plan's refusal: {error}"));
    assert!(
        needs >= held && error.ends_with(&format!("working limit is {limit} bytes")),
        "{error}"
    );
    // The same file opens at a limit that holds the plan.
    let doc = open(&path, LIMIT).unwrap();
    assert!(unsafe { &*doc }.is_lab());
    assert_eq!(unsafe { &*doc }.dimensions(), (w, h));
    free(doc);
}

fn base_clone(shape: &TiffShape) -> TiffShape {
    TiffShape {
        photometric: shape.photometric,
        bits: shape.bits,
        big: shape.big,
        extra: shape.extra,
        sample_format: shape.sample_format.clone(),
        planar: shape.planar,
    }
}

/// A transparent sRGB patch document, Mode ▸ Lab Color at `bits`, at
/// 240 × 180 ppi.
fn lab_document(bits: u32) -> *mut PrecisionDocument {
    let pixels: Vec<[u16; 4]> = (0..W * H)
        .map(|i| {
            let code = |k: u32| ((i * k + 17) % 256) as u16 * 257;
            [code(53), code(97), code(31), 65535 - (i as u16 * 1500)]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(
            image,
            Metadata::default(),
            Resolution { x: 240.0, y: 180.0 },
        )
        .unwrap(),
    ));
    let options = CString::new(format!(r#"{{"depth":{bits}}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let lab = unsafe {
        dr_precision_doc_convert_to_lab(rgb, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!lab.is_null(), "to Lab: {}", take_err_string(err));
    free(rgb);
    lab
}

#[test]
fn our_lab_tiff_save_reopens_with_its_codes_and_resolution() {
    let dir = TempDir::new().unwrap();
    for bits in [8u32, 16] {
        let doc = lab_document(bits);
        let path = dir.path().join(format!("saved{bits}.tif"));
        let request = CString::new(r#"{"format":"tiff"}"#).unwrap();
        let mut err = ptr::null_mut();
        let ok = unsafe {
            dr_precision_doc_save_lab(
                doc,
                ptr::null(),
                cpath(&path).as_ptr(),
                request.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(ok, "save: {}", take_err_string(err));
        assert_eq!(source(&path), 1);
        let back = open(&path, LIMIT).unwrap();
        assert!(unsafe { &*back }.is_lab());
        assert_eq!(depth(back), bits);
        let resolution = unsafe { &*back }.resolution();
        assert!(
            (resolution.x - 240.0).abs() < 0.01 && (resolution.y - 180.0).abs() < 0.01,
            "{resolution:?}"
        );
        let (mine, theirs) = (render(doc), render(back));
        for (i, (m, t)) in mine.iter().zip(&theirs).enumerate() {
            for c in 0..4 {
                let bound = if bits == 16 && (c == 1 || c == 2) {
                    1
                } else {
                    0
                };
                assert!(
                    m[c].abs_diff(t[c]) <= bound,
                    "{bits} bits pixel {i} channel {c}: {} saved, {} reopened",
                    m[c],
                    t[c]
                );
            }
        }
        // Below what the open must hold, it refuses.
        let error = open(&path, 4096).unwrap_err();
        assert!(
            error.contains("precision operation needs")
                && error.contains("working limit is 4096 bytes"),
            "{error}"
        );
        free(back);
        free(doc);
    }
}
