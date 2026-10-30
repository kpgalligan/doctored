//! The focus and saturation retouch brushes: Blur, Sharpen and Sponge
//! (Saturate / Desaturate). Each has a per-pixel TARGET — what the pixel
//! becomes at full strength under full stroke coverage — and one mix toward
//! it. The byte and 16-bit adapters (`doc_retouch_brush`,
//! `precision_retouch_brush`) read straight samples in [0, 1], call
//! [`targets`] over the rect the stroke covers and [`mix`] per covered pixel,
//! so the two depths share one arithmetic.
//!
//! - Blur: the alpha-weighted Gaussian of the straight colour at
//!   [`BLUR_BRUSH_SIGMA`], `Σ k·a·rgb / Σ k·a` over the separable taps of
//!   `sampling_gaussian::gaussian_taps` (the master copy of the tap
//!   arithmetic), reads clamped to the layer; the pixel's own colour where the
//!   weight sum is 0. Weighting by alpha keeps a transparent neighbour's
//!   latent colour out of the result.
//! - Sharpen: `clamp01(rgb + SHARPEN_BRUSH_AMOUNT · (rgb − blur))` with the
//!   same blur at [`SHARPEN_BRUSH_SIGMA`], an unsharp mask.
//! - Desaturate: the luma `Y` (`blend::LUMA_*`) on all three channels.
//! - Saturate: `clamp01(Y + 2·(rgb − Y))`, the chroma doubled.
//!
//! Alpha is never written, and the adapters skip alpha-0 pixels (their colour
//! is latent, not picture) — except under Sample All Layers (below), whose
//! picture can lay colour and alpha onto an empty pixel.
//!
//! That is 8 and 16 bits (`SampleRange::Unit`); [`halo`] and [`targets`] are
//! that form. A 32-bit float layer (`float_retouch_brush`) asks
//! [`halo_in`] and [`targets_in`] for `SampleRange::Extended` and gets the
//! same arithmetic over its straight LINEAR samples with two differences:
//!
//! - The Gaussian is the float FIR (`sampling_gaussian::gaussian_taps_in`):
//!   its half-width is where the dropped tails are under 1e-7 of the weight
//!   (about 5.33 sigma: 11 pixels for Blur, 6 for Sharpen), never
//!   `ceil(3·sigma)`. The narrower kernel drops 2.7e-3 of the weight, which
//!   beside a highlight of 5000 is far more than a 0.1 surround
//!   (`sampling_gaussian::EXTENDED_TAIL`).
//! - Nothing is clamped to 0…1 (`SampleRange::color`): a blur spreads a
//!   value above white and a Sharpen overshoot below zero is the target.
//!
//! The Sponge at float (R5) is the same `Extended` arithmetic: the luma
//! and `Y + 2·(rgb − Y)` over linear samples with no clamp to 0…1.
//!
//! On a Grayscale document (#1 phase 2c, [`Vibrancy::Gray`]) the Sponge
//! moves gray levels away from or toward the middle gray `m`: Saturate
//! `m + 2·(p − m)`, distance from the middle gray doubled, and Desaturate `m`
//! — the RGB rule with `m` in place of `Y`, finished by the same `range.color`
//! (clamped at `Unit`, unclamped at `Extended`, so a 32-bit Saturate may write
//! a negative linear value). Vibrance is ignored there: a gray pixel has no
//! chroma for it to weigh. The magnitude is Doctored's rule; no take measured
//! Photoshop's.
//!
//! On a Lab Color document (#1 phase 2e, [`Vibrancy::Lab`]) the samples
//! are the L\*, a\* and b\* codes and `n` is the a\* = b\* = 0 code: the
//! Sponge doubles (Saturate, `n + 2·(p − n)` on a\* and b\*) or zeroes
//! (Desaturate, `n`) the chroma about the neutral axis and keeps L\* — the
//! Gray rule on the two chroma codes, finished by the same `range.color`.
//! Vibrance is ignored there too (Gray's rule). Doctored's own rule; no take.
//!
//! # Vibrance
//!
//! The Sponge has a Vibrance checkbox. With it on ([`Vibrancy`] not
//! `Off`) the Saturate target is the Vibrance stage of Image ▸ Adjustments ▸
//! Vibrance at `+SPONGE_VIBRANCE` and the Desaturate target the same stage at
//! `−SPONGE_VIBRANCE` ([`sponge_slider`]; `adjust_color`'s stage, the one
//! Vibrance implementation, in linear light), mixed by coverage × strength as
//! before. At 8 and 16 bits the stage runs bound to the document's profile
//! exactly as the adjustment's Vibrance stage is (`Bound`, a
//! `BoundVibrance` the adapter binds), so on any RGB profile the Sponge's
//! target IS the adjustment's; on a CMYK record's `[L_c, L_m, L_y]` pass it
//! runs through the sRGB curve (`Encoded`, `Vibrance::vibrance_srgb`), and at
//! float through the display encoding's Scaled form (`Scaled`, rule V: what
//! that adjustment does at float). A gain that falls with chroma is what
//! spares an already saturated colour from clipping, which is the checkbox's
//! purpose.
//!
//! # Sample All Layers
//!
//! With Sample All Layers the Blur and Sharpen targets read the sampled
//! picture (`retouch_brush_sampling`) instead of the layer, and each covered
//! pixel becomes [`mix_over`] of the layer pixel and `[target, sampled α]`,
//! so an empty layer fills with the softened picture. The two option
//! refusals are [`VIBRANCE_REFUSAL`] and [`SAMPLE_ALL_LAYERS_REFUSAL`].
use crate::adjust_color::Vibrance;
use crate::adjust_vibrance::BoundVibrance;
use crate::blend::{LUMA_B, LUMA_G, LUMA_R};
use crate::display_encoding::DisplayEncoding;
use crate::filter_kernels::SampleRange;
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use crate::sampling_gaussian::gaussian_taps_in;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RetouchBrushKind {
    Blur,
    Sharpen,
    Saturate,
    Desaturate,
}

impl RetouchBrushKind {
    /// The C code of `dr_doc_retouch_brush_layer`: 0 blur, 1 sharpen,
    /// 2 saturate, 3 desaturate.
    pub(crate) fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Blur),
            1 => Some(Self::Sharpen),
            2 => Some(Self::Saturate),
            3 => Some(Self::Desaturate),
            _ => None,
        }
    }
}

/// The refusal of Vibrance on any brush but the Sponge.
pub(crate) const VIBRANCE_REFUSAL: &str = "vibrance applies to the sponge only";
/// The refusal of Sample All Layers on the Sponge.
pub(crate) const SAMPLE_ALL_LAYERS_REFUSAL: &str =
    "sample_all_layers applies to blur, sharpen and smudge";

/// The Vibrance slider the Sponge's Vibrance target runs at, as a fraction
/// (+ for Saturate, − for Desaturate): the full slider, so one pass at full
/// strength moves a pixel as far as the adjustment's own extreme does, and
/// Strength chooses how far toward it.
pub(crate) const SPONGE_VIBRANCE: f32 = 1.0;

/// How the Sponge's target treats Vibrance (module doc): `Off`, the plain
/// target; `Encoded`, the Vibrance stage through the sRGB curve (a CMYK
/// record's `L` pass); `Scaled`, the stage through a float document's display
/// encoding; `Bound`, the stage bound to an 8 or 16-bit document's profile
/// (the binding's slider is [`sponge_slider`]'s); `Gray`, a Grayscale
/// document's contrast rule around its middle gray; `Lab`, a Lab Color
/// document's chroma rule about its neutral a\*/b\* code (module doc).
#[derive(Clone, Copy)]
pub(crate) enum Vibrancy<'a> {
    Off,
    Encoded,
    Scaled(&'a DisplayEncoding<'a>),
    Bound(&'a BoundVibrance),
    Gray(f32),
    Lab(f32),
}

/// The Sponge's Vibrance slider for `kind` (module doc): `+SPONGE_VIBRANCE`
/// for Saturate, `−SPONGE_VIBRANCE` for Desaturate — the one place the sign is
/// chosen.
pub(crate) fn sponge_slider(kind: RetouchBrushKind) -> Vibrance {
    Vibrance::slider(if kind == RetouchBrushKind::Desaturate {
        -SPONGE_VIBRANCE
    } else {
        SPONGE_VIBRANCE
    })
}

/// The two option refusals (module doc): Vibrance on a kind that is not the
/// Sponge, Sample All Layers on the Sponge.
pub(crate) fn refuse_options(
    kind: RetouchBrushKind,
    vibrance: bool,
    sample_all_layers: bool,
) -> Result<(), String> {
    let sponge = matches!(
        kind,
        RetouchBrushKind::Saturate | RetouchBrushKind::Desaturate
    );
    if vibrance && !sponge {
        return Err(VIBRANCE_REFUSAL.into());
    }
    if sample_all_layers && sponge {
        return Err(SAMPLE_ALL_LAYERS_REFUSAL.into());
    }
    Ok(())
}

/// The Blur brush's Gaussian sigma in pixels: a visible softening under one
/// pass at full strength that still leaves room to build up with more passes,
/// which is how the tool is used. Fixed rather than tied to the tip size, so
/// Size chooses where the brush acts and Strength how far.
pub(crate) const BLUR_BRUSH_SIGMA: f64 = 2.0;
/// The Sharpen brush's unsharp-mask sigma: one pixel, the scale of the edge
/// detail the tool is for; a larger sigma would raise local contrast instead.
pub(crate) const SHARPEN_BRUSH_SIGMA: f64 = 1.0;
/// The Sharpen brush's unsharp-mask amount at full strength: the detail
/// `rgb − blur` added once (100%).
pub(crate) const SHARPEN_BRUSH_AMOUNT: f32 = 1.0;

/// How many pixels past a rect the kind's target reads on each side at 8 and
/// 16 bits: [`halo_in`] at `SampleRange::Unit`, the Gaussian's half-width
/// `ceil(3·sigma)`, 0 for Sponge.
pub(crate) fn halo(kind: RetouchBrushKind) -> u32 {
    halo_in(kind, &SampleRange::Unit)
}

/// How many pixels past a rect the kind's target reads on each side in
/// `range`: the half-width of the taps [`targets_in`] sums
/// (`gaussian_taps_in`, so the two cannot disagree), 0 for Sponge. Taps that
/// cannot be allocated answer 0 here and refuse in [`targets_in`].
pub(crate) fn halo_in(kind: RetouchBrushKind, range: &SampleRange) -> u32 {
    let sigma = match kind {
        RetouchBrushKind::Blur => BLUR_BRUSH_SIGMA,
        RetouchBrushKind::Sharpen => SHARPEN_BRUSH_SIGMA,
        RetouchBrushKind::Saturate | RetouchBrushKind::Desaturate => return 0,
    };
    gaussian_taps_in(sigma, range).map_or(0, |taps| (taps.len() / 2) as u32)
}

fn luma(p: [f32; 4]) -> f32 {
    LUMA_R * p[0] + LUMA_G * p[1] + LUMA_B * p[2]
}

/// The alpha-weighted Gaussian of the straight colour over a `w × h` rect in
/// `range` (its taps): rows first into a `(h + 2·half)`-row strip, then
/// columns.
fn blurred(
    sigma: f64,
    (w, h): (usize, usize),
    range: &SampleRange,
    read: &impl Fn(i64, i64) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<Vec<[f32; 3]>, String> {
    let taps = gaussian_taps_in(sigma, range)?;
    let half = taps.len() / 2;
    let rows = h + 2 * half;
    let mut strip: Vec<[f64; 4]> = reserved_vec(w * rows)?;
    for row in 0..rows {
        job.check()?;
        let y = row as i64 - half as i64;
        for x in 0..w {
            let mut acc = [0.0_f64; 4];
            for (j, k) in taps.iter().enumerate() {
                let p = read(x as i64 + j as i64 - half as i64, y);
                let weight = k * f64::from(p[3]);
                for c in 0..3 {
                    acc[c] += weight * f64::from(p[c]);
                }
                acc[3] += weight;
            }
            strip.push(acc);
        }
    }
    let mut out = reserved_vec(w * h)?;
    for y in 0..h {
        job.check()?;
        for x in 0..w {
            let mut acc = [0.0_f64; 4];
            for (j, k) in taps.iter().enumerate() {
                let p = strip[(y + j) * w + x];
                for c in 0..4 {
                    acc[c] += k * p[c];
                }
            }
            out.push(if acc[3] > 0.0 {
                [
                    (acc[0] / acc[3]) as f32,
                    (acc[1] / acc[3]) as f32,
                    (acc[2] / acc[3]) as f32,
                ]
            } else {
                let p = read(x as i64, y as i64);
                [p[0], p[1], p[2]]
            });
        }
    }
    Ok(out)
}

/// The kind's target colour for every pixel of a `dimensions` rect at 8 and
/// 16 bits: [`targets_in`] at `SampleRange::Unit`, with `read` over
/// [`halo`]'s reach.
pub(crate) fn targets(
    kind: RetouchBrushKind,
    vibrancy: Vibrancy<'_>,
    dimensions: (u32, u32),
    read: impl Fn(i64, i64) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<Vec<[f32; 3]>, String> {
    targets_in(kind, vibrancy, dimensions, &SampleRange::Unit, read, job)
}

/// The kind's target colour for every pixel of a `dimensions` rect in
/// `range`, row-major. `read(x, y)` answers the straight sample at
/// rect-relative `(x, y)`, for `x` in `−halo..width + halo` and `y` in
/// `−halo..height + halo` with `halo` [`halo_in`]'s for the same range; the
/// caller clamps positions outside its layer to the layer's edge. Sharpen and
/// Saturate finish with `range.color`: clamped to 0…1 at `Unit`, kept as
/// computed at `Extended`. Only Saturate and Desaturate read `vibrancy`
/// (module doc, Vibrance).
pub(crate) fn targets_in(
    kind: RetouchBrushKind,
    vibrancy: Vibrancy<'_>,
    dimensions: (u32, u32),
    range: &SampleRange,
    read: impl Fn(i64, i64) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<Vec<[f32; 3]>, String> {
    let (w, h) = (dimensions.0 as usize, dimensions.1 as usize);
    // `range.color` over a value computed in f32: the widening and the
    // narrowing are exact, so `Unit` is the f32 clamp it replaced, bit for bit.
    let finish = |v: f32| range.color(f64::from(v)) as f32;
    match kind {
        RetouchBrushKind::Blur => blurred(BLUR_BRUSH_SIGMA, (w, h), range, &read, job),
        RetouchBrushKind::Sharpen => {
            let mut out = blurred(SHARPEN_BRUSH_SIGMA, (w, h), range, &read, job)?;
            for (i, target) in out.iter_mut().enumerate() {
                job.every(i)?;
                let p = read((i % w) as i64, (i / w) as i64);
                for c in 0..3 {
                    target[c] = finish(p[c] + SHARPEN_BRUSH_AMOUNT * (p[c] - target[c]));
                }
            }
            Ok(out)
        }
        RetouchBrushKind::Saturate | RetouchBrushKind::Desaturate => {
            let slider = sponge_slider(kind);
            let mut out = reserved_vec(w * h)?;
            for i in 0..w * h {
                job.every(i)?;
                let p = read((i % w) as i64, (i / w) as i64);
                let rgb = [p[0], p[1], p[2]];
                out.push(match vibrancy {
                    Vibrancy::Off => {
                        let y = luma(p);
                        if kind == RetouchBrushKind::Desaturate {
                            [y; 3]
                        } else {
                            std::array::from_fn(|c| finish(y + 2.0 * (p[c] - y)))
                        }
                    }
                    Vibrancy::Encoded => slider.vibrance_srgb(rgb),
                    Vibrancy::Scaled(encoding) => encoding.scaled(rgb, |c| slider.vibrance_srgb(c)),
                    Vibrancy::Bound(bound) => bound.apply(rgb),
                    Vibrancy::Gray(m) if kind == RetouchBrushKind::Desaturate => [m; 3],
                    Vibrancy::Gray(m) => std::array::from_fn(|c| finish(m + 2.0 * (p[c] - m))),
                    Vibrancy::Lab(n) if kind == RetouchBrushKind::Desaturate => [p[0], n, n],
                    Vibrancy::Lab(n) => [
                        p[0],
                        finish(n + 2.0 * (p[1] - n)),
                        finish(n + 2.0 * (p[2] - n)),
                    ],
                });
            }
            Ok(out)
        }
    }
}

/// `source` moved toward `target` by `amount` (stroke coverage × strength, in
/// [0, 1]): `rgb + amount·(target − rgb)`. Alpha is `source`'s.
pub(crate) fn mix(source: [f32; 4], target: [f32; 3], amount: f32) -> [f32; 4] {
    [
        source[0] + amount * (target[0] - source[0]),
        source[1] + amount * (target[1] - source[1]),
        source[2] + amount * (target[2] - source[2]),
        source[3],
    ]
}

/// `layer` moved toward `target` by `amount` in [0, 1] as one premultiplied
/// mix of all four channels (Sample All Layers' write, module doc), in f32:
/// `α = (1 − t)·α_l + t·α_t` and each colour `((1 − t)·c_l·α_l + t·c_t·α_t) /
/// α`, so a transparent side contributes no colour. `doc_retouch_brush::
/// history_brush_layer` holds the master copy of this mix. Where `α` is 0
/// the layer's colour is kept with alpha 0. `frozen` (a transparency lock)
/// keeps `layer`'s alpha, and an alpha-0 `layer` pixel is returned as it is.
pub(crate) fn mix_over(layer: [f32; 4], target: [f32; 4], amount: f32, frozen: bool) -> [f32; 4] {
    if frozen && layer[3] == 0.0 {
        return layer;
    }
    let t = amount;
    let alpha = (1.0 - t) * layer[3] + t * target[3];
    let mut out = layer;
    if alpha > 0.0 {
        for c in 0..3 {
            out[c] = ((1.0 - t) * layer[c] * layer[3] + t * target[c] * target[3]) / alpha;
        }
    }
    out[3] = if frozen { layer[3] } else { alpha };
    out
}
