//! #1 Lab phase 6b-ii: Auto-Align Layers' Vignette Removal and Geometric
//! Distortion, and Auto-Blend Layers (Stack Images, Panorama with Seamless
//! Tones) on a Lab Color document, through the FFI. A Lab layer's linear
//! light is its codes' CIE Y; Vignette Removal's gain multiplies their XYZ;
//! Auto-Blend scores, takes its seam gradient and fits Seamless Tones on L\*
//! alone. Every oracle is written here (the CIE formulas are
//! `lab_fixture`'s). Bounds, declared before measuring:
//!
//! * (a) Vignette Removal on `geometry_align_vignette.rs`'s take2 geometry
//!   (three 600 × 400 views yawed −20, 0 and +20°, the test's own maps), the
//!   picture neutral with V = 1 − 0.4ρ² applied to its XYZ, encoded as 16-bit
//!   Lab codes: the plan's coefficient is within 0.02 of −0.4 and its gain
//!   within 0.01 of the closed-form mean of V; over the opaque pixels the L\*
//!   codes' mean |Δ| from the picture × gain is ≤ 1.5 × 257; on a stack whose
//!   middle frame carries a\*/b\* every code is within 4 of the test's own
//!   XYZ × gain/V(ρ) → Lab (the reported fit); and the same neutral stack
//!   converted into ProPhoto RGB (relative, BPC) plans a coefficient within
//!   0.005 and a gain within 0.002 of the Lab plan's;
//! * (b) Geometric Distortion on `geometry_align_distortion.rs`'s value-noise
//!   oracle as neutral 16-bit Lab codes: `fit_distortion` finds k (−0.05 and
//!   +0.04) within 0.005, and every map puts the rect's corners within 1 px of
//!   the analytic map;
//! * (c) Stack Images (`seamless` true) on a strip stack at 8 and 16 bits:
//!   the masks equal the same request's on an sRGB-tagged RGB16 document
//!   whose R = G = B are the Lab layers' L\* codes, exactly; with a\* = b\* =
//!   0 the merged record equals the same request's on an RGB16 document
//!   holding the same samples, exactly;
//! * (d) Panorama with Seamless Tones on two offset opaque frames with an L\*
//!   step and chroma: each reported gain is `[g, 1, 1]`, g the red gain of
//!   the same request on an RGB16 document holding the same samples; where a
//!   frame's label is another frame's, its a\*/b\* codes are unchanged and
//!   its L\* is `unit_to_u16(g·L)`, exactly.
#![allow(clippy::needless_range_loop)]

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_align_distortion::dr_precision_doc_fit_distortion;
use doctored_core::ffi_align_vignette::{
    dr_precision_doc_remove_vignette, dr_precision_doc_remove_vignette_plan,
};
use doctored_core::ffi_auto_blend::dr_precision_doc_auto_blend;
use doctored_core::ffi_lab::dr_precision_doc_convert_lab_to_rgb;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_ops::{plan_json, LIMIT};
use crate::lab_fixture::{decode, f, f_inverse, xyz_to_lab, WHITE};
use crate::lab_geometry::twin_profile;

const SIZE: (u32, u32) = (600, 400);
const FOCAL: f64 = 600.0;
const YAWS: [f64; 3] = [-20.0, 0.0, 20.0];
/// a\* = b\* = 0 (the ICC v4 encoding).
const NEUTRAL: u16 = 0x8080;

// ------------------------------------------------------------ the Lab codes --

/// One L\*a\*b\* colour's 16-bit codes: L\* clamped 0…100, a\*/b\* −128…127.
fn lab_codes(lab: [f64; 3]) -> [u16; 3] {
    let code = |unit: f64| (unit.clamp(0.0, 1.0) * 65535.0).round() as u16;
    [
        code(lab[0] / 100.0),
        code((lab[1].clamp(-128.0, 127.0) + 128.0) / 255.0),
        code((lab[2].clamp(-128.0, 127.0) + 128.0) / 255.0),
    ]
}

/// The CIE XYZ (D50) of an L\*a\*b\* colour.
fn lab_to_xyz(lab: [f64; 3]) -> [f64; 3] {
    let fy = (lab[0] + 16.0) / 116.0;
    let fx = fy + lab[1] / 500.0;
    let fz = fy - lab[2] / 200.0;
    [
        WHITE[0] * f_inverse(fx),
        WHITE[1] * f_inverse(fy),
        WHITE[2] * f_inverse(fz),
    ]
}

/// The L\* of a CIE Y.
fn lightness(y: f64) -> f64 {
    116.0 * f(y) - 16.0
}

/// One frame: its samples, size and offset.
type Frame = (Vec<[u16; 4]>, (u32, u32), (i32, i32));

/// Opaque Lab frames of `size` at `offsets`, one layer each, as a Lab
/// document at `bits` (`lab` true) or an sRGB-tagged RGB document holding
/// the same samples.
fn stack(size: (u32, u32), frames: &[Frame], bits: u32, lab: bool) -> PrecisionDocument {
    let profile = if lab {
        twin_profile()
    } else {
        IccProfile::srgb()
    };
    let records = frames
        .iter()
        .enumerate()
        .map(|(k, (pixels, dims, offset))| {
            let image =
                PrecisionImage::from_rgba16(dims.0, dims.1, pixels.clone(), profile.clone(), None)
                    .unwrap();
            Layer::raster(&format!("L{k}"), image, *offset, 1.0, true).unwrap()
        })
        .collect();
    let doc = PrecisionDocument::from_parts(
        size.0,
        size.1,
        profile,
        Metadata::default(),
        Resolution::default(),
        records,
    )
    .unwrap();
    if lab {
        doc.with_lab_depth(bits).unwrap()
    } else {
        doc
    }
}

fn samples(doc: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    doc.layer(index)
        .unwrap()
        .source()
        .unwrap()
        .to_rgba16()
        .unwrap()
}

// ------------------------------------------- (a) take2's vignette geometry --
// The geometry is `geometry_align_vignette.rs`'s (the master copy), restated
// here for its one luminance channel.

/// The smooth linear picture over the unrolled directions (channel 0 of the
/// master's).
fn picture(u: f64, v: f64) -> f64 {
    0.35 + 0.22 * (u / 41.0 + 0.7).sin() * (v / 29.0 - 0.3).cos()
        + 0.1 * (u / 13.0 - v / 19.0).sin()
}

/// The view yawed `yaw` degrees at its pixel centre (fx, fy), linear.
fn view(yaw: f64, fx: f64, fy: f64) -> f64 {
    let (xc, yc) = (fx - f64::from(SIZE.0) / 2.0, fy - f64::from(SIZE.1) / 2.0);
    let theta = yaw.to_radians() + xc.atan2(FOCAL);
    picture(FOCAL * theta, FOCAL * yc / xc.hypot(FOCAL))
}

fn radius() -> f64 {
    (f64::from(SIZE.0) / 2.0 - 0.5).hypot(f64::from(SIZE.1) / 2.0 - 0.5)
}

fn rho2(fx: f64, fy: f64) -> f64 {
    let (dx, dy) = (fx - f64::from(SIZE.0) / 2.0, fy - f64::from(SIZE.1) / 2.0);
    (dx * dx + dy * dy) / (radius() * radius())
}

/// The closed-form mean of 1 + a·ρ² over the frame's pixel centres.
fn mean_falloff(a: f64) -> f64 {
    let (w, h) = (f64::from(SIZE.0), f64::from(SIZE.1));
    1.0 + a * ((w * w - 1.0) / 12.0 + (h * h - 1.0) / 12.0) / (radius() * radius())
}

fn product(a: &[f64; 9], b: &[f64; 9]) -> [f64; 9] {
    let mut out = [0.0; 9];
    for r in 0..3 {
        for c in 0..3 {
            out[r * 3 + c] = (0..3).map(|k| a[r * 3 + k] * b[k * 3 + c]).sum();
        }
    }
    out
}

fn apply(m: &[f64; 9], p: [f64; 2]) -> [f64; 2] {
    let w = m[6] * p[0] + m[7] * p[1] + m[8];
    [
        (m[0] * p[0] + m[1] * p[1] + m[2]) / w,
        (m[3] * p[0] + m[4] * p[1] + m[5]) / w,
    ]
}

/// The map of the layer yawed `yaw` onto the reference yawed `YAWS[0]`:
/// T(c)·K·R_y(yaw − yaw₀)·K⁻¹·T(−c), row-major (both masters' `map`).
fn map(yaw: f64) -> [f64; 9] {
    let a = (yaw - YAWS[0]).to_radians();
    let (c, s) = (a.cos(), a.sin());
    let (cx, cy) = (f64::from(SIZE.0) / 2.0, f64::from(SIZE.1) / 2.0);
    let m = [c, 0.0, s * FOCAL, 0.0, 1.0, 0.0, -s / FOCAL, 0.0, c];
    let t = [1.0, 0.0, cx, 0.0, 1.0, cy, 0.0, 0.0, 1.0];
    let back = [1.0, 0.0, -cx, 0.0, 1.0, -cy, 0.0, 0.0, 1.0];
    product(&t, &product(&m, &back))
}

/// Take2's three frames at the canvas origin, each pixel's Lab codes from
/// `value(frame, yaw, fx, fy)`.
fn take2_stack(value: impl Fn(usize, f64, f64, f64) -> [u16; 3]) -> PrecisionDocument {
    let frames: Vec<_> = YAWS
        .iter()
        .enumerate()
        .map(|(k, yaw)| {
            let pixels = (0..SIZE.0 * SIZE.1)
                .map(|i| {
                    let (fx, fy) = (f64::from(i % SIZE.0) + 0.5, f64::from(i / SIZE.0) + 0.5);
                    let [l, a, b] = value(k, *yaw, fx, fy);
                    [l, a, b, 65535]
                })
                .collect();
            (pixels, SIZE, (0, 0))
        })
        .collect();
    stack(SIZE, &frames, 16, true)
}

/// The middle frame's chroma at (fx, fy), in a\*/b\* units.
fn chroma(fx: f64, fy: f64) -> [f64; 2] {
    [
        30.0 * (fx / 37.0).sin() + 6.0,
        -25.0 * (fy / 23.0 + 0.4).cos(),
    ]
}

/// The vignetted luminance: the view times V = 1 − 0.4ρ².
fn vignetted_y(yaw: f64, fx: f64, fy: f64) -> f64 {
    view(yaw, fx, fy) * (1.0 - 0.4 * rho2(fx, fy))
}

fn vignette_request() -> Value {
    json!({
        "op": "remove_vignette",
        "layers": YAWS.iter().enumerate()
            .map(|(layer, yaw)| json!({"layer": layer, "map": map(*yaw).to_vec()}))
            .collect::<Vec<_>>(),
    })
}

fn vignette_plan(doc: &PrecisionDocument) -> Value {
    let text = CString::new(vignette_request().to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_remove_vignette_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    plan_json(out, err).unwrap()
}

fn remove_vignette(doc: &PrecisionDocument) -> PrecisionDocument {
    let text = CString::new(vignette_request().to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_remove_vignette(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!out.is_null(), "remove_vignette: {}", take_err_string(err));
    unsafe { *Box::from_raw(out) }
}

/// `doc` converted into 16-bit ProPhoto RGB (relative, black point
/// compensation).
fn prophoto(doc: &PrecisionDocument) -> PrecisionDocument {
    let profile = IccProfile::prophoto_rgb();
    let options =
        CString::new(json!({"intent":"relative","black_point_compensation":true}).to_string())
            .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_lab_to_rgb(
            doc,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to ProPhoto: {}", take_err_string(err));
    unsafe { *Box::from_raw(out) }
}

#[test]
fn vignette_removal_reads_cie_y_and_writes_the_gain_into_xyz() {
    const A: f64 = -0.4;
    let neutral =
        take2_stack(|_, yaw, fx, fy| lab_codes([lightness(vignetted_y(yaw, fx, fy)), 0.0, 0.0]));
    let plan = vignette_plan(&neutral);
    let coefficient = plan["coefficient"].as_f64().unwrap();
    let gain = plan["gain"].as_f64().unwrap();
    println!("Lab 16-bit: coefficient {coefficient:.4}, gain {gain:.4}");
    assert!((coefficient - A).abs() <= 0.02, "coefficient {coefficient}");
    assert!(
        (gain - mean_falloff(A)).abs() <= 0.01,
        "gain {gain} against {}",
        mean_falloff(A)
    );
    assert_eq!(plan["layers"], json!([0, 1, 2]));

    let out = remove_vignette(&neutral);
    assert!(out.is_lab());
    let (mut sum, mut count) = (0.0, 0usize);
    for (k, yaw) in YAWS.iter().enumerate() {
        for (i, p) in samples(&out, k).iter().enumerate() {
            let (fx, fy) = (
                f64::from(i as u32 % SIZE.0) + 0.5,
                f64::from(i as u32 / SIZE.0) + 0.5,
            );
            let truth = lab_codes([lightness(view(*yaw, fx, fy) * gain), 0.0, 0.0]);
            sum += (f64::from(p[0]) - f64::from(truth[0])).abs();
            count += 1;
            assert_eq!(p[3], 65535, "alpha stays");
        }
    }
    let mean = sum / count as f64;
    println!("Lab 16-bit: mean |L* out - picture x gain| {mean:.2} codes");
    assert!(mean <= 1.5 * 257.0, "mean {mean} codes");

    // The same neutral stack in ProPhoto RGB fits the same falloff.
    let rgb = prophoto(&neutral);
    let rgb_plan = vignette_plan(&rgb);
    let (rgb_coefficient, rgb_gain) = (
        rgb_plan["coefficient"].as_f64().unwrap(),
        rgb_plan["gain"].as_f64().unwrap(),
    );
    println!("ProPhoto: coefficient {rgb_coefficient:.4}, gain {rgb_gain:.4}");
    assert!(
        (rgb_coefficient - coefficient).abs() <= 0.005,
        "ProPhoto coefficient {rgb_coefficient} against {coefficient}"
    );
    assert!(
        (rgb_gain - gain).abs() <= 0.002,
        "ProPhoto gain {rgb_gain} against {gain}"
    );

    // The middle frame carries a*/b*: every code is the test's own XYZ times
    // gain / V(ρ) of the reported fit.
    let coloured = take2_stack(|k, yaw, fx, fy| {
        let [a, b] = if k == 1 { chroma(fx, fy) } else { [0.0, 0.0] };
        lab_codes([lightness(vignetted_y(yaw, fx, fy)), a, b])
    });
    let plan = vignette_plan(&coloured);
    let (coefficient, gain) = (
        plan["coefficient"].as_f64().unwrap(),
        plan["gain"].as_f64().unwrap(),
    );
    let before: Vec<_> = (0..3).map(|k| samples(&coloured, k)).collect();
    let out = remove_vignette(&coloured);
    let mut worst = 0i32;
    for k in 0..3 {
        for (i, (p, q)) in before[k].iter().zip(samples(&out, k)).enumerate() {
            let (fx, fy) = (
                f64::from(i as u32 % SIZE.0) + 0.5,
                f64::from(i as u32 / SIZE.0) + 0.5,
            );
            let scale = gain / (1.0 + coefficient * rho2(fx, fy));
            let want = lab_codes(xyz_to_lab(lab_to_xyz(decode(*p)).map(|v| v * scale)));
            for c in 0..3 {
                let off = (i32::from(q[c]) - i32::from(want[c])).abs();
                worst = worst.max(off);
                assert!(
                    off <= 4,
                    "frame {k} pixel {i} channel {c}: {} against {}",
                    q[c],
                    want[c]
                );
            }
            assert_eq!(q[3], 65535);
        }
    }
    println!("Lab chroma frame: largest |Δ| {worst} codes");
}

// ------------------------------------------ (b) the value-noise distortion --
// `geometry_align_distortion.rs`'s oracle (the master copy), its luminance
// channel only.

const CENTRE: [f64; 2] = [300.0, 200.0];
const LENS_RADIUS: f64 = 300.0;

fn lattice(i: i64, j: i64, seed: u64) -> f64 {
    let mut h = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (j as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ seed.wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 31;
    h = h.wrapping_mul(0xD6E8_FEB8_6659_FD93);
    h ^= h >> 32;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

fn noise(u: f64, v: f64, cell: f64, seed: u64) -> f64 {
    let (x, y) = (u / cell, v / cell);
    let (i, j) = (x.floor(), y.floor());
    let (s, t) = (x - i, y - j);
    let (s, t) = (s * s * (3.0 - 2.0 * s), t * t * (3.0 - 2.0 * t));
    let (i, j) = (i as i64, j as i64);
    let top = lattice(i, j, seed) * (1.0 - s) + lattice(i + 1, j, seed) * s;
    let bottom = lattice(i, j + 1, seed) * (1.0 - s) + lattice(i + 1, j + 1, seed) * s;
    top * (1.0 - t) + bottom * t
}

fn textured(u: f64, v: f64) -> f64 {
    0.42 + 0.12 * (u / 41.0 + 0.7).sin() * (v / 29.0 - 0.3).cos()
        + 0.05 * (u / 13.0 - v / 19.0).sin()
        + 0.22 * (noise(u, v, 23.0, 1) - 0.5)
        + 0.12 * (noise(u, v, 9.0, 2) - 0.5)
}

fn textured_view(yaw: f64, fx: f64, fy: f64) -> f64 {
    let (xc, yc) = (fx - CENTRE[0], fy - CENTRE[1]);
    let theta = yaw.to_radians() + xc.atan2(FOCAL);
    textured(FOCAL * theta, FOCAL * yc / xc.hypot(FOCAL))
}

/// The test's own inverse of the core's convention: Newton on
/// ρ(1 + kρ²) = ρ_p.
fn undistort(p: [f64; 2], k: f64) -> [f64; 2] {
    let q = [
        (p[0] - CENTRE[0]) / LENS_RADIUS,
        (p[1] - CENTRE[1]) / LENS_RADIUS,
    ];
    let target = q[0].hypot(q[1]);
    if target == 0.0 {
        return p;
    }
    let mut rho = target;
    for _ in 0..50 {
        let step = (rho * (1.0 + k * rho * rho) - target) / (1.0 + 3.0 * k * rho * rho);
        rho -= step;
        if step.abs() < 1e-14 {
            break;
        }
    }
    let s = rho / target;
    [
        CENTRE[0] + LENS_RADIUS * q[0] * s,
        CENTRE[1] + LENS_RADIUS * q[1] * s,
    ]
}

#[test]
fn geometric_distortion_fits_k_on_a_lab_stacks_cie_y() {
    for k in [-0.05, 0.04] {
        let doc = take2_stack(|_, yaw, fx, fy| {
            let u = undistort([fx, fy], k);
            lab_codes([lightness(textured_view(yaw, u[0], u[1])), 0.0, 0.0])
        });
        let request = json!({
            "op": "fit_distortion",
            "layers": YAWS.iter().enumerate()
                .map(|(layer, yaw)| json!({
                    "layer": layer, "anchor": layer.saturating_sub(1), "map": map(*yaw).to_vec(),
                }))
                .collect::<Vec<_>>(),
        });
        let text = CString::new(request.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_fit_distortion(&doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        };
        let fitted = plan_json(out, err).unwrap();
        let coefficient = fitted["coefficient"].as_f64().unwrap();
        println!("Lab k {k}: coefficient {coefficient:.5}");
        assert!((coefficient - k).abs() <= 0.005, "k {k}: {coefficient}");
        let maps = fitted["maps"].as_array().unwrap();
        assert_eq!(maps.len(), 3);
        for (layer, yaw) in YAWS.iter().enumerate() {
            let got: Vec<f64> = maps[layer]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            let got: [f64; 9] = got.try_into().unwrap();
            let want = map(*yaw);
            for corner in [[0.0, 0.0], [600.0, 0.0], [600.0, 400.0], [0.0, 400.0]] {
                let (a, b) = (apply(&got, corner), apply(&want, corner));
                let off = (a[0] - b[0]).hypot(a[1] - b[1]);
                assert!(
                    off <= 1.0,
                    "k {k} layer {layer} corner {corner:?}: {a:?} against {b:?}"
                );
            }
        }
    }
}

// ------------------------------------------------------ (c), (d) Auto-Blend --

const STRIP: u32 = 64;
const HEIGHT: u32 = 96;

fn hash(i: u32) -> u32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^ (x >> 12)
}

/// White noise in 0.2…0.8 per pixel and channel, `seed` apart.
fn noise_plane(w: u32, h: u32, seed: u32) -> Vec<[f64; 3]> {
    (0..w * h)
        .map(|i| {
            let unit = |c: u32| f64::from(hash(i * 3 + c + seed) & 0xffff) / 65535.0;
            [0, 1, 2].map(|c| 0.2 + 0.6 * unit(c))
        })
        .collect()
}

/// A separable box mean of radius 3 with clamped edges.
fn box_blur(source: &[[f64; 3]], w: u32, h: u32) -> Vec<[f64; 3]> {
    let (w, h) = (w as i64, h as i64);
    let pass = |input: &[[f64; 3]], horizontal: bool| -> Vec<[f64; 3]> {
        let mut out = vec![[0.0; 3]; input.len()];
        for y in 0..h {
            for x in 0..w {
                let mut sum = [0.0; 3];
                for d in -3..=3i64 {
                    let (sx, sy) = if horizontal {
                        ((x + d).clamp(0, w - 1), y)
                    } else {
                        (x, (y + d).clamp(0, h - 1))
                    };
                    let p = input[(sy * w + sx) as usize];
                    for c in 0..3 {
                        sum[c] += p[c];
                    }
                }
                out[(y * w + x) as usize] = sum.map(|v| v / 7.0);
            }
        }
        out
    };
    pass(&pass(source, true), false)
}

/// A unit value as a code at `bits` (the ×257 grid at 8).
fn code(v: f64, bits: u32) -> u16 {
    if bits == 8 {
        ((v * 255.0).round() * 257.0) as u16
    } else {
        (v * 65535.0).round() as u16
    }
}

/// `dr_precision_doc_auto_blend`: the document and the report.
fn auto_blend(doc: &PrecisionDocument, request: &Value) -> (PrecisionDocument, Value) {
    let text = CString::new(request.to_string()).unwrap();
    let (mut report, mut err) = (ptr::null_mut(), ptr::null_mut());
    let out = unsafe {
        dr_precision_doc_auto_blend(
            doc,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut report,
            &mut err,
        )
    };
    assert!(
        !out.is_null(),
        "auto_blend {request}: {}",
        take_err_string(err)
    );
    let report = plan_json(report, ptr::null_mut()).unwrap();
    (unsafe { *Box::from_raw(out) }, report)
}

fn mask(doc: &PrecisionDocument, index: usize) -> Vec<u16> {
    doc.layer(index)
        .unwrap()
        .mask()
        .expect("a layer mask")
        .plane()
        .samples()
        .to_vec()
}

/// Two strip frames over a 128 × 96 canvas, each pixel's codes `[L, a, b]`
/// from `pixel(own, other)`: `own` is sharp in strip k and blurred elsewhere,
/// `other` the reverse, so a chroma read from `other` is sharp exactly where
/// L\* is not.
fn strip_frames(pixel: impl Fn([f64; 3], [f64; 3]) -> [u16; 3]) -> Vec<Frame> {
    let size = (2 * STRIP, HEIGHT);
    let sharp = noise_plane(size.0, size.1, 17);
    let blurred = box_blur(&sharp, size.0, size.1);
    (0..2u32)
        .map(|k| {
            let pixels = (0..size.0 * size.1)
                .map(|i| {
                    let (s, b) = (sharp[i as usize], blurred[i as usize]);
                    let (own, other) = if (i % size.0) / STRIP == k {
                        (s, b)
                    } else {
                        (b, s)
                    };
                    let [l, a, b] = pixel(own, other);
                    [l, a, b, 65535]
                })
                .collect();
            (pixels, size, (0, 0))
        })
        .collect()
}

#[test]
fn stack_images_scores_lightness_alone() {
    let request = json!({"layers":[0,1],"seamless":true,"method":"stack_images"});
    for bits in [8, 16] {
        // With chroma sharp where L* is blurred: the masks are the L* stack's
        // (a Rec. 709 score of the codes would follow a*).
        let frames = strip_frames(|own, other| {
            [
                code(own[0], bits),
                code(other[1], bits),
                code(other[2], bits),
            ]
        });
        let grey: Vec<_> = frames
            .iter()
            .map(|(pixels, dims, offset)| {
                let pixels = pixels.iter().map(|p| [p[0], p[0], p[0], p[3]]).collect();
                (pixels, *dims, *offset)
            })
            .collect();
        let size = (2 * STRIP, HEIGHT);
        let (lab, _) = auto_blend(&stack(size, &frames, bits, true), &request);
        let (rgb, _) = auto_blend(&stack(size, &grey, bits, false), &request);
        assert!(lab.is_lab());
        for k in 0..2 {
            assert_eq!(
                mask(&lab, k),
                mask(&rgb, k),
                "{bits} bits: frame {k}'s mask"
            );
        }
        // a* = b* = 0: the merged record is the RGB document's.
        let neutral = strip_frames(|own, _| [code(own[0], bits), NEUTRAL, NEUTRAL]);
        let (lab, lab_report) = auto_blend(&stack(size, &neutral, bits, true), &request);
        let (rgb, rgb_report) = auto_blend(&stack(size, &neutral, bits, false), &request);
        let merged = lab_report["merged_layer"].as_u64().expect("a merged layer") as usize;
        assert_eq!(rgb_report["merged_layer"], lab_report["merged_layer"]);
        for k in 0..2 {
            assert_eq!(
                mask(&lab, k),
                mask(&rgb, k),
                "{bits} bits: neutral mask {k}"
            );
        }
        assert_eq!(
            samples(&lab, merged),
            samples(&rgb, merged),
            "{bits} bits: the merged record"
        );
    }
}

#[test]
fn seamless_tones_gain_lightness_alone() {
    let canvas = (240u32, 96u32);
    let frame = (160u32, 96u32);
    let scene = noise_plane(canvas.0, canvas.1, 101);
    let smooth = box_blur(&scene, canvas.0, canvas.1);
    let frames: Vec<_> = [(0i32, 1.0), (80i32, 0.8)]
        .iter()
        .map(|&(x0, step)| {
            let pixels = (0..frame.0 * frame.1)
                .map(|i| {
                    let (x, y) = (i % frame.0 + x0 as u32, i / frame.0);
                    let p = smooth[(y * canvas.0 + x) as usize];
                    let l = code(p[0] * step, 16);
                    let a = code(0.5 + 0.6 * (p[1] - 0.5), 16);
                    let b = code(0.5 - 0.6 * (p[2] - 0.5), 16);
                    [l, a, b, 65535]
                })
                .collect::<Vec<_>>();
            (pixels, frame, (x0, 0))
        })
        .collect();
    let request = json!({"layers":[0,1],"seamless":true,"method":"panorama"});
    let source = stack(canvas, &frames, 16, true);
    let (lab, lab_report) = auto_blend(&source, &request);
    let (_, rgb_report) = auto_blend(&stack(canvas, &frames, 16, false), &request);
    assert!(lab.is_lab());
    let gain = |report: &Value, k: usize| -> [f64; 3] {
        let g = &report["layers"][k]["gain"];
        [0, 1, 2].map(|c| g[c].as_f64().expect("a gain"))
    };
    let mut gains = Vec::new();
    for k in 0..2 {
        let (ours, theirs) = (gain(&lab_report, k), gain(&rgb_report, k));
        println!("frame {k}: Lab gain {ours:?}, RGB gain {theirs:?}");
        assert_eq!(ours[1], 1.0, "frame {k}'s a* gain");
        assert_eq!(ours[2], 1.0, "frame {k}'s b* gain");
        assert_eq!(ours[0], theirs[0], "frame {k}'s L* gain is the red gain");
        gains.push(ours[0] as f32);
    }
    // Where a frame lost, its own pixels are its gained samples.
    let mut checked = 0usize;
    for k in 0..2 {
        let won = mask(&lab, k);
        let (x0, _) = frames[k].2;
        let before = samples(&source, k);
        let after = samples(&lab, k);
        for (i, (p, q)) in before.iter().zip(&after).enumerate() {
            let (x, y) = (i as u32 % frame.0 + x0 as u32, i as u32 / frame.0);
            if won[(y * canvas.0 + x) as usize] == 65535 {
                continue;
            }
            let lightness = (gains[k] * (f32::from(p[0]) / 65535.0)).clamp(0.0, 1.0);
            assert_eq!(
                q[0],
                (lightness * 65535.0).round() as u16,
                "frame {k} pixel {i} L*"
            );
            assert_eq!((q[1], q[2]), (p[1], p[2]), "frame {k} pixel {i} a*/b*");
            checked += 1;
        }
    }
    assert!(checked > 1000, "{checked} lost pixels checked");
}
