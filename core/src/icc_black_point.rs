//! Black points: the darkest colour a profile can reproduce, which ISO
//! 18619's black-point compensation maps one onto the other
//! (`icc_intent::bpc_terms`). Little CMS is the reference CMM, so a
//! conversion here lands where lcms lands.
//!
//! # The source black (`cmsDetectBlackPoint`)
//!
//! * matrix/TRC and Gray — the profile's own device zero, pushed FORWARD
//!   through the profile (which is exactly what this crate can do; there is
//!   no curve inverter here);
//! * absolute colorimetric is asked as relative — lcms never runs BPC at
//!   absolute, and the two share one table pair;
//! * a LUT **v4** profile at perceptual or saturation — the fixed
//!   perceptual-reference-medium black, lcms's `cmsPERCEPTUAL_BLACK_X/Y/Z`
//!   (ICC.1:2010 Annex A's Y = 0.0034731);
//! * an OUTPUT-class (`prtr`) CMYK profile at relative colorimetric — the
//!   PERCEPTUAL black: Lab(0, 0, 0) through `B2A0`, then back through the
//!   relative `A2B1`. That discounts the press's ink limit, which a
//!   relative `B2A1` would otherwise bake into the black;
//! * every other LUT case — the DARKER COLORANT at the asked intent's own
//!   table: full ink for CMYK, device zero for RGB, through `A2B0` for
//!   perceptual (a v2 perceptual table has no fixed reference medium),
//!   `A2B1` for relative, `A2B2` for saturation, each with `icc_intent`'s
//!   table-0 fallback.
//!
//! Every measured black is then made NEUTRAL (a* = b* = 0 in Lab, then back
//! to XYZ) and its L* clipped, as lcms does: a press black has a cast that
//! BPC must not spread over the shadows, and a black lighter than mid grey
//! is a broken table, not a medium. The perceptual black is clipped to
//! L* ≤ 50. Every other measured black — a matrix/TRC or Gray device zero
//! and a LUT darker colorant alike — goes through lcms's
//! `BlackPointAsDarkerColorant` ([`darker_colorant`]): clipped to L* ≤ 50,
//! and `[0, 0, 0]` when its L* is above 95 (a synthetic negative profile)
//! or below 0. lcms's matrix-shaper and Gray paths ARE that function, which
//! is why a lifted, tinted TRC toe is neutralized here too rather than
//! spread over the shadows with its cast. A well-formed matrix profile's
//! device zero is exactly `[0, 0, 0]`, whose L* is exactly 0, which comes
//! back as exactly `[0, 0, 0]`: no shipped matrix conversion moves. A
//! missing table answers `[0, 0, 0]` — lcms's answer for a profile it
//! cannot read in that direction — and BPC against sRGB's own zero black is
//! then the identity, so such a conversion runs relative colorimetric
//! uncompensated.
//!
//! # The destination black (`cmsDetectDestinationBlackPoint`)
//!
//! lcms's `cmscnvrt.c` (`ComputeConversion`) asks the DESTINATION side of
//! every black-point-compensated step a different question: not "what does
//! the darkest colorant measure" but "where does the profile's own round
//! trip actually bottom out" — Adobe's published algorithm, which lcms
//! ports. [`destination_black_point`] answers it for a document or
//! conversion destination, [`proof_destination_black_point`] for a proof
//! profile. A matrix/TRC profile has no table to round-trip through and
//! answers its source black, so a matrix destination is unchanged by
//! construction. For a LUT profile (lcms master `src/cmssamp.c`):
//!
//! 1. Absolute is asked as relative. A Gray `kTRC` profile, a v4 profile at
//!    perceptual or saturation, or one with no PCS → device table at all
//!    (after the table-0 fallback) answers the source black — lcms's own
//!    "handle as input" exits.
//! 2. The initial guess is the source black's Lab at relative colorimetric,
//!    Lab 0 at perceptual and saturation.
//! 3. A 256-point L* ramp at the guess's a*/b* (each clamped to ±50) runs
//!    Lab → device at the intent → Lab at relative, the round trip a print
//!    actually takes; the output ramp is made monotone from the top down.
//! 4. At relative, a round trip that is straight (within 4 L*) everywhere
//!    above the lowest fifth of its range answers the guess unchanged.
//! 5. Otherwise the shadow section — normalized output in [0.1, 0.5) at
//!    relative, [0.03, 0.25) at perceptual and saturation — is fitted with a
//!    least-squares quadratic, and its root (clamped to [0, 50], and to 0
//!    below) is the black's L*, at the guess's a*/b*.
//!
//! Any failure on the way — a table that cannot answer, an output ramp that
//! does not rise, fewer than three shadow points — is lcms's `FALSE` with a
//! zero black: `[0, 0, 0]`.
//!
//! SOURCE (`black_point` / `proof_black_point`, `cmsDetectBlackPoint`): matrix/Gray device
//! zero forward and every LUT darker colorant through `darker_colorant` (lcms's
//! `BlackPointAsDarkerColorant`: neutral, L* ≤ 50, `[0, 0, 0]` outside 0…95 — a lifted, tinted TRC
//! toe included, while a well-formed matrix black stays exactly zero); absolute asked as
//! relative; v4 perceptual/saturation the fixed `PERCEPTUAL_BLACK`. DESTINATION
//! (`destination_black_point` / `proof_destination_black_point`, `cmsDetectDestinationBlackPoint`
//! from lcms master `src/cmssamp.c`): a matrix profile is its source black; a LUT profile's
//! 256-point Lab → device → Lab ramp at the intent, made monotone, keeps the source black when
//! straight above its shadows (relative only) and is otherwise fitted with lcms's least-squares
//! quadratic (`icc_transform::invert`, the crate's one 3×3 inverse), Gray `kTRC` / v4
//! perceptual-saturation / no B2A answering the source black. Every BPC DESTINATION argument comes
//! from a destination function (`ColorPipeline`, both soft-proof legs, the flat open's sRGB);
//! sources are unchanged.

use crate::icc::IccProfile;
use crate::icc_intent::RenderingIntent;
use crate::icc_lut::{self, DeviceSpace, ProofProfile, D50};
use crate::icc_transform::{self, MatrixTrc};

/// The perceptual reference medium's black point, Little CMS's
/// `cmsPERCEPTUAL_BLACK_X/Y/Z` (Y is ICC.1:2010 Annex A's 0.0034731). A v4
/// perceptual or saturation table is written to land on it, so detecting it
/// by round trip would measure the table's rounding instead.
const PERCEPTUAL_BLACK: [f64; 3] = [0.00336, 0.0034731, 0.00287];

/// lcms's clip on a measured black: anything lighter than mid grey is a
/// broken table rather than a medium.
const BLACK_LSTAR_MAX: f64 = 50.0;

/// lcms's `BlackPointAsDarkerColorant` answers zero for a darker colorant
/// lighter than this (a synthetic negative profile) or below L* 0.
const DARKER_COLORANT_LSTAR_ZERO: f64 = 95.0;

/// The destination ramp's length: lcms samples L* 0…100 at 256 points.
const RAMP_POINTS: usize = 256;

/// lcms's "nearly straight midrange" test ignores the lowest fifth of the
/// round trip's output range — the toe the fit is for.
const STRAIGHT_SHADOW_FRACTION: f64 = 0.2;

/// …and calls the rest straight when every point is within 4 L* of the
/// identity.
const STRAIGHT_MAX_DELTA: f64 = 4.0;

/// lcms clamps the ramp's a*/b* to ±50, so a wildly tinted guess cannot
/// walk the ramp out of every table's gamut.
const RAMP_AB_LIMIT: f64 = 50.0;

/// The normalized-output window the quadratic is fitted over at relative
/// colorimetric: past the flat black section, short of the straight line.
const RELATIVE_FIT_WINDOW: (f64, f64) = (0.1, 0.5);

/// The same window at perceptual and saturation, whose tables compress the
/// shadows into a much shorter toe.
const PERCEPTUAL_FIT_WINDOW: (f64, f64) = (0.03, 0.25);

/// lcms's test for a vanishing quadratic or linear coefficient in the fit.
const FIT_EPSILON: f64 = 1e-10;

/// The XYZ of a profile's darkest reproducible colour, under `intent`
/// (module doc, "The source black"). `None` when the profile is neither
/// matrix/TRC nor a LUT profile this build reads.
pub fn black_point(profile: &IccProfile, intent: RenderingIntent) -> Option<[f64; 3]> {
    if let Some(model) = profile.model() {
        return Some(matrix_black(model));
    }
    profile.lut().map(|lut| lut_black(lut, intent))
}

/// The same question asked of a [`ProofProfile`] — the proof side, which is
/// not an `IccProfile` because a document may not carry Gray or CMYK.
/// `[0, 0, 0]` when the table the rule reads is missing (module doc).
pub fn proof_black_point(profile: &ProofProfile, intent: RenderingIntent) -> [f64; 3] {
    lut_black(profile, intent)
}

/// The black a conversion INTO `profile` compensates onto (module doc, "The
/// destination black"). A matrix/TRC profile answers [`black_point`];
/// `None` exactly when [`black_point`] is `None`.
pub fn destination_black_point(profile: &IccProfile, intent: RenderingIntent) -> Option<[f64; 3]> {
    if profile.model().is_some() {
        return black_point(profile, intent);
    }
    profile.lut().map(|lut| lut_destination_black(lut, intent))
}

/// [`destination_black_point`] asked of a [`ProofProfile`] — the soft
/// proof's simulated medium, which is the destination of the proof's first
/// leg.
pub fn proof_destination_black_point(profile: &ProofProfile, intent: RenderingIntent) -> [f64; 3] {
    lut_destination_black(profile, intent)
}

fn matrix_black(model: &MatrixTrc) -> [f64; 3] {
    // Device zero pushed forward: for a well-formed profile this is
    // [0, 0, 0], and for one whose TRC has a lifted toe it is not — and is
    // then neutralized and clipped like every other darker colorant.
    darker_colorant(model.to_pcs_xyz64([0.0, 0.0, 0.0]))
}

/// lcms's `BlackPointAsDarkerColorant` tail: a measured darkest colorant
/// made neutral with L* ≤ 50, or `[0, 0, 0]` when its L* is outside
/// 0…95 (or not a number).
fn darker_colorant(xyz: [f64; 3]) -> [f64; 3] {
    let lstar = icc_lut::xyz_to_lab(xyz)[0];
    if !(0.0..=DARKER_COLORANT_LSTAR_ZERO).contains(&lstar) {
        return [0.0, 0.0, 0.0];
    }
    icc_lut::lab_to_xyz([lstar.min(BLACK_LSTAR_MAX), 0.0, 0.0])
}

/// `cmsDetectBlackPoint` over a LUT (or Gray) profile, in the module doc's
/// order. Which tables exist is `ProofProfile::to_pcs`/`from_pcs`'s
/// fallback, not lcms's stricter per-intent tag test.
fn lut_black(profile: &ProofProfile, intent: RenderingIntent) -> [f64; 3] {
    if let Some(curve) = profile.gray_curve() {
        let y = curve.eval(0.0);
        return darker_colorant([D50[0] * y, D50[1] * y, D50[2] * y]);
    }
    let relative = RenderingIntent::RelativeColorimetric;
    let intent = if intent.is_absolute() {
        relative
    } else {
        intent
    };
    if profile.is_v4()
        && matches!(
            intent,
            RenderingIntent::Perceptual | RenderingIntent::Saturation
        )
    {
        return PERCEPTUAL_BLACK;
    }
    let perceptual_black =
        intent == relative && profile.is_output_class() && profile.device() == DeviceSpace::Cmyk;
    if perceptual_black {
        // lcms's `BlackPointUsingPerceptualBlack`: Lab 0 through the
        // perceptual B2A0, back through the relative A2B1, made neutral and
        // clipped to L* 50 — with no L* 95 zero, which is the darker
        // colorant's rule alone.
        let measured = profile
            .from_xyz([0.0, 0.0, 0.0], RenderingIntent::Perceptual)
            .and_then(|(device, count)| profile.to_xyz(&device[..count], relative));
        return match measured {
            Some(xyz) => {
                let lstar = icc_lut::xyz_to_lab(xyz)[0];
                icc_lut::lab_to_xyz([lstar.min(BLACK_LSTAR_MAX), 0.0, 0.0])
            }
            None => [0.0, 0.0, 0.0],
        };
    }
    let darkest: &[f64] = match profile.device() {
        DeviceSpace::Cmyk => &[1.0, 1.0, 1.0, 1.0],
        _ => &[0.0, 0.0, 0.0],
    };
    match profile.to_xyz(darkest, intent) {
        Some(xyz) => darker_colorant(xyz),
        None => [0.0, 0.0, 0.0],
    }
}

/// `cmsDetectDestinationBlackPoint` over a LUT (or Gray) profile, the module
/// doc's five steps. The fixed-size ramps are lcms's own arrays.
fn lut_destination_black(profile: &ProofProfile, intent: RenderingIntent) -> [f64; 3] {
    const ZERO: [f64; 3] = [0.0, 0.0, 0.0];
    let relative = RenderingIntent::RelativeColorimetric;
    let intent = if intent.is_absolute() {
        relative
    } else {
        intent
    };
    let v4_reference = profile.is_v4()
        && matches!(
            intent,
            RenderingIntent::Perceptual | RenderingIntent::Saturation
        );
    // lcms's exits to `cmsDetectBlackPoint`: no CLUT to round-trip through
    // in the output direction (a Gray `kTRC`; a profile with no B2A, after
    // the table-0 fallback that is this crate's stated table rule), or the
    // v4 perceptual reference medium.
    if profile.gray_curve().is_some() || v4_reference || profile.from_pcs(intent).is_none() {
        return lut_black(profile, intent);
    }
    let initial = if intent == relative {
        icc_lut::xyz_to_lab(lut_black(profile, relative))
    } else {
        [0.0, 0.0, 0.0]
    };
    let a = initial[1].clamp(-RAMP_AB_LIMIT, RAMP_AB_LIMIT);
    let b = initial[2].clamp(-RAMP_AB_LIMIT, RAMP_AB_LIMIT);
    let mut input = [0.0f64; RAMP_POINTS];
    let mut output = [0.0f64; RAMP_POINTS];
    for (l, (inp, out)) in input.iter_mut().zip(output.iter_mut()).enumerate() {
        let lstar = l as f64 * 100.0 / (RAMP_POINTS - 1) as f64;
        // lcms's `CreateRoundtripXForm`: Lab → the profile at `intent` →
        // Lab at relative colorimetric.
        let round_trip = profile
            .from_xyz(icc_lut::lab_to_xyz([lstar, a, b]), intent)
            .and_then(|(device, count)| profile.to_xyz(&device[..count], relative));
        let Some(xyz) = round_trip else {
            return ZERO;
        };
        *inp = lstar;
        *out = icc_lut::xyz_to_lab(xyz)[0];
    }
    // Monotone from the top down, lcms's `for (l = 254; l > 0; --l)`.
    for l in (1..RAMP_POINTS - 1).rev() {
        output[l] = output[l].min(output[l + 1]);
    }
    let (min_l, max_l) = (output[0], output[RAMP_POINTS - 1]);
    // lcms's `!(outRamp[0] < outRamp[255])`: a ramp that does not rise
    // (or a NaN) has no black to find.
    if min_l.partial_cmp(&max_l) != Some(std::cmp::Ordering::Less) {
        return ZERO;
    }
    if intent == relative {
        let shadow = min_l + STRAIGHT_SHADOW_FRACTION * (max_l - min_l);
        let straight = input
            .iter()
            .zip(output.iter())
            .all(|(&i, &o)| i <= shadow || (i - o).abs() < STRAIGHT_MAX_DELTA);
        if straight {
            return icc_lut::lab_to_xyz(initial);
        }
    }
    let (lo, hi) = if intent == relative {
        RELATIVE_FIT_WINDOW
    } else {
        PERCEPTUAL_FIT_WINDOW
    };
    let mut x = [0.0f64; RAMP_POINTS];
    let mut y = [0.0f64; RAMP_POINTS];
    let mut n = 0;
    for (&i, &o) in input.iter().zip(output.iter()) {
        let normalized = (o - min_l) / (max_l - min_l);
        if normalized >= lo && normalized < hi {
            x[n] = i;
            y[n] = normalized;
            n += 1;
        }
    }
    if n < 3 {
        return ZERO;
    }
    let lstar = root_of_least_squares_quadratic(&x[..n], &y[..n]).max(0.0);
    icc_lut::lab_to_xyz([lstar, initial[1], initial[2]])
}

/// lcms's `RootOfLeastSquaresFitQuadraticCurve`: the least-squares
/// `y = a·x² + b·x + c` through the points, solved from its normal equations
/// with the crate's one 3×3 inverse (`icc_transform::invert`), and the root
/// where the fitted shadow curve leaves zero, clamped to [0, 50]. 0 for
/// fewer than four points, a singular system or no real root. The linear
/// case is lcms master's `cmsmax(0, cmsmin(50, −c/b))` clamp of `−c/b`
/// into [0, 50].
fn root_of_least_squares_quadratic(x: &[f64], y: &[f64]) -> f64 {
    if x.len() < 4 {
        return 0.0;
    }
    let mut sums = [0.0f64; 7]; // Σx, Σx², Σx³, Σx⁴, Σy, Σyx, Σyx²
    for (&xn, &yn) in x.iter().zip(y) {
        sums[0] += xn;
        sums[1] += xn * xn;
        sums[2] += xn * xn * xn;
        sums[3] += xn * xn * xn * xn;
        sums[4] += yn;
        sums[5] += yn * xn;
        sums[6] += yn * xn * xn;
    }
    let n = x.len() as f64;
    let [sx, sx2, sx3, sx4, sy, syx, syx2] = sums;
    // The normal-equation matrix is symmetric, so its columns are its rows
    // and `invert`'s column convention needs no transpose.
    let m = [[n, sx, sx2], [sx, sx2, sx3], [sx2, sx3, sx4]];
    let v = [sy, syx, syx2];
    let Some(inverse) = icc_transform::invert(m) else {
        return 0.0;
    };
    let solve = |r: usize| inverse[r][0] * v[0] + inverse[r][1] * v[1] + inverse[r][2] * v[2];
    let (c, b, a) = (solve(0), solve(1), solve(2));
    if a.abs() < FIT_EPSILON {
        if b.abs() < FIT_EPSILON {
            return 0.0;
        }
        return (-c / b).clamp(0.0, BLACK_LSTAR_MAX);
    }
    let d = b * b - 4.0 * a * c;
    if d <= 0.0 {
        return 0.0;
    }
    ((-b + d.sqrt()) / (2.0 * a)).clamp(0.0, BLACK_LSTAR_MAX)
}
