//! v1.x HDR photos: a host-decoded HDR still into a 32-bit float
//! document through `dr_precision_doc_from_host_hdr_pixels`.
//!
//! The oracles are independent: the f32 linear samples must come back bit
//! for bit (straightened by one division where the layout is
//! premultiplied), the PQ and HLG codes against the BT.2100 formulas written
//! here (ST 2084's decimals; Table 5's inverse OETF and OOTF at 1000 cd/m²,
//! γ 1.2), divided by 203, and BT.2020 against BT.2087's published matrix.
//! Every refusal is checked by its sentence.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::common::take_err_string;
use doctored_core::ffi_host_pixels::dr_precision_doc_from_host_hdr_pixels;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// (pixels, import warnings, sRGB profile, Display P3 profile).
type Opened = (Vec<[f32; 4]>, Vec<String>, bool, bool);

struct Intake<'a> {
    bytes: &'a [u8],
    width: u32,
    height: u32,
    row_bytes: usize,
    layout: &'a str,
    orientation: u32,
    transfer: &'a str,
    primaries: Option<&'a str>,
    profile: Option<&'a [u8]>,
    limit: u64,
}

impl<'a> Intake<'a> {
    fn linear(bytes: &'a [u8], width: u32, height: u32, layout: &'a str, icc: &'a [u8]) -> Self {
        Self {
            bytes,
            width,
            height,
            row_bytes: width as usize * 16,
            layout,
            orientation: 1,
            transfer: "linear",
            primaries: None,
            profile: Some(icc),
            limit: LIMIT,
        }
    }

    fn signal(
        bytes: &'a [u8],
        width: u32,
        height: u32,
        transfer: &'a str,
        primaries: &'a str,
    ) -> Self {
        Self {
            bytes,
            width,
            height,
            row_bytes: width as usize * 8,
            layout: "rgba16_le",
            orientation: 1,
            transfer,
            primaries: Some(primaries),
            profile: None,
            limit: LIMIT,
        }
    }

    /// The document's first-layer pixels, its import warnings, and whether
    /// its profile is the built-in sRGB / Display P3; or the refusal.
    fn run(&self) -> Result<Opened, String> {
        let layout = CString::new(self.layout).unwrap();
        let transfer = CString::new(self.transfer).unwrap();
        let primaries = self.primaries.map(|p| CString::new(p).unwrap());
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_from_host_hdr_pixels(
                self.bytes.as_ptr(),
                self.bytes.len(),
                self.width,
                self.height,
                self.row_bytes,
                layout.as_ptr(),
                self.orientation,
                transfer.as_ptr(),
                primaries.as_ref().map_or(ptr::null(), |p| p.as_ptr()),
                self.profile.map_or(ptr::null(), <[u8]>::as_ptr),
                self.profile.map_or(0, <[u8]>::len),
                self.limit,
                0,
                ptr::null(),
                &mut err,
            )
        };
        if out.is_null() {
            return Err(take_err_string(err));
        }
        let document: &PrecisionDocument = unsafe { &*out };
        assert!(document.is_float());
        assert_eq!(
            document.dimensions(),
            (self.width, self.height).into_oriented(self.orientation)
        );
        let pixels = document.layer_source(0).unwrap().to_rgba32().unwrap();
        let info = document.info().unwrap();
        let warnings = info["import_warnings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w.as_str().unwrap().to_string())
            .collect();
        let srgb = document.profile().describes_srgb();
        let p3 = document.profile().bytes() == IccProfile::display_p3().bytes();
        unsafe { dr_precision_doc_free(out) };
        Ok((pixels, warnings, srgb, p3))
    }
}

trait Oriented {
    fn into_oriented(self, orientation: u32) -> (u32, u32);
}
impl Oriented for (u32, u32) {
    fn into_oriented(self, orientation: u32) -> (u32, u32) {
        if orientation >= 5 {
            (self.1, self.0)
        } else {
            self
        }
    }
}

fn f32_bytes(pixels: &[[f32; 4]]) -> Vec<u8> {
    pixels
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}

fn u16_bytes(pixels: &[[u16; 4]]) -> Vec<u8> {
    pixels
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}

fn srgb_icc() -> Vec<u8> {
    IccProfile::srgb().bytes().to_vec()
}

fn pq_nits(signal: f64) -> f64 {
    let p = signal.powf(1.0 / 78.843_75);
    10000.0
        * ((p - 0.835_937_5).max(0.0) / (18.851_562_5 - 18.687_5 * p))
            .powf(1.0 / 0.159_301_757_812_5)
}

fn hlg_scene(signal: f64) -> f64 {
    if signal <= 0.5 {
        signal * signal / 3.0
    } else {
        (((signal - 0.559_910_73) / 0.178_832_77).exp() + 0.284_668_92) / 12.0
    }
}

#[test]
fn linear_f32_pixels_open_bit_for_bit_with_their_profile() {
    let pixels = [
        [0.25, 0.5, 1.0, 1.0],
        [4.0, 2.5, 0.125, 1.0],
        [-0.03, 12.0, 0.0, 0.5],
        [65504.0, 0.0, 1.0, 0.0],
    ];
    let icc = srgb_icc();
    let bytes = f32_bytes(&pixels);
    let (out, warnings, srgb, _) = Intake::linear(&bytes, 2, 2, "rgba32f_le", &icc)
        .run()
        .unwrap();
    assert!(srgb);
    assert_eq!(out, pixels.to_vec(), "straight samples cross unchanged");
    assert!(warnings.is_empty(), "{warnings:?}");

    // Premultiplied divides once; alpha 0 is the canonical zero.
    let (out, _, _, _) = Intake::linear(&bytes, 2, 2, "rgba32f_le_premultiplied", &icc)
        .run()
        .unwrap();
    assert_eq!(out[2], [-0.03 / 0.5, 12.0 / 0.5, 0.0, 0.5]);
    assert_eq!(out[3], [0.0, 0.0, 0.0, 0.0]);
    assert_eq!(out[1], pixels[1]);

    // Opaque ignores the stored alpha.
    let (out, _, _, _) = Intake::linear(&bytes, 2, 2, "rgba32f_le_opaque", &icc)
        .run()
        .unwrap();
    assert!(out.iter().all(|p| p[3] == 1.0));
    assert_eq!(out[2][..3], pixels[2][..3]);

    // A Display P3 profile becomes the document profile.
    let p3 = IccProfile::display_p3().bytes().to_vec();
    let (_, _, srgb, is_p3) = Intake::linear(&bytes, 2, 2, "rgba32f_le", &p3)
        .run()
        .unwrap();
    assert!(!srgb && is_p3);
}

#[test]
fn orientation_and_row_padding_are_honoured() {
    // 2 × 1 stored, padded rows, EXIF 6 (rotate 90° clockwise) → 1 × 2.
    let pixels = [[1.0, 0.0, 0.0, 1.0], [0.0, 2.0, 0.0, 1.0]];
    let mut bytes = f32_bytes(&pixels);
    bytes.extend([0u8; 16]);
    let icc = srgb_icc();
    let mut intake = Intake::linear(&bytes, 2, 1, "rgba32f_le", &icc);
    intake.row_bytes = 48;
    intake.orientation = 6;
    let (out, _, _, _) = intake.run().unwrap();
    assert_eq!(out, vec![pixels[0], pixels[1]]);
    intake.orientation = 3;
    let (out, _, _, _) = intake.run().unwrap();
    assert_eq!(out, vec![pixels[1], pixels[0]]);
}

#[test]
fn pq_codes_open_as_linear_light_at_203_nits_per_unit() {
    let codes: Vec<[u16; 4]> = [0u16, 1000, 20000, 38050, 40000, 50000, 60000, 65535]
        .iter()
        .map(|&c| [c, c / 2, 65535 - c, 65535])
        .collect();
    let bytes = u16_bytes(&codes);
    let (out, warnings, srgb, _) = Intake::signal(&bytes, 8, 1, "pq", "bt709").run().unwrap();
    assert!(srgb);
    assert!(warnings.is_empty(), "{warnings:?}");
    for (pixel, code) in out.iter().zip(&codes) {
        for c in 0..3 {
            let expected = pq_nits(f64::from(code[c]) / 65535.0) / 203.0;
            let got = f64::from(pixel[c]);
            assert!(
                (got - expected).abs() <= 1e-6 * expected.max(1.0),
                "{code:?}:{c} = {got} vs {expected}"
            );
        }
        assert_eq!(pixel[3], 1.0);
    }
    // The PQ code of 203 cd/m² is 1.0 within one 16-bit code.
    let white = (0.580_64_f64 * 65535.0).round() as u16;
    let bytes = u16_bytes(&[[white, white, white, 65535]]);
    let (out, _, _, _) = Intake::signal(&bytes, 1, 1, "pq", "display_p3")
        .run()
        .unwrap();
    assert!((out[0][0] - 1.0).abs() < 0.002, "{:?}", out[0]);
}

#[test]
fn hlg_codes_go_through_the_ootf_with_the_stated_primaries_luminance() {
    let codes = [
        [49151u16, 49151, 49151, 65535],
        [58981, 26214, 13107, 65535],
    ];
    let bytes = u16_bytes(&codes);
    let (out, _, srgb, _) = Intake::signal(&bytes, 2, 1, "hlg", "bt709").run().unwrap();
    assert!(srgb);
    let luma = [0.2126, 0.7152, 0.0722];
    for (pixel, code) in out.iter().zip(&codes) {
        let scene = [0, 1, 2].map(|c| hlg_scene(f64::from(code[c]) / 65535.0));
        let ys: f64 = (0..3).map(|c| luma[c] * scene[c]).sum();
        for c in 0..3 {
            let expected = 1000.0 * ys.powf(0.2) * scene[c] / 203.0;
            assert!(
                (f64::from(pixel[c]) - expected).abs() < 1e-4 * expected.max(1.0),
                "{code:?}:{c} {} vs {expected}",
                pixel[c]
            );
        }
    }
    // A 75 % grey is reference white.
    assert!((out[0][0] - 1.0).abs() < 0.003, "{:?}", out[0]);
}

#[test]
fn bt2020_converts_to_srgb_primaries_with_a_warning_and_keeps_negatives() {
    // A saturated BT.2020 green, mid PQ code.
    let codes = [[0u16, 40000, 0, 65535]];
    let bytes = u16_bytes(&codes);
    let (out, warnings, srgb, _) = Intake::signal(&bytes, 1, 1, "pq", "bt2020").run().unwrap();
    assert!(srgb);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].starts_with("HDR still primaries (0.708 0.292"),
        "{}",
        warnings[0]
    );
    assert!(warnings[0].ends_with("converted in linear light to sRGB primaries"));
    let g = pq_nits(40000.0 / 65535.0) / 203.0;
    // BT.2087's column for green.
    let expected = [-0.5876 * g, 1.1329 * g, -0.1006 * g];
    for c in 0..3 {
        assert!(
            (f64::from(out[0][c]) - expected[c]).abs() < 3e-4 * g,
            "{:?} vs {expected:?}",
            out[0]
        );
    }
    assert!(out[0][0] < 0.0 && out[0][2] < 0.0, "negatives stay");
}

#[test]
fn ten_bit_packed_codes_widen_by_the_layout_rule() {
    // 1023/512/0 in R/G/B.
    let word: u32 = (1023 << 20) | (512 << 10);
    let bytes = word.to_le_bytes().to_vec();
    let mut intake = Intake::signal(&bytes, 1, 1, "pq", "bt709");
    intake.layout = "rgb10_packed_le";
    intake.row_bytes = 4;
    let (out, _, _, _) = intake.run().unwrap();
    let g16 = (512 * 65535 + 511) / 1023;
    assert!((f64::from(out[0][0]) - 10000.0 / 203.0).abs() < 1e-3);
    let expected = pq_nits(f64::from(g16) / 65535.0) / 203.0;
    assert!((f64::from(out[0][1]) - expected).abs() < 1e-6 * expected.max(1.0));
    assert_eq!(out[0][2], 0.0);
}

#[test]
fn every_refusal_names_its_reason() {
    let icc = srgb_icc();
    let good = f32_bytes(&[[1.0, 1.0, 1.0, 1.0]]);
    let codes = u16_bytes(&[[1, 2, 3, 65535]]);

    for (value, word) in [
        (f32::NAN, "a non-finite sample"),
        (70000.0, "a sample outside ±65504"),
    ] {
        let bytes = f32_bytes(&[[0.0, 0.0, 0.0, 1.0], [0.5, value, 0.0, 1.0]]);
        let error = Intake::linear(&bytes, 2, 1, "rgba32f_le", &icc)
            .run()
            .unwrap_err();
        assert_eq!(error, format!("HDR host pixels hold {word} at (1, 0)"));
    }

    let mut intake = Intake::linear(&good, 1, 1, "rgba32f_le", &icc);
    intake.transfer = "gamma";
    assert_eq!(
        intake.run().unwrap_err(),
        "host HDR transfer \"gamma\" is not one of linear, pq or hlg"
    );
    intake.transfer = "linear";
    intake.primaries = Some("bt709");
    assert!(intake.run().unwrap_err().contains("primaries must be NULL"));
    intake.primaries = None;
    intake.profile = None;
    assert_eq!(
        intake.run().unwrap_err(),
        "linear HDR host pixels require their original ICC"
    );
    intake.profile = Some(&icc);
    intake.layout = "rgba16_le";
    intake.row_bytes = 8;
    let short = u16_bytes(&[[1, 1, 1, 1]]);
    intake.bytes = &short;
    assert_eq!(
        intake.run().unwrap_err(),
        "linear HDR host pixels require an rgba32f layout"
    );

    let mut intake = Intake::signal(&codes, 1, 1, "pq", "bt709");
    intake.profile = Some(&icc);
    assert_eq!(
        intake.run().unwrap_err(),
        "PQ and HLG host pixels carry no ICC; profile must be NULL"
    );
    intake.profile = None;
    intake.primaries = Some("aces");
    assert_eq!(
        intake.run().unwrap_err(),
        "HDR host primaries \"aces\" are not one of bt709, display_p3 or bt2020"
    );
    intake.primaries = None;
    assert!(intake
        .run()
        .unwrap_err()
        .contains("host HDR primaries is NULL"));
    intake.primaries = Some("bt709");
    intake.layout = "rgba8";
    intake.row_bytes = 4;
    let bytes8 = [1u8, 2, 3, 255];
    intake.bytes = &bytes8;
    assert_eq!(
        intake.run().unwrap_err(),
        "PQ and HLG host pixels require an integer 16-bit or rgb10_packed_le layout"
    );
    intake.layout = "rgba32f_le";
    intake.row_bytes = 16;
    intake.bytes = &good;
    assert_eq!(
        intake.run().unwrap_err(),
        "PQ and HLG host pixels require an integer 16-bit or rgb10_packed_le layout"
    );

    // A short buffer, and a limit too small for the plan.
    let mut intake = Intake::linear(&good[..8], 1, 1, "rgba32f_le", &icc);
    assert!(intake
        .run()
        .unwrap_err()
        .contains("row padding and buffer length"));
    intake.bytes = &good;
    intake.limit = 1024;
    let error = intake.run().unwrap_err();
    assert!(
        error.starts_with("HDR still capacity for host pixels"),
        "{error}"
    );
}

#[test]
fn null_arguments_are_refused_without_crashing() {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_from_host_hdr_pixels(
            ptr::null(),
            0,
            1,
            1,
            16,
            ptr::null(),
            1,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(take_err_string(err), "host HDR samples are required");
    let bytes = [0u8; 16];
    let out = unsafe {
        dr_precision_doc_from_host_hdr_pixels(
            bytes.as_ptr(),
            16,
            1,
            1,
            16,
            ptr::null(),
            1,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    };
    assert!(out.is_null());
    unsafe { dr_precision_doc_free(ptr::null_mut()) };
}
