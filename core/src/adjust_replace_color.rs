//! Replace Color: a Hue/Saturation edit weighted by how close each
//! pixel is to a set of sampled colours. The edit itself is
//! `adjust_hue_saturation::HueSaturation`'s master sliders — the ONE
//! Hue/Saturation edit — so a
//! pixel at full weight gets exactly what `hue_saturation` with the same
//! sliders gives it.
//!
//! # The weight
//!
//! `d` is the Euclidean distance from the pixel to the NEAREST listed colour,
//! in 0…255 code units of the ENCODED document numbers (a 16-bit document's
//! values are scaled into the same units, so a setting means the same reach
//! at either depth). With `f` the fuzziness:
//!
//! * `w = 1` for `d ≤ f/2` — a core that takes the whole edit, so a sampled
//!   colour with a little noise around it changes evenly;
//! * `w = 2(f − d)/f` between `f/2` and `f` — a linear falloff to 0, so the
//!   edit fades out rather than leaving a hard edge round the selection;
//! * `w = 0` at and beyond `f`. `f = 0` matches only `d = 0`, the exact
//!   colour (to within [`EXACT`], an f32 round trip's noise).
//!
//! The pixel then moves `w` of the way to its edit: `rgb + w·(HS(rgb) −
//! rgb)`.
//!
//! # Why encoded RGB and not Color Range's Lab
//!
//! Color Range measures in Lab through the document profile. An adjustment
//! op parses WITHOUT a profile (`Adjustment::from_op` is profile-free, and
//! the destructive twin runs the same parse), so a Lab distance would need
//! a profile binding step every other pointwise op does without. The
//! sampled colours are the document's own encoded numbers, as every colour
//! parameter of an adjustment is, so the distance is measured in the same
//! numbers the samples were taken in. Encoded RGB is roughly perceptual
//! already, which is what the fuzziness slider needs.

use serde_json::{Map, Value};

use crate::adjust_hue_saturation::HueSaturation;
use crate::adjust_parse::num_in;
use crate::style_names::parse_color;

/// The most colours one description lists — the sample set a dialog
/// builds with shift-clicks; past it a click replaces rather than adds.
const MAX_COLORS: usize = 8;

/// The distance, in code units, that still counts as the exact colour: a
/// code's trip through [0, 1] in f32 (`v / 255 · 255`) can land a few 1e-5
/// away from itself, and `fuzziness: 0` must still match the colour it was
/// sampled from. Far below any real colour difference (one code is 1.0).
const EXACT: f32 = 1e-3;

/// A parsed `replace_color` op.
#[derive(Clone)]
pub(crate) struct ReplaceColor {
    /// The sampled colours in 0…255 code units.
    colors: Vec<[f32; 3]>,
    fuzziness: f32,
    edit: HueSaturation,
}

impl ReplaceColor {
    /// Parses the `replace_color` params per the schema row in `adjust`:
    /// `colors` is REQUIRED (1…8 `"#rrggbb"`); `fuzziness`, `hue`,
    /// `saturation` and `lightness` take their defaults. `None` for a
    /// missing or malformed `colors`, or any value out of range.
    pub(crate) fn parse(params: &Map<String, Value>) -> Option<ReplaceColor> {
        let listed = params.get("colors")?.as_array()?;
        if listed.is_empty() || listed.len() > MAX_COLORS {
            return None;
        }
        let colors = listed
            .iter()
            .map(|v| parse_color(v.as_str()?).map(|c| c.map(f32::from)))
            .collect::<Option<Vec<_>>>()?;
        let fuzziness = num_in(params, "fuzziness", 0.0..=200.0, 40.0)?;
        // Only the three master sliders reach the edit: a `bands` or
        // `colorize` key beside them is not part of this op's schema.
        let mut sliders = Map::new();
        for key in ["hue", "saturation", "lightness"] {
            if let Some(v) = params.get(key) {
                sliders.insert(key.to_owned(), v.clone());
            }
        }
        Some(ReplaceColor {
            colors,
            fuzziness,
            edit: HueSaturation::parse(&sliders)?,
        })
    }

    /// The weight in [0, 1] of the straight RGB triple `rgb` (in [0, 1])
    /// against the nearest listed colour (module doc).
    pub(crate) fn weight(&self, rgb: [f32; 3]) -> f32 {
        let code = rgb.map(|v| v * 255.0);
        let d = self
            .colors
            .iter()
            .map(|c| {
                let e = [code[0] - c[0], code[1] - c[1], code[2] - c[2]];
                (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt()
            })
            .fold(f32::INFINITY, f32::min);
        falloff(d, self.fuzziness)
    }

    /// One straight RGB triple in [0, 1]: `w` of the way to its
    /// Hue/Saturation edit. Full weight takes the edit verbatim and zero
    /// weight returns the input untouched, so neither end is a lerp's
    /// rounding.
    pub(crate) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let w = self.weight(rgb);
        if w <= 0.0 {
            return rgb;
        }
        let edited = self.edit.apply(rgb);
        if w >= 1.0 {
            return edited;
        }
        std::array::from_fn(|c| rgb[c] + w * (edited[c] - rgb[c]))
    }

    /// True when the edit changes nothing, whatever the colours match.
    pub(crate) fn is_identity(&self) -> bool {
        self.edit.is_identity()
    }
}

/// The weight of a pixel `distance` code units from its nearest colour at
/// `fuzziness` (module doc): 1 within `f/2` (or [`EXACT`]), a linear falloff
/// to 0 at `f`, 0 beyond. The CMYK op measures its distance over the four
/// inks (`cmyk_adjust_ink`).
pub(crate) fn falloff(distance: f32, fuzziness: f32) -> f32 {
    let f = fuzziness;
    if distance <= (f / 2.0).max(EXACT) {
        1.0
    } else if distance < f {
        2.0 * (f - distance) / f
    } else {
        0.0
    }
}
