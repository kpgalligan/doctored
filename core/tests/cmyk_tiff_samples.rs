//! THE CMYK TIFF sample reader (#1 phase 3, `cmyk_tiff_read`): chunky and
//! planar files at 8 and 16 bits, with no transparency, associated or
//! unassociated alpha, and extra samples past it, each built by
//! `common/cmyk_tiff_builder.rs` (from the TIFF 6.0 specification, not from
//! any decoder here). The CMYK document's ink and alpha are checked against
//! the planes as written — associated ink unpremultiplied by this file's own
//! rational arithmetic — the extra samples' warning by name, and the flat
//! sRGB open's alpha against the file's own transparency.

use std::path::Path;
use std::ptr;

use doctored_core::cmyk_image::CmykSample;
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_open};
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_info, dr_precision_doc_open,
};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_tiff_builder::cmyk_tiff_planes;
use crate::common::{cpath, layer_pixels, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 7;
const H: u32 = 5;

/// `count` deterministic samples at `bits`, including 0 and the maximum.
fn plane(seed: u32, bits: u16, count: usize) -> Vec<u16> {
    let max = if bits == 8 { 255u32 } else { 65535 };
    (0..count as u32)
        .map(|i| match (i + seed) % 7 {
            0 => 0,
            1 => max as u16,
            _ => ((i * 7919 + seed * 104_729) % (max + 1)) as u16,
        })
        .collect()
}

fn open_cmyk(path: &Path) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let doc =
        unsafe { dr_precision_doc_open(cpath(path).as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(
        !doc.is_null(),
        "{}: {}",
        path.display(),
        take_err_string(err)
    );
    doc
}

fn warnings(doc: *const PrecisionDocument) -> Vec<String> {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_info(doc, &mut err) };
    let info: Value = serde_json::from_str(&take_err_string(text)).unwrap();
    info["import_warnings"]
        .as_array()
        .map(|w| w.iter().map(|v| v.as_str().unwrap().to_string()).collect())
        .unwrap_or_default()
}

/// The straight CMYKA16 sample a reader owes for written planes (module
/// doc): associated ink `round(v · max / a)` capped at `max`, ink 0 at
/// alpha 0, every value ×257 at 8 bits.
fn expected(planes: &[Vec<u16>], i: usize, bits: u16, alpha: Option<u16>) -> CmykSample {
    let max = if bits == 8 { 255.0 } else { 65535.0 };
    let scale: u32 = if bits == 8 { 257 } else { 1 };
    let a = alpha.map_or(max, |_| f64::from(planes[4][i]));
    let mut out = [0u16; 5];
    for c in 0..4 {
        let v = f64::from(planes[c][i]);
        let straight = match alpha {
            Some(1) if a == 0.0 => 0.0,
            Some(1) => (v * max / a).round().min(max),
            _ => v,
        };
        out[c] = (straight as u32 * scale) as u16;
    }
    out[4] = (a as u32 * scale) as u16;
    out
}

#[test]
fn every_cmyk_tiff_layout_reads_its_ink_and_transparency() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Test");
    let count = (W * H) as usize;
    for bits in [8u16, 16] {
        for planar in [false, true] {
            for alpha in [None, Some(1u16), Some(2)] {
                for extras in [0usize, 2] {
                    let mut planes: Vec<Vec<u16>> = (0..4).map(|c| plane(c, bits, count)).collect();
                    let mut tag: Vec<u16> = Vec::new();
                    if let Some(kind) = alpha {
                        planes.push(plane(11, bits, count));
                        tag.push(kind);
                    }
                    for e in 0..extras {
                        planes.push(plane(20 + e as u32, bits, count));
                        tag.push(0);
                    }
                    let name = format!("t{bits}-{planar}-{alpha:?}-{extras}.tif");
                    let path = dir.path().join(&name);
                    std::fs::write(
                        &path,
                        cmyk_tiff_planes(W, H, bits, planar, &tag, &planes, Some(&profile)),
                    )
                    .unwrap();
                    let doc = open_cmyk(&path);
                    let got = unsafe { &*doc }.cmyk_layer_samples(0).unwrap();
                    for (i, sample) in got.iter().enumerate() {
                        assert_eq!(
                            *sample,
                            expected(&planes, i, bits, alpha),
                            "{name}: pixel {i}"
                        );
                    }
                    let sentence = format!("This CMYK TIFF has {extras} extra samples (spot or alpha channels) beyond its transparency; they are not read.");
                    assert_eq!(
                        warnings(doc).contains(&sentence),
                        extras > 0,
                        "{name}: {:?}",
                        warnings(doc)
                    );
                    unsafe { dr_precision_doc_free(doc) };
                    // The flat sRGB open reads the same transparency.
                    let mut err = ptr::null_mut();
                    let flat =
                        unsafe { dr_doc_open(cpath(&path).as_ptr(), std::ptr::null(), &mut err) };
                    assert!(!flat.is_null(), "{name}: {}", take_err_string(err));
                    let rgba = layer_pixels(flat, 0);
                    for (i, px) in rgba.chunks_exact(4).enumerate() {
                        let a16 = expected(&planes, i, bits, alpha)[4];
                        assert_eq!(
                            u32::from(px[3]),
                            (u32::from(a16) + 128) / 257,
                            "{name}: alpha {i}"
                        );
                    }
                    unsafe { dr_doc_free(flat) };
                }
            }
        }
    }
}

#[test]
fn a_planar_tiff_missing_a_strip_set_refuses_by_name() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Test");
    let count = (W * H) as usize;
    let planes: Vec<Vec<u16>> = (0..4).map(|c| plane(c, 8, count)).collect();
    // A chunky one-strip file whose PlanarConfiguration says 2.
    let mut bytes = cmyk_tiff_planes(W, H, 8, false, &[], &planes, Some(&profile));
    let directory = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let entries = usize::from(u16::from_le_bytes([bytes[directory], bytes[directory + 1]]));
    for i in 0..entries {
        let at = directory + 2 + i * 12;
        if u16::from_le_bytes([bytes[at], bytes[at + 1]]) == 284 {
            bytes[at + 8] = 2;
        }
    }
    let path = dir.path().join("short-planar.tif");
    std::fs::write(&path, &bytes).unwrap();
    // The gate refuses it by name, so it is no CMYK source and the flat
    // open fails with that sentence.
    assert_eq!(
        unsafe { doctored_core::ffi_cmyk::dr_path_cmyk_source(cpath(&path).as_ptr()) },
        0
    );
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_doc_open(cpath(&path).as_ptr(), std::ptr::null(), &mut err) };
    assert!(doc.is_null());
    let message = take_err_string(err);
    assert!(
        message.contains("a planar CMYK TIFF needs one strip or tile set per sample"),
        "{message}"
    );
}
