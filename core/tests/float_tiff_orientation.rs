//! A 32-bit float TIFF stored rotated or mirrored (Orientation 2 to 8) opens
//! upright, as every other flat TIFF open does (`float_io::from_float_tiff`
//! through `pixel_orientation`): the canvas swaps for a quarter turn, every
//! sample lands where the orientation says, and the resolution's axes swap
//! with the canvas. One, two, three and four samples, untagged (an RGB float
//! document) and under a Gray profile (a 32-bit Gray document), all through
//! `dr_precision_doc_open`.
//!
//! The file is assembled here (one uncompressed strip of little-endian f32,
//! every sample distinct), not by the crate's writer. The oracle places each
//! STORED pixel by the TIFF 6.0 definition of tag 274, which names the
//! visual side the 0th row and the 0th column sit on (`SIDES`), a forward
//! placement written apart from the crate's inverse lookup.
//!
//! A turned document's 32-bit export states Orientation 1 and reopens as the
//! same document (a translucent colour sample within the render's 2 ulp),
//! its flat-state list empty.

use std::ffi::CString;
use std::ptr;

use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_export::dr_precision_doc_export_encode;
use doctored_core::ffi_precision::dr_precision_doc_open;
use doctored_core::ffi_preservation::dr_precision_doc_flat_state_losses;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES as LIMIT;
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_fixture::gray_icc;

/// The stored raster: 3 × 2, so every quarter turn changes the canvas.
const SW: usize = 3;
const SH: usize = 2;
/// The stored resolution, unequal so a swap shows.
const X_PPI: u32 = 300;
const Y_PPI: u32 = 150;

/// The visual side of the 0th row and of the 0th column, per Orientation
/// 1–8 (TIFF 6.0, tag 274).
#[derive(Clone, Copy)]
enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

const SIDES: [(Side, Side); 8] = [
    (Side::Top, Side::Left),
    (Side::Top, Side::Right),
    (Side::Bottom, Side::Right),
    (Side::Bottom, Side::Left),
    (Side::Left, Side::Top),
    (Side::Right, Side::Top),
    (Side::Right, Side::Bottom),
    (Side::Left, Side::Bottom),
];

/// Stored sample `c` of stored pixel `(r, k)` (row, column): distinct across
/// the whole file, alpha in (0, 1].
fn stored_sample(r: usize, k: usize, c: usize, channels: usize) -> f32 {
    let pixel = (r * SW + k) as f32;
    let is_alpha = channels == 2 && c == 1 || channels == 4 && c == 3;
    if is_alpha {
        0.3 + 0.1 * pixel
    } else {
        0.05 + pixel * 0.25 + c as f32 * 0.0625
    }
}

/// The document's RGBA of a stored pixel: one or two samples are gray.
fn stored_rgba(r: usize, k: usize, channels: usize) -> [f32; 4] {
    let s = |c| stored_sample(r, k, c, channels);
    match channels {
        1 => [s(0), s(0), s(0), 1.0],
        2 => [s(0), s(0), s(0), s(1)],
        3 => [s(0), s(1), s(2), 1.0],
        _ => [s(0), s(1), s(2), s(3)],
    }
}

/// The upright raster, built by placing each stored pixel on the side its
/// row and column name.
fn oracle(orientation: u8, channels: usize) -> (usize, usize, Vec<[f32; 4]>) {
    let (rows, cols) = SIDES[usize::from(orientation) - 1];
    let quarter = matches!(rows, Side::Left | Side::Right);
    let (w, h) = if quarter { (SH, SW) } else { (SW, SH) };
    let mut out = vec![[f32::NAN; 4]; w * h];
    for r in 0..SH {
        for k in 0..SW {
            let (mut x, mut y) = (None, None);
            match rows {
                Side::Top => y = Some(r),
                Side::Bottom => y = Some(h - 1 - r),
                Side::Left => x = Some(r),
                Side::Right => x = Some(w - 1 - r),
            }
            match cols {
                Side::Top => y = Some(k),
                Side::Bottom => y = Some(h - 1 - k),
                Side::Left => x = Some(k),
                Side::Right => x = Some(w - 1 - k),
            }
            let (x, y) = (x.unwrap(), y.unwrap());
            out[y * w + x] = stored_rgba(r, k, channels);
        }
    }
    (w, h, out)
}

/// One IFD entry: tag, type, count, and the value or offset bytes.
fn entry(out: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: u32) {
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&value.to_le_bytes());
}

/// A little-endian float TIFF of the stored raster at `orientation`, with
/// the resolution and, when given, an ICC profile.
fn float_tiff(channels: usize, orientation: u8, icc: Option<&[u8]>) -> Vec<u8> {
    let mut strip = Vec::new();
    for r in 0..SH {
        for k in 0..SW {
            for c in 0..channels {
                strip.extend_from_slice(&stored_sample(r, k, c, channels).to_le_bytes());
            }
        }
    }
    // Header, then the strip, the out-of-line values and the IFD.
    let mut file = b"II\x2a\x00\0\0\0\0".to_vec();
    let strip_at = file.len() as u32;
    file.extend_from_slice(&strip);
    let bits_at = file.len() as u32;
    for _ in 0..channels {
        file.extend_from_slice(&32u16.to_le_bytes());
    }
    let format_at = file.len() as u32;
    for _ in 0..channels {
        file.extend_from_slice(&3u16.to_le_bytes());
    }
    let x_at = file.len() as u32;
    file.extend_from_slice(&X_PPI.to_le_bytes());
    file.extend_from_slice(&1u32.to_le_bytes());
    let y_at = file.len() as u32;
    file.extend_from_slice(&Y_PPI.to_le_bytes());
    file.extend_from_slice(&1u32.to_le_bytes());
    let icc_at = file.len() as u32;
    if let Some(icc) = icc {
        file.extend_from_slice(icc);
    }
    if file.len() % 2 == 1 {
        file.push(0);
    }
    let ifd_at = file.len() as u32;
    file[4..8].copy_from_slice(&ifd_at.to_le_bytes());
    // A SHORT array of one or two values fits in the value field.
    let shorts = |values: &[u16]| {
        let mut field = [0u8; 4];
        for (i, v) in values.iter().take(2).enumerate() {
            field[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
        u32::from_le_bytes(field)
    };
    let n = channels as u32;
    let mut entries = Vec::new();
    let mut count = 0u16;
    let mut add = |tag: u16, kind: u16, cnt: u32, value: u32| {
        entry(&mut entries, tag, kind, cnt, value);
        count += 1;
    };
    add(256, 3, 1, SW as u32);
    add(257, 3, 1, SH as u32);
    add(
        258,
        3,
        n,
        if channels <= 2 {
            shorts(&vec![32; channels])
        } else {
            bits_at
        },
    );
    add(259, 3, 1, 1);
    add(262, 3, 1, if channels <= 2 { 1 } else { 2 });
    add(273, 4, 1, strip_at);
    add(274, 3, 1, u32::from(orientation));
    add(277, 3, 1, n);
    add(278, 3, 1, SH as u32);
    add(279, 4, 1, strip.len() as u32);
    add(282, 5, 1, x_at);
    add(283, 5, 1, y_at);
    add(284, 3, 1, 1);
    add(296, 3, 1, 2);
    if channels == 2 || channels == 4 {
        add(338, 3, 1, 2); // unassociated alpha
    }
    add(
        339,
        3,
        n,
        if channels <= 2 {
            shorts(&vec![3; channels])
        } else {
            format_at
        },
    );
    if let Some(icc) = icc {
        add(34675, 7, icc.len() as u32, icc_at);
    }
    file.extend_from_slice(&count.to_le_bytes());
    file.extend_from_slice(&entries);
    file.extend_from_slice(&0u32.to_le_bytes());
    file
}

/// IFD0's Orientation in a little-endian TIFF, read with nothing of the
/// crate's.
fn orientation_of(file: &[u8]) -> Option<u16> {
    assert_eq!(&file[..2], b"II", "the export is little-endian");
    let ifd = u32::from_le_bytes(file[4..8].try_into().unwrap()) as usize;
    let entries = usize::from(u16::from_le_bytes([file[ifd], file[ifd + 1]]));
    (0..entries).map(|e| ifd + 2 + e * 12).find_map(|at| {
        (u16::from_le_bytes([file[at], file[at + 1]]) == 274)
            .then(|| u16::from_le_bytes([file[at + 8], file[at + 9]]))
    })
}

fn open(path: &std::path::Path) -> Result<Box<PrecisionDocument>, String> {
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_open(cpath(path).as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(unsafe { Box::from_raw(out) })
    }
}

fn samples(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    match doc.layer_source(0).expect("one layer").to_rgba32() {
        Ok(samples) => samples,
        Err(_) => panic!("the layer holds no float samples"),
    }
}

/// How far apart two f32 samples are, in units in the last place.
fn ulps(a: f32, b: f32) -> u32 {
    (a.to_bits() as i64 - b.to_bits() as i64).unsigned_abs() as u32
}

/// `reopened` is `original` as the export writes it: alpha bit-exact, and a
/// colour sample bit-exact where alpha is 1, else within 2 ulp — the render
/// the export encodes composites a translucent pixel through one
/// multiplication and one division by its alpha (`float_io`'s OpenEXR round
/// trip states the same bound).
fn same_within_render(reopened: &[[f32; 4]], original: &[[f32; 4]]) -> bool {
    reopened.len() == original.len()
        && reopened.iter().zip(original).all(|(r, o)| {
            r[3].to_bits() == o[3].to_bits()
                && (0..3).all(|c| {
                    if o[3] == 1.0 {
                        r[c].to_bits() == o[c].to_bits()
                    } else {
                        ulps(r[c], o[c]) <= 2
                    }
                })
        })
}

fn flat_tiff_losses(doc: &PrecisionDocument) -> Vec<String> {
    let mut err = ptr::null_mut();
    let json = unsafe { dr_precision_doc_flat_state_losses(doc, true, true, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let text = unsafe { std::ffi::CStr::from_ptr(json) }
        .to_str()
        .unwrap()
        .to_owned();
    unsafe { dr_string_free(json) };
    serde_json::from_str(&text).unwrap()
}

fn export_32(doc: &PrecisionDocument) -> Vec<u8> {
    let request = CString::new(r#"{"format":"tiff","profile":"document","bit_depth":32}"#).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
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
    assert!(!out.is_null(), "export: {}", take_err_string(err));
    let result: Box<ExportResult> = unsafe { Box::from_raw(out) };
    result.bytes().to_vec()
}

#[test]
fn a_float_tiff_stored_in_any_orientation_opens_upright() {
    let dir = TempDir::new().unwrap();
    let icc = gray_icc("Orientation Gray", 2.2);
    let cases: [(usize, bool); 5] = [(1, false), (1, true), (2, true), (3, false), (4, false)];
    let mut failures = Vec::new();
    for (channels, tagged) in cases {
        for orientation in 1..=8u8 {
            let tag = format!(
                "{channels} sample(s){}, Orientation {orientation}",
                if tagged { " under a Gray profile" } else { "" }
            );
            let path = dir
                .path()
                .join(format!("o{orientation}-{channels}-{tagged}.tif"));
            let file = float_tiff(channels, orientation, tagged.then_some(&icc[..]));
            std::fs::write(&path, &file).unwrap();
            let doc = match open(&path) {
                Ok(doc) => doc,
                Err(e) => {
                    failures.push(format!("{tag}: the open refused: {e}"));
                    continue;
                }
            };
            if !doc.is_float() || doc.is_gray() != tagged {
                failures.push(format!(
                    "{tag}: opened float {} gray {}",
                    doc.is_float(),
                    doc.is_gray()
                ));
            }
            let (w, h, want) = oracle(orientation, channels);
            if doc.dimensions() != (w as u32, h as u32) {
                failures.push(format!(
                    "{tag}: canvas {:?}, want {w}×{h}",
                    doc.dimensions()
                ));
                continue;
            }
            let got = samples(&doc);
            let same = got.len() == want.len()
                && got
                    .iter()
                    .zip(&want)
                    .all(|(g, e)| g.iter().zip(e).all(|(a, b)| a.to_bits() == b.to_bits()));
            if !same {
                failures.push(format!("{tag}: samples {got:?}, want {want:?}"));
            }
            let quarter = w != SW;
            let (x, y) = if quarter {
                (Y_PPI, X_PPI)
            } else {
                (X_PPI, Y_PPI)
            };
            let resolution = doc.resolution();
            if (resolution.x, resolution.y) != (x as f32, y as f32) {
                failures.push(format!(
                    "{tag}: resolution {}×{}, want {x}×{y}",
                    resolution.x, resolution.y
                ));
            }
            // Written upright, the copy states Orientation 1 and reopens as
            // the same document.
            let copy = export_32(&doc);
            if orientation_of(&copy) != Some(1) {
                failures.push(format!(
                    "{tag}: the export states Orientation {:?}",
                    orientation_of(&copy)
                ));
            }
            let again_path = dir
                .path()
                .join(format!("again-{orientation}-{channels}-{tagged}.tif"));
            std::fs::write(&again_path, &copy).unwrap();
            match open(&again_path) {
                Ok(again) => {
                    if again.dimensions() != doc.dimensions()
                        || !same_within_render(&samples(&again), &got)
                        || again.is_gray() != doc.is_gray()
                        || again.resolution().x != resolution.x
                        || again.resolution().y != resolution.y
                    {
                        failures.push(format!(
                            "{tag}: the export reopens otherwise: {:?} gray {} {:?} {:?}",
                            again.dimensions(),
                            again.is_gray(),
                            again.resolution(),
                            samples(&again)
                        ));
                    }
                }
                Err(e) => failures.push(format!("{tag}: the export reopen refused: {e}")),
            }
            let losses = flat_tiff_losses(&doc);
            if !losses.is_empty() {
                failures.push(format!("{tag}: a flat TIFF loses {losses:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
