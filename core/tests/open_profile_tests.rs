//! The flat open's embedded-profile decision (`open_profile`): a
//! grayscale file with a Gray ICC and a CMYK TIFF or CMYK/YCCK JPEG
//! with a CMYK ICC are converted from that profile to sRGB as they are read,
//! every profile the open does not keep is named in the import report
//! (`profile_refusal`, also the last `warnings` entry), a CMYK file the one
//! gate refuses fails the open with no `profile_refusal`, a platform
//! decode of a Gray, CMYK or Lab image is worded by
//! `dr_platform_conversion_sentence` — the converted sentence when the open
//! would convert from the profile itself, else a plain one — and a kept or
//! absent profile opens exactly as before.
//!
//! Everything goes through the FFI. Profiles are built byte by byte
//! (`common/icc_lut_builders`, plus a local `kTRC`); the CMYK TIFFs are
//! hand-assembled little-endian classic TIFFs and the CMYK JPEGs
//! hand-assembled baseline JPEGs (`common/cmyk_jpeg_builder`). The oracles
//! are written out here: the IEC 61966-2-1 encode, the CIE L* inverse and
//! ISO 18619's BPC affine — never the crate's own tables.

use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::*;
use doctored_core::ffi_color::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_metadata_report::{
    dr_path_metadata_import_report, dr_platform_conversion_sentence,
};
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_info, dr_precision_doc_open,
};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use tempfile::TempDir;

use crate::common;
use common::{cpath, img_pixels, layer_dims, layer_pixels, take_err_string};

use crate::icc_lut_builders as builders;
use builders::{build, tag_desc, tag_mft2, tag_wtpt};

use crate::cmyk_jpeg_builder as jpeg_builder;
use jpeg_builder::{cmyk_jpeg, CmykJpeg};

use crate::cmyk_tiff_builder::cmyk_tiff;

const D50: [f64; 3] = [0.9642, 1.0, 0.8249];

// -------------------------------------------------------------- profiles --

/// A `curv` holding one u8.8 gamma.
fn curv_gamma(gamma: f64) -> Vec<u8> {
    let mut t = b"curv\0\0\0\0".to_vec();
    t.extend_from_slice(&1u32.to_be_bytes());
    t.extend_from_slice(&((gamma * 256.0).round() as u16).to_be_bytes());
    t
}

/// A sampled `curv` of `n` points holding `f`.
fn curv_table(n: usize, f: impl Fn(f64) -> f64) -> Vec<u8> {
    let mut t = b"curv\0\0\0\0".to_vec();
    t.extend_from_slice(&(n as u32).to_be_bytes());
    for i in 0..n {
        let x = i as f64 / (n - 1) as f64;
        t.extend_from_slice(&((f(x).clamp(0.0, 1.0) * 65535.0).round() as u16).to_be_bytes());
    }
    t
}

fn gray_profile(name: &str, trc: Vec<u8>) -> Vec<u8> {
    build(
        [2, 0x10, 0, 0],
        b"mntr",
        b"GRAY",
        b"XYZ ",
        &[
            (b"desc", tag_desc(name)),
            (b"wtpt", tag_wtpt(D50)),
            (b"kTRC", trc),
        ],
    )
}

/// The γ1.8 Gray profile's gamma exactly as the u8.8 field stores it.
const GAMMA_18: f64 = 461.0 / 256.0;

/// A Dot-Gain-like sampled curve: darker than γ2.2 in the mid-tones.
fn dot_gain(x: f64) -> f64 {
    let base = x.powf(2.2);
    (base + 0.2 * base * (1.0 - base)).min(1.0) * 0.98 + 0.02 * x
}

/// The press test profile (#1 of the objections): `prtr`/`CMYK`/`Lab `, v2
/// `mft2` tables on a two-point grid. A2B0 = A2B1 gives
/// L* = 10 + 90·(1−c)(1−k), a* = b* = 0 — a black ABOVE L* = 0, so BPC has
/// something to move — and B2A1 = B2A0 inverts it on k alone, so the
/// black-point round trip (Lab 0 → k = 1 → L* 10) is computable.
fn cmyk_profile(name: &str) -> Vec<u8> {
    press_tables(name, &[b"A2B0", b"A2B1", b"B2A0", b"B2A1"])
}

/// The press test profile holding only the `keep` tables.
fn press_tables(name: &str, keep: &[&[u8; 4]]) -> Vec<u8> {
    // Legacy 16-bit Lab: L* = 100 sits at 0xFF00, a* = b* = 0 at 0x8000.
    let l = |lstar: f64| lstar / 100.0 * 65280.0 / 65535.0;
    let zero_ab = 32768.0 / 65535.0;
    let a2b = tag_mft2(4, 3, 2, &|d: &[f64]| {
        vec![
            l(10.0 + 90.0 * (1.0 - d[0]) * (1.0 - d[3])),
            zero_ab,
            zero_ab,
        ]
    });
    let b2a = tag_mft2(3, 4, 2, &|lab: &[f64]| vec![0.0, 0.0, 0.0, 1.0 - lab[0]]);
    let mut tags: Vec<(&[u8; 4], Vec<u8>)> =
        vec![(b"desc", tag_desc(name)), (b"wtpt", tag_wtpt(D50))];
    for sig in keep {
        let table = if sig.starts_with(b"A2B") { &a2b } else { &b2a };
        tags.push((sig, table.clone()));
    }
    build([2, 0x10, 0, 0], b"prtr", b"CMYK", b"Lab ", &tags)
}

fn builtin(which: std::ffi::c_int) -> Vec<u8> {
    let len = dr_builtin_profile_len(which);
    let mut out = vec![0u8; len];
    assert!(unsafe { dr_builtin_profile(which, out.as_mut_ptr(), len) });
    out
}

fn srgb_profile() -> Vec<u8> {
    builtin(0)
}

fn p3_profile() -> Vec<u8> {
    builtin(1)
}

// --------------------------------------------------------------- oracles --

/// The sRGB forward transfer function, IEC 61966-2-1.
fn srgb_encode(linear: f64) -> f64 {
    let v = linear.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn code8(encoded: f64) -> i32 {
    (encoded.clamp(0.0, 1.0) * 255.0).round() as i32
}

/// Y from CIE L* — the inverse of L* = 116·f(Y) − 16.
fn y_of_lstar(lstar: f64) -> f64 {
    let fy = (lstar + 16.0) / 116.0;
    let cube = fy * fy * fy;
    if cube > 216.0 / 24389.0 {
        cube
    } else {
        (116.0 * fy - 16.0) * 27.0 / 24389.0
    }
}

/// The test press profile's source black, as Y.
fn press_black() -> f64 {
    y_of_lstar(10.0)
}

/// ISO 18619 onto a zero destination black, on a neutral (module doc of
/// `icc_intent`): Y' = (Y − Yb) / (1 − Yb).
fn bpc(y: f64) -> f64 {
    let black = press_black();
    (y - black) / (1.0 - black)
}

/// The expected sRGB code for one CMYK pixel of the press profile.
fn cmyk_oracle(c: f64, k: f64) -> i32 {
    let lstar = 10.0 + 90.0 * (1.0 - c) * (1.0 - k);
    code8(srgb_encode(bpc(y_of_lstar(lstar))))
}

// ------------------------------------------------------------ containers --

fn png(w: u32, h: u32, color: ExtendedColorType, raw: &[u8], icc: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = PngEncoder::new(&mut out);
    if let Some(icc) = icc {
        enc.set_icc_profile(icc.to_vec()).unwrap();
    }
    enc.write_image(raw, w, h, color).unwrap();
    out
}

fn gray_jpeg(w: u32, h: u32, raw: &[u8], icc: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, 100);
    enc.set_icc_profile(icc.to_vec()).unwrap();
    enc.write_image(raw, w, h, ExtendedColorType::L8).unwrap();
    out
}

/// The CMYK sample grid the conversion tests read: every (c, k) pair of
/// `LEVELS`, each with two different (m, y) — which the press table ignores.
const LEVELS: [u16; 6] = [0, 51, 102, 153, 204, 255];

fn cmyk_grid() -> Vec<[u16; 4]> {
    let mut out = Vec::new();
    for &c in &LEVELS {
        for &k in &LEVELS {
            out.push([c, 0, 0, k]);
            out.push([c, 200, 37, k]);
        }
    }
    out
}

// ------------------------------------------------------------ FFI driver --

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn open_doc(path: &Path) -> *mut DrDocument {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_doc_open(c.as_ptr(), &mut err) };
    assert!(!doc.is_null(), "open {path:?}: {}", take_err_string(err));
    doc
}

/// The document's first layer and its profile, the document freed.
fn open_pixels(path: &Path) -> (Vec<u8>, Vec<u8>) {
    let doc = open_doc(path);
    let pixels = layer_pixels(doc, 0);
    let len = unsafe { dr_doc_icc_profile_len(doc) };
    let mut profile = vec![0u8; len];
    assert!(unsafe { dr_doc_icc_profile(doc, profile.as_mut_ptr(), len) });
    unsafe { dr_doc_free(doc) };
    (pixels, profile)
}

fn image_open_pixels(path: &Path) -> Vec<u8> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let img = unsafe { dr_image_open(c.as_ptr(), &mut err) };
    assert!(
        !img.is_null(),
        "dr_image_open {path:?}: {}",
        take_err_string(err)
    );
    let pixels = img_pixels(img);
    unsafe { dr_image_free(img) };
    pixels
}

fn report(path: &Path) -> serde_json::Value {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let json = unsafe { dr_path_metadata_import_report(c.as_ptr(), &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(json) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(json) };
    value
}

/// `profile_refusal`, asserting it is also the LAST warning.
fn refusal(path: &Path) -> Option<String> {
    let report = report(path);
    let sentence = report.get("profile_refusal")?.as_str().unwrap().to_string();
    let warnings = report["warnings"].as_array().unwrap();
    assert_eq!(
        warnings.last().and_then(|w| w.as_str()),
        Some(sentence.as_str()),
        "the refusal is the last warning: {report}"
    );
    Some(sentence)
}

const GRAY: std::ffi::c_int = 0;
const CMYK: std::ffi::c_int = 1;
const LAB: std::ffi::c_int = 2;

/// `dr_platform_conversion_sentence` over `bytes` (NULL when `None`).
fn platform_bytes(bytes: Option<&[u8]>, source: std::ffi::c_int) -> String {
    let (data, len) = bytes.map_or((ptr::null(), 0), |b| (b.as_ptr(), b.len()));
    let mut err = ptr::null_mut();
    let sentence = unsafe { dr_platform_conversion_sentence(data, len, source, &mut err) };
    assert!(!sentence.is_null(), "{}", take_err_string(err));
    assert!(err.is_null());
    let text = unsafe { CStr::from_ptr(sentence) }
        .to_str()
        .unwrap()
        .to_string();
    unsafe { dr_string_free(sentence) };
    text
}

/// The platform decode's sentence for the file at `path`, read as the host
/// reads it: the whole file handed over.
fn platform(path: &Path, source: std::ffi::c_int) -> String {
    platform_bytes(Some(&std::fs::read(path).unwrap()), source)
}

/// The plain platform sentence for a colour model.
fn plain(model: &str) -> String {
    format!(
        "The image is {model}, with no embedded colour profile Doctored can convert from, so \
         it was converted to sRGB as it was read."
    )
}

/// The report of a file the core open refuses: no `profile_refusal`, and
/// (the old `platform_profile_conversion` key is gone) nothing about the
/// platform's decode either.
fn assert_gate_refused_report(path: &Path) {
    assert_eq!(refusal(path), None);
    let report = report(path);
    assert!(
        report.get("platform_profile_conversion").is_none(),
        "{report}"
    );
}

fn converted(name: &str, kind: &str) -> String {
    format!(
        "The embedded colour profile “{name}” is a {kind} profile, so the image was converted \
         from it to sRGB as it was read."
    )
}

const READ_AS_SRGB: &str = "so it was ignored and the image's colour numbers were read as sRGB.";

/// An untagged CMYK file's sentence when no working CMYK profile is
/// set — never in this binary, which sets none (`untagged_cmyk_working`
/// runs in its own process for that).
const UNTAGGED_CMYK_UNSET: &str = "The image is CMYK with no embedded colour profile and no \
     working CMYK profile is set, so its colours were converted to sRGB without colour \
     management as it was read.";

fn assert_close(actual: i32, expected: i32, what: &str) {
    assert!(
        (actual - expected).abs() <= 1,
        "{what}: got {actual}, expected {expected} ±1"
    );
}

fn assert_neutral_opaque(pixels: &[u8], what: &str) {
    for (i, px) in pixels.chunks_exact(4).enumerate() {
        assert!(
            px[0] == px[1] && px[1] == px[2],
            "{what}: pixel {i} not neutral {px:?}"
        );
        assert_eq!(px[3], 255, "{what}: pixel {i} alpha");
    }
}

// ------------------------------------------------------------------ gray --

#[test]
fn a_gray_png_with_a_gamma_profile_converts_every_code() {
    let dir = TempDir::new().unwrap();
    let raw: Vec<u8> = (0..=255u8).collect();
    let profile = gray_profile("Gray Gamma 1.8", curv_gamma(1.8));
    let path = write(
        &dir,
        "gray18.png",
        &png(256, 1, ExtendedColorType::L8, &raw, Some(&profile)),
    );
    let (pixels, doc_profile) = open_pixels(&path);
    assert_neutral_opaque(&pixels, "γ1.8");
    for (g, px) in pixels.chunks_exact(4).enumerate() {
        let expected = code8(srgb_encode((g as f64 / 255.0).powf(GAMMA_18)));
        assert_close(i32::from(px[0]), expected, &format!("γ1.8 code {g}"));
    }
    // A mid-tone moves: the conversion is real, not a relabel.
    assert!((i32::from(pixels[128 * 4]) - 128).abs() >= 8);
    assert_eq!(doc_profile, srgb_profile(), "the document is sRGB");
    assert_eq!(
        refusal(&path).as_deref(),
        Some(converted("Gray Gamma 1.8", "Gray").as_str())
    );
    assert_eq!(image_open_pixels(&path), pixels, "dr_image_open agrees");
}

#[test]
fn a_gray_alpha_png_with_a_sampled_curve_keeps_its_alpha() {
    let dir = TempDir::new().unwrap();
    let mut raw = Vec::new();
    for g in 0..=255u8 {
        raw.extend_from_slice(&[g, g.wrapping_mul(7).wrapping_add(3)]);
    }
    let profile = gray_profile("Dot Gain Test", curv_table(1024, dot_gain));
    let path = write(
        &dir,
        "graya.png",
        &png(256, 1, ExtendedColorType::La8, &raw, Some(&profile)),
    );
    let (pixels, _) = open_pixels(&path);
    for (g, px) in pixels.chunks_exact(4).enumerate() {
        assert_eq!(px[3], raw[g * 2 + 1], "alpha {g} kept exactly");
        assert!(px[0] == px[1] && px[1] == px[2], "neutral {g}");
        let expected = code8(srgb_encode(dot_gain(g as f64 / 255.0)));
        assert_close(i32::from(px[0]), expected, &format!("dot gain code {g}"));
    }
    assert_eq!(
        refusal(&path).as_deref(),
        Some(converted("Dot Gain Test", "Gray").as_str())
    );
}

#[test]
fn a_16_bit_gray_png_converts_from_its_16_bit_samples() {
    let dir = TempDir::new().unwrap();
    // γ0.5 is steep near black, so the low byte decides the answer: 128/65535
    // reads as 0 in 8 bits but as sRGB ≈ 59 from the 16-bit sample.
    let samples: Vec<u16> = [0u16, 64, 128, 300, 1000, 4096, 20000, 32768, 50000, 65535]
        .into_iter()
        .chain((0..64).map(|i| i * 1021 + 17))
        .collect();
    let raw: Vec<u8> = samples.iter().flat_map(|s| s.to_ne_bytes()).collect();
    let profile = gray_profile("Gray Gamma 0.5", curv_gamma(0.5));
    let path = write(
        &dir,
        "gray16.png",
        &png(
            samples.len() as u32,
            1,
            ExtendedColorType::L16,
            &raw,
            Some(&profile),
        ),
    );
    let (pixels, doc_profile) = open_pixels(&path);
    assert_neutral_opaque(&pixels, "gray16");
    for (&s, px) in samples.iter().zip(pixels.chunks_exact(4)) {
        let expected = code8(srgb_encode((f64::from(s) / 65535.0).sqrt()));
        assert_close(i32::from(px[0]), expected, &format!("16-bit sample {s}"));
    }
    let eight_bit = code8(srgb_encode(((128.0f64 / 257.0).round() / 255.0).sqrt()));
    assert!(
        (i32::from(pixels[2 * 4]) - eight_bit).abs() >= 20,
        "sample 128 converts from 16 bits, not from its 8-bit reduction"
    );
    assert_eq!(doc_profile, srgb_profile());
    assert_eq!(
        refusal(&path).as_deref(),
        Some(converted("Gray Gamma 0.5", "Gray").as_str())
    );
}

#[test]
fn a_gray_jpeg_with_a_gray_profile_converts() {
    let dir = TempDir::new().unwrap();
    let raw: Vec<u8> = (0..64u32 * 8).map(|i| ((i % 64) * 4) as u8).collect();
    let profile = gray_profile("Gray Gamma 1.8", curv_gamma(1.8));
    let bytes = gray_jpeg(64, 8, &raw, &profile);
    let path = write(&dir, "gray.jpg", &bytes);
    let decoded = image::load_from_memory(&bytes).unwrap().to_luma8();
    let (pixels, doc_profile) = open_pixels(&path);
    assert_neutral_opaque(&pixels, "gray jpeg");
    for (luma, px) in decoded.pixels().zip(pixels.chunks_exact(4)) {
        let expected = code8(srgb_encode((f64::from(luma[0]) / 255.0).powf(GAMMA_18)));
        assert_close(
            i32::from(px[0]),
            expected,
            &format!("jpeg luma {}", luma[0]),
        );
    }
    assert_eq!(doc_profile, srgb_profile());
    assert_eq!(
        refusal(&path).as_deref(),
        Some(converted("Gray Gamma 1.8", "Gray").as_str())
    );
}

// ------------------------------------------------------------------ CMYK --

fn check_cmyk(path: &Path, grid: &[[u16; 4]], scale: f64, what: &str) {
    let (pixels, doc_profile) = open_pixels(path);
    assert_eq!(pixels.len(), grid.len() * 4);
    assert_neutral_opaque(&pixels, what);
    for (sample, px) in grid.iter().zip(pixels.chunks_exact(4)) {
        let (c, k) = (f64::from(sample[0]) / scale, f64::from(sample[3]) / scale);
        assert_close(
            i32::from(px[0]),
            cmyk_oracle(c, k),
            &format!("{what} {sample:?}"),
        );
    }
    // m and y change nothing: the press table ignores them.
    for pair in pixels.chunks_exact(8) {
        assert_eq!(pair[..4], pair[4..], "{what}: m/y moved the colour");
    }
    // Full ink opens at black — with BPC — not at the lifted grey of the
    // press's own black, which is where relative colorimetric alone lands.
    let no_bpc = code8(srgb_encode(press_black()));
    assert!(no_bpc >= 20, "the test profile's black is visibly lifted");
    let full_ink = pixels.chunks_exact(4).nth(grid.len() - 1).unwrap();
    assert!(full_ink[0] <= 1, "{what}: full ink {:?} is black", full_ink);
    assert!((no_bpc - i32::from(full_ink[0])).abs() >= 19);
    assert_eq!(doc_profile, srgb_profile(), "{what}: the document is sRGB");
    assert_eq!(
        refusal(path).as_deref(),
        Some(converted("Press Test", "CMYK").as_str()),
        "{what}"
    );
    assert_eq!(
        image_open_pixels(path),
        pixels,
        "{what}: dr_image_open agrees"
    );
}

#[test]
fn a_cmyk_tiff_with_a_cmyk_profile_converts_with_black_point_compensation() {
    let dir = TempDir::new().unwrap();
    let profile = cmyk_profile("Press Test");
    let grid = cmyk_grid();
    let n = grid.len() as u32;

    let data8: Vec<u8> = grid.iter().flatten().map(|&v| v as u8).collect();
    let path8 = write(
        &dir,
        "cmyk8.tif",
        &cmyk_tiff(n, 1, 8, 4, 0, &data8, Some(&profile)),
    );
    check_cmyk(&path8, &grid, 255.0, "CMYK8");

    let grid16: Vec<[u16; 4]> = grid.iter().map(|s| s.map(|v| v * 257)).collect();
    let data16: Vec<u8> = grid16
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let path16 = write(
        &dir,
        "cmyk16.tif",
        &cmyk_tiff(n, 1, 16, 4, 0, &data16, Some(&profile)),
    );
    check_cmyk(&path16, &grid16, 65535.0, "CMYK16");
}

/// #547 phase 3: an extra sample past the inks (ExtraSamples 0, a spot or
/// saved alpha channel) is counted and never read (`cmyk_tiff_read`), so the
/// file converts exactly as its four-sample twin, under the converted
/// sentence the header report shares with the open.
#[test]
fn a_cmyk_tiff_with_a_fifth_unspecified_sample_converts_its_four_inks() {
    let dir = TempDir::new().unwrap();
    let profile = cmyk_profile("Press Test");
    let data: Vec<u8> = (0..8u8 * 5).collect();
    let four: Vec<u8> = data.chunks(5).flat_map(|s| s[..4].to_vec()).collect();
    let path = write(
        &dir,
        "cmyk5.tif",
        &cmyk_tiff(8, 1, 8, 5, 0, &data, Some(&profile)),
    );
    let twin = write(
        &dir,
        "cmyk4.tif",
        &cmyk_tiff(8, 1, 8, 4, 0, &four, Some(&profile)),
    );
    assert_eq!(open_pixels(&path), open_pixels(&twin));
    assert_eq!(
        refusal(&path).as_deref(),
        Some(converted("Press Test", "CMYK").as_str())
    );
    assert_eq!(platform(&path, CMYK), converted("Press Test", "CMYK"));
}

/// A PLANAR (PlanarConfiguration = 2) CMYK8 TIFF: one uncompressed strip per
/// plane, `planes` holding C, M, Y and K in that order.
fn planar_cmyk_tiff(w: u32, h: u32, planes: [&[u8]; 4], icc: Option<&[u8]>) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    let mut offsets = Vec::new();
    for plane in planes {
        offsets.push(8 + body.len() as u32);
        body.extend_from_slice(plane);
    }
    let mut place = |bytes: &[u8]| -> u32 {
        if body.len() % 2 == 1 {
            body.push(0);
        }
        let at = 8 + body.len() as u32;
        body.extend_from_slice(bytes);
        at
    };
    let longs = |v: &[u32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    let bits_at = place(&[8, 0, 8, 0, 8, 0, 8, 0]);
    let offsets_at = place(&longs(&offsets));
    let counts_at = place(&longs(&planes.map(|p| p.len() as u32)));
    let icc_at = icc.map(&mut place);
    if body.len() % 2 == 1 {
        body.push(0);
    }
    let short = |v: u16| -> [u8; 4] {
        let b = v.to_le_bytes();
        [b[0], b[1], 0, 0]
    };
    let mut entries: Vec<(u16, u16, u32, [u8; 4])> = vec![
        (256, 4, 1, w.to_le_bytes()),
        (257, 4, 1, h.to_le_bytes()),
        (258, 3, 4, bits_at.to_le_bytes()),
        (259, 3, 1, short(1)),
        (262, 3, 1, short(5)),
        (273, 4, 4, offsets_at.to_le_bytes()),
        (277, 3, 1, short(4)),
        (278, 4, 1, h.to_le_bytes()),
        (279, 4, 4, counts_at.to_le_bytes()),
        (284, 3, 1, short(2)),
    ];
    if let (Some(icc), Some(at)) = (icc, icc_at) {
        entries.push((34675, 7, icc.len() as u32, at.to_le_bytes()));
    }
    let mut out = vec![0x49, 0x49, 42, 0];
    out.extend_from_slice(&(8 + body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, kind, count, value) in entries {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&value);
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

/// #547 phase 3: a planar CMYK TIFF is read by THE CMYK TIFF reader
/// (`cmyk_tiff_read`), so it converts exactly as its chunky twin, and the
/// header report — sharing the open's gate — calls it converted.
#[test]
fn a_planar_cmyk_tiff_converts_as_its_chunky_twin() {
    let dir = TempDir::new().unwrap();
    let profile = cmyk_profile("Press Test");
    let planes: [Vec<u8>; 4] =
        std::array::from_fn(|c| (0..8u8).map(|i| i * 30 + c as u8 * 7).collect());
    let path = write(
        &dir,
        "planar.tif",
        &planar_cmyk_tiff(
            8,
            1,
            [&planes[0], &planes[1], &planes[2], &planes[3]],
            Some(&profile),
        ),
    );
    let chunky: Vec<u8> = (0..8)
        .flat_map(|i| planes.iter().map(move |p| p[i]))
        .collect();
    let twin = write(
        &dir,
        "chunky.tif",
        &cmyk_tiff(8, 1, 8, 4, 0, &chunky, Some(&profile)),
    );
    assert_eq!(open_pixels(&path), open_pixels(&twin));
    assert_eq!(
        refusal(&path).as_deref(),
        Some(converted("Press Test", "CMYK").as_str())
    );
    assert_eq!(platform(&path, CMYK), converted("Press Test", "CMYK"));
}

/// The typed 16-bit open's Gray conversion words its warning through
/// the flat open's sentence, so the host can pick the typed open's own
/// sentence out of `import_warnings` by the header report's `profile_refusal`.
#[test]
fn the_typed_gray_warning_is_the_header_reports_sentence() {
    let dir = TempDir::new().unwrap();
    let samples: Vec<u16> = (0..64u16).map(|i| i * 1021 + 17).collect();
    let raw: Vec<u8> = samples.iter().flat_map(|s| s.to_ne_bytes()).collect();
    let profile = gray_profile("Gray Test", curv_gamma(1.8));
    let path = write(
        &dir,
        "gray16_typed.png",
        &png(64, 1, ExtendedColorType::L16, &raw, Some(&profile)),
    );
    let sentence = refusal(&path).expect("the header report names the profile");
    assert_eq!(sentence, converted("Gray Test", "Gray"));

    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open(c.as_ptr(), 2 * 1024 * 1024 * 1024, 0, ptr::null(), &mut err)
    };
    assert!(!doc.is_null(), "typed open: {}", take_err_string(err));
    let json = unsafe { dr_precision_doc_info(doc, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let info: serde_json::Value =
        serde_json::from_str(unsafe { CStr::from_ptr(json) }.to_str().unwrap()).unwrap();
    unsafe {
        dr_string_free(json);
        dr_precision_doc_free(doc);
    }
    let warnings = info["import_warnings"].as_array().unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str() == Some(sentence.as_str())),
        "the typed open's warnings carry the header sentence verbatim: {info}"
    );
}

/// Objection #2: a single-strip CMYK8 TIFF whose strip is past `tiff`'s
/// 128 MiB default intermediate buffer, which `image` admits, still opens.
#[test]
fn a_single_strip_cmyk_tiff_past_128_mib_still_opens() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (5800u32, 5800u32);
    let strip = (w * h * 4) as usize;
    assert!(strip > 128 * 1024 * 1024);
    let mut data = vec![0u8; strip];
    // One full-ink pixel at the end, so the whole strip is read.
    data[strip - 4..].copy_from_slice(&[0, 0, 0, 255]);
    let path = write(
        &dir,
        "large.tif",
        &cmyk_tiff(w, h, 8, 4, 0, &data, Some(&cmyk_profile("Press Test"))),
    );
    drop(data);
    let doc = open_doc(&path);
    let pixels = layer_pixels(doc, 0);
    unsafe { dr_doc_free(doc) };
    assert_eq!(pixels.len(), strip);
    assert_eq!(pixels[..4], [255, 255, 255, 255]);
    assert!(pixels[strip - 4] <= 1, "full ink is black");
}

// ---------------------------------------------------------------- guards --

#[test]
fn untagged_and_kept_profiles_open_exactly_as_before() {
    let dir = TempDir::new().unwrap();

    // An untagged CMYK TIFF, with no working CMYK profile set in this
    // process, is still `image`'s naive conversion, now under the sentence
    // saying so (`untagged_cmyk_working` covers a set one).
    let grid = cmyk_grid();
    let data8: Vec<u8> = grid.iter().flatten().map(|&v| v as u8).collect();
    let bytes = cmyk_tiff(grid.len() as u32, 1, 8, 4, 0, &data8, None);
    let path = write(&dir, "untagged-cmyk.tif", &bytes);
    let expected = image::load_from_memory(&bytes)
        .unwrap()
        .to_rgba8()
        .into_raw();
    assert_eq!(open_pixels(&path), (expected, srgb_profile()));
    assert_eq!(refusal(&path), Some(UNTAGGED_CMYK_UNSET.to_string()));

    // An untagged gray PNG.
    let raw: Vec<u8> = (0..=255u8).collect();
    let bytes = png(256, 1, ExtendedColorType::L8, &raw, None);
    let path = write(&dir, "untagged-gray.png", &bytes);
    let expected = image::load_from_memory(&bytes)
        .unwrap()
        .to_rgba8()
        .into_raw();
    assert_eq!(open_pixels(&path), (expected, srgb_profile()));
    assert_eq!(refusal(&path), None);

    // A storable profile is kept, numbers untouched.
    let rgba: Vec<u8> = (0..64u32 * 4).map(|i| (i * 7 % 256) as u8).collect();
    let bytes = png(64, 1, ExtendedColorType::Rgba8, &rgba, Some(&p3_profile()));
    let path = write(&dir, "p3.png", &bytes);
    assert_eq!(open_pixels(&path), (rgba, p3_profile()));
    assert_eq!(refusal(&path), None);

    // A `.rz` file never carries the sentence.
    let doc = open_doc(&path);
    let native = dir.path().join("kept.rz");
    let c = cpath(&native);
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(doc, c.as_ptr(), &mut err) },
        "{}",
        take_err_string(err)
    );
    unsafe { dr_doc_free(doc) };
    assert_eq!(refusal(&native), None);
}

// -------------------------------------------------------- ignored profiles --

/// Asserts the profiled file opens as the same file without a profile, in
/// sRGB, and reports `sentence` exactly.
fn assert_ignored(
    dir: &TempDir,
    name: &str,
    make: impl Fn(Option<&[u8]>) -> Vec<u8>,
    icc: &[u8],
    sentence: &str,
) {
    let plain = write(dir, &format!("plain-{name}"), &make(None));
    let tagged = write(dir, name, &make(Some(icc)));
    let (expected, _) = open_pixels(&plain);
    assert_eq!(open_pixels(&tagged), (expected, srgb_profile()), "{name}");
    assert_eq!(refusal(&tagged).as_deref(), Some(sentence), "{name}");
}

#[test]
fn profiles_the_open_cannot_use_are_ignored_and_named() {
    let dir = TempDir::new().unwrap();
    let rgb: Vec<u8> = (0..32u32 * 3).map(|i| (i * 11 % 256) as u8).collect();
    let rgb_png = |icc: Option<&[u8]>| png(32, 1, ExtendedColorType::Rgb8, &rgb, icc);
    let gray: Vec<u8> = (0..32u8).map(|i| i * 8).collect();
    let gray_png = |icc: Option<&[u8]>| png(32, 1, ExtendedColorType::L8, &gray, icc);

    // A Gray profile over RGB samples.
    assert_ignored(
        &dir,
        "gray-on-rgb.png",
        rgb_png,
        &gray_profile("Gray Gamma 1.8", curv_gamma(1.8)),
        &format!(
            "The embedded colour profile “Gray Gamma 1.8” is a Gray profile that Doctored \
             cannot convert this image from, {READ_AS_SRGB}"
        ),
    );

    // A CMYK profile over gray samples.
    assert_ignored(
        &dir,
        "cmyk-on-gray.png",
        gray_png,
        &cmyk_profile("Press Test"),
        &format!(
            "The embedded colour profile “Press Test” is a CMYK profile that Doctored cannot \
             convert this image from, {READ_AS_SRGB}"
        ),
    );

    // An RGB device link.
    let link = build(
        [2, 0x10, 0, 0],
        b"link",
        b"RGB ",
        b"RGB ",
        &[(b"desc", tag_desc("Look Link"))],
    );
    assert_ignored(
        &dir,
        "link.png",
        rgb_png,
        &link,
        &format!(
            "The embedded colour profile “Look Link” is a device link, an abstract profile or a \
             named-colour list, not a colour space, {READ_AS_SRGB}"
        ),
    );

    // Bytes that are not an ICC profile at all.
    let garbage: Vec<u8> = (0..200u32).map(|i| (i * 37 % 251) as u8).collect();
    assert_ignored(
        &dir,
        "garbage.png",
        rgb_png,
        &garbage,
        &format!(
            "The embedded colour profile is not an ICC profile Doctored can read, {READ_AS_SRGB}"
        ),
    );

    // A matrix profile past the 16 MiB a document can carry.
    let mut big = p3_profile();
    let declared = 16 * 1024 * 1024 + 4096;
    big.resize(declared, 0);
    big[0..4].copy_from_slice(&(declared as u32).to_be_bytes());
    assert_ignored(
        &dir,
        "big.png",
        rgb_png,
        &big,
        &format!(
            "The embedded colour profile “Display P3” is {declared} bytes, over the 16 MiB a \
             Doctored document can carry, {READ_AS_SRGB}"
        ),
    );
}

// ------------------------------------------------------------- CMYK JPEG --

/// (c, k) ink pairs the JPEG tests read, each its own uniform file.
const JPEG_INKS: [(u8, u8); 5] = [(0, 0), (51, 204), (153, 102), (255, 0), (255, 255)];

/// Every pixel of a uniform CMYK file at the press oracle for (c, k), the
/// document sRGB, and the converted sentence with no platform key.
fn check_uniform_cmyk(path: &Path, (w, h): (u32, u32), (c, k): (u8, u8), what: &str) {
    let doc = open_doc(path);
    assert_eq!(layer_dims(doc, 0), (w, h), "{what}");
    unsafe { dr_doc_free(doc) };
    let (pixels, doc_profile) = open_pixels(path);
    assert_eq!(pixels.len(), (w * h * 4) as usize, "{what}");
    assert_neutral_opaque(&pixels, what);
    let expected = cmyk_oracle(f64::from(c) / 255.0, f64::from(k) / 255.0);
    for px in pixels.chunks_exact(4) {
        assert_close(i32::from(px[0]), expected, &format!("{what} c={c} k={k}"));
    }
    assert_eq!(doc_profile, srgb_profile(), "{what}: the document is sRGB");
    assert_eq!(
        refusal(path).as_deref(),
        Some(converted("Press Test", "CMYK").as_str()),
        "{what}"
    );
    assert_eq!(
        image_open_pixels(path),
        pixels,
        "{what}: dr_image_open agrees"
    );
}

#[test]
fn cmyk_and_ycck_jpegs_convert_through_their_embedded_profile() {
    let dir = TempDir::new().unwrap();
    let profile = cmyk_profile("Press Test");
    for (c, k) in JPEG_INKS {
        // Photoshop's CMYK (APP14 transform 0): every channel stored
        // inverted. m and y are arbitrary — the press table ignores them.
        let adobe = CmykJpeg {
            adobe_transform: Some(0),
            icc: Some(&profile),
            exif: None,
        };
        let path = write(
            &dir,
            &format!("adobe-{c}-{k}.jpg"),
            &cmyk_jpeg(16, 8, [255 - c, 55, 218, 255 - k], &adobe),
        );
        check_uniform_cmyk(&path, (16, 8), (c, k), "Adobe CMYK JPEG");

        // YCCK (transform 2): c = m = y, so Cb = Cr = 128 and Y is the ink;
        // K is stored inverted.
        let ycck = CmykJpeg {
            adobe_transform: Some(2),
            icc: Some(&profile),
            exif: None,
        };
        let path = write(
            &dir,
            &format!("ycck-{c}-{k}.jpg"),
            &cmyk_jpeg(16, 8, [c, 128, 128, 255 - k], &ycck),
        );
        check_uniform_cmyk(&path, (16, 8), (c, k), "YCCK JPEG");

        // No APP14: CMYK stored as ink.
        let plain = CmykJpeg {
            adobe_transform: None,
            icc: Some(&profile),
            exif: None,
        };
        let path = write(
            &dir,
            &format!("plain-{c}-{k}.jpg"),
            &cmyk_jpeg(16, 8, [c, 200, 37, k], &plain),
        );
        check_uniform_cmyk(&path, (16, 8), (c, k), "APP14-less CMYK JPEG");
    }

    // A chromatic YCCK sample pins the chroma terms and the Cb/Cr order:
    // Y 60, Cb 100, Cr 150 is c = round(60 + 1.402·(150 − 128)) = 91 by the
    // JFIF equations (m = 54, y = 10, both unclamped and ignored by the
    // press table). Cb/Cr swapped would give c = 21, chroma dropped c = 60.
    let ycck = CmykJpeg {
        adobe_transform: Some(2),
        icc: Some(&profile),
        exif: None,
    };
    let path = write(
        &dir,
        "ycck-chromatic.jpg",
        &cmyk_jpeg(16, 8, [60, 100, 150, 255 - 51], &ycck),
    );
    check_uniform_cmyk(&path, (16, 8), (91, 51), "chromatic YCCK JPEG");
}

/// zune's default header cap is 16384 pixels a side; the CMYK reader uses
/// `image`'s uncapped options for its header check and its decode alike.
#[test]
fn a_cmyk_jpeg_wider_than_16384_converts() {
    let dir = TempDir::new().unwrap();
    let profile = cmyk_profile("Press Test");
    let extra = CmykJpeg {
        adobe_transform: Some(0),
        icc: Some(&profile),
        exif: None,
    };
    let path = write(
        &dir,
        "wide.jpg",
        &cmyk_jpeg(16400, 1, [255 - 102, 0, 0, 255 - 51], &extra),
    );
    check_uniform_cmyk(&path, (16400, 1), (102, 51), "16400-wide CMYK JPEG");
}

#[test]
fn a_rotated_cmyk_jpeg_opens_upright() {
    let dir = TempDir::new().unwrap();
    let profile = cmyk_profile("Press Test");
    let exif = common::metadata_fixtures::build_exif(false, 6, (72, 72), None, false).bytes;
    let extra = CmykJpeg {
        adobe_transform: Some(0),
        icc: Some(&profile),
        exif: Some(&exif),
    };
    let path = write(
        &dir,
        "exif6.jpg",
        &cmyk_jpeg(16, 8, [255 - 51, 0, 0, 255 - 153], &extra),
    );
    // Stored 16 x 8, a quarter turn: 8 x 16 upright.
    check_uniform_cmyk(&path, (8, 16), (51, 153), "EXIF-6 CMYK JPEG");
}

/// With no working CMYK profile set in this process, an untagged CMYK
/// JPEG reads `image`'s naive numbers, under the sentence saying so.
#[test]
fn an_untagged_cmyk_jpeg_without_a_working_profile_says_so() {
    let dir = TempDir::new().unwrap();
    let extra = CmykJpeg {
        adobe_transform: Some(0),
        icc: None,
        exif: None,
    };
    let bytes = cmyk_jpeg(16, 8, [200, 100, 50, 25], &extra);
    let path = write(&dir, "untagged.jpg", &bytes);
    let expected = image::load_from_memory(&bytes)
        .unwrap()
        .to_rgba8()
        .into_raw();
    assert_eq!(open_pixels(&path), (expected, srgb_profile()));
    assert_eq!(refusal(&path), Some(UNTAGGED_CMYK_UNSET.to_string()));
}

// ----------------------------------------------------- the one CMYK gate --

/// The open's error for `path`, asserting it fails.
fn open_error(path: &Path) -> String {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_doc_open(c.as_ptr(), &mut err) };
    assert!(doc.is_null(), "{path:?} must not open");
    take_err_string(err)
}

/// A profile with no B2A has no black the rule can read, which is
/// lcms's zero black — BPC onto sRGB's zero black is the identity, so the
/// conversion is relative colorimetric uncompensated, under the same
/// sentence.
#[test]
fn a_cmyk_tiff_whose_profile_has_no_b2a_converts_uncompensated() {
    let dir = TempDir::new().unwrap();
    let profile = press_tables("Press A2B", &[b"A2B0", b"A2B1"]);
    let grid = cmyk_grid();
    let data8: Vec<u8> = grid.iter().flatten().map(|&v| v as u8).collect();
    let path = write(
        &dir,
        "a2b-only.tif",
        &cmyk_tiff(grid.len() as u32, 1, 8, 4, 0, &data8, Some(&profile)),
    );
    let (pixels, doc_profile) = open_pixels(&path);
    assert_neutral_opaque(&pixels, "A2B-only");
    for (sample, px) in grid.iter().zip(pixels.chunks_exact(4)) {
        let (c, k) = (f64::from(sample[0]) / 255.0, f64::from(sample[3]) / 255.0);
        let lstar = 10.0 + 90.0 * (1.0 - c) * (1.0 - k);
        assert_close(
            i32::from(px[0]),
            code8(srgb_encode(y_of_lstar(lstar))),
            &format!("A2B-only {sample:?}"),
        );
    }
    let full_ink = pixels.chunks_exact(4).nth(grid.len() - 1).unwrap();
    assert_close(
        i32::from(full_ink[0]),
        code8(srgb_encode(press_black())),
        "full ink stays the press's own black",
    );
    assert_eq!(doc_profile, srgb_profile());
    assert_eq!(
        refusal(&path).as_deref(),
        Some(converted("Press A2B", "CMYK").as_str())
    );
}

/// A B2A-only profile — formerly "ignored" — is
/// not one the open could convert from, and neither is no profile with no
/// working CMYK profile set, so a planar TIFF (which the gate reads since
/// #547 phase 3) is left to the platform by name, gets no report sentence, and the platform's conversion is worded plainly rather
/// than as one from that profile, exactly as for an untagged file.
#[test]
fn a_gate_refused_cmyk_tiff_with_an_unusable_profile_gets_the_plain_sentence() {
    let dir = TempDir::new().unwrap();
    let plane: Vec<u8> = (0..8u8).map(|i| i * 30).collect();
    let planes = [plane.as_slice(); 4];
    let b2a_only = press_tables("Press B2A", &[b"B2A0", b"B2A1"]);
    for (name, icc) in [
        ("b2a-planar.tif", Some(b2a_only.as_slice())),
        ("untagged-planar.tif", None),
    ] {
        let path = write(&dir, name, &planar_cmyk_tiff(8, 1, planes, icc));
        let message = open_error(&path);
        assert!(
            message.contains("a CMYK TIFF with transparency, extra samples or planes is read here only through a CMYK profile it converts from"),
            "{name}: {message}"
        );
        assert_gate_refused_report(&path);
        assert_eq!(platform(&path, CMYK), plain("CMYK"), "{name}");
    }
}

/// #547 phase 3: a CMYK TIFF with unassociated alpha converts its four inks
/// as its opaque twin does and keeps its transparency as the RGBA8 alpha,
/// under the converted sentence.
#[test]
fn a_cmyk_alpha_tiff_converts_with_its_transparency() {
    let dir = TempDir::new().unwrap();
    let profile = cmyk_profile("Press Test");
    let data: Vec<u8> = (0..8u8 * 5).map(|i| i.wrapping_mul(13)).collect();
    let four: Vec<u8> = data.chunks(5).flat_map(|s| s[..4].to_vec()).collect();
    let path = write(
        &dir,
        "cmyka.tif",
        &cmyk_tiff(8, 1, 8, 5, 2, &data, Some(&profile)),
    );
    let twin = write(
        &dir,
        "cmyk.tif",
        &cmyk_tiff(8, 1, 8, 4, 0, &four, Some(&profile)),
    );
    let (pixels, icc) = open_pixels(&path);
    let (opaque, twin_icc) = open_pixels(&twin);
    assert_eq!(icc, twin_icc);
    for (i, (px, twin_px)) in pixels
        .chunks_exact(4)
        .zip(opaque.chunks_exact(4))
        .enumerate()
    {
        assert_eq!(px[..3], twin_px[..3], "pixel {i}: the inks");
        assert_eq!(px[3], data[i * 5 + 4], "pixel {i}: the transparency");
    }
    assert_eq!(
        refusal(&path).as_deref(),
        Some(converted("Press Test", "CMYK").as_str())
    );
    assert_eq!(platform(&path, CMYK), converted("Press Test", "CMYK"));
}

/// The gate refuses what `tiff` would refuse under `image`'s own limits,
/// BEFORE any pixel is read — the files below carry 8 bytes of data.
#[test]
fn cmyk_tiffs_past_the_decode_limits_are_refused_by_the_gate() {
    let dir = TempDir::new().unwrap();
    let profile = cmyk_profile("Press Test");
    let (w, h) = (8200u32, 8200u32);
    let max_alloc: u64 = 512 * 1024 * 1024;

    // 16 bits: the output buffer alone is past `image`'s 512 MiB.
    let output = u64::from(w) * u64::from(h) * 8;
    assert!(output > max_alloc);
    let path = write(
        &dir,
        "big16.tif",
        &cmyk_tiff(w, h, 16, 4, 0, &[0u8; 8], Some(&profile)),
    );
    let message = open_error(&path);
    assert!(
        message.contains(&format!(
            "a {w} x {h} 16-bit CMYK image is {output} bytes, over the {max_alloc}-byte decode \
             limit"
        )),
        "{message}"
    );
    assert_gate_refused_report(&path);
    assert_eq!(platform(&path, CMYK), converted("Press Test", "CMYK"));

    // 8 bits: the output fits, but one strip is larger than what is left.
    let output = u64::from(w) * u64::from(h) * 4;
    let mut bytes = cmyk_tiff(w, h, 8, 4, 0, &[0u8; 8], Some(&profile));
    let entry = [0x17, 0x01, 4, 0, 1, 0, 0, 0, 8, 0, 0, 0];
    let at = bytes
        .windows(12)
        .position(|window| window == entry)
        .expect("the StripByteCounts entry");
    bytes[at + 8..at + 12].copy_from_slice(&(output as u32).to_le_bytes());
    let path = write(&dir, "big-strip.tif", &bytes);
    let message = open_error(&path);
    assert!(
        message.contains(&format!(
            "a {output}-byte strip or tile is over the {} bytes the decode leaves beside the image",
            max_alloc - output
        )),
        "{message}"
    );
    assert_gate_refused_report(&path);
    assert_eq!(platform(&path, CMYK), converted("Press Test", "CMYK"));
}

// ------------------------------------------------------ platform decodes --

/// What a host says about an image ImageIO or CoreGraphics converted to sRGB
/// as it read it: the open's converted sentence only for a profile the open
/// would itself convert such samples from, otherwise the plain sentence for
/// the colour model — and the plain one always for Lab.
#[test]
fn platform_decodes_are_worded_by_the_cores_own_decision() {
    let dir = TempDir::new().unwrap();
    let gamma = gray_profile("Gray Gamma 1.8", curv_gamma(1.8));
    let raw: Vec<u8> = (0..16u8).map(|v| v * 16).collect();
    let profiled = png(16, 1, ExtendedColorType::L8, &raw, Some(&gamma));
    let untagged = png(16, 1, ExtendedColorType::L8, &raw, None);

    // A Gray kTRC over gray samples: the open's own conversion and sentence.
    let path = write(&dir, "gray.png", &profiled);
    assert_eq!(platform(&path, GRAY), converted("Gray Gamma 1.8", "Gray"));
    assert_eq!(
        refusal(&path).as_deref(),
        Some(converted("Gray Gamma 1.8", "Gray").as_str()),
        "the same words the core open reports"
    );
    // No profile at all.
    assert_eq!(platform_bytes(Some(&untagged), GRAY), plain("Gray"));
    // A Gray profile is not a CMYK source.
    assert_eq!(platform_bytes(Some(&profiled), CMYK), plain("CMYK"));
    // Lab is never converted from a profile here, whatever the bytes.
    for bytes in [&profiled, &untagged] {
        assert_eq!(platform_bytes(Some(bytes), LAB), plain("Lab"));
    }
    // NULL and empty bytes are an image with no profile.
    assert_eq!(platform_bytes(None, GRAY), plain("Gray"));
    assert_eq!(platform_bytes(Some(&[]), CMYK), plain("CMYK"));
    // An unknown colour model is an error, not a sentence.
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_platform_conversion_sentence(profiled.as_ptr(), profiled.len(), 7, &mut err) };
    assert!(out.is_null());
    assert_eq!(take_err_string(err), "unknown platform source 7");
}
