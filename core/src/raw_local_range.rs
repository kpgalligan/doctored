//! Luminance and colour range of a RAW local: a second coverage, in 0…1, that
//! multiplies the local's shape coverage.
//!
//! # Which pixels the range reads, and in which space
//!
//! The range reads the BASE development: the camera-only global render, before
//! any local, the manual vignette, optics and grain. Each base sample goes
//! through the base's own profile to CIE L*a*b* against the D50 PCS white
//! (`lab.rs` has the pipeline), and the laws below are written on that Lab.
//!
//! Why the base and not the picture being built: a local's exposure or white
//! balance must not move its own selection. Read from the mixed result, a
//! local that darkens the highlights would push them out of a highlight range
//! and select less on the next render.
//!
//! Why Lab and not the encoded samples: the same bounds then select the same
//! tones whether the development is sRGB, Display P3, Adobe RGB or ProPhoto,
//! whose encodings of one colour differ. L* and the Euclidean distance in
//! L*a*b* (ΔE*ab) are also perceptually even, so a falloff width means the same
//! at any tone; a width in encoded or linear units would be wide in the shadows
//! and narrow in the highlights, or the reverse.
//!
//! # The laws
//!
//! `l` is L*/100 (so 0 ≤ l ≤ 1, see [`lab`]), `d` the Euclidean distance in
//! L*a*b* from the range's colour, and `s(t) = t²(3 − 2t)`, the smoothstep
//! `raw_local_coverage::radial` uses for its soft edge.
//!
//! - Luminance: 1 for `low ≤ l ≤ high`. Below `low`: 0 when `low_falloff` is 0,
//!   else `s(clamp((l − low + low_falloff) / low_falloff, 0, 1))`. Above `high`:
//!   0 when `high_falloff` is 0, else `1 − s(clamp((l − high) / high_falloff, 0, 1))`.
//! - Colour: 1 for `d ≤ amount`; 0 for `d ≥ amount + falloff` or `falloff` 0;
//!   else `1 − s((d − amount) / falloff)`.
//! - `invert` gives `1 − value`. A range's coverage is the product of its
//!   present components.
//!
//! The range is evaluated per pixel and is not blurred: a local's feather and
//! density act on its shape.
use crate::icc_transform::MatrixTrc;
use serde::Deserialize;

/// Side of the square of base pixels a sampled colour is the mean of. Five
/// averages out sensor noise and demosaic texture at one click without
/// reaching across a small feature.
pub(crate) const SAMPLE_WINDOW: u32 = 5;

#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LuminanceRange {
    pub low: f64,
    pub high: f64,
    pub low_falloff: f64,
    pub high_falloff: f64,
    pub invert: bool,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorRange {
    pub lab: [f64; 3],
    pub amount: f64,
    pub falloff: f64,
    pub invert: bool,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalRange {
    #[serde(default)]
    pub luminance: Option<LuminanceRange>,
    #[serde(default)]
    pub color: Option<ColorRange>,
}

fn within(v: f64, low: f64, high: f64) -> bool {
    v.is_finite() && (low..=high).contains(&v)
}
fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}
impl LuminanceRange {
    fn valid(&self) -> bool {
        within(self.low, 0.0, 1.0)
            && within(self.high, self.low, 1.0)
            && within(self.low_falloff, 0.0, 1.0)
            && within(self.high_falloff, 0.0, 1.0)
    }
    fn coverage(&self, l: f64) -> f64 {
        let value = if l < self.low {
            if self.low_falloff == 0.0 {
                0.0
            } else {
                smooth(((l - self.low + self.low_falloff) / self.low_falloff).clamp(0.0, 1.0))
            }
        } else if l > self.high {
            if self.high_falloff == 0.0 {
                0.0
            } else {
                1.0 - smooth(((l - self.high) / self.high_falloff).clamp(0.0, 1.0))
            }
        } else {
            1.0
        };
        if self.invert {
            1.0 - value
        } else {
            value
        }
    }
}
impl ColorRange {
    fn valid(&self) -> bool {
        within(self.lab[0], 0.0, 100.0)
            && within(self.lab[1], -256.0, 256.0)
            && within(self.lab[2], -256.0, 256.0)
            && within(self.amount, 0.0, 200.0)
            && within(self.falloff, 0.0, 200.0)
    }
    fn coverage(&self, lab: [f64; 3]) -> f64 {
        let d = (0..3)
            .map(|c| (lab[c] - self.lab[c]).powi(2))
            .sum::<f64>()
            .sqrt();
        let value = if d <= self.amount {
            1.0
        } else if self.falloff == 0.0 || d >= self.amount + self.falloff {
            0.0
        } else {
            1.0 - smooth((d - self.amount) / self.falloff)
        };
        if self.invert {
            1.0 - value
        } else {
            value
        }
    }
}
impl LocalRange {
    /// Refuses a range with no component, a non-finite number, bounds outside
    /// `0 ≤ low ≤ high ≤ 1`, a luminance falloff outside 0…1, a colour outside
    /// L 0…100 and a, b −256…256, or an amount or colour falloff outside 0…200.
    pub fn validate(&self) -> Result<(), String> {
        if (self.luminance.is_none() && self.color.is_none())
            || self.luminance.is_some_and(|r| !r.valid())
            || self.color.is_some_and(|r| !r.valid())
        {
            return Err("invalid local range bounds, falloff, amount or colour".into());
        }
        Ok(())
    }
    /// The range's coverage of one base sample's Lab ([`lab`]), in 0…1: the
    /// product of its present components (module doc).
    pub(crate) fn coverage(&self, lab: [f32; 3]) -> f32 {
        let lab = lab.map(f64::from);
        let mut value = 1.0;
        if let Some(range) = &self.luminance {
            value *= range.coverage(lab[0] / 100.0);
        }
        if let Some(range) = &self.color {
            value *= range.coverage(lab);
        }
        value as f32
    }
}

/// One base sample as CIE L*a*b* against the D50 PCS white, through the base's
/// own profile, with L* clamped to 0…100. a* and b* are untouched.
pub(crate) fn lab(model: &MatrixTrc, rgb: [u16; 3]) -> [f32; 3] {
    let mut lab = crate::lab::xyz_to_lab(model.to_pcs_xyz(rgb.map(|c| f32::from(c) / 65535.0)));
    // A profile's white is not exactly Y = 1: the built-in Display P3 and
    // ProPhoto Y entries sum to 65537/65536 as s15Fixed16 (`icc_builtin.rs`,
    // `columns`), so code 65535 in all three channels reads L* = 100.0006
    // unclamped. A hard `high` of 1 would then leave blown highlights out of
    // a 0…1 range, and a colour sampled there would fail the `L in 0…100`
    // check. Black can land a rounding step below 0 the same way
    // (`116·f(0) − 16` in f32). Clamping here, and only here, makes `l` a
    // number in 0…1 for every sample the laws see.
    lab[0] = lab[0].clamp(0.0, 100.0);
    lab
}

/// [`lab`] for a base that is already LINEAR light in the profile's primaries
/// (a 32-bit float development's samples): the profile's matrix with no tone
/// curve, then the same Lab and the same L* clamp. A sample above white reads
/// L* 100 and one below zero L* 0, so every extended-range sample has an `l`
/// in 0…1 and a hard `high` of 1 keeps the highlights the float development
/// holds above white; a* and b* are untouched and stay finite for any finite
/// sample.
pub(crate) fn lab_linear(model: &MatrixTrc, rgb: [f32; 3]) -> [f32; 3] {
    let xyz = model.linear_to_pcs(rgb.map(f64::from));
    let mut lab = crate::lab::xyz_to_lab(xyz.map(|v| v as f32));
    lab[0] = lab[0].clamp(0.0, 100.0);
    lab
}

/// The mean of [`lab`] over the [`SAMPLE_WINDOW`] square about the pixel `at`
/// on a plane of `dimensions`, clipped to the frame: the colour a range's
/// Sample Colour click stores. `sample(x, y)` is the base's u16 RGB there;
/// `at` is inside the frame. Develop's `raw_range_sample` (16-bit base) and
/// the Develop Filter's `sample` colour (`develop_filter_masks`) both read it.
pub(crate) fn window_mean(
    model: &MatrixTrc,
    dimensions: (u32, u32),
    at: (u32, u32),
    sample: impl Fn(u32, u32) -> [u16; 3],
) -> [f64; 3] {
    let (w, h) = dimensions;
    let (cx, cy) = at;
    let reach = SAMPLE_WINDOW / 2;
    let mut sum = [0.0_f64; 3];
    let mut count = 0.0;
    for py in cy.saturating_sub(reach)..=(cy + reach).min(h - 1) {
        for px in cx.saturating_sub(reach)..=(cx + reach).min(w - 1) {
            for (slot, v) in sum.iter_mut().zip(lab(model, sample(px, py))) {
                *slot += f64::from(v);
            }
            count += 1.0;
        }
    }
    sum.map(|v| v / count)
}
