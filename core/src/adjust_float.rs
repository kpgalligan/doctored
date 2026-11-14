//! The 32-bit float document's adjustment forms: which
//! closed form each adjustment takes on a float scene's stored LINEAR
//! values, derived once at parse (`PrecisionAdjustment::parse`) and never
//! persisted. A thin adapter: it names `Adjustment` and `IccProfile`, so it
//! is not a kernel module, and every form runs its master's own statements
//! in the kernel modules (`adjust_levels`, `adjust_mix`,
//! `adjust_white_balance`, `adjust_tone`, `adjust_calibrated`,
//! `adjust_curves`, `adjust_color`, `adjust_color_balance`, `adjust_lut`).
//!
//! # The forms, fitted to Photoshop 26.10.0
//!
//! Receipt: `doctored-fixtures/photoshop/float-adjust-473/take2/`
//! (`judged.json` authoritative), driven by
//! `scripts/photoshop-probe/float_adjust.py`. Each form was fitted from the
//! receipt's TIFFs; the error is the max over every patch of every set on
//! both profiles (sRGB and ROMM) of `|PS32 − form| / max(1, |form|)`, against
//! a tolerance of 2e-3.
//!
//! | op | form on stored linear values | max err |
//! |---|---|---|
//! | Levels | the master stage with every clamp removed, `sgn(t)·abs(t)^(1/γ)` | 1.5e-6 |
//! | Black & White (Tint off) | the master decomposition, `max(0, gray)`, no upper clamp | 8.7e-8 |
//! | Black & White (Tint on) | the grey `max(0, gray)` capped at 16, the tint's numbers set to it (SetLum, ClipColor at 16), `+ 1/510` when a branch fired | take6 0.003 tolerance units (float_adjust_take6.py; the cap at 16 is take6's fit, take7 stalled) |
//! | Channel Mixer | the master matrix, coefficients in 1/1024 steps, constant `trunc(512·K)/1024`, no clamp | 4.8e-8 |
//! | Photo Filter | the density multiply on white-normalized XYZ in the profile's NATIVE basis, then Y held | 4.2e-4 |
//! | `bcs` | the master affine with no clamp (Photoshop's legacy Brightness/Contrast; pivot 0.5 vs Photoshop's 127/255) | 8.4e-4 |
//! | `bcs` modern (`legacy: false`) | the master `brightness_contrast_modern` on the stored values, no clamp: a value outside [0, 1] passes the power stages unchanged, the saturation scale is unclamped | not fitted (own oracle) |
//! | Exposure | per stored channel `sgn(u)·abs(u)^(1/γ)`, `u = v·2^e + o`, no reference-basis matrices, both evaluations | 9.5e-8 |
//! | Curves (point form) | the master natural spline, no clamp, continued along the end slope above the last point; flat below the first, or along its slope when it sits at 0 | 1.9e-7 |
//! | Curves (map / pencil form) | each map piecewise linear on `X = 255·v`, its end segments continued along their slopes, the channel map before the master, no clamp | take3 0.98 tolerance units (float_adjust_take3.py) |
//! | Hue/Saturation (Colorize; master Lightness alone; master Hue alone) | C32 colorize; per-component two-sided lerp, a component past the target held; hexcone rotation by a shift on a 1/24576-turn grid, truncated toward 0 | take3 max 0.11 / 0.60, take4 0.74 tolerance units (hue_sat_parity.py, hue_sat_take4.py) |
//! | Color Balance | the 16-bit per-channel Levels curve in range, continued along its 4096-stage end slopes outside (`stages`) | take6 0.74 / 4.3e-4 tolerance units in range / beyond (color_balance_take6.py) |
//! | Color Lookup (a 3D table, domain 0…1, entries in 0…1) | the 8/16-bit `CubeLut::apply` on the stored values: tetrahedral, an input above 1 taken as 1 and one below 0 as 0, then the strength lerp | take3 0.004 tolerance units in range, above 1 and below 0 (float_adjust_take3.py) |
//!
//! A calibrated (`document_v2`) White Balance keeps
//! `Calibrated::apply_linear`.
//!
//! # The forms of decision A (chosen, not fitted)
//!
//! | op | form on stored linear values |
//! |---|---|
//! | Hue Rotate, Sepia | the master matrix (`SEPIA`, the hue rotation's own), no clamp ([`FloatForm::Matrix`]) |
//! | Grayscale | `adjust_math::luma` on all three channels, no clamp |
//! | Threshold, Posterize, Gradient Map | display-mapped: `D(master(E(clamp(v,0,1))))`, above white classified as white |
//! | Vibrance, Selective Color, Replace Color, Hue/Saturation with no closed form | display-scaled: `m·(D(master(E(max(c,0)))) + min(c,0))`, `m = max(1, max rgb)`, `c = rgb/m` |
//! | Color Lookup, any other table (1D, any domain, entries outside 0…1, abstract-baked) | `CubeLut::apply` on the stored values, as the 3D form |
//!
//! The master of both display forms is `Adjustment::apply_at(c, 0.0, xy)`,
//! the 8/16-bit entry; E is the document profile's forward curve
//! (`PrecisionToneEncoder::new` of its model) and D its decode
//! (`MatrixTrc::decode_rgb`), both built once per form here
//! (`display_encoding::through_display`). Hue/Saturation takes the
//! display form exactly when its closed form (`apply_float`) does not
//! evaluate: Saturation, Hue with another master slider, or a hue band.
//!
//! Refused by decision, by name, each with its reason in
//! `float_document`'s operator table: Invert, Shadows/Highlights and the
//! legacy-evaluation White Balance; a display form or Photo Filter on a
//! profile with no matrix/TRC model refuses by name.
//!
//! # Photo Filter's basis
//!
//! `M` is the profile's D50 matrix and `D` its native→D50 adaptation
//! (`adjust_white_balance::native_adaptation`: the `chad` tag, else Bradford
//! from a non-D50 `wtpt`, else the identity). The filter colour's ratio is
//! `M·decode(color) / D50`, its hex being the document's ENCODED numbers as
//! every sheet writes them (`adjust_photo_filter::pcs_ratio`); a pixel goes
//! `x = D⁻¹·M·v`, through `PhotoFilter::filter_native_xyz`, and back by
//! `M⁻¹·D`. 8/16 bits use the D50 PCS basis instead, with no adaptation
//! (`adjust_photo_filter`): Photoshop renders the depths differently.

use crate::adjust::{Adjustment, SEPIA};
use crate::adjust_math::{posterize, threshold};
use crate::adjust_mix::{brightness_contrast_modern, brightness_contrast_saturation};
use crate::adjust_tone::Exposure;
use crate::adjust_white_balance::{mat_mul, mat_vec, native_adaptation};
use crate::display_encoding::{through_display, DisplayRule};
use crate::float_document::{
    refused_sentence, REFUSED_INVERT, REFUSED_LEGACY_WHITE_BALANCE, REFUSED_SHADOWS_HIGHLIGHTS,
};
use crate::icc::{IccBytes, IccProfile};
use crate::icc_transform::{MatrixTrc, PrecisionToneEncoder};

/// Photo Filter's prepared basis: linear RGB → native XYZ, its way back,
/// and the filter colour's white-normalized XYZ ratio.
#[derive(Clone)]
pub(crate) struct NativeFilter {
    to_native: [[f64; 3]; 3],
    from_native: [[f64; 3]; 3],
    ratio: [f64; 3],
}

/// The form an adjustment takes in a 32-bit float document.
#[derive(Clone)]
pub(crate) enum FloatForm {
    Levels,
    BlackAndWhite,
    ChannelMixer,
    Bcs,
    /// `brightness_contrast_modern` on the stored values.
    BcsModern,
    PhotoFilter(NativeFilter),
    /// `Exposure::apply_linear` per stored channel, legacy or calibrated.
    Exposure,
    /// `CurveLuts::apply_float`: the point form's natural spline, or the
    /// map form's continued piecewise-linear maps.
    Curves,
    /// `HueSaturation::apply_float`: Colorize, or master Lightness alone.
    HueSaturation,
    /// `ColorBalance::apply_float`: every setting.
    ColorBalance,
    /// `CubeLut::apply`, the 8/16-bit entry, on every table.
    ColorLookup,
    Calibrated,
    /// A 3×3 matrix on the stored values with no clamp: Hue Rotate's own,
    /// or [`SEPIA`].
    Matrix([[f32; 3]; 3]),
    /// `adjust_math::luma` on all three channels, no clamp.
    Grayscale,
    /// The master through the display copy (`display_encoding`, A):
    /// `encoder` is E and `model`'s decode D, built once here (boxed: the
    /// profile model would otherwise size every form).
    Display {
        rule: DisplayRule,
        encoder: PrecisionToneEncoder,
        model: Box<MatrixTrc>,
    },
    /// The complete refusal sentence.
    Refused(String),
}

/// The form `kernel` takes against the document `profile`.
pub(crate) fn prepare(kernel: &Adjustment, profile: &IccProfile) -> FloatForm {
    match kernel {
        Adjustment::Levels(_) => FloatForm::Levels,
        Adjustment::ChannelMixer(_) => FloatForm::ChannelMixer,
        Adjustment::Bcs { .. } => FloatForm::Bcs,
        Adjustment::BcsModern { .. } => FloatForm::BcsModern,
        Adjustment::BlackAndWhite(_) => FloatForm::BlackAndWhite,
        Adjustment::PhotoFilter(pf) => match native_filter(pf.filter().color(), profile) {
            Some(filter) => FloatForm::PhotoFilter(filter),
            None => FloatForm::Refused(refused_sentence(
                "the photo_filter adjustment on a profile without an RGB matrix/TRC model",
            )),
        },
        // Both the point and the map (pencil) form have a float form
        // exactly when `apply_float` evaluates; a parse that kept no
        // continuous form refuses.
        Adjustment::Curves(luts) if luts.apply_float([0.0; 3]).is_none() => {
            FloatForm::Refused(refused_sentence("the curves adjustment"))
        }
        Adjustment::Curves(_) => FloatForm::Curves,
        // Colorize, master Lightness alone and master Hue alone have a
        // closed float form exactly when `apply_float` evaluates;
        // every other setting is display-scaled.
        Adjustment::HueSaturation(hs) if hs.apply_float([0.0; 3]).is_none() => {
            display(DisplayRule::Scaled, kernel, profile)
        }
        Adjustment::HueSaturation(_) => FloatForm::HueSaturation,
        Adjustment::ColorBalance(_) => FloatForm::ColorBalance,
        // Every table runs on the stored values.
        Adjustment::ColorLookup(_) => FloatForm::ColorLookup,
        Adjustment::Exposure(_) => FloatForm::Exposure,
        Adjustment::Calibrated(op) if op.exposure().is_some() => FloatForm::Exposure,
        Adjustment::Calibrated(_) => FloatForm::Calibrated,
        Adjustment::HueRotate { matrix } => FloatForm::Matrix(*matrix),
        Adjustment::Sepia => FloatForm::Matrix(SEPIA),
        Adjustment::Grayscale => FloatForm::Grayscale,
        Adjustment::Threshold { .. }
        | Adjustment::Posterize { .. }
        | Adjustment::GradientMap(_) => display(DisplayRule::Mapped, kernel, profile),
        Adjustment::Vibrance(_) | Adjustment::SelectiveColor(_) | Adjustment::ReplaceColor(_) => {
            display(DisplayRule::Scaled, kernel, profile)
        }
        // Refused by decision; `float_document`'s table rows give each reason.
        Adjustment::Invert => FloatForm::Refused(refused_sentence(REFUSED_INVERT)),
        Adjustment::ShadowsHighlights(_) => {
            FloatForm::Refused(refused_sentence(REFUSED_SHADOWS_HIGHLIGHTS))
        }
        Adjustment::WhiteBalance(_) => {
            FloatForm::Refused(refused_sentence(REFUSED_LEGACY_WHITE_BALANCE))
        }
    }
}

/// A display form of `kernel` under `profile` (module doc): E and D built
/// once from the profile's model. A profile with no matrix/TRC model, or
/// one whose curves the encoder refuses, refuses by name, as Photo Filter
/// does.
fn display(rule: DisplayRule, kernel: &Adjustment, profile: &IccProfile) -> FloatForm {
    let refused = || {
        FloatForm::Refused(refused_sentence(&format!(
            "the {} adjustment on a profile without an RGB matrix/TRC model",
            kernel.op_name()
        )))
    };
    let Some(model) = profile.model() else {
        return refused();
    };
    match PrecisionToneEncoder::new(model) {
        Ok(encoder) => FloatForm::Display {
            rule,
            encoder,
            model: Box::new(model.clone()),
        },
        Err(_) => refused(),
    }
}

/// Photo Filter's basis under `profile` (module doc). `None` without a
/// matrix/TRC model, or for an adaptation that cannot be inverted.
fn native_filter(color: [f32; 3], profile: &IccProfile) -> Option<NativeFilter> {
    let model = profile.model()?;
    let unit = |c: usize| std::array::from_fn::<f64, 3, _>(|k| f64::from(u8::from(k == c)));
    // `linear_to_pcs` of a unit vector is a COLUMN of M; `pcs_to_linear` of
    // one is a column of M⁻¹.
    let columns = [0, 1, 2].map(|c| model.linear_to_pcs(unit(c)));
    let inverse_columns = [0, 1, 2].map(|c| model.pcs_to_linear(unit(c)));
    let m: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|c| columns[c][i]));
    let m_inv: [[f64; 3]; 3] =
        std::array::from_fn(|i| std::array::from_fn(|c| inverse_columns[c][i]));
    let media_white = IccBytes::new(profile.bytes()).and_then(|icc| icc.media_white());
    let d = native_adaptation(profile.chromatic_adaptation(), media_white);
    // `invert` takes COLUMNS and returns ROWS of the inverse.
    let d_columns: [[f64; 3]; 3] = std::array::from_fn(|c| std::array::from_fn(|i| d[i][c]));
    let d_inv = crate::icc_transform::invert(d_columns)?;
    Some(NativeFilter {
        to_native: mat_mul(d_inv, m),
        from_native: mat_mul(m_inv, d),
        ratio: crate::adjust_photo_filter::pcs_ratio(model, color),
    })
}

/// `rgb` (stored linear values, any sign or magnitude) under `form`, which
/// [`prepare`] derived from this `kernel`. `xy` is the pixel's position,
/// read only by a display form's master (Gradient Map's dither), on the
/// convention `Adjustment::apply_at` states. `Err` is the refusal sentence.
pub(crate) fn apply(
    form: &FloatForm,
    kernel: &Adjustment,
    rgb: [f32; 3],
    xy: (i64, i64),
) -> Result<[f32; 3], String> {
    match (form, kernel) {
        (FloatForm::Refused(reason), _) => Err(reason.clone()),
        (FloatForm::Levels, Adjustment::Levels(levels)) => Ok(levels.apply_float(rgb)),
        (FloatForm::BlackAndWhite, Adjustment::BlackAndWhite(bw)) => Ok(bw.apply_float(rgb)),
        (FloatForm::ChannelMixer, Adjustment::ChannelMixer(cm)) => Ok(cm.apply_float(rgb)),
        (
            FloatForm::Bcs,
            Adjustment::Bcs {
                brightness,
                contrast_slope,
                saturation_scale,
            },
        ) => Ok(brightness_contrast_saturation(
            rgb,
            *brightness,
            *contrast_slope,
            *saturation_scale,
        )),
        (
            FloatForm::BcsModern,
            Adjustment::BcsModern {
                gamma,
                contrast_power,
                saturation_scale,
            },
        ) => Ok(brightness_contrast_modern(
            rgb,
            *gamma,
            *contrast_power,
            *saturation_scale,
        )),
        (FloatForm::PhotoFilter(filter), Adjustment::PhotoFilter(pf)) => {
            let x = mat_vec(filter.to_native, rgb.map(f64::from));
            let y = pf.filter().filter_native_xyz(x, filter.ratio);
            Ok(mat_vec(filter.from_native, y).map(|v| v as f32))
        }
        (FloatForm::Curves, Adjustment::Curves(luts)) => {
            luts.apply_float(rgb).ok_or_else(|| mismatch(kernel))
        }
        (FloatForm::HueSaturation, Adjustment::HueSaturation(hs)) => {
            hs.apply_float(rgb).ok_or_else(|| mismatch(kernel))
        }
        (FloatForm::ColorBalance, Adjustment::ColorBalance(cb)) => Ok(cb.apply_float(rgb)),
        (FloatForm::ColorLookup, Adjustment::ColorLookup(lut)) => Ok(lut.apply(rgb)),
        (FloatForm::Exposure, Adjustment::Exposure(op)) => Ok(exposure(op, rgb)),
        (FloatForm::Exposure, Adjustment::Calibrated(op)) => match op.exposure() {
            Some(op) => Ok(exposure(op, rgb)),
            None => Err(mismatch(kernel)),
        },
        (FloatForm::Calibrated, Adjustment::Calibrated(op)) => Ok(op.apply_linear(rgb)),
        (FloatForm::Matrix(matrix), _) => {
            Ok(matrix.map(|row| row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2]))
        }
        (FloatForm::Grayscale, Adjustment::Grayscale) => Ok([crate::adjust_math::luma(rgb); 3]),
        // The display forms run each op's master on the display value: the
        // unbound sRGB-curve Vibrance (its 8/16-bit form binds the document
        // TRC, `adjust_vibrance`) and the continuous Posterize bins (`top` 1,
        // `adjust_math::posterize`); every other op its own kernel.
        (
            FloatForm::Display {
                rule,
                encoder,
                model,
            },
            Adjustment::Vibrance(v),
        ) => Ok(through_display(*rule, rgb, encoder, model, |c| {
            v.op().apply(c)
        })),
        (
            FloatForm::Display {
                rule,
                encoder,
                model,
            },
            Adjustment::Posterize { levels, .. },
        ) => Ok(through_display(*rule, rgb, encoder, model, |c| {
            c.map(|v| posterize(v, *levels, 1))
        })),
        // The continuous Threshold luma (`top` 1, `adjust_math::threshold`):
        // the 16-bit form's 15-bit grid is Photoshop's 16-bit storage.
        (
            FloatForm::Display {
                rule,
                encoder,
                model,
            },
            Adjustment::Threshold { level, .. },
        ) => Ok(through_display(*rule, rgb, encoder, model, |c| {
            [threshold(c, *level, 1); 3]
        })),
        (
            FloatForm::Display {
                rule,
                encoder,
                model,
            },
            _,
        ) => Ok(through_display(*rule, rgb, encoder, model, |c| {
            kernel.apply_at(c, 0.0, xy)
        })),
        _ => Err(mismatch(kernel)),
    }
}

/// Photoshop's 32-bit Exposure: the master `Exposure::apply_linear` on each
/// stored channel, with no reference-basis matrices.
fn exposure(op: &Exposure, rgb: [f32; 3]) -> [f32; 3] {
    op.apply_linear(rgb.map(f64::from)).map(|v| v as f32)
}

/// `prepare` derived the form from this kernel, so a mismatch is a defect;
/// refuse by name rather than evaluate the wrong form.
fn mismatch(kernel: &Adjustment) -> String {
    refused_sentence(&format!("the {} adjustment", kernel.op_name()))
}
