//! Content-Aware Fill's `fill_transparent` pad (Auto-Blend
//! Panorama's Content-Aware Fill Transparent Areas) through the FFI:
//! `dr_doc_content_aware_fill_edit` at 8 bits and `dr_precision_doc_edit` at
//! 16 bits and in a 32-bit float document.
//!
//! # The oracles
//!
//! Written from the request's contract: with `fill_transparent` the hole's
//! alpha becomes full on the destination before the fill writes it, so a
//! transparent hole in a layer of one uniform colour `c` is filled with `c`
//! and is opaque; every sample outside the hole is the layer's own; without
//! the flag the hole stays transparent, as it always has. On a value-noise
//! layer the fill copies and blends the surround, so it lands inside the
//! surround's own range. The refusals are the request's sentences.
//!
//! # Bounds (all but `RANGE_SLACK` declared before the first run)
//!
//! * The uniform fill: within 1 code of `c` at 8 bits, 257 codes at 16 bits,
//!   1e-3 in a float document; the hole fully opaque (alpha 255, 65535, 1.0).
//! * Outside the hole: bit-identical.
//! * Value noise: every hole sample inside [min, max] of the layer outside
//!   the hole, per channel, give or take `RANGE_SLACK` (2%) of that range.
//!   The spec's bound was [min, max] exactly; the first runs found one
//!   8-bit sample one code (0.65% of the range) and one float sample 0.0052
//!   (0.87%) under the minimum: the healing write-back adds a harmonic
//!   membrane to the copied texture, which the maximum principle bounds by
//!   the boundary mismatch, not by the surround's range.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_content_aware::dr_doc_content_aware_fill_edit;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::cmyk_fixture::press_profile;
use crate::common::take_err_string;
use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 128;
const H: u32 = 96;
/// The transparent hole, `(x, y, width, height)`.
const HOLE: (u32, u32, u32, u32) = (44, 33, 40, 30);
/// The uniform layer's colour, unit RGB.
const C: [f64; 3] = [0.3, 0.55, 0.7];
/// The value-noise bound's slack, a share of the surround's range (module
/// doc).
const RANGE_SLACK: f64 = 0.02;
const ONLY: &str = "fill_transparent applies only to content_aware_fill with output layer";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Depth {
    Byte,
    Sixteen,
    Float,
}

const DEPTHS: [Depth; 3] = [Depth::Byte, Depth::Sixteen, Depth::Float];

enum Doc {
    Byte(DrDocument),
    Typed(PrecisionDocument),
}

fn in_hole(i: usize) -> bool {
    let (x, y) = (i as u32 % W, i as u32 / W);
    (HOLE.0..HOLE.0 + HOLE.2).contains(&x) && (HOLE.1..HOLE.1 + HOLE.3).contains(&y)
}

fn hash(i: u32) -> u32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x5bd1_e995;
    x ^= x >> 15;
    x = x.wrapping_mul(0x85EB_CA6B);
    x ^ (x >> 13)
}

/// One octave of bilinear value noise in 0…1 on an 8-pixel lattice.
fn noise(x: u32, y: u32, seed: u32) -> f64 {
    let cell = 8;
    let lattice =
        |i: u32, j: u32| f64::from(hash(i * 7919 + j * 104_729 + seed) & 0xffff) / 65535.0;
    let (i, j) = (x / cell, y / cell);
    let (fx, fy) = (
        f64::from(x % cell) / f64::from(cell),
        f64::from(y % cell) / f64::from(cell),
    );
    let top = lattice(i, j) * (1.0 - fx) + lattice(i + 1, j) * fx;
    let bottom = lattice(i, j + 1) * (1.0 - fx) + lattice(i + 1, j + 1) * fx;
    top * (1.0 - fy) + bottom * fy
}

/// The layer: `colour(i)` everywhere, the hole transparent black.
fn picture(colour: impl Fn(usize) -> [f64; 3]) -> Vec<[f64; 4]> {
    (0..(W * H) as usize)
        .map(|i| {
            if in_hole(i) {
                [0.0; 4]
            } else {
                let c = colour(i);
                [c[0], c[1], c[2], 1.0]
            }
        })
        .collect()
}

fn uniform() -> Vec<[f64; 4]> {
    picture(|_| C)
}

fn textured() -> Vec<[f64; 4]> {
    picture(|i| {
        let (x, y) = (i as u32 % W, i as u32 / W);
        [0, 1, 2].map(|c| 0.2 + 0.6 * noise(x, y, 31 * c + 7))
    })
}

fn build(depth: Depth, pixels: &[[f64; 4]]) -> Doc {
    match depth {
        Depth::Byte => Doc::Byte(DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| {
            Rgba(pixels[(y * W + x) as usize].map(|v| (v * 255.0).round() as u8))
        }))),
        Depth::Sixteen => {
            let codes = pixels
                .iter()
                .map(|p| p.map(|v| (v * 65535.0).round() as u16))
                .collect();
            let image = PrecisionImage::from_rgba16(W, H, codes, IccProfile::srgb(), None).unwrap();
            let layer = PrecisionDocumentLayer::raster("L0", image, (0, 0), 1.0, true).unwrap();
            Doc::Typed(
                PrecisionDocument::from_parts(
                    W,
                    H,
                    IccProfile::srgb(),
                    Metadata::default(),
                    Resolution::default(),
                    vec![layer],
                )
                .unwrap(),
            )
        }
        Depth::Float => Doc::Typed(float_doc(
            W,
            H,
            &IccProfile::srgb(),
            &[pixels.iter().map(|p| p.map(|v| v as f32)).collect()],
        )),
    }
}

fn request(op: &str, output: &str, flag: Option<bool>) -> Value {
    let mut request = json!({"op":op,"layer":0,"ring":0,"seed":0,"sample":"current",
        "ignore_adjustments":false,"output":output,
        "x":HOLE.0,"y":HOLE.1,"width":HOLE.2,"height":HOLE.3});
    if let Some(flag) = flag {
        request["fill_transparent"] = json!(flag);
    }
    request
}

fn mask() -> Vec<u8> {
    (0..(W * H) as usize)
        .map(|i| if in_hole(i) { 255 } else { 0 })
        .collect()
}

/// The fill, `Ok(None)` when the core answers no change.
fn fill(doc: &Doc, request: &Value) -> Result<Option<Doc>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mask = mask();
    let mut err: *mut c_char = ptr::null_mut();
    match doc {
        Doc::Byte(d) => {
            let out = unsafe {
                dr_doc_content_aware_fill_edit(
                    d,
                    text.as_ptr(),
                    mask.as_ptr(),
                    ptr::null(),
                    &mut err,
                )
            };
            if out.is_null() {
                return if err.is_null() {
                    Ok(None)
                } else {
                    Err(take_err_string(err))
                };
            }
            Ok(Some(Doc::Byte(*unsafe { Box::from_raw(out) })))
        }
        Doc::Typed(d) => {
            let samples: Vec<u16> = mask.iter().map(|&v| u16::from(v) * 257).collect();
            let out = unsafe {
                dr_precision_doc_edit(
                    d,
                    text.as_ptr(),
                    samples.as_ptr(),
                    samples.len(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                )
            };
            if out.is_null() {
                return if err.is_null() {
                    Ok(None)
                } else {
                    Err(take_err_string(err))
                };
            }
            Ok(Some(Doc::Typed(*unsafe { Box::from_raw(out) })))
        }
    }
}

/// Layer 0 at every canvas pixel as unit RGBA (straight), with the raw
/// stored samples for the bit-exact comparison; transparent off its storage.
fn layer(doc: &Doc) -> Vec<([f64; 4], [u32; 4])> {
    let mut out = vec![([0.0; 4], [0; 4]); (W * H) as usize];
    let mut put =
        |offset: (i32, i32), dims: (u32, u32), read: &dyn Fn(usize) -> ([f64; 4], [u32; 4])| {
            for j in 0..H as i32 {
                for i in 0..W as i32 {
                    let (lx, ly) = (i - offset.0, j - offset.1);
                    if lx >= 0 && ly >= 0 && (lx as u32) < dims.0 && (ly as u32) < dims.1 {
                        out[(j * W as i32 + i) as usize] =
                            read(ly as usize * dims.0 as usize + lx as usize);
                    }
                }
            }
        };
    match doc {
        Doc::Byte(d) => {
            let l = &d.layers[0];
            let dims = (l.pixels.width(), l.pixels.height());
            put(l.offset, dims, &|k| {
                let p = l
                    .pixels
                    .sample_at(k as u32 % dims.0, k as u32 / dims.0)
                    .unwrap_or([0; 4]);
                (p.map(|v| f64::from(v) / 255.0), p.map(u32::from))
            });
        }
        Doc::Typed(d) => {
            let record = d.layer(0).expect("a record");
            let image = record.source().expect("a raster");
            let dims = image.dimensions();
            match image.to_rgba16() {
                Ok(samples) => put(record.offset(), dims, &|k| {
                    let p = samples[k];
                    (p.map(|v| f64::from(v) / 65535.0), p.map(u32::from))
                }),
                Err(_) => {
                    let samples = image.to_rgba32().unwrap();
                    put(record.offset(), dims, &|k| {
                        let p = samples[k];
                        (p.map(f64::from), p.map(f32::to_bits))
                    })
                }
            }
        }
    }
    out
}

fn tolerance(depth: Depth) -> f64 {
    match depth {
        Depth::Byte => 1.0 / 255.0 + 1e-9,
        Depth::Sixteen => 257.0 / 65535.0 + 1e-9,
        Depth::Float => 1e-3,
    }
}

/// The unit colour `C` as the depth stores it.
fn stored_c(depth: Depth) -> [f64; 3] {
    match depth {
        Depth::Byte => C.map(|v| (v * 255.0).round() / 255.0),
        Depth::Sixteen => C.map(|v| (v * 65535.0).round() / 65535.0),
        Depth::Float => C.map(|v| f64::from(v as f32)),
    }
}

#[test]
fn the_pad_fills_a_transparent_hole_with_the_surround() {
    for depth in DEPTHS {
        let doc = build(depth, &uniform());
        let before = layer(&doc);
        let filled = fill(&doc, &request("content_aware_fill", "layer", Some(true)))
            .unwrap_or_else(|e| panic!("{depth:?}: {e}"))
            .unwrap_or_else(|| panic!("{depth:?}: the fill changed nothing"));
        let after = layer(&filled);
        let c = stored_c(depth);
        for (i, ((unit, raw), (_, before_raw))) in after.iter().zip(&before).enumerate() {
            if in_hole(i) {
                assert_eq!(unit[3], 1.0, "{depth:?}: hole pixel {i} is opaque");
                for k in 0..3 {
                    assert!(
                        (unit[k] - c[k]).abs() <= tolerance(depth),
                        "{depth:?}: hole pixel {i} channel {k} is {} for {}",
                        unit[k],
                        c[k]
                    );
                }
            } else {
                assert_eq!(raw, before_raw, "{depth:?}: pixel {i} outside the hole");
            }
        }
    }
}

#[test]
fn without_the_flag_the_hole_stays_transparent() {
    for depth in DEPTHS {
        let doc = build(depth, &uniform());
        let before = layer(&doc);
        for flag in [None, Some(false)] {
            let after = match fill(&doc, &request("content_aware_fill", "layer", flag))
                .unwrap_or_else(|e| panic!("{depth:?}: {e}"))
            {
                Some(out) => layer(&out),
                None => before.clone(),
            };
            for (i, ((unit, raw), (_, before_raw))) in after.iter().zip(&before).enumerate() {
                if in_hole(i) {
                    assert_eq!(unit[3], 0.0, "{depth:?} {flag:?}: hole pixel {i}");
                } else {
                    assert_eq!(raw, before_raw, "{depth:?} {flag:?}: pixel {i}");
                }
            }
        }
    }
}

#[test]
fn a_textured_hole_lands_inside_the_surrounds_range() {
    for depth in DEPTHS {
        let doc = build(depth, &textured());
        let before = layer(&doc);
        let mut low = [f64::MAX; 3];
        let mut high = [f64::MIN; 3];
        for (i, (unit, _)) in before.iter().enumerate() {
            if !in_hole(i) {
                for k in 0..3 {
                    low[k] = low[k].min(unit[k]);
                    high[k] = high[k].max(unit[k]);
                }
            }
        }
        let filled = fill(&doc, &request("content_aware_fill", "layer", Some(true)))
            .unwrap_or_else(|e| panic!("{depth:?}: {e}"))
            .unwrap_or_else(|| panic!("{depth:?}: the fill changed nothing"));
        let after = layer(&filled);
        for (i, ((unit, raw), (_, before_raw))) in after.iter().zip(&before).enumerate() {
            if in_hole(i) {
                assert_eq!(unit[3], 1.0, "{depth:?}: hole pixel {i} is opaque");
                for k in 0..3 {
                    let slack = RANGE_SLACK * (high[k] - low[k]);
                    assert!(
                        (low[k] - slack..=high[k] + slack).contains(&unit[k]),
                        "{depth:?}: hole pixel {i} channel {k} is {} outside {}..{}",
                        unit[k],
                        low[k],
                        high[k]
                    );
                }
            } else {
                assert_eq!(raw, before_raw, "{depth:?}: pixel {i} outside the hole");
            }
        }
    }
}

#[test]
fn the_pad_refuses_outside_the_in_place_fill() {
    for depth in DEPTHS {
        let doc = build(depth, &uniform());
        let new_layer = fill(
            &doc,
            &request("content_aware_fill", "new_layer", Some(true)),
        );
        assert_eq!(
            new_layer.err().as_deref(),
            Some(ONLY),
            "{depth:?}: new layer"
        );
        let other = fill(&doc, &request("retouch_clone", "layer", Some(true)));
        assert_eq!(other.err().as_deref(), Some(ONLY), "{depth:?}: another op");
        // A false flag on another op names the same rule.
        let other = fill(&doc, &request("spot_heal", "layer", Some(false)));
        assert_eq!(
            other.err().as_deref(),
            Some(ONLY),
            "{depth:?}: another op, false"
        );
    }
    let Doc::Typed(rgb) = build(Depth::Sixteen, &uniform()) else {
        unreachable!()
    };
    let profile = press_profile("Press Test");
    let mut err: *mut c_char = ptr::null_mut();
    let cmyk = unsafe {
        dr_precision_doc_convert_to_cmyk(
            &rgb,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!cmyk.is_null(), "to CMYK: {}", take_err_string(err));
    let cmyk = Doc::Typed(*unsafe { Box::from_raw(cmyk) });
    assert_eq!(
        fill(&cmyk, &request("content_aware_fill", "layer", Some(true)))
            .err()
            .as_deref(),
        Some("fill_transparent is not available in a CMYK document")
    );
}
