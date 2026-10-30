//! The CMYK paste tests' SECOND synthetic press profile and the CMYK → CMYK
//! oracle (#1 phase 4c), written from the ICC specification and the CIE
//! formulas beside `common/cmyk_fixture.rs` (profile A), never read off the
//! implementation.
//!
//! Profile B is built exactly like A — a v2 `prtr` CMYK profile with a Lab
//! PCS and `mft2` tables on a 2-node grid, every table AFFINE with INTEGER
//! corner codes, so every interpolation returns the affine function — with
//! other coefficients:
//!
//! * `A2B0` = `A2B1`: `L = 65280 − 11000c − 14000m − 7000y − 26000k`,
//!   `a = 32768 − 5000c + 6000m`, `b = 32768 − 3000c − 2000m + 7000y`;
//! * `B2A1`: `C = 28000(1 − l) + 14000(1 − a)`, `M = 26000(1 − l) + 10000a`,
//!   `Y = 32000(1 − l) + 11000b`, `K = 24000(1 − l)`;
//! * `B2A0`: every ink `65535(1 − l)`.
//!
//! The black points, from the tables (`common/cmyk_fixture.rs` derives A's
//! the same way): each profile's SOURCE black is lcms's perceptual black —
//! Lab 0 through `B2A0` is full ink, back through `A2B1` its L* (A 8.0, B
//! 11.2), neutral and clipped to 50; each profile's DESTINATION black at
//! relative colorimetric is zero, because its `B2A1` → `A2B1` round trip of a
//! neutral L* ramp is affine and far from straight (B's runs from L* 60 to
//! 96), so lcms's shadow fit over the normalized ramp — exactly L* / 100 —
//! has its root at L* = 0.
//!
//! The CMYK → CMYK oracle is then ISO 18619 in closed form: the source's
//! `A2B` to Lab, Lab → XYZ, the BPC affine from the source's black to zero
//! (`XYZ' = D50·(XYZ − s) / (D50 − s)` per component), XYZ → Lab, the legacy
//! lut16 Lab encoding, the destination's `B2A1`.
#![allow(dead_code)]

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;

use crate::cmyk_fixture::{b2a1_codes, ink_lab, lab_to_xyz, source_black, xyz_to_lab, D50};
use crate::cmyk_structure_fixture::{free, RgbLayer, LIMIT};
use crate::common::take_err_string;
use crate::icc_lut_builders::{build, tag_desc, tag_mft2, tag_wtpt};

/// Profile B's A2B legacy Lab codes for ink in [0, 1].
pub fn b_a2b_codes(d: [f64; 4]) -> [f64; 3] {
    [
        65280.0 - 11000.0 * d[0] - 14000.0 * d[1] - 7000.0 * d[2] - 26000.0 * d[3],
        32768.0 - 5000.0 * d[0] + 6000.0 * d[1],
        32768.0 - 3000.0 * d[0] - 2000.0 * d[1] + 7000.0 * d[2],
    ]
}

/// Profile B's B2A1 ink codes for normalized legacy Lab in [0, 1].
pub fn b_b2a1_codes(lab: [f64; 3]) -> [f64; 4] {
    let (l, a, b) = (lab[0], lab[1], lab[2]);
    [
        28000.0 * (1.0 - l) + 14000.0 * (1.0 - a),
        26000.0 * (1.0 - l) + 10000.0 * a,
        32000.0 * (1.0 - l) + 11000.0 * b,
        24000.0 * (1.0 - l),
    ]
}

/// Profile B's bytes, named `name`.
pub fn second_press_profile(name: &str) -> Vec<u8> {
    let a2b = tag_mft2(4, 3, 2, &|d: &[f64]| {
        b_a2b_codes([d[0], d[1], d[2], d[3]])
            .iter()
            .map(|v| v / 65535.0)
            .collect()
    });
    let b2a1 = tag_mft2(3, 4, 2, &|lab: &[f64]| {
        b_b2a1_codes([lab[0], lab[1], lab[2]])
            .iter()
            .map(|v| v / 65535.0)
            .collect()
    });
    let b2a0 = tag_mft2(3, 4, 2, &|lab: &[f64]| vec![1.0 - lab[0]; 4]);
    build(
        [2, 0x10, 0, 0],
        b"prtr",
        b"CMYK",
        b"Lab ",
        &[
            (b"desc", tag_desc(name)),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", a2b.clone()),
            (b"A2B1", a2b),
            (b"B2A0", b2a0),
            (b"B2A1", b2a1),
        ],
    )
}

/// Ink → profile B's own Lab (relative, D50).
pub fn ink_lab_b(ink: [f64; 4]) -> [f64; 3] {
    let c = b_a2b_codes(ink);
    [
        c[0] / 65280.0 * 100.0,
        c[1] / 256.0 - 128.0,
        c[2] / 256.0 - 128.0,
    ]
}

/// Profile B's source black (module doc): full ink's L*, neutral, ≤ 50.
pub fn b_source_black() -> [f64; 3] {
    lab_to_xyz([ink_lab_b([1.0; 4])[0].min(50.0), 0.0, 0.0])
}

/// The BPC affine from `black` to zero.
fn compensated(xyz: [f64; 3], black: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| D50[i] * (xyz[i] - black[i]) / (D50[i] - black[i]))
}

/// Lab as the lut16 tables' normalized legacy encoding, clamped.
fn encoded(lab: [f64; 3]) -> [f64; 3] {
    [
        (lab[0] / 100.0 * 65280.0 / 65535.0).clamp(0.0, 1.0),
        ((lab[1] + 128.0) * 256.0 / 65535.0).clamp(0.0, 1.0),
        ((lab[2] + 128.0) * 256.0 / 65535.0).clamp(0.0, 1.0),
    ]
}

/// Ink in profile A → ink in profile B (module doc), each in [0, 1].
pub fn a_to_b(ink: [f64; 4]) -> [f64; 4] {
    let xyz = compensated(lab_to_xyz(ink_lab(ink)), source_black());
    b_b2a1_codes(encoded(xyz_to_lab(xyz))).map(|v| (v / 65535.0).clamp(0.0, 1.0))
}

/// Ink in profile B → ink in profile A (module doc).
pub fn b_to_a(ink: [f64; 4]) -> [f64; 4] {
    let xyz = compensated(lab_to_xyz(ink_lab_b(ink)), b_source_black());
    b2a1_codes(encoded(xyz_to_lab(xyz))).map(|v| (v / 65535.0).clamp(0.0, 1.0))
}

/// A `w × h` CMYK document in the CMYK profile `profile`, converted from an
/// RGB document of `layers` (`cmyk_structure_fixture::cmyk_layers`' shape),
/// declared at `depth` bits.
pub fn cmyk_layers_in(
    profile: &[u8],
    depth: u32,
    w: u32,
    h: u32,
    layers: Vec<RgbLayer>,
) -> *mut PrecisionDocument {
    let srgb = IccProfile::srgb();
    let records = layers
        .into_iter()
        .map(|layer| {
            let image =
                PrecisionImage::from_rgba16(layer.w, layer.h, layer.pixels, srgb.clone(), None)
                    .unwrap();
            PrecisionDocumentLayer::raster(layer.name, image, layer.offset, 1.0, true).unwrap()
        })
        .collect();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            w,
            h,
            srgb,
            Metadata::default(),
            Resolution::default(),
            records,
        )
        .unwrap(),
    ));
    let options = CString::new(format!(r#"{{"depth":{depth}}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            rgb,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to CMYK: {}", take_err_string(err));
    free(rgb);
    out
}
