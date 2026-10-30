//! Hue/Saturation at 8 and 16 bits as measured against Photoshop 26.10.0,
//! and its measured 32-bit closed forms. `adjust` keeps only the
//! schema row and the dispatch.
//!
//! # 8 and 16 bits: take4's reference form R0
//!
//! Fitted from take2 and take3 and confirmed by take4
//! (`doctored-fixtures/photoshop/adjust-parity-535-536/take4`, runner
//! `scripts/photoshop-probe/hue_sat_take4.py`, whose docstring states every
//! member below as FORM4 with gain `tid`, hue `s4096t`, domain16 `float`,
//! weights `h359`, combine `mgrid`, order `before`, net `netted`). take4
//! recorded 8-bit maxima of 2 codes on its lightness, gain and hue sets, 3
//! on the master set, 5 on combine and 7 on single bands; the band and
//! combine residuals are band-weight fixed point the form does not model.
//! On the `hue2` fixture (hue 20, saturation 25, lightness −10) it lands
//! within 2 codes at 8 bits and 0.05 of a code at 16.
//!
//! With `M`/`m` the pixel's max/min, `L = (M + m)/2`, `h` its hexcone hue
//! and `s`, `l` the sliders in Photoshop units (`saturation · 100`):
//!
//! ```text
//! 1. Lm: per component, l > 0: c + (1 − c)·l/100;  l < 0: c·(1 + l/100)
//! 2. P = h·1536/360 from Lm's output (undefined for a grey); a band's
//!    handles (degrees) sit at H(d) = floor(d·1536/359 + 1/2) mod 1536 and
//!    its weight w_b(P) ramps linearly between them
//! 3. hue: hexcone rotation by q(hue) + Σ w_b(P)·q(hue_b),
//!    q(d) = trunc(d·24576/360)·360/24576          (a 1/24576-turn grid)
//! 4. band lightness, netted: lb = Σ w_b(P)·l_b/100;
//!    lb > 0: c + (M − c)·lb;  lb < 0: c + (c − m)·lb
//! 5. saturation about L: c' = L + (c − L)·K, K limited so no component
//!    leaves [0, 1]
//! 6. clamp to [0, 1]
//! ```
//!
//! The gain is on Photoshop's 1/256 grid: `Km = 255/(255 − floor(2.54 s))`
//! above 0 and `1 + s/100` at or below it, `G = 256 + trunc(256·Km − 256)`
//! and `K = min(G, 32767)/256`. Band saturation builds a 1536-entry table
//! `T(k) = clamp(G0 + Σ_b rha(w_b(k)·D_b), 0, 32767)` with
//! `D_b = rha(G0·(G_b − 256)/256)`, read at `P = k + f` by linear
//! interpolation (`rha` rounds half away from zero). A grey (`M == m`) has
//! no hue, so steps 3 and 5 leave it alone and no band reaches it: only
//! master lightness moves it. This replaces the HSL kernel and its chroma
//! gate: Photoshop's saturation scales about `L` in RGB, not the HSL `S`.
//!
//! Colorize REPLACES rather than shifts (`H' = colorize_hue`,
//! `S' = colorize_saturation`, `L'` the pixel's own lightness through the
//! two-sided curve) and ignores the master and band controls. This is
//! Photoshop's 16-bit Colorize (`C16` in `hue_sat_parity.py`; take3 max
//! 0.585 tolerance units over 14,172 samples).
//!
//! ## 32 bits
//!
//! [`HueSaturation::apply_float`] is the form on a float document's stored
//! linear values, for the two settings take3 pinned there
//! (`doctored-fixtures/photoshop/adjust-parity-535-536/take3`) and the one
//! take4 added:
//!
//! ```text
//! Colorize (C32):  Y = two_sided_extended(L, a)       # L = (max + min) / 2
//!                  c = S (1 - |2Y - 1|) (1 - |a|)     # no clamp, any sign
//!                  out = hexcone(Y, c, H)
//! Lightness only:  out_k = two_sided_extended(x_k, a) # per component
//! Hue only:        out = hexcone(L, max - min, h + grid_hue(hue))
//! ```
//!
//! Hue alone is take4's `s4096t` (max 0.74 / x32 0.35 tolerance units), so
//! −20 deg turns by −19.99512 deg. Every other setting has no 32-bit form
//! here and takes the display-scaled form over the 8/16-bit kernel
//! (`adjust_float`): take4 left master Saturation's gain grid
//! unmatched at 32 bits (`tid` 1.09 units).

use serde_json::{Map, Value};

use crate::adjust_math::{
    hexcone, hsl_to_rgb, rgb_to_hsl, two_sided, two_sided_extended, HUE_WHEEL,
};
use crate::adjust_parse::{boolean, num_in, obj};

/// Photoshop's hue wheel: 1536 units a turn (256 per 60 deg sector).
const WHEEL_UNITS: usize = 1536;
/// The largest gain Photoshop's table holds, in 1/256 units.
const GAIN_CAP: f32 = 32767.0;

/// One editable hue band as parsed, plus R0's derived values.
#[derive(Clone, Copy)]
struct Band {
    hue: f32,
    saturation: f32,
    lightness: f32,
    center: f32,
    inner: f32,
    falloff: f32,
    /// The four handles on the 1536-unit wheel (step 2).
    quad: [f32; 4],
    /// `q(hue)`, the band's shift on the 1/24576-turn grid.
    grid_hue: f32,
}

/// A parsed `hue_saturation` op.
#[derive(Clone)]
pub(crate) struct HueSaturation {
    hue: f32,
    saturation: f32,
    lightness: f32,
    bands: [Band; 6],
    /// True when at least one band actually edits something.
    banded: bool,
    colorize: bool,
    colorize_hue: f32,
    colorize_saturation: f32,
    colorize_lightness: f32,
    /// The master gain K (step 5) with no band saturation.
    gain: f32,
    /// The band-saturation table `T(k)` with `T(1536) = T(0)` appended so
    /// the interpolation needs no wrap; empty when no band sets saturation.
    table: Vec<f32>,
}

/// A hue in DEGREES: the schema's half-open `[0, 360)`, which no inclusive
/// range can express, so 360 is refused explicitly rather than folded (a
/// host that means 0 must say 0).
fn hue_degrees(params: &Map<String, Value>, key: &str, default: f64) -> Option<f32> {
    let value = num_in(params, key, 0.0..=360.0, default)?;
    (value < 360.0).then_some(value)
}

/// T4-hue's pinned member (`s4096t`): a hue shift in degrees on Photoshop's
/// 1/24576-turn grid (1/4096 of a 60 deg sector), truncated toward zero.
/// The product is formed in f64 so that an f32 setting a multiple of 15 deg,
/// which lands exactly on the grid, cannot be pushed below its integer by
/// f32 noise and lose a whole step.
fn grid_hue(degrees: f32) -> f32 {
    const STEPS: f64 = 24576.0;
    ((f64::from(degrees) * STEPS / 360.0).trunc() * 360.0 / STEPS) as f32
}

/// A slider in Photoshop's integer units (`v · 100`). The dialog's values
/// are integers, and `0.29_f32 · 100` is 28.999999; an f32 that names an
/// integer to within 1e-3 IS that integer, so `floor(2.54 s)` below cannot
/// lose a step to the narrowing.
fn slider_units(v: f32) -> f64 {
    let units = f64::from(v) * 100.0;
    let whole = units.round();
    if (units - whole).abs() < 1e-3 {
        whole
    } else {
        units
    }
}

/// `G = 256·K` on Photoshop's grid for a saturation slider (`tid`, module
/// doc): `Km = 255/(255 − floor(254 s/100))` above 0, `1 + s/100` at or
/// below it, then `256 + trunc(256·Km − 256)`. Not capped.
fn grid_gain(saturation: f32) -> f64 {
    let s = slider_units(saturation);
    let km = if s <= 0.0 {
        1.0 + s / 100.0
    } else {
        255.0 / (255.0 - (254.0 * s / 100.0).floor())
    };
    256.0 + (256.0 * km - 256.0).trunc()
}

/// `H(d)`: a handle in degrees on the 1536-unit wheel, as Photoshop places
/// it (`floor(d·1536/359 + 1/2) mod 1536`, take3's F-wheel).
fn handle(degrees: f32) -> f32 {
    let d = f64::from(degrees).rem_euclid(360.0);
    ((d * WHEEL_UNITS as f64 / 359.0 + 0.5).floor() as usize % WHEEL_UNITS) as f32
}

/// A band's weight at wheel position `t`: rises from handle A to B, 1 from
/// B to C, falls from C to D, 0 elsewhere (arcs taken mod 1536).
fn weight(t: f32, [a, b, c, d]: [f32; 4]) -> f32 {
    let wheel = WHEEL_UNITS as f32;
    let t = (t - a).rem_euclid(wheel);
    let (tb, tc, td) = (
        (b - a).rem_euclid(wheel),
        (c - a).rem_euclid(wheel),
        (d - a).rem_euclid(wheel),
    );
    if t <= tb {
        if tb > 0.0 {
            t / tb
        } else {
            1.0
        }
    } else if t <= tc {
        1.0
    } else if t <= td && td > tc {
        (td - t) / (td - tc)
    } else {
        0.0
    }
}

/// Step 5: `c' = L + (c − L)·k` about `L = (M + m)/2`, with `k` lowered so
/// no component leaves [0, 1]. A grey is returned as is.
fn saturate(v: [f32; 3], k: f32) -> [f32; 3] {
    let big = v[0].max(v[1]).max(v[2]);
    let small = v[0].min(v[1]).min(v[2]);
    if big == small {
        return v;
    }
    let mid = (big + small) / 2.0;
    let mut k = k;
    for c in v {
        if c > mid {
            k = k.min((1.0 - mid) / (c - mid));
        } else if c < mid {
            k = k.min(-mid / (c - mid));
        }
    }
    v.map(|c| mid + (c - mid) * k)
}

impl HueSaturation {
    /// Parses the `hue_saturation` params per the schema row in `adjust`.
    /// Each band's centre defaults to `60 * index` and its handles to the
    /// factory 15 deg inner and 30 deg falloff.
    pub(crate) fn parse(params: &Map<String, Value>) -> Option<HueSaturation> {
        let mut bands = std::array::from_fn(|i| Band {
            hue: 0.0,
            saturation: 0.0,
            lightness: 0.0,
            center: 60.0 * i as f32,
            inner: 15.0,
            falloff: 30.0,
            quad: [0.0; 4],
            grid_hue: 0.0,
        });
        if let Some(listed) = obj(params, "bands")? {
            for (band, name) in bands.iter_mut().zip(HUE_WHEEL) {
                let Some(sub) = obj(listed, name)? else {
                    continue;
                };
                band.hue = num_in(sub, "hue", -180.0..=180.0, 0.0)?;
                band.saturation = num_in(sub, "saturation", -1.0..=1.0, 0.0)?;
                band.lightness = num_in(sub, "lightness", -1.0..=1.0, 0.0)?;
                band.center = hue_degrees(sub, "center", f64::from(band.center))?;
                band.inner = num_in(sub, "inner", 0.0..=180.0, 15.0)?;
                band.falloff = num_in(sub, "falloff", 0.0..=180.0, 30.0)?;
            }
        }
        for band in &mut bands {
            let (c, i, f) = (band.center, band.inner, band.falloff);
            band.quad = [c - i - f, c - i, c + i, c + i + f].map(handle);
            band.grid_hue = grid_hue(band.hue);
        }
        let banded = bands
            .iter()
            .any(|b| b.hue != 0.0 || b.saturation != 0.0 || b.lightness != 0.0);
        let saturation = num_in(params, "saturation", -1.0..=1.0, 0.0)?;
        let g0 = grid_gain(saturation);
        let table = gain_table(g0, &bands);
        Some(HueSaturation {
            hue: num_in(params, "hue", -180.0..=180.0, 0.0)?,
            saturation,
            lightness: num_in(params, "lightness", -1.0..=1.0, 0.0)?,
            bands,
            banded,
            colorize: boolean(params, "colorize", false)?,
            colorize_hue: hue_degrees(params, "colorize_hue", 0.0)?,
            colorize_saturation: num_in(params, "colorize_saturation", 0.0..=1.0, 0.25)?,
            colorize_lightness: num_in(params, "colorize_lightness", -1.0..=1.0, 0.0)?,
            gain: (g0.min(f64::from(GAIN_CAP)) / 256.0) as f32,
            table,
        })
    }

    /// The band-saturation table (`lut_cache` inventory).
    pub(crate) fn table(&self) -> &Vec<f32> {
        &self.table
    }

    /// One straight RGB triple in [0, 1] through R0 (module doc), or
    /// Colorize's `C16`. An identity setting returns its input exactly.
    pub(crate) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.colorize {
            let hsl = rgb_to_hsl(rgb);
            return hsl_to_rgb([
                self.colorize_hue,
                self.colorize_saturation,
                two_sided(hsl[2], self.colorize_lightness),
            ]);
        }
        if self.is_identity() {
            return rgb;
        }
        let mut v = rgb;
        if self.lightness != 0.0 {
            v = v.map(|c| two_sided(c, self.lightness));
        }
        let big = v[0].max(v[1]).max(v[2]);
        let small = v[0].min(v[1]).min(v[2]);
        if big == small {
            // A grey has no hue: no rotation, no band, no saturation.
            return v;
        }
        let [h, _, _] = rgb_to_hsl(v);
        let p = h * WHEEL_UNITS as f32 / 360.0;
        let mut shift = grid_hue(self.hue);
        let mut band_lightness = 0.0;
        if self.banded {
            for band in &self.bands {
                if band.hue == 0.0 && band.lightness == 0.0 {
                    continue;
                }
                let w = weight(p, band.quad);
                shift += w * band.grid_hue;
                band_lightness += w * band.lightness;
            }
        }
        if shift != 0.0 {
            v = hexcone((big + small) / 2.0, big - small, h + shift);
        }
        if band_lightness != 0.0 {
            let big = v[0].max(v[1]).max(v[2]);
            let small = v[0].min(v[1]).min(v[2]);
            v = if band_lightness > 0.0 {
                v.map(|c| c + (big - c) * band_lightness)
            } else {
                v.map(|c| c + (c - small) * band_lightness)
            };
        }
        let k = if self.table.is_empty() {
            self.gain
        } else {
            let k = (p.floor() as usize).min(WHEEL_UNITS - 1);
            let f = p - k as f32;
            (self.table[k] + f * (self.table[k + 1] - self.table[k])) / 256.0
        };
        if k != 1.0 {
            v = saturate(v, k);
        }
        v.map(|c| c.clamp(0.0, 1.0))
    }

    /// The 32-bit form on stored linear values (any sign or magnitude), or
    /// `None` when take3 / take4 pinned none for these settings: a
    /// Saturation edit, Hue with another master slider, or any band edit,
    /// has no measured 32-bit form.
    ///
    /// * Colorize on (the master and band values are ignored, as at 8/16
    ///   bits): `C32`. `Y` is the pixel's `(max + min) / 2` through
    ///   [`two_sided_extended`], so a lightness already past the slider's
    ///   end is held; the chroma is `(1 − |2Y − 1|) · S · (1 − |a|)`, NOT
    ///   clamped at 0, and the triple is [`hexcone`] at `colorize_hue`.
    ///   Both the `(1 − |a|)` factor and the unclamped (so, above `Y = 1`,
    ///   negative) chroma are measured, not derived: they are what
    ///   Photoshop 26.10.0 renders at 32 bits (take3 max 0.11 tolerance
    ///   units).
    /// * Only master Lightness moved: [`two_sided_extended`] per component,
    ///   each component past the target held (take3 max 0.60 units).
    /// * Only master Hue moved: the hexcone rotation of the pixel's own
    ///   lightness `(max + min) / 2` and chroma `max − min` by
    ///   [`grid_hue`]'s shift, unbounded; a grey (`max == min`) has no hue
    ///   and is returned as is (take4 max 0.74 units).
    pub(crate) fn apply_float(&self, rgb: [f32; 3]) -> Option<[f32; 3]> {
        if self.colorize {
            let a = self.colorize_lightness;
            let y = two_sided_extended(rgb_to_hsl(rgb)[2], a);
            let chroma = (1.0 - (2.0 * y - 1.0).abs()) * self.colorize_saturation * (1.0 - a.abs());
            return Some(hexcone(y, chroma, self.colorize_hue));
        }
        if self.hue == 0.0 && self.saturation == 0.0 && !self.banded {
            return Some(rgb.map(|c| two_sided_extended(c, self.lightness)));
        }
        if self.saturation == 0.0 && self.lightness == 0.0 && !self.banded {
            let chroma = rgb[0].max(rgb[1]).max(rgb[2]) - rgb[0].min(rgb[1]).min(rgb[2]);
            if chroma == 0.0 {
                return Some(rgb);
            }
            let [h, _, l] = rgb_to_hsl(rgb);
            return Some(hexcone(l, chroma, h + grid_hue(self.hue)));
        }
        None
    }

    /// Colorize is on (it replaces hue and saturation rather than shifting
    /// them; `cmyk_adjust::hue_saturation_on_ink` treats K differently then).
    pub(crate) fn colorizes(&self) -> bool {
        self.colorize
    }

    pub(crate) fn is_identity(&self) -> bool {
        !self.colorize
            && !self.banded
            && self.hue == 0.0
            && self.saturation == 0.0
            && self.lightness == 0.0
    }
}

/// The `mgrid` band-saturation table (module doc), or an empty one when no
/// band sets saturation. `g0` is the master's uncapped `G`.
fn gain_table(g0: f64, bands: &[Band; 6]) -> Vec<f32> {
    let deltas: Vec<([f32; 4], f64)> = bands
        .iter()
        .filter(|b| b.saturation != 0.0)
        .map(|b| {
            (
                b.quad,
                (g0 * (grid_gain(b.saturation) - 256.0) / 256.0).round(),
            )
        })
        .collect();
    if deltas.is_empty() {
        return Vec::new();
    }
    let mut table: Vec<f32> = (0..WHEEL_UNITS)
        .map(|k| {
            let sum: f64 = deltas
                .iter()
                .map(|&(quad, d)| (f64::from(weight(k as f32, quad)) * d).round())
                .sum();
            (g0 + sum).clamp(0.0, f64::from(GAIN_CAP)) as f32
        })
        .collect();
    table.push(table[0]);
    table
}
