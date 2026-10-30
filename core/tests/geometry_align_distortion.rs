//! Auto-Align Layers' Geometric Distortion: `fit_distortion`
//! at 8 bits, 16 bits and 32-bit float through the FFI
//! (`dr_doc_fit_distortion(_plan)`, `dr_precision_doc_fit_distortion(_plan)`),
//! and `project_layer`'s `distortion`.
//!
//! # The oracle
//!
//! Take2's geometry, written here: three 600 × 400 pinhole views at f 600,
//! yawed −20, 0 and +20°, each sampling one smooth LINEAR picture textured
//! with value noise (no period a search could lock onto)
//! over the directions, every layer at the canvas origin, the bottom one the
//! reference. Each layer's map is the test's own T(c)·K·R·K⁻¹·T(−c), the
//! lens-free (analytic) map. A known coefficient k is applied in the core's
//! convention with the test's own Newton inverse: the frame's pixel p shows
//! the pinhole view at the u with p = c + R·q·(1 + k·|q|²), q = (u − c)/R, c
//! the frame's centre and R = 300. The fit must find k within 0.005, every
//! returned map must put the rect's corners within 1 px of the analytic map,
//! and the Spherical reprojection at the true focal and rotations with the
//! fitted k must be the picture: mean |Δ| ≤ 1.5 codes at 8 bits over pixels
//! whose source is at least 4 px inside the frame. Take2's own `dist`
//! construction (k 0.06 over the half diagonal, the other direction),
//! projected at the host's origin (the reference's axis on the pixel centre
//! (300.5, 200.5)), must land each layer's rect within 2 px of Photoshop's D
//! rows (take3: Spherical 584/583/583 × 393, Cylindrical left 584 × 407),
//! grow it within 2 px of the growth those rows show over take2's lens-free C
//! rows, and land the Spherical union within 3 px of 1002 × 393. No golden
//! images.
#![allow(clippy::needless_range_loop)]

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use crate::cmyk_fixture::press_profile;
use crate::common::selected_pixels::take_err;
use crate::float_fixture::float_doc;
use doctored_core::doc::{DrDocument, LayerRaster};
use doctored_core::doc_lock::LOCK_PIXELS;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_align_distortion::{
    dr_doc_fit_distortion, dr_doc_fit_distortion_plan, dr_precision_doc_fit_distortion,
    dr_precision_doc_fit_distortion_plan,
};
use doctored_core::ffi_align_projection::{dr_doc_project_layer, dr_doc_project_layer_plan};
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_group::dr_doc_layer_bounds;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::RgbaImage;
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const SIZE: (u32, u32) = (600, 400);
const FOCAL: f64 = 600.0;
const YAWS: [f64; 3] = [-20.0, 0.0, 20.0];
/// The lens's centre and normalising radius for a 600 × 400 frame at the
/// origin: the rect's centre and half its longer side.
const CENTRE: [f64; 2] = [300.0, 200.0];
const RADIUS: f64 = 300.0;
/// The host's origin (`AutoAlignReprojection.swift`): the reference's axis
/// on the pixel centre nearest its rect's centre.
const AXIS: [f64; 2] = [300.5, 200.5];
/// The documented plan terms: `PLAN_BOOKKEEPING`, `FIT_SCRATCH_BYTES` (4096
/// match records of 72 bytes, one 15 × 15 patch and one 31 × 31 window of
/// f64) and the level-2 window at step 1 (17 × 17 f64).
const BOOKKEEPING: u64 = 2 * 1024 * 1024;
const FIT_SCRATCH: u64 = 4096 * 72 + (15 * 15 + 31 * 31) * 8;
const REFINE_STEP_1: u64 = 17 * 17 * 8;

/// A lattice value in 0…1 for the integer cell (i, j) of octave `seed`: a
/// 64-bit integer hash, so the texture repeats nowhere a match could reach.
fn lattice(i: i64, j: i64, seed: u64) -> f64 {
    let mut h = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (j as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ seed.wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 31;
    h = h.wrapping_mul(0xD6E8_FEB8_6659_FD93);
    h ^= h >> 32;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// Smooth value noise with cells of `cell` px: the lattice values blended
/// with the C¹ smoothstep, so the picture is continuous at every point.
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

/// The smooth textured linear picture over the unrolled directions, per
/// channel: the vignette test's smooth waves plus value noise with 23 and
/// 9 px cells, so a 15 px patch has structure in both directions and no
/// period a search could lock onto.
fn picture(u: f64, v: f64, channel: usize) -> f64 {
    let base = 0.42
        + 0.12 * (u / 41.0 + 0.7).sin() * (v / 29.0 - 0.3).cos()
        + 0.05 * (u / 13.0 - v / 19.0).sin()
        + 0.22 * (noise(u, v, 23.0, 1) - 0.5)
        + 0.12 * (noise(u, v, 9.0, 2) - 0.5);
    base * [1.0, 0.92, 1.06][channel]
}

/// The pinhole view yawed `yaw` degrees at its point (fx, fy), linear.
fn view(yaw: f64, fx: f64, fy: f64, channel: usize) -> f64 {
    let (xc, yc) = (fx - CENTRE[0], fy - CENTRE[1]);
    let theta = yaw.to_radians() + xc.atan2(FOCAL);
    picture(FOCAL * theta, FOCAL * yc / xc.hypot(FOCAL), channel)
}

/// The core's convention forward: the frame point p that shows the pinhole
/// point u.
fn distort(u: [f64; 2], k: f64) -> [f64; 2] {
    let q = [(u[0] - CENTRE[0]) / RADIUS, (u[1] - CENTRE[1]) / RADIUS];
    let f = 1.0 + k * (q[0] * q[0] + q[1] * q[1]);
    [CENTRE[0] + RADIUS * q[0] * f, CENTRE[1] + RADIUS * q[1] * f]
}

/// The test's own inverse of [`distort`]: Newton on ρ(1 + kρ²) = ρ_p.
fn undistort(p: [f64; 2], k: f64) -> [f64; 2] {
    let q = [(p[0] - CENTRE[0]) / RADIUS, (p[1] - CENTRE[1]) / RADIUS];
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
    [CENTRE[0] + RADIUS * q[0] * s, CENTRE[1] + RADIUS * q[1] * s]
}

fn encode(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
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
/// T(c)·K·R_y(yaw − yaw₀)·K⁻¹·T(−c), row-major.
fn map(yaw: f64) -> [f64; 9] {
    let a = (yaw - YAWS[0]).to_radians();
    let (c, s) = (a.cos(), a.sin());
    let m = [c, 0.0, s * FOCAL, 0.0, 1.0, 0.0, -s / FOCAL, 0.0, c];
    let t = [1.0, 0.0, CENTRE[0], 0.0, 1.0, CENTRE[1], 0.0, 0.0, 1.0];
    let back = [1.0, 0.0, -CENTRE[0], 0.0, 1.0, -CENTRE[1], 0.0, 0.0, 1.0];
    product(&t, &product(&m, &back))
}

/// Row-major R_y(yaw).
fn rotation(yaw_degrees: f64) -> [f64; 9] {
    let a = yaw_degrees.to_radians();
    [a.cos(), 0.0, a.sin(), 0.0, 1.0, 0.0, -a.sin(), 0.0, a.cos()]
}

/// The request: the reference, then each layer anchored on its neighbour
/// (the host's chain), every map the analytic one.
fn request() -> Value {
    json!({
        "op": "fit_distortion",
        "layers": YAWS.iter().enumerate()
            .map(|(layer, yaw)| json!({
                "layer": layer, "anchor": layer.saturating_sub(1), "map": map(*yaw).to_vec(),
            }))
            .collect::<Vec<_>>(),
    })
}

/// Each frame's samples in 0…1 of its depth's encoding: `value(yaw, x, y,
/// channel)` at each pixel centre.
fn frames(value: impl Fn(f64, f64, f64, usize) -> f64) -> Vec<Vec<[f64; 3]>> {
    YAWS.iter()
        .map(|yaw| {
            (0..SIZE.0 * SIZE.1)
                .map(|i| {
                    let (fx, fy) = (f64::from(i % SIZE.0) + 0.5, f64::from(i / SIZE.0) + 0.5);
                    std::array::from_fn(|c| value(*yaw, fx, fy, c))
                })
                .collect()
        })
        .collect()
}

/// The linear views with the coefficient `k` applied in the core's
/// convention.
fn distorted(k: f64) -> impl Fn(f64, f64, f64, usize) -> f64 {
    move |yaw, fx, fy, c| {
        let u = undistort([fx, fy], k);
        view(yaw, u[0], u[1], c)
    }
}

fn byte_doc(frames: &[Vec<[f64; 3]>]) -> DrDocument {
    let image = |samples: &Vec<[f64; 3]>| {
        RgbaImage::from_fn(SIZE.0, SIZE.1, |x, y| {
            let p = samples[(y * SIZE.0 + x) as usize];
            image::Rgba([
                (p[0] * 255.0).round() as u8,
                (p[1] * 255.0).round() as u8,
                (p[2] * 255.0).round() as u8,
                255,
            ])
        })
    };
    let mut doc = DrDocument::from_pixels(image(&frames[0]));
    let base = doc.layers[0].clone();
    for samples in &frames[1..] {
        let mut layer = base.clone();
        layer.pixels = LayerRaster::new(image(samples));
        doc.layers.push(layer);
    }
    doc
}

fn typed16(frames: &[Vec<[f64; 3]>]) -> PrecisionDocument {
    let records = frames
        .iter()
        .enumerate()
        .map(|(k, samples)| {
            let px = samples
                .iter()
                .map(|p| {
                    let c = |v: f64| (v * 65535.0).round() as u16;
                    [c(p[0]), c(p[1]), c(p[2]), 65535]
                })
                .collect();
            let image =
                PrecisionImage::from_rgba16(SIZE.0, SIZE.1, px, IccProfile::srgb(), None).unwrap();
            PrecisionDocumentLayer::raster(&format!("L{k}"), image, (0, 0), 1.0, true).unwrap()
        })
        .collect();
    PrecisionDocument::from_parts(
        SIZE.0,
        SIZE.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        records,
    )
    .unwrap()
}

fn typed32(frames: &[Vec<[f64; 3]>]) -> PrecisionDocument {
    let records: Vec<Vec<[f32; 4]>> = frames
        .iter()
        .map(|samples| {
            samples
                .iter()
                .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32, 1.0])
                .collect()
        })
        .collect();
    float_doc(SIZE.0, SIZE.1, &IccProfile::srgb(), &records)
}

enum TestDoc {
    Byte(DrDocument),
    Typed(PrecisionDocument),
}

fn string(text: *mut c_char) -> String {
    let out = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    out
}

impl TestDoc {
    fn plan(&self, request: &Value) -> Result<Value, String> {
        let text = CString::new(request.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            match self {
                TestDoc::Byte(d) => {
                    dr_doc_fit_distortion_plan(d, text.as_ptr(), LIMIT, 0, &mut err)
                }
                TestDoc::Typed(d) => {
                    dr_precision_doc_fit_distortion_plan(d, text.as_ptr(), LIMIT, 0, &mut err)
                }
            }
        };
        if let Some(message) = take_err(err) {
            assert!(out.is_null());
            return Err(message);
        }
        Ok(serde_json::from_str(&string(out)).unwrap())
    }

    fn fit(&self, request: &Value) -> Result<Value, String> {
        let text = CString::new(request.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            match self {
                TestDoc::Byte(d) => {
                    dr_doc_fit_distortion(d, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
                }
                TestDoc::Typed(d) => dr_precision_doc_fit_distortion(
                    d,
                    text.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                ),
            }
        };
        if let Some(message) = take_err(err) {
            assert!(out.is_null());
            return Err(message);
        }
        Ok(serde_json::from_str(&string(out)).unwrap())
    }
}

/// `project_layer` on a byte document.
fn project(doc: &DrDocument, request: &Value) -> Result<DrDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_project_layer(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if let Some(message) = take_err(err) {
        assert!(out.is_null());
        return Err(message);
    }
    assert!(!out.is_null());
    Ok(unsafe { *Box::from_raw(out) })
}

fn project_plan(doc: &DrDocument, request: &Value) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_project_layer_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    if let Some(message) = take_err(err) {
        assert!(out.is_null());
        return Err(message);
    }
    Ok(serde_json::from_str(&string(out)).unwrap())
}

/// The take's projection request for layer `layer` (yawed `YAWS[layer]`).
fn projection(layer: usize, kind: &str, distortion: Option<f64>) -> Value {
    let mut out = json!({
        "op": "project_layer", "layer": layer, "projection": kind, "focal": FOCAL,
        "rotation": rotation(YAWS[0] - YAWS[layer]).to_vec(),
        "centre": CENTRE.to_vec(), "origin": CENTRE.to_vec(),
    });
    if let Some(k) = distortion {
        out["distortion"] = json!(k);
    }
    out
}

fn bounds(doc: &DrDocument, layer: usize) -> [i64; 4] {
    let mut xywh = [0i32; 4];
    assert!(unsafe { dr_doc_layer_bounds(doc, layer, xywh.as_mut_ptr()) });
    xywh.map(i64::from)
}

#[derive(Clone, Copy)]
enum Depth {
    Byte,
    Sixteen,
    Float,
}

fn known_doc(depth: Depth, k: f64) -> TestDoc {
    match depth {
        Depth::Byte => TestDoc::Byte(byte_doc(&frames(move |yaw, x, y, c| {
            encode(distorted(k)(yaw, x, y, c))
        }))),
        Depth::Sixteen => TestDoc::Typed(typed16(&frames(move |yaw, x, y, c| {
            encode(distorted(k)(yaw, x, y, c))
        }))),
        Depth::Float => TestDoc::Typed(typed32(&frames(distorted(k)))),
    }
}

/// The 8-bit pixel check: each layer reprojected onto the sphere at the true
/// focal and rotations with the fitted coefficient, against the picture.
fn spherical_matches_the_picture(doc: &DrDocument, k_true: f64, k_fit: f64) {
    let mut doc = doc.clone();
    for layer in 0..3 {
        doc = project(&doc, &projection(layer, "spherical", Some(k_fit))).unwrap();
    }
    let (mut sum, mut count) = (0.0, 0usize);
    for layer in 0..3 {
        let entry = &doc.layers[layer];
        let (w, h) = entry.pixels.dimensions();
        let (x0, y0) = (f64::from(entry.offset.0), f64::from(entry.offset.1));
        let px = entry.pixels.read_rect(0, 0, w, h).unwrap();
        let r = rotation(YAWS[0] - YAWS[layer]);
        for (i, p) in px.iter().enumerate() {
            if p[3] != 255 {
                continue;
            }
            let dest = [
                x0 + f64::from(i as u32 % w) + 0.5,
                y0 + f64::from(i as u32 / w) + 0.5,
            ];
            let theta = (dest[0] - CENTRE[0]) / FOCAL;
            let phi = (dest[1] - CENTRE[1]) / FOCAL;
            let d = [theta.sin() * phi.cos(), phi.sin(), theta.cos() * phi.cos()];
            let ray = [
                r[0] * d[0] + r[1] * d[1] + r[2] * d[2],
                r[3] * d[0] + r[4] * d[1] + r[5] * d[2],
                r[6] * d[0] + r[7] * d[1] + r[8] * d[2],
            ];
            if ray[2] <= 0.0 {
                continue;
            }
            let pinhole = [
                CENTRE[0] + FOCAL * ray[0] / ray[2],
                CENTRE[1] + FOCAL * ray[1] / ray[2],
            ];
            let source = distort(pinhole, k_true);
            let inside = source[0] >= 4.0
                && source[1] >= 4.0
                && source[0] <= f64::from(SIZE.0) - 4.0
                && source[1] <= f64::from(SIZE.1) - 4.0;
            if !inside {
                continue;
            }
            let u = FOCAL * (YAWS[0].to_radians() + theta);
            let v = FOCAL * phi.tan();
            for c in 0..3 {
                sum += (f64::from(p[c]) - encode(picture(u, v, c)) * 255.0).abs();
                count += 1;
            }
        }
    }
    let mean = sum / count as f64;
    println!(
        "8-bit spherical with k {k_fit:.4}: mean |out - picture| {mean:.3} codes over {count}"
    );
    assert!(count > 100_000, "{count} samples compared");
    assert!(mean <= 1.5, "mean {mean} codes");
}

/// Case (a) at one depth, for k −0.05 and +0.04.
fn known_distortion(depth: Depth) {
    let label = match depth {
        Depth::Byte => "8-bit",
        Depth::Sixteen => "16-bit",
        Depth::Float => "float",
    };
    for k in [-0.05, 0.04] {
        let doc = known_doc(depth, k);
        let fitted = doc.fit(&request()).unwrap();
        let coefficient = fitted["coefficient"].as_f64().unwrap();
        println!(
            "{label} k {k}: coefficient {coefficient:.5}, matches {}, residual {:.4}",
            fitted["matches"], fitted["residual"]
        );
        assert!(
            (coefficient - k).abs() <= 0.005,
            "{label} k {k}: coefficient {coefficient}"
        );
        assert_eq!(fitted["layers"], json!([0, 1, 2]));
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
            assert!((got[8] - 1.0).abs() < 1e-9, "m[8] is 1");
            let want = map(*yaw);
            for corner in [[0.0, 0.0], [600.0, 0.0], [600.0, 400.0], [0.0, 400.0]] {
                let (a, b) = (apply(&got, corner), apply(&want, corner));
                let off = (a[0] - b[0]).hypot(a[1] - b[1]);
                assert!(
                    off <= 1.0,
                    "{label} k {k} layer {layer} corner {corner:?}: {a:?} against {b:?}"
                );
            }
        }
        if let TestDoc::Byte(d) = &doc {
            spherical_matches_the_picture(d, k, coefficient);
        }
    }
}

#[test]
fn a_known_distortion_is_fitted_at_8_bits() {
    known_distortion(Depth::Byte);
}

#[test]
fn a_known_distortion_is_fitted_at_16_bits() {
    known_distortion(Depth::Sixteen);
}

#[test]
fn a_known_distortion_is_fitted_at_float() {
    known_distortion(Depth::Float);
}

/// Case (b): take2's `dist` construction (the view sampled at c + (p − c)(1 +
/// 0.06 (r / R_diag)²)), fitted, then projected at the host's origin
/// ([`AXIS`]) as Photoshop's take2 and take3 rows came back. Photoshop's D
/// rows (take3): Spherical left 584 × 393, centre and right 583 × 393, union
/// 1002 × 393; Cylindrical left 584 × 407. Its lens-free rows on the same
/// views (take2's C): Spherical 555 / 556 / 555 × 385, Cylindrical left
/// 555 × 399. Each layer's rect must be within 2 px of its D row (absolute,
/// now that the projected rect matches Photoshop's measured rows, phase 2b-5),
/// its GROWTH under the lens (its rect with the fitted k minus its rect on
/// the lens-free views) within 2 px of Photoshop's growth, and the Spherical
/// union within 3 px of Photoshop's.
#[test]
fn take2_dist_reproduces_photoshops_d_rows() {
    const SPHERICAL_D: [[i64; 2]; 3] = [[584, 393], [583, 393], [583, 393]];
    const SPHERICAL_C: [[i64; 2]; 3] = [[555, 385], [556, 385], [555, 385]];
    const CYLINDRICAL_D: [i64; 2] = [584, 407];
    const CYLINDRICAL_C: [i64; 2] = [555, 399];
    let at_axis = |mut request: Value| {
        request["origin"] = json!(AXIS.to_vec());
        request
    };
    let diagonal = 300.0_f64.hypot(200.0);
    let dist = frames(move |yaw, fx, fy, c| {
        let (xc, yc) = (fx - CENTRE[0], fy - CENTRE[1]);
        let rr = (xc.hypot(yc) / diagonal).powi(2);
        let f = 1.0 + 0.06 * rr;
        encode(view(yaw, CENTRE[0] + xc * f, CENTRE[1] + yc * f, c))
    });
    let doc = byte_doc(&dist);
    let rot = byte_doc(&frames(|yaw, x, y, c| encode(view(yaw, x, y, c))));
    let fitted = TestDoc::Byte(doc.clone()).fit(&request()).unwrap();
    let k = fitted["coefficient"].as_f64().unwrap();
    println!(
        "take2 dist: coefficient {k:.5}, matches {}, residual {:.4}",
        fitted["matches"], fitted["residual"]
    );
    let (mut spherical, mut plain) = (doc.clone(), rot.clone());
    for layer in 0..3 {
        spherical = project(
            &spherical,
            &at_axis(projection(layer, "spherical", Some(k))),
        )
        .unwrap();
        plain = project(&plain, &at_axis(projection(layer, "spherical", None))).unwrap();
    }
    let boxes: Vec<[i64; 4]> = (0..3).map(|l| bounds(&spherical, l)).collect();
    let lens_free: Vec<[i64; 4]> = (0..3).map(|l| bounds(&plain, l)).collect();
    let x0 = boxes.iter().map(|b| b[0]).min().unwrap();
    let y0 = boxes.iter().map(|b| b[1]).min().unwrap();
    let x1 = boxes.iter().map(|b| b[0] + b[2]).max().unwrap();
    let y1 = boxes.iter().map(|b| b[1] + b[3]).max().unwrap();
    println!(
        "take2 dist spherical: {} x {}, layers {boxes:?}; lens-free {lens_free:?}",
        x1 - x0,
        y1 - y0
    );
    assert!(
        (x1 - x0 - 1002).abs() <= 3 && (y1 - y0 - 393).abs() <= 3,
        "spherical union {} x {} against Photoshop's 1002 x 393",
        x1 - x0,
        y1 - y0
    );
    for layer in 0..3 {
        let (b, c) = (boxes[layer], lens_free[layer]);
        assert!(
            (b[2] - SPHERICAL_D[layer][0]).abs() <= 2 && (b[3] - SPHERICAL_D[layer][1]).abs() <= 2,
            "spherical layer {layer} is {}x{} against Photoshop's {}x{}",
            b[2],
            b[3],
            SPHERICAL_D[layer][0],
            SPHERICAL_D[layer][1]
        );
        let growth = [b[2] - c[2], b[3] - c[3]];
        let want = [
            SPHERICAL_D[layer][0] - SPHERICAL_C[layer][0],
            SPHERICAL_D[layer][1] - SPHERICAL_C[layer][1],
        ];
        assert!(
            (growth[0] - want[0]).abs() <= 2 && (growth[1] - want[1]).abs() <= 2,
            "spherical layer {layer} grows {growth:?} ({}x{}) against Photoshop's {want:?} \
             ({}x{})",
            b[2],
            b[3],
            SPHERICAL_D[layer][0],
            SPHERICAL_D[layer][1]
        );
    }
    let b = bounds(
        &project(&doc, &at_axis(projection(0, "cylindrical", Some(k)))).unwrap(),
        0,
    );
    let c = bounds(
        &project(&rot, &at_axis(projection(0, "cylindrical", None))).unwrap(),
        0,
    );
    println!(
        "take2 dist cylindrical left: {} x {} (lens-free {} x {})",
        b[2], b[3], c[2], c[3]
    );
    assert!(
        (b[2] - CYLINDRICAL_D[0]).abs() <= 2 && (b[3] - CYLINDRICAL_D[1]).abs() <= 2,
        "cylindrical left is {}x{} against Photoshop's {CYLINDRICAL_D:?}",
        b[2],
        b[3]
    );
    let growth = [b[2] - c[2], b[3] - c[3]];
    let want = [
        CYLINDRICAL_D[0] - CYLINDRICAL_C[0],
        CYLINDRICAL_D[1] - CYLINDRICAL_C[1],
    ];
    assert!(
        (growth[0] - want[0]).abs() <= 2 && (growth[1] - want[1]).abs() <= 2,
        "cylindrical left grows {growth:?} against Photoshop's {want:?}"
    );
}

/// Case (c): an undistorted stack fits k near 0, and `distortion` 0 is the
/// request without it, byte for byte.
#[test]
fn an_undistorted_stack_fits_zero_and_zero_changes_nothing() {
    let doc = byte_doc(&frames(|yaw, x, y, c| encode(view(yaw, x, y, c))));
    let fitted = TestDoc::Byte(doc.clone()).fit(&request()).unwrap();
    let coefficient = fitted["coefficient"].as_f64().unwrap();
    println!("undistorted: coefficient {coefficient:.5}");
    assert!(coefficient.abs() <= 0.002, "coefficient {coefficient}");
    for kind in ["cylindrical", "spherical"] {
        let without = project(&doc, &projection(2, kind, None)).unwrap();
        let with = project(&doc, &projection(2, kind, Some(0.0))).unwrap();
        assert_eq!(without.layers[2].offset, with.layers[2].offset);
        let (w, h) = without.layers[2].pixels.dimensions();
        assert_eq!((w, h), with.layers[2].pixels.dimensions());
        assert_eq!(
            without.layers[2].pixels.read_rect(0, 0, w, h).unwrap(),
            with.layers[2].pixels.read_rect(0, 0, w, h).unwrap(),
            "{kind}: distortion 0 is byte-identical"
        );
        assert_eq!(
            project_plan(&doc, &projection(2, kind, None)).unwrap(),
            project_plan(&doc, &projection(2, kind, Some(0.0))).unwrap()
        );
    }
}

/// Case (d): the plan's documented formula at each depth.
#[test]
fn the_plan_is_the_documented_formula() {
    let picture = frames(distorted(-0.05));
    let encoded = frames(move |yaw, x, y, c| encode(distorted(-0.05)(yaw, x, y, c)));
    let n = u64::from(SIZE.0) * u64::from(SIZE.1);
    for (doc, bytes) in [
        (TestDoc::Byte(byte_doc(&encoded)), 4),
        (TestDoc::Typed(typed16(&encoded)), 8),
        (TestDoc::Typed(typed32(&picture)), 16),
    ] {
        let plan = doc.plan(&request()).unwrap();
        let keys: Vec<&String> = plan.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["retained_bytes", "scratch_bytes"]);
        assert_eq!(
            plan["scratch_bytes"],
            json!(BOOKKEEPING + FIT_SCRATCH + REFINE_STEP_1)
        );
        assert!(plan["retained_bytes"].as_u64().unwrap() >= 3 * n * bytes);
    }
}

fn refusal(doc: &TestDoc, request: Value) -> String {
    let planned = doc.plan(&request).err();
    let fitted = doc.fit(&request).expect_err("the fit refuses");
    if let Some(planned) = planned {
        assert_eq!(planned, fitted);
    }
    fitted
}

/// Case (e): every refusal sentence.
#[test]
fn refusals_name_the_reason() {
    let encoded = frames(move |yaw, x, y, c| encode(distorted(-0.05)(yaw, x, y, c)));
    let doc = TestDoc::Byte(byte_doc(&encoded));
    let good = request();
    let with = |key: &str, value: Value| {
        let mut r = good.clone();
        r[key] = value;
        r
    };
    let identity = vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let entry = |layer: usize, anchor: usize, map: &Vec<f64>| json!({"layer": layer, "anchor": anchor, "map": map});
    assert_eq!(
        refusal(&doc, with("op", json!("remove_vignette"))),
        "invalid fit distortion request: op must be fit_distortion"
    );
    assert_eq!(
        refusal(&doc, with("extra", json!(1))),
        "invalid fit distortion request: unknown field `extra`"
    );
    assert_eq!(
        refusal(&doc, with("layers", json!([entry(0, 0, &identity)]))),
        "invalid fit distortion request: layers must name at least 2 layers"
    );
    assert_eq!(
        refusal(
            &doc,
            with(
                "layers",
                json!([entry(0, 0, &identity), entry(0, 0, &identity)])
            )
        ),
        "invalid fit distortion request: layer 0 is listed twice"
    );
    let reference = "invalid fit distortion request: the first entry must be the reference: \
                     its own anchor, with the identity map";
    assert_eq!(
        refusal(
            &doc,
            with(
                "layers",
                json!([entry(0, 1, &identity), entry(1, 0, &identity)])
            )
        ),
        reference
    );
    let shifted = vec![1.0, 0.0, 3.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    assert_eq!(
        refusal(
            &doc,
            with(
                "layers",
                json!([entry(0, 0, &shifted), entry(1, 0, &identity)])
            )
        ),
        reference
    );
    assert_eq!(
        refusal(
            &doc,
            with(
                "layers",
                json!([
                    entry(0, 0, &identity),
                    entry(1, 2, &identity),
                    entry(2, 0, &identity)
                ])
            )
        ),
        "invalid fit distortion request: the anchor of layer 1 must be an earlier entry's layer"
    );
    let mirrored = vec![-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let map_refusal =
        "invalid fit distortion request: map must be 9 finite numbers with a positive determinant";
    assert_eq!(
        refusal(
            &doc,
            with(
                "layers",
                json!([entry(0, 0, &identity), entry(1, 0, &mirrored)])
            )
        ),
        map_refusal
    );
    assert_eq!(
        refusal(
            &doc,
            with(
                "layers",
                json!([entry(0, 0, &identity), {"layer": 1, "anchor": 0, "map": [1, 0, 0]}])
            )
        ),
        map_refusal
    );
    assert_eq!(
        refusal(
            &doc,
            with(
                "layers",
                json!([entry(0, 0, &identity), {"layer": 1, "map": identity}])
            )
        ),
        "invalid fit distortion request: anchor must be a layer index"
    );
    assert_eq!(
        refusal(
            &doc,
            with(
                "layers",
                json!([entry(0, 0, &identity), {"layer": 1, "anchor": 0, "map": identity, "x": 1}])
            )
        ),
        "invalid fit distortion request: unknown field `x`"
    );
    let too_little = "Geometric Distortion found too little overlap between the layers to \
                      measure the distortion. Nothing was changed.";
    let far = vec![1.0, 0.0, 5000.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    assert_eq!(
        refusal(
            &doc,
            with("layers", json!([entry(0, 0, &identity), entry(1, 0, &far)]))
        ),
        too_little
    );
    // A flat stack: every patch is below the contrast floor.
    let flat = TestDoc::Byte(byte_doc(&frames(|_, _, _, _| 0.5)));
    assert_eq!(refusal(&flat, request()), too_little);
    assert_eq!(
        refusal(
            &doc,
            with(
                "layers",
                json!([entry(0, 0, &identity), entry(7, 0, &identity)])
            )
        ),
        "Geometric Distortion cannot change layer 7: there is no such layer."
    );
    let mut locked = byte_doc(&encoded);
    locked.layers[2].locks = LOCK_PIXELS;
    assert_eq!(
        refusal(&TestDoc::Byte(locked), good.clone()),
        "Geometric Distortion cannot change layer 2: it is locked (pixels)."
    );
    let mut described = byte_doc(&encoded);
    described.layers[1].meta = Some(json!({"type":"text"}).to_string());
    assert_eq!(
        refusal(&TestDoc::Byte(described), good.clone()),
        "Geometric Distortion cannot change layer 1: it holds artwork or a placed source; \
         rasterize it first."
    );
    // CMYK: a two-layer 16-bit stack converted into the press profile.
    let small: Vec<Vec<[f64; 3]>> = (0..2).map(|_| vec![[0.4, 0.5, 0.6]; 64]).collect();
    let rgb = |samples: &Vec<[f64; 3]>| {
        let px = samples
            .iter()
            .map(|p| p.map(|v| (v * 65535.0) as u16))
            .map(|p| [p[0], p[1], p[2], 65535])
            .collect();
        let image = PrecisionImage::from_rgba16(8, 8, px, IccProfile::srgb(), None).unwrap();
        PrecisionDocumentLayer::raster("L", image, (0, 0), 1.0, true).unwrap()
    };
    let source = Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            8,
            8,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            small.iter().map(rgb).collect(),
        )
        .unwrap(),
    ));
    let profile = press_profile("Press Test");
    let mut err = ptr::null_mut();
    let cmyk = unsafe {
        dr_precision_doc_convert_to_cmyk(
            source,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!cmyk.is_null(), "to CMYK: {:?}", take_err(err));
    unsafe { drop(Box::from_raw(source)) };
    let cmyk = TestDoc::Typed(unsafe { *Box::from_raw(cmyk) });
    let pair = json!({"op": "fit_distortion", "layers": [
        entry(0, 0, &identity), entry(1, 0, &identity),
    ]});
    assert_eq!(
        refusal(&cmyk, pair),
        "Geometric Distortion does not work in CMYK."
    );
    let typed = TestDoc::Typed(typed16(&encoded));
    assert_eq!(
        refusal(&typed, with("op", json!("x"))),
        "invalid fit distortion request: op must be fit_distortion"
    );

    // project_layer's distortion.
    let TestDoc::Byte(byte) = &doc else {
        unreachable!()
    };
    let mut bad = projection(0, "spherical", None);
    bad["distortion"] = json!("x");
    assert_eq!(
        project(byte, &bad).err().unwrap(),
        "invalid project layer request: distortion must be a finite number"
    );
    // −0.11 passes `radial_folds` (3 × 0.11 × 1.444 < 0.5) but leaves the
    // corner with no corrected point; +0.25 fails `radial_folds`.
    for k in [-0.11, 0.25] {
        let request = projection(0, "spherical", Some(k));
        assert_eq!(
            project(byte, &request).err().unwrap(),
            "invalid project layer request: distortion folds the layer",
            "k {k}"
        );
        assert_eq!(
            project_plan(byte, &request).err().unwrap(),
            "invalid project layer request: distortion folds the layer"
        );
    }
    // Inside the corner reach both ways, the projection runs.
    for k in [-0.10, 0.1] {
        project_plan(byte, &projection(0, "spherical", Some(k))).unwrap();
    }
}

/// Case (f): NULL handles.
#[test]
fn null_handles_answer_null_with_no_error() {
    let text = CString::new("{}").unwrap();
    let mut err = ptr::null_mut();
    unsafe {
        assert!(
            dr_doc_fit_distortion_plan(ptr::null(), text.as_ptr(), LIMIT, 0, &mut err).is_null()
        );
        assert!(
            dr_doc_fit_distortion(ptr::null(), text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
                .is_null()
        );
        assert!(dr_precision_doc_fit_distortion_plan(
            ptr::null(),
            text.as_ptr(),
            LIMIT,
            0,
            &mut err
        )
        .is_null());
        assert!(dr_precision_doc_fit_distortion(
            ptr::null(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        let doc = DrDocument::from_pixels(RgbaImage::new(4, 4));
        assert!(dr_doc_fit_distortion_plan(&doc, ptr::null(), LIMIT, 0, ptr::null_mut()).is_null());
        assert!(
            dr_doc_fit_distortion(&doc, ptr::null(), LIMIT, 0, ptr::null(), ptr::null_mut())
                .is_null()
        );
    }
    assert!(err.is_null());
}
