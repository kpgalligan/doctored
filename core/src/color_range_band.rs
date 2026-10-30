//! Select ▸ Color Range's hue presets, Skin Tones and Localized Color Clusters:
//! the per-pixel band coverage of Reds, Yellows, Greens, Cyans, Blues
//! and Magentas, Skin Tones' coverage at a Fuzziness, the one ramp Color Range
//! shapes a distance with, and the spatial weight a sample point lends the
//! pixels around it. Free functions over plain numbers;
//! `selection_tools::color_range_samples` is the one caller and applies Invert
//! and the source alpha after them.
//!
//! **Bands.** A pixel's RGB (its encoded codes over the code maximum: native
//! codes, a float document's R1 display codes, a CMYK document's view) is
//! split by `adjust_math::decompose` into a neutral part, one primary
//! (R, G or B: `max − mid`) and one secondary (Y, C or M: `mid − min`) — the
//! six-slot model Black & White and Selective Color already stand on. The
//! band's coverage is that slot's amount times `2 − max(r, g, b)`, clamped
//! to 1, and `0` for any other slot: pure red is full Reds, orange
//! (1, 0.5, 0) splits between Reds and Yellows, a dark red reaches full
//! coverage before its amount does, and a grey or a pastel's neutral part
//! is outside every band.
//!
//! **Skin Tones.** The distance `D` is ΔE76 in the document-profile D50 Lab
//! from (70, 18, 19); at Fuzziness `F` (0…200, default 40) coverage is 1 for
//! `D ≤ 18`, then falls linearly to 0 over a shoulder `0.25·F` wide (at least
//! 0.01, so `F = 0` is a hard edge at 18). The pixel's RGB plays no part.
//!
//! **Clusters.** With sample points, sample i's colour ramp is weighted by
//! `1 − d/R` clamped to 0…1 (`LIN`), `d` the Euclidean distance between the
//! pixel's centre and the point's, `R = range/100 · max(w, h)`; `R = 0`
//! keeps only the point's own pixel (weight 1 at `d = 0`, else 0), never a
//! NaN.
//!
//! `scripts/photoshop-probe/color_range_671_take.py` fixes the candidates,
//! the fit and judge charts and the gate; its take1 against Photoshop
//! 26.10.0 (record in `designs/phase-6-selection-quality.md`):
//!
//! * Bands: take1 fitted `DEC(1.25)` (a MISS by its gate) and showed that
//!   `amount · (2 − max)` reproduces every take1 mask exactly.
//!   `color_range_671_take2.py` declared that rule before its take and
//!   judged it on the held-out HW3 chart (hues x + 0.5°, five (S, V) pairs
//!   not in take1) at 8 and 16 bits: SHIP, every held-out row max 0 codes
//!   at 8 bits and at most 0.51 (mean 0.084) at 16 bits on the 8-bit
//!   scale, and exact again on take1's 13 masks. It has no free number.
//! * Clusters UNMEASURED: Photoshop's descriptor carries no cluster
//!   position, so the declared `LIN` over `max(w, h)` ships.
//! * Skin Tones: `color_range_671_take3.py` declared three families (`LAB`,
//!   `AB`, `HS`, each through the ramp above with `r = t1·F + t0`,
//!   `j = max(0.01, s1·F + s0)`), a coordinate-descent fit on SK1 at six
//!   Fuzziness values and SK2, and a gate (max 2, mean 0.5 codes) on the
//!   held-out SK3 at 8 and 16 bits and the NASA portrait. take3 against
//!   Photoshop 26.10.0: the fitted `LAB (70, 18, 19) · RAMP (0, 18, 0.25, 0)`
//!   MISSES (judged max 221–255, mean 25–46 codes) and ships with that
//!   residual pinned, with no claim of agreement. Photoshop selects
//!   nothing at `F = 0` and its peak falls as `F` rises, a shape none of
//!   the three families has. Detect Faces is described only (it re-weights
//!   the whole frame) and is not built.

use crate::adjust_math::{decompose, HUE_WHEEL};

/// The band's position on `adjust_math::HUE_WHEEL` (0 reds … 5 magentas),
/// or `None` for any other name.
pub(crate) fn band_index(name: &str) -> Option<usize> {
    HUE_WHEEL.iter().position(|band| *band == name)
}

/// The coverage 0…1 of `band` at one pixel, `rgb` in 0…1 (module doc,
/// **Bands**). A primary band (even index) reads the primary amount, a
/// secondary (odd) the secondary amount, each times `2 − max(r, g, b)`.
pub(crate) fn band_coverage(band: usize, rgb: [f32; 3]) -> f32 {
    let rgb = rgb.map(|v| v.clamp(0.0, 1.0));
    let parts = decompose(rgb);
    let amount = if parts.primary == band {
        parts.primary_amount
    } else if parts.secondary == band {
        parts.secondary_amount
    } else {
        0.0
    };
    let max = rgb[0].max(rgb[1]).max(rgb[2]);
    (amount * (2.0 - max)).clamp(0.0, 1.0)
}

/// The parser's default for `fuzziness`.
pub(crate) const SKIN_DEFAULT_FUZZINESS: f32 = 40.0;

/// Skin Tones' centre in the document-profile D50 Lab, take3's fitted `LAB`
/// member (module doc, **Skin Tones**).
const SKIN_LAB: [f32; 3] = [70.0, 18.0, 19.0];
/// `[t1, t0, s1, s0]`: the tolerance `r = t1·F + t0` and shoulder
/// `j = max(0.01, s1·F + s0)` of the ramp at Fuzziness `F`, take3's fitted
/// `RAMP` (module doc, **Skin Tones**).
const SKIN_RAMP: [f32; 4] = [0.0, 18.0, 0.25, 0.0];
/// The shoulder's floor: a zero shoulder would make the ramp a step that a
/// pixel exactly at the tolerance could fall either side of.
const SKIN_MIN_SHOULDER: f32 = 0.01;

/// Skin Tones coverage 0…1: `rgb` the encoded codes / MAX, `lab` the
/// document-profile D50 Lab (`selection_tools::lab`), `fuzziness` 0…200. The
/// distance is ΔE76 from `SKIN_LAB`, ramped by [`ramp`] (module doc, **Skin
/// Tones**).
pub(crate) fn skin_coverage(rgb: [f32; 3], lab: [f32; 3], fuzziness: f32) -> f32 {
    // The shipped LAB member reads only Lab; `rgb` is the HSV family's input,
    // kept so the take's three families share one signature.
    let _ = rgb;
    let d = lab
        .iter()
        .zip(SKIN_LAB)
        .map(|(v, c)| (v - c) * (v - c))
        .sum::<f32>()
        .sqrt();
    let [t1, t0, s1, s0] = SKIN_RAMP;
    let tolerance = t1 * fuzziness + t0;
    let shoulder = (s1 * fuzziness + s0).max(SKIN_MIN_SHOULDER);
    ramp(d, tolerance, shoulder)
}

/// Color Range's one ramp: full coverage within `tolerance`, then a linear
/// shoulder `softness` wide (a zero shoulder is a hard edge). Sampled
/// Colors, the tonal presets and Skin Tones all use it.
pub(crate) fn ramp(distance: f32, tolerance: f32, softness: f32) -> f32 {
    if distance <= tolerance {
        1.0
    } else if softness == 0.0 {
        0.0
    } else {
        (1.0 - (distance - tolerance) / softness).clamp(0.0, 1.0)
    }
}

/// The cluster radius in canvas pixels for a Range of `range_percent`
/// (0…100) on a `dimensions` canvas (module doc, **Clusters**).
pub(crate) fn cluster_radius(range_percent: f32, dimensions: (u32, u32)) -> f32 {
    range_percent / 100.0 * dimensions.0.max(dimensions.1) as f32
}

/// The weight 0…1 a sample point lends a pixel `distance` pixels from it
/// (module doc, **Clusters**). A zero radius keeps the point's own pixel.
pub(crate) fn cluster_weight(distance: f32, radius: f32) -> f32 {
    if radius <= 0.0 {
        return if distance <= 0.0 { 1.0 } else { 0.0 };
    }
    (1.0 - distance / radius).clamp(0.0, 1.0)
}
