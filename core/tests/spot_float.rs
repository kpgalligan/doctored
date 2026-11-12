//! #1 spot channels on a 32-bit float document (`spot_typed`'s float arm):
//! Merge Spot Channel and the flattened export lay each ink with Doctored's
//! overprint rule on the composite's DISPLAY ENCODING — the document
//! profile's curve on 0…1, continued above white by `1 + s·ln v`
//! (`display_encoding`) — and decode the result back to linear light, so a
//! float document's file holds what its display shows and a value above white
//! survives. Through the FFI (`dr_precision_doc_add_spot_channel`,
//! `dr_precision_doc_merge_spot_channel`).
//!
//! The oracle is written here: per plane on unit values over an opaque
//! composite, `D((1 − w)·E(c) + w·((1 − s)·E(c)·ink + s·ink))`, with `E` and `D`
//! the built-in sRGB curve and its logarithm above white as
//! `float_filters_display` restates them, and the ink from the core's encoding
//! seam (`spot_view::ink_encoded`), itself checked against the closed form for
//! a neutral ink. A 32-bit Gray document takes the same rule over its Gray Gamma
//! 2.2 twin: `E(y) = y^(1/2.2)`, the ink its `L*` alone (Lab → Grayscale's
//! rule). The bounds are declared before the first run and never widened:
//!
//! - RGB merge: every component of a covered pixel within 8/65535 of the
//!   oracle in encoded units (the TWIN bound of `float_filters_display`, 4,
//!   for the encode, and 4 for the decode and the f32 blend); coverage 0
//!   bit-exact; alpha 1.
//! - RGB PNG16 export: every component within 64 codes of
//!   `65535·clamp(code, 0, 1)` (the export's own clip and encode).
//! - The neutral ink's seam within 0.002 of `E(Y(L*))`.
//! - Gray merge: R = G = B, each within 12/65535 of the oracle in encoded
//!   units (8 as above, and 4 for the Gray Gamma 2.2 table against the exact
//!   power, `spot_typed_export`'s Gray bound).

use std::ffi::CString;
use std::ptr;

use doctored_core::doc_export::ExportRequest;
use doctored_core::ffi_spot_channel::dr_precision_doc_add_spot_channel;
use doctored_core::ffi_spot_merge::dr_precision_doc_merge_spot_channel;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::spot_channel::SpotInk;
use doctored_core::spot_view::ink_encoded;
use image::DynamicImage;

use crate::common::take_err_string;
use crate::float_filters_display::{decode, encode};
use crate::float_fixture::float_doc;

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 4;
const H: u32 = 3;
/// The RGB merge's bound, in encoded units.
const RGB_BOUND: f64 = 8.0 / 65535.0;
/// The PNG16 export's bound, in 16-bit codes.
const EXPORT_BOUND: f64 = 64.0;
/// The neutral ink's seam against the closed form.
const INK_BOUND: f64 = 0.002;
/// The Gray merge's bound, in encoded units.
const GRAY_BOUND: f64 = 12.0 / 65535.0;

/// Twelve coverage values, 0 and 65535 among them (`spot_merge.rs`'s).
fn coverage16() -> Vec<u16> {
    [0u16, 255, 17, 51, 0, 128, 200, 254, 1, 77, 160, 0]
        .iter()
        .map(|&c| c * 257)
        .collect()
}

/// The overprint on one encoded plane value over an opaque pixel.
fn printed(code: f64, w: f64, s: f64, ink: f64) -> f64 {
    (1.0 - w) * code + w * ((1.0 - s) * code * ink + s * ink)
}

/// An opaque sRGB float canvas: values inside 0…1, above white (4.0 at full
/// coverage, 2.5 at half) and one below zero.
fn rgb_pixels() -> Vec<[f32; 4]> {
    vec![
        [0.5, 0.5, 0.5, 1.0],
        [4.0, 4.0, 4.0, 1.0],
        [0.2, 0.05, 0.8, 1.0],
        [1.0, 0.0, 0.3, 1.0],
        [0.7, 0.9, 0.1, 1.0],
        [2.5, 0.4, 0.6, 1.0],
        [0.01, 0.15, 0.35, 1.0],
        [0.9, 0.6, 1.5, 1.0],
        [-0.05, 0.3, 0.45, 1.0],
        [0.33, 0.66, 0.99, 1.0],
        [0.12, 0.24, 0.48, 1.0],
        [3.0, 0.5, 0.25, 1.0],
    ]
}

fn render32(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap()
}

/// `doc` with one spot channel of `ink` over `coverage16()`, then merged,
/// both through the FFI. Returns (with the channel, merged).
fn add_and_merge(
    doc: &PrecisionDocument,
    lab: [f64; 3],
    solidity: u8,
) -> (Box<PrecisionDocument>, Box<PrecisionDocument>) {
    let name = CString::new("Spot").unwrap();
    let coverage = coverage16();
    let mut err = ptr::null_mut();
    let with = unsafe {
        dr_precision_doc_add_spot_channel(
            doc,
            name.as_ptr(),
            lab[0],
            lab[1],
            lab[2],
            solidity,
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!with.is_null(), "{}", take_err_string(err));
    let index = unsafe { &*with }.channels().len() - 1;
    let merged = unsafe {
        dr_precision_doc_merge_spot_channel(with, index, LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!merged.is_null(), "{}", take_err_string(err));
    // Both handles are the FFI's boxes; owning them here frees them.
    unsafe { (Box::from_raw(with), Box::from_raw(merged)) }
}

/// `L*` alone as linear luminance (CIE's inverse).
fn luminance_of(l_star: f64) -> f64 {
    let fy = (l_star + 16.0) / 116.0;
    if fy.powi(3) > 216.0 / 24389.0 {
        fy.powi(3)
    } else {
        l_star * 27.0 / 24389.0
    }
}

#[test]
fn the_neutral_inks_seam_is_its_luminance_through_the_srgb_curve() {
    let ink = SpotInk::new([50.0, 0.0, 0.0], 0).unwrap();
    let seam = ink_encoded(&ink, Some(&IccProfile::srgb())).unwrap();
    let want = encode(luminance_of(50.0));
    for (c, &v) in seam.iter().enumerate() {
        assert!(
            (f64::from(v) - want).abs() <= INK_BOUND,
            "plane {c}: {v} vs {want}"
        );
    }
}

#[test]
fn a_float_merge_overprints_on_the_display_encoding() {
    let doc = float_doc(W, H, &IccProfile::srgb(), &[rgb_pixels()]);
    let before = render32(&doc);
    let coverage = coverage16();
    // A neutral mid ink at 60 % and a coloured one at 0 % (Multiply).
    for (lab, solidity) in [([50.0, 0.0, 0.0], 60u8), ([60.0, 30.0, -20.0], 0u8)] {
        let ink = SpotInk::new(lab, solidity).unwrap();
        let encoded = ink_encoded(&ink, Some(&IccProfile::srgb())).unwrap();
        let (_, merged) = add_and_merge(&doc, lab, solidity);
        assert!(merged.is_float() && merged.channels().is_empty(), "{lab:?}");
        let got = render32(&merged);
        let s = f64::from(solidity) / 100.0;
        for (i, &cov) in coverage.iter().enumerate() {
            let w = f64::from(cov) / 65535.0;
            for c in 0..3 {
                if cov == 0 {
                    assert_eq!(
                        got[i][c].to_bits(),
                        before[i][c].to_bits(),
                        "{lab:?} px {i} c {c} untouched"
                    );
                    continue;
                }
                let code = printed(encode(f64::from(before[i][c])), w, s, f64::from(encoded[c]));
                let value = encode(f64::from(got[i][c]));
                assert!(
                    (value - code).abs() <= RGB_BOUND,
                    "{lab:?} px {i} c {c}: E(out) {value} vs {code} (linear {} vs {})",
                    got[i][c],
                    decode(code)
                );
                if code > 1.0 {
                    assert!(got[i][c] > 1.0, "{lab:?} px {i} c {c} stays above white");
                }
            }
            assert!((got[i][3] - 1.0).abs() <= 1e-6, "{lab:?} px {i} alpha");
        }
    }
}

#[test]
fn a_float_png16_export_composites_the_encoded_overprint() {
    let doc = float_doc(W, H, &IccProfile::srgb(), &[rgb_pixels()]);
    let before = render32(&doc);
    let (lab, solidity) = ([60.0, 30.0, -20.0], 60u8);
    let ink = SpotInk::new(lab, solidity).unwrap();
    let encoded = ink_encoded(&ink, Some(&IccProfile::srgb())).unwrap();
    let (with, _) = add_and_merge(&doc, lab, solidity);
    let request = ExportRequest::parse(r#"{"format":"png","bit_depth":16}"#).unwrap();
    let result = with.export_encoded(None, &request, LIMIT, 0, None).unwrap();
    let read = image::load_from_memory(result.bytes()).unwrap();
    assert!(
        matches!(
            read,
            DynamicImage::ImageRgb16(_) | DynamicImage::ImageRgba16(_)
        ),
        "a 16-bit PNG"
    );
    let read = read.to_rgba16();
    let s = f64::from(solidity) / 100.0;
    for (i, &cov) in coverage16().iter().enumerate() {
        let w = f64::from(cov) / 65535.0;
        let pixel = read.get_pixel(i as u32 % W, i as u32 / W).0;
        for c in 0..3 {
            let code = printed(encode(f64::from(before[i][c])), w, s, f64::from(encoded[c]));
            let expected = 65535.0 * code.clamp(0.0, 1.0);
            let value = f64::from(pixel[c]);
            assert!(
                (value - expected).abs() <= EXPORT_BOUND,
                "px {i} c {c}: {value} vs {expected}"
            );
        }
    }
}

#[test]
fn a_gray32_merge_overprints_the_inks_gray_on_its_encoding() {
    let neutral = [
        0.5f32, 0.9, 0.2, 0.05, 0.7, 0.35, 0.6, 0.15, 0.8, 0.25, 0.45, 1.0,
    ];
    let pixels: Vec<[f32; 4]> = neutral.iter().map(|&v| [v, v, v, 1.0]).collect();
    let gray = float_doc(W, H, &IccProfile::srgb(), &[pixels])
        .convert_to_gray(None, r#"{"space":"gray_gamma_22"}"#, LIMIT, 0, None)
        .unwrap();
    assert!(gray.is_gray() && gray.is_float());
    let before = render32(&gray);
    let (lab, solidity) = ([35.0, -40.0, 60.0], 30u8);
    let (_, merged) = add_and_merge(&gray, lab, solidity);
    assert!(merged.is_gray() && merged.is_float());
    let got = render32(&merged);
    let ink = luminance_of(lab[0]).powf(1.0 / 2.2);
    let s = f64::from(solidity) / 100.0;
    let mut misses = Vec::new();
    for (i, &cov) in coverage16().iter().enumerate() {
        assert_eq!(got[i][0].to_bits(), got[i][1].to_bits(), "px {i} R = G");
        assert_eq!(got[i][0].to_bits(), got[i][2].to_bits(), "px {i} R = B");
        if cov == 0 {
            assert_eq!(
                got[i][0].to_bits(),
                before[i][0].to_bits(),
                "px {i} untouched"
            );
            continue;
        }
        let w = f64::from(cov) / 65535.0;
        let code = printed(f64::from(before[i][0]).powf(1.0 / 2.2), w, s, ink);
        let value = f64::from(got[i][0]).powf(1.0 / 2.2);
        if (value - code).abs() > GRAY_BOUND {
            misses.push(format!(
                "px {i} (before {}, w {w:.4}): E(out) {value} vs {code} ({:+.1} codes)",
                before[i][0],
                (value - code) * 65535.0
            ));
        }
    }
    assert!(misses.is_empty(), "{misses:#?}");
}
