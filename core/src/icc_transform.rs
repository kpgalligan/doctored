//! The ICC matrix/TRC NUMERICS: the tone-curve model, the profile→PCS
//! matrix, the equivalence test two profiles are compared with, and the
//! 8-bit-in/8-bit-out [`Transform`] between two of them. The parser that
//! builds these values from profile bytes is `icc`; the four profiles this
//! build writes are `icc_builtin`.
//!
//! # The pipeline, and why it is shaped this way
//!
//! Straight (non-premultiplied) RGBA8 in, straight RGBA8 out, with the
//! **alpha byte copied untouched** — alpha is coverage, not colour.
//!
//! **Decode is a 256-entry table per channel and it is exact.** The input is
//! 8-bit, so every byte's linear contribution is known up front; the columns
//! of the combined matrix are folded into that table, which makes the
//! per-pixel path nine adds with no multiplies and no interpolation error on
//! the source side.
//!
//! **Encode is a binary search over 255 forward-evaluated thresholds, and
//! there is no curve inverter in this crate.** `thresh[c][k]` is the linear
//! value at which the destination's 8-bit code rounds up from `k` to `k + 1`,
//! obtained by evaluating the destination curve FORWARD at `(k + 0.5) / 255`;
//! `partition_point` then counts the thresholds strictly below the pixel's
//! linear value, which is exactly `round(inverse(lin) * 255)` for a strictly
//! increasing curve and picks the lowest pre-image on a plateau. Adding an
//! inverter back is a regression, for two independent reasons measured
//! before this module was written:
//!
//! * a table indexed uniformly in LINEAR value loses two 8-bit codes in the
//!   shadows of any gamma-2.2 destination (code 3 came back as 1 at
//!   γ = 2.2 and γ = 2.4 — Adobe RGB (1998) and ProPhoto are exactly the
//!   profiles a photographer's files carry after sRGB and Display P3), and
//! * indexing a 4096-entry table by `floor(lin * 4095) + 1` reads one past
//!   the end at `lin == 1.0`. The search below indexes no table with a
//!   computed float at all: its result is a count in `0..=255`, so that
//!   class of bug is structurally impossible rather than merely guarded.
//!
//! Round-tripping all 256 byte values through `eval` and back through the
//! search is byte-exact (max |Δ| = 0) for the sRGB `para` curve, for
//! `Gamma(1.0 / 1.8 / 2.19921875 / 2.2 / 2.4)` and for a 1024-entry
//! `Sampled` sRGB curve; `color_tests::identity_is_exact_for_every_curve_kind`
//! is that measurement as a committed test.
//!
//! **Out of gamut** is the destination's own black and white: the linear
//! value is clamped to [0, 1] before the search, never reported as an error.
//!
//! Cost is three 8-step binary searches per pixel. A conversion is a one-shot
//! user command, so this is not a hot path and there is deliberately no
//! transform cache.

use image::RgbaImage;

#[path = "icc_precision.rs"]
mod precision;
pub(crate) use precision::LinearInk;
pub use precision::{PrecisionToneEncoder, PrecisionTransform};

/// How far the COMBINED matrix `inv(b) · a` may sit from the identity, in
/// any entry, for two profiles to count as the same colour space.
///
/// 5e-4, and the number is measured rather than chosen. macOS's shipped
/// `sRGB Profile.icc` — the HP/IEC profile Photoshop, GIMP and most cameras
/// embed — has `bXYZ.Z = 0.714096` against this build's `0.713928`, an
/// ENTRYWISE gap of 1.83e-4; an entrywise 1e-4 tolerance would therefore
/// reject the world's most common tagged file, making every open of one a
/// full-image transform with an extra ±1 requantization and making
/// `convert_to_profile` to the built-in sRGB always report a change. The
/// COMBINED matrix for that same pair deviates from the identity by only
/// 2.585e-4, so comparing the combination — which is what a conversion would
/// actually apply — takes the "nothing would change" branch, which is the
/// right answer.
///
/// 5e-4 is far below one 8-bit code: the induced encoded error is bounded by
/// `d(encode)/d(lin) · lin · tol ≤ 0.44 · 5e-4 ≈ 0.06 code` above the toe,
/// and smaller still inside it where `lin` itself is tiny.
const MATRIX_TOL: f64 = 5e-4;

/// How far two tone curves may differ, at any of 256 evenly spaced inputs,
/// for two profiles to count as the same colour space. One part in 10 000 is
/// a quarter of an 8-bit code at the steepest point of the sRGB curve.
const TRC_TOL: f64 = 1e-4;

/// A determinant this small means the three primaries are collinear (or the
/// tags are zero): the profile describes no invertible colour space, so it
/// is refused as unmodellable rather than producing infinities.
const MIN_DET: f64 = 1e-9;

/// One tone reproduction curve, in the four shapes ICC `curv` and `para`
/// tags express. Every variant maps [0, 1] → [0, 1] through
/// [`Curve::eval`], which is the ONE evaluator: nothing in this crate
/// inverts a curve (see the module doc).
#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    /// `curv` with a count of 0 — the linear curve.
    Identity,
    /// `curv` with a count of 1: the single u8Fixed8 entry is a gamma
    /// exponent (`raw / 256`). A non-positive exponent is degraded to
    /// [`Curve::Identity`] at parse time.
    Gamma(f64),
    /// `curv` with a count above 1: a table sampled uniformly over [0, 1]
    /// with 16-bit outputs, linearly interpolated over `n - 1` intervals.
    /// Monotonized by a running maximum at parse time, so the threshold
    /// search's precondition holds for any input file.
    Sampled(Vec<u16>),
    /// `para`, function types 0 to 4, with the unused parameters zero.
    Para {
        kind: u8,
        g: f64,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
    },
}

impl Curve {
    /// The curve at `x`, clamped into [0, 1] on both sides. A parameter set
    /// that produces a non-finite result (a negative base under a fractional
    /// exponent, a zero base under a negative one) yields 0 rather than a
    /// NaN that would poison the matrix.
    pub(crate) fn eval(&self, x: f64) -> f64 {
        let x = if x.is_finite() {
            x.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let y = self.raw_eval(x);
        if y.is_finite() {
            y.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
    // One formula implementation for legacy defensive evaluation and strict
    // typed validation. The public byte result policy remains unchanged.
    fn raw_eval(&self, x: f64) -> f64 {
        match self {
            Curve::Identity => x,
            Curve::Gamma(g) => {
                if *g > 0.0 {
                    x.powf(*g)
                } else {
                    x
                }
            }
            Curve::Sampled(table) => {
                let n = table.len();
                if n < 2 {
                    // Guarded at parse time; defensive, and linear is the
                    // only honest reading of a table with no interval.
                    return x;
                }
                let pos = x * (n - 1) as f64;
                let i = pos.floor() as usize;
                if i >= n - 1 {
                    f64::from(table[n - 1]) / 65535.0
                } else {
                    let f = pos - i as f64;
                    (f64::from(table[i]) * (1.0 - f) + f64::from(table[i + 1]) * f) / 65535.0
                }
            }
            // The ICC parametricCurveType formulas, one per function type.
            Curve::Para {
                kind,
                g,
                a,
                b,
                c,
                d,
                e,
                f,
            } => match kind {
                0 => pow(x, *g),
                1 => {
                    if *a != 0.0 && x >= -b / a {
                        pow(a * x + b, *g)
                    } else {
                        0.0
                    }
                }
                2 => {
                    if *a != 0.0 && x >= -b / a {
                        pow(a * x + b, *g) + c
                    } else {
                        *c
                    }
                }
                3 => {
                    if x >= *d {
                        pow(a * x + b, *g)
                    } else {
                        c * x
                    }
                }
                _ => {
                    if x >= *d {
                        pow(a * x + b, *g) + e
                    } else {
                        c * x + f
                    }
                }
            },
        }
    }
}

/// The pure power `document_v2` Exposure linearizes with when a curve is
/// not itself one (rule Γ, [`Curve::effective_gamma`]): sRGB-shaped and
/// sampled curves. 2.2, measured (#578, `scripts/photoshop-probe/
/// stroke_exposure_578_take.py` take1): Photoshop's Exposure on sRGB (a
/// sampled `curv`) and on Apple's Display P3 (an offset `para`, g 2.4) fits
/// x^2.2 within 1.76 codes at 8 and 16 bits, where P3's own g (2.4) misses
/// by 129 codes and its mid-grey exponent (2.224) by 65, both near a zero
/// crossing; take2 (`stroke_exposure_578_take2.py`) confirms sRGB 8 and P3
/// 16 within 1.01. In a profile whose curves FAIL the precision check, an
/// offset `para` that is not sRGB-shaped takes [`FAILING_OFFSET_CURVE_GAMMA`]
/// instead (`MatrixTrc::exposure_domain`); a passing one (ITU-2020) keeps
/// this value.
pub(crate) const OFFSET_CURVE_GAMMA: f64 = 2.2;

/// The pure power `document_v2` Exposure linearizes an offset `para` channel
/// with (class O of `MatrixTrc::exposure_domain`) when the profile fails the
/// precision check on a falling branch boundary: Apple's `ITU-709.icc` (g
/// 2.222, b 0.090, a linear toe whose top lies above the power branch's
/// start). 1.883, fitted on take1's 16-bit ITU-709 rows (#578,
/// `scripts/photoshop-probe/stroke_exposure_578_take2.py`, grid 1.500 to
/// 2.600 by worst code) BEFORE take2, which confirmed it within 1.0 code at
/// 8 and 16 bits on four fresh parameter sets — where 2.2 misses by 114,
/// the curve's own g by 116, its mid-grey exponent by 68 and the curve
/// itself both ways by 57.
pub(crate) const FAILING_OFFSET_CURVE_GAMMA: f64 = 1.883;

impl Curve {
    /// Rule Γ (#578): the pure power `document_v2` Exposure evaluates a
    /// channel of this curve in. `Identity` → 1; `Gamma(g)` → g; a `Para`
    /// whose power term is a pure power (b = 0, and no added constant: c for
    /// type 2, e for type 4) → its g, any linear toe ignored (ROMM's γ 1.8
    /// with its toe fits x^1.8 as well as its own curve, 0.76 codes max,
    /// and better on the mean, 0.023 against 0.036); a `Sampled` table, a
    /// `Para` with an offset, or a non-positive / non-finite exponent →
    /// [`OFFSET_CURVE_GAMMA`].
    pub(crate) fn effective_gamma(&self) -> f64 {
        let usable = |g: f64| g.is_finite() && g > 0.0;
        match self {
            Curve::Identity => 1.0,
            Curve::Gamma(g) if usable(*g) => *g,
            Curve::Para {
                kind, g, b, c, e, ..
            } if usable(*g)
                && *b == 0.0
                && !(*kind == 2 && *c != 0.0)
                && !(*kind >= 4 && *e != 0.0) =>
            {
                *g
            }
            _ => OFFSET_CURVE_GAMMA,
        }
    }
}

impl Curve {
    /// Whether this is IEC 61966-2-1's sRGB curve as a `para` of type 3 or
    /// 4: each of (g, a, b, c, d) within 1e-3 of (2.4, 1/1.055,
    /// 0.055/1.055, 1/12.92, 0.04045) — Apple's Display P3 carries it.
    pub(crate) fn is_srgb_shaped(&self) -> bool {
        const IEC: [f64; 5] = [2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045];
        match self {
            Curve::Para {
                kind: 3 | 4,
                g,
                a,
                b,
                c,
                d,
                ..
            } => [*g, *a, *b, *c, *d]
                .iter()
                .zip(IEC)
                .all(|(v, w)| (v - w).abs() <= 1e-3),
            _ => false,
        }
    }

    /// The exponent of this channel's class in a profile that fails the
    /// precision check (`MatrixTrc::exposure_domain`): an offset `para`
    /// that is not sRGB-shaped (class O) → [`FAILING_OFFSET_CURVE_GAMMA`];
    /// everything else (class P, a pure power; class S, sampled or
    /// sRGB-shaped) → [`Self::effective_gamma`]. "Offset" is the negation of
    /// `effective_gamma`'s pure-power condition, restated: b ≠ 0, or the added
    /// constant c (type 2) or e (type 4).
    fn failing_exposure_gamma(&self) -> f64 {
        match self {
            Curve::Para { kind, b, c, e, .. }
                if (*b != 0.0 || (*kind == 2 && *c != 0.0) || (*kind >= 4 && *e != 0.0))
                    && !self.is_srgb_shaped() =>
            {
                FAILING_OFFSET_CURVE_GAMMA
            }
            _ => self.effective_gamma(),
        }
    }
}

/// `base^exp` with a negative base read as 0 — the ICC formulas are only
/// defined for a non-negative base, and `f64::powf` would answer NaN.
fn pow(base: f64, exp: f64) -> f64 {
    if base <= 0.0 {
        0.0
    } else {
        base.powf(exp)
    }
}

/// A profile we can actually transform with: the three D50-adapted matrix
/// columns and the three tone curves.
///
/// **The PCS white is always D50 and `wtpt`/`chad` are advisory.** For a
/// matrix/TRC profile the ICC spec requires `rXYZ`/`gXYZ`/`bXYZ` to be
/// already D50-adapted; `chad` merely RECORDS the adaptation that was
/// applied so a CMM can recover the native white. Adapting again with
/// either tag double-adapts. The proof ships with macOS: `sRGB Profile.icc`
/// has D50-adapted columns, no `chad` tag at all, and a `wtpt` holding the
/// unadapted D65 (0.950455, 1.0, 1.089050) — a reader that trusts `wtpt` as
/// the PCS white gets sRGB visibly wrong.
#[derive(Clone, Debug)]
pub struct MatrixTrc {
    /// COLUMNS of the profile→PCS matrix: `to_pcs[c]` is the D50-adapted
    /// XYZ of channel `c` at full intensity (r, g, b in that order), so
    /// `XYZ = Σ_c to_pcs[c] · linear[c]`.
    to_pcs: [[f64; 3]; 3],
    /// ROWS of its inverse, the PCS→profile matrix:
    /// `linear[i] = Σ_k from_pcs[i][k] · XYZ[k]`. Computed once at
    /// construction, which is also where a singular matrix is refused.
    from_pcs: [[f64; 3]; 3],
    /// rTRC, gTRC, bTRC.
    trc: [Curve; 3],
    // The byte parser repairs decreasing sampled curves. Preserve that legacy
    // behavior while preventing a typed conversion from silently ENCODING
    // through repairs; decoding a source through them is allowed.
    precision_compatible: bool,
}

impl MatrixTrc {
    /// Sampled curves are the model's only out-of-line allocations.
    pub(crate) fn record_resources(&self, inventory: &mut crate::resources::Inventory) {
        self.record_resources_with_cache(inventory, false);
    }
    pub(crate) fn record_resources_with_cache(
        &self,
        inventory: &mut crate::resources::Inventory,
        cache: bool,
    ) {
        for curve in &self.trc {
            if let Curve::Sampled(samples) = curve {
                inventory.vec(samples, "profile", cache);
            }
        }
    }

    /// Builds the model from the three matrix columns and the three curves.
    /// `None` when the columns are not an invertible basis (collinear
    /// primaries, or all-zero tags) — that profile describes no colour space
    /// this crate can convert through, and is reported as unmodellable
    /// rather than as an error.
    pub fn new(to_pcs: [[f64; 3]; 3], trc: [Curve; 3]) -> Option<MatrixTrc> {
        if !to_pcs.iter().all(|col| col.iter().all(|v| v.is_finite())) {
            return None;
        }
        let from_pcs = invert(to_pcs)?;
        Some(MatrixTrc {
            to_pcs,
            from_pcs,
            trc,
            precision_compatible: true,
        })
    }

    /// True when converting between the two would change nothing: the
    /// COMBINED matrix `inv(other.to_pcs) · self.to_pcs` is within
    /// [`MATRIX_TOL`] of the identity in every entry AND the three curve
    /// pairs agree within [`TRC_TOL`] at 256 evenly spaced inputs. See
    /// [`MATRIX_TOL`] for why the combination is compared and not the
    /// entries.
    pub fn approx_eq(&self, other: &MatrixTrc) -> bool {
        let combined = self.combined_with(other);
        for (i, row) in combined.iter().enumerate() {
            for (c, &v) in row.iter().enumerate() {
                let target = if i == c { 1.0 } else { 0.0 };
                if (v - target).abs() > MATRIX_TOL {
                    return false;
                }
            }
        }
        for (mine, theirs) in self.trc.iter().zip(other.trc.iter()) {
            for s in 0..256 {
                let x = f64::from(s) / 255.0;
                if (mine.eval(x) - theirs.eval(x)).abs() > TRC_TOL {
                    return false;
                }
            }
        }
        true
    }

    /// One ENCODED straight RGB triple in [0, 1] as PCS XYZ: linearized
    /// with this profile's own three TRCs, then through its own matrix
    /// columns. The result is ALREADY D50-adapted — that is what the three
    /// XYZ tags mean (module doc) — so nothing downstream adapts again.
    ///
    /// Forward direction only. There is deliberately no `from_pcs_xyz`
    /// twin: coming back would need a curve INVERTER, and this crate not
    /// having one is a documented decision, not an omission.
    pub(crate) fn to_pcs_xyz(&self, rgb: [f32; 3]) -> [f32; 3] {
        let mut xyz = [0.0f64; 3];
        for (c, column) in self.to_pcs.iter().enumerate() {
            let linear = self.trc[c].eval(f64::from(rgb[c]));
            for (slot, entry) in xyz.iter_mut().zip(column) {
                *slot += entry * linear;
            }
        }
        xyz.map(|v| v as f32)
    }

    /// [`MatrixTrc::to_pcs_xyz`] in full f64 — the same nine multiplies and
    /// the same curve evaluator, without the f32 round trip on either side.
    /// `icc_intent`'s general pipeline needs it because a BPC or absolute
    /// step then moves the result and re-encodes it, and rounding to f32
    /// twice inside one conversion is a measurable drift. The existing f32
    /// entry point is untouched.
    /// `pub` because `soft_proof_tests`' independent gamut oracle must ask
    /// the DOCUMENT's own model for the pixel's PCS colour: the rule being
    /// measured is the round trip, the Lab distance and the threshold, not
    /// the decode, and a second decode here would compare two different
    /// questions at a hard threshold.
    pub fn to_pcs_xyz64(&self, rgb: [f64; 3]) -> [f64; 3] {
        let mut xyz = [0.0f64; 3];
        for (c, column) in self.to_pcs.iter().enumerate() {
            let linear = self.trc[c].eval(rgb[c]);
            for (slot, entry) in xyz.iter_mut().zip(column) {
                *slot += entry * linear;
            }
        }
        xyz
    }

    /// LINEAR components through this profile's own matrix columns to PCS
    /// XYZ: [`MatrixTrc::to_pcs_xyz64`] with no curve, for values that are
    /// already linear (a 32-bit float document's samples, any sign or
    /// magnitude).
    pub(crate) fn linear_to_pcs(&self, rgb: [f64; 3]) -> [f64; 3] {
        let mut xyz = [0.0f64; 3];
        for (c, column) in self.to_pcs.iter().enumerate() {
            for (slot, entry) in xyz.iter_mut().zip(column) {
                *slot += entry * rgb[c];
            }
        }
        xyz
    }

    /// The PCS→profile matrix applied to `xyz`, under the name `icc_intent`
    /// reads it by. Matrix only: the caller encodes the linear result
    /// itself, because there is still no curve inverter in this crate.
    #[allow(clippy::wrong_self_convention)]
    pub(crate) fn from_pcs_linear(&self, xyz: [f64; 3]) -> [f64; 3] {
        self.pcs_to_linear(xyz)
    }

    /// A matrix/TRC profile's media white is D50, always — the module doc's
    /// PCS white rule. Named as a function so `icc_intent` can ask both
    /// kinds of profile the same question.
    pub(crate) fn media_white(&self) -> [f64; 3] {
        crate::icc_lut::D50
    }

    /// One PCS XYZ triple as this profile's LINEAR components: the
    /// [`MatrixTrc::from_pcs`] rows applied to `xyz`. Matrix only — there is
    /// still no curve inverter in this crate (module doc), so the caller
    /// encodes the result itself. `lab.rs` is the one caller: a Photoshop Lab
    /// colour comes back through the built-in sRGB model's inverse matrix and
    /// `adjust_math::linear_to_srgb`.
    pub(crate) fn pcs_to_linear(&self, xyz: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|i| (0..3).map(|k| self.from_pcs[i][k] * xyz[k]).sum())
    }

    /// The three channels' rule-Γ exponents ([`Curve::effective_gamma`]),
    /// in r, g, b order: the per-channel domain of `document_v2` Exposure.
    pub(crate) fn effective_gammas(&self) -> [f64; 3] {
        std::array::from_fn(|c| self.trc[c].effective_gamma())
    }

    /// The per-channel exponents `document_v2` Exposure evaluates in (#578),
    /// or `None` where it cannot. A profile that passes
    /// [`Self::validate_precision`] → [`Self::effective_gammas`] (rule Γ,
    /// unchanged: ITU-2020 stays 2.2). One that fails it → `None` when its
    /// sampled curves were repaired (`precision_compatible` false) or a
    /// curve's own parameters are unusable (`Curve::parameters_valid`), else
    /// each channel's class exponent (`Curve::failing_exposure_gamma`: ROMM's
    /// pure power 1.8, ITU-709's offset curve 1.883). Exposure only needs
    /// that one exponent per channel, never the curve's encoder, which is
    /// why a falling branch boundary does not refuse it. Its second reader is
    /// a 16-bit Vibrance (`BoundVibrance::prepare_precision`), which decodes
    /// and encodes through the same power where the precision checks fail.
    pub(crate) fn exposure_domain(&self) -> Option<[f64; 3]> {
        if self.validate_precision().is_ok() {
            return Some(self.effective_gammas());
        }
        if !self.precision_compatible || !self.trc.iter().all(Curve::parameters_valid) {
            return None;
        }
        Some(std::array::from_fn(|c| {
            self.trc[c].failing_exposure_gamma()
        }))
    }

    /// Decode continuous encoded components for calibrated operations. This
    /// shares the profile's forward curve evaluator with the conversion tables.
    pub(crate) fn decode_rgb(&self, rgb: [f32; 3]) -> [f64; 3] {
        std::array::from_fn(|c| self.trc[c].eval(f64::from(rgb[c])))
    }

    /// `inv(dst.to_pcs) · self.to_pcs`, row-major: the matrix a conversion
    /// from `self` to `dst` applies to linear RGB.
    pub(crate) fn combined_with(&self, dst: &MatrixTrc) -> [[f64; 3]; 3] {
        let mut out = [[0.0f64; 3]; 3];
        for (i, row) in out.iter_mut().enumerate() {
            for (c, entry) in row.iter_mut().enumerate() {
                *entry = (0..3).map(|k| dst.from_pcs[i][k] * self.to_pcs[c][k]).sum();
            }
        }
        out
    }
}

/// Inverts a matrix given by its COLUMNS, returning ROWS of the inverse.
/// `None` for a determinant too small to trust (see [`MIN_DET`]).
pub(crate) fn invert(cols: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    // m[row][col]; cols[c] is column c.
    let m = |r: usize, c: usize| cols[c][r];
    let det = m(0, 0) * (m(1, 1) * m(2, 2) - m(1, 2) * m(2, 1))
        - m(0, 1) * (m(1, 0) * m(2, 2) - m(1, 2) * m(2, 0))
        + m(0, 2) * (m(1, 0) * m(2, 1) - m(1, 1) * m(2, 0));
    if !det.is_finite() || det.abs() < MIN_DET {
        return None;
    }
    let cof = |r: usize, c: usize| {
        let (r0, r1) = ((r + 1) % 3, (r + 2) % 3);
        let (c0, c1) = ((c + 1) % 3, (c + 2) % 3);
        m(r0, c0) * m(r1, c1) - m(r0, c1) * m(r1, c0)
    };
    let mut inv = [[0.0f64; 3]; 3];
    for (r, row) in inv.iter_mut().enumerate() {
        for (c, entry) in row.iter_mut().enumerate() {
            // inv[r][c] = cofactor(c, r) / det (adjugate = transposed
            // cofactor matrix).
            *entry = cof(c, r) / det;
        }
    }
    Some(inv)
}

/// A prepared source→destination conversion. See the module doc for the
/// decode table and the threshold encode.
pub struct Transform {
    /// `col[c][v]` is source channel `c`'s contribution, for input byte `v`,
    /// to the three DESTINATION linear channels — the combined matrix's
    /// column `c` scaled by the source curve at `v / 255`.
    col: [[[f32; 3]; 256]; 3],
    /// `thresh[c][k]` is the destination linear value at which output code
    /// `k` rounds up to `k + 1`: the destination curve evaluated FORWARD at
    /// `(k + 0.5) / 255`, made monotone by a running maximum.
    encoder: ToneEncoder,
}

impl Transform {
    /// Prepares the conversion from `src` to `dst`.
    pub fn new(src: &MatrixTrc, dst: &MatrixTrc) -> Transform {
        let combined = src.combined_with(dst);
        let mut col = [[[0.0f32; 3]; 256]; 3];
        for (c, channel) in col.iter_mut().enumerate() {
            for (v, entry) in channel.iter_mut().enumerate() {
                let lin = src.trc[c].eval(v as f64 / 255.0);
                for (i, slot) in entry.iter_mut().enumerate() {
                    *slot = (combined[i][c] * lin) as f32;
                }
            }
        }
        Transform {
            col,
            encoder: ToneEncoder::new(dst),
        }
    }

    /// The transform from `src` to `dst`, or `None` when the two describe
    /// the SAME space — the [`MatrixTrc::approx_eq`] rule, asked once here
    /// so "there is nothing to convert" has one spelling: a document op
    /// refuses (no phantom undo step) and a compositing caller leaves its
    /// colours exactly as authored (no requantization of an sRGB colour on
    /// an sRGB document).
    pub(crate) fn between(src: &MatrixTrc, dst: &MatrixTrc) -> Option<Transform> {
        (!src.approx_eq(dst)).then(|| Transform::new(src, dst))
    }

    /// One straight RGB triple converted — the ONE per-colour step, run by
    /// [`Transform::apply`] over a buffer and by `style_composite` over an
    /// authored layer-style colour.
    pub(crate) fn color(&self, rgb: [u8; 3]) -> [u8; 3] {
        let (cr, cg, cb) = (
            &self.col[0][rgb[0] as usize],
            &self.col[1][rgb[1] as usize],
            &self.col[2][rgb[2] as usize],
        );
        let mut out = [0u8; 3];
        for (i, o) in out.iter_mut().enumerate() {
            let lin = (cr[i] + cg[i] + cb[i]).clamp(0.0, 1.0);
            // `partition_point` counts the thresholds strictly below `lin`,
            // which is the output code — always in 0..=255, so the cast is
            // total.
            *o = self.encoder.channel(i, lin);
        }
        out
    }

    /// Converts straight (non-premultiplied) RGBA8 pixels in place. Alpha is
    /// copied untouched.
    pub fn apply(&self, pixels: &mut RgbaImage) {
        for px in pixels.chunks_exact_mut(4) {
            let out = self.color([px[0], px[1], px[2]]);
            px[..3].copy_from_slice(&out);
        }
    }
}

/// The ONE forward-threshold encoder, shared by ICC conversions and
/// document-context adjustments. No curve inversion or uniform-linear LUT.
#[derive(Clone)]
pub(crate) struct ToneEncoder {
    thresh: [[f32; 255]; 3],
}

impl ToneEncoder {
    pub(crate) fn new(dst: &MatrixTrc) -> Self {
        let mut thresh = [[0.0f32; 255]; 3];
        for (c, channel) in thresh.iter_mut().enumerate() {
            let mut run = f64::NEG_INFINITY;
            for (k, slot) in channel.iter_mut().enumerate() {
                // Defensive monotonization is the historical Transform rule.
                run = run.max(dst.trc[c].eval((k as f64 + 0.5) / 255.0));
                *slot = run as f32;
            }
        }
        Self { thresh }
    }

    pub(crate) fn channel(&self, channel: usize, linear: f32) -> u8 {
        self.thresh[channel].partition_point(|&t| t < linear.clamp(0.0, 1.0)) as u8
    }

    pub(crate) fn rgb(&self, linear: [f64; 3]) -> [f32; 3] {
        std::array::from_fn(|c| f32::from(self.channel(c, linear[c] as f32)) / 255.0)
    }
}
