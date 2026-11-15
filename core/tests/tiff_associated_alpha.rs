//! A TIFF whose alpha sample is declared ExtraSamples 1 (associated) stores
//! its colour multiplied by alpha, and every flat open of it reads straight
//! colour: RGBA and gray + alpha, 8 and 16 bits, through the C API.
//!
//! `core/tests/fixtures/psd_photoshop/tiff_associated_alpha/` holds
//! Photoshop 26.10.0's own flat (and one layered) TIFFs of a scene with
//! partial transparency (its `README`); `gray/layers*_Ramp.tif` are earlier
//! per-layer files with partial alpha. The oracles share no code with
//! `src/`:
//! - this file's own reader of each file's uncompressed strip, in either byte
//!   order, and the unpremultiply restated in f64: `min(max, round(s · max /
//!   a))`, and 0 where alpha is 0. Every opened colour sample must equal it
//!   exactly, and every opened alpha the stored one;
//! - the scene formula, restated from the runner's declaration
//!   (`photoshop.json`'s `scenes`): the opened colour within what the
//!   store's own error becomes through the division (`scene_bound`);
//! - each file's SHA-256 against `photoshop.json`.
//!
//! Hand-built gray + alpha files (no profile, so every open keeps their
//! codes) carry the rule through the Grayscale document open, the
//! clipboard's gray TIFF and the 16-bit typed RGB open; an unassociated
//! alpha (ExtraSamples 2) is the control, read as stored.

use std::ffi::CString;
use std::path::Path;
use std::ptr;

use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_open, dr_image_pixels_rgba, dr_image_width,
};
use doctored_core::ffi_clipboard::{dr_clipboard_payload_free, dr_clipboard_payload_image};
use doctored_core::ffi_gray::{dr_path_gray_source, dr_precision_doc_open_gray};
use doctored_core::ffi_gray_clipboard::dr_clipboard_payload_from_gray_image;
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_open};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::Value;
use tempfile::TempDir;

use crate::common::psd_photoshop::fixture;
use crate::common::sha256::sha256_hex;
use crate::common::{cpath, take_err_string};
use crate::gray_fixture::gray_tiff;

const FAMILY: &str = "tiff_associated_alpha";
const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const GRAY_SOURCE_TIFF: i32 = 2;

fn manifest() -> Value {
    serde_json::from_slice(&std::fs::read(fixture(FAMILY, "photoshop.json")).unwrap()).unwrap()
}

/// Every take file of `mode` ("rgb" or "gray") at `bits`: (name, its row).
fn files(manifest: &Value, mode: &str, bits: u64) -> Vec<(String, Value)> {
    let out: Vec<(String, Value)> = manifest["files"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, row)| row["mode"] == mode && row["bits"] == bits)
        .map(|(name, row)| (name.clone(), row.clone()))
        .collect();
    assert!(!out.is_empty(), "no {mode}{bits} file in the manifest");
    out
}

// ------------------------------------------------------------ the reader --

/// A classic TIFF's first-directory facts and its strip samples.
struct Stored {
    width: usize,
    height: usize,
    /// Samples a pixel; the alpha is the last.
    samples: usize,
    max: u32,
    values: Vec<u32>,
}

impl Stored {
    fn read(path: &Path) -> Stored {
        let file = std::fs::read(path).unwrap();
        let little = match &file[..4] {
            b"II*\0" => true,
            b"MM\0*" => false,
            _ => panic!("{}: not a classic TIFF", path.display()),
        };
        let u16_at = |at: usize| {
            let b = [file[at], file[at + 1]];
            u32::from(if little {
                u16::from_le_bytes(b)
            } else {
                u16::from_be_bytes(b)
            })
        };
        let u32_at = |at: usize| {
            let b: [u8; 4] = file[at..at + 4].try_into().unwrap();
            if little {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        };
        let ifd = u32_at(4) as usize;
        // A SHORT or LONG array, inline when it fits in four bytes.
        let values = |tag: u32| -> Option<Vec<u32>> {
            let entry = (0..u16_at(ifd) as usize)
                .map(|e| ifd + 2 + e * 12)
                .find(|&at| u16_at(at) == tag)?;
            let (kind, count) = (u16_at(entry + 2), u32_at(entry + 4) as usize);
            let size = if kind == 3 { 2 } else { 4 };
            let at = if size * count <= 4 {
                entry + 8
            } else {
                u32_at(entry + 8) as usize
            };
            Some(
                (0..count)
                    .map(|i| {
                        if kind == 3 {
                            u16_at(at + 2 * i)
                        } else {
                            u32_at(at + 4 * i)
                        }
                    })
                    .collect(),
            )
        };
        let one = |tag: u32| values(tag).unwrap()[0] as usize;
        assert_eq!(one(259), 1, "{}: uncompressed", path.display());
        assert_eq!(values(284).map_or(1, |v| v[0]), 1, "chunky");
        assert_eq!(
            values(338).unwrap()[0],
            1,
            "{}: ExtraSamples 1",
            path.display()
        );
        let bits = values(258).unwrap()[0];
        let strip: Vec<u8> = values(273)
            .unwrap()
            .into_iter()
            .zip(values(279).unwrap())
            .flat_map(|(at, n)| file[at as usize..(at + n) as usize].to_vec())
            .collect();
        let values: Vec<u32> = if bits == 8 {
            strip.iter().map(|&v| u32::from(v)).collect()
        } else {
            strip
                .chunks_exact(2)
                .map(|b| {
                    u32::from(if little {
                        u16::from_le_bytes([b[0], b[1]])
                    } else {
                        u16::from_be_bytes([b[0], b[1]])
                    })
                })
                .collect()
        };
        let (width, height, samples) = (one(256), one(257), one(277));
        assert!(values.len() >= width * height * samples);
        Stored {
            width,
            height,
            samples,
            max: if bits == 8 { 255 } else { 65535 },
            values,
        }
    }

    /// Pixel `p`'s colour samples and its alpha.
    fn pixel(&self, p: usize) -> (Vec<u32>, u32) {
        let at = p * self.samples;
        let colour = self.values[at..at + self.samples - 1].to_vec();
        (colour, self.values[at + self.samples - 1])
    }
}

/// The unpremultiply, restated in f64 (module doc).
fn straight(stored: u32, alpha: u32, max: u32) -> u32 {
    if alpha == 0 {
        return 0;
    }
    (f64::from(stored) * f64::from(max) / f64::from(alpha))
        .round()
        .min(f64::from(max)) as u32
}

// ------------------------------------------------------------- the scene --

fn image16(k: i64) -> i64 {
    (65535 * k + 16384) / 32768
}

/// The runner's declared colour of channel `c` at (x, y).
fn colour(seed: i64, gray: bool, c: i64, x: i64, y: i64, bits: u64) -> u32 {
    let l = i64::from(gray);
    let a = (29 * seed + 61 * l + 97 * c + 11) % 256;
    let b = 1 + (seed + 3 * l + 5 * c) % 13;
    let cc = 1 + (2 * seed + 7 * l + 3 * c) % 11;
    let e = (seed + l + c) % 4;
    if bits == 8 {
        (64 + (a + b * x + cc * y + e * x * y) % 192) as u32
    } else {
        image16(8192 + (127 * a + 59 + 331 * b * x + 523 * cc * y + 97 * e * x * y) % 24577) as u32
    }
}

/// The runner's declared alpha at (x, y).
fn alpha(width: i64, x: i64, y: i64, bits: u64) -> u32 {
    let max = if bits == 8 { 255 } else { 65535 };
    if x < 3 {
        return 0;
    }
    if x >= width - 3 {
        return max;
    }
    let (n, i) = (width - 6, x - 3);
    if bits == 8 {
        (31 + 162 * i / (n - 1) + y) as u32
    } else {
        image16(3932 + 20972 * i / (n - 1) + 113 * y) as u32
    }
}

/// How far an opened colour may sit from the scene's at stored alpha `a`:
/// the store moved a colour by up to `store` codes and the alpha by up to 2
/// (the runner's H2), which the division scales by `max / a`, plus its own
/// rounding.
fn scene_bound(a: u32, max: u32) -> u32 {
    let store = if max == 255 { 1 } else { 8 };
    1 + ((store + 2) * max).div_ceil(a)
}

fn scene_of(manifest: &Value, row: &Value) -> (i64, i64, i64) {
    let scene = &manifest["scenes"][row["scene"].as_str().unwrap()];
    let int = |key: &str| scene[key].as_i64().unwrap();
    (int("width"), int("height"), int("seed"))
}

/// Opened pixels (`rgba`, at `scale` × the file's depth: 1, or 257 for an
/// 8-bit file read at 16 bits) against the stored samples exactly, and
/// against the scene formula when `scene` is given.
fn assert_straight(
    what: &str,
    stored: &Stored,
    rgba: &[[u32; 4]],
    scale: u32,
    scene: Option<((i64, i64, i64), bool, u64)>,
) {
    assert_eq!(rgba.len(), stored.width * stored.height, "{what}: pixels");
    let mut partial = 0;
    for (p, pixel) in rgba.iter().enumerate() {
        let (colour, a) = stored.pixel(p);
        assert_eq!(pixel[3], a * scale, "{what}: alpha at pixel {p}");
        for (c, &s) in colour.iter().enumerate() {
            let want = straight(s, a, stored.max) * scale;
            assert_eq!(
                pixel[c], want,
                "{what}: pixel {p} channel {c}: stored {s} at alpha {a}"
            );
        }
        // A gray file is read R = G = B.
        if colour.len() == 1 {
            assert!(
                pixel[0] == pixel[1] && pixel[1] == pixel[2],
                "{what}: neutral"
            );
        }
        if let Some(((width, _, seed), gray, bits)) = scene {
            let (x, y) = ((p % stored.width) as i64, (p / stored.width) as i64);
            let declared = alpha(width, x, y, bits);
            if a > 0 && a < stored.max {
                partial += 1;
            }
            for (c, &opened) in pixel.iter().enumerate().take(colour.len()) {
                let got = opened / scale;
                if declared == 0 {
                    assert_eq!(got, 0, "{what}: no colour at alpha 0 ({p})");
                    continue;
                }
                let want = colour_at(seed, gray, c, x, y, bits);
                let bound = scene_bound(a, stored.max);
                assert!(
                    got.abs_diff(want) <= bound,
                    "{what}: pixel {p} channel {c}: {got} against the scene's {want} (bound {bound})"
                );
            }
        }
    }
    if scene.is_some() {
        assert!(
            partial >= 100,
            "{what}: {partial} partially transparent pixels"
        );
    }
}

fn colour_at(seed: i64, gray: bool, c: usize, x: i64, y: i64, bits: u64) -> u32 {
    colour(seed, gray, c as i64, x, y, bits)
}

// --------------------------------------------------------------- the opens --

fn open_typed(path: &Path) -> Result<*mut PrecisionDocument, String> {
    let mut err = ptr::null_mut();
    let doc =
        unsafe { dr_precision_doc_open(cpath(path).as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if doc.is_null() {
        return Err(take_err_string(err));
    }
    Ok(doc)
}

fn open_gray(path: &Path) -> Result<*mut PrecisionDocument, String> {
    let working = CString::new("dot_gain_20").unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_gray(
            cpath(path).as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if doc.is_null() {
        return Err(take_err_string(err));
    }
    Ok(doc)
}

/// The one layer's RGBA16 samples, and the document freed.
fn layer_pixels(doc: *mut PrecisionDocument) -> Vec<[u32; 4]> {
    let pixels = unsafe { &*doc }
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap();
    unsafe { dr_precision_doc_free(doc) };
    pixels.iter().map(|p| p.map(u32::from)).collect()
}

/// The byte open's straight RGBA8.
fn byte_pixels(path: &Path) -> Vec<[u32; 4]> {
    let mut err = ptr::null_mut();
    let image = unsafe { dr_image_open(cpath(path).as_ptr(), &mut err) };
    assert!(
        !image.is_null(),
        "{}: {}",
        path.display(),
        take_err_string(err)
    );
    let count = unsafe { dr_image_width(image) * dr_image_height(image) } as usize;
    let bytes = unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(image), count * 4) };
    let pixels = bytes
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]].map(u32::from))
        .collect();
    unsafe { dr_image_free(image) };
    pixels
}

fn gray_source(path: &Path) -> i32 {
    unsafe { dr_path_gray_source(cpath(path).as_ptr()) }
}

// ------------------------------------------------------------------ tests --

/// The runner's own verdicts, as `photoshop.json` records them: straight (S)
/// and white-matted (W) colour miss every judged sample of every file. take1
/// judged its 16-bit files under the 4-code tolerance declared before it,
/// which multiplied colour (P) missed on a few samples by under half a code
/// (UNRESOLVED); later takes judge under the 8-code tolerance declared after
/// take1 (the README).
#[test]
fn every_take_file_is_the_one_its_manifest_describes() {
    let manifest = manifest();
    for (name, row) in manifest["files"].as_object().unwrap() {
        let bytes = std::fs::read(fixture(FAMILY, name)).unwrap();
        assert_eq!(sha256_hex(&bytes), row["sha256"], "{name}");
        let judged = row["judged"].as_u64().unwrap();
        assert!(judged >= 100, "{name}: {judged} judged samples");
        assert_eq!(row["misses"]["S"], judged, "{name}: straight colour");
        assert_eq!(row["misses"]["W"], judged, "{name}: colour matted on white");
        let want = if row["scene"] == "t1" && row["bits"] == 16 {
            "UNRESOLVED"
        } else {
            "CONFIRMED P"
        };
        assert_eq!(row["verdict"], want, "{name}: the take's verdict");
    }
}

#[test]
fn photoshops_rgba16_tiffs_open_flat_with_straight_colour() {
    let manifest = manifest();
    for (name, row) in files(&manifest, "rgb", 16) {
        let path = fixture(FAMILY, &name);
        let stored = Stored::read(&path);
        let doc = open_typed(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let scene = scene_of(&manifest, &row);
        assert_straight(
            &name,
            &stored,
            &layer_pixels(doc),
            1,
            Some((scene, false, 16)),
        );
    }
    // The files the flat open first refused: a layered composite's alpha
    // takes only two values there, so only the exact rule is checked.
    for name in ["s2_16_zip_ibm.tif", "s2_16_zip_mac.tif"] {
        let path = fixture("tiff_little_endian", name);
        let doc = open_typed(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_straight(name, &Stored::read(&path), &layer_pixels(doc), 1, None);
    }
}

#[test]
fn photoshops_gray_alpha_tiffs_open_as_grayscale_documents_with_straight_gray() {
    let manifest = manifest();
    for bits in [8, 16] {
        for (name, row) in files(&manifest, "gray", bits) {
            let path = fixture(FAMILY, &name);
            assert_eq!(gray_source(&path), GRAY_SOURCE_TIFF, "{name}");
            let stored = Stored::read(&path);
            let doc = open_gray(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            let scale = if bits == 8 { 257 } else { 1 };
            let scene = scene_of(&manifest, &row);
            assert_straight(
                &name,
                &stored,
                &layer_pixels(doc),
                scale,
                Some((scene, true, bits)),
            );
        }
    }
    // The gray-files take's per-layer files (`gray_psd_photoshop.rs`).
    for (name, scale) in [("layers16_Ramp.tif", 1), ("layers8_Ramp.tif", 257)] {
        let path = fixture("gray", name);
        assert_eq!(gray_source(&path), GRAY_SOURCE_TIFF, "{name}");
        let doc = open_gray(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_straight(name, &Stored::read(&path), &layer_pixels(doc), scale, None);
    }
}

#[test]
fn the_byte_open_reads_associated_colour_straight_at_8_and_16_bits() {
    let manifest = manifest();
    for (name, row) in files(&manifest, "rgb", 8) {
        let path = fixture(FAMILY, &name);
        let scene = scene_of(&manifest, &row);
        assert_straight(
            &name,
            &Stored::read(&path),
            &byte_pixels(&path),
            1,
            Some((scene, false, 8)),
        );
    }
    // A 16-bit file read at 8 bits: the straight colour, then narrowed.
    for (name, _) in files(&manifest, "rgb", 16) {
        let path = fixture(FAMILY, &name);
        let stored = Stored::read(&path);
        let narrow = |v: u32| (f64::from(v) / 257.0).round() as u32;
        for (p, pixel) in byte_pixels(&path).iter().enumerate() {
            let (colour, a) = stored.pixel(p);
            assert_eq!(pixel[3], narrow(a), "{name}: alpha at {p}");
            for (c, &s) in colour.iter().enumerate() {
                assert_eq!(pixel[c], narrow(straight(s, a, 65535)), "{name}: {p} {c}");
            }
        }
    }
}

/// A gray + alpha TIFF with no profile, whose codes every open keeps.
fn hand_gray(bits: u16, extra: u16) -> (Vec<u8>, Stored) {
    let (w, h) = (16u32, 5u32);
    let max = if bits == 8 { 255u32 } else { 65535 };
    let mut samples = Vec::new();
    for y in 0..h {
        for x in 0..w {
            // Alpha 0 on the first column, full on the last, between them
            // a ramp; a straight gray that varies across both axes.
            let a = max * x / (w - 1);
            let gray = (max / 4) + (max / 2) * (x + 3 * y) / (w + 3 * h);
            samples.push(if extra == 1 {
                (gray * a + max / 2) / max
            } else {
                gray
            });
            samples.push(a);
        }
    }
    let data: Vec<u8> = if bits == 8 {
        samples.iter().map(|&v| v as u8).collect()
    } else {
        samples
            .iter()
            .flat_map(|&v| (v as u16).to_le_bytes())
            .collect()
    };
    let file = gray_tiff(w, h, bits, 2, 1, Some(extra), None, &data, None);
    let stored = Stored {
        width: w as usize,
        height: h as usize,
        samples: 2,
        max,
        values: samples,
    };
    (file, stored)
}

#[test]
fn a_hand_built_gray_alpha_tiff_with_associated_alpha_opens_straight_everywhere() {
    let dir = TempDir::new().unwrap();
    for bits in [8u16, 16] {
        let (file, stored) = hand_gray(bits, 1);
        let path = dir.path().join(format!("graya{bits}.tif"));
        std::fs::write(&path, &file).unwrap();
        let scale = if bits == 8 { 257 } else { 1 };
        let what = format!("{bits}-bit");
        // The Grayscale document open.
        assert_eq!(gray_source(&path), GRAY_SOURCE_TIFF, "{what}");
        let doc = open_gray(&path).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert_straight(
            &format!("{what} gray"),
            &stored,
            &layer_pixels(doc),
            scale,
            None,
        );
        // The clipboard's gray TIFF, read by the same decode.
        let working = CString::new("gray_gamma_22").unwrap();
        let (mut assigned, mut err) = (ptr::null_mut(), ptr::null_mut());
        let payload = unsafe {
            dr_clipboard_payload_from_gray_image(
                file.as_ptr(),
                file.len(),
                ptr::null(),
                0,
                working.as_ptr(),
                LIMIT,
                0,
                &mut assigned,
                &mut err,
            )
        };
        assert!(!payload.is_null(), "{what}: {}", take_err_string(err));
        take_err_string(assigned);
        let image = unsafe { dr_clipboard_payload_image(payload) };
        assert!(!image.is_null());
        let image = unsafe { Box::from_raw(image) };
        let pixels: Vec<[u32; 4]> = image
            .to_rgba16()
            .unwrap()
            .iter()
            .map(|p| p.map(u32::from))
            .collect();
        assert_straight(&format!("{what} clipboard"), &stored, &pixels, scale, None);
        unsafe { dr_clipboard_payload_free(payload) };
        // The typed RGB open (no profile: the codes are copied) reads 16 bits
        // only; the byte open's decoder reads no gray + alpha TIFF.
        if bits == 16 {
            let doc = open_typed(&path).unwrap_or_else(|e| panic!("{what}: {e}"));
            assert_straight(
                &format!("{what} typed"),
                &stored,
                &layer_pixels(doc),
                1,
                None,
            );
        }
    }
}

#[test]
fn an_unassociated_alpha_is_read_as_it_is_stored() {
    let dir = TempDir::new().unwrap();
    for bits in [8u16, 16] {
        let (file, stored) = hand_gray(bits, 2);
        let path = dir.path().join(format!("gray{bits}.tif"));
        std::fs::write(&path, &file).unwrap();
        let scale = if bits == 8 { 257 } else { 1 };
        let doc = open_gray(&path).unwrap();
        let pixels = layer_pixels(doc);
        for (p, pixel) in pixels.iter().enumerate() {
            let (colour, a) = stored.pixel(p);
            assert_eq!(
                pixel[0],
                colour[0] * scale,
                "{bits}-bit straight gray at {p}"
            );
            assert_eq!(pixel[3], a * scale);
        }
    }
}
