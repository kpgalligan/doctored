//! Chromaticities for the float interchange containers: the xy primaries an
//! OpenEXR `chromaticities` attribute and a Radiance `PRIMARIES=` line state,
//! against the primaries of a matrix profile.
//!
//! A float document holds LINEAR light in its profile's primaries, and neither
//! container carries an ICC profile, so the two directions are:
//!
//! * **Out** — the file declares the document profile's primaries
//!   ([`of_model`], or the published constants for the two built-ins) and no
//!   sample is converted.
//! * **In** — a file within [`MATCH_TOLERANCE`] of [`REC709`] or
//!   [`DISPLAY_P3`] takes that built-in profile unchanged; any other set is
//!   converted once, in linear light, to sRGB primaries
//!   ([`conversion`]`(file, &REC709)`). An HDR still in [`BT2020`] takes the
//!   same conversion (`float_host_intake`), and its HLG luminance comes from
//!   [`luminance_row`].
//!
//! # Why the constants and not a derivation
//!
//! [`REC709`] and [`DISPLAY_P3`] are the published xy pairs (ITU-R BT.709-6
//! and SMPTE EG 432-1, both with the D65 white 0.3127, 0.3290). OpenEXR's own
//! default — the primaries a file without the attribute is in — is exactly
//! [`REC709`], so writing the constants makes a round trip through another
//! tool compare equal; primaries derived from a profile's D50-adapted columns
//! land within about 1e-4 of them and would not.
//!
//! # Why 0.002
//!
//! The same primaries reach a file through different roads (an ICC profile's
//! s15Fixed16 columns, a D50 adaptation and its inverse, a renderer's own
//! four-decimal table), and those differ in the fourth decimal. The nearest
//! distinct space a file is likely to declare is far outside that: Display P3
//! against Rec.709 differs by 0.04 in red x, and Adobe RGB against Rec.709 by
//! 0.09 in green x. 0.002 admits every rounding and none of the spaces.
//!
//! # The conversion
//!
//! `M_file` is the usual primaries matrix (each primary's XYZ at Y = 1,
//! scaled so their sum is the white), `M_srgb` the same construction over
//! [`REC709`], and the result is `M_srgb⁻¹ · B · M_file` with `B` the
//! Bradford adaptation between the two whites
//! (`adjust_white_balance::bradford`, the crate's one copy). It is applied to
//! linear samples as they are, so a negative component stays a negative
//! component.

use crate::adjust_white_balance::{bradford, mat_mul, mat_vec};
use crate::exr_float::Chromaticities;
use crate::icc_transform::{invert, MatrixTrc};

/// ITU-R BT.709 / sRGB primaries with D65: red, green, blue, white.
pub const REC709: Chromaticities = [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06], [0.3127, 0.3290]];

/// Display P3 (SMPTE EG 432-1 primaries, D65 white).
pub const DISPLAY_P3: Chromaticities = [
    [0.680, 0.320],
    [0.265, 0.690],
    [0.150, 0.060],
    [0.3127, 0.3290],
];

/// ITU-R BT.2020 / BT.2100 primaries with D65.
pub const BT2020: Chromaticities = [
    [0.708, 0.292],
    [0.170, 0.797],
    [0.131, 0.046],
    [0.3127, 0.3290],
];

/// How far apart two xy coordinates may be and still name the same space
/// (module doc).
pub const MATCH_TOLERANCE: f32 = 0.002;

/// True when every coordinate of `a` is within [`MATCH_TOLERANCE`] of `b`'s.
/// A non-finite coordinate matches nothing.
pub fn matches(a: &Chromaticities, b: &Chromaticities) -> bool {
    a.iter()
        .flatten()
        .zip(b.iter().flatten())
        .all(|(x, y)| (x - y).abs() <= MATCH_TOLERANCE)
}

/// `XYZ` as its xy chromaticity; a black or non-finite triple is (0, 0).
fn xy_of(xyz: [f64; 3]) -> [f32; 2] {
    let sum = xyz[0] + xyz[1] + xyz[2];
    if !sum.is_finite() || sum.abs() < 1e-12 {
        return [0.0, 0.0];
    }
    [(xyz[0] / sum) as f32, (xyz[1] / sum) as f32]
}

/// The NATIVE primaries and white of a matrix profile: its D50-adapted
/// columns through the inverse of `native_to_d50`
/// (`adjust_white_balance::native_adaptation` of the profile's `chad` and
/// media white). An adaptation that cannot be inverted is treated as the
/// identity, which states the D50-adapted primaries.
pub fn of_model(model: &MatrixTrc, native_to_d50: [[f64; 3]; 3]) -> Chromaticities {
    let unit = |c: usize| std::array::from_fn::<f64, 3, _>(|k| f64::from(u8::from(k == c)));
    // `invert` takes COLUMNS and returns ROWS of the inverse.
    let columns: [[f64; 3]; 3] =
        std::array::from_fn(|c| std::array::from_fn(|i| native_to_d50[i][c]));
    let back = invert(columns).unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    let native = |rgb: [f64; 3]| xy_of(mat_vec(back, model.linear_to_pcs(rgb)));
    [
        native(unit(0)),
        native(unit(1)),
        native(unit(2)),
        native([1.0; 3]),
    ]
}

/// An xy pair at Y = 1; `None` for a non-finite pair or `y = 0`.
fn xyz_of(xy: [f32; 2]) -> Option<[f64; 3]> {
    let (x, y) = (f64::from(xy[0]), f64::from(xy[1]));
    if !x.is_finite() || !y.is_finite() || y.abs() < 1e-9 {
        return None;
    }
    Some([x / y, 1.0, (1.0 - x - y) / y])
}

/// The linear RGB → XYZ matrix of a set of primaries, row-major, and its
/// white as XYZ. `None` when the primaries are not a basis.
fn rgb_to_xyz(c: &Chromaticities) -> Option<([[f64; 3]; 3], [f64; 3])> {
    let primaries = [xyz_of(c[0])?, xyz_of(c[1])?, xyz_of(c[2])?];
    let white = xyz_of(c[3])?;
    // `primaries[k]` is COLUMN k, which is what `invert` takes.
    let scale = mat_vec(invert(primaries)?, white);
    if scale.iter().any(|s| !s.is_finite()) {
        return None;
    }
    let m = std::array::from_fn(|i| std::array::from_fn(|k| primaries[k][i] * scale[k]));
    Some((m, white))
}

/// The matrix that takes linear RGB in `from`'s primaries to linear RGB in
/// `to`'s primaries, row-major (module doc, "The conversion", with `to` in
/// place of sRGB). `None` for a singular basis on either side: a zero `y`,
/// collinear primaries or a non-finite coordinate.
pub fn conversion(from: &Chromaticities, to: &Chromaticities) -> Option<[[f64; 3]; 3]> {
    let (source, from_white) = rgb_to_xyz(from)?;
    let (target, to_white) = rgb_to_xyz(to)?;
    let to_columns: [[f64; 3]; 3] = std::array::from_fn(|c| std::array::from_fn(|i| target[i][c]));
    let back = invert(to_columns)?;
    let adapted = mat_mul(bradford(from_white, to_white), source);
    let out = mat_mul(back, adapted);
    out.iter().flatten().all(|v| v.is_finite()).then_some(out)
}

/// The luminance (Y) row of `primaries`' RGB → XYZ matrix: the weights that
/// make linear RGB's luminance, summing to 1 at the white. `None` for a
/// singular basis.
pub fn luminance_row(primaries: &Chromaticities) -> Option<[f64; 3]> {
    rgb_to_xyz(primaries).map(|(m, _)| m[1])
}
