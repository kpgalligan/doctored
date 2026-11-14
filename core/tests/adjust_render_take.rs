//! #2: Threshold at 8 and 16 bits and Photo Filter's 8-bit Preserve
//! Luminosity against Photoshop 26.10.0's renders of `adjust-render-2`
//! take2 (`scripts/photoshop-probe/adjust_render_2_take2.py`; the committed
//! fixture `psd_photoshop/adjust_render_2/`, its README): fresh levels 25,
//! 77, 150, 205 and 240 over designed colours (exact half-code ties, colours
//! on which Rec. 709 luma disagrees, samples one 15-bit step either side of
//! a 16-bit level), and five fresh filters each with Preserve Luminosity on
//! and off.
//!
//! The take's own verdict: the fixed-point luma `blend::lum_fixed` on the
//! codes (8 bits) and on the 15-bit values (16 bits) put every sample on
//! Photoshop's side, and the integer SetLum on Photoshop's own filtered
//! codes was within 1 code. This file holds the CORE to the render, with
//! the bound the runner declared before the take: Threshold equal to
//! Photoshop's on every sample at both depths (through `dr_image_threshold`
//! and `dr_doc_adjust_image` at 8 bits, `PrecisionAdjustment` at 16), and
//! the core's 8-bit Photo Filter (its own filter arithmetic, through
//! `dr_doc_adjust_image` on an sRGB document) within max 2 / mean 0.5 codes
//! per filter, Preserve Luminosity on and off. Each row prints its residual.
//!
//! A second, independent oracle restates the luma from its definition (the
//! 0.30 and 0.11 weights floored at 2^15, green the remainder, half up) and
//! holds `dr_image_threshold` to it on every level over a lattice of codes.

use std::ffi::CString;
use std::path::PathBuf;
use std::ptr;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_calibrated::dr_doc_adjust_image;
use doctored_core::ffi_filters::dr_image_threshold;
use doctored_core::icc::IccProfile;
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::{pixels, take_err_string};

const LEVELS: [u32; 5] = [25, 77, 150, 205, 240];
/// (name, colour, density %), as the take wrote and read them back.
const FILTERS: [(&str, &str, u32); 5] = [
    ("F6", "#8040ff", 50),
    ("F7", "#c0a000", 70),
    ("F8", "#00ff80", 35),
    ("F9", "#ff6000", 95),
    ("F10", "#4080c0", 20),
];

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/adjust_render_2")
        .join(format!("{name}.png"))
}

/// The fixture's samples (one pixel each, alpha 0 past the last) at the
/// document's depth: 8-bit codes for `eight`, else 16-bit codes.
fn samples(name: &str, eight: bool) -> Vec<[u16; 3]> {
    let image = image::open(fixture(name)).expect(name).to_rgba16();
    image
        .pixels()
        .filter(|p| p.0[3] != 0)
        .map(|p| {
            let rgb = [p.0[0], p.0[1], p.0[2]];
            if eight {
                rgb.map(|c| {
                    assert_eq!(c % 257, 0, "{name}: an 8-bit sample is code x 257");
                    c / 257
                })
            } else {
                rgb
            }
        })
        .collect()
}

fn image_of(codes: &[[u16; 3]]) -> RgbaImage {
    RgbaImage::from_fn(codes.len() as u32, 1, |x, _| {
        let c = codes[x as usize];
        Rgba([c[0] as u8, c[1] as u8, c[2] as u8, 255])
    })
}

fn rgb_of(out: *mut DrImage) -> Vec<[u16; 3]> {
    let got = pixels(out)
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2]].map(u16::from))
        .collect();
    unsafe { dr_image_free(out) };
    got
}

/// `op`/`params` over 8-bit `codes` through `dr_doc_adjust_image` on an sRGB
/// document (the profile-bound path the layer render takes).
fn doc_adjust8(codes: &[[u16; 3]], op: &str, params: &Value) -> Vec<[u16; 3]> {
    let src = image_of(codes);
    let mut doc = DrDocument::from_pixels(src.clone());
    doc.profile = IccProfile::srgb();
    let op = CString::new(op).unwrap();
    let params = CString::new(params.to_string()).unwrap();
    unsafe {
        let image = dr_image_from_rgba(src.as_raw().as_ptr(), codes.len() as u32, 1);
        assert!(!image.is_null());
        let mut error = ptr::null_mut();
        let out = dr_doc_adjust_image(&doc, image, op.as_ptr(), params.as_ptr(), &mut error);
        dr_image_free(image);
        assert!(!out.is_null(), "{}", take_err_string(error));
        rgb_of(out)
    }
}

fn threshold8(codes: &[[u16; 3]], level: u32) -> Vec<[u16; 3]> {
    let src = image_of(codes);
    unsafe {
        let image = dr_image_from_rgba(src.as_raw().as_ptr(), codes.len() as u32, 1);
        assert!(!image.is_null());
        let out = dr_image_threshold(image, level as f32 / 255.0);
        dr_image_free(image);
        assert!(!out.is_null());
        rgb_of(out)
    }
}

fn threshold16(codes: &[[u16; 3]], level: u32) -> Vec<[u16; 3]> {
    let meta = json!({
        "type": "adjust", "evaluation": "precision_v1", "op": "threshold",
        "params": {"level": f64::from(level) / 255.0}
    })
    .to_string();
    let op = PrecisionAdjustment::parse(&meta, &IccProfile::srgb()).expect("parses");
    codes
        .iter()
        .map(|c| {
            op.apply_at(c.map(|v| f32::from(v) / 65535.0), 0.0, (0, 0))
                .map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16)
        })
        .collect()
}

/// The samples whose output differs from Photoshop's, as (input, ours, ps).
fn mismatches(
    input: &[[u16; 3]],
    ours: &[[u16; 3]],
    ps: &[[u16; 3]],
) -> Vec<([u16; 3], [u16; 3], [u16; 3])> {
    assert_eq!(ours.len(), ps.len());
    input
        .iter()
        .zip(ours.iter().zip(ps))
        .filter(|(_, (o, p))| o != p)
        .map(|(i, (o, p))| (*i, *o, *p))
        .collect()
}

#[test]
fn threshold_matches_the_take_at_8_bits() {
    let input = samples("thr8b_in", true);
    for level in LEVELS {
        let ps = samples(&format!("thr_{level}_thr8b"), true);
        let params = json!({"level": f64::from(level) / 255.0});
        for (path, ours) in [
            ("dr_image_threshold", threshold8(&input, level)),
            (
                "dr_doc_adjust_image",
                doc_adjust8(&input, "threshold", &params),
            ),
        ] {
            let wrong = mismatches(&input, &ours, &ps);
            println!(
                "thr8 {level} {path}: {} of {} differ",
                wrong.len(),
                input.len()
            );
            assert!(
                wrong.is_empty(),
                "level {level} via {path}: {:?}",
                &wrong[..wrong.len().min(6)]
            );
        }
    }
}

#[test]
fn threshold_matches_the_take_at_16_bits() {
    let input = samples("thr16b_in", false);
    for level in LEVELS {
        let ps = samples(&format!("thr_{level}_thr16b"), false);
        let wrong = mismatches(&input, &threshold16(&input, level), &ps);
        println!("thr16 {level}: {} of {} differ", wrong.len(), input.len());
        assert!(
            wrong.is_empty(),
            "level {level}: {:?}",
            &wrong[..wrong.len().min(6)]
        );
    }
}

#[test]
fn photo_filter_preserve_luminosity_matches_the_take_at_8_bits() {
    let input = samples("pf8b_in", true);
    for (name, color, density) in FILTERS {
        for preserve in [true, false] {
            let ps = samples(
                &format!("pf_{name}_{}", if preserve { "on" } else { "off" }),
                true,
            );
            let params = json!({"color": color, "density": f64::from(density) / 100.0,
                                "preserve_luminosity": preserve});
            let ours = doc_adjust8(&input, "photo_filter", &params);
            let diffs: Vec<f64> = ours
                .iter()
                .zip(&ps)
                .flat_map(|(o, p)| (0..3).map(move |k| f64::from(o[k].abs_diff(p[k]))))
                .collect();
            let max = diffs.iter().copied().fold(0.0, f64::max);
            let mean = diffs.iter().sum::<f64>() / diffs.len() as f64;
            println!("pf {name} preserve {preserve}: max {max} mean {mean:.4}");
            assert!(
                max <= 2.0 && mean <= 0.5,
                "{name} preserve {preserve}: max {max}, mean {mean:.4}, bound 2 / 0.5"
            );
        }
    }
}

/// The luma restated from its definition: 0.30 and 0.11 floored at 2^15,
/// green the remainder, rounded half up.
fn oracle_luma(c: [u32; 3]) -> u32 {
    let r = (0.30_f64 * 32768.0).floor() as u32;
    let b = (0.11_f64 * 32768.0).floor() as u32;
    let g = 32768 - r - b;
    (r * c[0] + g * c[1] + b * c[2] + 16384) / 32768
}

#[test]
fn dr_image_threshold_is_the_fixed_point_luma_on_a_lattice() {
    let codes: Vec<[u16; 3]> = (0..256u16)
        .step_by(5)
        .flat_map(|r| (0..256u16).step_by(7).map(move |g| (r, g)))
        .flat_map(|(r, g)| (0..256u16).step_by(11).map(move |b| [r, g, b]))
        .collect();
    for level in [0u32, 1, 64, 127, 128, 153, 254, 255] {
        let ours = threshold8(&codes, level);
        for (c, o) in codes.iter().zip(&ours) {
            let white = oracle_luma(c.map(u32::from)) >= level;
            assert_eq!(o[0] == 255, white, "{c:?} at level {level}");
        }
    }
}
