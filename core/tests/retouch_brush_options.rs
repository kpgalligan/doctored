//! The retouch brushes' two options, through the FFI,
//! against oracles written here in f64:
//!
//! * the Sponge's Vibrance on uniform patches at 8, 16 and 32 bits: the
//!   Vibrance stage (`common/vibrance_reference.rs`, the runner's recorded
//!   law) in linear light through the sRGB curve, at float through the sRGB
//!   display encoding's Scaled form, and on a ProPhoto document through
//!   ProPhoto's own curve, as the adjustment's Vibrance runs there;
//! * Sample All Layers on Blur and Smudge against a manually flattened
//!   picture of every visible layer (`SAMPLE_ALL_LAYERS` is `All`): the
//!   alpha-weighted Gaussian of that picture and the premultiplied mix of all
//!   four channels toward it, an empty target filled from it at 8, 16 and 32
//!   bits, a visible layer above read and a hidden one not, and Smudge's
//!   write rule;
//! * an offset layer reaching past the canvas keeps its off-canvas pixels;
//! * the 16-bit sampled Blur and Smudge admitted at exactly their exported
//!   plan, refused one byte below it;
//! * the two option refusals by name.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::doc::{DrDocument, LayerRaster};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_color::dr_doc_assign_profile;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_retouch_brush::*;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::retouch_sampling::RetouchSampling;
use doctored_core::{RegionRequest, SAMPLE_ALL_LAYERS};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::{mask_fixture, solid, take_err_string};
use crate::float_fixture::float_doc;
use crate::native_scalar::{doc as doc16, LIMIT};
use crate::vibrance_reference::{self, Trc};

const HARD: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;

// --------------------------------------------------------------- oracles --

/// The Vibrance stage at slider `v` (a fraction) on one encoded triple
/// through the sRGB curve: decoded, the stage in linear light
/// (`vibrance_reference::stage`), encoded.
fn vibrance(rgb: [f64; 3], v: f64) -> [f64; 3] {
    vibrance_reference::stage(rgb.map(|c| srgb_decode(c.clamp(0.0, 1.0))), v).map(srgb_encode)
}

fn srgb_encode(v: f64) -> f64 {
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// The Scaled display form of `master` on a linear triple (`display_encoding`
/// rule A): `m = max(1, max rgb)`, `c = rgb / m`, `m·(D(master(E(max(c, 0))))
/// + min(c, 0))`.
fn scaled(rgb: [f64; 3], master: impl Fn([f64; 3]) -> [f64; 3]) -> [f64; 3] {
    let m = rgb.iter().fold(1.0_f64, |m, &v| m.max(v));
    let c = rgb.map(|v| v / m);
    let out = master(c.map(|v| srgb_encode(v.max(0.0))));
    std::array::from_fn(|k| m * (srgb_decode(out[k]) + c[k].min(0.0)))
}

fn taps(sigma: f64) -> Vec<f64> {
    let half = (3.0 * sigma).ceil() as i64;
    let raw: Vec<f64> = (-half..=half)
        .map(|k| (-0.5 * (k as f64 / sigma).powi(2)).exp())
        .collect();
    let sum: f64 = raw.iter().sum();
    raw.iter().map(|v| v / sum).collect()
}

/// The alpha-weighted Gaussian (sigma 2) of a straight unit picture with
/// reads clamped to its edges; the pixel's own colour where no weight lands.
fn blur_of(picture: &[[f64; 4]], (w, h): (u32, u32), x: u32, y: u32) -> [f64; 3] {
    let k = taps(2.0);
    let half = (k.len() / 2) as i64;
    let at = |x: i64, y: i64| {
        let (cx, cy) = (x.clamp(0, i64::from(w) - 1), y.clamp(0, i64::from(h) - 1));
        picture[(cy * i64::from(w) + cx) as usize]
    };
    let (mut acc, mut total) = ([0.0; 3], 0.0);
    for (j, ky) in k.iter().enumerate() {
        for (i, kx) in k.iter().enumerate() {
            let p = at(
                i64::from(x) + i as i64 - half,
                i64::from(y) + j as i64 - half,
            );
            let weight = kx * ky * p[3];
            for c in 0..3 {
                acc[c] += weight * p[c];
            }
            total += weight;
        }
    }
    if total > 0.0 {
        acc.map(|v| v / total)
    } else {
        let p = at(i64::from(x), i64::from(y));
        [p[0], p[1], p[2]]
    }
}

/// Sample All Layers' write in f64: the premultiplied mix of all four
/// channels of `layer` toward `target` by `t`.
fn mix_over(layer: [f64; 4], target: [f64; 4], t: f64) -> [f64; 4] {
    let alpha = (1.0 - t) * layer[3] + t * target[3];
    let mut out = layer;
    if alpha > 0.0 {
        for c in 0..3 {
            out[c] = ((1.0 - t) * layer[c] * layer[3] + t * target[c] * target[3]) / alpha;
        }
    }
    out[3] = alpha;
    out
}

fn close(got: f64, want: f64, tolerance: f64, what: &str) {
    assert!(
        (got - want).abs() <= tolerance,
        "{what}: {got} against {want}"
    );
}

// ------------------------------------------------------------- byte calls --

fn handle(doc: &DrDocument) -> *mut DrDocument {
    Box::into_raw(Box::new(doc.clone()))
}

/// `dr_doc_retouch_brush_layer` under full coverage: the result, or the
/// error sentence.
fn byte_brush(
    doc: &DrDocument,
    idx: usize,
    overlay: &[u8],
    kind: u8,
    strength: f32,
    vibrance: bool,
    sample_all_layers: bool,
) -> Result<Option<DrDocument>, String> {
    let d = handle(doc);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_retouch_brush_layer(
            d,
            idx,
            overlay.as_ptr(),
            doc.width,
            doc.height,
            kind,
            strength,
            vibrance,
            sample_all_layers,
            &mut err,
        )
    };
    unsafe { dr_doc_free(d) };
    if !err.is_null() {
        return Err(take_err_string(err));
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

fn byte_smudge(
    doc: &DrDocument,
    idx: usize,
    dabs: &[[f64; 4]],
    strength: f32,
    sample_all_layers: bool,
) -> Result<Option<DrDocument>, String> {
    let d = handle(doc);
    let brush = CString::new(HARD).unwrap();
    let flat: Vec<f64> = dabs.iter().flatten().copied().collect();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_smudge_layer(
            d,
            idx,
            brush.as_ptr(),
            flat.as_ptr(),
            dabs.len(),
            strength,
            ptr::null(),
            0,
            sample_all_layers,
            &mut err,
        )
    };
    unsafe { dr_doc_free(d) };
    if !err.is_null() {
        return Err(take_err_string(err));
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

/// A canvas overlay whose alpha is `f(x, y)` (its colour bytes are junk).
fn overlay(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            out.extend_from_slice(&[(x * 13) as u8, 7, 201, f(x, y)]);
        }
    }
    out
}

fn layer_bytes(doc: &DrDocument, idx: usize) -> RgbaImage {
    doc.layers[idx].pixels.to_contiguous().unwrap()
}

fn unit8(p: [u8; 4]) -> [f64; 4] {
    p.map(|v| f64::from(v) / 255.0)
}

/// [`stack8`]'s structured opaque background at (x, y).
fn background8(x: u32, y: u32) -> Rgba<u8> {
    Rgba([
        ((x * 37 + y * 11) % 256) as u8,
        ((x * 5 + y * 53) % 256) as u8,
        if (x / 4 + y / 4).is_multiple_of(2) {
            30
        } else {
            220
        },
        255,
    ])
}

/// [`stack8`]'s layer above at column `x`: opaque over the right half.
fn patch8(w: u32, x: u32) -> Rgba<u8> {
    if x >= w / 2 {
        Rgba([250, 10, 10, 255])
    } else {
        Rgba([0, 0, 0, 0])
    }
}

/// A structured opaque background under an empty target (layer 1) and, when
/// `above`, an opaque patch layer over the target (layer 2).
fn stack8(w: u32, h: u32, target: RgbaImage, above: bool) -> DrDocument {
    let background = RgbaImage::from_fn(w, h, background8);
    let mut doc = DrDocument::from_pixels(background)
        .adding_image_layer(0, target, "Target")
        .expect("a target layer");
    if above {
        let patch = RgbaImage::from_fn(w, h, |x, _| patch8(w, x));
        doc = doc
            .adding_image_layer(1, patch, "Above")
            .expect("a layer above");
    }
    doc
}

/// The picture `SAMPLE_ALL_LAYERS` (`All`) names for a target of `doc`: the
/// projection of every visible layer.
fn byte_picture(doc: &DrDocument) -> Vec<[f64; 4]> {
    let flat = doc.flattened().expect("the document flattens");
    flat.pixels().map(|p| unit8(p.0)).collect()
}

/// The picture of a [`stack8`] document with an EMPTY target, flattened by
/// hand: the patch where it is opaque (when `above`), else the background.
fn hand_picture8(w: u32, h: u32, above: bool) -> Vec<[f64; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let patch = patch8(w, x);
            unit8(if above && patch.0[3] == 255 {
                patch.0
            } else {
                background8(x, y).0
            })
        })
        .collect()
}

// --------------------------------------------------------------- Vibrance --

const PATCHES: [[u8; 3]; 5] = [
    [180, 120, 90],
    [40, 160, 210],
    [128, 128, 128],
    [250, 20, 30],
    [90, 200, 100],
];

#[test]
fn byte_vibrance_is_the_vibrance_stage_on_a_uniform_patch() {
    for rgb in PATCHES {
        let doc = DrDocument::from_pixels(solid(6, 5, [rgb[0], rgb[1], rgb[2], 255]));
        let full = overlay(6, 5, |_, _| 255);
        for (kind, slider) in [(2_u8, 1.0), (3_u8, -1.0)] {
            for strength in [1.0_f32, 0.5] {
                let source = rgb.map(|v| f64::from(v) / 255.0);
                let target = Trc::of(IccProfile::srgb().bytes()).vibrance(source, slider);
                let out =
                    byte_brush(&doc, 0, &full, kind, strength, true, false).expect("no refusal");
                let pixels = out.map_or_else(|| layer_bytes(&doc, 0), |d| layer_bytes(&d, 0));
                for p in pixels.pixels() {
                    for c in 0..3 {
                        let want =
                            (source[c] + f64::from(strength) * (target[c] - source[c])) * 255.0;
                        close(
                            f64::from(p.0[c]),
                            want,
                            1.0,
                            &format!("{rgb:?} kind {kind}"),
                        );
                    }
                    assert_eq!(p.0[3], 255, "alpha is never written");
                }
            }
        }
    }
}

#[test]
fn sixteen_bit_vibrance_is_the_vibrance_stage_on_a_uniform_patch() {
    for rgb in PATCHES {
        let code = rgb.map(|v| u16::from(v) * 257);
        let d = doc16(5, 4, vec![[code[0], code[1], code[2], 65535]; 20]);
        for (kind, slider) in [("saturate", 1.0), ("desaturate", -1.0)] {
            let request = json!({"op":"retouch_brush","layer":0,"kind":kind,
                "strength":1.0,"vibrance":true});
            let out = crate::native_scalar::edit(&d, request, &[65535; 20]);
            let source = code.map(|v| f64::from(v) / 65535.0);
            let target = Trc::of(IccProfile::srgb().bytes()).vibrance(source, slider);
            let got = out
                .unwrap_or_else(|| d.clone())
                .layer_source(0)
                .unwrap()
                .to_rgba16()
                .unwrap();
            for p in got {
                for c in 0..3 {
                    close(
                        f64::from(p[c]),
                        target[c] * 65535.0,
                        3.0,
                        &format!("{rgb:?} {kind}"),
                    );
                }
                assert_eq!(p[3], 65535);
            }
        }
    }
}

/// On a ProPhoto document the Sponge's Vibrance target is the adjustment's
/// Vibrance stage at ±100 in ProPhoto's own linear light (#578): the 8-bit
/// document assigned builtin ProPhoto through `dr_doc_assign_profile`, the
/// 16-bit one built on it; within 1 code and 2/65535 of the oracle through
/// ProPhoto's curve, and on at least one patch more than twice that
/// tolerance away from the sRGB curve's result.
#[test]
fn sponge_vibrance_follows_the_documents_profile() {
    let prophoto = IccProfile::prophoto_rgb();
    let curve = Trc::of(prophoto.bytes());
    let srgb = Trc::of(IccProfile::srgb().bytes());
    let mut differs = false;
    for rgb in PATCHES {
        let source = rgb.map(|v| f64::from(v) / 255.0);
        let plain = DrDocument::from_pixels(solid(6, 5, [rgb[0], rgb[1], rgb[2], 255]));
        let d = handle(&plain);
        let mut err = ptr::null_mut();
        let assigned = unsafe {
            dr_doc_assign_profile(
                d,
                prophoto.bytes().as_ptr(),
                prophoto.bytes().len(),
                &mut err,
            )
        };
        unsafe { dr_doc_free(d) };
        assert!(!assigned.is_null(), "{}", take_err_string(err));
        let doc = *unsafe { Box::from_raw(assigned) };
        let full = overlay(6, 5, |_, _| 255);
        for (kind, slider) in [(2_u8, 1.0), (3_u8, -1.0)] {
            let want = curve.vibrance(source, slider);
            differs |=
                (0..3).any(|c| ((want[c] - srgb.vibrance(source, slider)[c]) * 255.0).abs() > 2.0);
            let out = byte_brush(&doc, 0, &full, kind, 1.0, true, false).expect("no refusal");
            let pixels = out.map_or_else(|| layer_bytes(&doc, 0), |d| layer_bytes(&d, 0));
            for p in pixels.pixels() {
                for (c, w) in want.iter().enumerate() {
                    close(
                        f64::from(p.0[c]),
                        w * 255.0,
                        1.0,
                        &format!("8-bit ProPhoto {rgb:?} kind {kind}"),
                    );
                }
            }
        }
        let code = rgb.map(|v| u16::from(v) * 257);
        let image = PrecisionImage::from_rgba16(
            5,
            4,
            vec![[code[0], code[1], code[2], 65535]; 20],
            prophoto.clone(),
            None,
        )
        .unwrap();
        let d16 = PrecisionDocument::from_parts(
            5,
            4,
            prophoto.clone(),
            Default::default(),
            Default::default(),
            vec![PrecisionDocumentLayer::raster("Patch", image, (0, 0), 1.0, true).unwrap()],
        )
        .unwrap();
        let source = code.map(|v| f64::from(v) / 65535.0);
        for (kind, slider) in [("saturate", 1.0), ("desaturate", -1.0)] {
            let request = json!({"op":"retouch_brush","layer":0,"kind":kind,
                "strength":1.0,"vibrance":true});
            let out = crate::native_scalar::edit(&d16, request, &[65535; 20]);
            let want = curve.vibrance(source, slider);
            let got = out
                .unwrap_or_else(|| d16.clone())
                .layer_source(0)
                .unwrap()
                .to_rgba16()
                .unwrap();
            for p in got {
                for (c, w) in want.iter().enumerate() {
                    close(
                        f64::from(p[c]),
                        w * 65535.0,
                        2.0,
                        &format!("16-bit ProPhoto {rgb:?} {kind}"),
                    );
                }
            }
        }
    }
    assert!(
        differs,
        "ProPhoto's curve moves no patch differently from sRGB's"
    );
}

#[test]
fn float_vibrance_runs_the_stage_through_the_scaled_display_form() {
    // Linear patches in range, and one above white (m > 1).
    let patches: [[f32; 3]; 4] = [
        [0.45, 0.2, 0.1],
        [0.05, 0.3, 0.6],
        [0.25, 0.25, 0.25],
        [2.0, 0.6, 0.3],
    ];
    for rgb in patches {
        let pixels = vec![[rgb[0], rgb[1], rgb[2], 1.0]; 12];
        let d = float_doc(4, 3, &IccProfile::srgb(), &[pixels]);
        for (kind, slider) in [("saturate", 1.0), ("desaturate", -1.0)] {
            let request = json!({"op":"retouch_brush","layer":0,"kind":kind,
                "strength":1.0,"vibrance":true});
            let out = crate::native_scalar::edit(&d, request, &[65535; 12]).expect("a change");
            let want = scaled(rgb.map(f64::from), |c| vibrance(c, slider));
            for p in out.layer_source(0).unwrap().to_rgba32().unwrap() {
                for c in 0..3 {
                    // The display encoder is the policy's own tone encoder,
                    // within its interval of the closed-form sRGB curve.
                    let tolerance = 2e-3 * want[c].abs().max(1.0);
                    close(
                        f64::from(p[c]),
                        want[c],
                        tolerance,
                        &format!("{rgb:?} {kind}"),
                    );
                }
                assert_eq!(p[3], 1.0);
            }
        }
    }
}

// ------------------------------------------------------ Sample All Layers --

#[test]
fn byte_sampled_blur_mixes_the_layer_toward_the_flattened_picture() {
    assert_eq!(SAMPLE_ALL_LAYERS, RetouchSampling::All);
    let (w, h) = (24_u32, 18_u32);
    // A target of every alpha class over the background, a layer above.
    let target = RgbaImage::from_fn(w, h, |x, y| {
        let alpha = match (x + y) % 5 {
            0 => 0,
            1 => 120,
            _ => 255,
        };
        Rgba([((x * 9) % 256) as u8, 200, ((y * 13) % 256) as u8, alpha])
    });
    let doc = stack8(w, h, target, true);
    let picture = byte_picture(&doc);
    let strength = 0.7_f32;
    let cover = |x: u32, y: u32| ((x * 7 + y * 3) % 256) as u8;
    let full = overlay(w, h, cover);
    let out = byte_brush(&doc, 1, &full, 0, strength, false, true)
        .expect("no refusal")
        .expect("a change");
    let before = layer_bytes(&doc, 1);
    let after = layer_bytes(&out, 1);
    for y in 0..h {
        for x in 0..w {
            let old = before.get_pixel(x, y).0;
            let got = after.get_pixel(x, y).0;
            let t = f64::from(cover(x, y)) / 255.0 * f64::from(strength);
            if cover(x, y) == 0 {
                assert_eq!(got, old, "uncovered ({x}, {y})");
                continue;
            }
            let blurred = blur_of(&picture, (w, h), x, y);
            let alpha = picture[(y * w + x) as usize][3];
            let want = mix_over(unit8(old), [blurred[0], blurred[1], blurred[2], alpha], t);
            close(f64::from(got[3]), want[3] * 255.0, 1.0, "alpha");
            if got[3] > 0 {
                for c in 0..3 {
                    close(
                        f64::from(got[c]),
                        want[c] * 255.0,
                        1.0,
                        &format!("({x}, {y})"),
                    );
                }
            }
        }
    }
    // The other layers are untouched.
    assert_eq!(layer_bytes(&out, 0), layer_bytes(&doc, 0));
    assert_eq!(layer_bytes(&out, 2), layer_bytes(&doc, 2));
}

#[test]
fn byte_sampled_blur_fills_an_empty_target_from_every_visible_layer() {
    assert_eq!(SAMPLE_ALL_LAYERS, RetouchSampling::All);
    let (w, h) = (20_u32, 14_u32);
    let empty = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 0]));
    let full = overlay(w, h, |_, _| 255);
    let with_above = stack8(w, h, empty.clone(), true);
    let without = stack8(w, h, empty, false);
    let filled = |doc: &DrDocument| {
        let out = byte_brush(doc, 1, &full, 0, 1.0, false, true)
            .expect("no refusal")
            .expect("an empty layer fills");
        layer_bytes(&out, 1)
    };
    let a = filled(&with_above);
    let b = filled(&without);
    // An empty target at full strength becomes [blur(picture), picture α],
    // the picture flattened by hand from every visible layer.
    let picture = hand_picture8(w, h, true);
    assert_eq!(
        picture,
        byte_picture(&with_above),
        "the hand-flattened picture is the projection"
    );
    for y in 0..h {
        for x in 0..w {
            let blurred = blur_of(&picture, (w, h), x, y);
            let p = a.get_pixel(x, y).0;
            close(
                f64::from(p[3]),
                picture[(y * w + x) as usize][3] * 255.0,
                1.0,
                "α",
            );
            for c in 0..3 {
                close(
                    f64::from(p[c]),
                    blurred[c] * 255.0,
                    1.0,
                    &format!("({x}, {y})"),
                );
            }
        }
    }
    assert_ne!(a, b, "the visible layer above is read");
    let hidden = with_above
        .with_layer_visible(2, false)
        .expect("the layer above hides");
    assert_eq!(filled(&hidden), b, "a hidden layer above is not read");
}

#[test]
fn byte_sampled_smudge_writes_only_what_the_walk_moved() {
    assert_eq!(SAMPLE_ALL_LAYERS, RetouchSampling::All);
    let (w, h) = (40_u32, 24_u32);
    let empty = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 0]));
    let doc = stack8(w, h, empty, true);
    let picture = byte_picture(&doc);
    let dabs = [
        [8.0, 12.0, 10.0, 1.0],
        [16.0, 12.0, 10.0, 1.0],
        [24.0, 12.0, 10.0, 1.0],
    ];
    let out = byte_smudge(&doc, 1, &dabs, 0.6, true)
        .expect("no refusal")
        .expect("a smear");
    // The oracle field: the plain Smudge of a layer holding the picture.
    let bytes = RgbaImage::from_fn(w, h, |x, y| {
        let p = picture[(y * w + x) as usize];
        Rgba(p.map(|v| (v * 255.0).round() as u8))
    });
    let mut plain_doc = doc.clone();
    plain_doc.layers[1].pixels = LayerRaster::new(bytes.clone());
    let plain = byte_smudge(&plain_doc, 1, &dabs, 0.6, false)
        .expect("no refusal")
        .expect("a smear");
    let plain = layer_bytes(&plain, 1);
    let got = layer_bytes(&out, 1);
    let mut moved = 0;
    for y in 0..h {
        for x in 0..w {
            let (g, p, start) = (
                got.get_pixel(x, y).0,
                plain.get_pixel(x, y).0,
                bytes.get_pixel(x, y).0,
            );
            if p != start {
                // A moved sample is written straight with its alpha.
                assert_eq!(g, p, "moved ({x}, {y})");
                moved += 1;
            } else {
                // Left alone: the layer's own bytes (or the same picture
                // bytes when the move quantized back to them).
                assert!(g == [0; 4] || g == p, "unmoved ({x}, {y}): {g:?}");
            }
        }
    }
    assert!(moved > 0, "the smear moved something");
}

/// A 200 × 100 layer at (−50, 0) on a 100 × 100 canvas, 8 bits.
fn offset_byte_layer() -> DrDocument {
    let image = RgbaImage::from_fn(200, 100, |x, y| {
        Rgba([
            (x % 256) as u8,
            ((y * 2) % 256) as u8,
            ((x + y) % 256) as u8,
            255,
        ])
    });
    let mut doc = mask_fixture((100, 100), (200, 100), (-50, 0));
    doc.layers[1].pixels = LayerRaster::new(image);
    doc
}

fn off_canvas(image: &RgbaImage) -> Vec<[u8; 4]> {
    image
        .enumerate_pixels()
        .filter(|(x, _, _)| *x < 50 || *x >= 150)
        .map(|(_, _, p)| p.0)
        .collect()
}

#[test]
fn byte_sampling_never_touches_a_layers_pixels_outside_the_canvas() {
    let doc = offset_byte_layer();
    let before = off_canvas(&layer_bytes(&doc, 1));
    let disc = overlay(100, 100, |x, y| {
        let d = (f64::from(x) - 5.0).hypot(f64::from(y) - 50.0);
        if d <= 20.0 {
            255
        } else {
            0
        }
    });
    let blurred = byte_brush(&doc, 1, &disc, 0, 1.0, false, true)
        .expect("no error")
        .expect("the on-canvas part changes");
    assert_eq!(off_canvas(&layer_bytes(&blurred, 1)), before, "blur");
    let dabs = [
        [5.0, 50.0, 40.0, 1.0],
        [15.0, 50.0, 40.0, 1.0],
        [25.0, 50.0, 40.0, 1.0],
    ];
    let smudged = byte_smudge(&doc, 1, &dabs, 1.0, true)
        .expect("no error")
        .expect("the on-canvas part smears");
    assert_eq!(off_canvas(&layer_bytes(&smudged, 1)), before, "smudge");
}

// --------------------------------------------------------- 16-bit sampling --

/// [`stack16_with`]'s structured opaque background at (x, y).
fn background16(x: u32, y: u32) -> [u16; 4] {
    [
        ((x * 2311 + y * 409) % 65536) as u16,
        ((x * 977 + y * 4099) % 65536) as u16,
        if (x / 3 + y / 3).is_multiple_of(2) {
            9000
        } else {
            60000
        },
        65535,
    ]
}

fn stack16(w: u32, h: u32, above: bool) -> PrecisionDocument {
    let target: Vec<[u16; 4]> = (0..(w * h) as usize)
        .map(|i| match i % 4 {
            0 => [0; 4],
            1 => [40000, 1000, 30000, 30000],
            _ => [12000, 50000, 22000, 65535],
        })
        .collect();
    stack16_with(w, h, target, above.then_some(true))
}

/// A structured opaque background under `target` (layer 1) and, with
/// `Some(visible)`, an opaque red patch over the right half (layer 2,
/// "Above") at that visibility.
fn stack16_with(w: u32, h: u32, target: Vec<[u16; 4]>, above: Option<bool>) -> PrecisionDocument {
    let n = (w * h) as usize;
    let background: Vec<[u16; 4]> = (0..n)
        .map(|i| background16(i as u32 % w, i as u32 / w))
        .collect();
    let image = |pixels: Vec<[u16; 4]>| {
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap()
    };
    let mut layers = vec![
        PrecisionDocumentLayer::raster("Background", image(background), (0, 0), 1.0, true).unwrap(),
        PrecisionDocumentLayer::raster("Target", image(target), (0, 0), 1.0, true).unwrap(),
    ];
    if let Some(visible) = above {
        let patch = (0..n)
            .map(|i| {
                if i as u32 % w >= w / 2 {
                    [65535, 0, 0, 65535]
                } else {
                    [0; 4]
                }
            })
            .collect();
        layers.push(
            PrecisionDocumentLayer::raster("Above", image(patch), (0, 0), 1.0, visible).unwrap(),
        );
    }
    PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Default::default(),
        Default::default(),
        layers,
    )
    .unwrap()
}

/// The typed render of the picture `SAMPLE_ALL_LAYERS` (`All`) names for
/// target 1: every visible layer of [`stack16`].
fn picture16(w: u32, h: u32, above: bool) -> Vec<[f64; 4]> {
    let source = stack16(w, h, above);
    render16(&source)
}

/// The typed render of the whole of `source`, straight, in unit range.
fn render16(source: &PrecisionDocument) -> Vec<[f64; 4]> {
    let (w, h) = source.dimensions();
    let request = RegionRequest {
        x: 0,
        y: 0,
        width: w,
        height: h,
        preview_divisor: 1,
    };
    source
        .render_region(request, LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
        .iter()
        .map(|p| p.map(|v| f64::from(v) / 65535.0))
        .collect()
}

/// The picture of a [`stack16_with`] document with an EMPTY target,
/// flattened by hand: the red patch where it is opaque (when `above`), else
/// the background.
fn hand_picture16(w: u32, h: u32, above: bool) -> Vec<[f64; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if above && x >= w / 2 {
                [1.0, 0.0, 0.0, 1.0]
            } else {
                background16(x, y).map(|v| f64::from(v) / 65535.0)
            }
        })
        .collect()
}

fn samples16(d: &PrecisionDocument, layer: usize) -> Vec<[u16; 4]> {
    d.layer_source(layer).unwrap().to_rgba16().unwrap()
}

#[test]
fn sixteen_bit_sampled_blur_mixes_toward_the_rendered_picture() {
    assert_eq!(SAMPLE_ALL_LAYERS, RetouchSampling::All);
    let (w, h) = (19_u32, 15_u32);
    let d = stack16(w, h, true);
    let picture = picture16(w, h, true);
    let n = (w * h) as usize;
    let cover: Vec<u16> = (0..n).map(|i| ((i * 7919) % 65536) as u16).collect();
    let request = json!({"op":"retouch_brush","layer":1,"kind":"blur","strength":0.8,
        "sample_all_layers":true});
    let out = crate::native_scalar::edit(&d, request, &cover).expect("a change");
    let (before, after) = (samples16(&d, 1), samples16(&out, 1));
    for i in 0..n {
        let (x, y) = (i as u32 % w, i as u32 / w);
        if cover[i] == 0 {
            assert_eq!(after[i], before[i]);
            continue;
        }
        let t = f64::from(cover[i]) / 65535.0 * 0.8;
        let b = blur_of(&picture, (w, h), x, y);
        let want = mix_over(
            before[i].map(|v| f64::from(v) / 65535.0),
            [b[0], b[1], b[2], picture[i][3]],
            t,
        );
        close(f64::from(after[i][3]), want[3] * 65535.0, 4.0, "alpha");
        if after[i][3] > 0 {
            for c in 0..3 {
                close(
                    f64::from(after[i][c]),
                    want[c] * 65535.0,
                    6.0,
                    &format!("pixel {i}"),
                );
            }
        }
    }
    assert_eq!(
        samples16(&out, 2),
        samples16(&d, 2),
        "the layer above stays"
    );
    let without = crate::native_scalar::edit(
        &stack16(w, h, false),
        json!({"op":"retouch_brush","layer":1,"kind":"blur","strength":0.8,
            "sample_all_layers":true}),
        &cover,
    )
    .expect("a change");
    assert_ne!(
        samples16(&without, 1),
        after,
        "the visible layer above is read"
    );
}

#[test]
fn sixteen_bit_sampled_blur_fills_an_empty_target_from_every_visible_layer() {
    assert_eq!(SAMPLE_ALL_LAYERS, RetouchSampling::All);
    let (w, h) = (20_u32, 14_u32);
    let n = (w * h) as usize;
    let empty = vec![[0_u16; 4]; n];
    let full = vec![65535_u16; n];
    let filled = |d: &PrecisionDocument| {
        let request = json!({"op":"retouch_brush","layer":1,"kind":"blur","strength":1.0,
            "sample_all_layers":true});
        samples16(
            &crate::native_scalar::edit(d, request, &full).expect("an empty layer fills"),
            1,
        )
    };
    let with_above = stack16_with(w, h, empty.clone(), Some(true));
    let a = filled(&with_above);
    let b = filled(&stack16_with(w, h, empty.clone(), None));
    let picture = hand_picture16(w, h, true);
    assert_eq!(
        picture,
        render16(&with_above),
        "the hand-flattened picture is the render"
    );
    for i in 0..n {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let blurred = blur_of(&picture, (w, h), x, y);
        close(f64::from(a[i][3]), picture[i][3] * 65535.0, 4.0, "alpha");
        for c in 0..3 {
            close(
                f64::from(a[i][c]),
                blurred[c] * 65535.0,
                6.0,
                &format!("({x}, {y})"),
            );
        }
    }
    assert_ne!(a, b, "the visible layer above is read");
    let hidden = filled(&stack16_with(w, h, empty, Some(false)));
    assert_eq!(hidden, b, "a hidden layer above is not read");
}

#[test]
fn float_sampled_blur_reads_the_layer_above() {
    assert_eq!(SAMPLE_ALL_LAYERS, RetouchSampling::All);
    let (w, h) = (40_u32, 12_u32);
    let n = (w * h) as usize;
    let background: Vec<[f32; 4]> = (0..n)
        .map(|i| {
            let (x, y) = (i as u32 % w, i as u32 / w);
            [
                (x * 37 + y * 11) as f32 % 97.0 / 97.0,
                (x * 5 + y * 53) as f32 % 89.0 / 89.0,
                if (x / 4 + y / 4).is_multiple_of(2) {
                    0.1
                } else {
                    0.85
                },
                1.0,
            ]
        })
        .collect();
    let empty = vec![[0.0_f32; 4]; n];
    let patch: Vec<[f32; 4]> = (0..n)
        .map(|i| {
            if i as u32 % w >= w / 2 {
                [1.0, 0.0, 1.0, 1.0]
            } else {
                [0.0; 4]
            }
        })
        .collect();
    let srgb = IccProfile::srgb();
    let request = json!({"op":"retouch_brush","layer":1,"kind":"blur","strength":1.0,
        "sample_all_layers":true});
    let full = vec![65535_u16; n];
    let blurred = |d: &PrecisionDocument| {
        crate::native_scalar::edit(d, request.clone(), &full)
            .expect("an empty layer fills")
            .layer_source(1)
            .unwrap()
            .to_rgba32()
            .unwrap()
    };
    let with_above = blurred(&float_doc(
        w,
        h,
        &srgb,
        &[background.clone(), empty.clone(), patch],
    ));
    let without = blurred(&float_doc(w, h, &srgb, &[background, empty]));
    for (i, p) in with_above.iter().enumerate() {
        if i as u32 % w >= w / 2 + 8 {
            for (c, want) in [1.0, 0.0, 1.0, 1.0].into_iter().enumerate() {
                close(
                    f64::from(p[c]),
                    want,
                    1e-4,
                    &format!("pixel {i} channel {c}"),
                );
            }
        }
    }
    assert_ne!(with_above, without, "the layer above is read");
}

fn smudge_plan16(d: &PrecisionDocument, layer: usize, dabs: &[f64], sample: bool) -> Value {
    let brush = CString::new(HARD).unwrap();
    let mut err = ptr::null_mut();
    let p = unsafe {
        dr_precision_doc_smudge_plan(
            d,
            layer,
            brush.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            false,
            sample,
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(!p.is_null(), "{}", take_err_string(err));
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    serde_json::from_str(&text).unwrap()
}

fn smudge16(
    d: &PrecisionDocument,
    layer: usize,
    dabs: &[f64],
    sample: bool,
    limit: u64,
    other: u64,
) -> Result<Option<PrecisionDocument>, String> {
    let brush = CString::new(HARD).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_smudge(
            d,
            layer,
            brush.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            0.6,
            ptr::null(),
            0,
            sample,
            limit,
            other,
            ptr::null(),
            &mut err,
        )
    };
    if !err.is_null() {
        return Err(take_err_string(err));
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

#[test]
fn sixteen_bit_sampled_smudge_writes_only_what_the_walk_moved() {
    let (w, h) = (36_u32, 20_u32);
    let d = stack16(w, h, true);
    let dabs = [
        8.0, 10.0, 10.0, 1.0, 16.0, 10.0, 10.0, 1.0, 24.0, 10.0, 10.0, 1.0,
    ];
    let out = smudge16(&d, 1, &dabs, true, LIMIT, 0)
        .expect("no refusal")
        .expect("a smear");
    // The oracle field: the plain Smudge of a layer holding the picture.
    let picture = picture16(w, h, true);
    let codes: Vec<[u16; 4]> = picture
        .iter()
        .map(|p| p.map(|v| (v * 65535.0).round() as u16))
        .collect();
    let image = PrecisionImage::from_rgba16(w, h, codes.clone(), IccProfile::srgb(), None).unwrap();
    let plain_doc = PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Default::default(),
        Default::default(),
        vec![PrecisionDocumentLayer::raster("Picture", image, (0, 0), 1.0, true).unwrap()],
    )
    .unwrap();
    let plain = samples16(
        &smudge16(&plain_doc, 0, &dabs, false, LIMIT, 0)
            .unwrap()
            .expect("a smear"),
        0,
    );
    let (before, got) = (samples16(&d, 1), samples16(&out, 1));
    let mut moved = 0;
    for i in 0..(w * h) as usize {
        if plain[i] != codes[i] {
            assert_eq!(got[i], plain[i], "moved {i}");
            moved += 1;
        } else {
            assert!(got[i] == before[i] || got[i] == plain[i], "unmoved {i}");
        }
    }
    assert!(moved > 0);
}

#[test]
fn sixteen_bit_sampling_never_touches_a_layers_pixels_outside_the_canvas() {
    let pixels: Vec<[u16; 4]> = (0..200 * 100)
        .map(|i| {
            let (x, y) = (i % 200, i / 200);
            [
                (x * 300) as u16,
                (y * 600) as u16,
                ((x + y) * 150) as u16,
                65535,
            ]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(200, 100, pixels, IccProfile::srgb(), None).unwrap();
    let d = PrecisionDocument::from_parts(
        100,
        100,
        IccProfile::srgb(),
        Default::default(),
        Default::default(),
        vec![PrecisionDocumentLayer::raster("Wide", image, (-50, 0), 1.0, true).unwrap()],
    )
    .unwrap();
    let off = |d: &PrecisionDocument| -> Vec<[u16; 4]> {
        samples16(d, 0)
            .into_iter()
            .enumerate()
            .filter(|(i, _)| i % 200 < 50 || i % 200 >= 150)
            .map(|(_, p)| p)
            .collect()
    };
    let before = off(&d);
    let disc: Vec<u16> = (0..100 * 100)
        .map(|i| {
            let (x, y) = (f64::from(i % 100), f64::from(i / 100));
            if (x - 5.0).hypot(y - 50.0) <= 20.0 {
                65535
            } else {
                0
            }
        })
        .collect();
    let blurred = crate::native_scalar::edit(
        &d,
        json!({"op":"retouch_brush","layer":0,"kind":"blur","strength":1.0,
            "sample_all_layers":true}),
        &disc,
    );
    let blurred = blurred.expect("the on-canvas part changes");
    assert_eq!(off(&blurred), before, "blur");
    let dabs = [
        5.0, 50.0, 40.0, 1.0, 15.0, 50.0, 40.0, 1.0, 25.0, 50.0, 40.0, 1.0,
    ];
    let smudged = smudge16(&d, 0, &dabs, true, LIMIT, 0)
        .expect("no error")
        .expect("the on-canvas part smears");
    assert_eq!(off(&smudged), before, "smudge");
}

fn edit_plan16(d: &PrecisionDocument, request: &Value) -> Value {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let p = unsafe { dr_precision_doc_edit_plan(d, text.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!p.is_null(), "{}", take_err_string(err));
    let out = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    serde_json::from_str(&out).unwrap()
}

fn edit16(
    d: &PrecisionDocument,
    request: &Value,
    input: &[u16],
    limit: u64,
    other: u64,
) -> Result<(), String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            d,
            text.as_ptr(),
            input.as_ptr(),
            input.len(),
            limit,
            other,
            ptr::null(),
            &mut err,
        )
    };
    if !out.is_null() {
        drop(unsafe { Box::from_raw(out) });
    }
    if err.is_null() {
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

#[test]
fn sixteen_bit_sampled_blur_and_smudge_are_admitted_at_exactly_their_exported_plan() {
    let (w, h) = (48_u32, 32_u32);
    let d = stack16(w, h, true);
    let other = 12_345_u64;
    let request = json!({"op":"retouch_brush","layer":1,"kind":"blur","strength":0.5,
        "sample_all_layers":true});
    let plan = edit_plan16(&d, &request);
    let plain = edit_plan16(
        &d,
        &json!({"op":"retouch_brush","layer":1,"kind":"blur","strength":0.5}),
    );
    let number = |p: &Value, key: &str| p[key].as_u64().unwrap();
    assert!(
        number(&plan, "scratch_bytes") > number(&plain, "scratch_bytes"),
        "the picture is charged"
    );
    let exact = number(&plan, "retained_bytes") + number(&plan, "scratch_bytes") + other;
    let cover = vec![65535_u16; (w * h) as usize];
    edit16(&d, &request, &cover, exact, other).expect("admitted at exactly the plan");
    assert!(
        edit16(&d, &request, &cover, exact - 1, other).is_err(),
        "one byte below the plan refuses"
    );
    let dabs = [
        8.0, 10.0, 12.0, 1.0, 20.0, 12.0, 12.0, 1.0, 30.0, 16.0, 12.0, 1.0,
    ];
    let plan = smudge_plan16(&d, 1, &dabs, true);
    let plain = smudge_plan16(&d, 1, &dabs, false);
    assert!(number(&plan, "scratch_bytes") > number(&plain, "scratch_bytes"));
    let exact = number(&plan, "retained_bytes") + number(&plan, "scratch_bytes") + other;
    smudge16(&d, 1, &dabs, true, exact, other).expect("admitted at exactly the plan");
    assert!(
        smudge16(&d, 1, &dabs, true, exact - 1, other).is_err(),
        "one byte below the plan refuses"
    );
}

// --------------------------------------------------------------- refusals --

#[test]
fn the_options_refuse_by_name_on_the_kinds_they_do_not_apply_to() {
    let doc = DrDocument::from_pixels(solid(6, 4, [90, 140, 200, 255]));
    let full = overlay(6, 4, |_, _| 255);
    assert_eq!(
        byte_brush(&doc, 0, &full, 0, 1.0, true, false)
            .err()
            .as_deref(),
        Some("vibrance applies to the sponge only")
    );
    assert_eq!(
        byte_brush(&doc, 0, &full, 3, 1.0, false, true)
            .err()
            .as_deref(),
        Some("sample_all_layers applies to blur, sharpen and smudge")
    );
    let d = doc16(6, 4, vec![[30000, 20000, 9000, 65535]; 24]);
    for (request, sentence) in [
        (
            json!({"op":"retouch_brush","layer":0,"kind":"sharpen","strength":1.0,
                "vibrance":true}),
            "vibrance applies to the sponge only",
        ),
        (
            json!({"op":"retouch_brush","layer":0,"kind":"saturate","strength":1.0,
                "sample_all_layers":true}),
            "sample_all_layers applies to blur, sharpen and smudge",
        ),
    ] {
        let text = CString::new(request.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let p = unsafe { dr_precision_doc_edit_plan(&d, text.as_ptr(), LIMIT, 0, &mut err) };
        assert!(p.is_null());
        assert_eq!(take_err_string(err), sentence);
    }
}
