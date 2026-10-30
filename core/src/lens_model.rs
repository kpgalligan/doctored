//! The radial lens model shared by RAW development and Filters ▸ Lens
//! Correction: the inverse radial distortion / transverse chromatic
//! aberration map, its fold checks and the radial vignette's radius and stops.
//! One copy of each: `raw_source_geometry` (the `Lens` source map and its plan's
//! fold refusal), `raw_local_develop` (the manual vignette) and
//! `sampling_lens` (the destructive filter) all call these, so the RAW output
//! stays bit-identical to the arithmetic that lived inline before (the same
//! operations in the same order; Rust contracts no `a·b + c` into a fused
//! multiply-add, so moving an expression into a function changes no bit).
//!
//! Coordinates are normalised by a radius `R` (half the longer side of the
//! reference frame, RAW's convention): `q = (p − c) / R`, `r² = |q|²`, and the
//! source of `p` is `c + R·q·f(r²)·scale` with `f(r²) = 1 + k1·r² + k2·r⁴ +
//! k3·r⁶` evaluated in Horner form.
//!
//! The Lensfun profile models (v1-lensfun: [`Distortion`], [`Tca`], the `pa`
//! vignetting and [`LensProfile`], which `sampling_lens` composes after the
//! manual stages) are written from Lensfun's documentation, not translated
//! from its LGPL library source: the model formulas from the comments of
//! `lensfun.h` (v0.3.4), the order of the corrections from its manual's "How it
//! is really done", the normalisation from the Hugin/PanoTools lens and
//! vignetting model pages, and the crop factor as 43.27 mm over the sensor
//! diagonal. The notes above the models give the arithmetic.

/// `1 + r2·(k1 + r2·(k2 + r2·k3))`: the radial factor in RAW's Horner order.
pub(crate) fn radial_factor(r2: f64, radial: [f64; 3]) -> f64 {
    1.0 + r2 * (radial[0] + r2 * (radial[1] + r2 * radial[2]))
}

/// The source position of `point` under the radial map about `center` with
/// normalising `radius`; `scale` is the per-channel lateral (TCA) scale, 1 for
/// green.
pub(crate) fn radial_map(
    center: [f64; 2],
    radius: f64,
    point: [f64; 2],
    radial: [f64; 3],
    scale: f64,
) -> [f64; 2] {
    let q = [
        (point[0] - center[0]) / radius,
        (point[1] - center[1]) / radius,
    ];
    let r2 = q[0] * q[0] + q[1] * q[1];
    let d = radial_factor(r2, radial);
    [
        center[0] + radius * q[0] * d * scale,
        center[1] + radius * q[1] * d * scale,
    ]
}

/// The inverse of [`radial_map`] at scale 1: the `u` with `radial_map(center,
/// radius, u, radial, 1.0) == point`, or `None`. Along the ray through `point`
/// it solves `ρ·f(ρ²) = ρ_s` by Newton from `ρ = ρ_s` (the identity's answer,
/// so a small coefficient converges in a few steps), at most 20 steps to
/// `|Δρ| ≤ 1e-12`; the derivative is [`radial_monotone`]'s, so a step that
/// lands past the first fold, where the map stops being one-to-one, refuses
/// rather than returning the mirrored band's preimage. `None` also when the
/// iteration does not converge or leaves the finite numbers.
pub(crate) fn radial_unmap(
    center: [f64; 2],
    radius: f64,
    point: [f64; 2],
    radial: [f64; 3],
) -> Option<[f64; 2]> {
    let q = [
        (point[0] - center[0]) / radius,
        (point[1] - center[1]) / radius,
    ];
    let target = q[0].hypot(q[1]);
    if !target.is_finite() {
        return None;
    }
    if target == 0.0 {
        return Some(point);
    }
    let mut rho = target;
    for _ in 0..20 {
        let r2 = rho * rho;
        if !radial_monotone(r2, radial) {
            return None;
        }
        let slope = 1.0 + r2 * (3.0 * radial[0] + r2 * (5.0 * radial[1] + r2 * 7.0 * radial[2]));
        let step = (rho * radial_factor(r2, radial) - target) / slope;
        rho -= step;
        if !rho.is_finite() || rho < 0.0 {
            return None;
        }
        if step.abs() <= 1e-12 {
            if !radial_monotone(rho * rho, radial) {
                return None;
            }
            let s = rho / target;
            return Some([center[0] + radius * q[0] * s, center[1] + radius * q[1] * s]);
        }
    }
    None
}

/// Whether the map may fold within normalised radius² `rmax2`: a nonfinite
/// radius, or a bound on the derivative's departure, `3|k1|r² + 5|k2|r⁴ +
/// 7|k3|r⁶`, above one half. RAW's plan-level refusal: it keeps a camera profile
/// well inside its monotone range over the whole frame.
pub(crate) fn radial_folds(radial: [f64; 3], rmax2: f64) -> bool {
    !rmax2.is_finite()
        || 3.0 * radial[0].abs() * rmax2
            + 5.0 * radial[1].abs() * rmax2.powi(2)
            + 7.0 * radial[2].abs() * rmax2.powi(3)
            > 0.5
}

/// Whether the map is still increasing at normalised radius² `r2`:
/// `d(r·f(r²))/dr = 1 + r2·(3k1 + r2·(5k2 + r2·7k3)) > 0`. Past the first radius
/// where it fails, a larger output radius reads a SMALLER source radius, so the
/// picture would fold back on itself as a mirrored band; a point there is
/// outside the corrected picture. The per-point test the filter uses, where
/// Scale and Perspective can carry a point past any whole-frame bound.
pub(crate) fn radial_monotone(r2: f64, radial: [f64; 3]) -> bool {
    1.0 + r2 * (3.0 * radial[0] + r2 * (5.0 * radial[1] + r2 * 7.0 * radial[2])) > 0.0
}

/// The radial vignette's normalising radius: the distance from `center` to the
/// farthest pixel centre of a `width`×`height` frame, so the farthest corner
/// pixel is at fraction 1.
pub(crate) fn vignette_radius(center: [f64; 2], width: u32, height: u32) -> f64 {
    [0.5, width as f64 - 0.5]
        .into_iter()
        .flat_map(|x| [0.5, height as f64 - 0.5].map(move |y| [x, y]))
        .map(|p| (p[0] - center[0]).hypot(p[1] - center[1]))
        .fold(0.0, f64::max)
}

/// The vignette's exposure in stops at `fraction` of its radius: `stops ·
/// fraction^falloff`, so the centre is untouched and the farthest corner gets
/// the full `stops`; a larger `falloff` keeps the middle clearer.
pub(crate) fn vignette_stops(stops: f64, fraction: f64, falloff: f64) -> f64 {
    stops * fraction.powf(falloff)
}

// The Lensfun lens profile models (v1-lensfun). Written from Lensfun's public
// model documentation (`lensfun.h`'s model comments and the manual's section
// on the actual order of corrections, v0.3.4), the Hugin/PanoTools pages on
// the lens and vignetting models, and the crop-factor definition; nothing here
// is translated from Lensfun's LGPL library source. The coefficients are the
// bundled CC BY-SA database's (`lens_db`).
//
// Lensfun stores maps from the corrected (undistorted) radius to the source
// (distorted) one, the direction the filter's inverse map already runs, so a
// corrected pixel reads the source at `c + (q − c)·r_d/r_u`. Radii are in
// PanoTools units: 1 is half the shorter side of the calibration frame. A
// plane's own point is `r_img = |q − c| / (S/2)` with S the plane's shorter
// side; its calibration radius is `r_img · (Cc/Ci) · √(1+Ac²)/√(1+Ai²)`,
// because a crop factor is 43.27 mm over the sensor diagonal: the frame's
// half short side is that diagonal over `2√(1+A²)`, so the ratio of the two
// half short sides is the ratio of crops times the ratio of those roots (Ci the
// image's crop factor, Cc the calibration's, Ac the calibration aspect, Ai the
// plane's long/short). The vignetting polynomial uses `r/√(1+Ac²)`, so the
// calibration frame's corner is radius 1.

/// A Lensfun distortion model (`r_d = r_u · factor(r_u)`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Distortion {
    /// `factor = 1 − k1 + k1·r²`.
    Poly3 { k1: f64 },
    /// `factor = 1 + k1·r² + k2·r⁴`.
    Poly5 { k1: f64, k2: f64 },
    /// `factor = a·r³ + b·r² + c·r + 1 − a − b − c`.
    PtLens { a: f64, b: f64, c: f64 },
}

/// A Lensfun transverse chromatic aberration model, on the distorted radius:
/// red and blue are scaled about the centre, green is the reference.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Tca {
    /// `r · k` per channel.
    Linear { kr: f64, kb: f64 },
    /// `r · (b·r² + c·r + v)` per channel, each `[v, c, b]`.
    Poly3 { red: [f64; 3], blue: [f64; 3] },
}

/// `r_d / r_u` at undistorted calibration radius `r`.
pub(crate) fn distortion_factor(d: Distortion, r: f64) -> f64 {
    match d {
        Distortion::Poly3 { k1 } => 1.0 - k1 + k1 * r * r,
        Distortion::Poly5 { k1, k2 } => {
            let r2 = r * r;
            1.0 + r2 * (k1 + r2 * k2)
        }
        Distortion::PtLens { a, b, c } => ((a * r + b) * r + c) * r + 1.0 - a - b - c,
    }
}

/// `d(r·factor(r))/dr`: where it is not positive, a larger corrected radius
/// reads a smaller source radius and the picture folds back on itself, so a
/// point there is outside the corrected picture (`radial_monotone`'s rule).
pub(crate) fn distortion_slope(d: Distortion, r: f64) -> f64 {
    match d {
        Distortion::Poly3 { k1 } => 1.0 - k1 + 3.0 * k1 * r * r,
        Distortion::Poly5 { k1, k2 } => {
            let r2 = r * r;
            1.0 + r2 * (3.0 * k1 + r2 * 5.0 * k2)
        }
        Distortion::PtLens { a, b, c } => {
            ((4.0 * a * r + 3.0 * b) * r + 2.0 * c) * r + 1.0 - a - b - c
        }
    }
}

/// The lateral scale of `channel` (0 red, 1 green, 2 blue) at distorted
/// calibration radius `r`; green is always 1.
pub(crate) fn tca_factor(t: Tca, channel: usize, r: f64) -> f64 {
    match (t, channel) {
        (Tca::Linear { kr, .. }, 0) => kr,
        (Tca::Linear { kb, .. }, 2) => kb,
        (Tca::Poly3 { red, .. }, 0) => (red[2] * r + red[1]) * r + red[0],
        (Tca::Poly3 { blue, .. }, 2) => (blue[2] * r + blue[1]) * r + blue[0],
        _ => 1.0,
    }
}

/// The PanoTools `pa` falloff `1 + k1·r² + k2·r⁴ + k3·r⁶` at `r2 = r²`: the
/// measured brightness relative to the centre. It is RAW's radial polynomial,
/// so it is [`radial_factor`].
pub(crate) fn pa_falloff(k: [f64; 3], r2: f64) -> f64 {
    radial_factor(r2, k)
}

/// The darkest falloff a profile corrects: four stops. A coefficient set that
/// predicts less (or a nonpositive value, far outside the calibrated radius)
/// is held there rather than dividing by a vanishing number.
const MIN_FALLOFF: f64 = 1.0 / 16.0;

/// A profile model's object and its `model` name, or `None` when null.
type ModelObject<'a> = Option<(&'a serde_json::Map<String, serde_json::Value>, String)>;

/// A resolved Lensfun profile: the coefficients at one focal length, f-number
/// and distance (`lens_db` interpolates them), and the frame they were
/// calibrated on. Numbers only, so `LensCorrection` stays `Copy`; the lens id
/// stays with the host.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LensProfile {
    pub distortion: Option<Distortion>,
    pub tca: Option<Tca>,
    /// The `pa` coefficients `[k1, k2, k3]`.
    pub vignetting: Option<[f64; 3]>,
    /// The image's camera crop factor (Ci).
    pub crop_factor: f64,
    /// The crop factor of the camera the lens was calibrated on (Cc).
    pub calibration_crop_factor: f64,
    /// The calibration frame's long/short aspect (Ac).
    pub aspect_ratio: f64,
    /// The optical centre's offset from the plane's centre, in half shorter
    /// sides.
    pub center: [f64; 2],
}

impl LensProfile {
    /// The strict profile object (the `dr_lens_match` answer's `profile`, and
    /// `lens_correction`'s `profile` parameter): unknown keys, out-of-range
    /// values and a profile with all three models null are refused by name.
    /// A model that is the identity is kept as absent.
    pub(crate) fn from_json(v: &serde_json::Value) -> Result<Self, String> {
        use serde_json::Value;
        let object = v.as_object().ok_or("lens profile must be an object")?;
        const KEYS: [&str; 11] = [
            "id",
            "crop_factor",
            "calibration_crop_factor",
            "aspect_ratio",
            "center",
            "focal_length",
            "f_number",
            "subject_distance",
            "distortion",
            "tca",
            "vignetting",
        ];
        if let Some(key) = object.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(format!("lens profile has unknown key {key}"));
        }
        if !object.get("id").is_some_and(Value::is_string) {
            return Err("lens profile id must be a string".into());
        }
        let number = |o: &serde_json::Map<String, Value>, name: &str, what: &str| {
            o.get(name)
                .and_then(Value::as_f64)
                .filter(|n| n.is_finite())
                .ok_or_else(|| format!("{what} {name} must be a finite number"))
        };
        let within = |o: &serde_json::Map<String, Value>,
                      name: &str,
                      what: &str,
                      lo: f64,
                      hi: f64,
                      open: bool|
         -> Result<f64, String> {
            let n = number(o, name, what)?;
            let low_ok = if open { n > lo } else { n >= lo };
            if !low_ok || n > hi {
                let left = if open { "(" } else { "[" };
                return Err(format!("{what} {name} must be in {left}{lo}, {hi}]"));
            }
            Ok(n)
        };
        let what = "lens profile";
        let crop_factor = within(object, "crop_factor", what, 0.1, 20.0, true)?;
        let calibration_crop_factor =
            within(object, "calibration_crop_factor", what, 0.1, 20.0, true)?;
        let aspect_ratio = within(object, "aspect_ratio", what, 1.0, 4.0, false)?;
        let focal = number(object, "focal_length", what)?;
        if focal <= 0.0 {
            return Err("lens profile focal_length must be positive".into());
        }
        for name in ["f_number", "subject_distance"] {
            match object.get(name) {
                None | Some(Value::Null) => {}
                Some(v) if v.as_f64().is_some_and(|n| n.is_finite() && n > 0.0) => {}
                Some(_) => {
                    return Err(format!(
                        "lens profile {name} must be a positive number or null"
                    ))
                }
            }
        }
        let center = match object.get("center") {
            None => [0.0, 0.0],
            Some(v) => {
                let pair = v
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .and_then(|a| Some([a[0].as_f64()?, a[1].as_f64()?]))
                    .filter(|c| c.iter().all(|n| n.is_finite()))
                    .ok_or("lens profile center must be two numbers")?;
                if pair[0].hypot(pair[1]) > 1.0 {
                    return Err("lens profile center must be within 1 of the middle".into());
                }
                pair
            }
        };
        let model = |name: &str| -> Result<ModelObject<'_>, String> {
            match object.get(name) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::Object(o)) => {
                    let model = o
                        .get("model")
                        .and_then(Value::as_str)
                        .ok_or_else(|| format!("lens profile {name} needs a model"))?;
                    Ok(Some((o, model.to_owned())))
                }
                Some(_) => Err(format!("lens profile {name} must be an object or null")),
            }
        };
        let strict = |o: &serde_json::Map<String, Value>, name: &str, keys: &[&str]| match o
            .keys()
            .find(|k| *k != "model" && !keys.contains(&k.as_str()))
        {
            Some(key) => Err(format!("lens profile {name} has unknown key {key}")),
            None => Ok(()),
        };
        let distortion = match model("distortion")? {
            None => None,
            Some((o, kind)) => {
                let what = "lens profile distortion";
                let term = |name: &str| within(o, name, what, -1.0, 1.0, false);
                Some(match kind.as_str() {
                    "poly3" => {
                        strict(o, "distortion", &["k1"])?;
                        Distortion::Poly3 { k1: term("k1")? }
                    }
                    "poly5" => {
                        strict(o, "distortion", &["k1", "k2"])?;
                        Distortion::Poly5 {
                            k1: term("k1")?,
                            k2: term("k2")?,
                        }
                    }
                    "ptlens" => {
                        strict(o, "distortion", &["a", "b", "c"])?;
                        Distortion::PtLens {
                            a: term("a")?,
                            b: term("b")?,
                            c: term("c")?,
                        }
                    }
                    other => {
                        return Err(format!(
                            "lens profile distortion model {other} is not poly3, poly5 or ptlens"
                        ))
                    }
                })
            }
        };
        let tca = match model("tca")? {
            None => None,
            Some((o, kind)) => {
                let what = "lens profile tca";
                let scale = |name: &str| within(o, name, what, 0.9, 1.1, false);
                let term = |name: &str| within(o, name, what, -0.1, 0.1, false);
                Some(match kind.as_str() {
                    "linear" => {
                        strict(o, "tca", &["kr", "kb"])?;
                        Tca::Linear {
                            kr: scale("kr")?,
                            kb: scale("kb")?,
                        }
                    }
                    "poly3" => {
                        strict(o, "tca", &["vr", "cr", "br", "vb", "cb", "bb"])?;
                        Tca::Poly3 {
                            red: [scale("vr")?, term("cr")?, term("br")?],
                            blue: [scale("vb")?, term("cb")?, term("bb")?],
                        }
                    }
                    other => {
                        return Err(format!(
                            "lens profile tca model {other} is not linear or poly3"
                        ))
                    }
                })
            }
        };
        let vignetting = match model("vignetting")? {
            None => None,
            Some((o, kind)) => {
                if kind != "pa" {
                    return Err(format!("lens profile vignetting model {kind} is not pa"));
                }
                strict(o, "vignetting", &["k1", "k2", "k3"])?;
                let term =
                    |name: &str| within(o, name, "lens profile vignetting", -5.0, 5.0, false);
                Some([term("k1")?, term("k2")?, term("k3")?])
            }
        };
        if distortion.is_none() && tca.is_none() && vignetting.is_none() {
            return Err("lens profile corrects nothing".into());
        }
        Ok(Self {
            distortion: distortion.filter(|d| !distortion_identity(*d)),
            tca: tca.filter(|t| !tca_identity(*t)),
            vignetting: vignetting.filter(|k| *k != [0.0; 3]),
            crop_factor,
            calibration_crop_factor,
            aspect_ratio,
            center,
        })
    }

    /// Every model the identity (or absent): the profile changes nothing and
    /// is treated as absent, so the filter's exact bypass still holds.
    pub(crate) fn neutral(&self) -> bool {
        self.distortion.is_none_or(distortion_identity)
            && self.tca.is_none_or(tca_identity)
            && self.vignetting.is_none_or(|k| k == [0.0; 3])
    }

    /// The optical centre, the plane's half shorter side and the factor from
    /// half-short-side units of the plane to calibration units (module notes
    /// above).
    fn frame(&self, dims: (u32, u32)) -> ([f64; 2], f64, f64) {
        let (w, h) = (f64::from(dims.0), f64::from(dims.1));
        let short = w.min(h);
        let half = short / 2.0;
        let c = [
            w / 2.0 + self.center[0] * half,
            h / 2.0 + self.center[1] * half,
        ];
        let plane_aspect = if short > 0.0 { w.max(h) / short } else { 1.0 };
        let scale = (self.calibration_crop_factor / self.crop_factor)
            * (1.0 + self.aspect_ratio * self.aspect_ratio).sqrt()
            / (1.0 + plane_aspect * plane_aspect).sqrt();
        (c, half, scale)
    }

    /// The source position, in plane pixels, of corrected point `q` for
    /// colour `channel` on a plane of `dims`: the distortion, then the
    /// channel's TCA on the distorted radius (the inverse of Lensfun's
    /// correction order). `None` past the distortion's fold.
    pub(crate) fn source(&self, dims: (u32, u32), q: [f64; 2], channel: usize) -> Option<[f64; 2]> {
        let (c, half, scale) = self.frame(dims);
        if half <= 0.0 {
            return Some(q);
        }
        let d = [q[0] - c[0], q[1] - c[1]];
        let r = d[0].hypot(d[1]) / half * scale;
        if !r.is_finite() {
            return None;
        }
        let mut factor = 1.0;
        if let Some(distortion) = self.distortion {
            if distortion_slope(distortion, r) <= 0.0 {
                return None;
            }
            factor = distortion_factor(distortion, r);
        }
        if let Some(tca) = self.tca {
            factor *= tca_factor(tca, channel, r * factor);
        }
        Some([c[0] + d[0] * factor, c[1] + d[1] * factor])
    }

    /// The correction in stops at source point `s`: the falloff `V` there is
    /// divided out, so `−log2 max(V, 1/16)`; 0 without vignetting.
    pub(crate) fn vignette_stops(&self, dims: (u32, u32), s: [f64; 2]) -> f64 {
        let Some(k) = self.vignetting else {
            return 0.0;
        };
        let (c, half, scale) = self.frame(dims);
        if half <= 0.0 {
            return 0.0;
        }
        let r = (s[0] - c[0]).hypot(s[1] - c[1]) / half * scale;
        let rv2 = r * r / (1.0 + self.aspect_ratio * self.aspect_ratio);
        let falloff = pa_falloff(k, rv2);
        if falloff.is_nan() {
            return 0.0;
        }
        -falloff.max(MIN_FALLOFF).log2()
    }
}

fn distortion_identity(d: Distortion) -> bool {
    match d {
        Distortion::Poly3 { k1 } => k1 == 0.0,
        Distortion::Poly5 { k1, k2 } => k1 == 0.0 && k2 == 0.0,
        Distortion::PtLens { a, b, c } => a == 0.0 && b == 0.0 && c == 0.0,
    }
}

fn tca_identity(t: Tca) -> bool {
    match t {
        Tca::Linear { kr, kb } => kr == 1.0 && kb == 1.0,
        Tca::Poly3 { red, blue } => red == [1.0, 0.0, 0.0] && blue == [1.0, 0.0, 0.0],
    }
}
