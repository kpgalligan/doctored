//! The colour kernel of Filter ▸ Neural Filters ▸ Colorize (M4 phase
//! 2). DDColor predicts Lab chroma from luminance at a fixed square size;
//! this module builds the model's input from a layer, adjusts the predicted
//! chroma with the classical Colorize sliders, and carries it onto
//! the layer's own full-resolution L*.
//!
//! **Input** (`input`). Each source pixel's L* in the document's own profile
//! (`MatrixTrc::to_pcs_xyz64` → `icc_lut::xyz_to_lab`; alpha ignored, the
//! model sees the whole layer) is resized to `side²` by
//! `sampling_walk::resize_into` (`Kernel::Bilinear`, which widens on
//! reduction), and each cell is written as the sRGB gray of Lab(L, 0, 0)
//! (`lab::lab_to_srgb`): DDColor's own pipeline feeds exactly that gray.
//!
//! **Adjustments** (`adjusted_plane`), on the model's `side²` chroma, in
//! order: Color Artifact Reduction is a Gaussian of σ =
//! `ARTIFACT_SIGMA_MAX`·r/100 model pixels (separable, clamped edges,
//! binary64, the taps `sampling_gaussian::gaussian_taps`), which smooths the
//! blotches a colourisation model leaves without touching luminance;
//! Saturation scales a*, b* by `1 + s/50` (−50 is gray, +50 doubles chroma);
//! the three balances add `(v/50)·BALANCE_CHROMA` along the unit a*b*
//! direction of the named sRGB primary (positive is the second-named colour:
//! red, green, blue), as Color Balance pushes towards one end of each axis.
//! Saturation and balance are affine, so applying them before the
//! weights-sum-to-one upscale equals applying them per output pixel.
//!
//! **Reconstruction** (`reconstruct`). Per output pixel, the source pixel's
//! own L* and the bilinear sample of the adjusted plane at `s = (o + 0.5)·
//! side/dim` (`Kernel::weights_f64`, clamped taps). When Lab(L, a, b) lies
//! outside the profile's gamut the chroma is scaled by the largest `t` in
//! [0, 1] (16 bisection steps) whose colour is inside, so hue and L* are
//! kept and only chroma gives way. L* is never altered, so the source's
//! luminance and detail survive up to the encoder's quantization; alpha is
//! copied bit-exact.
use serde::Deserialize;

use crate::adjust_calibrated::Encoder;
use crate::icc_lut::{lab_to_xyz, xyz_to_lab};
use crate::icc_transform::MatrixTrc;
use crate::job_control::JobContext;
use crate::sampling::{resize_into, Kernel, Unorm, MAX_TAPS};

/// The smallest model side accepted: below it the model sees too few cells
/// for any image structure (and `neural_tiling`'s smallest tile is the same).
pub(crate) const MIN_SIDE: u32 = 16;
/// The largest model side accepted: DDColor is converted at 512², and twice
/// that bounds the host's input and plane buffers (12 MiB and 8 MiB of f32).
pub(crate) const MAX_SIDE: u32 = 1024;
/// The a*b* push of a balance slider at ±50, in Lab units: 30 is a clearly
/// visible cast on a neutral (about a quarter of an sRGB primary's chroma)
/// without driving ordinary predicted colours out of gamut.
pub(crate) const BALANCE_CHROMA: f64 = 30.0;
/// Color Artifact Reduction's σ at 100, in model pixels: 4 of 512 is under
/// 1 % of the image, enough to dissolve the small colour blotches DDColor
/// leaves while keeping object-sized colour regions apart.
pub(crate) const ARTIFACT_SIGMA_MAX: f64 = 4.0;
/// Bisection steps of the gamut map: 2^-16 of the chroma, far below one
/// 16-bit code.
const GAMUT_STEPS: u32 = 16;

/// Colorize's classical sliders (module doc).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Adjust {
    pub(crate) saturation: f64,
    pub(crate) cyan_red: f64,
    pub(crate) magenta_green: f64,
    pub(crate) yellow_blue: f64,
    pub(crate) color_artifact_reduction: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(default)]
    saturation: f64,
    #[serde(default)]
    cyan_red: f64,
    #[serde(default)]
    magenta_green: f64,
    #[serde(default)]
    yellow_blue: f64,
    #[serde(default)]
    color_artifact_reduction: f64,
}

impl Adjust {
    /// The sliders from a JSON object, every key optional (0); refuses
    /// malformed JSON, an unknown key, a text over 4 KiB, a balance or
    /// saturation outside −50…50 and an artifact reduction outside 0…100.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        if text.len() > 4096 {
            return Err("colorize adjustments exceed 4 KiB".into());
        }
        let invalid = |e: serde_json::Error| format!("invalid colorize adjustments: {e}");
        // An object only: serde would also read a struct from a JSON array.
        let value: serde_json::Value = serde_json::from_str(text).map_err(invalid)?;
        if !value.is_object() {
            return Err("invalid colorize adjustments: expected a JSON object".into());
        }
        let r: Request = serde_json::from_value(value).map_err(invalid)?;
        for (key, value) in [
            ("saturation", r.saturation),
            ("cyan_red", r.cyan_red),
            ("magenta_green", r.magenta_green),
            ("yellow_blue", r.yellow_blue),
        ] {
            if !value.is_finite() || !(-50.0..=50.0).contains(&value) {
                return Err(format!("colorize {key} must be -50 to 50"));
            }
        }
        let artifact = r.color_artifact_reduction;
        if !artifact.is_finite() || !(0.0..=100.0).contains(&artifact) {
            return Err("colorize color_artifact_reduction must be 0 to 100".into());
        }
        Ok(Self {
            saturation: r.saturation,
            cyan_red: r.cyan_red,
            magenta_green: r.magenta_green,
            yellow_blue: r.yellow_blue,
            color_artifact_reduction: artifact,
        })
    }

    /// The constant a*b* offset of the three balances.
    fn balance(&self) -> Result<[f64; 2], String> {
        let mut offset = [0.0; 2];
        for (value, primary) in [
            (self.cyan_red, [1.0, 0.0, 0.0]),
            (self.magenta_green, [0.0, 1.0, 0.0]),
            (self.yellow_blue, [0.0, 0.0, 1.0]),
        ] {
            if value == 0.0 {
                continue;
            }
            let lab = crate::lab::srgb_to_lab(primary).ok_or("colorize needs the sRGB model")?;
            let (a, b) = (f64::from(lab[1]), f64::from(lab[2]));
            let length = a.hypot(b);
            let push = value / 50.0 * BALANCE_CHROMA / length;
            offset[0] += push * a;
            offset[1] += push * b;
        }
        Ok(offset)
    }
}

/// Refuses a model side outside `MIN_SIDE…MAX_SIDE`.
pub(crate) fn check_side(side: u32) -> Result<(), String> {
    if !(MIN_SIDE..=MAX_SIDE).contains(&side) {
        return Err(format!("colorize side must be {MIN_SIDE} to {MAX_SIDE}"));
    }
    Ok(())
}

/// One encoded pixel's L* through `model`.
fn lightness<T: Unorm>(pixel: &[T; 4], model: &MatrixTrc) -> f64 {
    let rgb = [0, 1, 2].map(|c| f64::from(pixel[c].raw()) / f64::from(T::MAX));
    xyz_to_lab(model.to_pcs_xyz64(rgb))[0]
}

/// The model input (module doc): `out` is channel-major RGB, `3·side²`.
pub(crate) fn input<T: Unorm>(
    src: &[[T; 4]],
    dims: (u32, u32),
    model: &MatrixTrc,
    side: u32,
    out: &mut [f32],
    job: JobContext<'_>,
) -> Result<(), String> {
    check_side(side)?;
    let cells = side as usize * side as usize;
    if out.len() != 3 * cells {
        return Err("the colorize input must hold 3 × side² samples".into());
    }
    if src.len() != dims.0 as usize * dims.1 as usize || src.is_empty() {
        return Err("the colorize source does not match its dimensions".into());
    }
    let mut lightnesses = crate::precision_image::reserved_vec::<f32>(cells)?;
    lightnesses.resize(cells, 0.0);
    resize_into::<f32, 1>(
        dims,
        (side, side),
        Kernel::Bilinear,
        |i| lightness(&src[i], model) as f32,
        |acc, value, weight| acc[0] += value * weight,
        |acc| acc[0],
        |i, value| lightnesses[i] = value,
        true,
        job.range(0.0, 0.9),
    )?;
    for (i, &l) in lightnesses.iter().enumerate() {
        job.every(i)?;
        let (gray, _) = crate::lab::lab_to_srgb([l.clamp(0.0, 100.0), 0.0, 0.0])
            .ok_or("colorize needs the sRGB model")?;
        for c in 0..3 {
            out[c * cells + i] = gray[c];
        }
    }
    job.report(1.0, 6)?;
    Ok(())
}

/// One separable pass of `taps` over `plane` (`side × side`), clamped edges,
/// along x when `horizontal`.
fn blur_pass(plane: &[[f64; 2]], side: usize, taps: &[f64], horizontal: bool) -> Vec<[f64; 2]> {
    let half = (taps.len() / 2) as i64;
    let last = side as i64 - 1;
    let mut out = vec![[0.0; 2]; plane.len()];
    for y in 0..side {
        for x in 0..side {
            let mut acc = [0.0; 2];
            for (k, &w) in taps.iter().enumerate() {
                let offset = k as i64 - half;
                let (sx, sy) = if horizontal {
                    ((x as i64 + offset).clamp(0, last) as usize, y)
                } else {
                    (x, (y as i64 + offset).clamp(0, last) as usize)
                };
                let v = plane[sy * side + sx];
                acc[0] += w * v[0];
                acc[1] += w * v[1];
            }
            out[y * side + x] = acc;
        }
    }
    out
}

/// The adjusted chroma plane (module doc): `chroma` is channel-major a* then
/// b*, `2·side²`, in Lab units; the answer is `side²` (a*, b*) pairs.
pub(crate) fn adjusted_plane(
    chroma: &[f32],
    side: u32,
    adjust: &Adjust,
) -> Result<Vec<[f64; 2]>, String> {
    check_side(side)?;
    let cells = side as usize * side as usize;
    if chroma.len() != 2 * cells {
        return Err("the colorize chroma must hold 2 × side² samples".into());
    }
    if chroma.iter().any(|v| !v.is_finite()) {
        return Err("the colorize model's output is not finite".into());
    }
    let mut plane = crate::precision_image::reserved_vec::<[f64; 2]>(cells)?;
    plane.extend((0..cells).map(|i| [f64::from(chroma[i]), f64::from(chroma[cells + i])]));
    if adjust.color_artifact_reduction > 0.0 {
        let sigma = ARTIFACT_SIGMA_MAX * adjust.color_artifact_reduction / 100.0;
        let taps = crate::sampling_gaussian::gaussian_taps(sigma)?;
        let across = blur_pass(&plane, side as usize, &taps, true);
        plane = blur_pass(&across, side as usize, &taps, false);
    }
    let gain = 1.0 + adjust.saturation / 50.0;
    let offset = adjust.balance()?;
    for ab in &mut plane {
        ab[0] = ab[0] * gain + offset[0];
        ab[1] = ab[1] * gain + offset[1];
    }
    Ok(plane)
}

/// True when Lab(L, a, b) is inside `model`'s gamut.
fn inside(model: &MatrixTrc, lab: [f64; 3]) -> bool {
    model
        .pcs_to_linear(lab_to_xyz(lab))
        .iter()
        .all(|v| (0.0..=1.0).contains(v))
}

/// The largest chroma scale in [0, 1] whose colour is inside the gamut
/// (module doc); 0 when even the neutral is outside.
fn gamut_scale(model: &MatrixTrc, l: f64, ab: [f64; 2]) -> f64 {
    if inside(model, [l, ab[0], ab[1]]) {
        return 1.0;
    }
    if !inside(model, [l, 0.0, 0.0]) {
        return 0.0;
    }
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..GAMUT_STEPS {
        let mid = 0.5 * (low + high);
        if inside(model, [l, mid * ab[0], mid * ab[1]]) {
            low = mid;
        } else {
            high = mid;
        }
    }
    low
}

/// The two clamped bilinear taps at source coordinate `s` over `side`.
fn taps(s: f64, side: u32) -> ([usize; 2], [f64; 2]) {
    let mut weights = [0.0_f64; MAX_TAPS];
    let first = Kernel::Bilinear.weights_f64(s, &mut weights);
    let last = i64::from(side) - 1;
    let index = |t: i64| (first + t).clamp(0, last) as usize;
    ([index(0), index(1)], [weights[0], weights[1]])
}

/// The colorized layer (module doc) into `out`, the source's size.
#[allow(clippy::too_many_arguments)]
pub(crate) fn reconstruct<T: Unorm>(
    src: &[[T; 4]],
    dims: (u32, u32),
    model: &MatrixTrc,
    encoder: &Encoder,
    plane: &[[f64; 2]],
    side: u32,
    out: &mut [[T; 4]],
    job: JobContext<'_>,
) -> Result<(), String> {
    check_side(side)?;
    let (w, h) = (dims.0 as usize, dims.1 as usize);
    if src.len() != w * h || out.len() != src.len() || src.is_empty() {
        return Err("the colorize source does not match its dimensions".into());
    }
    if plane.len() != side as usize * side as usize {
        return Err("the colorize chroma must hold 2 × side² samples".into());
    }
    let ratio_x = f64::from(side) / f64::from(dims.0);
    let ratio_y = f64::from(side) / f64::from(dims.1);
    let columns: Vec<([usize; 2], [f64; 2])> = (0..dims.0)
        .map(|x| taps((f64::from(x) + 0.5) * ratio_x, side))
        .collect();
    let stride = side as usize;
    for y in 0..h {
        job.check()?;
        let (rows, wy) = taps((y as f64 + 0.5) * ratio_y, side);
        for (x, &(cols, wx)) in columns.iter().enumerate() {
            let i = y * w + x;
            let mut ab = [0.0; 2];
            for (r, &weight_y) in rows.iter().zip(&wy) {
                for (c, &weight_x) in cols.iter().zip(&wx) {
                    let v = plane[r * stride + c];
                    let weight = weight_y * weight_x;
                    ab[0] += weight * v[0];
                    ab[1] += weight * v[1];
                }
            }
            let l = lightness(&src[i], model);
            let t = gamut_scale(model, l, ab);
            let linear = model.pcs_to_linear(lab_to_xyz([l, t * ab[0], t * ab[1]]));
            let encoded = encoder.rgb(linear);
            let pixel = &mut out[i];
            for c in 0..3 {
                pixel[c] = T::quantize(encoded[c] * T::MAX);
            }
            pixel[3] = src[i][3];
        }
        job.report((y + 1) as f64 / h as f64, 6)?;
    }
    Ok(())
}
