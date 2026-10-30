//! The colour picker's value-only Lab pair, `dr_srgb_to_lab` and
//! `dr_lab_to_srgb`, exercised through the C ABI.
//!
//! The forward direction is pinned twice: against `dr_doc_lab` on an
//! untagged (sRGB) document — the Info panel's readout, so the picker and
//! the panel must agree — and against an independent CIE oracle written
//! here from the published sRGB D50 matrix. The inverse is pinned by 8-bit
//! round trips and by colours whose gamut status is known by construction.

use std::ptr;

use doctored_core::ffi_adjust::*;
use doctored_core::ffi_doc::*;
use tempfile::TempDir;

use crate::common;
use common::*;

const D50: [f64; 3] = [0.96422, 1.0, 0.82521];
/// Lindbloom's published sRGB → XYZ matrix, Bradford-adapted to D50.
const SRGB_TO_XYZ_D50: [[f64; 3]; 3] = [
    [0.4360747, 0.3850649, 0.1430804],
    [0.2225045, 0.7168786, 0.0606169],
    [0.0139322, 0.0971045, 0.7141733],
];

/// Lab from encoded RGB through a D50-adapted matrix, written from the CIE
/// formulas: linearize with the sRGB transfer function, matrix, then
/// L*a*b* against the D50 PCS white. (`stats_tests`' oracle, copied: test
/// files share nothing but `tests/common`.)
fn lab_oracle(rgb: [u8; 3], matrix: [[f64; 3]; 3]) -> [f64; 3] {
    let lin = rgb.map(|v| {
        let v = f64::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    let xyz: [f64; 3] = matrix.map(|row| row[0] * lin[0] + row[1] * lin[1] + row[2] * lin[2]);
    let eps = 216.0 / 24389.0;
    let kappa = 24389.0 / 27.0;
    let f = |t: f64| {
        if t > eps {
            t.cbrt()
        } else {
            (kappa * t + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (f(xyz[0] / D50[0]), f(xyz[1] / D50[1]), f(xyz[2] / D50[2]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn srgb_to_lab(rgb: [f32; 3]) -> Option<[f32; 3]> {
    let mut out = [0f32; 3];
    unsafe { dr_srgb_to_lab(rgb.as_ptr(), out.as_mut_ptr()) }.then_some(out)
}

fn lab_to_srgb(lab: [f32; 3]) -> Option<([f32; 3], bool)> {
    let mut out = [0f32; 3];
    let mut in_gamut = false;
    unsafe { dr_lab_to_srgb(lab.as_ptr(), out.as_mut_ptr(), &mut in_gamut) }
        .then_some((out, in_gamut))
}

fn encoded(rgb: [u8; 3]) -> [f32; 3] {
    rgb.map(|v| f32::from(v) / 255.0)
}

fn codes(rgb: [f32; 3]) -> [u8; 3] {
    rgb.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// The triples `stats_tests::lab_is_read_through_the_documents_own_profile`
/// reads.
const READOUT_TRIPLES: [[u8; 3]; 7] = [
    [255, 255, 255],
    [128, 128, 128],
    [255, 0, 0],
    [0, 255, 0],
    [0, 0, 255],
    [200, 150, 120],
    [0, 0, 0],
];

#[test]
fn srgb_to_lab_matches_the_info_panel_readout_on_an_srgb_document() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "lab.png", &solid(4, 4, [10, 20, 30, 255]));
    for rgb in READOUT_TRIPLES {
        let mut want = [0f32; 3];
        assert!(unsafe { dr_doc_lab(doc, rgb[0], rgb[1], rgb[2], want.as_mut_ptr()) });
        let got = srgb_to_lab(encoded(rgb)).expect("built-in sRGB models");
        for c in 0..3 {
            assert!(
                (got[c] - want[c]).abs() <= 1e-4,
                "{rgb:?} component {c}: picker {} vs readout {}",
                got[c],
                want[c]
            );
        }
    }
    unsafe { dr_doc_free(doc) };
}

#[test]
fn srgb_to_lab_matches_an_independent_cie_oracle() {
    for rgb in READOUT_TRIPLES {
        let got = srgb_to_lab(encoded(rgb)).expect("built-in sRGB models");
        let want = lab_oracle(rgb, SRGB_TO_XYZ_D50);
        for c in 0..3 {
            // 0.05 for the same reason as `stats_tests`: the built-in
            // profile blob's matrix columns and curve agree with the
            // published numbers to about the 7th decimal, not exactly.
            assert!(
                (f64::from(got[c]) - want[c]).abs() < 5e-2,
                "{rgb:?} component {c}: {} vs {}",
                got[c],
                want[c]
            );
        }
    }
}

#[test]
fn srgb_to_lab_clamps_its_input() {
    let clamped = srgb_to_lab([1.5, -0.25, 0.5]).expect("finite input converts");
    let inside = srgb_to_lab([1.0, 0.0, 0.5]).expect("finite input converts");
    assert_eq!(clamped, inside);
}

#[test]
fn eight_bit_codes_survive_a_lab_round_trip_in_gamut() {
    for rgb in [
        [255u8, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [0, 255, 255],
        [255, 0, 255],
        [255, 255, 0],
        [0, 0, 0],
        [255, 255, 255],
        [128, 128, 128],
        [200, 150, 120],
    ] {
        let lab = srgb_to_lab(encoded(rgb)).expect("built-in sRGB models");
        let (back, in_gamut) = lab_to_srgb(lab).expect("finite Lab converts");
        assert_eq!(codes(back), rgb, "{rgb:?} via {lab:?}");
        assert!(in_gamut, "{rgb:?} via {lab:?} flagged out of gamut");
    }
}

#[test]
fn lab_outside_srgb_is_flagged_and_clamped() {
    for lab in [[50f32, 120.0, 0.0], [100.0, 0.0, 100.0]] {
        let (rgb, in_gamut) = lab_to_srgb(lab).expect("finite Lab converts");
        assert!(!in_gamut, "{lab:?} should be out of the sRGB gamut");
        for v in rgb {
            assert!((0.0..=1.0).contains(&v), "{lab:?} gave {rgb:?}");
        }
    }
}

#[test]
fn lab_white_and_black_are_in_gamut() {
    // White is the case `GAMUT_SLACK` exists for: the built-in profile's
    // s15Fixed16 white is not exactly D50, so Lab (100, 0, 0) lands a hair
    // above 1.0 in linear blue.
    for (lab, want) in [
        ([100f32, 0.0, 0.0], [255u8, 255, 255]),
        ([0.0, 0.0, 0.0], [0, 0, 0]),
    ] {
        let (rgb, in_gamut) = lab_to_srgb(lab).expect("finite Lab converts");
        assert!(in_gamut, "{lab:?} flagged out of gamut");
        assert_eq!(codes(rgb), want, "{lab:?}");
    }
}

#[test]
fn non_finite_input_is_refused() {
    assert!(srgb_to_lab([f32::NAN, 0.0, 0.0]).is_none());
    assert!(srgb_to_lab([0.0, f32::INFINITY, 0.0]).is_none());
    assert!(lab_to_srgb([f32::NAN, 0.0, 0.0]).is_none());
    assert!(lab_to_srgb([50.0, 0.0, f32::NEG_INFINITY]).is_none());
}

#[test]
fn null_pointers_are_refused() {
    let rgb = [0.5f32; 3];
    let lab = [50f32, 0.0, 0.0];
    let mut out = [0f32; 3];
    let mut in_gamut = false;
    unsafe {
        assert!(!dr_srgb_to_lab(ptr::null(), out.as_mut_ptr()));
        assert!(!dr_srgb_to_lab(rgb.as_ptr(), ptr::null_mut()));
        assert!(!dr_lab_to_srgb(
            ptr::null(),
            out.as_mut_ptr(),
            &mut in_gamut
        ));
        assert!(!dr_lab_to_srgb(
            lab.as_ptr(),
            ptr::null_mut(),
            &mut in_gamut
        ));
        assert!(!dr_lab_to_srgb(
            lab.as_ptr(),
            out.as_mut_ptr(),
            ptr::null_mut()
        ));
    }
}
