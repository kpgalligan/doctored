//! Independent qualification metrics and a frozen prior-only trimap policy.
//! The original24-case alpha metric and gate are unchanged. Color diagnostics
//! use known reference foregrounds and controlled replacement backgrounds;
//! they never treat the input composite as true foreground color.
#![allow(dead_code)]
use image::{GrayImage, RgbaImage};
use serde_json::{json, Value};

pub fn alpha_metrics(truth: &GrayImage, mask: &GrayImage) -> Value {
    let n = truth.as_raw().len();
    let mut total = 0.;
    let mut squared = 0.;
    let mut boundary = 0.;
    let mut boundary_n = 0;
    let mut detail = 0;
    let mut retained = 0;
    let (w, h) = truth.dimensions();
    for y in 0..h {
        for x in 0..w {
            let t = truth.get_pixel(x, y)[0];
            let m = mask.get_pixel(x, y)[0];
            let e = f64::from(t.abs_diff(m)) / 255.;
            total += e;
            squared += e * e;
            let varying = (y.saturating_sub(2)..=(y + 2).min(h - 1)).any(|ny| {
                (x.saturating_sub(2)..=(x + 2).min(w - 1)).any(|nx| truth.get_pixel(nx, ny)[0] != t)
            });
            if varying || (t > 0 && t < 255) {
                boundary += e;
                boundary_n += 1;
            }
            if (32..224).contains(&t) {
                detail += 1;
                if m >= 16 {
                    retained += 1;
                }
            }
        }
    }
    json!({"mad":total/n as f64,"mse":squared/n as f64,"boundary_mad":boundary/boundary_n.max(1) as f64,"soft_detail_recall":if detail==0 {1.} else {retained as f64/detail as f64}})
}

/// The original predeclared24-case gate, verbatim thresholds.
pub fn alpha_gate(before: &Value, after: &Value) -> bool {
    after["boundary_mad"].as_f64().unwrap()
        <= before["boundary_mad"].as_f64().unwrap() + 1.0 / 255.0
        && after["soft_detail_recall"].as_f64().unwrap() >= 0.95
}

/// Post-hoc input audit, NEVER an estimator input. Keeping known labels and
/// substituting reference alpha only at Unknown pixels simultaneously minimizes
/// absolute alpha error and maximizes this gate's soft-detail recall. Therefore
/// failure of this counterfactual bound proves that no constraint-respecting
/// estimator can pass this case with this trimap. Success does not qualify one.
pub fn constraint_audit(truth: &GrayImage, trimap: &GrayImage, before: &Value) -> Value {
    assert_eq!(truth.dimensions(), trimap.dimensions());
    let mut counterfactual = truth.clone();
    let mut false_background = 0;
    let mut false_foreground = 0;
    let mut counts = [0_usize; 3];
    for ((value, &label), &reference) in counterfactual
        .as_mut()
        .iter_mut()
        .zip(trimap.as_raw())
        .zip(truth.as_raw())
    {
        match label {
            0 => {
                counts[0] += 1;
                false_background += usize::from(reference != 0);
                *value = 0;
            }
            128 => {
                counts[1] += 1;
            }
            255 => {
                counts[2] += 1;
                false_foreground += usize::from(reference != 255);
                *value = 255;
            }
            _ => panic!("invalid categorical trimap in qualification audit"),
        }
    }
    let bound = alpha_metrics(truth, &counterfactual);
    json!({"reference_used_only_for_post_hoc_bound":true,
        "known_background_pixels":counts[0],"unknown_pixels":counts[1],"known_foreground_pixels":counts[2],
        "incorrect_known_background_pixels":false_background,"incorrect_known_foreground_pixels":false_foreground,
        "minimum_possible_boundary_mad":bound["boundary_mad"],
        "maximum_possible_soft_detail_recall":bound["soft_detail_recall"],
        "alpha_gate_feasible_under_known_constraints":alpha_gate(before, &bound)})
}

/// Frozen before candidate measurement: prior<=8 is tentative background,
/// prior>=247 tentative foreground; erode each set with a clipped7x7 square
/// (radius3). Everything else is unknown128. No truth/source colors are read.
pub fn trimap_from_prior(prior: &GrayImage) -> GrayImage {
    let (w, h) = prior.dimensions();
    GrayImage::from_fn(w, h, |x, y| {
        let value = prior.get_pixel(x, y)[0];
        let class = if value <= 8 {
            0
        } else if value >= 247 {
            255
        } else {
            return image::Luma([128]);
        };
        for ny in y.saturating_sub(3)..=(y + 3).min(h - 1) {
            for nx in x.saturating_sub(3)..=(x + 3).min(w - 1) {
                let p = prior.get_pixel(nx, ny)[0];
                if (class == 0 && p > 8) || (class == 255 && p < 247) {
                    return image::Luma([128]);
                }
            }
        }
        image::Luma([class])
    })
}

pub fn colors(image: &RgbaImage) -> Vec<[f32; 3]> {
    image
        .pixels()
        .map(|p| {
            [
                f32::from(p[0]) / 255.0,
                f32::from(p[1]) / 255.0,
                f32::from(p[2]) / 255.0,
            ]
        })
        .collect()
}

/// White/dark/checker are independent replacement backgrounds, never the
/// observed source background. Every scene is evaluated over all three.
pub fn replacement_background(name: &str, x: u32, y: u32) -> [f64; 3] {
    match name {
        "white" => [1.0; 3],
        "dark" => [0.02; 3],
        "checker" => {
            [if (x / 12 + y / 12).is_multiple_of(2) {
                0.25
            } else {
                0.75
            }; 3]
        }
        _ => panic!("unknown replacement background"),
    }
}

/// Color error conditional on known F/alpha. Returns an error for malformed
/// dimensions/nonfinite colors instead of serializing NaN as a plausible score.
/// Foreground errors weight by truth alpha; fractional-only scores prevent
/// opaque interiors from hiding fringes. Recomposite scores couple F and alpha.
pub fn color_metrics(
    truth_alpha: &GrayImage,
    alpha: &GrayImage,
    truth: &[[f32; 3]],
    estimated: &[[f32; 3]],
    source: &RgbaImage,
) -> Result<Value, String> {
    let n = truth_alpha.as_raw().len();
    if n == 0
        || truth_alpha.dimensions() != alpha.dimensions()
        || source.dimensions() != alpha.dimensions()
        || truth.len() != n
        || estimated.len() != n
    {
        return Err("quality inputs have incompatible dimensions".into());
    }
    if truth
        .iter()
        .chain(estimated)
        .flatten()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err("quality colors must be finite normalized RGB".into());
    }
    let (w, h) = alpha.dimensions();
    let mut weighted = 0.0;
    let mut weight = 0.0;
    let mut edge = 0.0;
    let mut edge_weight = 0.0;
    let mut opaque = 0;
    let mut opaque_changed = 0;
    let mut opaque_changed_max_delta: u8 = 0;
    let mut opaque_changed_outside_boundary: usize = 0;
    let mut fractional = 0;
    let mut recomposite = [0.0; 3];
    let mut recomposite_edge = [0.0; 3];
    let mut boundary_count = 0;
    let mut leak = 0.0;
    let mut background_count = 0;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let raw = truth_alpha.as_raw()[i];
            let a = f64::from(raw) / 255.0;
            let p = f64::from(alpha.as_raw()[i]) / 255.0;
            let soft = raw > 0 && raw < 255;
            let varying = (y.saturating_sub(2)..=(y + 2).min(h - 1)).any(|ny| {
                (x.saturating_sub(2)..=(x + 2).min(w - 1))
                    .any(|nx| truth_alpha.get_pixel(nx, ny)[0] != raw)
            });
            let boundary = soft || varying;
            if boundary {
                boundary_count += 1;
            }
            if soft {
                fractional += 1;
            }
            if raw == 0 {
                background_count += 1;
                leak += p;
            }
            if raw == 255 {
                opaque += 1;
                let delta = (0..3)
                    .map(|c| {
                        ((estimated[i][c] * 255.0).round() as u8)
                            .abs_diff(source.as_raw()[i * 4 + c])
                    })
                    .max()
                    .unwrap_or(0);
                if delta > 0 {
                    opaque_changed += 1;
                    opaque_changed_max_delta = opaque_changed_max_delta.max(delta);
                    if !varying {
                        opaque_changed_outside_boundary += 1;
                    }
                }
            }
            weight += a;
            if soft {
                edge_weight += a;
            }
            for c in 0..3 {
                let f = f64::from(truth[i][c]);
                let estimate = f64::from(estimated[i][c]);
                weighted += a * (estimate - f).abs();
                if soft {
                    edge += a * (estimate - f).abs();
                }
                for (b, name) in ["white", "dark", "checker"].iter().enumerate() {
                    let background = replacement_background(name, x, y)[c];
                    let error = (p * estimate + (1.0 - p) * background
                        - (a * f + (1.0 - a) * background))
                        .abs();
                    recomposite[b] += error;
                    if boundary {
                        recomposite_edge[b] += error;
                    }
                }
            }
        }
    }
    let backgrounds:serde_json::Map<String,Value>=["white","dark","checker"].iter().enumerate().map(|(i,name)|
        (name.to_string(),json!({"mad":recomposite[i]/(3*n) as f64,"boundary_mad":recomposite_edge[i]/(3*boundary_count.max(1)) as f64}))).collect();
    Ok(
        json!({"alpha_weighted_foreground_mad":weighted/(3.0*weight.max(f64::EPSILON)),
        "fractional_foreground_mad":edge/(3.0*edge_weight.max(f64::EPSILON)),"fractional_pixels":fractional,
        "opaque_pixels":opaque,"opaque_changed_pixels":opaque_changed,
        "opaque_changed_max_code_delta":opaque_changed_max_delta,"opaque_changed_outside_boundary":opaque_changed_outside_boundary,
        "false_positive_alpha_mean":leak/(background_count.max(1) as f64),"recomposite":backgrounds}),
    )
}

/// New color gate frozen before candidate measurements. In addition to
/// nonregression, a visible initial fringe (>0.02 normalized RGB error) must
/// improve at least20%; a byte-level identity alone is not qualification.
pub fn color_gate(before: &Value, after: &Value) -> bool {
    let initial = before["fractional_foreground_mad"].as_f64().unwrap();
    let edge = after["fractional_foreground_mad"].as_f64().unwrap();
    after["opaque_changed_pixels"].as_u64() == Some(0)
        && after["alpha_weighted_foreground_mad"].as_f64().unwrap()
            <= before["alpha_weighted_foreground_mad"].as_f64().unwrap() + 1.0 / 255.0
        && edge <= 0.05
        && (initial <= 0.02 || edge <= initial * 0.8)
        && ["white", "dark", "checker"]
            .iter()
            .all(|name| after["recomposite"][name]["boundary_mad"].as_f64().unwrap() <= 0.02)
}
