//! Lensfun lens profiles (v1-lensfun): `dr_lens_exif_identity`,
//! `dr_lens_match` and `lens_correction`'s `profile` parameter behind
//! `dr_image_filter_op` (8 bits) and `dr_precision_doc_selected_edit` (16 and
//! 32 bits).
//!
//! The geometry oracle is Lensfun's published model formulas, written here:
//! `r_d = r_u(1 − k1 + k1 r_u²)` (poly3), `r_u(1 + k1 r_u² + k2 r_u⁴)`
//! (poly5), `r_u(a r_u³ + b r_u² + c r_u + 1 − a − b − c)` (ptlens), TCA on
//! the distorted radius (`r·k`, `r(b r² + c r + v)`), in PanoTools units
//! (half the shorter side of the calibration frame) reached through the crop
//! factors and aspects. Over a linear opaque ramp Catmull-Rom reproduces the
//! ramp exactly wherever all 16 taps are inside, so an interior pixel is the
//! ramp at its source point (±1 level). The vignetting oracle divides out the
//! `pa` falloff `1 + k1 r² + k2 r⁴ + k3 r⁶` (corner of the calibration frame
//! at 1) through an independent transfer function: IEC 61966-2-1 at 8 bits,
//! the profiles' pure gammas (563/256, 461/256) at 16, none at 32. The
//! interpolation oracles hold the v0.3.4 rows they use.
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_filters::{dr_image_filter_op, dr_image_filter_op_scratch_bytes};
use doctored_core::ffi_lens_profile::{dr_lens_exif_identity, dr_lens_match};
use doctored_core::ffi_selected_adjustment::{
    dr_precision_doc_selected_edit, dr_precision_doc_selected_edit_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::icc_transform::PrecisionToneEncoder;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::DrImage;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use crate::float_fixture::float_doc;

const DATABASE: &str = include_str!("../data/lensfun/lensfun-db.json");

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

fn lens_match(request: &Value) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_lens_match(text.as_ptr(), &mut err) };
    if out.is_null() {
        return Err(take(err));
    }
    assert!(err.is_null());
    Ok(serde_json::from_str(&take(out)).unwrap())
}

fn image(w: u32, h: u32, pixels: &[u8]) -> *mut DrImage {
    let p = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!p.is_null());
    p
}

/// `Ok(None)` is the exact bypass (NULL with no error).
fn filter8(w: u32, h: u32, pixels: &[u8], params: &Value) -> Result<Option<Vec<u8>>, String> {
    let img = image(w, h, pixels);
    let op = CString::new("lens_correction").unwrap();
    let text = CString::new(params.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_filter_op(img, op.as_ptr(), text.as_ptr(), ptr::null(), &mut err) };
    let result = if out.is_null() {
        let message = take(err);
        if message.is_empty() {
            Ok(None)
        } else {
            Err(message)
        }
    } else {
        let n = (w * h * 4) as usize;
        let v = unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), n) }.to_vec();
        unsafe { dr_image_free(out) };
        Ok(Some(v))
    };
    unsafe { dr_image_free(img) };
    result
}

fn filtered8(w: u32, h: u32, pixels: &[u8], params: &Value) -> Vec<u8> {
    filter8(w, h, pixels, params)
        .unwrap_or_else(|e| panic!("{params}: {e}"))
        .unwrap_or_else(|| panic!("{params}: the bypass"))
}

fn scratch8(w: u32, h: u32, params: &Value) -> Result<u64, String> {
    let img = image(w, h, &vec![128; (w * h * 4) as usize]);
    let op = CString::new("lens_correction").unwrap();
    let text = CString::new(params.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let bytes =
        unsafe { dr_image_filter_op_scratch_bytes(img, op.as_ptr(), text.as_ptr(), &mut err) };
    unsafe { dr_image_free(img) };
    let message = take(err);
    if message.is_empty() {
        Ok(bytes)
    } else {
        Err(message)
    }
}

fn request(params: &Value) -> CString {
    CString::new(json!({"layer": 0, "op": "lens_correction", "params": params}).to_string())
        .unwrap()
}

fn selected(doc: &PrecisionDocument, params: &Value) -> PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            doc,
            request(params).as_ptr(),
            ptr::null(),
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let error = take(err);
    assert!(!out.is_null(), "{params}: {error}");
    *unsafe { Box::from_raw(out) }
}

fn plan(doc: &PrecisionDocument, params: &Value) -> u64 {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit_plan(
            doc,
            request(params).as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            &mut err,
        )
    };
    assert!(!out.is_null(), "{params}: {}", take(err));
    let v: Value = serde_json::from_str(&take(out)).unwrap();
    v["scratch_bytes"].as_u64().unwrap()
}

fn doc16(w: u32, h: u32, value: u16, profile: Arc<IccProfile>) -> PrecisionDocument {
    let pixels = vec![[value, value, value, 65535]; (w * h) as usize];
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, profile, None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

// ---- the oracle ---------------------------------------------------------

/// A profile as the header states it, read back from its JSON.
struct Oracle {
    ci: f64,
    cc: f64,
    ac: f64,
    center: [f64; 2],
    distortion: Option<Value>,
    tca: Option<Value>,
    pa: Option<[f64; 3]>,
}

impl Oracle {
    fn of(profile: &Value) -> Self {
        let f = |k: &str| profile[k].as_f64().unwrap();
        let model = |k: &str| Some(profile[k].clone()).filter(|v| !v.is_null());
        Oracle {
            ci: f("crop_factor"),
            cc: f("calibration_crop_factor"),
            ac: f("aspect_ratio"),
            center: profile
                .get("center")
                .and_then(Value::as_array)
                .map_or([0.0, 0.0], |a| {
                    [a[0].as_f64().unwrap(), a[1].as_f64().unwrap()]
                }),
            distortion: model("distortion"),
            tca: model("tca"),
            pa: model("vignetting").map(|v| {
                [
                    v["k1"].as_f64().unwrap(),
                    v["k2"].as_f64().unwrap(),
                    v["k3"].as_f64().unwrap(),
                ]
            }),
        }
    }

    /// The optical centre, half the plane's shorter side, and plane → calibration units.
    fn frame(&self, w: u32, h: u32) -> ([f64; 2], f64, f64) {
        let (w, h) = (f64::from(w), f64::from(h));
        let half = w.min(h) / 2.0;
        let ai = w.max(h) / w.min(h);
        let c = [
            w / 2.0 + self.center[0] * half,
            h / 2.0 + self.center[1] * half,
        ];
        let scale = self.cc / self.ci * (1.0 + self.ac * self.ac).sqrt() / (1.0 + ai * ai).sqrt();
        (c, half, scale)
    }

    /// `r_d(r_u)` and its derivative, from the published polynomials.
    fn distort(&self, r: f64) -> (f64, f64) {
        let Some(d) = &self.distortion else {
            return (r, 1.0);
        };
        let t = |k: &str| d[k].as_f64().unwrap();
        match d["model"].as_str().unwrap() {
            "poly3" => {
                let k1 = t("k1");
                (r * (1.0 - k1 + k1 * r * r), 1.0 - k1 + 3.0 * k1 * r * r)
            }
            "poly5" => {
                let (k1, k2) = (t("k1"), t("k2"));
                (
                    r * (1.0 + k1 * r.powi(2) + k2 * r.powi(4)),
                    1.0 + 3.0 * k1 * r.powi(2) + 5.0 * k2 * r.powi(4),
                )
            }
            "ptlens" => {
                let (a, b, c) = (t("a"), t("b"), t("c"));
                (
                    r * (a * r.powi(3) + b * r.powi(2) + c * r + 1.0 - a - b - c),
                    4.0 * a * r.powi(3) + 3.0 * b * r.powi(2) + 2.0 * c * r + 1.0 - a - b - c,
                )
            }
            other => panic!("model {other}"),
        }
    }

    fn lateral(&self, channel: usize, rd: f64) -> f64 {
        let Some(t) = &self.tca else {
            return 1.0;
        };
        let k = |name: &str| t[name].as_f64().unwrap_or(0.0);
        match (t["model"].as_str().unwrap(), channel) {
            ("linear", 0) => k("kr"),
            ("linear", 2) => k("kb"),
            ("poly3", 0) => k("br") * rd * rd + k("cr") * rd + k("vr"),
            ("poly3", 2) => k("bb") * rd * rd + k("cb") * rd + k("vb"),
            _ => 1.0,
        }
    }

    /// The source point of pixel centre `(x, y)` for `channel`, or None past
    /// the fold.
    fn source(&self, w: u32, h: u32, x: u32, y: u32, channel: usize) -> Option<(f64, f64)> {
        let (c, half, scale) = self.frame(w, h);
        let q = [f64::from(x) + 0.5 - c[0], f64::from(y) + 0.5 - c[1]];
        let r = q[0].hypot(q[1]) / half * scale;
        let (rd, slope) = self.distort(r);
        if slope <= 0.0 {
            return None;
        }
        let factor = if r == 0.0 { 1.0 } else { rd / r } * self.lateral(channel, rd);
        Some((c[0] + q[0] * factor, c[1] + q[1] * factor))
    }

    /// The correction in stops at source point `s`.
    fn stops(&self, w: u32, h: u32, s: (f64, f64)) -> f64 {
        let Some(k) = self.pa else {
            return 0.0;
        };
        let (c, half, scale) = self.frame(w, h);
        let r = (s.0 - c[0]).hypot(s.1 - c[1]) / half * scale / (1.0 + self.ac * self.ac).sqrt();
        let v = 1.0 + k[0] * r.powi(2) + k[1] * r.powi(4) + k[2] * r.powi(6);
        -v.max(1.0 / 16.0).log2()
    }
}

fn profile(ci: f64, cc: f64, ac: f64, models: Value) -> Value {
    let mut p = json!({
        "id": "test lens", "crop_factor": ci, "calibration_crop_factor": cc,
        "aspect_ratio": ac, "center": [0.0, 0.0], "focal_length": 35.0,
        "f_number": null, "subject_distance": null,
        "distortion": null, "tca": null, "vignetting": null,
    });
    for (k, v) in models.as_object().unwrap() {
        p[k] = v.clone();
    }
    p
}

/// The ramp's three channels at pixel index `(x, y)`.
fn ramp_at(x: f64, y: f64) -> [f64; 3] {
    [40.0 + 2.0 * x, 30.0 + 2.0 * y, 20.0 + x + y]
}

fn ramp(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let v = ramp_at(f64::from(i % w), f64::from(i / w));
            [v[0] as u8, v[1] as u8, v[2] as u8, 255]
        })
        .collect()
}

fn interior(s: f64, size: u32) -> bool {
    s >= 2.0 && s <= f64::from(size) - 3.0
}

/// Every pixel whose taps are all inside is the ramp at its channel's source
/// point; returns how many were checked.
fn ramp_checked(w: u32, h: u32, profile: &Value) -> usize {
    let oracle = Oracle::of(profile);
    let out = filtered8(w, h, &ramp(w, h), &json!({ "profile": profile }));
    let mut checked = 0;
    for y in 0..h {
        for x in 0..w {
            let sources: Vec<_> = (0..3).map(|c| oracle.source(w, h, x, y, c)).collect();
            if !sources
                .iter()
                .all(|s| s.is_some_and(|(sx, sy)| interior(sx, w) && interior(sy, h)))
            {
                continue;
            }
            let i = ((y * w + x) * 4) as usize;
            for c in 0..3 {
                let (sx, sy) = sources[c].unwrap();
                let want = ramp_at(sx - 0.5, sy - 0.5)[c];
                let got = f64::from(out[i + c]);
                assert!(
                    (got - want).abs() <= 1.0,
                    "{profile} {x},{y} {c}: {got} vs {want}"
                );
            }
            assert_eq!(out[i + 3], 255);
            checked += 1;
        }
    }
    checked
}

// ---- the models ---------------------------------------------------------

#[test]
fn every_distortion_model_reads_its_published_source_point() {
    let (w, h) = (96_u32, 80_u32);
    for (ci, cc, ac) in [
        (1.0, 1.0, 1.5),
        (1.5, 1.0, 1.5),
        (1.0, 1.53, 4.0 / 3.0),
        (2.0, 1.0, 1.0),
    ] {
        for model in [
            json!({"model": "poly3", "k1": -0.05}),
            json!({"model": "poly3", "k1": 0.04}),
            json!({"model": "poly5", "k1": -0.03, "k2": 0.012}),
            json!({"model": "ptlens", "a": 0.02, "b": -0.06, "c": 0.03}),
        ] {
            let p = profile(ci, cc, ac, json!({ "distortion": model }));
            let checked = ramp_checked(w, h, &p);
            assert!(checked > 3000, "{p}: only {checked}");
        }
    }
}

#[test]
fn tca_scales_red_and_blue_on_the_distorted_radius_and_leaves_green() {
    let (w, h) = (96_u32, 80_u32);
    for models in [
        json!({"tca": {"model": "linear", "kr": 1.02, "kb": 0.985}}),
        json!({"tca": {"model": "poly3", "vr": 1.01, "cr": 0.004, "br": -0.003,
            "vb": 0.99, "cb": -0.002, "bb": 0.005}}),
        json!({"distortion": {"model": "ptlens", "a": 0.01, "b": -0.03, "c": 0.02},
            "tca": {"model": "poly3", "vr": 1.02, "cr": 0.0, "br": 0.0,
            "vb": 0.98, "cb": 0.0, "bb": 0.0}}),
    ] {
        let p = profile(1.0, 1.0, 1.5, models.clone());
        assert!(ramp_checked(w, h, &p) > 3000, "{p}");
        if models.get("distortion").is_none() {
            let src = ramp(w, h);
            let out = filtered8(w, h, &src, &json!({ "profile": p }));
            for i in (0..(w * h) as usize).map(|i| i * 4) {
                assert_eq!(out[i + 1], src[i + 1], "{p}: green moved");
            }
        }
    }
}

#[test]
fn crop_and_aspect_scale_the_radius() {
    // The same lens on a body with the same crop as its calibration is the
    // same correction whatever that crop is.
    let (w, h) = (60_u32, 40_u32);
    let src = ramp(w, h);
    let distortion = json!({"model": "poly5", "k1": -0.08, "k2": 0.0});
    let a = filtered8(
        w,
        h,
        &src,
        &json!({"profile": profile(1.0, 1.0, 1.5, json!({"distortion": distortion}))}),
    );
    let b = filtered8(
        w,
        h,
        &src,
        &json!({"profile": profile(2.5, 2.5, 1.5, json!({"distortion": distortion}))}),
    );
    assert_eq!(a, b);
    // A smaller sensor (larger crop) sees a smaller radius, so a poly5
    // correction (no linear term) moves the picture less.
    let c = filtered8(
        w,
        h,
        &src,
        &json!({"profile": profile(3.0, 1.0, 1.5, json!({"distortion": distortion}))}),
    );
    let moved = |out: &[u8]| {
        out.iter()
            .zip(&src)
            .map(|(o, s)| u64::from(o.abs_diff(*s)))
            .sum::<u64>()
    };
    assert!(moved(&c) < moved(&a), "{} vs {}", moved(&c), moved(&a));
}

#[test]
fn a_point_past_the_fold_is_transparent() {
    let (w, h) = (90_u32, 60_u32);
    let p = profile(
        1.0,
        3.0,
        1.5,
        json!({"distortion": {"model": "poly3", "k1": -1.0}}),
    );
    let oracle = Oracle::of(&p);
    let out = filtered8(w, h, &ramp(w, h), &json!({ "profile": p }));
    let mut folded = 0;
    for y in 0..h {
        for x in 0..w {
            if oracle.source(w, h, x, y, 1).is_none() {
                assert_eq!(out[((y * w + x) * 4 + 3) as usize], 0, "{x},{y}");
                folded += 1;
            }
        }
    }
    assert!(folded > 100, "{folded}");
    assert_eq!(out[3], 0, "the corner is past the fold");
}

#[test]
fn a_neutral_profile_is_the_exact_bypass() {
    let (w, h) = (30_u32, 20_u32);
    let src = ramp(w, h);
    let neutral = profile(
        1.5,
        1.0,
        1.5,
        json!({"distortion": {"model": "poly3", "k1": 0.0},
            "tca": {"model": "linear", "kr": 1.0, "kb": 1.0},
            "vignetting": {"model": "pa", "k1": 0.0, "k2": 0.0, "k3": 0.0}}),
    );
    assert_eq!(
        filter8(w, h, &src, &json!({ "profile": neutral })),
        Ok(None)
    );
    assert_eq!(scratch8(w, h, &json!({ "profile": neutral })), Ok(0));
    let manual = json!({"distortion": 30, "vignette_amount": -20});
    let mut with = manual.clone();
    with["profile"] = neutral;
    assert_eq!(filtered8(w, h, &src, &manual), filtered8(w, h, &src, &with));
}

#[test]
fn bad_profiles_are_refused_by_name() {
    let good = profile(
        1.0,
        1.0,
        1.5,
        json!({"distortion": {"model": "poly3", "k1": 0.01}}),
    );
    let with = |key: &str, v: Value| {
        let mut p = good.clone();
        p[key] = v;
        p
    };
    let without = |key: &str| {
        let mut p = good.clone();
        p.as_object_mut().unwrap().remove(key);
        p
    };
    for (p, wanted) in [
        (
            with("lens", json!("x")),
            "lens profile has unknown key lens",
        ),
        (without("id"), "lens profile id must be a string"),
        (
            with("crop_factor", json!(0.1)),
            "lens profile crop_factor must be in (0.1, 20]",
        ),
        (
            with("calibration_crop_factor", json!(21)),
            "lens profile calibration_crop_factor must be in (0.1, 20]",
        ),
        (
            with("aspect_ratio", json!(0.9)),
            "lens profile aspect_ratio must be in [1, 4]",
        ),
        (
            with("center", json!([0.9, 0.9])),
            "lens profile center must be within 1 of the middle",
        ),
        (
            with("distortion", json!({"model": "poly3", "k1": 1.5})),
            "lens profile distortion k1 must be in [-1, 1]",
        ),
        (
            with("distortion", json!({"model": "poly7", "k1": 0.1})),
            "lens profile distortion model poly7 is not poly3, poly5 or ptlens",
        ),
        (
            with(
                "distortion",
                json!({"model": "poly3", "k1": 0.1, "k2": 0.0}),
            ),
            "lens profile distortion has unknown key k2",
        ),
        (
            with("tca", json!({"model": "linear", "kr": 1.2, "kb": 1.0})),
            "lens profile tca kr must be in [0.9, 1.1]",
        ),
        (
            with(
                "tca",
                json!({"model": "poly3", "vr": 1.0, "cr": 0.2, "br": 0.0, "vb": 1.0, "cb": 0.0, "bb": 0.0}),
            ),
            "lens profile tca cr must be in [-0.1, 0.1]",
        ),
        (
            with(
                "vignetting",
                json!({"model": "pa", "k1": 6.0, "k2": 0.0, "k3": 0.0}),
            ),
            "lens profile vignetting k1 must be in [-5, 5]",
        ),
        (
            with("distortion", Value::Null),
            "lens profile corrects nothing",
        ),
        (json!("profile"), "lens profile must be an object"),
    ] {
        assert_eq!(
            filter8(8, 6, &[128; 192], &json!({ "profile": p })),
            Err(wanted.into()),
            "{p}"
        );
    }
}

// ---- vignetting ---------------------------------------------------------

fn srgb_decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn srgb_encode(u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    if u <= 0.003_130_8 {
        12.92 * u
    } else {
        1.055 * u.powf(1.0 / 2.4) - 0.055
    }
}

fn pa_profile() -> Value {
    profile(
        1.2,
        1.0,
        1.5,
        json!({"vignetting": {"model": "pa", "k1": -0.35, "k2": 0.06, "k3": -0.02}}),
    )
}

#[test]
fn pa_divides_out_the_falloff_at_8_bits_in_srgb_light() {
    let (w, h) = (41_u32, 31_u32);
    let p = pa_profile();
    let oracle = Oracle::of(&p);
    let flat: Vec<u8> = (0..w * h).flat_map(|_| [100, 100, 100, 255]).collect();
    let out = filtered8(w, h, &flat, &json!({ "profile": p }));
    for y in 0..h {
        for x in 0..w {
            let s = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            let want =
                srgb_encode(srgb_decode(100.0 / 255.0) * oracle.stops(w, h, s).exp2()) * 255.0;
            let i = ((y * w + x) * 4) as usize;
            for c in 0..3 {
                let got = f64::from(out[i + c]);
                assert!((got - want).abs() <= 1.0, "{x},{y}: {got} vs {want}");
            }
        }
    }
    assert!(
        out[0] > out[((h / 2 * w + w / 2) * 4) as usize],
        "the corner is lifted"
    );
}

#[test]
fn pa_multiplies_linear_light_on_a_float_layer() {
    let (w, h) = (41_u32, 31_u32);
    let colour = [0.18_f32, 1.5, -0.2];
    let doc = float_doc(
        w,
        h,
        &IccProfile::srgb(),
        &[vec![
            [colour[0], colour[1], colour[2], 1.0];
            (w * h) as usize
        ]],
    );
    let p = pa_profile();
    let oracle = Oracle::of(&p);
    let out = selected(&doc, &json!({ "profile": p }))
        .layer_source(0)
        .unwrap()
        .to_rgba32()
        .unwrap();
    for (i, got) in out.iter().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let gain = oracle
            .stops(w, h, (f64::from(x) + 0.5, f64::from(y) + 0.5))
            .exp2();
        for c in 0..3 {
            let want = f64::from(colour[c]) * gain;
            let got = f64::from(got[c]);
            assert!(
                (got - want).abs() <= 3e-6 * want.abs().max(1.0),
                "{x},{y} {c}: {got} vs {want}"
            );
        }
    }
}

#[test]
fn pa_at_16_bits_is_applied_in_the_documents_own_gamma() {
    let (w, h) = (41_u32, 31_u32);
    let value = 30000_u16;
    let p = pa_profile();
    let oracle = Oracle::of(&p);
    for (profile, gamma) in [
        (IccProfile::adobe_rgb(), 563.0 / 256.0),
        (IccProfile::prophoto_rgb(), 461.0 / 256.0),
    ] {
        let doc = doc16(w, h, value, profile);
        let out = selected(&doc, &json!({ "profile": p }))
            .layer_source(0)
            .unwrap()
            .to_rgba16()
            .unwrap();
        let linear = (f64::from(value) / 65535.0).powf(gamma);
        for (i, got) in out.iter().enumerate() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            let gain = oracle
                .stops(w, h, (f64::from(x) + 0.5, f64::from(y) + 0.5))
                .exp2();
            let want = (linear * gain).clamp(0.0, 1.0).powf(1.0 / gamma) * 65535.0;
            for c in 0..3 {
                let got = f64::from(got[c]);
                assert!(
                    (got - want).abs() <= 2.0,
                    "γ {gamma} {x},{y}: {got} vs {want}"
                );
            }
            assert_eq!(got[3], 65535);
        }
        // The plan holds the encoder's tables beside the plane: on this small
        // plate the slider's vignette plans the same plane without them.
        let manual = plan(&doc, &json!({"vignette_amount": -20}));
        assert_eq!(
            plan(&doc, &json!({ "profile": p })),
            manual + PrecisionToneEncoder::scratch_bytes(),
            "γ {gamma}"
        );
    }
}

// ---- the database -------------------------------------------------------

fn z6(lens: &str, focal: f64, f_number: Option<f64>) -> Value {
    json!({"make": "NIKON CORPORATION", "model": "NIKON Z 6", "lens_model": lens,
        "focal_length": focal, "f_number": f_number})
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * b.abs().max(1.0)
}

#[test]
fn a_zoom_interpolates_linearly_in_focal_length() {
    // NIKKOR Z 24-70mm f/4 S, v0.3.4 rows: ptlens and poly3 TCA at 28 and
    // 35 mm, pa at f/4 at 28 and 34.5 mm (distance 10 and 1000 alike).
    let answer =
        lens_match(&json!({"identity": z6("NIKKOR Z 24-70mm f/4 S", 30.0, Some(4.0))})).unwrap();
    assert_eq!(
        answer["selected"]["id"], "NIKKOR Z 24-70mm f/4 S",
        "{answer}"
    );
    assert_eq!(answer["crop_factor_source"], "camera");
    let p = &answer["profile"];
    let t = 2.0 / 7.0;
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let d = &p["distortion"];
    assert_eq!(d["model"], "ptlens");
    assert!(
        close(d["a"].as_f64().unwrap(), lerp(0.022, 0.019, t)),
        "{d}"
    );
    assert!(
        close(d["b"].as_f64().unwrap(), lerp(-0.077, -0.056, t)),
        "{d}"
    );
    assert!(
        close(d["c"].as_f64().unwrap(), lerp(0.061, 0.063, t)),
        "{d}"
    );
    let tca = &p["tca"];
    assert!(
        close(tca["vr"].as_f64().unwrap(), lerp(1.0001203, 1.0000506, t)),
        "{tca}"
    );
    assert!(
        close(tca["vb"].as_f64().unwrap(), lerp(1.000131, 1.0001152, t)),
        "{tca}"
    );
    let v = &p["vignetting"];
    let tv = 2.0 / 6.5;
    for (key, lo, hi) in [
        ("k1", -0.3718631, -0.3397245),
        ("k2", -0.9318504, -0.8805424),
        ("k3", 0.6048719, 0.601324),
    ] {
        assert!(
            close(v[key].as_f64().unwrap(), lerp(lo, hi, tv)),
            "{key}: {v}"
        );
    }
    assert_eq!(p["crop_factor"], 1.0);
    assert_eq!(p["focal_length"], 30.0);
    // A zoom with no focal length has no profile.
    let mut identity = z6("NIKKOR Z 24-70mm f/4 S", 30.0, Some(4.0));
    identity["focal_length"] = Value::Null;
    let answer = lens_match(&json!({ "identity": identity })).unwrap();
    assert!(answer["profile"].is_null());
    assert_eq!(answer["reason"], "focal length unknown");
}

#[test]
fn pa_weights_aperture_and_distance_by_inverse_distance() {
    // Sigma 35mm f/1.4 DG HSM, v0.3.4 pa rows at 35 mm: (N, d, k1, k2, k3).
    let rows = [
        (1.4, 0.3, -1.2015, 0.9026, -0.3583),
        (1.4, 1000.0, -1.8858, 1.9295, -0.8556),
        (2.0, 0.3, -0.1579, -0.3705, 0.0878),
        (2.0, 1000.0, -0.227, -1.0931, 0.6582),
        (2.8, 0.3, -0.2655, 0.0048, 0.0296),
        (2.8, 1000.0, -0.2521, -0.1862, -0.0193),
        (4.0, 0.3, -0.2681, 0.0047, 0.0334),
        (4.0, 1000.0, -0.3937, 0.2511, -0.2028),
        (5.6, 0.3, -0.2729, 0.016, 0.0253),
        (5.6, 1000.0, -0.3411, 0.0175, 0.0299),
        (8.0, 0.3, -0.2754, 0.0196, 0.0226),
        (8.0, 1000.0, -0.3436, 0.0164, 0.0327),
        (11.0, 0.3, -0.2803, 0.0277, 0.0162),
        (11.0, 1000.0, -0.353, 0.0292, 0.0271),
        (16.0, 0.3, -0.2898, 0.0395, 0.0103),
        (16.0, 1000.0, -0.367, 0.0498, 0.0171),
    ];
    let d850 = |n: f64, d: Option<f64>| {
        json!({"identity": {"make": "NIKON CORPORATION", "model": "NIKON D850",
            "lens_model": "Sigma 35mm F1.4 DG HSM", "focal_length": 35.0,
            "f_number": n, "subject_distance": d}})
    };
    for (n, d) in [(2.4, Some(2.0)), (3.2, Some(0.5)), (6.0, None)] {
        let answer = lens_match(&d850(n, d)).unwrap();
        assert_eq!(
            answer["selected"]["id"], "Sigma 35mm f/1.4 DG HSM",
            "{answer}"
        );
        let distance = d.unwrap_or(1000.0);
        let (mut sum, mut weights) = ([0.0; 3], 0.0);
        for (rn, rd, k1, k2, k3) in rows {
            let w = 1.0 / ((1.0 / rn - 1.0 / n).powi(2) + (1.0 / rd - 1.0 / distance).powi(2));
            weights += w;
            for (s, k) in sum.iter_mut().zip([k1, k2, k3]) {
                *s += w * k;
            }
        }
        let v = &answer["profile"]["vignetting"];
        for (i, key) in ["k1", "k2", "k3"].into_iter().enumerate() {
            assert!(
                close(v[key].as_f64().unwrap(), sum[i] / weights),
                "{n} {d:?} {key}: {v}"
            );
        }
    }
    // An exact hit takes the row; no f-number corrects no vignetting.
    let answer = lens_match(&d850(4.0, Some(0.3))).unwrap();
    assert_eq!(answer["profile"]["vignetting"]["k1"], -0.2681);
    let mut request = d850(4.0, None);
    request["identity"]["f_number"] = Value::Null;
    let answer = lens_match(&request).unwrap();
    assert!(answer["profile"]["vignetting"].is_null());
    assert!(!answer["profile"]["distortion"].is_null());
    assert_eq!(
        answer["reason"],
        "f-number unknown, so vignetting is not corrected"
    );
}

/// A little-endian EXIF stream: IFD0 (Make, Model, Exif IFD), then the Exif
/// IFD, then the out-of-line data.
fn exif() -> Vec<u8> {
    enum V {
        Ascii(&'static str),
        Rational(u32, u32),
        Short(u16),
        Long(u32),
        Pointer,
    }
    let ifd0 = [
        (0x010F_u16, V::Ascii("NIKON CORPORATION")),
        (0x0110, V::Ascii("NIKON Z 6")),
        (0x8769, V::Pointer),
    ];
    let exif = [
        (0x829D_u16, V::Rational(56, 10)),
        (0x9206, V::Rational(25, 10)),
        (0x920A, V::Rational(300, 10)),
        (0xA002, V::Long(6048)),
        (0xA003, V::Short(4024)),
        (0xA405, V::Short(30)),
        (0xA433, V::Ascii("Nikon")),
        (0xA434, V::Ascii("NIKKOR Z 24-70mm f/4 S")),
    ];
    let ifd_size = |n: usize| 2 + 12 * n + 4;
    let ifd0_at = 8;
    let exif_at = ifd0_at + ifd_size(ifd0.len());
    let mut data_at = exif_at + ifd_size(exif.len());
    let mut out = b"II*\0".to_vec();
    out.extend((ifd0_at as u32).to_le_bytes());
    let mut data = Vec::new();
    let mut write = |entries: &[(u16, V)], out: &mut Vec<u8>, data: &mut Vec<u8>| {
        out.extend((entries.len() as u16).to_le_bytes());
        for (tag, v) in entries {
            out.extend(tag.to_le_bytes());
            let (kind, count, field): (u16, u32, [u8; 4]) = match v {
                V::Ascii(s) => {
                    let bytes = [s.as_bytes(), b"\0"].concat();
                    let field = if bytes.len() <= 4 {
                        let mut f = [0; 4];
                        f[..bytes.len()].copy_from_slice(&bytes);
                        f
                    } else {
                        let f = (data_at as u32).to_le_bytes();
                        data_at += bytes.len();
                        data.extend(&bytes);
                        f
                    };
                    (2, bytes.len() as u32, field)
                }
                V::Rational(n, d) => {
                    let f = (data_at as u32).to_le_bytes();
                    data_at += 8;
                    data.extend(n.to_le_bytes());
                    data.extend(d.to_le_bytes());
                    (5, 1, f)
                }
                V::Short(s) => {
                    let mut f = [0; 4];
                    f[..2].copy_from_slice(&s.to_le_bytes());
                    (3, 1, f)
                }
                V::Long(l) => (4, 1, l.to_le_bytes()),
                V::Pointer => (4, 1, (exif_at as u32).to_le_bytes()),
            };
            out.extend(kind.to_le_bytes());
            out.extend(count.to_le_bytes());
            out.extend(field);
        }
        out.extend(0_u32.to_le_bytes());
    };
    write(&ifd0, &mut out, &mut data);
    write(&exif, &mut out, &mut data);
    out.extend(data);
    out
}

fn identity_of(bytes: &[u8]) -> Option<Value> {
    let out = unsafe { dr_lens_exif_identity(bytes.as_ptr(), bytes.len()) };
    (!out.is_null()).then(|| serde_json::from_str(&take(out)).unwrap())
}

#[test]
fn the_exif_identity_reads_the_named_fields() {
    let identity = identity_of(&exif()).unwrap();
    assert_eq!(
        identity,
        json!({"make": "NIKON CORPORATION", "model": "NIKON Z 6", "lens_make": "Nikon",
            "lens_model": "NIKKOR Z 24-70mm f/4 S", "focal_length": 30.0, "f_number": 5.6,
            "subject_distance": 2.5, "focal_length_35mm": 30.0, "pixel_width": 6048,
            "pixel_height": 4024})
    );
    // It is what dr_lens_match reads.
    let answer = lens_match(&json!({ "identity": identity })).unwrap();
    assert_eq!(answer["selected"]["id"], "NIKKOR Z 24-70mm f/4 S");
    assert!(answer["reason"].is_null(), "{answer}");
    // An unreadable packet: every field null.
    let garbage = identity_of(b"not a tiff stream").unwrap();
    assert!(
        garbage.as_object().unwrap().values().all(Value::is_null),
        "{garbage}"
    );
    assert_eq!(garbage.as_object().unwrap().len(), 10);
}

fn selected_id(make: &str, model: &str, lens: &str, focal: f64) -> Value {
    let answer = lens_match(&json!({"identity": {"make": make, "model": model,
        "lens_model": lens, "focal_length": focal, "f_number": 5.6}}))
    .unwrap();
    assert!(!answer["profile"].is_null(), "{answer}");
    answer
}

#[test]
fn the_fixture_cameras_match_their_lenses() {
    for (make, model, lens, focal, want) in [
        (
            "NIKON CORPORATION",
            "NIKON Z 6",
            "NIKKOR Z 24-70mm f/4 S",
            24.0,
            "NIKKOR Z 24-70mm f/4 S",
        ),
        (
            "NIKON CORPORATION",
            "NIKON D850",
            "Sigma 35mm F1.4 DG HSM",
            35.0,
            "Sigma 35mm f/1.4 DG HSM",
        ),
        (
            "NIKON CORPORATION",
            "NIKON D70s",
            "Tamron AF 18-250mm f/3.5-6.3 Di II LD Aspherical (IF) Macro (A18)",
            50.0,
            "Tamron AF 18-250mm f/3.5-6.3 Di II LD Aspherical (IF) Macro",
        ),
        (
            "SONY",
            "ILCE-7M3",
            "Sony FE 16-35mm F2.8",
            20.0,
            "FE 16-35mm f/2.8 GM",
        ),
        (
            "Canon",
            "Canon EOS R5",
            "RF24-105mm F4-7.1 IS STM",
            50.0,
            "Canon RF 24-105mm F4-7.1 IS STM",
        ),
    ] {
        let answer = selected_id(make, model, lens, focal);
        assert_eq!(answer["selected"]["id"], want, "{model}: {answer}");
        assert_eq!(
            answer["camera"]["model"].as_str().unwrap().to_lowercase(),
            model.to_lowercase()
        );
        assert!(
            answer["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["id"] != "Canon RF 24-105mm F4L IS USM"),
            "{model}: {answer}"
        );
    }
}

#[test]
fn no_match_answers_carry_a_reason() {
    // An unknown camera with no 35 mm focal length: the crop is unknown.
    let answer = lens_match(&json!({"identity": {"make": "Acme", "model": "X1",
        "lens_model": "NIKKOR Z 24-70mm f/4 S", "focal_length": 30.0}}))
    .unwrap();
    assert!(
        answer["camera"].is_null() && answer["profile"].is_null(),
        "{answer}"
    );
    assert_eq!(answer["reason"], "crop factor unknown");
    // The same with the 35 mm focal length resolves.
    let answer = lens_match(&json!({"identity": {"make": "Acme", "model": "X1",
        "lens_model": "NIKKOR Z 24-70mm f/4 S", "focal_length": 30.0, "focal_length_35mm": 45}}))
    .unwrap();
    assert_eq!(answer["crop_factor_source"], "exif_35mm");
    assert_eq!(answer["profile"]["crop_factor"], 1.5);
    // No lens name.
    let answer = lens_match(
        &json!({"identity": {"make": "NIKON CORPORATION", "model": "NIKON Z 6",
        "focal_length": 30.0}}),
    )
    .unwrap();
    assert!(answer["profile"].is_null());
    assert_eq!(answer["reason"], "the EXIF names no lens");
    // A lens no row names.
    let answer = lens_match(&json!({"identity": z6("Nonesuch 13mm f/9", 13.0, None)})).unwrap();
    assert!(answer["profile"].is_null() && answer["selected"].is_null());
    assert_eq!(
        answer["reason"],
        "no Lensfun lens matches \"Nonesuch 13mm f/9\""
    );
}

#[test]
fn auto_refuses_a_plane_that_is_not_the_camera_frame() {
    let mut identity = z6("NIKKOR Z 24-70mm f/4 S", 30.0, Some(4.0));
    identity["pixel_width"] = json!(3456);
    identity["pixel_height"] = json!(2304);
    let answer = lens_match(&json!({"identity": identity, "dimensions": [1573, 2302]})).unwrap();
    assert!(answer["profile"].is_null(), "{answer}");
    assert_eq!(
        answer["reason"],
        "the image is not the camera frame (1573×2302 against EXIF 3456×2304); choose a lens \
         to apply one anyway"
    );
    let answer = lens_match(&json!({"identity": identity, "dimensions": [1573, 2302],
        "lens": "NIKKOR Z 24-70mm f/4 S"}))
    .unwrap();
    assert!(
        !answer["profile"].is_null() && answer["reason"].is_null(),
        "{answer}"
    );
    for dimensions in [json!([1728, 1152]), json!([2304, 3456])] {
        let answer = lens_match(&json!({"identity": identity, "dimensions": dimensions})).unwrap();
        assert!(!answer["profile"].is_null(), "{dimensions}: {answer}");
    }
}

#[test]
fn every_bundled_lens_resolves_at_each_calibrated_focal_length() {
    let db: Value = serde_json::from_str(DATABASE).unwrap();
    assert_eq!(db["database"]["license"], "CC BY-SA 3.0");
    assert_eq!(db["database"]["tag"], "v0.3.4");
    let lenses = db["lenses"].as_array().unwrap();
    assert!(lenses.len() > 1200, "{}", lenses.len());
    let mut resolved = 0;
    for lens in lenses {
        let id = lens["id"].as_str().unwrap();
        let crop = lens["crop"].as_f64().unwrap();
        let mut focals: Vec<(f64, Option<f64>)> = Vec::new();
        for key in ["poly3", "poly5", "ptlens", "tca_linear", "tca_poly3", "pa"] {
            for row in lens
                .get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let row = row.as_array().unwrap();
                let focal = row[0].as_f64().unwrap();
                let aperture = (key == "pa").then(|| row[1].as_f64().unwrap());
                if !focals.iter().any(|f| f.0 == focal) {
                    focals.push((focal, aperture));
                }
            }
        }
        assert!(!focals.is_empty(), "{id} has no rows");
        for (focal, aperture) in focals {
            let answer = lens_match(&json!({"lens": id, "identity": {"focal_length": focal,
                "focal_length_35mm": focal * crop, "f_number": aperture}}))
            .unwrap_or_else(|e| panic!("{id}: {e}"));
            let profile = &answer["profile"];
            assert!(!profile.is_null(), "{id} at {focal}: {answer}");
            scratch8(64, 48, &json!({ "profile": profile }))
                .unwrap_or_else(|e| panic!("{id} at {focal}: {e}"));
            resolved += 1;
        }
    }
    assert!(resolved > 4000, "{resolved}");
}

#[test]
fn match_requests_are_strict() {
    for (request, wanted) in [
        (
            json!({"identity": {}, "lens": "No Such Lens"}),
            "unknown lens id No Such Lens",
        ),
        (
            json!({"identity": {}, "camera": 1}),
            "lens match request has unknown key camera",
        ),
        (
            json!({"identity": {"maker": "x"}}),
            "identity has unknown key maker",
        ),
        (
            json!({"identity": {"focal_length": "30"}}),
            "identity focal_length must be a number or null",
        ),
        (
            json!({"identity": {}, "dimensions": [0, 4]}),
            "dimensions must be two positive integers",
        ),
        (
            json!({"identity": {}, "all": 1}),
            "all must be true or false",
        ),
    ] {
        assert_eq!(lens_match(&request), Err(wanted.into()), "{request}");
    }
    let bad = CString::new("{not json").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_lens_match(bad.as_ptr(), &mut err) }.is_null());
    assert!(take(err).starts_with("invalid lens match request"));
    // all:true lists the mount family's lenses; the database credit rides along.
    let answer =
        lens_match(&json!({"identity": z6("NIKKOR Z 24-70mm f/4 S", 30.0, None), "all": true}))
            .unwrap();
    let family = answer["mount_lenses"].as_array().unwrap();
    assert!(family.iter().any(|l| l["id"] == "NIKKOR Z 24-70mm f/4 S"));
    assert!(family.len() > answer["candidates"].as_array().unwrap().len());
    assert_eq!(
        answer["database"],
        json!({"name": "Lensfun", "tag": "v0.3.4", "license": "CC BY-SA 3.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/3.0/",
            "url": "https://github.com/lensfun/lensfun"})
    );
    assert!(lens_match(&json!({"identity": {}}))
        .unwrap()
        .get("mount_lenses")
        .is_none());
}

#[test]
fn null_arguments_are_tolerated() {
    assert!(unsafe { dr_lens_exif_identity(ptr::null(), 16) }.is_null());
    let bytes = exif();
    assert!(unsafe { dr_lens_exif_identity(bytes.as_ptr(), 0) }.is_null());
    assert!(unsafe { dr_lens_match(ptr::null(), ptr::null_mut()) }.is_null());
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_lens_match(ptr::null(), &mut err) }.is_null());
    assert_eq!(take(err), "request_json is NULL");
}
