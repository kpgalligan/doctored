//! `gradient_map`: the pixel's luma read as a position along a multi-stop
//! gradient. The payload, its `parse` and its kernel live here; `adjust`
//! keeps only the schema row and the dispatch.
//!
//! # One gradient spelling in the core
//!
//! The gradient is `style::GradientFill`, parsed by
//! `style_json::gradient` — the SAME object and the SAME parser a layer
//! style's Gradient Overlay carries, because a second gradient spelling in
//! one core is worth less than the one difference between them costs to
//! document. That difference: a style's stop colours are AUTHORED sRGB,
//! converted into the document's space by the compositor, while a map's are
//! the DOCUMENT's own numbers, converted nowhere (`adjust`'s colour note).
//!
//! A map has no geometry, so nothing else is reused — no `GradientSampler`,
//! no gradient box. Only `stops`, `reverse` and (Classic) `smoothness` are
//! read; `style`, `angle`, `scale` and `align_with_layer` are accepted and
//! ignored, so one editor and one JSON shape serve both places. The model
//! has no separate opacity stops (one stop carries position, colour and
//! opacity).
//!
//! # The method
//!
//! `method` absent is the LEGACY map, every map an earlier build wrote: the
//! linear ramp, a stop's `midpoint` / `opacity_midpoint` and the gradient's
//! `smoothness` accepted and ignored (the midpoints reset to 0.5 by
//! [`GradientMap::parse`]), today's arithmetic statement for statement.
//! `method: "classic"` ([`MapMethod::Classic`]) is the Classic
//! method: the colour is `gradient_field::smooth_color` at the gradient's
//! Smoothness (0…1, default 1) with the bracket bound
//! (`gradient_field::ClassicBound::Bracket`, its module doc) and stop `i`'s
//! colour midpoint on segment `i..i + 1` (the fills' model rule; Photoshop's
//! file carries it on stop `i + 1`, translated in `psd_adjustment_grdm`
//! only); the opacity stays linear through stop `i`'s opacity midpoint
//! (`style_gradient::gradient_color_with`, unmeasured). Why:
//! `scripts/photoshop-probe/gradient_map_678_take.py` (an offline fit on
//! gradient-map-673 take1, then one confirm take: every SMOOTH and SEG row
//! within 2.0 / 0.095 codes at 8 and 16 bits).
//!
//! `method: "perceptual"` ([`MapMethod::Perceptual`]) is Photoshop's
//! Perceptual method: the SAME Classic law (`smooth_color`, the Smoothness,
//! the bracket bound, the midpoints, the linear opacity) run over each stop's
//! Oklab value (`oklab::linear_to_oklab` of the stop's sRGB-decoded colour,
//! computed once at parse), the result taken back through
//! `oklab::oklab_to_linear` and the sRGB encode (`adjust_math`). Why: the
//! same runner's PERC-OK, fitted on take1 (1.0 / 0.027 codes at 8 bits) and
//! met by the confirm take on all six Perceptual rows (worst 1.0 / 0.042, 8
//! and 16 bits). The document's numbers are read as sRGB, the take's space
//! (a Lab document's map already runs in sRGB, `lab_adjust::color_space`);
//! another RGB profile reads them the same way, unmeasured.
//!
//! Linear and Smooth are refusals for good: phase 1's candidates and phase
//! 2's one bounded attempt (LIN-XU, LIN-FRAC, SMOO-NAT, SMOO-CRT, the
//! runner's RECORDED-2) all miss the gate, the best Linear N4 at 100 % by 30
//! codes and the best Smooth by 16. Classic and Perceptual validate what
//! legacy ignores: `smoothness` outside [0, 1] or a midpoint outside [0.05,
//! 0.95] refuses.
//!
//! # The kernel
//!
//! ```text
//! t = luma(rgb)                              # Rec. 709
//! t = clamp(t + (dissolve_threshold(x, y) - 0.5)/255, 0, 1)   # when dithering
//! t = 1 - t                                  # when the gradient is reversed
//! (colour, opacity) = gradient_color(fill, t)          # legacy
//! colour = smooth_color(k, Bracket, midpoints), opacity linear   # classic
//! colour = encode(oklab_to_linear(smooth_color(k, Bracket, midpoints,
//!          oklab stops))), opacity linear                       # perceptual
//! out = rgb + (colour - rgb) * opacity
//! ```
//!
//! A stop's OPACITY blends between the ORIGINAL pixel (0) and the mapped
//! colour (1) — the only meaning available to an adjustment, which may
//! never touch alpha.
//!
//! `reverse` is applied HERE: `style_gradient::gradient_color` does not
//! apply it (the sampler's `t` does, and a map has no sampler).
//!
//! **Dither**, on by default, is what stops an 8-bit map banding over a
//! smooth sky: a deterministic +/- half a code of jitter on `t`, keyed on
//! the pixel's canvas position through `blend::dissolve_threshold` — the
//! same primitive the Dissolve blend mode uses, so the core has one
//! position-keyed hash and not two. It is pure (the same pixel always gets
//! the same offset), which is what lets it live under a `.rz` round trip and
//! a cached composite.

use serde_json::{Map, Value};

use crate::adjust_math::{linear_to_srgb, luma, srgb_to_linear};
use crate::adjust_parse::boolean;
use crate::blend::dissolve_threshold;
use crate::gradient_field::{smooth_color, ClassicBound};
use crate::oklab::{linear_to_oklab, oklab_to_linear};
use crate::style::{GradientFill, Strictness};
use crate::style_gradient::{gradient_color, gradient_color_with};
use crate::style_json;
use crate::style_render::unit_color;

/// A map's interpolation method (module doc, "The method").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MapMethod {
    Classic,
    Perceptual,
}

impl MapMethod {
    /// The `method` key of `gradient_map` params: `Some(None)` when absent
    /// (the legacy map), `Some(Some(Classic))` for `"classic"`,
    /// `Some(Some(Perceptual))` for `"perceptual"`, `None` (a refusal) for any
    /// other value, `"linear"` and `"smooth"` included (refused for good,
    /// module doc).
    pub(crate) fn parse(params: &Map<String, Value>) -> Option<Option<MapMethod>> {
        match params.get("method") {
            None => Some(None),
            Some(v) => match v.as_str()? {
                "classic" => Some(Some(MapMethod::Classic)),
                "perceptual" => Some(Some(MapMethod::Perceptual)),
                _ => None,
            },
        }
    }
}

/// Whether a Classic map's raw `smoothness` and stop midpoints are in range
/// (module doc): the style parser clamps them, and a map refuses instead.
fn classic_values_valid(params: &Map<String, Value>) -> bool {
    let Some(gradient) = params.get("gradient").and_then(Value::as_object) else {
        return true;
    };
    // Each checked in f32, the value's own type, so 0.05 and 0.95 themselves
    // are admitted.
    let valid = |v: Option<&Value>, ok: fn(f32) -> bool| {
        v.is_none_or(|v| v.as_f64().is_some_and(|f| ok(f as f32)))
    };
    let smoothness = |k: f32| crate::gradient_field::validate_smoothness(k).is_ok();
    let midpoint = crate::gradient_field::midpoint_valid;
    valid(gradient.get("smoothness"), smoothness)
        && gradient
            .get("stops")
            .and_then(Value::as_array)
            .is_none_or(|stops| {
                stops.iter().filter_map(Value::as_object).all(|stop| {
                    valid(stop.get("midpoint"), midpoint)
                        && valid(stop.get("opacity_midpoint"), midpoint)
                })
            })
}

/// A parsed `gradient_map` op.
#[derive(Clone)]
pub(crate) struct GradientMap {
    fill: GradientFill,
    dither: bool,
    dither_divisor: f32,
    method: Option<MapMethod>,
    /// Each stop's Oklab value (module doc), filled at parse for a Perceptual
    /// map and empty for every other.
    oklab: Vec<[f32; 3]>,
}

impl GradientMap {
    /// The gradient stop list and a Perceptual map's Oklab stop table are this
    /// parsed payload's only nested allocations.
    pub(crate) fn record_cached_resources(&self, inventory: &mut crate::resources::Inventory) {
        inventory.vec(&self.fill.stops, "lut_cache", true);
        inventory.vec(&self.oklab, "lut_cache", true);
    }
    pub(crate) fn record_resources(&self, inventory: &mut crate::resources::Inventory) {
        inventory.vec(&self.fill.stops, "precision_adjustment", false);
        inventory.vec(&self.oklab, "precision_adjustment", false);
    }

    /// Parses the `gradient_map` params per the schema row in `adjust`.
    /// The gradient goes through the style parser in its LENIENT mode: an
    /// unknown `style` name is defaulted rather than refused, the way the
    /// `.rz` reader treats a file from a newer build, since a map ignores
    /// that key anyway. Structural mistakes (a non-object, a 33-stop list,
    /// a colour that is not `#rrggbb`) are still refusals, as are a `method`
    /// other than `"classic"` or `"perceptual"` and, on such a map, a
    /// `smoothness` or a midpoint out of range (module doc, "The method").
    pub(crate) fn parse(params: &Map<String, Value>) -> Option<GradientMap> {
        let method = MapMethod::parse(params)?;
        if method.is_some() && !classic_values_valid(params) {
            return None;
        }
        let mut fill =
            style_json::gradient(params, "params", "gradient", Strictness::Lenient).ok()?;
        if method.is_none() {
            // The legacy map reads no midpoint (module doc).
            for stop in &mut fill.stops {
                stop.midpoint = 0.5;
                stop.opacity_midpoint = 0.5;
            }
        }
        let oklab = match method {
            Some(MapMethod::Perceptual) => fill
                .stops
                .iter()
                .map(|s| linear_to_oklab(unit_color(s.color).map(srgb_to_linear)))
                .collect(),
            Some(MapMethod::Classic) | None => Vec::new(),
        };
        Some(GradientMap {
            fill,
            dither: boolean(params, "dither", true)?,
            dither_divisor: 255.0,
            method,
            oklab,
        })
    }

    pub(crate) fn use_precision(&mut self) {
        self.dither_divisor = 65535.0;
    }

    /// One straight RGB triple in [0, 1] at canvas position `xy`.
    pub(crate) fn apply_at(&self, rgb: [f32; 3], xy: (i64, i64)) -> [f32; 3] {
        let mut t = luma(rgb);
        if self.dither {
            t += (dissolve_threshold(xy.0, xy.1) - 0.5) / self.dither_divisor;
        }
        t = t.clamp(0.0, 1.0);
        if self.fill.reverse {
            t = 1.0 - t;
        }
        let (mapped, opacity) = match self.method {
            None => gradient_color(&self.fill, t),
            Some(MapMethod::Classic) => {
                let stops = &self.fill.stops;
                let (_, opacity) = gradient_color_with::<0>(&self.fill, t, |_| []);
                let color = smooth_color(
                    stops.len(),
                    t,
                    self.fill.smoothness,
                    ClassicBound::Bracket,
                    |i| stops[i].position,
                    |i| stops[i].midpoint,
                    |i| unit_color(stops[i].color),
                );
                (color, opacity)
            }
            Some(MapMethod::Perceptual) => {
                let (stops, oklab) = (&self.fill.stops, &self.oklab);
                let (_, opacity) = gradient_color_with::<0>(&self.fill, t, |_| []);
                let lab = smooth_color(
                    stops.len(),
                    t,
                    self.fill.smoothness,
                    ClassicBound::Bracket,
                    |i| stops[i].position,
                    |i| stops[i].midpoint,
                    |i| oklab[i],
                );
                (oklab_to_linear(lab).map(linear_to_srgb), opacity)
            }
        };
        let opacity = opacity.clamp(0.0, 1.0);
        std::array::from_fn(|c| rgb[c] + (mapped[c] - rgb[c]) * opacity)
    }
}
