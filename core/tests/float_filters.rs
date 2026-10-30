//! Filters on a raster layer of a 32-bit float document,
//! through `dr_precision_doc_selected_edit` and its plan. This file
//! holds the Gaussian family (Gaussian Blur, Unsharp Mask, High Pass), the
//! five means (Box, Motion, Radial, Lens Blur, Tilt-Shift) and the cases every
//! filter shares (coverage, the 2^-24 floor, refusals, the plan, identity,
//! cancel). `float_filters_geometry.rs` holds Offset, Lens Correction and
//! Liquify, `float_filters_highlight.rs` the 5000-on-0.1 bound, and
//! `float_filters_display.rs` / `float_filters_develop.rs` the filters that
//! run on the display-encoded copy.
//!
//! Each oracle is a naive f64 loop written here from the filter's documented
//! formula, over a picture holding values below 0, above 1 and (in the spiked
//! twin) one pixel in the thousands, with opaque, partial and transparent
//! alpha. Nothing is read back from a kernel.
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_float::dr_float_operators;
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_selected_adjustment::{
    dr_precision_doc_selected_edit, dr_precision_doc_selected_edit_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::{json, Value};

use crate::extended_box_reference::{fitted_blur_rgba, FITTED_MIN_RADIUS};
use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// The float mode's transparent floor (2^-24).
pub(crate) const FLOOR: f64 = 1.0 / 16_777_216.0;
const LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];
const NOT_U16: &str = "selected edits require native u16 storage";
const RANGE: &str = "32-bit float samples must be finite within ±65504 with alpha in [0,1]";

// ---- the FFI ------------------------------------------------------------

fn take(err: *mut c_char) -> String {
    if err.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    text
}

fn request(op: &str, params: &Value) -> CString {
    CString::new(json!({"layer": 0, "op": op, "params": params}).to_string()).unwrap()
}

/// `Ok(None)` is the exact no-op answer (NULL with no error).
fn run(
    doc: &PrecisionDocument,
    op: &str,
    params: &Value,
    coverage: &[u16],
    control: *const DrJobControl,
) -> Result<Option<PrecisionDocument>, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            doc,
            request(op, params).as_ptr(),
            if coverage.is_empty() {
                ptr::null()
            } else {
                coverage.as_ptr()
            },
            coverage.len(),
            LIMIT,
            0,
            control,
            &mut err,
        )
    };
    let error = take(err);
    if out.is_null() {
        return if error.is_empty() {
            Ok(None)
        } else {
            Err(error)
        };
    }
    assert!(error.is_empty(), "{error}");
    Ok(Some(*unsafe { Box::from_raw(out) }))
}

/// `op` on layer 0 under `coverage` (empty: the whole layer), no job control.
pub(crate) fn filter(
    doc: &PrecisionDocument,
    op: &str,
    params: &Value,
    coverage: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
    run(doc, op, params, coverage, ptr::null())
}

/// The filter's result, which must exist.
pub(crate) fn filtered(doc: &PrecisionDocument, op: &str, params: &Value) -> Vec<[f32; 4]> {
    samples(
        &filter(doc, op, params, &[])
            .unwrap_or_else(|e| panic!("{op} {params}: {e}"))
            .unwrap_or_else(|| panic!("{op} {params}: no change")),
    )
}

pub(crate) fn plan(doc: &PrecisionDocument, op: &str, params: &Value) -> Result<Value, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit_plan(doc, request(op, params).as_ptr(), LIMIT, 0, &mut err)
    };
    let error = take(err);
    if out.is_null() {
        return Err(error);
    }
    Ok(serde_json::from_str(&take(out)).unwrap())
}

pub(crate) fn samples(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    doc.layer_source(0).unwrap().to_rgba32().unwrap()
}

pub(crate) fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

// ---- pictures -----------------------------------------------------------

/// A `w × h` plane of straight float samples, row 0 on top.
#[derive(Clone)]
pub(crate) struct Picture {
    pub(crate) w: u32,
    pub(crate) h: u32,
    pub(crate) pixels: Vec<[f32; 4]>,
}

impl Picture {
    /// The premultiplied sample at `(x, y)`, clamped to the plane. The
    /// kernels premultiply in f32, so this does too.
    pub(crate) fn premultiplied(&self, x: i64, y: i64) -> [f64; 4] {
        let cx = x.clamp(0, i64::from(self.w) - 1) as usize;
        let cy = y.clamp(0, i64::from(self.h) - 1) as usize;
        let p = self.pixels[cy * self.w as usize + cx];
        [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]].map(f64::from)
    }

    /// The premultiplied sample at continuous `(qx, qy)` (pixel centres at
    /// half-integers): the 2×2 tent, clamped edges.
    fn bilinear(&self, qx: f64, qy: f64) -> [f64; 4] {
        let (sx, sy) = (qx - 0.5, qy - 0.5);
        let (x0, y0) = (sx.floor() as i64, sy.floor() as i64);
        let mut out = [0.0; 4];
        for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            let (px, py) = (x0 + dx, y0 + dy);
            let weight = (1.0 - (sx - px as f64).abs()) * (1.0 - (sy - py as f64).abs());
            let p = self.premultiplied(px, py);
            for c in 0..4 {
                out[c] += weight * p[c];
            }
        }
        out
    }

    /// The largest magnitude of a premultiplied colour component.
    pub(crate) fn magnitude(&self) -> f64 {
        self.pixels
            .iter()
            .flat_map(|p| [p[0] * p[3], p[1] * p[3], p[2] * p[3]])
            .fold(0.0_f64, |m, v| m.max(f64::from(v.abs())))
    }

    fn count(&self) -> usize {
        self.pixels.len()
    }
}

/// A one-layer float document holding `picture`. The paste that builds it
/// trims a layer to its visible pixels, so the picture's corners must be
/// visible; the layer's stored samples are read back as the picture, because
/// a transparent pixel's hidden colour is the document's to keep or drop.
pub(crate) fn document(picture: &Picture) -> (PrecisionDocument, Picture) {
    let doc = float_doc(
        picture.w,
        picture.h,
        &IccProfile::srgb(),
        std::slice::from_ref(&picture.pixels),
    );
    let image = doc.layer_source(0).unwrap();
    assert_eq!(image.dimensions(), (picture.w, picture.h), "untrimmed");
    let stored = Picture {
        w: picture.w,
        h: picture.h,
        pixels: samples(&doc),
    };
    (doc, stored)
}

/// [`document`] for a picture with no transparent pixel.
pub(crate) fn solid_doc(picture: &Picture) -> PrecisionDocument {
    document(picture).0
}

/// 40×36: every colour channel runs below 0 and above 1; an alpha-0.5 block,
/// a transparent block and scattered alpha-0.125 pixels; with `spike`, one
/// opaque pixel in the thousands.
pub(crate) fn hdr_picture(spike: bool) -> Picture {
    hdr_plate(40, 36, spike.then_some((21, 9)))
}

/// [`hdr_picture`]'s pattern at any size of at least 40×36 (the alpha blocks
/// stay where they are), with the pixel in the thousands at `spike`.
pub(crate) fn hdr_plate(w: u32, h: u32, spike: Option<(u32, u32)>) -> Picture {
    let pixels = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let r = -0.4 + 0.11 * (x % 40) as f32 + 0.3 * ((x * 7 + y * 3) % 5) as f32;
            let g = 2.5 * ((x * 13 + y * 29) % 11) as f32 / 11.0 - 0.3;
            let b = 0.004 * ((y % 36) * (y % 36)) as f32 - 0.5 + 0.2 * ((x + 2 * y) % 3) as f32;
            let alpha = if (24..30).contains(&x) && (20..28).contains(&y) {
                0.0
            } else if (10..18).contains(&x) && (8..20).contains(&y) {
                0.5
            } else if i % 17 == 3 && x > 0 && x + 1 < w {
                0.125
            } else {
                1.0
            };
            if spike == Some((x, y)) {
                return [3000.0, 4500.0, 2000.0, 1.0];
            }
            [r, g, b, alpha]
        })
        .collect();
    Picture { w, h, pixels }
}

// ---- references ---------------------------------------------------------

/// Straight colour from a premultiplied mean, by the float mode's rule: below
/// 2^-24 of alpha transparent black, alpha at most 1.
pub(crate) fn straight_of(mean: [f64; 4]) -> [f64; 4] {
    if mean[3] < FLOOR {
        return [0.0; 4];
    }
    [
        mean[0] / mean[3],
        mean[1] / mean[3],
        mean[2] / mean[3],
        mean[3].min(1.0),
    ]
}

/// The premultiplied Gaussian of `picture`: separable, clamped edges, the
/// sampled Gaussian `exp(−k²/2σ²)` out to ±ceil(8σ) (a weight there is below
/// e^-32), normalized.
pub(crate) fn gaussian_reference(picture: &Picture, sigma: f64) -> Vec<[f64; 4]> {
    let half = (8.0 * sigma).ceil() as i64;
    let weights: Vec<f64> = (-half..=half)
        .map(|k| (-0.5 * (k as f64 / sigma).powi(2)).exp())
        .collect();
    let total: f64 = weights.iter().sum();
    let (w, h) = (i64::from(picture.w), i64::from(picture.h));
    let mut across = vec![[0.0_f64; 4]; picture.count()];
    for y in 0..h {
        for x in 0..w {
            let acc = &mut across[(y * w + x) as usize];
            for (j, weight) in weights.iter().enumerate() {
                let p = picture.premultiplied(x + j as i64 - half, y);
                for c in 0..4 {
                    acc[c] += weight / total * p[c];
                }
            }
        }
    }
    let mut out = vec![[0.0_f64; 4]; picture.count()];
    for y in 0..h {
        for x in 0..w {
            let acc = &mut out[(y * w + x) as usize];
            for (j, weight) in weights.iter().enumerate() {
                let sy = (y + j as i64 - half).clamp(0, h - 1);
                let p = across[(sy * w + x) as usize];
                for c in 0..4 {
                    acc[c] += weight / total * p[c];
                }
            }
        }
    }
    out
}

/// Gaussian Blur's premultiplied plane from radius 7 up: the
/// extended-box kernel, by `extended_box_reference`'s independent oracle.
pub(crate) fn fitted_reference(picture: &Picture, radius: f64) -> Vec<[f64; 4]> {
    let w = picture.w as usize;
    let premultiplied: Vec<[f64; 4]> = (0..picture.count())
        .map(|i| picture.premultiplied((i % w) as i64, (i / w) as i64))
        .collect();
    fitted_blur_rgba(&premultiplied, w, picture.h as usize, radius)
}

/// The premultiplied mean over the `(2r+1)²` square, clamped edges.
pub(crate) fn box_reference(picture: &Picture, radius: u32) -> Vec<[f64; 4]> {
    let r = i64::from(radius);
    let (w, h) = (i64::from(picture.w), i64::from(picture.h));
    let side = (2 * r + 1) as f64;
    let mut across = vec![[0.0_f64; 4]; picture.count()];
    for y in 0..h {
        for x in 0..w {
            for dx in -r..=r {
                let p = picture.premultiplied(x + dx, y);
                for c in 0..4 {
                    across[(y * w + x) as usize][c] += p[c];
                }
            }
        }
    }
    let mut out = vec![[0.0_f64; 4]; picture.count()];
    for y in 0..h {
        for x in 0..w {
            for dy in -r..=r {
                let p = across[((y + dy).clamp(0, h - 1) * w + x) as usize];
                for c in 0..4 {
                    out[(y * w + x) as usize][c] += p[c] / (side * side);
                }
            }
        }
    }
    out
}

/// Motion Blur: the mean of `distance + 1` bilinear samples one pixel apart
/// along the line at `angle` (counterclockwise on the y-down plane), centred
/// on the pixel.
pub(crate) fn motion_reference(picture: &Picture, angle: f64, distance: u32) -> Vec<[f64; 4]> {
    let (sin, cos) = angle.to_radians().sin_cos();
    let count = f64::from(distance + 1);
    (0..picture.count())
        .map(|i| {
            let (x, y) = (i as u32 % picture.w, i as u32 / picture.w);
            let mut acc = [0.0; 4];
            for k in 0..=distance {
                let t = f64::from(k) - f64::from(distance) / 2.0;
                let p =
                    picture.bilinear(f64::from(x) + 0.5 + t * cos, f64::from(y) + 0.5 - t * sin);
                for c in 0..4 {
                    acc[c] += p[c] / count;
                }
            }
            acc
        })
        .collect()
}

/// Radial Blur at Good or Best: inside `frame` (`x, y, width, height`; `None`
/// is the plane) the mean of `N = 1 + min(cap, ⌈length · density⌉)` bilinear
/// samples at `t_k = k / (N − 1)` along the arc (Spin: one degree per Amount)
/// or the ray (Zoom: 0.005 of the radius per Amount) through the pixel, about
/// `center` given in frame fractions; outside it, the premultiplied source.
pub(crate) fn radial_reference(
    picture: &Picture,
    method: &str,
    quality: &str,
    amount: u32,
    center: (f64, f64),
    frame: Option<(u32, u32, u32, u32)>,
) -> Vec<[f64; 4]> {
    let (fx, fy, fw, fh) = frame.unwrap_or((0, 0, picture.w, picture.h));
    let (density, cap) = match quality {
        "good" => (1.0, 64.0),
        "best" => (2.0, 128.0),
        other => panic!("no reference for quality {other}"),
    };
    let spin = method == "spin";
    let per_radius = f64::from(amount)
        * if spin {
            std::f64::consts::PI / 180.0
        } else {
            0.005
        };
    let cx = f64::from(fx) + center.0 * f64::from(fw);
    let cy = f64::from(fy) + center.1 * f64::from(fh);
    (0..picture.count())
        .map(|i| {
            let (x, y) = (i as u32 % picture.w, i as u32 / picture.w);
            if x < fx || y < fy || x >= fx + fw || y >= fy + fh {
                return picture.premultiplied(i64::from(x), i64::from(y));
            }
            let (dx, dy) = (f64::from(x) + 0.5 - cx, f64::from(y) + 0.5 - cy);
            let length = dx.hypot(dy) * per_radius;
            let n = 1 + (length * density).ceil().min(cap) as u32;
            let mut acc = [0.0; 4];
            for k in 0..n {
                let t = if n == 1 {
                    0.5
                } else {
                    f64::from(k) / f64::from(n - 1)
                };
                let (a, b) = if spin {
                    let (sin, cos) = (per_radius * (t - 0.5)).sin_cos();
                    (cos, sin)
                } else {
                    (1.0 + per_radius * (t - 0.5), 0.0)
                };
                let p = picture.bilinear(cx + a * dx - b * dy, cy + b * dx + a * dy);
                for c in 0..4 {
                    acc[c] += p[c] / f64::from(n);
                }
            }
            acc
        })
        .collect()
}

/// The weighted premultiplied mean over the lattice disc `dx² + dy² ≤
/// (r + ½)²` about `(x, y)`, clamped edges. `highlight` is Lens Blur's
/// `(brightness, threshold)`: a pixel weighs `1 + brightness/100 · 8 ·
/// clamp((Y − t) / max(1 − t, 1/255), 0, 1)`, `t = threshold/255`, `Y` the
/// luma of its premultiplied colour.
fn disc_mean(picture: &Picture, r: i64, highlight: Option<(f64, f64)>, x: i64, y: i64) -> [f64; 4] {
    let mut acc = [0.0; 4];
    let mut total = 0.0;
    for dy in -r..=r {
        for dx in -r..=r {
            if 4 * (dx * dx + dy * dy) > (2 * r + 1) * (2 * r + 1) {
                continue;
            }
            let p = picture.premultiplied(x + dx, y + dy);
            let weight = match highlight {
                None => 1.0,
                Some((brightness, threshold)) => {
                    let luma = LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2];
                    let t = threshold / 255.0;
                    1.0 + brightness / 100.0
                        * 8.0
                        * ((luma - t) / (1.0 - t).max(1.0 / 255.0)).clamp(0.0, 1.0)
                }
            };
            for c in 0..4 {
                acc[c] += weight * p[c];
            }
            total += weight;
        }
    }
    acc.map(|v| v / total)
}

/// Lens Blur: [`disc_mean`] at one radius everywhere.
pub(crate) fn disc_reference(
    picture: &Picture,
    radius: u32,
    highlight: Option<(f64, f64)>,
) -> Vec<[f64; 4]> {
    (0..picture.count())
        .map(|i| {
            let (x, y) = (i as u32 % picture.w, i as u32 / picture.w);
            disc_mean(
                picture,
                i64::from(radius),
                highlight,
                i64::from(x),
                i64::from(y),
            )
        })
        .collect()
}

/// Tilt-Shift `(radius, center, angle, focus, transition)`: the plain disc
/// mean at radius `ρ = radius · clamp((d − focus·H) / (transition·H), 0, 1)`,
/// `d` the distance from the pixel centre to the line through `center` (plane
/// fractions) at `angle`; a fractional ρ is the lerp of the two discs around
/// it.
pub(crate) fn tilt_reference(
    picture: &Picture,
    (radius, center, angle, focus, transition): (u32, (f64, f64), f64, f64, f64),
) -> Vec<[f64; 4]> {
    let (width, height) = (f64::from(picture.w), f64::from(picture.h));
    let (sin, cos) = angle.to_radians().sin_cos();
    (0..picture.count())
        .map(|i| {
            let (x, y) = (i as u32 % picture.w, i as u32 / picture.w);
            let d = ((f64::from(x) + 0.5 - center.0 * width) * sin
                + (f64::from(y) + 0.5 - center.1 * height) * cos)
                .abs();
            let beyond = d - focus * height;
            let t = if transition > 0.0 {
                (beyond / (transition * height)).clamp(0.0, 1.0)
            } else if beyond > 0.0 {
                1.0
            } else {
                0.0
            };
            let rho = f64::from(radius) * t;
            let lower = rho.floor();
            let (x, y) = (i64::from(x), i64::from(y));
            let mut mean = disc_mean(picture, lower as i64, None, x, y);
            if rho > lower {
                let upper = disc_mean(picture, lower as i64 + 1, None, x, y);
                for c in 0..4 {
                    mean[c] += (rho - lower) * (upper[c] - mean[c]);
                }
            }
            mean
        })
        .collect()
}

/// `got` (straight) against a premultiplied reference mean, compared
/// premultiplied so a small alpha does not amplify the comparison: each of
/// colour·alpha and alpha within `relative · |want| + absolute`. A reference
/// below the floor must be transparent black bit for bit.
pub(crate) fn close(
    got: &[[f32; 4]],
    reference: &[[f64; 4]],
    relative: f64,
    absolute: f64,
    what: &str,
) {
    assert_eq!(got.len(), reference.len(), "{what}");
    for (i, (got, mean)) in got.iter().zip(reference).enumerate() {
        // Within rounding of the floor either answer is right.
        if (mean[3] - FLOOR).abs() <= 1e-6 * FLOOR {
            continue;
        }
        let want = straight_of(*mean);
        if want[3] == 0.0 {
            assert_eq!(
                got.map(f32::to_bits),
                [0; 4],
                "{what} pixel {i}: transparent"
            );
            continue;
        }
        let alpha = f64::from(got[3]);
        assert!(alpha <= 1.0, "{what} pixel {i}: alpha {alpha}");
        for c in 0..4 {
            let (g, w) = if c == 3 {
                (alpha, want[3])
            } else {
                (f64::from(got[c]) * alpha, want[c] * want[3])
            };
            assert!(
                (g - w).abs() <= relative * w.abs() + absolute,
                "{what} pixel {i} component {c}: got {g}, want {w}"
            );
        }
    }
}

/// The tolerances of a comparison on `picture`: `(relative, absolute)`.
/// `relative` is f32 storage (two roundings of 2^-24 and the premultiply).
/// `absolute` scales with the picture's largest premultiplied magnitude `M`:
/// `share · M` is what a kernel may lose to its own stated rule.
fn tolerance(picture: &Picture, share: f64) -> (f64, f64) {
    (3e-6, share * picture.magnitude().max(1.0))
}

/// The Gaussian's share: the FIR drops under 1e-7 of the weight on each axis,
/// and its ring holds f32 rows whose rounding is relative to the row's own
/// magnitude (at most `M`).
const GAUSSIAN_SHARE: f64 = 5e-7;
/// A box, disc or path sum is exact in f64 (a float layer's Motion and Radial
/// Blur place and weigh their samples in binary64); what is left is f32
/// rounding of sums whose terms may cancel.
const SUM_SHARE: f64 = 2e-7;

// ---- the Gaussian family ------------------------------------------------

/// The σ a `blur` request runs: the parser narrows it to f32.
fn blur_sigma(sigma: f64) -> f64 {
    f64::from(sigma as f32)
}

#[test]
fn gaussian_blur_is_the_sampled_gaussian_in_linear_light_with_no_clamp() {
    for spike in [false, true] {
        let (doc, picture) = document(&hdr_picture(spike));
        let (relative, absolute) = tolerance(&picture, GAUSSIAN_SHARE);
        // 0.75 and 3.9 are below the other depths' switch to the recursive
        // kernel, 4 is the switch: a float layer runs one FIR at all of them.
        // From 7 up Gaussian Blur is the extended-box kernel at every depth,
        // so σ8, which was the FIR too, now has that kernel's oracle.
        for sigma in [0.75, 2.5, 3.9, 4.0, 8.0] {
            let got = filtered(&doc, "blur", &json!({"sigma": sigma}));
            let reference = if sigma >= FITTED_MIN_RADIUS {
                fitted_reference(&picture, sigma)
            } else {
                gaussian_reference(&picture, blur_sigma(sigma))
            };
            close(
                &got,
                &reference,
                relative,
                absolute,
                &format!("blur σ{sigma} spike {spike}"),
            );
        }
    }
}

#[test]
fn a_blurred_highlight_spreads_its_energy_and_lifts_its_neighbours_above_white() {
    let (w, h) = (48_u32, 44_u32);
    let spike = (w * 20 + 23) as usize;
    let mut pixels = vec![[0.25_f32, 0.25, 0.25, 1.0]; (w * h) as usize];
    pixels[spike] = [5000.0, 2500.0, 1250.0, 1.0];
    let picture = Picture { w, h, pixels };
    let doc = solid_doc(&picture);
    let total =
        |pixels: &[[f32; 4]], c: usize| -> f64 { pixels.iter().map(|p| f64::from(p[c])).sum() };
    for (op, params) in [
        ("blur", json!({"sigma": 2})),
        ("box_blur", json!({"radius": 2})),
        ("motion_blur", json!({"angle": 0, "distance": 6})),
        ("lens_blur", json!({"radius": 3})),
    ] {
        let out = filtered(&doc, op, &params);
        // The highlight is lower, the pixel beside it far above white, and
        // (the kernel is a normalized mean, the surround constant, the edges
        // clamped) the plane's sum is what it was.
        assert!(out[spike][0] < 2500.0, "{op}: {:?}", out[spike]);
        assert!(out[spike + 1][0] > 50.0, "{op}: {:?}", out[spike + 1]);
        for c in 0..3 {
            let (before, after) = (total(&picture.pixels, c), total(&out, c));
            assert!(
                (after - before).abs() <= 2e-6 * before,
                "{op} channel {c}: {before} became {after}"
            );
        }
        assert!(out.iter().all(|p| p[3] == 1.0), "{op}: opaque stays opaque");
    }
}

/// Unsharp Mask's and High Pass's `low`: the straight colour of the
/// premultiplied Gaussian, `None` below the floor.
fn low_pass(reference: [f64; 4]) -> Option<[f64; 3]> {
    (reference[3] >= FLOOR).then(|| {
        [
            reference[0] / reference[3],
            reference[1] / reference[3],
            reference[2] / reference[3],
        ]
    })
}

/// How far `low`'s component `c` may be from the reference: the Gaussian's
/// premultiplied tolerance divided by the mean alpha.
fn low_tolerance(picture: &Picture, reference: [f64; 4], c: usize) -> f64 {
    let (relative, absolute) = tolerance(picture, GAUSSIAN_SHARE);
    (relative * reference[c].abs() + absolute) / reference[3] * 2.0
}

#[test]
fn unsharp_mask_adds_the_linear_detail_and_keeps_values_below_zero() {
    for spike in [false, true] {
        let (doc, picture) = document(&hdr_picture(spike));
        for (radius, amount, threshold) in [(1.5, 1.0, 0.0), (3.0, 2.5, 0.0), (6.0, 0.8, 0.05)] {
            let params = json!({"radius": radius, "amount": amount, "threshold": threshold});
            let got = filtered(&doc, "unsharp_mask", &params);
            let reference = gaussian_reference(&picture, radius);
            let mut lowest = f64::INFINITY;
            for (i, (got, source)) in got.iter().zip(&picture.pixels).enumerate() {
                let what = format!("unsharp {params} spike {spike} pixel {i}");
                assert_eq!(got[3].to_bits(), source[3].to_bits(), "{what}: alpha");
                let Some(low) = low_pass(reference[i]).filter(|_| source[3] > 0.0) else {
                    assert_eq!(got.map(f32::to_bits), source.map(f32::to_bits), "{what}");
                    continue;
                };
                for c in 0..3 {
                    let slack = low_tolerance(&picture, reference[i], c);
                    let detail = f64::from(source[c]) - low[c];
                    // On the threshold either branch is right.
                    if (detail.abs() - threshold).abs() <= slack {
                        continue;
                    }
                    let want = if detail.abs() > threshold {
                        f64::from(source[c]) + amount * detail
                    } else {
                        f64::from(source[c])
                    };
                    let value = f64::from(got[c]);
                    assert!(
                        (value - want).abs() <= amount * slack + 1e-6 * want.abs(),
                        "{what} component {c}: got {value}, want {want}"
                    );
                    lowest = lowest.min(value);
                }
            }
            // Sharpening beside a brighter pixel goes below zero and stays.
            assert!(lowest < -0.4, "{params} spike {spike}: lowest {lowest}");
        }
    }
}

#[test]
fn high_pass_is_the_stored_half_plus_the_linear_detail_unclamped() {
    for spike in [false, true] {
        let (doc, picture) = document(&hdr_picture(spike));
        for radius in [2.0, 7.5] {
            let got = filtered(&doc, "high_pass", &json!({"radius": radius}));
            let reference = gaussian_reference(&picture, radius);
            let (mut lowest, mut highest) = (f64::INFINITY, f64::NEG_INFINITY);
            for (i, (got, source)) in got.iter().zip(&picture.pixels).enumerate() {
                let what = format!("high pass r{radius} spike {spike} pixel {i}");
                assert_eq!(got[3].to_bits(), source[3].to_bits(), "{what}: alpha");
                let Some(low) = low_pass(reference[i]).filter(|_| source[3] > 0.0) else {
                    assert_eq!(got.map(f32::to_bits), source.map(f32::to_bits), "{what}");
                    continue;
                };
                for c in 0..3 {
                    let want = 0.5 + f64::from(source[c]) - low[c];
                    let value = f64::from(got[c]);
                    assert!(
                        (value - want).abs()
                            <= low_tolerance(&picture, reference[i], c) + 1e-6 * want.abs(),
                        "{what} component {c}: got {value}, want {want}"
                    );
                    lowest = lowest.min(value);
                    highest = highest.max(value);
                }
            }
            assert!(lowest < 0.0 && highest > 1.0, "{lowest} … {highest}");
        }
    }
    // A constant plane is the pivot and nothing else, whatever its level:
    // `source = low + (high − 0.5)` with one constant at every depth.
    let flat = Picture {
        w: 12,
        h: 9,
        pixels: vec![[7.0, -0.25, 0.01, 1.0]; 108],
    };
    for p in filtered(&solid_doc(&flat), "high_pass", &json!({"radius": 3})) {
        for c in 0..3 {
            assert!((p[c] - 0.5).abs() <= 2e-6, "{p:?}");
        }
    }
}

// ---- the five means -----------------------------------------------------

#[test]
fn box_blur_is_the_square_mean() {
    for spike in [false, true] {
        let (doc, picture) = document(&hdr_picture(spike));
        let (relative, absolute) = tolerance(&picture, SUM_SHARE);
        for radius in [1_u32, 3, 50] {
            let got = filtered(&doc, "box_blur", &json!({"radius": radius}));
            let what = format!("box r{radius} spike {spike}");
            close(
                &got,
                &box_reference(&picture, radius),
                relative,
                absolute,
                &what,
            );
        }
    }
}

#[test]
fn motion_blur_is_the_mean_along_the_line() {
    for spike in [false, true] {
        let (doc, picture) = document(&hdr_picture(spike));
        let (relative, absolute) = tolerance(&picture, SUM_SHARE);
        for (angle, distance) in [(0.0, 1_u32), (0.0, 6), (90.0, 5), (30.0, 9), (-135.0, 12)] {
            let params = json!({"angle": angle, "distance": distance});
            let got = filtered(&doc, "motion_blur", &params);
            let what = format!("motion {params} spike {spike}");
            let reference = motion_reference(&picture, angle, distance);
            close(&got, &reference, relative, absolute, &what);
        }
    }
}

#[test]
fn radial_blur_is_the_mean_along_the_arc_or_the_ray() {
    for spike in [false, true] {
        let (doc, picture) = document(&hdr_picture(spike));
        let (relative, absolute) = tolerance(&picture, SUM_SHARE);
        for (method, quality, amount, center) in [
            ("spin", "good", 25_u32, (0.4, 0.55)),
            ("spin", "best", 60, (0.5, 0.5)),
            ("zoom", "good", 40, (0.5, 0.5)),
            ("zoom", "best", 100, (0.1, 0.9)),
        ] {
            let params = json!({"method": method, "quality": quality, "amount": amount,
                "center_x": center.0, "center_y": center.1});
            let got = filtered(&doc, "radial_blur", &params);
            let what = format!("radial {params} spike {spike}");
            let reference = radial_reference(&picture, method, quality, amount, center, None);
            close(&got, &reference, relative, absolute, &what);
        }
    }
}

#[test]
fn radial_blur_draft_stays_inside_the_picture_and_replays() {
    // Draft's jitter is its definition (a fixed positional stream), so there
    // is no independent reference for its samples: every output is a mean of
    // pixels the picture holds, and the same request gives the same bits.
    let (doc, picture) = document(&hdr_picture(true));
    let params = json!({"quality": "draft", "amount": 40});
    let first = filtered(&doc, "radial_blur", &params);
    assert_eq!(bits(&first), bits(&filtered(&doc, "radial_blur", &params)));
    for c in 0..3 {
        let visible = picture.pixels.iter().filter(|p| p[3] > 0.0).map(|p| p[c]);
        let lo = visible.clone().fold(f32::INFINITY, f32::min);
        let hi = visible.fold(f32::NEG_INFINITY, f32::max);
        let slack = 1e-5 * hi.abs().max(1.0);
        for p in first.iter().filter(|p| p[3] > 0.0) {
            assert!(p[c] >= lo - slack && p[c] <= hi + slack, "{p:?} {lo} {hi}");
        }
    }
    assert!(first.iter().any(|p| p[0] > 1.0 || p[1] > 1.0));
}

#[test]
fn lens_blur_is_the_weighted_disc_mean_on_linear_luma() {
    for spike in [false, true] {
        let (doc, picture) = document(&hdr_picture(spike));
        let (relative, absolute) = tolerance(&picture, SUM_SHARE);
        for (radius, brightness, threshold) in [
            (1_u32, 0.0, 255.0),
            (4, 0.0, 255.0),
            (3, 60.0, 128.0),
            (5, 100.0, 200.0),
            (2, 100.0, 255.0),
        ] {
            let params =
                json!({"radius": radius, "brightness": brightness, "threshold": threshold});
            let got = filtered(&doc, "lens_blur", &params);
            let highlight = (brightness > 0.0).then_some((brightness, threshold));
            let what = format!("lens {params} spike {spike}");
            let reference = disc_reference(&picture, radius, highlight);
            close(&got, &reference, relative, absolute, &what);
        }
    }
}

#[test]
fn tilt_shift_is_the_disc_mean_at_each_pixels_radius() {
    for spike in [false, true] {
        let (doc, picture) = document(&hdr_picture(spike));
        let (relative, absolute) = tolerance(&picture, SUM_SHARE);
        for tilt in [
            (4_u32, (0.5, 0.5), 0.0, 0.15, 0.25),
            (5, (0.3, 0.6), 35.0, 0.1, 0.4),
            (3, (0.5, 0.25), -90.0, 0.2, 0.0),
            (6, (0.5, 0.5), 0.0, 0.0, 1.0),
        ] {
            let params = json!({"radius": tilt.0, "center_x": tilt.1 .0, "center_y": tilt.1 .1,
                "angle": tilt.2, "focus": tilt.3, "transition": tilt.4});
            let got = filtered(&doc, "tilt_shift", &params);
            let what = format!("tilt {params} spike {spike}");
            close(
                &got,
                &tilt_reference(&picture, tilt),
                relative,
                absolute,
                &what,
            );
        }
    }
}

// ---- shared cases -------------------------------------------------------

/// One request per filter this file covers, each changing the HDR picture.
fn requests() -> Vec<(&'static str, Value)> {
    vec![
        ("blur", json!({"sigma": 2.5})),
        ("unsharp_mask", json!({"radius": 2, "amount": 1.5})),
        ("high_pass", json!({"radius": 3})),
        ("box_blur", json!({"radius": 2})),
        ("motion_blur", json!({"angle": 30, "distance": 7})),
        ("radial_blur", json!({"amount": 30})),
        (
            "lens_blur",
            json!({"radius": 3, "brightness": 50, "threshold": 128}),
        ),
        (
            "tilt_shift",
            json!({"radius": 4, "focus": 0.05, "transition": 0.3}),
        ),
    ]
}

#[test]
fn coverage_gates_the_write_bit_for_bit_at_its_ends_and_mixes_between() {
    let (doc, picture) = document(&hdr_picture(true));
    let count = picture.count();
    // Diagonals of 0, a sliver, half and full coverage: every edge of the
    // plane holds a covered pixel, so the coverage's bounding box is the plane.
    let weights: Vec<u16> = (0..count)
        .map(|i| [0, 1, 32768, u16::MAX][(i % 40 + i / 40) % 4])
        .collect();
    for (op, params) in requests() {
        // Radial Blur takes its frame from the coverage's bounding box, which
        // is the plane here, so its full-coverage result is the same request.
        let full = filtered(&doc, op, &params);
        let covered = vec![u16::MAX; count];
        assert_eq!(
            bits(&samples(
                &filter(&doc, op, &params, &covered).unwrap().unwrap()
            )),
            bits(&full),
            "{op}: a full coverage is no coverage"
        );
        assert!(
            filter(&doc, op, &params, &vec![0; count])
                .unwrap()
                .is_none(),
            "{op}: nothing selected"
        );
        let out = samples(&filter(&doc, op, &params, &weights).unwrap().unwrap());
        for i in 0..count {
            let (before, after, got) = (picture.pixels[i], full[i], out[i]);
            let what = format!("{op} pixel {i} weight {}", weights[i]);
            match weights[i] {
                0 => assert_eq!(got.map(f32::to_bits), before.map(f32::to_bits), "{what}"),
                u16::MAX => assert_eq!(got.map(f32::to_bits), after.map(f32::to_bits), "{what}"),
                weight => {
                    let t = f64::from(weight) / 65535.0;
                    let (a0, a1) = (f64::from(before[3]), f64::from(after[3]));
                    let same_alpha = before[3].to_bits() == after[3].to_bits();
                    let alpha = if same_alpha { a0 } else { a0 + t * (a1 - a0) };
                    if alpha < FLOOR {
                        if !same_alpha {
                            assert_eq!(got.map(f32::to_bits), [0; 4], "{what}");
                        }
                        continue;
                    }
                    assert!((f64::from(got[3]) - alpha).abs() <= 1e-6 * alpha, "{what}");
                    for c in 0..3 {
                        let (old, new) = (f64::from(before[c]), f64::from(after[c]));
                        // The straight lerp where alpha is kept; otherwise the
                        // premultiplied mix, so a colour arriving with more
                        // alpha weighs more.
                        let want = if same_alpha {
                            old + t * (new - old)
                        } else {
                            (old * a0 * (1.0 - t) + new * a1 * t) / alpha
                        };
                        let value = f64::from(got[c]);
                        assert!(
                            (value - want).abs() <= 2e-6 * (old.abs() + new.abs()).max(1.0),
                            "{what} component {c}: got {value}, want {want}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_mean_below_the_float_floor_is_transparent_black_and_above_it_keeps_its_colour() {
    // One pixel of alpha 2^-20 on a transparent layer; the corners hold a
    // sliver of alpha so the layer keeps its size.
    let (w, h) = (33_u32, 33_u32);
    let lone = [3.0_f32, -0.5, 0.25, 2f32.powi(-20)];
    let mut pixels = vec![[0.0_f32; 4]; (w * h) as usize];
    for corner in [0, w - 1, w * (h - 1), w * h - 1] {
        pixels[corner as usize] = [1.0, 1.0, 1.0, 2f32.powi(-40)];
    }
    let centre = (w * 16 + 16) as usize;
    pixels[centre] = lone;
    let (doc, picture) = document(&Picture { w, h, pixels });
    // Radius 1: the mean alpha is 2^-20 / 9, above 2^-24 (and below half a
    // 16-bit code, the floor of the other depths), so the 3×3 block keeps the
    // pixel's colour.
    let near = filtered(&doc, "box_blur", &json!({"radius": 1}));
    for (i, p) in near.iter().enumerate() {
        let (x, y) = (i as i64 % 33 - 16, i as i64 / 33 - 16);
        if x.abs() <= 1 && y.abs() <= 1 {
            assert!(
                (f64::from(p[3]) - f64::from(lone[3]) / 9.0).abs() <= 1e-12,
                "{p:?}"
            );
            for c in 0..3 {
                assert!((p[c] - lone[c]).abs() <= 1e-5, "{i}: {p:?}");
            }
        } else {
            assert_eq!(p.map(f32::to_bits), [0; 4], "{i}");
        }
    }
    // Radius 2: 2^-20 / 25 is below the floor everywhere.
    let far = filtered(&doc, "box_blur", &json!({"radius": 2}));
    assert!(far.iter().all(|p| p.map(f32::to_bits) == [0; 4]));
    // The same floor on the Gaussian family: at σ1 the pixel and its four
    // neighbours stay above it (2^-20 · 0.399 · 0.242 is 9.2e-8).
    let reference = gaussian_reference(&picture, 1.0);
    let blurred = filtered(&doc, "blur", &json!({"sigma": 1}));
    let mut kept = 0;
    for (i, (p, mean)) in blurred.iter().zip(&reference).enumerate() {
        if p.map(f32::to_bits) == [0; 4] {
            assert!(mean[3] < FLOOR * 1.001, "{i}: dropped alpha {}", mean[3]);
            continue;
        }
        kept += 1;
        assert!(f64::from(p[3]) >= FLOOR, "{i}: {p:?}");
        for c in 0..3 {
            assert!((p[c] - lone[c]).abs() <= 1e-4, "{i}: {p:?}");
        }
    }
    assert_eq!(kept, 5);
}

#[test]
fn one_opaque_pixel_under_a_wide_gaussian_is_its_colour_or_transparent_black() {
    let (w, h) = (131_u32, 131_u32);
    let colour = [12.0_f32, -0.75, 0.5];
    let mut pixels = vec![[0.0_f32; 4]; (w * h) as usize];
    for corner in [0, w - 1, w * (h - 1), w * h - 1] {
        pixels[corner as usize] = [1.0, 1.0, 1.0, 2f32.powi(-40)];
    }
    pixels[(w * 65 + 65) as usize] = [colour[0], colour[1], colour[2], 1.0];
    let (doc, picture) = document(&Picture { w, h, pixels });
    let out = filtered(&doc, "blur", &json!({"sigma": 8}));
    // Radius 8 is the extended-box kernel: its compact support
    // (±28 pixels) is the reach, where the Gaussian's tails were.
    let reference = fitted_reference(&picture, 8.0);
    let mut kept = 0;
    for (i, (p, mean)) in out.iter().zip(&reference).enumerate() {
        if p.map(f32::to_bits) == [0; 4] {
            // Dropped by the floor. (Beyond the kernel's own reach the alpha
            // left is far below it.)
            assert!(mean[3] < FLOOR * 1.001, "{i}: dropped alpha {}", mean[3]);
            continue;
        }
        kept += 1;
        assert!(f64::from(p[3]) >= FLOOR && p[3] <= 1.0, "{i}: {p:?}");
        assert!(
            (f64::from(p[3]) - mean[3]).abs() <= 3e-6 * mean[3],
            "{i}: {p:?}"
        );
        for c in 0..3 {
            assert!(
                (p[c] - colour[c]).abs() <= 2e-5 * colour[c].abs().max(1.0),
                "{i}: {p:?}"
            );
        }
    }
    assert!(kept > 2000, "{kept} pixels kept");
    for corner in [0, w - 1, w * (h - 1), w * h - 1] {
        assert_eq!(out[corner as usize].map(f32::to_bits), [0; 4]);
    }
}

#[test]
fn a_layer_below_the_floor_blurs_to_nothing_and_is_not_sharpened() {
    // Every pixel at alpha 2^-26: the Gaussian's mean alpha is below the
    // floor, so there is no low-pass colour.
    let (w, h) = (9_u32, 7_u32);
    let pixels = (0..w * h)
        .map(|i| [i as f32 * 0.5 - 3.0, 2.0, 0.25, 2f32.powi(-26)])
        .collect();
    let (doc, _) = document(&Picture { w, h, pixels });
    let blurred = filtered(&doc, "blur", &json!({"sigma": 2}));
    assert!(blurred.iter().all(|p| p.map(f32::to_bits) == [0; 4]));
    for (op, params) in [
        ("unsharp_mask", json!({"radius": 2, "amount": 2})),
        ("high_pass", json!({"radius": 2})),
    ] {
        assert!(filter(&doc, op, &params, &[]).unwrap().is_none(), "{op}");
    }
}

#[test]
fn a_result_outside_the_float_range_refuses() {
    let (w, h) = (8_u32, 8_u32);
    let pixels = (0..w * h)
        .map(|i| {
            let v = if i % w < 4 { 60000.0 } else { 0.0 };
            [v, v, v, 1.0]
        })
        .collect();
    let doc = solid_doc(&Picture { w, h, pixels });
    assert_eq!(
        filter(
            &doc,
            "unsharp_mask",
            &json!({"radius": 2, "amount": 5}),
            &[]
        )
        .err(),
        Some(RANGE.into())
    );
    // The same picture blurs: a mean never leaves the range its inputs hold.
    assert!(filter(&doc, "blur", &json!({"sigma": 2}), &[])
        .unwrap()
        .is_some());
}

#[test]
fn a_mean_of_partial_alpha_at_the_end_of_the_float_range_stays_inside_it() {
    // Every pixel holds the same colour, at the two ends of what a float
    // layer stores, so the exact mean of any of the five is that colour. The
    // kernels divide a rounded premultiplied mean by a rounded alpha, and
    // that quotient can land one ulp past ±65504, where the write would
    // refuse a picture that is legal. Each mean finishes in the layer's own
    // visible range, which here is the one colour: bit for bit.
    let (w, h) = (24_u32, 16_u32);
    let end = [65504.0_f32, -65504.0, 65504.0];
    let scale = 2f32.powi(-24);
    let layer = |alpha: &dyn Fn(u32) -> f32| {
        let pixels = (0..w * h)
            .map(|i| [end[0], end[1], end[2], alpha(i)])
            .collect();
        solid_doc(&Picture { w, h, pixels })
    };
    let held = |pixels: &[[f32; 4]], what: &str| {
        for (i, p) in pixels.iter().enumerate() {
            assert!(p[3] > 0.0 && p[3] <= 1.0, "{what} pixel {i}: {p:?}");
            assert_eq!(
                [p[0], p[1], p[2]].map(f32::to_bits),
                end.map(f32::to_bits),
                "{what} pixel {i}: {p:?}"
            );
        }
    };
    // One alpha everywhere, 8395652 · 2^-24: 65504 times it rounds up in f32
    // and the quotient is 65504.004. The layer's own mean is the layer, so
    // "no change" is the other right answer.
    let uniform = layer(&|_| 8_395_652.0 * scale);
    for radius in [1_u32, 2, 5] {
        let what = format!("uniform box r{radius}");
        match filter(&uniform, "box_blur", &json!({"radius": radius}), &[]) {
            Ok(None) => {}
            Ok(Some(out)) => held(&samples(&out), &what),
            Err(e) => panic!("{what}: {e}"),
        }
    }
    // A different 24-bit alpha at every pixel (none below 2^-12), so each
    // output is a different quotient.
    let varied = layer(&|i| {
        let k = u64::from(i) * 2_654_435_761 % 16_773_120;
        (4096 + k) as f32 * scale
    });
    for (op, params) in requests() {
        if matches!(op, "blur" | "unsharp_mask" | "high_pass") {
            continue;
        }
        let out = filter(&varied, op, &params, &[])
            .unwrap_or_else(|e| panic!("{op} {params}: {e}"))
            .unwrap_or_else(|| panic!("{op} {params}: no change"));
        held(&samples(&out), op);
    }
}

/// The filters a float layer still refuses, each with parameters its parser
/// takes: the Gaussian's other name and the automatic tones.
fn refused() -> Vec<(&'static str, Value)> {
    vec![
        ("gaussian_blur", json!({"radius": 2})),
        ("levels_channels", json!({})),
    ]
}

#[test]
fn every_other_filter_keeps_its_refusal_and_the_table_names_the_twenty_six() {
    let (doc, _) = document(&hdr_picture(false));
    for (op, params) in refused() {
        assert_eq!(
            plan(&doc, op, &params).err().as_deref(),
            Some(NOT_U16),
            "{op} plan"
        );
        assert_eq!(
            filter(&doc, op, &params, &[]).err().as_deref(),
            Some(NOT_U16),
            "{op}"
        );
    }
    let table: Value = serde_json::from_str(&take(unsafe { dr_float_operators() })).unwrap();
    let admitted = table["admitted"].as_array().unwrap();
    let names = [
        "blur",
        "box_blur",
        "motion_blur",
        "radial_blur",
        "lens_blur",
        "tilt_shift",
        "offset",
        "lens_correction",
        "liquify",
        "unsharp_mask",
        "high_pass",
        "surface_blur",
        "dust_and_scratches",
        "reduce_noise",
        "add_noise",
        "add_grain",
        "clouds",
        "difference_clouds",
        "emboss",
        "edge_detect",
        "pixelate",
        "sharpen",
        "presence",
        "develop_filter",
        "smart_sharpen",
        "depth_blur",
    ];
    for op in names {
        assert!(admitted.contains(&json!(format!("{op}_filter"))), "{op}");
    }
    let filters = admitted
        .iter()
        .filter(|row| row.as_str().unwrap().ends_with("_filter"))
        .count();
    assert_eq!(filters, names.len());
    assert!(!table["refused"].as_array().unwrap().iter().any(|row| {
        let operator = row["operator"].as_str().unwrap();
        operator.contains("Surface Blur") || operator.contains("Remove Red Eye")
    }));
    // A parameter refusal is the parser's, as at 16 bits.
    assert_eq!(
        filter(&doc, "box_blur", &json!({"radius": 0}), &[])
            .err()
            .as_deref(),
        Some("radius must be finite in [1,1000]")
    );
    assert_eq!(
        filter(&doc, "blur", &json!({"radius": 2}), &[])
            .err()
            .as_deref(),
        Some("unexpected blur parameter radius")
    );
}

#[test]
fn the_plan_reports_the_float_output_and_charges_the_apply() {
    let (doc, picture) = document(&hdr_picture(false));
    let count = picture.count() as u64;
    for (op, params) in requests() {
        let plan = plan(&doc, op, &params).unwrap();
        assert_eq!(plan["output_bytes"], count * 16, "{op}");
        assert_eq!(plan["input_samples"], count, "{op}");
        assert_eq!(plan["input_width"], picture.w, "{op}");
        assert_eq!(plan["input_height"], picture.h, "{op}");
        assert_eq!(plan["retained_bytes"], doc.retained_bytes(), "{op}");
        // The apply stage (40 bytes a pixel) and the borrowed coverage.
        assert!(
            plan["scratch_bytes"].as_u64().unwrap() >= count * 42,
            "{op}: {plan}"
        );
    }
    // A wider Gaussian holds a taller ring, and the plan says so.
    let scratch = |sigma: f64| {
        plan(&doc, "blur", &json!({"sigma": sigma})).unwrap()["scratch_bytes"]
            .as_u64()
            .unwrap()
    };
    assert!(scratch(1.0) <= scratch(3.0));
}

#[test]
fn an_identity_request_is_no_change_with_no_error() {
    let (doc, _) = document(&hdr_picture(true));
    for (op, params) in [
        ("unsharp_mask", json!({"radius": 2, "amount": 0})),
        ("unsharp_mask", json!({"radius": 0, "amount": 2})),
        ("high_pass", json!({"radius": 0})),
        ("offset", json!({})),
        ("lens_correction", json!({})),
    ] {
        assert!(filter(&doc, op, &params, &[]).unwrap().is_none(), "{op}");
        assert!(plan(&doc, op, &params).is_ok(), "{op} plan");
    }
}

#[test]
fn a_cancelled_control_stops_the_filter_and_a_live_one_changes_nothing() {
    let (doc, _) = document(&hdr_picture(true));
    for (op, params) in requests() {
        let plain = filtered(&doc, op, &params);
        let control = unsafe { dr_job_control_create() };
        let controlled = run(&doc, op, &params, &[], control).unwrap().unwrap();
        assert_eq!(
            bits(&samples(&controlled)),
            bits(&plain),
            "{op}: controlled"
        );
        unsafe { dr_job_control_free(control) };
        let control = unsafe { dr_job_control_create() };
        unsafe { dr_job_control_cancel(control) };
        assert_eq!(
            run(&doc, op, &params, &[], control).err().as_deref(),
            Some("operation cancelled"),
            "{op}"
        );
        unsafe { dr_job_control_free(control) };
    }
}
